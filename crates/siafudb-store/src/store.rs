//! The [`PageStore`] trait and its shared types.

use std::sync::Arc;

use zeroize::Zeroizing;

use crate::Result;

/// Default logical page size. Encrypted physical pages are
/// `PAGE_SIZE + EncryptedPageStore::OVERHEAD` bytes.
pub const PAGE_SIZE: usize = 4096;

/// Page number (0-based).
pub type PageNo = u64;

/// Storage errors. Every backend maps its native failures onto these.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// I/O failure in the backend (message from the OS or the host).
    #[error("io: {0}")]
    Io(String),
    /// Read of a page at or beyond `page_count`.
    #[error("page {0} out of range")]
    OutOfRange(PageNo),
    /// Authentication failed on a page: it was modified, torn, or never
    /// written. No plaintext is returned.
    #[error("integrity check failed on page {0}")]
    Integrity(PageNo),
    /// Content authenticates page-by-page but does not match the committed
    /// root: an older (replayed) or foreign page was substituted, or the
    /// store as a whole is older than the caller's `min_seq`.
    #[error("rollback or substitution detected: {0}")]
    Rollback(String),
    /// Another writer holds the lock.
    #[error("store is locked by another writer")]
    Locked,
    /// Cipher suite id not in the registry, or not compiled in.
    #[error("unsupported cipher suite {0}")]
    UnsupportedSuite(u16),
    /// The key does not open this store (KEK unwrap or key-check failed).
    #[error("wrong key")]
    WrongKey,
    /// Backend out of space (browser quota, disk full).
    #[error("quota exceeded")]
    Quota,
    /// The store holds no container / key header yet.
    #[error("store is not initialized")]
    NotInitialized,
    /// `create` on a store that already holds data.
    #[error("store already initialized")]
    AlreadyInitialized,
    /// Structurally invalid metadata (bad magic, bad lengths, overlaps).
    #[error("corrupt store: {0}")]
    Corrupt(String),
    /// Caller error (wrong buffer size, name too long, ...).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// A commit failed earlier; the in-memory state is no longer trusted.
    /// Drop the container and reopen the store.
    #[error("container poisoned by an earlier failed commit; reopen it")]
    Poisoned,
    /// Cryptographic primitive failure (RNG unavailable, KDF parameters).
    #[error("crypto: {0}")]
    Crypto(String),
}

/// What a backend can promise. Callers surface these to users (for example
/// "your data may be cleared" when `evictable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreCaps {
    /// `sync()` really reaches durable media (false for memory and some IDB paths).
    pub durable_sync: bool,
    /// Another process/tab can be excluded via [`PageStore::lock`].
    pub exclusive_lock: bool,
    /// Backend may evict data without notice (browser without `persist()`).
    pub evictable: bool,
}

/// Lock mode for [`PageStore::lock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockMode {
    /// Many readers.
    Shared,
    /// One writer, no readers.
    Exclusive,
}

#[cfg(not(target_arch = "wasm32"))]
mod sendsync {
    /// `Send + Sync` on native targets; no bound on wasm32, where host (JS)
    /// handles are `!Send` and there are no threads.
    pub trait MaybeSendSync: Send + Sync {}
    impl<T: Send + Sync + ?Sized> MaybeSendSync for T {}
}
#[cfg(target_arch = "wasm32")]
mod sendsync {
    /// `Send + Sync` on native targets; no bound on wasm32, where host (JS)
    /// handles are `!Send` and there are no threads.
    pub trait MaybeSendSync {}
    impl<T: ?Sized> MaybeSendSync for T {}
}
pub use sendsync::MaybeSendSync;

/// Held lock. Dropping it releases the lock.
pub struct LockGuard {
    _inner: Option<Box<dyn MaybeSendSync>>,
}

impl LockGuard {
    /// A guard that holds nothing (backends without locking).
    pub fn none() -> Self {
        Self { _inner: None }
    }
    /// Wrap any value whose `Drop` releases the lock.
    pub fn new<T: MaybeSendSync + 'static>(inner: T) -> Self {
        Self {
            _inner: Some(Box::new(inner)),
        }
    }
}

impl core::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("LockGuard")
    }
}

/// A synchronous device of fixed-size pages.
///
/// Sync, not async: native, mobile, memory and OPFS-in-a-worker
/// (`FileSystemSyncAccessHandle`) are synchronous. IndexedDB is async-only;
/// its backend is a write-behind page map (in the host) behind this same
/// trait, with `sync()` as the flush barrier.
///
/// Contract:
/// - every buffer is exactly `page_size()` bytes;
/// - `write_page` past the end extends the store (gap pages read as zeros);
/// - writes are buffered and become durable only at `sync()`; after a crash,
///   any subset of unsynced writes may have landed, possibly torn.
pub trait PageStore: MaybeSendSync {
    /// Size of every page in bytes.
    fn page_size(&self) -> usize;
    /// Backend capabilities.
    fn caps(&self) -> StoreCaps;
    /// Number of pages currently in the store.
    fn page_count(&self) -> Result<u64>;
    /// Read page `no` into `buf`. `OutOfRange` if `no >= page_count()`.
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()>;
    /// Write page `no`. Buffered; not durable until [`PageStore::sync`].
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()>;
    /// Set the store length to `pages` (shrinks, or grows with zero pages).
    fn truncate(&self, pages: u64) -> Result<()>;
    /// Durability barrier: fsync / flush SyncAccessHandle / commit IDB tx.
    fn sync(&self) -> Result<()>;
    /// Native: flock. Browser: Web Locks (host). Mobile: file lock in the
    /// app sandbox. Fails with `Locked` instead of blocking.
    fn lock(&self, mode: LockMode) -> Result<LockGuard>;
    /// Key for the container's keyed BLAKE3 commitments. Encrypting stores
    /// return a subkey of their data key; plain stores return `None` and the
    /// container falls back to a fixed public key (integrity, not secrecy).
    fn commitment_key(&self) -> Option<Zeroizing<[u8; 32]>> {
        None
    }
    /// Like [`PageStore::read_page`], and also return the per-write version
    /// the page was written with, authenticated together with its contents.
    /// Stores without versions return 0.
    fn read_page_versioned(&self, no: PageNo, buf: &mut [u8]) -> Result<u64> {
        self.read_page(no, buf).map(|()| 0)
    }
    /// Like [`PageStore::write_page`], and also return the version assigned
    /// to this write (0 for stores without versions). The container records
    /// these in its page-version vector.
    fn write_page_versioned(&self, no: PageNo, buf: &[u8]) -> Result<u64> {
        self.write_page(no, buf).map(|()| 0)
    }
    /// Versions up to `v` are known to exist; later writes must use larger
    /// ones. The container calls this at open with the committed maximum.
    fn observe_version(&self, _v: u64) {}
}

pub(crate) fn check_len(store_page: usize, got: usize) -> Result<()> {
    if store_page == got {
        Ok(())
    } else {
        Err(StoreError::InvalidArgument(format!(
            "buffer is {got} bytes, page size is {store_page}"
        )))
    }
}

macro_rules! forward_page_store {
    ($($ty:tt)*) => {
        impl<S: PageStore + ?Sized> PageStore for $($ty)*<S> {
            fn page_size(&self) -> usize { (**self).page_size() }
            fn caps(&self) -> StoreCaps { (**self).caps() }
            fn page_count(&self) -> Result<u64> { (**self).page_count() }
            fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()> { (**self).read_page(no, buf) }
            fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()> { (**self).write_page(no, buf) }
            fn truncate(&self, pages: u64) -> Result<()> { (**self).truncate(pages) }
            fn sync(&self) -> Result<()> { (**self).sync() }
            fn lock(&self, mode: LockMode) -> Result<LockGuard> { (**self).lock(mode) }
            fn commitment_key(&self) -> Option<Zeroizing<[u8; 32]>> { (**self).commitment_key() }
            fn read_page_versioned(&self, no: PageNo, buf: &mut [u8]) -> Result<u64> { (**self).read_page_versioned(no, buf) }
            fn write_page_versioned(&self, no: PageNo, buf: &[u8]) -> Result<u64> { (**self).write_page_versioned(no, buf) }
            fn observe_version(&self, v: u64) { (**self).observe_version(v) }
        }
    };
}
forward_page_store!(Arc);
forward_page_store!(Box);
