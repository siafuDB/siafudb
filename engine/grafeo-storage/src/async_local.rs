//! Built-in local filesystem async storage backend.
//!
//! Wraps [`AsyncLpgWal`] to provide a local implementation of
//! [`AsyncStorageBackend`]. This is the default backend used when the
//! `async-storage` feature is enabled.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use grafeo_common::utils::error::{Error, Result};

use super::async_backend::{AsyncStorageBackend, SnapshotMetadata};
use super::wal::AsyncLpgWal;

/// Local filesystem async storage backend.
///
/// Delegates WAL operations to [`AsyncLpgWal`] and provides snapshot
/// read/write via the filesystem. This is the built-in backend for
/// `grafeo-server` and other tokio-based applications.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> grafeo_common::utils::error::Result<()> {
/// use std::sync::Arc;
/// use grafeo_storage::async_local::AsyncLocalBackend;
/// use grafeo_storage::async_backend::AsyncStorageBackend;
/// use grafeo_storage::wal::AsyncLpgWal;
///
/// let wal = Arc::new(AsyncLpgWal::open("wal_dir").await?);
/// let backend = AsyncLocalBackend::new(wal);
/// assert_eq!(backend.name(), "local-async");
/// # Ok(())
/// # }
/// ```
pub struct AsyncLocalBackend {
    wal: Arc<AsyncLpgWal>,
}

impl AsyncLocalBackend {
    /// Creates a new local backend wrapping the given async WAL.
    #[must_use]
    pub fn new(wal: Arc<AsyncLpgWal>) -> Self {
        Self { wal }
    }

    /// Returns a reference to the underlying async WAL.
    #[must_use]
    pub fn wal(&self) -> &Arc<AsyncLpgWal> {
        &self.wal
    }
}

impl AsyncStorageBackend for AsyncLocalBackend {
    fn name(&self) -> &str {
        "local-async"
    }

    fn write_wal_batch<'a>(
        &'a self,
        records: &'a [Vec<u8>],
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            for record_data in records {
                // Each record is already serialized; write the raw frame.
                // force_sync is false: the caller should call sync() after a batch.
                self.wal.manager().write_frame(record_data, false).await?;
            }
            Ok(())
        })
    }

    fn sync(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { self.wal.sync().await })
    }

    fn write_snapshot<'a>(
        &'a self,
        _data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        // Snapshot writes for local storage are handled by the GrafeoFileManager
        // in the engine layer, not by the WAL backend. This method is a no-op
        // for local storage but is meaningful for remote backends (S3, Postgres).
        Box::pin(async {
            Err(Error::Internal(
                "local backend: use GrafeoDB::async_write_snapshot() instead".to_string(),
            ))
        })
    }

    fn read_snapshot(&self) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>>> + Send + '_>> {
        // Same as write_snapshot: local snapshots are managed by GrafeoFileManager.
        Box::pin(async {
            Err(Error::Internal(
                "local backend: use GrafeoDB::open() for snapshot recovery".to_string(),
            ))
        })
    }

    fn list_snapshots(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SnapshotMetadata>>> + Send + '_>> {
        Box::pin(async { Ok(vec![]) })
    }

    fn close(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async {
            self.wal.flush().await?;
            self.wal.sync().await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::{AsyncTypedWal, WalRecord};
    use grafeo_common::types::NodeId;

    #[tokio::test]
    async fn local_backend_name() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Arc::new(AsyncTypedWal::open(dir.path()).await.unwrap());
        let backend = AsyncLocalBackend::new(wal);
        assert_eq!(backend.name(), "local-async");
    }

    #[tokio::test]
    async fn local_backend_write_wal_batch() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Arc::new(AsyncTypedWal::open(dir.path()).await.unwrap());
        let backend = AsyncLocalBackend::new(Arc::clone(&wal));

        // Serialize some records
        let records: Vec<Vec<u8>> = (0..3)
            .map(|i| {
                let record = WalRecord::CreateNode {
                    id: NodeId::new(i),
                    labels: vec!["Test".to_string()],
                };
                bincode::serde::encode_to_vec(&record, bincode::config::standard()).unwrap()
            })
            .collect();

        backend.write_wal_batch(&records).await.unwrap();
        backend.sync().await.unwrap();

        assert_eq!(wal.record_count(), 3);
    }

    #[tokio::test]
    async fn local_backend_close_flushes() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Arc::new(AsyncTypedWal::open(dir.path()).await.unwrap());
        let backend = AsyncLocalBackend::new(Arc::clone(&wal));

        // Write a record via the WAL directly
        wal.log(&WalRecord::CreateNode {
            id: NodeId::new(1),
            labels: vec!["Test".to_string()],
        })
        .await
        .unwrap();

        // Close should flush + sync without error
        backend.close().await.unwrap();
    }

    #[tokio::test]
    async fn local_backend_as_trait_object() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Arc::new(AsyncTypedWal::open(dir.path()).await.unwrap());
        let backend: Arc<dyn AsyncStorageBackend> = Arc::new(AsyncLocalBackend::new(wal));

        assert_eq!(backend.name(), "local-async");
        backend.write_wal_batch(&[]).await.unwrap();
        backend.sync().await.unwrap();
        backend.close().await.unwrap();
    }
}
