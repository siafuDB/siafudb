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

/** Options for `db.sync.create` and `db.sync.join`. */
export interface SyncRelayOptions {
  /** Base URL of a GSP relay (`https://...`; `http://` for localhost). */
  relayUrl: string;
}

/** Result of `db.sync.status()`. */
export interface SyncStatus {
  /** This database is in a sync group. */
  enabled: boolean;
  /** The background sync loop is running (false after `stop()`). */
  running: boolean;
  /** The latest request to the relay succeeded. */
  connected: boolean;
  relayUrl: string | null;
  /** Local commits queued for upload (they survive reloads). */
  pendingUploads: number;
  /** Devices seen in the group, this one included. */
  devices: number;
  /** `Date.now()` of the latest successful request to the relay. */
  lastSyncAt: number | null;
  /** Why the latest request to the relay failed, if it did. */
  lastError: string | null;
  /** Local changes the protocol could not carry (for example a value over 64 KiB); they stay on this device. */
  notUploaded: number;
  /** Remote changes this device could not apply or verify. */
  notApplied: number;
  /** The latest reason for `notUploaded` / `notApplied`. */
  lastProblem: string | null;
}

/** Passed to `db.sync.onRemoteChange` listeners. */
export interface RemoteChangeEvent {
  /** Number of graph changes (node or edge puts and deletes) just applied. */
  changes: number;
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
  | { id: number; op: "syncCreate"; args: { relayUrl: string } }
  | { id: number; op: "syncJoin"; args: { relayUrl: string; code: string } }
  | {
      id: number;
      op:
        | "syncStatus"
        | "syncNow"
        | "syncStop"
        | "syncStart"
        | "syncLeave"
        | "syncPairingCode";
    }
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
  | { type: "saved"; counter: number }
  | { type: "sync-applied"; changes: number };
