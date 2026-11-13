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
- `db.sync.*`: device-to-device sync, see [Sync](#sync).

One tab may have a given database open at a time (Web Locks). Others wait (`lock: 'wait'`, optionally
bounded) or fail fast with `DatabaseLockedError` (`lock: 'fail'`).

## Sync

Devices that share a **pairing code** form a sync group: each keeps its own full copy of the
database and they converge through a relay, including after working offline. The relay is the
reference `gsp-relay` server from the Graph Sync Protocol repository (`siafuDB/siafudb-gsp`); you
run it, behind TLS. The protocol is GSP sync v0: a draft, not audited.

```ts
// first device
const code = await db.sync.create({ relayUrl: 'https://relay.example.org' }); // 'gsp0-…' — show it as text or a QR code
// every other device
await db.sync.join({ relayUrl: 'https://relay.example.org', code });

const off = db.sync.onRemoteChange(({ changes }) => rerunQueries()); // another device's writes were applied
await db.sync.syncNow();              // optional: upload and download now, rejects if the relay is unreachable
const s = await db.sync.status();     // { enabled, running, connected, pendingUploads, devices, lastSyncAt, lastError, … }
```

- `db.sync.create({ relayUrl })` → pairing code; `db.sync.join({ relayUrl, code })`. Data already in the
  database is uploaded. After that sync runs by itself and resumes on the next `open()`; nothing to call.
- `db.sync.status()`, `db.sync.syncNow()`, `db.sync.onRemoteChange(listener)` (returns an unsubscribe function).
- `db.sync.stop()` / `db.sync.start()`: pause and resume talking to the relay (remembered across reloads).
  `db.sync.leave()`: forget the group on this device; the data stays. `db.sync.pairingCode()`: the code again,
  to add a device.
- Writes are uploaded shortly after they commit. Writes made offline or while stopped are queued in the
  database itself, survive reloads, and go out when the relay is reachable again (retries back off up to 30 s).
- Conflicts merge per entity: the later write wins per property and per label, a delete beats any other
  change to the same node or edge, and deleting a node removes its edges on every device.
- Synced nodes and edges carry a reserved property `_gid` (their id across devices). Do not write it.

**What the relay sees.** Everything sent is encrypted and signed on the device with keys derived from the
pairing code. The relay stores ciphertext and hands it back. It can see: a random id for the group, how many
items there are, their sizes and timing, and client network addresses. It cannot see: node or edge contents,
labels, property names or values, which device wrote what, or the pairing code. It can withhold or delay
items; it cannot forge or alter them unnoticed.

**Limits of v0.**

- The pairing code is the only secret, and it does not change. Anyone who has it is a full member of the
  group: there are no per-device permissions, and a device cannot be removed. To exclude a device, create a
  new group and re-pair the others. The code is stored in the (encrypted) database on each device.
- Deleted items are remembered forever so that deletes win, and so is merge bookkeeping per property: the
  sync state only grows, on the devices and on the relay.
- A new device downloads the group's whole history from the relay; there is no snapshot transfer.
- The protocol adds no transport security of its own: use `https://` for the relay.
- The first device to upload to a relay claims the group's id there.
- One write (one op) can carry at most 64 KiB. A larger write is split; a single property value over that
  size is not synced and is counted in `status().notUploaded`.
- A well-formed pairing code for a group that does not exist joins a new, empty group: there is no error.

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
npm test                       # vitest on Node 24 (crypto, rollback, tamper, sync transport)
npm run test:e2e               # headless Chromium via Playwright (OPFS, IDB fallback, reload, two tabs, sync)
```

The sync e2e test starts a real relay. Build `gsp-relay` from the protocol repository
(`cargo build -p gsp-relay --release`) and set `GSP_RELAY_BIN` to the binary; without it that one suite
is skipped with a warning.
