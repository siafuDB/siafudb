//! UniFFI bindings (Swift, Kotlin) over `siafudb-core`.
//!
//! The surface is a single shared, thread-safe [`Database`] object with typed
//! parameters and typed results. Values are always bound parameters
//! (`$name`) and never interpolated into query text.

use std::collections::HashMap;
use std::sync::Arc;

use siafudb_core::{EngineValue, SiafuDB, SiafuError};

uniffi::setup_scaffolding!();

/// A database value crossing the FFI boundary.
///
/// Query results use every case. As a query parameter, `Bytes` and `List`
/// are rejected with `InvalidArgument` (the core binds parameters via JSON,
/// which cannot carry them losslessly).
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum Value {
    Null,
    Bool { v: bool },
    Int { v: i64 },
    Float { v: f64 },
    Text { v: String },
    Bytes { v: Vec<u8> },
    List { v: Vec<Value> },
}

/// Column names plus typed rows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// One captured write, for the sync layer.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Change {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub query: String,
    pub rows_affected: u64,
    /// Bound parameters as a JSON object.
    pub params_json: String,
}

/// Distinct failure classes so apps can react differently.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum DbError {
    /// The database file is open elsewhere (another handle or process).
    #[error("database locked: {msg}")]
    Locked { msg: String },
    /// The file or snapshot is damaged or not a SiafuDB image.
    #[error("database corrupt: {msg}")]
    Corrupt { msg: String },
    /// The query failed to parse or execute.
    #[error("query error: {msg}")]
    Query { msg: String },
    /// The caller passed something the API cannot accept.
    #[error("invalid argument: {msg}")]
    InvalidArgument { msg: String },
    /// Any other engine or I/O failure.
    #[error("engine error: {msg}")]
    Engine { msg: String },
}

fn classify_engine(msg: String) -> DbError {
    // The engine reports typed errors only as text through siafudb-core.
    if msg.contains("Database locked")
        || msg.contains("held by another process")
        || msg.contains("already open in this process")
    {
        DbError::Locked { msg }
    } else if msg.contains("is damaged") || msg.contains("orrupt") {
        DbError::Corrupt { msg }
    } else {
        DbError::Engine { msg }
    }
}

impl From<SiafuError> for DbError {
    fn from(e: SiafuError) -> Self {
        match e {
            SiafuError::QueryError(m) => DbError::Query { msg: m },
            SiafuError::EngineError(m) => classify_engine(m),
            SiafuError::IoError(e) => classify_engine(e.to_string()),
            other => DbError::Engine {
                msg: other.to_string(),
            },
        }
    }
}

fn from_engine(v: &EngineValue) -> Value {
    match v {
        EngineValue::Null => Value::Null,
        EngineValue::Bool(b) => Value::Bool { v: *b },
        EngineValue::Int64(i) => Value::Int { v: *i },
        EngineValue::Float64(f) => Value::Float { v: *f },
        EngineValue::String(s) => Value::Text { v: s.to_string() },
        EngineValue::Bytes(b) => Value::Bytes { v: b.to_vec() },
        EngineValue::List(items) => Value::List {
            v: items.iter().map(from_engine).collect(),
        },
        // Temporal, map and vector values have no dedicated case yet; they
        // arrive as their display text (ISO 8601 for temporal values).
        other => Value::Text {
            v: other.to_string(),
        },
    }
}

fn to_json_params(
    params: HashMap<String, Value>,
) -> Result<serde_json::Map<String, serde_json::Value>, DbError> {
    use serde_json::Value as J;
    let mut out = serde_json::Map::new();
    for (k, v) in params {
        let j = match v {
            Value::Null => J::Null,
            Value::Bool { v } => J::Bool(v),
            Value::Int { v } => J::from(v),
            Value::Float { v } => J::Number(serde_json::Number::from_f64(v).ok_or_else(|| {
                DbError::InvalidArgument {
                    msg: format!("parameter {k:?}: NaN and infinity are not supported"),
                }
            })?),
            Value::Text { v } => J::String(v),
            Value::Bytes { .. } | Value::List { .. } => {
                return Err(DbError::InvalidArgument {
                    msg: format!("parameter {k:?}: Bytes and List parameters are not supported"),
                });
            }
        };
        out.insert(k, j);
    }
    Ok(out)
}

/// A SiafuDB database. Safe to share and call from any thread.
#[derive(uniffi::Object)]
pub struct Database {
    inner: SiafuDB,
}

#[uniffi::export]
impl Database {
    /// A new empty in-memory database.
    #[uniffi::constructor]
    pub fn in_memory() -> Result<Arc<Self>, DbError> {
        Ok(Arc::new(Self {
            inner: SiafuDB::in_memory()?,
        }))
    }

    /// Open or create a file-backed database. Not available on browser builds.
    #[cfg(feature = "native")]
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, DbError> {
        if path.is_empty() {
            return Err(DbError::InvalidArgument {
                msg: "path is empty".into(),
            });
        }
        Ok(Arc::new(Self {
            inner: SiafuDB::open(path)?,
        }))
    }

    /// Restore an in-memory database from `export_snapshot` bytes.
    #[uniffi::constructor]
    pub fn from_snapshot(data: Vec<u8>) -> Result<Arc<Self>, DbError> {
        let inner = SiafuDB::from_snapshot(&data).map_err(|e| match e {
            // A snapshot that fails to import is bad input, not an engine fault.
            SiafuError::EngineError(m) => DbError::Corrupt { msg: m },
            other => other.into(),
        })?;
        Ok(Arc::new(Self { inner }))
    }

    /// Serialize the whole graph.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, DbError> {
        Ok(self.inner.export_snapshot()?)
    }

    /// Run a write query with `$name` parameters; returns the affected count.
    pub fn execute(&self, query: String, params: HashMap<String, Value>) -> Result<u64, DbError> {
        let r = self
            .inner
            .execute_with_params(&query, to_json_params(params)?)?;
        Ok(r.rows_affected as u64)
    }

    /// Run a read query with `$name` parameters.
    pub fn query(&self, query: String, params: HashMap<String, Value>) -> Result<Rows, DbError> {
        let r = self
            .inner
            .query_typed_with_params(&query, to_json_params(params)?)?;
        Ok(Rows {
            columns: r.columns,
            rows: r
                .rows
                .iter()
                .map(|row| row.iter().map(from_engine).collect())
                .collect(),
        })
    }

    pub fn kv_get(&self, key: String) -> Result<Option<String>, DbError> {
        Ok(self.inner.kv_get(&key)?)
    }

    pub fn kv_set(&self, key: String, value: String) -> Result<(), DbError> {
        Ok(self.inner.kv_set(&key, &value)?)
    }

    pub fn kv_delete(&self, key: String) -> Result<(), DbError> {
        Ok(self.inner.kv_delete(&key)?)
    }

    /// Writes with `sequence > seq` (pass 0 for all).
    pub fn changes_since(&self, seq: u64) -> Vec<Change> {
        let log = self.inner.change_log();
        let entries = match log.lock() {
            Ok(g) => g.since(seq),
            Err(p) => p.into_inner().since(seq),
        };
        entries
            .into_iter()
            .map(|e| Change {
                sequence: e.sequence,
                timestamp_ms: e.timestamp_ms,
                query: e.query,
                rows_affected: e.rows_affected as u64,
                params_json: serde_json::Value::Object(e.params).to_string(),
            })
            .collect()
    }

    /// Toggle change capture (e.g. off during a bulk import).
    pub fn set_mutation_tracking(&self, enabled: bool) {
        self.inner.set_mutation_tracking(enabled);
    }
}
