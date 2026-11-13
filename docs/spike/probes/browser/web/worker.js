import init, * as w from './pkg/browserprobe.js';
import * as s from './store.js';

const SECRET = 'SECRET-PLAINTEXT-ZEBRA-7731';
let ready = null;
const boot = () => (ready ??= init());

const ops = {
  async smoke() {
    await boot();
    w.init();
    const t0 = performance.now();
    const aff = w.exec("CREATE (:Person {name:'Amara', city:'Accra'})");
    w.exec("CREATE (:Person {name:'Tatenda', city:'Harare'})");
    const q = JSON.parse(w.query('MATCH (p:Person) RETURN p.name ORDER BY p.name'));
    return { aff, q, ms: performance.now() - t0, ctx: typeof WorkerGlobalScope !== 'undefined' ? 'worker' : 'main' };
  },
  async persist({ backend, n = 50 }) {
    await boot();
    w.init();
    for (let i = 0; i < n; i++) w.exec(`CREATE (:Doc {id: ${i}, name: 'doc-${i}', note: '${SECRET}'})`);
    const snap = w.export_snapshot();
    const plainHasSecret = s.containsBytes(snap, SECRET);
    const k = await s.getDataKey();
    const enc = await s.encrypt(k.dek, snap);
    const name = 'siafu.enc';
    if (backend === 'opfs') await s.opfsWrite(name, enc); else await s.idbWriteBlob(name, enc);
    return { backend, snapBytes: snap.length, encBytes: enc.length, plainHasSecret, encHasSecret: s.containsBytes(enc, SECRET), key: k };
  },
  async restore({ backend }) {
    await boot();
    const name = 'siafu.enc';
    const raw = backend === 'opfs' ? await s.opfsRead(name) : await s.idbReadBlob(name);
    if (!raw) return { found: false };
    const k = await s.getDataKey();
    const snap = await s.decrypt(k.dek, raw);
    w.load_snapshot(snap);
    const cnt = JSON.parse(w.query('MATCH (d:Doc) RETURN count(d)'));
    const one = JSON.parse(w.query("MATCH (d:Doc {id: 7}) RETURN d.name, d.note"));
    return { found: true, rawBytes: raw.length, rawHasSecret: s.containsBytes(raw, SECRET), kekCreated: k.kekCreated, cnt, one, secretOk: JSON.stringify(one).includes(SECRET) };
  },
  async tamper({ backend }) {
    await boot();
    const raw = backend === 'opfs' ? await s.opfsRead('siafu.enc') : await s.idbReadBlob('siafu.enc');
    raw[raw.length >> 1] ^= 1;
    const k = await s.getDataKey();
    try { await s.decrypt(k.dek, raw); return { rejected: false }; } catch (e) { return { rejected: true, err: String(e.name) }; }
  },
  async bench({ n = 10000 }) {
    await boot();
    w.init();
    let t = performance.now();
    w.bulk_insert(n);
    const insertMs = performance.now() - t;
    t = performance.now();
    const snap = w.export_snapshot();
    const exportMs = performance.now() - t;
    const k = await s.getDataKey();
    t = performance.now();
    const enc = await s.encrypt(k.dek, snap);
    const encMs = performance.now() - t;
    t = performance.now();
    await s.opfsWrite('bench.enc', enc);
    const writeMs = performance.now() - t;
    t = performance.now();
    const dec = await s.decrypt(k.dek, await s.opfsRead('bench.enc'));
    w.load_snapshot(dec);
    const restoreMs = performance.now() - t;
    const cnt = JSON.parse(w.query('MATCH (i:Item) RETURN count(i)'));
    await s.opfsDelete('bench.enc');
    return { n, insertMs, exportMs, encMs, writeMs, restoreMs, snapBytes: snap.length, encBytes: enc.length, cnt };
  },
  async cdc() {
    const m = await import('./cdcpkg/cdcprobe.js');
    await m.default();
    return JSON.parse(m.cdc_probe());
  },
};

self.onmessage = async (e) => {
  const { id, op, args } = e.data;
  try { self.postMessage({ id, ok: true, r: await ops[op](args || {}) }); }
  catch (err) { self.postMessage({ id, ok: false, err: String(err && err.stack || err) }); }
};
