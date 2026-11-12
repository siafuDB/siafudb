# 2. Is Grafeo's storage layer swappable?

**No at page level. Yes at snapshot level.** There is no virtual file layer
under Grafeo's file format. There is a supported way to persist without one, and
it fits the brief's "one storage layer, one change log" better than a page-level
fork would today.

## Evidence (Grafeo 0.5.43 source)

| Seam                                           | What it is                                                                                                                          | Usable for OPFS/IDB/encrypted pages?                                                       |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `GrafeoFileManager` (`grafeo-storage`)         | Single-file `.grafeo` format. Uses `std::fs`, `fs2` file locks, `memmap2`. About 50 `std::fs` call sites in engine, 31 in storage.  | **No.** Hard-wired to the OS filesystem. Not compiled for wasm.                            |
| `AsyncStorageBackend`                          | Documented extension point: `write_wal_batch`, `sync`, `write_snapshot`, `read_snapshot`, `list_snapshots`, `close`.                | **Not wired.** No code in `grafeo-engine` references it. Defining one has no effect today. |
| `PageFetcher`                                  | Read-only byte-range fetch for sections; only impl is mmap.                                                                         | Read side only.                                                                            |
| `GrafeoDB::with_store(Arc<dyn GraphStoreMut>)` | Replace the whole graph store.                                                                                                      | Technically yes, but you reimplement the storage engine. Not "smallest change".            |
| `export_snapshot()` / `import_snapshot(&[u8])` | Versioned (snapshot v4 in 0.5, v5 in 0.6) bincode blob of the whole database, including schema and indexes. Works on wasm (tested). | **Yes.** Doc comment says "e.g. in IndexedDB".                                             |
| `encryption` feature (`grafeo-engine`)         | AES-256-GCM, Argon2id or raw root key, HKDF-derived per-component DEKs, counter nonces, AAD binds to location.                      | Partly. See below.                                                                         |

Encryption limits found in 0.5.43:

- The `encryption` feature exists on `grafeo-engine`, **not on the `grafeo`
  facade crate** that SiafuDB depends on, so SiafuDB cannot switch it on without
  depending on `grafeo-engine` directly.
- Cipher is hard-wired to AES-256-GCM. The only version tag is the HKDF salt
  (`grafeo-hkdf-v1`). There is no cipher id, so no XChaCha20 and no agility.
- `KeyProvider::provide_root_key` returns raw 32 bytes. That cannot represent a
  Secure Enclave, Android Keystore or non-extractable WebCrypto key.
- It encrypts the file's sections and WAL. It does not help the snapshot path
  or the browser at all, since those never touch `GrafeoFileManager`.

## Recommendation: two phases

**Phase 1 (now, no upstream change): encrypted snapshot + signed log.**

- Run Grafeo in memory on every target. Persist through a SiafuDB-owned
  `PageStore` (below).
- Durability = last snapshot + append-only log of signed mutations (03). The
  log is the thing sync needs anyway, so it does double duty: one change log,
  one storage layer.
- Checkpoint (snapshot) on a size/time threshold. Cost is O(database) per
  checkpoint and the whole graph must fit in RAM. For personal fragments
  (phone-sized data, the 64 MB mobile profile) this is acceptable; grafeo-web
  ships the same model.
- Encryption lives in `EncryptedPageStore`, so every target (native, mobile,
  OPFS, IndexedDB, memory) encrypts identically and Grafeo never sees a key.

**Phase 2 (only if a real graph outgrows memory): page-level storage in
Grafeo.** The smallest upstream change is a `Vfs`/block-device trait under
`GrafeoFileManager` replacing `std::fs`, `fs2` and `memmap2`, plus feature-gating
those out of the `browser` profile. Roughly 6-10+ weeks including upstream
negotiation, and it touches the WAL and section code (about 8.5k lines in
`grafeo-storage`). Grafeo's own design notes (D1) mention a future explicit
pager, so upstream may be receptive, but I have not asked them. A fork carries
the same cost plus permanent merge debt. **Do not start this until Phase 1
shows a concrete limit.**

The trait is designed so Phase 2 slots in without changing it.

## Proposed signature (for sign-off; not implemented)

Design constraints that shaped it:

- **Sync, not async.** Native, mobile, memory and OPFS-in-a-worker
  (`FileSystemSyncAccessHandle`) are synchronous. IndexedDB is async-only, so
  its backend is a write-behind page map behind the same sync trait, with
  `sync()` as the flush barrier.
- **WebCrypto is async-only**, so non-extractable keys cannot sit on the hot
  path of a sync page cipher. Resolution: a non-extractable key-encryption key
  (KEK) in Secure Enclave / Keystore / WebCrypto unwraps a data key (DEK)
  **once at open**; pages then use a sync Rust AEAD. Honest cost: the DEK is in
  process memory during a session. An attacker with code execution in the origin
  or app can read it; the non-extractable KEK protects the key at rest, not in
  use.

```rust
/// Logical page. Ciphertext pages are PAGE_SIZE + EncryptedPageStore::OVERHEAD.
pub const PAGE_SIZE: usize = 4096;
pub type PageNo = u64;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")] Io(String),
    #[error("page {0} out of range")] OutOfRange(PageNo),
    #[error("integrity check failed on page {0}")] Integrity(PageNo),
    #[error("store is locked by another writer")] Locked,
    #[error("unsupported cipher suite {0}")] UnsupportedSuite(u16),
    #[error("quota exceeded")] Quota,
}

#[derive(Clone, Copy, Debug)]
pub struct StoreCaps {
    /// `sync()` really reaches durable media (false for some IDB/memory paths).
    pub durable_sync: bool,
    /// Another process/tab can be excluded via `lock`.
    pub exclusive_lock: bool,
    /// Backend may evict data without notice (browser without persist()).
    pub evictable: bool,
}

pub enum LockMode { Shared, Exclusive }

pub trait PageStore: Send + Sync {
    fn caps(&self) -> StoreCaps;
    fn page_count(&self) -> Result<u64, StoreError>;
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<(), StoreError>;
    /// Buffered. Not durable until `sync`.
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<(), StoreError>;
    fn truncate(&self, pages: u64) -> Result<(), StoreError>;
    /// Durability barrier: fsync / flush SyncAccessHandle / commit IDB tx.
    fn sync(&self) -> Result<(), StoreError>;
    /// Native: flock. Browser: Web Locks. Mobile: file lock in app sandbox.
    fn lock(&self, mode: LockMode) -> Result<Box<dyn Send>, StoreError>;
}

/// Registry ids, never strings. u16 so they fit a page header.
#[repr(u16)]
pub enum AeadSuite { XChaCha20Poly1305 = 1, Aes256Gcm = 2 }

/// Wraps and unwraps the data key. Implementations: Secure Enclave,
/// Android Keystore, WebCrypto non-extractable (unwrapped by host JS, then
/// handed in as a `StaticKek`), passphrase (Argon2id), raw key (tests).
pub trait KekProvider: Send + Sync {
    fn kek_id(&self) -> &str;                       // e.g. "secure-enclave/v1"
    fn wrap(&self, dek: &Dek) -> Result<WrappedDek, StoreError>;
    fn unwrap(&self, w: &WrappedDek) -> Result<Dek, StoreError>;
}

/// Encrypts every page identically on every target.
/// Page layout: [suite: u16][epoch: u32][nonce: 24][ciphertext][tag: 16].
/// AAD = db_id || page_no || epoch  (binds ciphertext to its location, so
/// pages cannot be moved or replayed across slots).
pub struct EncryptedPageStore<S: PageStore> { /* inner: S, dek, suite, epoch */ }
impl<S: PageStore> PageStore for EncryptedPageStore<S> { /* ... */ }

// Backends (each its own module/feature):
//   FilePageStore       native + iOS/Android (app sandbox), flock
//   OpfsPageStore       wasm, dedicated worker, FileSystemSyncAccessHandle
//   IdbPageStore        wasm fallback, write-behind page map, async flusher
//   MemPageStore        tests
```

Above the trait sits a small container (not shown): a double-buffered
superblock in pages 0 and 1 for atomic checkpoints, then snapshot extents and
the log. Crash-atomicity of that container is the main correctness work in
Phase 1.

Browser notes from the brief: call `navigator.storage.persist()` and record the
result in `StoreCaps.evictable`; surface "your data may be cleared" in the app
when it returns false. OPFS sync handles need a **dedicated worker**, so the
engine and `OpfsPageStore` both run in that worker.

## Effort

- Phase 1: 2-3 weeks. Container + superblock + crash tests (1 week), file/mem
  backends and `EncryptedPageStore` (4-5 days), OPFS + IDB backends with
  browser tests (1 week).
- Phase 2: 6-10+ weeks, high uncertainty, needs upstream agreement.

Not tested: any of the proposed trait. This section is a design.
