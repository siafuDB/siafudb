/* tslint:disable */
/* eslint-disable */

/**
 * An in-memory SiafuDB instance. Persistence is the caller's job:
 * `exportSnapshot()` / `Database.fromSnapshot()`.
 *
 * The `sync*` methods implement device-to-device sync (GSP sync v0) without
 * doing any I/O: they build and read relay frames and the caller moves the
 * bytes. Methods that take `now_ms` want `Date.now()`.
 */
export class Database {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Change-log entries with `sequence > seq`, as a JSON array.
     */
    changesSince(seq: number): string;
    /**
     * Run a write query. `params_json` is a JSON object string of `$name` bindings.
     * Returns JSON `{"rowsAffected": n}`.
     */
    execute(query: string, params_json?: string | null): string;
    /**
     * Serialise the whole database (plaintext; encrypt before storing). In a
     * sync group the bytes also hold the sync state, which includes the
     * group's secret: one blob, to be written in one step.
     */
    exportSnapshot(): Uint8Array;
    /**
     * Restore a database from bytes produced by `exportSnapshot()`.
     */
    static fromSnapshot(bytes: Uint8Array | number[]): Database;
    /**
     * Create an empty database.
     */
    constructor();
    /**
     * Run a read query. Returns JSON `{"columns": [...], "rows": [[...]]}`.
     */
    query(query: string, params_json?: string | null): string;
    /**
     * Set the string returned by `syncMeta`. Persisted by `exportSnapshot`.
     */
    setSyncMeta(meta: string): void;
    /**
     * Take the relay's `Ack`. Returns how many items left the outbox.
     */
    syncAcceptAck(reply: Uint8Array | number[]): number;
    /**
     * Call after every `execute` or `query` while in a sync group: queues
     * what the call wrote for upload. Returns the number of ops queued.
     */
    syncCommit(now_ms: number): number;
    /**
     * Create a new sync group with this database as its first device.
     * Returns the pairing code (a secret) to enter on another device. Data
     * already in the database is queued for upload.
     */
    syncCreate(now_ms: number): string;
    /**
     * Whether the database is in a sync group.
     */
    syncEnabled(): boolean;
    /**
     * Take the relay's `Batch` and apply it. Returns JSON `{"items": n,
     * "ops": n, "changes": n, "more": bool, "relayReset": bool,
     * "needsResync": bool, "forked": bool}`; `changes` is the number of
     * changes applied to the graph. Persist when `ops > 0`. With
     * `relayReset`, push again: what was in flight is back in the outbox.
     */
    syncIngestBatch(reply: Uint8Array | number[], now_ms: number): string;
    /**
     * Join the sync group of a pairing code.
     */
    syncJoin(code: string, now_ms: number): void;
    /**
     * Leave the sync group. The data stays; the sync state is dropped.
     */
    syncLeave(): void;
    /**
     * A string the transport keeps with the database (the relay URL).
     */
    syncMeta(): string;
    /**
     * The group's pairing code (a secret), to add another device.
     */
    syncPairingCode(): string | undefined;
    /**
     * Apply received changes that were held because their clock was ahead.
     * Returns the number of changes applied to the graph.
     */
    syncPoll(now_ms: number): number;
    /**
     * The `Pull` frame for the current cursor, for `POST /v0/pull`.
     */
    syncPullFrame(): Uint8Array;
    /**
     * The next `Push` frame for `POST /v0/push`, or `undefined` when there
     * is nothing to push. Pass the reply body to `syncAcceptAck`.
     */
    syncPushFrame(): Uint8Array | undefined;
    /**
     * Give a device whose status says `forked` a new identity in the group
     * so that it can upload again; writes made while forked are queued for
     * upload. Returns the number of ops queued for them. Throws if the
     * device is not forked.
     */
    syncRejoin(now_ms: number): number;
    /**
     * The bearer token (base64url) for `Authorization: Bearer ...` at the
     * relay with this origin (`scheme://host[:port]`). Every relay has its
     * own token. Throws if not in a sync group or the origin is not usable.
     */
    syncRelayToken(relay_origin: string): string;
    /**
     * Begin a resync: the next pulls read the relay's log from the start.
     * Call when a `syncIngestBatch` result or the status says
     * `needsResync`, with a growing delay between attempts.
     */
    syncStartResync(): void;
    /**
     * JSON: `null` when not in a sync group, else `{"groupId", "outbox",
     * "cursor", "localOps", "remoteOps", "pending", "devices", "rejected",
     * "droppedLocal", "skippedRemote", "lastProblem", "unechoed", "forked",
     * "needsResync", "relayResets", "deviceList": [{"id", "own",
     * "appliedSeq", "pending", "gap", "clockLeadMs", "equivocated"}]}`.
     */
    syncStatus(): string;
}

/**
 * The canonical form of a relay origin (`scheme://host[:port]`), as the
 * relay token is bound to it. Throws if `origin` is not an http(s) origin.
 */
export function canonicalRelayOrigin(origin: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_database_free: (a: number, b: number) => void;
    readonly canonicalRelayOrigin: (a: number, b: number) => [number, number, number, number];
    readonly database_changesSince: (a: number, b: number) => [number, number, number, number];
    readonly database_execute: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly database_exportSnapshot: (a: number) => [number, number, number, number];
    readonly database_fromSnapshot: (a: number, b: number) => [number, number, number];
    readonly database_new: () => [number, number, number];
    readonly database_query: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly database_setSyncMeta: (a: number, b: number, c: number) => void;
    readonly database_syncAcceptAck: (a: number, b: number, c: number) => [number, number, number];
    readonly database_syncCommit: (a: number, b: number) => [number, number, number];
    readonly database_syncCreate: (a: number, b: number) => [number, number, number, number];
    readonly database_syncEnabled: (a: number) => number;
    readonly database_syncIngestBatch: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly database_syncJoin: (a: number, b: number, c: number, d: number) => [number, number];
    readonly database_syncLeave: (a: number) => void;
    readonly database_syncMeta: (a: number) => [number, number];
    readonly database_syncPairingCode: (a: number) => [number, number];
    readonly database_syncPoll: (a: number, b: number) => [number, number, number];
    readonly database_syncPullFrame: (a: number) => [number, number, number, number];
    readonly database_syncPushFrame: (a: number) => [number, number, number, number];
    readonly database_syncRejoin: (a: number, b: number) => [number, number, number];
    readonly database_syncRelayToken: (a: number, b: number, c: number) => [number, number, number, number];
    readonly database_syncStartResync: (a: number) => [number, number];
    readonly database_syncStatus: (a: number) => [number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
