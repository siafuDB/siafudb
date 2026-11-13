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
const s = await db.sync.status();     // { enabled, running, connected, pendingUploads, catchingUp, needsRejoin, deviceList, … }
```

- `db.sync.create({ relayUrl, registrationToken? })` → pairing code; `db.sync.join({ relayUrl, code })`. Data
  already in the database is uploaded. After that sync runs by itself and resumes on the next `open()`.
  `relayUrl` must be `https://`; plain `http://` is accepted for `localhost`, `127.0.0.1` and `[::1]` only.
- `registrationToken`: for a relay that only accepts new groups from holders of its registration secret.
  Only `create` needs it (joining never registers); without it such a relay makes `create` reject and no group
  is made. It is kept in memory, never stored or logged; if the group still has to be registered after a reload,
  `status().lastError` says so and `db.sync.start({ registrationToken })` supplies it again.
- `db.sync.status()`, `db.sync.syncNow()`, `db.sync.onRemoteChange(listener)` (returns an unsubscribe function).
- `db.sync.stop()` / `db.sync.start()`: pause and resume talking to the relay (remembered across reloads).
  `db.sync.leave()`: forget the group on this device; the data stays. `db.sync.pairingCode()`: the code again,
  to add a device.
- Writes are uploaded shortly after they commit. Writes made offline or while stopped are queued in the
  database itself, survive reloads, and go out when the relay is reachable again (retries back off up to 30 s).
- Conflicts merge per entity: the later write wins per property and per label, a delete beats any other
  change to the same node or edge, and deleting a node removes its edges on every device.
- Synced nodes and edges carry a reserved property `_gid` (their id across devices). Do not write it.

**When something is wrong**, `status()` says what:

- **A device restored from an older copy** (a backup, a browser profile copied back) is behind what it had
  itself uploaded. If it can read the relay before it uploads anything new, it simply gets its own later
  writes back and carries on as the same device; nothing needs to be done. To make that the normal case,
  after every `open()` local writes are held from upload until the relay has been read once
  (`status().catchingUp`); they are in the local database at once and are uploaded right after. If the relay
  cannot be reached (offline, relay down, a 15 s timeout) or sync is stopped, they are not held: they are
  queued for upload as soon as that is known.
- `needsRejoin`: the remaining case. The restored device queued a write for upload before it could read the
  relay (it was offline or stopped), and that write now conflicts with its own earlier uploads. It keeps
  receiving, and local writes keep working, but nothing is uploaded until you call
  `db.sync.rejoinAsNewDevice()`. That gives the device a new identity; everything it wrote in the meantime is
  then uploaded and counts as written at that moment. It is never done automatically. The same call is the
  way out if `catchingUp` stays true while `connected` (the relay has lost part of this device's history).
- `deviceList`: one entry per device, `{ id, own, appliedSeq, pending, gap, clockLeadMs, equivocated }`.
  `clockLeadMs > 0` means that device's writes were ahead of this device's clock: held if under 24 hours,
  refused if over (one of the two clocks is wrong). `gap` means it is stuck until a resync; `equivocated`
  means its storage was rolled back at some point.
- `relayResets`: how often the relay was found to have lost its data. Devices that were up to date carry on;
  what only the relay held is gone, so a device that joins afterwards cannot read the others' later writes
  (they show as `pendingDownloads`). Create a new group to recover.
- `needsResync`: too many received changes were waiting; the relay's log is read again, with growing delays.
- `notUploaded` / `notApplied` / `lastProblem`: changes that could not be sent or applied, and why.
- Opening a database whose sync state was written by an older pre-release format fails with code
  `sync-state-version`. There is no migration.

**What the relay sees.** Everything sent is encrypted and signed on the device with keys derived from the
pairing code. The relay stores ciphertext and hands it back. It can see: a random id for the group, how many
items there are, their sizes and timing, and client network addresses. It cannot see: node or edge contents,
labels, property names or values, which device wrote what, or the pairing code. It can withhold or delay
items; it cannot forge or alter them unnoticed. The access token a device presents is derived for that one
relay's address, so a relay operator cannot use it on another relay.

**Limits of v0.**

- The pairing code is the only secret, and it does not change. Anyone who has it is a full member of the
  group: there are no per-device permissions, and a device cannot be removed. To exclude a device, create a
  new group and re-pair the others. The code is stored in the (encrypted) database on each device.
- Deleted items are remembered forever so that deletes win, and so is merge bookkeeping per property: the
  sync state only grows, on the devices and on the relay.
- A new device downloads the group's whole history from the relay; there is no snapshot transfer. A relay
  that loses its data loses that history for good.
- At most 64 devices per group. Each `rejoinAsNewDevice()` counts as one more.
- A device that was rolled back and uploaded before it could read the relay leaves a permanent trace: the other devices may
  disagree about a few of that device's own writes from around the rollback. Nobody else's data is affected.
- A device whose clock is more than 24 hours ahead cannot get its writes accepted until its clock is fixed;
  a device whose clock is far behind refuses everyone else's.
- The protocol adds no transport security of its own: hence `https://` only.
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

The sync e2e suite starts real relays. Build `gsp-relay` from the protocol repository at the commit pinned in
the workspace `Cargo.toml` (`cargo build -p gsp-relay --release`) and set `GSP_RELAY_BIN` to the binary.
Without it that suite is skipped with a warning locally, and fails in CI.
