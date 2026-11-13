/**
 * Browser blob backends. Both store only ciphertext.
 *
 * - {@link OpfsBackend}: Origin Private File System via `FileSystemSyncAccessHandle`
 *   (Worker only). Two slots (`<base>.0`, `<base>.1`) written alternately by
 *   counter parity, so a crash mid-write never destroys the previous good
 *   snapshot.
 * - {@link IdbBackend}: one IndexedDB record per database; a single `put` is atomic.
 */
import { SiafuError } from './errors.js';
import type { BlobBackend } from './store.js';
import { idbDelete, idbGet, idbPut, openIdb } from './vault.js';

/** OPFS file names used for database `name` (two alternating slots). Exposed for diagnostics and tests. */
export function opfsFileNames(name: string): [string, string] {
  const base = `siafudb-${encodeURIComponent(name)}`;
  return [`${base}.0`, `${base}.1`];
}

/** True when this context can use the OPFS backend (a Worker with sync access handles). */
export function opfsAvailable(): boolean {
  return (
    typeof navigator !== 'undefined' &&
    typeof navigator.storage?.getDirectory === 'function' &&
    typeof (globalThis as { FileSystemFileHandle?: { prototype: object } }).FileSystemFileHandle?.prototype === 'object' &&
    'createSyncAccessHandle' in (globalThis as { FileSystemFileHandle: { prototype: object } }).FileSystemFileHandle.prototype
  );
}

export class OpfsBackend implements BlobBackend {
  readonly kind = 'opfs' as const;
  private readonly files: [string, string];

  constructor(name: string) {
    this.files = opfsFileNames(name);
  }

  private async withHandle<T>(file: string, create: boolean, fn: (h: FileSystemSyncAccessHandle) => T): Promise<T | undefined> {
    const root = await navigator.storage.getDirectory();
    let fh: FileSystemFileHandle;
    try {
      fh = await root.getFileHandle(file, { create });
    } catch (e) {
      if ((e as DOMException).name === 'NotFoundError') return undefined;
      throw new SiafuError('storage', `OPFS open failed: ${(e as Error).message}`, { cause: e });
    }
    const h = await fh.createSyncAccessHandle();
    try {
      return fn(h);
    } finally {
      h.close();
    }
  }

  async readCandidates(): Promise<Uint8Array[]> {
    const out: Uint8Array[] = [];
    for (const f of this.files) {
      const buf = await this.withHandle(f, false, (h) => {
        const b = new Uint8Array(h.getSize());
        const n = h.read(b, { at: 0 });
        return n === b.length ? b : b.subarray(0, n);
      });
      if (buf && buf.length > 0) out.push(buf);
    }
    return out;
  }

  async write(blob: Uint8Array, counter: number): Promise<void> {
    const file = this.files[counter % 2]!;
    await this.withHandle(file, true, (h) => {
      h.truncate(0);
      const n = h.write(blob, { at: 0 });
      if (n !== blob.length) throw new SiafuError('storage', `short OPFS write ${n}/${blob.length}`);
      h.flush();
    });
  }

  async deleteAll(): Promise<void> {
    const root = await navigator.storage.getDirectory();
    for (const f of this.files) await root.removeEntry(f).catch(() => undefined);
  }

  async close(): Promise<void> {}
}

const DATA_DB = 'siafudb-data';
const STORE = 'snapshots';

export class IdbBackend implements BlobBackend {
  readonly kind = 'idb' as const;
  constructor(private readonly name: string) {}

  async readCandidates(): Promise<Uint8Array[]> {
    const db = await openIdb(DATA_DB, [STORE]);
    try {
      const v = await idbGet<Uint8Array>(db, STORE, this.name);
      return v ? [new Uint8Array(v)] : [];
    } finally {
      db.close();
    }
  }

  async write(blob: Uint8Array): Promise<void> {
    const db = await openIdb(DATA_DB, [STORE]);
    try {
      await idbPut(db, STORE, this.name, blob);
    } finally {
      db.close();
    }
  }

  async deleteAll(): Promise<void> {
    const db = await openIdb(DATA_DB, [STORE]);
    try {
      await idbDelete(db, STORE, this.name);
    } finally {
      db.close();
    }
  }

  async close(): Promise<void> {}
}

/** Pick a backend: OPFS when available and allowed, else IndexedDB. */
export function chooseBackend(name: string, want: 'auto' | 'opfs' | 'idb'): BlobBackend {
  if (want === 'idb') return new IdbBackend(name);
  if (opfsAvailable()) return new OpfsBackend(name);
  if (want === 'opfs') throw new SiafuError('storage', 'OPFS with sync access handles is not available in this context');
  return new IdbBackend(name);
}
