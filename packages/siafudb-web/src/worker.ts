/**
 * Dedicated-worker entry point. Owns the wasm engine, the single-writer Web
 * Lock, the encrypted snapshot store and the debounced autosave. The main
 * thread talks to it through `client.ts`; do not import this file directly.
 */
import init, { canonicalRelayOrigin, Database } from "./wasm/siafudb_wasm.js";
import { chooseBackend, IdbBackend, OpfsBackend } from "./backends.js";
import { DatabaseLockedError, SiafuError, serializeError } from "./errors.js";
import type {
  ChangeLogEntry,
  InitArgs,
  InitResult,
  Request,
  Response,
  SyncStatus,
  WorkerEvent,
} from "./protocol.js";
import { EncryptedSnapshotStore } from "./store.js";
import { parseRelayUrl, RelayError, SyncLoop } from "./sync.js";
import { IdbKeyVault } from "./vault.js";

interface Scope {
  postMessage(m: unknown): void;
  onmessage: ((e: MessageEvent<Request>) => void) | null;
}
const scope = self as unknown as Scope;
const emit = (e: WorkerEvent) => scope.postMessage(e);

/** Take the exclusive per-database Web Lock; resolves to a release function. */
function acquireLock(
  name: string,
  mode: "wait" | "fail",
  timeoutMs: number | null,
): Promise<() => void> {
  if (!navigator.locks) {
    return Promise.reject(
      new SiafuError(
        "storage",
        "Web Locks API unavailable; refusing to open without a single-writer lock",
      ),
    );
  }
  return new Promise((resolve, reject) => {
    const ac =
      mode === "wait" && timeoutMs !== null ? new AbortController() : undefined;
    const timer = ac ? setTimeout(() => ac.abort(), timeoutMs!) : undefined;
    const opts: LockOptions =
      mode === "fail" ? { ifAvailable: true } : ac ? { signal: ac.signal } : {};
    navigator.locks
      .request(`siafudb:${name}`, opts, (lock) => {
        clearTimeout(timer);
        if (!lock) {
          reject(new DatabaseLockedError(name));
          return;
        }
        return new Promise<void>((release) => resolve(() => release()));
      })
      .catch((e: unknown) => {
        clearTimeout(timer);
        reject(
          (e as Error).name === "AbortError"
            ? new DatabaseLockedError(name)
            : e,
        );
      });
  });
}

/** What the transport keeps next to the sync state (`Database.syncMeta`). */
interface SyncMeta {
  relayUrl?: string;
  /** `db.sync.stop()` was called: do not start the loop on open. */
  stopped?: boolean;
}

interface WorkerScope {
  navigator: { onLine?: boolean };
  addEventListener(type: "online", listener: () => void): void;
  removeEventListener(type: "online", listener: () => void): void;
}

class Session {
  /** The running sync transport, if any. */
  sync: SyncLoop | null = null;
  private dirty = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private chain: Promise<void> = Promise.resolve();
  private closed = false;

  constructor(
    readonly db: Database,
    private readonly store: EncryptedSnapshotStore,
    private readonly release: () => void,
    private readonly autosaveMs: number,
  ) {}

  markDirty(): void {
    this.dirty = true;
    if (this.autosaveMs <= 0 || this.timer !== undefined) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      this.save().catch((e: unknown) =>
        emit({ type: "autosave-error", message: (e as Error).message }),
      );
    }, this.autosaveMs);
  }

  /** Queue a save if dirty; saves never overlap. */
  save(): Promise<void> {
    const run = this.chain.then(async () => {
      if (!this.dirty) return;
      this.dirty = false;
      try {
        const snap = this.db.exportSnapshot(); // synchronous: a consistent point-in-time image
        const counter = await this.store.save(snap);
        emit({ type: "saved", counter });
      } catch (e) {
        this.dirty = true;
        throw e;
      }
    });
    this.chain = run.catch(() => undefined);
    return run;
  }

  async flush(): Promise<void> {
    if (this.timer !== undefined) {
      clearTimeout(this.timer);
      this.timer = undefined;
    }
    await this.save();
  }

  // ------------------------------------------------------------------ sync

  private meta(): SyncMeta {
    try {
      const meta = JSON.parse(this.db.syncMeta() || "{}") as unknown;
      return typeof meta === "object" && meta !== null
        ? (meta as SyncMeta)
        : {};
    } catch {
      return {};
    }
  }

  /**
   * After a query ran: hand what it wrote to the sync replica, which queues
   * it for upload. A failure here is not the query's; the changes stay
   * captured and are handed over by the next call.
   */
  afterQuery(wrote: boolean): void {
    let queued = 0;
    try {
      if (this.db.syncEnabled()) queued = this.db.syncCommit(Date.now());
    } catch {
      queued = 0;
    }
    if (wrote || queued > 0) this.markDirty();
    if (queued > 0) this.sync?.notifyLocalWrite();
  }

  /** Start the transport if this database is in a group and was not stopped. */
  startSync(registrationToken?: string): void {
    if (this.closed || this.sync || !this.db.syncEnabled()) return;
    const { relayUrl, stopped } = this.meta();
    if (stopped || typeof relayUrl !== "string") return;
    const w = self as unknown as WorkerScope;
    try {
      parseRelayUrl(relayUrl);
    } catch {
      // A stored address this build no longer accepts: stay stopped rather
      // than fail to open. `status()` shows `running: false`.
      return;
    }
    this.sync = new SyncLoop(
      this.db,
      {
        fetch: (url, init) => fetch(url, init),
        now: () => Date.now(),
        random: () => Math.random(),
        isOnline: () => w.navigator.onLine !== false,
        onOnline: (listener) => {
          w.addEventListener("online", listener);
          return () => w.removeEventListener("online", listener);
        },
        persist: async () => {
          this.dirty = true;
          await this.flush().catch((e: unknown) =>
            emit({ type: "autosave-error", message: (e as Error).message }),
          );
        },
        durable: () => this.flush(),
        touched: () => this.markDirty(),
        applied: (changes) => emit({ type: "sync-applied", changes }),
      },
      registrationToken === undefined
        ? { relayUrl }
        : { relayUrl, registrationToken },
    );
    this.sync.start();
  }

  private async stopSync(): Promise<void> {
    const loop = this.sync;
    this.sync = null;
    await loop?.stop();
  }

  /** Create (`code` undefined) or join a group, persist it, then start syncing. */
  async syncSetup(
    relayUrl: string,
    code?: string,
    registrationToken?: string,
  ): Promise<string> {
    const relay = parseRelayUrl(relayUrl);
    try {
      // The token is bound to the origin: it must be one the engine accepts.
      canonicalRelayOrigin(relay.origin);
    } catch {
      throw new SiafuError(
        "sync",
        "relayUrl is not usable as a relay address: the host must be a plain host name or IP address",
      );
    }
    if (this.db.syncEnabled())
      throw new SiafuError(
        "sync",
        "this database is already in a sync group; call sync.leave() first",
      );
    let made: string;
    try {
      if (code === undefined) made = this.db.syncCreate(Date.now());
      else {
        this.db.syncJoin(code, Date.now());
        made = code;
      }
    } catch {
      // The engine's message is not passed on: it must never quote a code.
      throw new SiafuError(
        "sync",
        code === undefined
          ? "could not create a sync group"
          : "the pairing code is not valid",
      );
    }
    this.db.setSyncMeta(
      JSON.stringify({ relayUrl: relay.url } satisfies SyncMeta),
    );
    this.dirty = true;
    await this.flush();
    this.startSync(registrationToken);
    if (code === undefined && this.sync) {
      // Register the new group on the relay now, so that a relay that
      // refuses it is an error of this call and not a surprise later. A
      // relay that cannot be reached is not an error: the upload waits.
      try {
        await this.sync.pushNow();
      } catch (e) {
        if (e instanceof RelayError && e.status === 403) {
          await this.syncLeave();
          throw new SiafuError(
            "sync",
            registrationToken === undefined
              ? "this relay requires a registration token to create a sync group; pass registrationToken to sync.create()"
              : "the relay did not accept the registration token; no sync group was created",
          );
        }
      }
    }
    return made;
  }

  /** Give a forked device a new identity; what it wrote meanwhile is queued for upload. */
  async syncRejoin(): Promise<void> {
    this.needGroup();
    if (!this.syncStatus().needsRejoin)
      throw new SiafuError(
        "sync",
        "this device does not need to rejoin (status().needsRejoin is false)",
      );
    this.db.syncRejoin(Date.now());
    this.dirty = true;
    await this.flush();
    this.sync?.notifyLocalWrite();
  }

  syncStatus(): SyncStatus {
    const engine = JSON.parse(this.db.syncStatus()) as {
      outbox: number;
      pending: number;
      devices: number;
      rejected: number;
      droppedLocal: number;
      skippedRemote: number;
      lastProblem: string | null;
      forked: boolean;
      needsResync: boolean;
      relayResets: number;
      deviceList: SyncStatus["deviceList"];
    } | null;
    const loop = this.sync?.state();
    return {
      enabled: engine !== null,
      running: loop?.running ?? false,
      connected: loop?.connected ?? false,
      relayUrl: engine === null ? null : (this.meta().relayUrl ?? null),
      pendingUploads: engine?.outbox ?? 0,
      pendingDownloads: engine?.pending ?? 0,
      devices: engine?.devices ?? 0,
      lastSyncAt: loop?.lastSyncAt ?? null,
      lastError: loop?.lastError ?? null,
      notUploaded: engine?.droppedLocal ?? 0,
      notApplied: (engine?.skippedRemote ?? 0) + (engine?.rejected ?? 0),
      lastProblem: engine?.lastProblem ?? null,
      needsRejoin: engine?.forked ?? false,
      needsResync: engine?.needsResync ?? false,
      relayResets: engine?.relayResets ?? 0,
      deviceList: engine?.deviceList ?? [],
    };
  }

  private needGroup(): void {
    if (!this.db.syncEnabled())
      throw new SiafuError("sync", "sync is not set up for this database");
  }

  syncNow(): Promise<void> {
    this.needGroup();
    if (!this.sync) throw new SiafuError("sync", "sync is stopped");
    return this.sync.syncNow();
  }

  /** Stop or resume the transport and remember the choice. */
  async syncPause(stopped: boolean, registrationToken?: string): Promise<void> {
    this.needGroup();
    const meta = this.meta();
    if (stopped) await this.stopSync();
    if (Boolean(meta.stopped) !== stopped) {
      const next: SyncMeta = { ...meta };
      if (stopped) next.stopped = true;
      else delete next.stopped;
      this.db.setSyncMeta(JSON.stringify(next));
      this.dirty = true;
      await this.flush();
    }
    if (!stopped) {
      // A new secret needs a new transport: the running one has none or another.
      if (registrationToken !== undefined) await this.stopSync();
      this.startSync(registrationToken);
    }
  }

  async syncLeave(): Promise<void> {
    await this.stopSync();
    if (!this.db.syncEnabled()) return;
    this.db.syncLeave();
    this.db.setSyncMeta("");
    this.dirty = true;
    await this.flush();
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    try {
      await this.stopSync();
      await this.flush();
    } finally {
      await this.store.close();
      this.db.free();
      this.release();
    }
  }
}

let session: Session | null = null;
let wasmReady: Promise<unknown> | null = null;

async function doInit(a: InitArgs): Promise<InitResult> {
  if (session) throw new SiafuError("internal", "worker already initialised");
  wasmReady ??= init(a.wasmUrl ? { module_or_path: a.wasmUrl } : undefined);
  await wasmReady;
  const release = await acquireLock(a.name, a.lock, a.lockTimeoutMs);
  try {
    const backend = chooseBackend(a.name, a.backend);
    const store = new EncryptedSnapshotStore(
      a.name,
      backend,
      new IdbKeyVault(),
    );
    const snap = await store.load();
    let db: Database;
    try {
      db = snap ? Database.fromSnapshot(snap) : new Database();
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      // The one engine error an app has to tell apart: see errors.ts.
      throw message.startsWith("stored sync state version")
        ? new SiafuError("sync-state-version", message)
        : e;
    } finally {
      snap?.fill(0);
    }
    session = new Session(db, store, release, a.autosaveMs);
    // Writes captured before the last save but not yet queued for upload.
    session.afterQuery(false);
    session.startSync();
    let persisted: boolean | null = null;
    try {
      persisted = (await navigator.storage?.persist?.()) ?? null;
    } catch {
      persisted = null;
    }
    // Make a brand-new database durable immediately so its key and first snapshot exist.
    if (!snap) {
      session.markDirty();
      await session.flush();
    }
    return {
      backend: backend.kind === "opfs" ? "opfs" : "idb",
      restored: snap !== null,
      persisted,
      writeCounter: store.writeCounter,
    };
  } catch (e) {
    release();
    throw e;
  }
}

async function doDestroy(name: string): Promise<void> {
  const release = await acquireLock(name, "fail", null);
  try {
    const vault = new IdbKeyVault();
    await new OpfsBackend(name).deleteAll().catch(() => undefined);
    await new IdbBackend(name).deleteAll();
    await vault.delete(name);
  } finally {
    release();
  }
}

function need(): Session {
  if (!session) throw new SiafuError("closed", "database is closed");
  return session;
}

async function handle(req: Request): Promise<unknown> {
  switch (req.op) {
    case "init":
      return doInit(req.args);
    case "execute": {
      const s = need();
      const r = JSON.parse(
        s.db.execute(req.args.query, req.args.params ?? undefined),
      ) as unknown;
      s.afterQuery(true);
      return r;
    }
    case "query": {
      const s = need();
      const r = JSON.parse(
        s.db.query(req.args.query, req.args.params ?? undefined),
      ) as unknown;
      // A "read" query can write (MERGE, CREATE ... RETURN).
      s.afterQuery(false);
      return r;
    }
    case "changesSince":
      return JSON.parse(
        need().db.changesSince(req.args.seq),
      ) as ChangeLogEntry[];
    case "flush":
      return need().flush();
    case "close": {
      const s = need();
      session = null;
      return s.close();
    }
    case "destroy":
      return doDestroy(req.args.name);
    case "syncCreate":
      return need().syncSetup(
        req.args.relayUrl,
        undefined,
        req.args.registrationToken,
      );
    case "syncJoin":
      return need()
        .syncSetup(req.args.relayUrl, req.args.code)
        .then(() => undefined);
    case "syncStatus":
      return need().syncStatus();
    case "syncNow":
      return need().syncNow();
    case "syncStop":
      return need().syncPause(true);
    case "syncStart":
      return need().syncPause(false, req.args.registrationToken);
    case "syncRejoin":
      return need().syncRejoin();
    case "syncLeave":
      return need().syncLeave();
    case "syncPairingCode":
      return need().db.syncPairingCode() ?? null;
  }
}

scope.onmessage = (e) => {
  const req = e.data;
  handle(req).then(
    (result) =>
      scope.postMessage({ id: req.id, ok: true, result } satisfies Response),
    (err: unknown) =>
      scope.postMessage({
        id: req.id,
        ok: false,
        error: serializeError(wrapEngine(req.op, err)),
      } satisfies Response),
  );
};

/** Engine (wasm) errors surface as plain Errors; tag them as query errors. */
function wrapEngine(op: Request["op"], err: unknown): unknown {
  if (err instanceof SiafuError) return err;
  if (op === "execute" || op === "query" || op === "changesSince") {
    return new SiafuError(
      "query",
      err instanceof Error ? err.message : String(err),
    );
  }
  // Fixed text: an engine message from a sync call is not passed on.
  if (op.startsWith("sync")) return new SiafuError("sync", "sync failed");
  return err;
}

emit({ type: "ready" });
