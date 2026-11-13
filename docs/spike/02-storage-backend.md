# 2. Is Grafeo's storage layer swappable?

**No at page level out of the box. Yes at snapshot level. And, since the
engine is now our fork of GrafeoDB 0.6.2
([ADR-0001](../adr/0001-fork-grafeo-and-freeze-upstream.md)), a page-level seam
is something we can build ourselves, with a real store contract to build it on.**
There is no virtual file layer under the file format. Persisting without one
is supported and fits the brief's "one storage layer, one change log"; the
page-level work is Phase 2 (below), and 0.6 makes it more feasible than the
0.5.43 analysis first concluded.

> **Update after the first draft.** This page was written against Grafeo
> 0.5.43. Two facts changed the picture: (1) the engine is now a fork frozen
> at 0.6.2 under `engine/`, so "needs upstream agreement" no longer applies;
> (2) 0.6 fixed encryption at rest and added a `GraphBackend` store contract.
> The table below is updated for 0.6.2; where 0.5.43 differed it says so.

## Evidence (engine fork, GrafeoDB 0.6.2 source)

| Seam                                                              | What it is                                                                                                                                                                                                                                                                                                                                                                                                                      | Usable for OPFS/IDB/encrypted pages?                                                                                                                                                                                   |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GrafeoFileManager` (`grafeo-storage`)                            | Single-file `.grafeo` format. Uses `std::fs`, `fs2` file locks, `memmap2`. About 50 `std::fs` call sites in engine, 31 in storage.                                                                                                                                                                                                                                                                                              | **No.** Hard-wired to the OS filesystem. Not compiled for wasm.                                                                                                                                                        |
| `AsyncStorageBackend`                                             | Documented extension point: `write_wal_batch`, `sync`, `write_snapshot`, `read_snapshot`, `list_snapshots`, `close`.                                                                                                                                                                                                                                                                                                            | **Not wired.** No code in `grafeo-engine` references it. Defining one has no effect today.                                                                                                                             |
| `PageFetcher`                                                     | Read-only byte-range fetch for sections; only impl is mmap.                                                                                                                                                                                                                                                                                                                                                                     | Read side only.                                                                                                                                                                                                        |
| `GrafeoDB::with_store(Arc<dyn GraphBackend>, Config)` (0.6)       | Replace the whole graph store. `GraphBackend` (grafeo-core, `graph/apply.rs`) is a documented contract: id reservation, `begin`, `apply(op, writer)`, commit and rollback, with the engine owning transactions, claims and edge-end integrity. The built-in row-group store implements it, and the repo ships a worked `ExampleBackend` test plus a durability test. In 0.5.43 this was `GraphStoreMut`, with no such contract. | **Yes, and the realistic seam.** A page-backed `GraphBackend` is the Phase 2 route. It is a graph-level contract, not a byte-level one, so it still means writing a storage engine, but against a specified interface. |
| `export_snapshot()` / `import_snapshot(&[u8])`                    | Versioned (snapshot v4 in 0.5, v5 in 0.6) bincode blob of the whole database, including schema and indexes. Works on wasm (tested, and now in Chromium, see [01](01-wasm.md#browser-measurements)).                                                                                                                                                                                                                             | **Yes.** Doc comment says "e.g. in IndexedDB".                                                                                                                                                                         |
| `encryption` feature (`grafeo`, `grafeo-engine`, `grafeo-common`) | AES-256-GCM, Argon2id or raw root key, HKDF-derived per-component DEKs, counter nonces, AAD binds to location. **Real in 0.6**, see below.                                                                                                                                                                                                                                                                                      | Partly. See below.                                                                                                                                                                                                     |

### Encryption at rest: 0.5.x did nothing, 0.6 does something

- **In 0.5.x, encryption at rest silently did nothing.** The 0.6 CHANGELOG
  lists it under "Data that 0.5.x silently dropped": "encryption at rest did
  nothing, and now encrypts the file and its WAL (an open without the right
  key fails ...)" ([upstream CHANGELOG](https://github.com/GrafeoDB/grafeo/blob/main/CHANGELOG.md)).
  The first draft of this page treated the 0.5.43 `encryption` feature as a
  working primitive; it was not, and nothing built on it would have been
  encrypted. That is why SiafuDB's own `EncryptedPageStore` (below) was the
  right call, and why we test ciphertext rather than trust a feature flag
  (the browser probe asserts the secret is absent from the stored bytes).
- **0.6 has a real `encryption` feature on the `grafeo` facade crate**
  (declared in `engine/grafeo/Cargo.toml`, enabling the `encryption` feature
  of `grafeo-engine` and `grafeo-common`), so SiafuDB can enable it without
  depending on `grafeo-engine` directly. The 0.5.43 gap is closed.
- Still limiting, checked in the 0.6.2 source: the cipher is AES-256-GCM with
  no cipher id (no XChaCha20, no agility), and `KeyProvider::provide_root_key`
  returns raw 32 bytes, which cannot represent a Secure Enclave, Android
  Keystore or non-extractable WebCrypto key. Both are now ours to change
  (recorded in `engine/CHANGES.md` when we do).
- It encrypts the file's sections and WAL. It does not help the snapshot path
  or the browser, since those never touch `GrafeoFileManager`. Per the
  CHANGELOG it also fails with a spill path, `TierOverride::ForceDisk` or no
  path, and a migrated database's `.pre-0.6` copy stays unencrypted.

## Recommendation: two phases

**Phase 1 (now, no engine change): encrypted snapshot + signed log.**

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

**Phase 2 (only if a real graph outgrows memory): page-level storage in the
engine.** Two routes, both now ours to take because the engine is our fork:

1. **A page-backed `GraphBackend`** behind `GrafeoDB::with_store`, on top of
   `PageStore`. 0.6 specifies this contract (see the table), so it can be
   built without touching the engine's transaction code. This is the
   preferred route.
2. **A `Vfs`/block-device trait under `GrafeoFileManager`** replacing
   `std::fs`, `fs2` and `memmap2`, plus feature-gating those out of the
   `browser` profile. This touches the WAL and section code (about 8.5k lines
   in `grafeo-storage`).

**Revised effort: roughly 4-8 weeks, down from 6-10+.** The upstream
negotiation is gone (we own the code), route 1 has a contract and a worked
example to follow, and the merge debt a fork usually carries is zero by policy
(ADR-0001: no upstream tracking). It is still a guess from reading the code,
nothing has been prototyped, and the uncertainty is about how much of the
engine assumes the in-memory store (indexes, vector and text search, spill).
**Do not start this until Phase 1 shows a concrete limit**; ADR-0002 schedules
it in Phase 3 "if memory-bound".

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
- Phase 2: about 4-8 weeks (revised down from 6-10+ now that we own the engine and 0.6 has the `GraphBackend` contract), high uncertainty, no upstream dependency.

Not tested: any of the proposed trait. This section is a design.
