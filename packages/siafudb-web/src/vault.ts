/**
 * Key custody.
 *
 * - KEK: one non-extractable AES-256-GCM `CryptoKey` per origin, persisted in
 *   IndexedDB by structured clone. Page script (and an XSS) can *use* it but
 *   cannot read its bytes.
 * - DEK: one random 256-bit key per database. Only its KEK-wrapped form
 *   (`nonce || ct || tag`, AAD = "siafudb:dek:v1:" + name) is stored; at runtime
 *   it is imported as a non-extractable `CryptoKey`.
 * - Counter high-water mark: the highest snapshot write counter ever
 *   committed, stored next to the keys and *outside* the snapshot store, so an
 *   attacker who swaps the snapshot file for an older valid copy is caught.
 *
 * Threat model: this protects data at rest (disk/profile theft, other
 * software reading OPFS/IndexedDB files) and detects tampering/rollback of
 * the snapshot store. It does not protect against code running in the origin
 * (it can call the KEK) or an attacker who can erase both the key vault and
 * the snapshot store.
 */
import { KeyMissingError } from "./errors.js";

/** Where data keys and counters live. Implemented by {@link IdbKeyVault} (browser) and {@link MemoryKeyVault} (tests). */
export interface KeyVault {
  /** Return the database's data key. With `create: false`, throws {@link KeyMissingError} if absent. */
  getDataKey(name: string, opts: { create: boolean }): Promise<CryptoKey>;
  /** Highest write counter committed for `name` (0 if none). */
  getCounter(name: string): Promise<number>;
  /** Record that counter `n` has been committed. Must never lower the stored value. */
  setCounter(name: string, n: number): Promise<void>;
  /** Forget the key and counter of `name`. */
  delete(name: string): Promise<void>;
}

const te = new TextEncoder();
const dekAad = (name: string) => te.encode(`siafudb:dek:v1:${name}`);

async function wrapDek(
  kek: CryptoKey,
  raw: Uint8Array,
  name: string,
): Promise<Uint8Array> {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(
    await crypto.subtle.encrypt(
      { name: "AES-GCM", iv, additionalData: dekAad(name) as BufferSource },
      kek,
      raw as BufferSource,
    ),
  );
  const out = new Uint8Array(12 + ct.length);
  out.set(iv);
  out.set(ct, 12);
  return out;
}

async function unwrapDek(
  kek: CryptoKey,
  wrapped: Uint8Array,
  name: string,
): Promise<CryptoKey> {
  const raw = new Uint8Array(
    await crypto.subtle.decrypt(
      {
        name: "AES-GCM",
        iv: wrapped.slice(0, 12),
        additionalData: dekAad(name) as BufferSource,
      },
      kek,
      wrapped.slice(12),
    ),
  );
  try {
    return await crypto.subtle.importKey("raw", raw, "AES-GCM", false, [
      "encrypt",
      "decrypt",
    ]);
  } finally {
    raw.fill(0);
  }
}

async function newDek(
  kek: CryptoKey,
  name: string,
): Promise<{ wrapped: Uint8Array; key: CryptoKey }> {
  const raw = crypto.getRandomValues(new Uint8Array(32));
  const wrapped = await wrapDek(kek, raw, name);
  const key = await crypto.subtle.importKey("raw", raw, "AES-GCM", false, [
    "encrypt",
    "decrypt",
  ]);
  raw.fill(0);
  return { wrapped, key };
}

const newKek = () =>
  crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, [
    "encrypt",
    "decrypt",
  ]);

/** In-memory vault for tests and non-browser hosts. Nothing survives the process. */
export class MemoryKeyVault implements KeyVault {
  private kek: CryptoKey | null = null;
  private readonly wrapped = new Map<string, Uint8Array>();
  private readonly counters = new Map<string, number>();

  async getDataKey(
    name: string,
    opts: { create: boolean },
  ): Promise<CryptoKey> {
    this.kek ??= await newKek();
    const w = this.wrapped.get(name);
    if (w) return unwrapDek(this.kek, w, name);
    if (!opts.create) throw new KeyMissingError(name);
    const { wrapped, key } = await newDek(this.kek, name);
    this.wrapped.set(name, wrapped);
    return key;
  }
  async getCounter(name: string): Promise<number> {
    return this.counters.get(name) ?? 0;
  }
  async setCounter(name: string, n: number): Promise<void> {
    this.counters.set(name, Math.max(n, this.counters.get(name) ?? 0));
  }
  async delete(name: string): Promise<void> {
    this.wrapped.delete(name);
    this.counters.delete(name);
  }
}

// ---- IndexedDB plumbing (shared with the IDB blob backend) ----

/** Open (and create on first use) an IndexedDB database with the given object stores. */
export function openIdb(
  dbName: string,
  stores: string[],
): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(dbName, 1);
    req.onupgradeneeded = () => {
      for (const s of stores)
        if (!req.result.objectStoreNames.contains(s))
          req.result.createObjectStore(s);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
    req.onblocked = () => reject(new Error("indexedDB open blocked"));
  });
}

export function idbGet<T>(
  db: IDBDatabase,
  store: string,
  key: string,
): Promise<T | undefined> {
  return new Promise((resolve, reject) => {
    const r = db.transaction(store, "readonly").objectStore(store).get(key);
    r.onsuccess = () => resolve(r.result as T | undefined);
    r.onerror = () => reject(r.error);
  });
}

export function idbPut(
  db: IDBDatabase,
  store: string,
  key: string,
  value: unknown,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const t = db.transaction(store, "readwrite");
    t.objectStore(store).put(value, key);
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
    t.onabort = () => reject(t.error);
  });
}

export function idbDelete(
  db: IDBDatabase,
  store: string,
  key: string,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const t = db.transaction(store, "readwrite");
    t.objectStore(store).delete(key);
    t.oncomplete = () => resolve();
    t.onerror = () => reject(t.error);
  });
}

/** Browser vault: non-extractable KEK + wrapped DEKs + counters, all in IndexedDB. */
export class IdbKeyVault implements KeyVault {
  constructor(private readonly dbName = "siafudb-keys") {}

  private open() {
    return openIdb(this.dbName, ["keys", "counters"]);
  }

  async getDataKey(
    name: string,
    opts: { create: boolean },
  ): Promise<CryptoKey> {
    const db = await this.open();
    try {
      let kek = await idbGet<CryptoKey>(db, "keys", "kek");
      const wrapped = await idbGet<Uint8Array>(db, "keys", `dek:${name}`);
      if (wrapped) {
        if (!kek) throw new KeyMissingError(name);
        return await unwrapDek(kek, wrapped, name);
      }
      if (!opts.create) throw new KeyMissingError(name);
      if (!kek) {
        kek = await newKek();
        await idbPut(db, "keys", "kek", kek);
      }
      const fresh = await newDek(kek, name);
      await idbPut(db, "keys", `dek:${name}`, fresh.wrapped);
      return fresh.key;
    } finally {
      db.close();
    }
  }

  async getCounter(name: string): Promise<number> {
    const db = await this.open();
    try {
      return (await idbGet<number>(db, "counters", name)) ?? 0;
    } finally {
      db.close();
    }
  }

  async setCounter(name: string, n: number): Promise<void> {
    const db = await this.open();
    try {
      const cur = (await idbGet<number>(db, "counters", name)) ?? 0;
      if (n > cur) await idbPut(db, "counters", name, n);
    } finally {
      db.close();
    }
  }

  async delete(name: string): Promise<void> {
    const db = await this.open();
    try {
      await idbDelete(db, "keys", `dek:${name}`);
      await idbDelete(db, "counters", name);
    } finally {
      db.close();
    }
  }
}
