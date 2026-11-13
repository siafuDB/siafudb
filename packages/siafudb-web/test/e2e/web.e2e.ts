import { createReadStream, existsSync } from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, type Browser, type BrowserContext, type Page } from 'playwright';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const MIME: Record<string, string> = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.map': 'application/json',
};
const MARKER = 'SECRET-PLAINTEXT-ZEBRA-7731';
// Strings that would break naive interpolation into a query.
const HOSTILE = [
  `O'Brien`,
  `a"b`,
  `back\\slash\\`,
  `x' }) DETACH DELETE n //`,
  `"; MATCH (n) DETACH DELETE n; //`,
  `\\' OR 1=1 --`,
  `line1\nline2\ttab \u0000nul 🐜`,
  '`tick`',
];

let server: http.Server;
let origin = '';
let browser: Browser;
let ctx: BrowserContext;

beforeAll(async () => {
  server = http.createServer((req, res) => {
    const rel = decodeURIComponent((req.url ?? '/').split('?')[0]!);
    const file = path.join(root, rel === '/' ? '/test/e2e/site/index.html' : rel);
    if (!file.startsWith(root) || !existsSync(file) || file === root) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
    createReadStream(file).pipe(res);
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  browser = await chromium.launch();
  ctx = await browser.newContext();
});

afterAll(async () => {
  await browser?.close();
  server?.close();
});

async function newPage(): Promise<Page> {
  const page = await ctx.newPage();
  page.on('pageerror', (e) => console.log('PAGEERROR', e.message));
  await page.goto(`${origin}/test/e2e/site/index.html`);
  return page;
}

interface Failure {
  failed: { name: string; code?: string; message: string };
}
const isFailure = (x: unknown): x is Failure => typeof x === 'object' && x !== null && 'failed' in x;

/** Raw bytes of every OPFS file belonging to `name`, read from the page (main thread). */
async function readOpfs(page: Page, name: string): Promise<number[][]> {
  return page.evaluate(async (n) => {
    const { opfsFileNames } = await (window as any).siafuReady;
    const root = await navigator.storage.getDirectory();
    const out: number[][] = [];
    for (const f of opfsFileNames(n)) {
      try {
        const file = await (await root.getFileHandle(f)).getFile();
        out.push([...new Uint8Array(await file.arrayBuffer())]);
      } catch {
        out.push([]);
      }
    }
    return out;
  }, name);
}

async function writeOpfs(page: Page, name: string, slots: number[][]): Promise<void> {
  await page.evaluate(
    async ({ n, slots }) => {
      const { opfsFileNames } = await (window as any).siafuReady;
      const root = await navigator.storage.getDirectory();
      const names = opfsFileNames(n);
      for (let i = 0; i < names.length; i++) {
        const bytes = slots[i] ?? [];
        if (bytes.length === 0) {
          await root.removeEntry(names[i]!).catch(() => undefined);
          continue;
        }
        const w = await (await root.getFileHandle(names[i]!, { create: true })).createWritable();
        await w.write(new Uint8Array(bytes));
        await w.close();
      }
    },
    { n: name, slots },
  );
}

const hasMarker = (bytes: number[]) => Buffer.from(bytes).includes(Buffer.from(MARKER));

/** Open `name` in the page, swallowing errors into a `{failed}` result. */
function openIn(page: Page, name: string, opts: Record<string, unknown> = {}, key = 'db') {
  return page.evaluate(
    async ({ name, opts, key }) => {
      const { SiafuDB } = await (window as any).siafuReady;
      try {
        const db = await SiafuDB.open(name, opts);
        (window as unknown as Record<string, unknown>)[key] = db;
        return { backend: db.backend, restored: db.restored, persisted: db.persisted };
      } catch (e) {
        const err = e as Error & { code?: string };
        return { failed: { name: err.name, code: err.code, message: err.message } };
      }
    },
    { name, opts, key },
  );
}

describe('@siafudb/web in headless Chromium', () => {
  it('create + query with hostile parameters, typed results, persist() and estimate()', async () => {
    const page = await newPage();
    const opened = await openIn(page, 'hostile');
    expect(isFailure(opened)).toBe(false);
    if (isFailure(opened)) return;
    expect(opened.backend).toBe('opfs');
    expect(opened.restored).toBe(false);
    expect([true, false, null]).toContain(opened.persisted);

    const r = await page.evaluate(async (hostile) => {
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      for (const [i, s] of hostile.entries()) {
        await db.execute('CREATE (:Doc {id: $id, name: $s, n: $n, ok: $ok, none: $none})', {
          id: i,
          s,
          n: 1.5,
          ok: true,
          none: null,
        });
      }
      const total = await db.query('MATCH (d:Doc) RETURN count(d)');
      const found: unknown[] = [];
      for (const s of hostile) {
        const q = await db.query('MATCH (d:Doc {name: $s}) RETURN d.name, d.id, d.n, d.ok', { s });
        found.push(q);
      }
      const bad = await db.query('MATCH (d:Doc) WHERE d.name = $s RETURN count(d)', { s: `' OR 1=1 //` });
      const changes = await db.changesSince(0);
      const tail = await db.changesSince(changes.length - 1);
      let errName = '';
      try {
        await db.query('THIS IS NOT A QUERY');
      } catch (e) {
        errName = (e as Error & { code?: string }).code ?? (e as Error).name;
      }
      const est = await (await (window as any).siafuReady).SiafuDB.estimate();
      const storage = await db.storage();
      return { total, found, bad, nChanges: changes.length, firstChange: changes[0], tail, errName, quota: est?.quota ?? 0, storage };
    }, HOSTILE);

    expect(r.total.rows).toEqual([[HOSTILE.length]]);
    r.found.forEach((q, i) => {
      const rows = (q as { rows: unknown[][] }).rows;
      expect(rows).toEqual([[HOSTILE[i], i, 1.5, true]]); // exactly one row, round-tripped byte-for-byte, typed cells
    });
    expect(r.bad.rows).toEqual([[0]]);
    expect(r.nChanges).toBe(HOSTILE.length);
    expect(r.firstChange).toMatchObject({ sequence: 1, params: { id: 0, s: HOSTILE[0] } });
    expect(r.tail).toHaveLength(1);
    expect(r.errName).toBe('query');
    expect(r.quota).toBeGreaterThan(0);
    expect(r.storage.estimate?.quota).toBeGreaterThan(0);
    await page.close();
  });

  it('persists to OPFS, survives reload, and the raw OPFS bytes are ciphertext', async () => {
    const page = await newPage();
    await openIn(page, 'persist1', { autosaveMs: 0 });
    await page.evaluate(async (marker) => {
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      for (let i = 0; i < 50; i++) await db.execute('CREATE (:Doc {id: $i, note: $m})', { i, m: marker });
      await db.flush();
      await db.close();
    }, MARKER);

    const raw = await readOpfs(page, 'persist1');
    const present = raw.filter((b) => b.length > 0);
    expect(present.length).toBeGreaterThan(0);
    for (const b of present) {
      expect(hasMarker(b)).toBe(false);
      expect(Buffer.from(b.slice(0, 4)).toString()).toBe('SFDB');
    }

    await page.reload();
    const again = await openIn(page, 'persist1');
    expect(isFailure(again)).toBe(false);
    if (isFailure(again)) return;
    expect(again.backend).toBe('opfs');
    expect(again.restored).toBe(true);
    const q = await page.evaluate(async () => {
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      return {
        count: await db.query('MATCH (d:Doc) RETURN count(d)'),
        one: await db.query('MATCH (d:Doc {id: $i}) RETURN d.note', { i: 7 }),
      };
    });
    expect(q.count.rows).toEqual([[50]]);
    expect(q.one.rows).toEqual([[MARKER]]);
    await page.close();
  });

  it('debounced autosave persists without flush() or close()', async () => {
    const page = await newPage();
    await page.evaluate(async () => {
      const { SiafuDB } = await (window as any).siafuReady;
      let resolveSaved!: () => void;
      const saved = new Promise<void>((r) => (resolveSaved = r));
      let writes = 0;
      const db = await SiafuDB.open('autosave1', { autosaveMs: 50, onSaved: () => writes++ || undefined });
      const initial = writes; // the creation save
      void initial;
      (window as unknown as Record<string, unknown>).db = db;
      (window as unknown as Record<string, unknown>).savedPromise = saved;
      (window as unknown as Record<string, unknown>).resolveSaved = resolveSaved;
      await db.execute('CREATE (:T {v: $v})', { v: 'autosaved' });
      // wait until a save after the write has been reported
      await new Promise<void>((r) => {
        const t = setInterval(async () => {
          if (writes >= 2) {
            clearInterval(t);
            r();
          }
        }, 20);
      });
    });
    await page.reload(); // no flush/close: the worker is simply killed
    await openIn(page, 'autosave1');
    const q = await page.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.query('MATCH (t:T) RETURN t.v'));
    expect(q.rows).toEqual([['autosaved']]);
    await page.close();
  });

  it('rejects tampered bytes in OPFS (every slot) with TamperError', async () => {
    const page = await newPage();
    await openIn(page, 'tamper1', { autosaveMs: 0 });
    await page.evaluate(async (m) => {
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      await db.execute('CREATE (:Doc {note: $m})', { m });
      await db.flush();
      await db.close();
    }, MARKER);
    const raw = await readOpfs(page, 'tamper1');
    const evil = raw.map((b) => {
      const c = b.slice();
      if (c.length) c[c.length >> 1] = c[c.length >> 1]! ^ 1;
      return c;
    });
    await writeOpfs(page, 'tamper1', evil);
    const res = await openIn(page, 'tamper1');
    expect(isFailure(res) && res.failed.name).toBe('TamperError');
    expect(isFailure(res) && res.failed.code).toBe('tamper');
    // the failed open must not leave the single-writer lock held
    await writeOpfs(page, 'tamper1', raw);
    const ok = await openIn(page, 'tamper1');
    expect(isFailure(ok)).toBe(false);
    await page.close();
  });

  it('rejects a rolled-back (older, perfectly valid) snapshot with RollbackError', async () => {
    const page = await newPage();
    await openIn(page, 'rollback1', { autosaveMs: 0 });
    const save = (v: string) =>
      page.evaluate(async (v) => {
        const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
        await db.execute('CREATE (:V {v: $v})', { v });
        await db.flush();
      }, v);
    await save('v1');
    const old = await readOpfs(page, 'rollback1');
    await save('v2');
    await save('v3');
    await page.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.close());
    await writeOpfs(page, 'rollback1', old); // attacker restores the old, authentic files
    const res = await openIn(page, 'rollback1');
    expect(isFailure(res) && res.failed.name).toBe('RollbackError');
    await page.close();
  });

  it('falls back to IndexedDB when OPFS is unavailable; ciphertext only; survives reload; tamper rejected', async () => {
    const page = await newPage();
    const workerUrl = `${origin}/test/e2e/site/worker-no-opfs.js`;
    const opened = await openIn(page, 'idb1', { workerUrl, autosaveMs: 0 });
    expect(isFailure(opened)).toBe(false);
    if (isFailure(opened)) return;
    expect(opened.backend).toBe('idb');
    await page.evaluate(async (m) => {
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      for (let i = 0; i < 30; i++) await db.execute('CREATE (:Doc {id: $i, note: $m})', { i, m });
      await db.flush();
      await db.close();
    }, MARKER);

    const readBlob = () =>
      page.evaluate(
        () =>
          new Promise<number[]>((resolve, reject) => {
            const r = indexedDB.open('siafudb-data', 1);
            r.onerror = () => reject(r.error);
            r.onsuccess = () => {
              const g = r.result.transaction('snapshots').objectStore('snapshots').get('idb1');
              g.onsuccess = () => {
                r.result.close();
                resolve([...new Uint8Array(g.result as Uint8Array)]);
              };
              g.onerror = () => reject(g.error);
            };
          }),
      );
    const blob = await readBlob();
    expect(blob.length).toBeGreaterThan(100);
    expect(hasMarker(blob)).toBe(false);

    await page.reload();
    const again = await openIn(page, 'idb1', { workerUrl });
    expect(isFailure(again) ? again.failed.message : again.backend).toBe('idb');
    const q = await page.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.query('MATCH (d:Doc) RETURN count(d)'));
    expect(q.rows).toEqual([[30]]);
    await page.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.close());

    await page.evaluate(
      (bytes) =>
        new Promise<void>((resolve, reject) => {
          const b = new Uint8Array(bytes);
          b[b.length >> 1] = b[b.length >> 1]! ^ 1;
          const r = indexedDB.open('siafudb-data', 1);
          r.onsuccess = () => {
            const t = r.result.transaction('snapshots', 'readwrite');
            t.objectStore('snapshots').put(b, 'idb1');
            t.oncomplete = () => {
              r.result.close();
              resolve();
            };
            t.onerror = () => reject(t.error);
          };
        }),
      blob,
    );
    const bad = await openIn(page, 'idb1', { workerUrl });
    expect(isFailure(bad) && bad.failed.name).toBe('TamperError');
    await page.close();
  });

  it('two tabs: single-writer lock prevents concurrent opens and neither corrupts the other', async () => {
    const a = await newPage();
    const b = await newPage();
    await openIn(a, 'shared', { autosaveMs: 0 });
    await a.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.execute('CREATE (:Row {who: $w})', { w: 'A1' }));
    await a.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.flush());

    // B refuses immediately (lock: 'fail')
    const refused = await openIn(b, 'shared', { lock: 'fail' });
    expect(isFailure(refused) && refused.failed.name).toBe('DatabaseLockedError');
    // ...or times out while waiting
    const timedOut = await openIn(b, 'shared', { lock: 'wait', lockTimeoutMs: 200 });
    expect(isFailure(timedOut) && timedOut.failed.name).toBe('DatabaseLockedError');

    // B queues (default lock: 'wait') while A still holds it
    await b.evaluate(async () => {
      const { SiafuDB } = await (window as any).siafuReady;
      (window as unknown as Record<string, unknown>).bOpen = SiafuDB.open('shared', { autosaveMs: 0 }).then((db) => {
        (window as unknown as Record<string, unknown>).db = db;
        return db;
      });
    });
    await new Promise((r) => setTimeout(r, 300));
    expect(await b.evaluate(() => !!(window as unknown as Record<string, unknown>).db)).toBe(false); // still waiting

    await a.evaluate(async () => {
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      await db.execute('CREATE (:Row {who: $w})', { w: 'A2' });
      await db.close(); // flushes, then releases the lock
    });

    const seenByB = await b.evaluate(async () => {
      await (window as unknown as { bOpen: Promise<unknown> }).bOpen;
      const db = (window as unknown as { db: import('/dist/index.js').SiafuDB }).db;
      const before = await db.query('MATCH (r:Row) RETURN r.who ORDER BY r.who');
      await db.execute('CREATE (:Row {who: $w})', { w: 'B1' });
      await db.close();
      return before.rows;
    });
    expect(seenByB).toEqual([['A1'], ['A2']]);

    await openIn(a, 'shared'); // A reopens and must see B's write on top of its own
    const final = await a.evaluate(() => (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.query('MATCH (r:Row) RETURN r.who ORDER BY r.who'));
    expect(final.rows).toEqual([['A1'], ['A2'], ['B1']]);
    await a.close();
    await b.close();
  });

  it('destroy() deletes the database and resets rollback state; refuses while open', async () => {
    const page = await newPage();
    await openIn(page, 'gone1', { autosaveMs: 0 });
    const blockedWhileOpen = await page.evaluate(async () => {
      const { SiafuDB } = await (window as any).siafuReady;
      try {
        await SiafuDB.destroy('gone1');
        return 'destroyed';
      } catch (e) {
        return (e as Error).name;
      }
    });
    expect(blockedWhileOpen).toBe('DatabaseLockedError');
    await page.evaluate(async () => {
      const { SiafuDB } = await (window as any).siafuReady;
      await (window as unknown as { db: import('/dist/index.js').SiafuDB }).db.close();
      await SiafuDB.destroy('gone1');
    });
    const fresh = await openIn(page, 'gone1');
    expect(isFailure(fresh)).toBe(false);
    if (!isFailure(fresh)) expect(fresh.restored).toBe(false);
    await page.close();
  });
});
