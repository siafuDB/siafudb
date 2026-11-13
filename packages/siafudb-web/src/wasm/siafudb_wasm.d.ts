/* tslint:disable */
/* eslint-disable */

/**
 * An in-memory SiafuDB instance. Persistence is the caller's job:
 * `exportSnapshot()` / `Database.fromSnapshot()`.
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
     * Serialise the whole database (plaintext; encrypt before storing).
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
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_database_free: (a: number, b: number) => void;
    readonly database_changesSince: (a: number, b: number) => [number, number, number, number];
    readonly database_execute: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly database_exportSnapshot: (a: number) => [number, number, number, number];
    readonly database_fromSnapshot: (a: number, b: number) => [number, number, number];
    readonly database_new: () => [number, number, number];
    readonly database_query: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
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
