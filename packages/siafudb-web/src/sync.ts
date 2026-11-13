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
  /** JSON `{"items": n, "ops": n, "changes": n, "more": bool, "stalled": bool}`. */
  syncIngestBatch(reply: Uint8Array, nowMs: number): string;
  syncPoll(nowMs: number): number;
  syncRelayToken(): string | undefined;
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
  /** The outbox shrank: a save is wanted soon, not necessarily now. */
  touched(): void;
  /** Remote changes were applied to the graph. */
  applied(changes: number): void;
}

export interface SyncLoopOptions {
  relayUrl: string;
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

/**
 * Check a relay URL and return its canonical form (no trailing slash).
 * Only `http:` and `https:`; no credentials, query or fragment.
 */
export function normalizeRelayUrl(input: string): string {
  let url: URL;
  try {
    url = new URL(input);
  } catch {
    throw new SiafuError("sync", "relayUrl is not a valid URL");
  }
  if (url.protocol !== "https:" && url.protocol !== "http:")
    throw new SiafuError("sync", "relayUrl must be an http(s) URL");
  if (url.username || url.password || url.search || url.hash)
    throw new SiafuError(
      "sync",
      "relayUrl must not carry credentials, a query or a fragment",
    );
  return url.origin + url.pathname.replace(/\/+$/, "");
}

/** A failed relay request. `permanent`: retrying soon will not help. */
class RelayError extends SiafuError {
  constructor(
    message: string,
    readonly permanent: boolean,
  ) {
    super("sync", message);
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
    this.url = normalizeRelayUrl(options.relayUrl);
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
  private backoff(failures: number, permanent: boolean): number {
    const ceiling = permanent
      ? this.maxBackoffMs
      : Math.min(
          this.maxBackoffMs,
          this.minBackoffMs * 2 ** Math.min(failures - 1, 20),
        );
    return Math.ceil(ceiling * (0.5 + this.host.random() / 2));
  }

  private failed(e: unknown): boolean {
    this.connected = false;
    // Only our own fixed texts are kept; anything else could quote a request.
    this.lastError =
      e instanceof RelayError ? e.message : "sync failed unexpectedly";
    return e instanceof RelayError && e.permanent;
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
    const token = this.db.syncRelayToken();
    if (token === undefined)
      throw new RelayError("sync is not set up for this database", true);
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
          headers: {
            authorization: `Bearer ${token}`,
            "content-type": "application/cbor",
          },
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
        throw new RelayError(`relay answered ${res.status}`, !retry);
      }
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
        const permanent = this.failed(e);
        for (const w of waiters) w.reject(e as Error);
        failures += 1;
        await this.pause(this.backoff(failures, permanent));
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
      const waitMs = more || waiters.length > 0 ? 0 : this.longPollMs;
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
              changes: number;
              more: boolean;
              stalled: boolean;
            },
        );
        // Ops held for a clock ahead of ours may be due by now.
        const late = this.engine(() => this.db.syncPoll(now));
        this.succeeded();
        failures = 0;
        // "More" with nothing in the frame would spin; a relay does not say that.
        more = out.more && out.items > 0;
        // Cursor and data are one blob: save before asking for more, so a
        // reload continues from here.
        if (out.items > 0 || late > 0) await this.host.persist();
        if (out.changes + late > 0) this.host.applied(out.changes + late);
        // The engine could not take an item yet and the relay would hand it
        // straight back: retry after a long delay instead.
        if (out.stalled)
          throw new RelayError("received changes cannot be applied yet", true);
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
        const permanent = this.failed(e);
        for (const w of waiters) w.reject(e as Error);
        failures += 1;
        more = true;
        await this.pause(this.backoff(failures, permanent));
      } finally {
        if (this.pullAbort === mine) this.pullAbort = undefined;
      }
    }
  }
}
