//! In-memory page store (tests, ephemeral databases).

use std::sync::{Arc, Mutex, MutexGuard};

use crate::store::{LockGuard, LockMode, PageNo, PageStore, StoreCaps, StoreError, check_len};
use crate::Result;

#[derive(Default)]
struct LockState {
    shared: usize,
    exclusive: bool,
}

/// Pages in a `Vec<u8>`. `sync` is a no-op. `lock` is enforced between
/// handles that share the store (clones / `Arc`), not across processes.
#[derive(Clone)]
pub struct MemPageStore {
    page_size: usize,
    data: Arc<Mutex<Vec<u8>>>,
    locks: Arc<Mutex<LockState>>,
}

struct MemLock {
    locks: Arc<Mutex<LockState>>,
    mode: LockMode,
}

impl Drop for MemLock {
    fn drop(&mut self) {
        let mut l = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        match self.mode {
            LockMode::Shared => l.shared -= 1,
            LockMode::Exclusive => l.exclusive = false,
        }
    }
}

impl MemPageStore {
    /// Empty store of `page_size`-byte pages.
    pub fn new(page_size: usize) -> Self {
        Self::from_bytes(page_size, Vec::new()).expect("empty is aligned")
    }

    /// Store over existing raw bytes (length must be a multiple of `page_size`).
    pub fn from_bytes(page_size: usize, bytes: Vec<u8>) -> Result<Self> {
        if page_size == 0 || bytes.len() % page_size != 0 {
            return Err(StoreError::InvalidArgument(
                "length is not a multiple of the page size".into(),
            ));
        }
        Ok(Self {
            page_size,
            data: Arc::new(Mutex::new(bytes)),
            locks: Arc::default(),
        })
    }

    /// Copy of all raw bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.data().clone()
    }

    /// Independent deep copy (fresh lock state).
    pub fn deep_clone(&self) -> Self {
        Self::from_bytes(self.page_size, self.to_bytes()).expect("aligned")
    }

    /// Mutate raw bytes directly (tamper tests).
    pub fn with_bytes_mut<R>(&self, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
        f(&mut self.data())
    }

    fn data(&self) -> MutexGuard<'_, Vec<u8>> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl PageStore for MemPageStore {
    fn page_size(&self) -> usize {
        self.page_size
    }
    fn caps(&self) -> StoreCaps {
        StoreCaps {
            durable_sync: false,
            exclusive_lock: true,
            evictable: true,
        }
    }
    fn page_count(&self) -> Result<u64> {
        Ok((self.data().len() / self.page_size) as u64)
    }
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        let d = self.data();
        let start = usize::try_from(no)
            .ok()
            .and_then(|n| n.checked_mul(self.page_size))
            .filter(|s| s + self.page_size <= d.len())
            .ok_or(StoreError::OutOfRange(no))?;
        buf.copy_from_slice(&d[start..start + self.page_size]);
        Ok(())
    }
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        let start = usize::try_from(no)
            .ok()
            .and_then(|n| n.checked_mul(self.page_size))
            .ok_or(StoreError::Quota)?;
        let mut d = self.data();
        if d.len() < start + self.page_size {
            d.resize(start + self.page_size, 0);
        }
        d[start..start + self.page_size].copy_from_slice(buf);
        Ok(())
    }
    fn truncate(&self, pages: u64) -> Result<()> {
        let len = usize::try_from(pages)
            .ok()
            .and_then(|n| n.checked_mul(self.page_size))
            .ok_or(StoreError::Quota)?;
        self.data().resize(len, 0);
        Ok(())
    }
    fn sync(&self) -> Result<()> {
        Ok(())
    }
    fn lock(&self, mode: LockMode) -> Result<LockGuard> {
        let mut l = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        let ok = match mode {
            LockMode::Shared => !l.exclusive,
            LockMode::Exclusive => !l.exclusive && l.shared == 0,
        };
        if !ok {
            return Err(StoreError::Locked);
        }
        match mode {
            LockMode::Shared => l.shared += 1,
            LockMode::Exclusive => l.exclusive = true,
        }
        Ok(LockGuard::new(MemLock {
            locks: self.locks.clone(),
            mode,
        }))
    }
}
