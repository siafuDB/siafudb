//! A page store whose I/O is supplied by the host as closures.
//!
//! This is how OPFS (`FileSystemSyncAccessHandle` in a dedicated worker) and
//! IndexedDB (write-behind page map) backends live in TypeScript without a
//! second Rust implementation: the wasm layer wraps JS functions in these
//! closures. Encryption stays in Rust ([`crate::EncryptedPageStore`] on top),
//! so the host only ever sees ciphertext pages of a fixed size.
//!
//! On wasm32 the closures need not be `Send`/`Sync` (JS handles are not); on
//! native targets they must be, so the same type also serves UniFFI hosts.

use crate::Result;
use crate::store::{LockGuard, LockMode, PageNo, PageStore, StoreCaps, StoreError, check_len};

#[cfg(not(target_arch = "wasm32"))]
mod fns {
    use super::*;
    pub type ReadFn = Box<dyn Fn(PageNo, &mut [u8]) -> Result<()> + Send + Sync>;
    pub type WriteFn = Box<dyn Fn(PageNo, &[u8]) -> Result<()> + Send + Sync>;
    pub type CountFn = Box<dyn Fn() -> Result<u64> + Send + Sync>;
    pub type TruncateFn = Box<dyn Fn(u64) -> Result<()> + Send + Sync>;
    pub type SyncFn = Box<dyn Fn() -> Result<()> + Send + Sync>;
    pub type LockFn = Box<dyn Fn(LockMode) -> Result<LockGuard> + Send + Sync>;
}
#[cfg(target_arch = "wasm32")]
mod fns {
    use super::*;
    pub type ReadFn = Box<dyn Fn(PageNo, &mut [u8]) -> Result<()>>;
    pub type WriteFn = Box<dyn Fn(PageNo, &[u8]) -> Result<()>>;
    pub type CountFn = Box<dyn Fn() -> Result<u64>>;
    pub type TruncateFn = Box<dyn Fn(u64) -> Result<()>>;
    pub type SyncFn = Box<dyn Fn() -> Result<()>>;
    pub type LockFn = Box<dyn Fn(LockMode) -> Result<LockGuard>>;
}
use fns::*;

/// Host-backed page store. Build with [`CallbackPageStore::builder`].
pub struct CallbackPageStore {
    page_size: usize,
    caps: StoreCaps,
    read: ReadFn,
    write: WriteFn,
    count: CountFn,
    truncate: TruncateFn,
    sync: SyncFn,
    lock: Option<LockFn>,
}

/// Builder for [`CallbackPageStore`]. `read`, `write` and `page_count` are
/// required; `truncate` defaults to an error, `sync` to a no-op (set
/// `caps.durable_sync` accordingly), `lock` to no locking.
pub struct CallbackPageStoreBuilder {
    page_size: usize,
    caps: StoreCaps,
    read: Option<ReadFn>,
    write: Option<WriteFn>,
    count: Option<CountFn>,
    truncate: Option<TruncateFn>,
    sync: Option<SyncFn>,
    lock: Option<LockFn>,
}

impl CallbackPageStore {
    /// Start a builder for pages of `page_size` bytes. For an encrypted store
    /// that is the *physical* size, `EncryptedPageStore::physical_page_size`.
    pub fn builder(page_size: usize) -> CallbackPageStoreBuilder {
        CallbackPageStoreBuilder {
            page_size,
            caps: StoreCaps {
                durable_sync: false,
                exclusive_lock: false,
                evictable: true,
            },
            read: None,
            write: None,
            count: None,
            truncate: None,
            sync: None,
            lock: None,
        }
    }
}

impl CallbackPageStoreBuilder {
    /// Capabilities the host can promise (record `navigator.storage.persist()`
    /// in `evictable`).
    pub fn caps(mut self, caps: StoreCaps) -> Self {
        self.caps = caps;
        self
    }
    /// Page read.
    pub fn read(
        mut self,
        f: impl Fn(PageNo, &mut [u8]) -> Result<()> + crate::MaybeSendSync + 'static,
    ) -> Self {
        self.read = Some(Box::new(f));
        self
    }
    /// Page write.
    pub fn write(
        mut self,
        f: impl Fn(PageNo, &[u8]) -> Result<()> + crate::MaybeSendSync + 'static,
    ) -> Self {
        self.write = Some(Box::new(f));
        self
    }
    /// Page count.
    pub fn page_count(
        mut self,
        f: impl Fn() -> Result<u64> + crate::MaybeSendSync + 'static,
    ) -> Self {
        self.count = Some(Box::new(f));
        self
    }
    /// Truncate.
    pub fn truncate(
        mut self,
        f: impl Fn(u64) -> Result<()> + crate::MaybeSendSync + 'static,
    ) -> Self {
        self.truncate = Some(Box::new(f));
        self
    }
    /// Sync barrier.
    pub fn sync(mut self, f: impl Fn() -> Result<()> + crate::MaybeSendSync + 'static) -> Self {
        self.sync = Some(Box::new(f));
        self
    }
    /// Lock.
    pub fn lock(
        mut self,
        f: impl Fn(LockMode) -> Result<LockGuard> + crate::MaybeSendSync + 'static,
    ) -> Self {
        self.lock = Some(Box::new(f));
        self
    }
    /// Finish. Errors if `read`, `write` or `page_count` is missing.
    pub fn build(self) -> Result<CallbackPageStore> {
        let missing =
            |n: &str| StoreError::InvalidArgument(format!("callback store: `{n}` not set"));
        if self.page_size == 0 {
            return Err(StoreError::InvalidArgument("page size 0".into()));
        }
        Ok(CallbackPageStore {
            page_size: self.page_size,
            caps: self.caps,
            read: self.read.ok_or_else(|| missing("read"))?,
            write: self.write.ok_or_else(|| missing("write"))?,
            count: self.count.ok_or_else(|| missing("page_count"))?,
            truncate: self.truncate.unwrap_or_else(|| {
                Box::new(|_| {
                    Err(StoreError::Io(
                        "host store does not support truncate".into(),
                    ))
                })
            }),
            sync: self.sync.unwrap_or_else(|| Box::new(|| Ok(()))),
            lock: self.lock,
        })
    }
}

impl PageStore for CallbackPageStore {
    fn page_size(&self) -> usize {
        self.page_size
    }
    fn caps(&self) -> StoreCaps {
        self.caps
    }
    fn page_count(&self) -> Result<u64> {
        (self.count)()
    }
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        if no >= (self.count)()? {
            return Err(StoreError::OutOfRange(no));
        }
        (self.read)(no, buf)
    }
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        (self.write)(no, buf)
    }
    fn truncate(&self, pages: u64) -> Result<()> {
        (self.truncate)(pages)
    }
    fn sync(&self) -> Result<()> {
        (self.sync)()
    }
    fn lock(&self, mode: LockMode) -> Result<LockGuard> {
        match &self.lock {
            Some(f) => f(mode),
            None => Ok(LockGuard::none()),
        }
    }
}
