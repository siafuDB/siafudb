/**
 * Encrypted snapshot store: a {@link BlobBackend} (where bytes live) plus a
 * {@link KeyVault} (keys and the rollback high-water mark).
 */
import { RollbackError, SiafuError } from "./errors.js";
import { decodeSnapshot, encodeSnapshot, peekCounter } from "./format.js";
import type { KeyVault } from "./vault.js";

/** Byte storage for opaque (already encrypted) snapshot blobs. */
export interface BlobBackend {
  readonly kind: "opfs" | "idb" | "memory";
  /** Every blob currently stored (OPFS keeps two slots; IndexedDB one). */
  readCandidates(): Promise<Uint8Array[]>;
  /** Durably store `blob`, which carries write counter `counter`. Must not destroy the previous good blob until the new one is complete. */
  write(blob: Uint8Array, counter: number): Promise<void>;
  /** Remove all stored blobs. */
  deleteAll(): Promise<void>;
  close(): Promise<void>;
}

/** In-memory backend for unit tests. Exposes `slots` so tests can tamper or replay. */
export class MemoryBackend implements BlobBackend {
  readonly kind = "memory" as const;
  slots: Uint8Array[] = [];
  async readCandidates() {
    return this.slots.map((s) => s.slice());
  }
  async write(blob: Uint8Array, counter: number) {
    this.slots[counter % 2] = blob.slice();
  }
  async deleteAll() {
    this.slots = [];
  }
  async close() {}
}

export class EncryptedSnapshotStore {
  private counter = 0;

  constructor(
    private readonly name: string,
    private readonly backend: BlobBackend,
    private readonly vault: KeyVault,
  ) {}

  /** Last committed write counter known to this store. */
  get writeCounter(): number {
    return this.counter;
  }

  /**
   * Load and verify the newest valid snapshot, or `null` for a new database.
   *
   * The newest candidate wins. If it fails authentication, older candidates
   * are tried only to survive a torn write (they must still satisfy the
   * rollback check); otherwise the authentication error is reported.
   *
   * @throws {TamperError} @throws {RollbackError} @throws {KeyMissingError} @throws {FormatError}
   */
  async load(): Promise<Uint8Array | null> {
    const highWater = await this.vault.getCounter(this.name);
    const blobs = await this.backend.readCandidates();
    if (blobs.length === 0) {
      if (highWater > 0) throw new RollbackError(0, highWater);
      this.counter = 0;
      return null;
    }
    const key = await this.vault.getDataKey(this.name, { create: false });
    const ordered = blobs
      .map((b) => ({ b, c: peekCounter(b) }))
      .sort((x, y) => y.c - x.c);
    let firstError: unknown;
    let staleCounter = -1;
    for (const { b } of ordered) {
      try {
        const r = await decodeSnapshot({ name: this.name, key, blob: b });
        if (r.counter < highWater) {
          staleCounter = Math.max(staleCounter, r.counter);
          continue;
        }
        if (r.counter > highWater)
          await this.vault.setCounter(this.name, r.counter); // crash between blob and counter commit
        this.counter = r.counter;
        return r.plaintext;
      } catch (e) {
        if (!(e instanceof SiafuError)) throw e;
        firstError ??= e;
      }
    }
    if (firstError) throw firstError;
    throw new RollbackError(staleCounter, highWater);
  }

  /** Encrypt `plaintext` under a fresh nonce and the next counter, store it, then advance the high-water mark. Returns the new counter. */
  async save(plaintext: Uint8Array): Promise<number> {
    const key = await this.vault.getDataKey(this.name, { create: true });
    const highWater = await this.vault.getCounter(this.name);
    const counter = Math.max(this.counter, highWater) + 1;
    const blob = await encodeSnapshot({
      name: this.name,
      key,
      counter,
      plaintext,
    });
    await this.backend.write(blob, counter); // blob first, counter second: a crash in between is recoverable
    await this.vault.setCounter(this.name, counter);
    this.counter = counter;
    return counter;
  }

  /** Delete blobs, key and counter. */
  async destroy(): Promise<void> {
    await this.backend.deleteAll();
    await this.vault.delete(this.name);
    this.counter = 0;
  }

  close(): Promise<void> {
    return this.backend.close();
  }
}
