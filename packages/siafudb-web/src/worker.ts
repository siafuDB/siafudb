/**
 * Dedicated-worker entry point. Owns the wasm engine, the single-writer Web
 * Lock, the encrypted snapshot store and the debounced autosave. The main
 * thread talks to it through `client.ts`; do not import this file directly.
 */
import init, { Database } from "./wasm/siafudb_wasm.js";
import { chooseBackend, IdbBackend, OpfsBackend } from "./backends.js";
import { DatabaseLockedError, SiafuError, serializeError } from "./errors.js";
import type {
  ChangeLogEntry,
  InitArgs,
  InitResult,
  Request,
  Response,
  WorkerEvent,
} from "./protocol.js";
import { EncryptedSnapshotStore } from "./store.js";
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

class Session {
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

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    try {
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
    const db = snap ? Database.fromSnapshot(snap) : new Database();
    snap?.fill(0);
    session = new Session(db, store, release, a.autosaveMs);
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
      s.markDirty();
      return r;
    }
    case "query":
      return JSON.parse(
        need().db.query(req.args.query, req.args.params ?? undefined),
      ) as unknown;
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
  return err;
}

emit({ type: "ready" });
