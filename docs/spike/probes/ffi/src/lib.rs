use std::sync::Arc;
uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError { #[error("{msg}")] Failed { msg: String } }
impl From<siafudb_core::SiafuError> for FfiError { fn from(e: siafudb_core::SiafuError)->Self{ FfiError::Failed{msg:e.to_string()} } }

#[derive(uniffi::Record)]
pub struct Rows { pub columns: Vec<String>, pub rows: Vec<Vec<String>> }

#[derive(uniffi::Object)]
pub struct Db { inner: siafudb_core::SiafuDB }

#[uniffi::export]
impl Db {
    #[uniffi::constructor]
    pub fn in_memory() -> Result<Arc<Self>, FfiError> { Ok(Arc::new(Self{ inner: siafudb_core::SiafuDB::in_memory()? })) }
    pub fn execute(&self, q: String) -> Result<u64, FfiError> { Ok(self.inner.execute(&q)?.rows_affected as u64) }
    pub fn query(&self, q: String) -> Result<Rows, FfiError> {
        let r = self.inner.query(&q)?;
        Ok(Rows{ columns: r.columns, rows: r.rows.into_iter().map(|row| row.into_iter().map(|v| v.to_string()).collect()).collect() })
    }
}
