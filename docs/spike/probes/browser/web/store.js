// Storage + crypto helpers shared by page and worker. Run only in a worker for OPFS sync handles.
const te = new TextEncoder();

function idb(dbName = 'siafu-keys') {
  return new Promise((res, rej) => {
    const r = indexedDB.open(dbName, 1);
    r.onupgradeneeded = () => {
      r.result.createObjectStore('keys');
      r.result.createObjectStore('blobs');
    };
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
}
const idbGet = (db, store, k) => new Promise((res, rej) => {
  const q = db.transaction(store).objectStore(store).get(k);
  q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
});
const idbPut = (db, store, k, v) => new Promise((res, rej) => {
  const t = db.transaction(store, 'readwrite');
  t.objectStore(store).put(v, k);
  t.oncomplete = () => res(); t.onerror = () => rej(t.error);
});

async function aesEnc(key, bytes) {
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, bytes));
  const out = new Uint8Array(12 + ct.length);
  out.set(iv); out.set(ct, 12);
  return out;
}
async function aesDec(key, blob) {
  return new Uint8Array(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: blob.slice(0, 12) }, key, blob.slice(12)));
}

// KEK: non-extractable AES-GCM CryptoKey persisted in IndexedDB (structured clone).
// DEK: random 32 bytes, stored only wrapped (AES-GCM under KEK).
export async function getDataKey() {
  const db = await idb();
  let kek = await idbGet(db, 'keys', 'kek');
  let created = false;
  if (!kek) {
    kek = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, /*extractable*/ false, ['encrypt', 'decrypt']);
    await idbPut(db, 'keys', 'kek', kek);
    created = true;
  }
  let wrapped = await idbGet(db, 'keys', 'wrapped-dek');
  let raw;
  if (!wrapped) {
    raw = crypto.getRandomValues(new Uint8Array(32));
    wrapped = await aesEnc(kek, raw);
    await idbPut(db, 'keys', 'wrapped-dek', wrapped);
  } else {
    raw = await aesDec(kek, wrapped);
  }
  const dek = await crypto.subtle.importKey('raw', raw, 'AES-GCM', false, ['encrypt', 'decrypt']);
  raw.fill(0);
  db.close();
  return { dek, kekExtractable: kek.extractable, kekCreated: created, wrappedLen: wrapped.length };
}

export const encrypt = aesEnc;
export const decrypt = aesDec;

// ---- OPFS (worker only) ----
export async function opfsWrite(name, bytes) {
  const root = await navigator.storage.getDirectory();
  const fh = await root.getFileHandle(name, { create: true });
  const h = await fh.createSyncAccessHandle();
  try {
    h.truncate(0);
    const n = h.write(bytes, { at: 0 });
    h.flush();
    if (n !== bytes.length) throw new Error(`short write ${n}/${bytes.length}`);
  } finally { h.close(); }
}
export async function opfsRead(name) {
  const root = await navigator.storage.getDirectory();
  const fh = await root.getFileHandle(name);
  const h = await fh.createSyncAccessHandle();
  try {
    const buf = new Uint8Array(h.getSize());
    h.read(buf, { at: 0 });
    return buf;
  } finally { h.close(); }
}
export async function opfsDelete(name) {
  const root = await navigator.storage.getDirectory();
  await root.removeEntry(name).catch(() => {});
}

// ---- IndexedDB whole-blob ----
export async function idbWriteBlob(name, bytes) {
  const db = await idb(); await idbPut(db, 'blobs', name, bytes); db.close();
}
export async function idbReadBlob(name) {
  const db = await idb(); const v = await idbGet(db, 'blobs', name); db.close();
  return v ? new Uint8Array(v) : null;
}

export function containsBytes(hay, needleStr) {
  const n = te.encode(needleStr);
  outer: for (let i = 0; i <= hay.length - n.length; i++) {
    for (let j = 0; j < n.length; j++) if (hay[i + j] !== n[j]) continue outer;
    return true;
  }
  return false;
}
