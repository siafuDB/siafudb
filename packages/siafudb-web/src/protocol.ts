/** Wire types shared by the client and the worker. Not part of the public API. */

/** A JSON-representable value, as bound to `$name` query parameters and returned in result cells. */
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [k: string]: JsonValue };

/** Named query parameters (`$name` in the query text). */
export type Params = Record<string, JsonValue>;

/** Result of {@link SiafuDB.query}. Cells are typed JSON: numbers, strings, booleans, null, lists. */
export interface QueryResult {
  columns: string[];
  rows: JsonValue[][];
}

/** Result of {@link SiafuDB.execute}. */
export interface ExecuteResult {
  rowsAffected: number;
}

/** One captured write, as recorded by the engine's change log. */
export interface ChangeLogEntry {
  /** Monotonic, starts at 1 within this in-memory database instance. */
  sequence: number;
  /** Unix epoch milliseconds. */
  timestamp_ms: number;
  query: string;
  rows_affected: number;
  params: Params;
}

export type Backend = "opfs" | "idb";

export interface InitArgs {
  name: string;
  backend: "auto" | Backend;
  /** Debounce in ms; 0 disables autosave (use `flush()`). */
  autosaveMs: number;
  lock: "wait" | "fail";
  lockTimeoutMs: number | null;
  wasmUrl?: string;
}

export interface InitResult {
  backend: Backend;
  /** True if an existing snapshot was loaded. */
  restored: boolean;
  /** Result of `navigator.storage.persist()`; null if unsupported. */
  persisted: boolean | null;
  writeCounter: number;
}

export type Request =
  | { id: number; op: "init"; args: InitArgs }
  | {
      id: number;
      op: "execute" | "query";
      args: { query: string; params: string | null };
    }
  | { id: number; op: "changesSince"; args: { seq: number } }
  | { id: number; op: "flush" | "close" }
  | { id: number; op: "destroy"; args: { name: string } };

export type Response =
  | { id: number; ok: true; result: unknown }
  | {
      id: number;
      ok: false;
      error: {
        code: string;
        message: string;
        suite?: number;
        found?: number;
        expected?: number;
      };
    };

export type WorkerEvent =
  | { type: "ready" }
  | { type: "autosave-error"; message: string }
  | { type: "saved"; counter: number };
