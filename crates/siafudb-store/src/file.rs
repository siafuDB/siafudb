//! `std::fs` page store (desktop, server, iOS/Android app sandbox).

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::Result;
use crate::store::{LockGuard, LockMode, PageNo, PageStore, StoreCaps, StoreError, check_len};

/// One file, pages at `no * page_size`. `sync` is `fsync` (`sync_all`).
/// `lock` is an advisory OS file lock (`flock` on Unix, `LockFileEx` on
/// Windows) and fails with `Locked` instead of blocking.
pub struct FilePageStore {
    page_size: usize,
    path: PathBuf,
    file: Mutex<File>,
}

struct FileLock(File);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn io(e: std::io::Error) -> StoreError {
    if e.kind() == std::io::ErrorKind::StorageFull {
        StoreError::Quota
    } else {
        StoreError::Io(e.to_string())
    }
}

impl FilePageStore {
    /// Open `path`, creating it if missing. A new file's parent directory is
    /// fsynced so the directory entry survives a crash.
    pub fn open(path: impl AsRef<Path>, page_size: usize) -> Result<Self> {
        if page_size == 0 {
            return Err(StoreError::InvalidArgument("page size 0".into()));
        }
        let path = path.as_ref().to_path_buf();
        let existed = path.exists();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io)?;
        if !existed {
            file.sync_all().map_err(io)?;
            #[cfg(unix)]
            if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                File::open(dir).and_then(|d| d.sync_all()).map_err(io)?;
            }
        }
        Ok(Self {
            page_size,
            path,
            file: Mutex::new(file),
        })
    }

    /// Path of the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn file(&self) -> MutexGuard<'_, File> {
        self.file.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn offset(&self, no: PageNo) -> Result<u64> {
        no.checked_mul(self.page_size as u64)
            .ok_or(StoreError::OutOfRange(no))
    }
}

impl PageStore for FilePageStore {
    fn page_size(&self) -> usize {
        self.page_size
    }
    fn caps(&self) -> StoreCaps {
        StoreCaps {
            durable_sync: true,
            exclusive_lock: true,
            evictable: false,
        }
    }
    fn page_count(&self) -> Result<u64> {
        // A torn extension leaves a partial last page; it does not count.
        let len = self.file().metadata().map_err(io)?.len();
        Ok(len / self.page_size as u64)
    }
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        if no >= self.page_count()? {
            return Err(StoreError::OutOfRange(no));
        }
        let off = self.offset(no)?;
        let mut f = self.file();
        f.seek(SeekFrom::Start(off)).map_err(io)?;
        f.read_exact(buf).map_err(io)
    }
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        let off = self.offset(no)?;
        let mut f = self.file();
        f.seek(SeekFrom::Start(off)).map_err(io)?;
        f.write_all(buf).map_err(io)
    }
    fn truncate(&self, pages: u64) -> Result<()> {
        let len = self.offset(pages)?;
        self.file().set_len(len).map_err(io)
    }
    fn sync(&self) -> Result<()> {
        self.file().sync_all().map_err(io)
    }
    fn lock(&self, mode: LockMode) -> Result<LockGuard> {
        // Lock a dup of the handle: flock locks belong to the open file
        // description, which the dup shares, so the guard owns the lock.
        let f = self.file().try_clone().map_err(io)?;
        let r = match mode {
            LockMode::Shared => f.try_lock_shared(),
            LockMode::Exclusive => f.try_lock(),
        };
        match r {
            Ok(()) => Ok(LockGuard::new(FileLock(f))),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked),
            Err(TryLockError::Error(e)) => Err(io(e)),
        }
    }
}
