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
  /**
   * Base URL of a GSP relay. `https://` only, except `http://` for
   * `localhost`, `127.0.0.1` and `[::1]`.
   */
  relayUrl: string;
}

/** For relays that only accept new sync groups from holders of a registration secret. */
export interface SyncRegistrationOptions {
  /**
   * The relay's registration secret. It is sent to the relay when the group
   * is first uploaded and is kept in memory only: never stored, never logged.
   */
  registrationToken?: string;
}

/** What this device knows about one device of the sync group. */
export interface SyncDeviceStatus {
  /** Stable id of the device within the group (hex). Not a secret. */
  id: string;
  /** This device itself. */
  own: boolean;
  /** Number of that device's commits applied here. */
  appliedSeq: number;
  /** Commits of that device received here but not applicable yet. */
  pending: number;
  /** Some of that device's commits were dropped for lack of room; its later ones wait for a resync. */
  gap: boolean;
  /**
   * By how many ms that device's latest commit was ahead of this device's
   * clock when that kept it from being applied (0: it was not). Over 24
   * hours its commits are refused: that device's clock (or this one's) is wrong.
   */
  clockLeadMs: number;
  /** That device published two different histories (its storage was rolled back); devices may disagree about what it wrote. */
  equivocated: boolean;
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
  /**
   * Received commits that cannot be applied yet: they wait for an earlier
   * commit of the same device, for that device's first message, or for this
   * device's clock to catch up with theirs.
   */
  pendingDownloads: number;
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
  /**
   * This device's stored state was rolled back (restored from a backup) and
   * conflicts with what it had already uploaded. Its writes stay local until
   * `db.sync.rejoinAsNewDevice()` is called. Downloads continue.
   */
  needsRejoin: boolean;
  /**
   * Local writes are being held back from upload until this device has read
   * the relay: right after opening (for one round trip, or until the relay
   * turns out to be unreachable), or because the device was restored from
   * an older copy and is reading its own earlier uploads back. The writes
   * are in the database and are uploaded afterwards. If this stays true
   * while `connected`, the relay has lost part of this device's history:
   * call `db.sync.rejoinAsNewDevice()`.
   */
  catchingUp: boolean;
  /** Received commits were dropped for lack of room; the relay's log will be read again (automatic, with growing delays). */
  needsResync: boolean;
  /** Times the relay was found to have lost its data. History that only the relay held is gone. */
  relayResets: number;
  /** Every device known in the group, this one included. */
  deviceList: SyncDeviceStatus[];
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
  | {
      id: number;
      op: "syncCreate";
      args: { relayUrl: string; registrationToken?: string };
    }
  | { id: number; op: "syncStart"; args: { registrationToken?: string } }
  | { id: number; op: "syncJoin"; args: { relayUrl: string; code: string } }
  | {
      id: number;
      op:
        | "syncStatus"
        | "syncNow"
        | "syncStop"
        | "syncLeave"
        | "syncRejoin"
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
