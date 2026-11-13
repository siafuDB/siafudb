/** Machine-readable error codes carried by every {@link SiafuError}. */
export type SiafuErrorCode =
  | "format" // blob is not a SiafuDB snapshot, or an unknown format version
  | "unsupported-cipher" // header names a cipher suite this build cannot run
  | "tamper" // authentication failed: ciphertext, header or AAD was modified
  | "rollback" // write counter is older than the highest one seen (or blob vanished)
  | "key-missing" // an encrypted snapshot exists but its data key does not
  | "locked" // another tab/worker holds the single-writer lock
  | "closed" // the database handle was closed
  | "query" // the engine rejected a query
  | "storage" // the storage backend failed
  | "sync" // sync is not set up, was stopped, or the relay could not be used
  | "internal";

/** Base class for every error thrown by `@siafudb/web`. */
export class SiafuError extends Error {
  readonly code: SiafuErrorCode;
  constructor(
    code: SiafuErrorCode,
    message: string,
    options?: { cause?: unknown },
  ) {
    super(message, options);
    this.name = new.target.name;
    this.code = code;
  }
}

/** The stored blob is malformed or has an unknown format version. */
export class FormatError extends SiafuError {
  constructor(message: string) {
    super("format", message);
  }
}

/** The blob uses a cipher suite that this build does not implement (for example id 1). */
export class UnsupportedCipherError extends SiafuError {
  constructor(readonly suite: number) {
    super(
      "unsupported-cipher",
      `cipher suite ${suite} is not supported by this build`,
    );
  }
}

/** Authentication failed: the stored bytes were modified, or belong to another database/key. */
export class TamperError extends SiafuError {
  constructor(
    message = "snapshot failed authentication (tampered, corrupt, or wrong key)",
  ) {
    super("tamper", message);
  }
}

/** A snapshot older than one already seen was presented (rollback / replay), or it was deleted. */
export class RollbackError extends SiafuError {
  constructor(
    readonly found: number,
    readonly expectedAtLeast: number,
  ) {
    super(
      "rollback",
      `snapshot write counter ${found} is older than the last seen counter ${expectedAtLeast}; ` +
        "refusing to load (rollback, or the snapshot was deleted). Use SiafuDB.destroy(name) to reset.",
    );
  }
}

/** The data key for an existing encrypted snapshot is not available. */
export class KeyMissingError extends SiafuError {
  constructor(name: string) {
    super(
      "key-missing",
      `data key for database "${name}" is missing; the snapshot cannot be decrypted`,
    );
  }
}

/** Another tab or worker holds the exclusive single-writer lock for this database name. */
export class DatabaseLockedError extends SiafuError {
  constructor(name: string) {
    super("locked", `database "${name}" is open in another tab or worker`);
  }
}

/** The handle was closed (or its worker died). */
export class ClosedError extends SiafuError {
  constructor() {
    super("closed", "database is closed");
  }
}

/** Rebuild a typed error from the `{code, message}` pair that crosses the worker boundary. */
export function reviveError(e: {
  code?: string;
  message: string;
  suite?: number;
  found?: number;
  expected?: number;
}): Error {
  switch (e.code) {
    case "format":
      return new FormatError(e.message);
    case "unsupported-cipher":
      return new UnsupportedCipherError(e.suite ?? -1);
    case "tamper":
      return new TamperError(e.message);
    case "rollback":
      return new RollbackError(e.found ?? -1, e.expected ?? -1);
    case "locked":
      return Object.assign(new DatabaseLockedError("?"), {
        message: e.message,
      });
    case "closed":
      return new ClosedError();
    case "key-missing":
    case "query":
    case "storage":
    case "sync":
      return new SiafuError(e.code, e.message);
    default:
      return new SiafuError("internal", e.message);
  }
}

/** Flatten an error for `postMessage`. */
export function serializeError(err: unknown): {
  code: string;
  message: string;
  suite?: number;
  found?: number;
  expected?: number;
} {
  if (err instanceof SiafuError) {
    const out: {
      code: string;
      message: string;
      suite?: number;
      found?: number;
      expected?: number;
    } = {
      code: err.code,
      message: err.message,
    };
    if (err instanceof UnsupportedCipherError) out.suite = err.suite;
    if (err instanceof RollbackError) {
      out.found = err.found;
      out.expected = err.expectedAtLeast;
    }
    return out;
  }
  return {
    code: "internal",
    message: err instanceof Error ? err.message : String(err),
  };
}
