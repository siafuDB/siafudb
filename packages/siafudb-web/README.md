# @siafudb/web

SiafuDB in the browser: the Rust graph engine compiled to WebAssembly, running in a
dedicated Worker, persisted as an **encrypted** snapshot in OPFS (preferred) or
IndexedDB. Requires Node >= 24 for tooling and a secure context (https or localhost).

## Quickstart (plain Vite page)

```ts
// main.ts
import { SiafuDB } from '@siafudb/web';

const db = await SiafuDB.open('notes');                       // OPFS if available, else IndexedDB
await db.execute('CREATE (:Note {title: $t})', { t: `it's "quoted" \\ safe` }); // always bind parameters
const { columns, rows } = await db.query('MATCH (n:Note) RETURN n.title');
console.log(db.backend, db.persisted, rows);                  // 'opfs' true|false [["it's ..."]]
await db.flush();                                             // optional: autosave runs 500 ms after writes
```

```ts
// vite.config.ts  (the worker + wasm are bundled via `new URL(..., import.meta.url)`)
import { defineConfig } from 'vite';
export default defineConfig({ build: { target: 'esnext' }, worker: { format: 'es' }, optimizeDeps: { exclude: ['@siafudb/web'] } });
```

## Quickstart (Astro island)

Run it client-only, never during SSR: `<Notes client:only="react" />` (or any framework island),
or a plain `<script>` in an `.astro` page. Put the same Vite settings under `vite:` in `astro.config.mjs`
(`vite: { build: { target: 'esnext' }, worker: { format: 'es' }, optimizeDeps: { exclude: ['@siafudb/web'] } }`).
The Astro integration is not covered by this package's tests; the Vite recipe above is verified.

## API

- `SiafuDB.open(name, { backend, autosaveMs, lock, lockTimeoutMs, workerUrl, wasmUrl, onAutosaveError, onSaved })`
- `db.execute(query, params?)` / `db.query(query, params?)`: parameters are bound (`$name`), never interpolated.
- `db.changesSince(seq)`: engine change-log entries (query, bound params, timestamp).
- `db.flush()`, `db.close()`: flush encrypts and writes now; close also releases the cross-tab lock.
- `db.persisted` (result of `navigator.storage.persist()`), `db.storage()` / `SiafuDB.estimate()` (quota/usage).
- `SiafuDB.destroy(name)`: delete snapshots, key and rollback counter.

One tab may have a given database open at a time (Web Locks). Others wait (`lock: 'wait'`, optionally
bounded) or fail fast with `DatabaseLockedError` (`lock: 'fail'`).

## Encryption and storage format

A non-extractable AES-GCM key-encryption key lives in IndexedDB and wraps a random 256-bit data key per
database. Each save is `SFDB` header (format version, cipher-suite id, write counter) + random 96-bit nonce +
AES-256-GCM ciphertext; AAD binds database name, version, suite and counter. Cipher-suite ids: `1` XChaCha20-Poly1305
(planned default for the Rust-side store; recognised, refused here), `2` AES-256-GCM (implemented). The highest
counter is also recorded outside the snapshot store, so replaying an older valid file throws `RollbackError`;
modified bytes throw `TamperError`. This protects data at rest; code running in your origin can still use the key.

## Develop

```
export PATH=/opt/node24/bin:$PATH
npm install && npm run build   # builds the wasm (needs rustc + wasm32 target + wasm-bindgen-cli 0.2.130), then tsc
npm test                       # vitest on Node 24 (crypto, rollback, tamper)
npm run test:e2e               # headless Chromium via Playwright (OPFS, IDB fallback, reload, two tabs)
```
