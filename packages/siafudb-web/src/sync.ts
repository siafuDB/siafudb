/**
 * The sync transport: moves frames between the wasm engine and a GSP relay
 * (`POST /v0/push`, `POST /v0/pull?wait_ms=`). It runs in the worker and
 * knows nothing about the frames' content; the engine builds and reads them.
 *
 * Two loops, each strictly sequential, so neither ever overlaps itself:
 * - push: woken (debounced) after local writes, sends the outbox until empty;
 * - pull: long-polls the relay and hands every reply to the engine.
 *
 * Both back off with jitter after a failure and are woken early when the
 * browser reports it is back online. Nothing here logs, and no error message
 * holds the relay token or a pairing code.
 */
import { SiafuError } from "./errors.js";

/** The part of the wasm `Database` the transport uses. */
export interface SyncEngine {
  syncPushFrame(): Uint8Array | undefined;
  syncAcceptAck(reply: Uint8Array): number;
  syncPullFrame(): Uint8Array;
  /** JSON `{"items", "ops", "changes", "more", "relayReset", "needsResync", "forked"}`. */
  syncIngestBatch(reply: Uint8Array, nowMs: number): string;
  syncPoll(nowMs: number): number;
  /** The bearer token for the relay at this origin. Throws if there is none. */
  syncRelayToken(relayOrigin: string): string;
  syncStartResync(): void;
}

/** What the transport needs from its surroundings (injected for tests). */
export interface SyncHost {
  fetch(url: string, init: RequestInit): Promise<Response>;
  now(): number;
  /** Uniform in [0, 1), for jitter. */
  random(): number;
  /** `navigator.onLine`. */
  isOnline(): boolean;
  /** Subscribe to the `online` event; returns the unsubscribe function. */
  onOnline(listener: () => void): () => void;
  /** Persist the database now (engine and sync state are one blob). Must not reject. */
  persist(): Promise<void>;
  /**
   * Resolve once everything written so far is on disk; reject if it cannot
   * be saved. Called before every push: an op must not reach the relay
   * before the device itself has it durably, or a crash would make the
   * device reuse that op's sequence number for a different op.
   */
  durable(): Promise<void>;
  /** The outbox shrank or the cursor moved: a save is wanted soon, not necessarily now. */
  touched(): void;
  /** Remote changes were applied to the graph. */
  applied(changes: number): void;
}

export interface SyncLoopOptions {
  relayUrl: string;
  /**
   * The relay's registration secret, for a relay that requires one to accept
   * a new group. Sent as `x-gsp-registration` with pushes until one succeeds
   * (and again after the relay is seen to have lost its data). Kept in
   * memory only.
   */
  registrationToken?: string;
  /** First delay before a resync the engine asked for; doubles up to `resyncMaxMs`. */
  resyncMinMs?: number;
  resyncMaxMs?: number;
  /** `wait_ms` of an idle pull. The relay caps it at 25000. */
  longPollMs?: number;
  /** Delay between a local write and its push, to batch bursts. */
  pushDebounceMs?: number;
  /** First retry delay after a failure; doubles up to `maxBackoffMs`. */
  minBackoffMs?: number;
  maxBackoffMs?: number;
}

/** Transport view for `status()`. */
export interface SyncLoopState {
  running: boolean;
  /** The latest relay request succeeded. */
  connected: boolean;
  /** `Date.now()` of the latest successful relay request. */
  lastSyncAt: number | null;
  lastError: string | null;
}

/** Slack on top of `wait_ms` before a pull is given up as hung. */
const REQUEST_SLACK_MS = 15_000;

/** A relay address: where requests go, and the origin the token is bound to. */
export interface RelayAddress {
  /** Base URL without a trailing slash; `/v0/push` and `/v0/pull` are appended. */
  url: string;
  /** `scheme://host[:port]`. */
  origin: string;
}

const LOOPBACK = new Set(["localhost", "127.0.0.1", "[::1]"]);

/**
 * Check a relay URL. `https:` only, except that `http:` is accepted for
 * `localhost`, `127.0.0.1` and `[::1]` (development); no credentials, query
 * or fragment. The relay token and everything pushed travel over this
 * connection, and the protocol adds no transport security of its own.
 */
export function parseRelayUrl(input: string): RelayAddress {
  let url: URL;
  try {
    url = new URL(input);
  } catch {
    throw new SiafuError("sync", "relayUrl is not a valid URL");
  }
  if (url.protocol === "http:") {
    if (!LOOPBACK.has(url.hostname))
      throw new SiafuError(
        "sync",
        "relayUrl must use https:// (plain http:// is accepted for localhost, 127.0.0.1 and [::1] only)",
      );
  } else if (url.protocol !== "https:")
    throw new SiafuError("sync", "relayUrl must be an https:// URL");
  if (url.username || url.password || url.search || url.hash)
    throw new SiafuError(
      "sync",
      "relayUrl must not carry credentials, a query or a fragment",
    );
  return {
    url: url.origin + url.pathname.replace(/\/+$/, ""),
    origin: url.origin,
  };
}

/** A failed relay request. `permanent`: retrying soon will not help. */
export class RelayError extends SiafuError {
  constructor(
    message: string,
    readonly permanent: boolean,
    /** The HTTP status, when the relay answered. */
    readonly status?: number,
    /** The delay the relay asked for (`retry-after`), in ms. */
    readonly retryAfterMs?: number,
  ) {
    super("sync", message);
  }
}

/** What the relay's refusal means, in words. Never holds a secret. */
function refusal(status: number): string {
  switch (status) {
    case 401:
      return "the relay does not accept this sync group's token (relay answered 401)";
    case 403:
      return "the relay requires a registration token to accept a new sync group, and none or a wrong one was sent (relay answered 403)";
    case 413:
      return "the relay refused the upload as too large or over its quota (relay answered 413)";
    case 429:
    case 503:
      return `the relay is busy (relay answered ${status})`;
    default:
      return `relay answered ${status}`;
  }
}

/** A latch: `wait()` resolves once `set()` has been called since the last `wait()` returned. */
class Signal {
  private flag = false;
  private waiter: (() => void) | undefined;
  set(): void {
    this.flag = true;
    this.waiter?.();
  }
  async wait(): Promise<void> {
    if (!this.flag)
      await new Promise<void>((resolve) => {
        this.waiter = resolve;
      });
    this.waiter = undefined;
    this.flag = false;
  }
}

type Waiter = { resolve: () => void; reject: (e: Error) => void };

const stopped = () => new SiafuError("sync", "sync was stopped");

export class SyncLoop {
  private readonly url: string;
  private readonly origin: string;
  private readonly registrationToken: string | undefined;
  /** Whether the next push carries the registration secret. */
  private register: boolean;
  private readonly resyncMinMs: number;
  private readonly resyncMaxMs: number;
  /** Resyncs started since the engine last got through a replay without asking for another. */
  private resyncAttempts = 0;
  /** When the next resync may start (`now()` scale); `null`: none wanted. */
  private resyncAt: number | null = null;
  private readonly longPollMs: number;
  private readonly pushDebounceMs: number;
  private readonly minBackoffMs: number;
  private readonly maxBackoffMs: number;

  private running = false;
  private connected = false;
  private lastSyncAt: number | null = null;
  private lastError: string | null = null;

  private loops: Promise<void>[] = [];
  private offOnline: (() => void) | undefined;
  /** Aborts every request in flight on stop. */
  private abort = new AbortController();
  /** Aborts only the pull in flight (to turn a long poll into `syncNow`). */
  private pullAbort: AbortController | undefined;
  private readonly pushSignal = new Signal();
  private pushTimer: ReturnType<typeof setTimeout> | undefined;
  private pushWaiters: Waiter[] = [];
  private pullWaiters: Waiter[] = [];
  /** Sleepers of `pause()`, woken by stop, `online` and `syncNow`. */
  private readonly sleepers = new Set<() => void>();

  constructor(
    private readonly db: SyncEngine,
    private readonly host: SyncHost,
    options: SyncLoopOptions,
  ) {
    const relay = parseRelayUrl(options.relayUrl);
    this.url = relay.url;
    this.origin = relay.origin;
    this.registrationToken = options.registrationToken || undefined;
    this.register = this.registrationToken !== undefined;
    this.resyncMinMs = Math.max(1, options.resyncMinMs ?? 5_000);
    this.resyncMaxMs = Math.max(
      this.resyncMinMs,
      options.resyncMaxMs ?? 600_000,
    );
    this.longPollMs = Math.min(
      25_000,
      Math.max(0, options.longPollMs ?? 20_000),
    );
    this.pushDebounceMs = Math.max(0, options.pushDebounceMs ?? 100);
    this.minBackoffMs = Math.max(1, options.minBackoffMs ?? 500);
    this.maxBackoffMs = Math.max(
      this.minBackoffMs,
      options.maxBackoffMs ?? 30_000,
    );
  }

  get relayUrl(): string {
    return this.url;
  }

  state(): SyncLoopState {
    return {
      running: this.running,
      connected: this.running && this.connected,
      lastSyncAt: this.lastSyncAt,
      lastError: this.lastError,
    };
  }

  /** Start both loops. The outbox is pushed and the relay read at once. */
  start(): void {
    if (this.running) return;
    this.running = true;
    this.abort = new AbortController();
    this.offOnline = this.host.onOnline(() => this.wake());
    this.pushSignal.set();
    this.loops = [this.pushLoop(), this.pullLoop()];
  }

  /** Stop both loops, abort requests in flight, and wait until they have ended. */
  async stop(): Promise<void> {
    if (!this.running) return;
    this.running = false;
    this.connected = false;
    clearTimeout(this.pushTimer);
    this.pushTimer = undefined;
    this.offOnline?.();
    this.offOnline = undefined;
    this.abort.abort();
    this.pushSignal.set();
    this.wake();
    await Promise.all(this.loops);
    this.loops = [];
    for (const w of [...this.pushWaiters, ...this.pullWaiters])
      w.reject(stopped());
    this.pushWaiters = [];
    this.pullWaiters = [];
  }

  /** A local write was committed: push it after the debounce delay. */
  notifyLocalWrite(): void {
    if (!this.running || this.pushTimer !== undefined) return;
    this.pushTimer = setTimeout(() => {
      this.pushTimer = undefined;
      this.pushSignal.set();
    }, this.pushDebounceMs);
  }

  /**
   * Push everything queued and pull everything the relay has, now. Resolves
   * when both are done; rejects if the relay could not be reached (the loops
   * keep retrying on their own).
   */
  /**
   * Push everything queued, now. Resolves when the outbox is empty; rejects
   * with a {@link RelayError} if the relay could not be used.
   */
  pushNow(): Promise<void> {
    if (!this.running) return Promise.reject(stopped());
    const done = new Promise<void>((resolve, reject) =>
      this.pushWaiters.push({ resolve, reject }),
    );
    clearTimeout(this.pushTimer);
    this.pushTimer = undefined;
    this.pushSignal.set();
    this.wake();
    return done;
  }

  syncNow(): Promise<void> {
    if (!this.running) return Promise.reject(stopped());
    const wait = (list: Waiter[]) =>
      new Promise<void>((resolve, reject) => list.push({ resolve, reject }));
    const done = Promise.all([
      wait(this.pushWaiters),
      wait(this.pullWaiters),
    ]).then(() => undefined);
    clearTimeout(this.pushTimer);
    this.pushTimer = undefined;
    this.pushSignal.set();
    // End an idle long poll (and any backoff) so the pull loop turns around.
    this.pullAbort?.abort();
    this.wake();
    return done;
  }

  // ------------------------------------------------------------- internals

  private wake(): void {
    for (const s of [...this.sleepers]) s();
  }

  /** Sleep up to `ms`; ends early on stop, `online` and `syncNow`. */
  private pause(ms: number): Promise<void> {
    return new Promise<void>((resolve) => {
      const done = () => {
        clearTimeout(timer);
        this.sleepers.delete(done);
        resolve();
      };
      const timer = setTimeout(done, ms);
      this.sleepers.add(done);
    });
  }

  /** Delay before retry number `failures` (1-based), with jitter in [50%, 100%]. */
  private backoff(failures: number, error: unknown): number {
    // The relay named a delay: wait that long, plus up to half as much again.
    if (error instanceof RelayError && error.retryAfterMs !== undefined)
      return Math.ceil(error.retryAfterMs * (1 + this.host.random() / 2));
    const permanent = error instanceof RelayError && error.permanent;
    const ceiling = permanent
      ? this.maxBackoffMs
      : Math.min(
          this.maxBackoffMs,
          this.minBackoffMs * 2 ** Math.min(failures - 1, 20),
        );
    return Math.ceil(ceiling * (0.5 + this.host.random() / 2));
  }

  private failed(e: unknown): void {
    this.connected = false;
    // Only our own fixed texts are kept; anything else could quote a request.
    this.lastError =
      e instanceof RelayError ? e.message : "sync failed unexpectedly";
  }

  private succeeded(): void {
    this.connected = true;
    this.lastError = null;
    this.lastSyncAt = this.host.now();
  }

  private async post(
    path: string,
    body: Uint8Array,
    timeoutMs: number,
    extra?: AbortSignal,
  ): Promise<Uint8Array> {
    if (!this.host.isOnline()) throw new RelayError("offline", false);
    let token: string;
    try {
      token = this.db.syncRelayToken(this.origin);
    } catch {
      throw new RelayError(
        "sync is not set up for this database, or the relay address is not usable",
        true,
      );
    }
    const headers: Record<string, string> = {
      authorization: `Bearer ${token}`,
      "content-type": "application/cbor",
    };
    const registering = path === "/v0/push" && this.register;
    if (registering) headers["x-gsp-registration"] = this.registrationToken!;
    const ctl = new AbortController();
    const cancel = () => ctl.abort();
    const timer = setTimeout(cancel, timeoutMs);
    this.abort.signal.addEventListener("abort", cancel);
    extra?.addEventListener("abort", cancel);
    if (this.abort.signal.aborted || extra?.aborted) ctl.abort();
    try {
      let res: Response;
      try {
        res = await this.host.fetch(this.url + path, {
          method: "POST",
          headers,
          body: body as BufferSource,
          signal: ctl.signal,
          cache: "no-store",
          credentials: "omit",
          referrerPolicy: "no-referrer",
        });
      } catch {
        throw new RelayError("relay unreachable", false);
      }
      if (!res.ok) {
        const retry =
          res.status >= 500 || res.status === 429 || res.status === 408;
        let retryAfterMs: number | undefined;
        if (res.status === 429 || res.status === 503) {
          // `retry-after` in seconds. A cross-origin page can only read it
          // if the relay exposes the header; both statuses mean 1 s on the
          // reference relay.
          const seconds = Number(res.headers.get("retry-after") ?? "");
          retryAfterMs =
            Number.isFinite(seconds) && seconds > 0
              ? Math.min(seconds * 1000, this.maxBackoffMs)
              : 1_000;
        }
        throw new RelayError(
          refusal(res.status),
          !retry,
          res.status,
          retryAfterMs,
        );
      }
      // The group is registered on this relay now.
      if (registering) this.register = false;
      try {
        return new Uint8Array(await res.arrayBuffer());
      } catch {
        throw new RelayError("relay unreachable", false);
      }
    } finally {
      clearTimeout(timer);
      this.abort.signal.removeEventListener("abort", cancel);
      extra?.removeEventListener("abort", cancel);
    }
  }

  /** Wrap an engine call: its errors are about the relay's reply, never retried quickly. */
  private engine<T>(call: () => T): T {
    try {
      return call();
    } catch (e) {
      throw new RelayError(
        e instanceof Error ? e.message : "the engine refused a relay reply",
        true,
      );
    }
  }

  private async pushLoop(): Promise<void> {
    let failures = 0;
    while (this.running) {
      await this.pushSignal.wait();
      if (!this.running) break;
      const waiters = this.pushWaiters;
      this.pushWaiters = [];
      try {
        for (;;) {
          try {
            await this.host.durable();
          } catch {
            throw new RelayError(
              "could not save before uploading; nothing was sent",
              false,
            );
          }
          if (!this.running) break;
          const frame = this.engine(() => this.db.syncPushFrame());
          if (frame === undefined) break;
          const reply = await this.post("/v0/push", frame, REQUEST_SLACK_MS);
          this.engine(() => this.db.syncAcceptAck(reply));
          this.succeeded();
          this.host.touched();
          if (!this.running) break;
        }
        failures = 0;
        for (const w of waiters) w.resolve();
      } catch (e) {
        if (!this.running) {
          for (const w of waiters) w.reject(stopped());
          break;
        }
        this.failed(e);
        for (const w of waiters) w.reject(e as Error);
        failures += 1;
        await this.pause(this.backoff(failures, e));
        // What is still in the outbox is tried again.
        this.pushSignal.set();
      }
    }
  }

  private async pullLoop(): Promise<void> {
    let failures = 0;
    // Catch up first: the first pull does not wait.
    let more = true;
    while (this.running) {
      const waiters = this.pullWaiters;
      this.pullWaiters = [];
      // A resync the engine asked for, once its delay has passed: read the
      // relay's log from the start again.
      if (this.resyncAt !== null && this.host.now() >= this.resyncAt) {
        this.resyncAt = null;
        this.resyncAttempts += 1;
        try {
          this.db.syncStartResync();
          more = true;
        } catch {
          // Not in a group any more; the pull below reports it.
        }
      }
      let waitMs = more || waiters.length > 0 ? 0 : this.longPollMs;
      if (this.resyncAt !== null)
        waitMs = Math.min(waitMs, Math.max(0, this.resyncAt - this.host.now()));
      const mine = new AbortController();
      this.pullAbort = mine;
      try {
        const frame = this.engine(() => this.db.syncPullFrame());
        const reply = await this.post(
          `/v0/pull?wait_ms=${waitMs}`,
          frame,
          waitMs + REQUEST_SLACK_MS,
          mine.signal,
        );
        const now = this.host.now();
        const out = this.engine(
          () =>
            JSON.parse(this.db.syncIngestBatch(reply, now)) as {
              items: number;
              ops: number;
              changes: number;
              more: boolean;
              relayReset: boolean;
              needsResync: boolean;
            },
        );
        // Ops held for a clock ahead of ours may be due by now.
        const late = this.engine(() => this.db.syncPoll(now));
        this.succeeded();
        failures = 0;
        // "More" with nothing in the frame would spin; a relay does not say that.
        more = out.more && out.items > 0;
        // Cursor and data are one blob: save before asking for more, so a
        // reload continues from here. A frame that only echoed this
        // device's own items moved nothing but the cursor; the next
        // ordinary save covers that.
        if (out.ops > 0 || late > 0 || out.relayReset)
          await this.host.persist();
        else if (out.items > 0) this.host.touched();
        if (out.relayReset) {
          // The relay lost its data: what it never handed back is in the
          // outbox again, and the group has to be registered again.
          this.register = this.registrationToken !== undefined;
          this.pushSignal.set();
        }
        if (out.needsResync) {
          // Received ops were dropped for lack of room. Replaying helps only
          // if what blocked them is gone, so wait longer each time.
          this.resyncAt ??=
            now +
            Math.min(
              this.resyncMaxMs,
              this.resyncMinMs * 2 ** Math.min(this.resyncAttempts, 20),
            );
        } else if (!more && this.resyncAt === null) {
          // A whole replay went through without the engine asking again.
          this.resyncAttempts = 0;
        }
        if (out.changes + late > 0) this.host.applied(out.changes + late);
        if (more) this.pullWaiters.unshift(...waiters);
        else for (const w of waiters) w.resolve();
      } catch (e) {
        if (!this.running) {
          for (const w of waiters) w.reject(stopped());
          break;
        }
        if (mine.signal.aborted) {
          // `syncNow` ended a long poll: not a failure, go round at once.
          this.pullWaiters.unshift(...waiters);
          continue;
        }
        this.failed(e);
        for (const w of waiters) w.reject(e as Error);
        failures += 1;
        more = true;
        await this.pause(this.backoff(failures, e));
      } finally {
        if (this.pullAbort === mine) this.pullAbort = undefined;
      }
    }
  }
}
