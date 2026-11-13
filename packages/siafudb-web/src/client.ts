import { ClosedError, SiafuError, reviveError } from './errors.js';
import type {
  Backend,
  ChangeLogEntry,
  ExecuteResult,
  InitArgs,
  InitResult,
  Params,
  QueryResult,
  Request,
  Response,
  WorkerEvent,
} from './protocol.js';

/** Options for {@link SiafuDB.open}. */
export interface OpenOptions {
  /**
   * Storage backend. `'auto'` (default) uses OPFS when the worker supports
   * `FileSystemSyncAccessHandle` and falls back to IndexedDB otherwise.
   * `'opfs'` fails instead of falling back; `'idb'` forces IndexedDB.
   */
  backend?: 'auto' | Backend;
  /**
   * Debounce, in milliseconds, between the last write and an automatic
   * encrypted save. `0` disables autosave; call {@link SiafuDB.flush}.
   * Default 500.
   */
  autosaveMs?: number;
  /**
   * What to do if another tab already has this database open.
   * `'wait'` (default) queues behind the other tab (optionally bounded by
   * `lockTimeoutMs`); `'fail'` rejects immediately with `DatabaseLockedError`.
   */
  lock?: 'wait' | 'fail';
  /** With `lock: 'wait'`, give up after this many ms with `DatabaseLockedError`. */
  lockTimeoutMs?: number;
  /** Override the worker script URL (bundler setups that cannot resolve the default). */
  workerUrl?: string | URL;
  /** Override the `.wasm` URL passed to the engine loader. */
  wasmUrl?: string | URL;
  /** Called when a background (debounced) save fails. The data stays dirty and is retried on the next write or `flush()`. */
  onAutosaveError?: (error: Error) => void;
  /** Called after each successful save with the new write counter. */
  onSaved?: (counter: number) => void;
}

/** Result of `navigator.storage.persist()` plus a quota estimate. */
export interface StorageReport {
  /** Whether the browser granted persistent storage (`null`: API unsupported). */
  persisted: boolean | null;
  estimate: StorageEstimate | null;
}

type RequestBody = Request extends infer R ? (R extends { id: number } ? Omit<R, 'id'> : never) : never;
type Pending ={ resolve: (v: unknown) => void; reject: (e: Error) => void };

function spawnWorker(url: string | URL | undefined): Worker {
  // Keep the `new Worker(new URL(..., import.meta.url), ...)` form literal: Vite/webpack/Parcel
  // only bundle the worker (and its wasm) when they can see this exact pattern.
  if (url !== undefined) return new Worker(url, { type: 'module', name: 'siafudb' });
  return new Worker(new URL('./worker.js', import.meta.url), { type: 'module', name: 'siafudb' });
}

/** Minimal RPC channel to the worker. */
class Channel {
  private seq = 0;
  private readonly pending = new Map<number, Pending>();
  private readonly ready: Promise<void>;
  closed = false;

  constructor(
    private readonly worker: Worker,
    private readonly onEvent: (e: WorkerEvent) => void,
  ) {
    let markReady!: () => void;
    let failReady!: (e: Error) => void;
    this.ready = new Promise<void>((res, rej) => {
      markReady = res;
      failReady = rej;
    });
    worker.onmessage = (m: MessageEvent<Response | WorkerEvent>) => {
      const d = m.data;
      if ('type' in d) {
        if (d.type === 'ready') markReady();
        else this.onEvent(d);
        return;
      }
      const p = this.pending.get(d.id);
      if (!p) return;
      this.pending.delete(d.id);
      if (d.ok) p.resolve(d.result);
      else p.reject(reviveError(d.error));
    };
    worker.onerror = (ev) => {
      const err = new SiafuError('internal', `worker error: ${ev.message || 'failed to load worker'}`);
      failReady(err);
      this.failAll(err);
    };
  }

  private failAll(err: Error) {
    for (const p of this.pending.values()) p.reject(err);
    this.pending.clear();
  }

  async call<T>(req: RequestBody): Promise<T> {
    if (this.closed) throw new ClosedError();
    await this.ready;
    const id = ++this.seq;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ ...req, id });
    });
  }

  terminate() {
    this.closed = true;
    this.worker.terminate();
    this.failAll(new ClosedError());
  }
}

const toWasmUrl = (u: string | URL | undefined) => (u === undefined ? undefined : String(u));

function encodeParams(params: Params | undefined): string | null {
  return params === undefined ? null : JSON.stringify(params);
}

/**
 * A SiafuDB graph database running in a dedicated Worker, persisted as an
 * encrypted snapshot in OPFS (preferred) or IndexedDB.
 *
 * Queries use Cypher/GQL with **bound parameters** (`$name`); never build
 * query text by concatenating user input.
 *
 * ```ts
 * const db = await SiafuDB.open('notes');
 * await db.execute('CREATE (:Note {title: $t})', { t: "it's \"quoted\" \\ safe" });
 * const { rows } = await db.query('MATCH (n:Note) RETURN n.title');
 * await db.close();
 * ```
 */
export class SiafuDB {
  /** Which backend is in use. */
  readonly backend: Backend;
  /** True if an existing snapshot was restored on open. */
  readonly restored: boolean;
  /**
   * Result of the `navigator.storage.persist()` request made at open time:
   * `true` = the browser promised not to evict this origin's storage;
   * `false` = best-effort storage that may be evicted under pressure;
   * `null` = API unsupported.
   */
  readonly persisted: boolean | null;

  private constructor(
    private readonly channel: Channel,
    readonly name: string,
    init: InitResult,
    private readonly detach: () => void,
  ) {
    this.backend = init.backend;
    this.restored = init.restored;
    this.persisted = init.persisted;
  }

  /**
   * Open (creating if needed) the database `name`, decrypting any stored snapshot.
   * @throws {DatabaseLockedError} with `lock: 'fail'` (or after `lockTimeoutMs`) when another tab holds it
   * @throws {TamperError} the stored snapshot failed authentication
   * @throws {RollbackError} the stored snapshot is older than one previously seen
   */
  static async open(name: string, options: OpenOptions = {}): Promise<SiafuDB> {
    const channel = new Channel(spawnWorker(options.workerUrl), (e) => {
      if (e.type === 'autosave-error') options.onAutosaveError?.(new SiafuError('storage', e.message));
      else if (e.type === 'saved') options.onSaved?.(e.counter);
    });
    const args: InitArgs = {
      name,
      backend: options.backend ?? 'auto',
      autosaveMs: options.autosaveMs ?? 500,
      lock: options.lock ?? 'wait',
      lockTimeoutMs: options.lockTimeoutMs ?? null,
    };
    const wasmUrl = toWasmUrl(options.wasmUrl);
    if (wasmUrl !== undefined) args.wasmUrl = wasmUrl;
    let init: InitResult;
    try {
      init = await channel.call<InitResult>({ op: 'init', args });
    } catch (e) {
      channel.terminate();
      throw e;
    }
    // Best-effort flush when the page is hidden/frozen: tab switches and mobile backgrounding are the usual last chance.
    let db: SiafuDB;
    const onHide = () => {
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') void db.flush().catch(() => undefined);
    };
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onHide);
    db = new SiafuDB(channel, name, init, () => {
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onHide);
    });
    return db;
  }

  /**
   * Run a write query (CREATE/MERGE/SET/DELETE...). Schedules an autosave.
   * @param params values bound to `$name` placeholders (JSON-representable)
   */
  execute(query: string, params?: Params): Promise<ExecuteResult> {
    return this.channel.call({ op: 'execute', args: { query, params: encodeParams(params) } });
  }

  /**
   * Run a read query. Cells are typed JSON (numbers, strings, booleans, null, lists).
   * @param params values bound to `$name` placeholders
   */
  query(query: string, params?: Params): Promise<QueryResult> {
    return this.channel.call({ op: 'query', args: { query, params: encodeParams(params) } });
  }

  /**
   * Writes captured by the engine's change log with `sequence > seq`.
   * The log lives in memory for this open session (it restarts at 1 after reopen).
   */
  changesSince(seq: number): Promise<ChangeLogEntry[]> {
    return this.channel.call({ op: 'changesSince', args: { seq } });
  }

  /** Encrypt and persist now if there are unsaved writes. Resolves when the data is durable. */
  flush(): Promise<void> {
    return this.channel.call({ op: 'flush' });
  }

  /** Flush, release the cross-tab lock and stop the worker. The handle is unusable afterwards. */
  async close(): Promise<void> {
    if (this.channel.closed) return;
    this.detach();
    try {
      await this.channel.call({ op: 'close' });
    } finally {
      this.channel.terminate();
    }
  }

  /** Persistence status plus a storage quota estimate. */
  async storage(): Promise<StorageReport> {
    const estimate = (await navigator.storage?.estimate?.()) ?? null;
    return { persisted: this.persisted, estimate };
  }

  /** `navigator.storage.estimate()`: usage and quota for this origin. */
  static async estimate(): Promise<StorageEstimate | null> {
    return (await navigator.storage?.estimate?.()) ?? null;
  }

  /**
   * Permanently delete the database `name`: stored snapshots in both backends,
   * its data key and its rollback counter. Fails with `DatabaseLockedError`
   * if the database is open anywhere.
   */
  static async destroy(name: string, options: Pick<OpenOptions, 'workerUrl'> = {}): Promise<void> {
    const channel = new Channel(spawnWorker(options.workerUrl), () => undefined);
    try {
      await channel.call({ op: 'destroy', args: { name } });
    } finally {
      channel.terminate();
    }
  }
}
