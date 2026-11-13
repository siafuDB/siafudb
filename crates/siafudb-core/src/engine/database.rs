// siafudb-core/src/engine/database.rs
//
// SiafuDB — The primary database interface.
//
// Design principle: If you can use SQLite, you can use SiafuDB.
// Open a file. Execute queries. Get results. The graph is the upgrade
// path, not the entry requirement.
//
// Underneath, GrafeoDB provides the graph engine. SiafuDB adds:
// - Fragment awareness (this instance knows it's part of a larger graph)
// - Mutation tracking (every write is captured for the sync protocol)
// - Cryptographic identity (every node knows where it came from)
// - Multiple access patterns (graph, document, KV, time-series)

use crate::changelog::{ChangeLog, SharedChangeLog};
use crate::error::SiafuError;
use crate::fragment::Fragment;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// The main SiafuDB database instance.
///
/// Every SiafuDB instance is a fragment — it holds a meaningful subset
/// of a larger graph. On a phone, the fragment holds your personal context.
/// On an edge node, it holds regional or situational context. In a cloud
/// deployment, it might hold the full platform graph. The engine is the
/// same everywhere; the fragment scope is what differs.
///
/// # Quick Start
///
/// ```no_run
/// use siafudb_core::SiafuDB;
///
/// # fn main() -> Result<(), siafudb_core::SiafuError> {
/// // Open or create a database (just like SQLite)
/// let db = SiafuDB::open("my_app.siafu")?;
///
/// // Execute graph queries with Cypher
/// db.execute("CREATE (:Person {name: 'Amara', city: 'Accra'})")?;
/// db.execute("CREATE (:Person {name: 'Tatenda', city: 'Harare'})")?;
/// db.execute("
///     MATCH (a:Person {name: 'Amara'}), (t:Person {name: 'Tatenda'})
///     CREATE (a)-[:KNOWS {since: 2024}]->(t)
/// ")?;
///
/// // Query relationships
/// let friends = db.query("
///     MATCH (p:Person {name: 'Amara'})-[:KNOWS]->(friend)
///     RETURN friend.name, friend.city
/// ")?;
///
/// // Or use it like a KV store (graph underneath, simple API on top)
/// db.kv_set("session:token", "abc123")?;
/// let token = db.kv_get("session:token")?;
///
/// // Or store a JSON document (becomes a subgraph)
/// db.doc_insert("messages", serde_json::json!({
///     "from": "Amara",
///     "text": "Hello!",
///     "timestamp": "2026-04-16T10:00:00Z"
/// }))?;
/// # Ok(())
/// # }
/// ```
pub struct SiafuDB {
    /// The underlying GrafeoDB engine instance.
    /// This is where all graph storage and query execution happens.
    engine: grafeo::GrafeoDB,

    /// The fragment configuration for this instance.
    /// Defines what portion of the larger graph this instance holds,
    /// what its authority scope is, and how it relates to other fragments.
    fragment: Fragment,

    /// The identity of this database instance.
    /// Used for signing mutations and establishing provenance.
    instance_id: Uuid,

    /// Whether mutation tracking is enabled.
    /// When true, every write is captured in the mutation log
    /// for the sync protocol to pick up. Enabled by default;
    /// can be disabled for bulk import performance.
    mutation_tracking: AtomicBool,

    /// Path to the database file, if persistent.
    /// None for in-memory databases.
    path: Option<std::path::PathBuf>,

    /// The engine-level change log. Every successful execute() appends one
    /// entry while mutation_tracking is on. The sync protocol layer drains
    /// this and translates entries into transport-ready Mutations.
    change_log: SharedChangeLog,
}

impl SiafuDB {
    /// Open or create a persistent database at the given path.
    ///
    /// This is the SQLite-equivalent entry point. If the file exists,
    /// it opens the existing database. If it doesn't, it creates a new one.
    /// The file is a single file — the entire graph, indexes, and metadata
    /// in one place. Easy to backup, easy to encrypt, easy to sync.
    #[cfg(feature = "native")]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, SiafuError> {
        let path = path.as_ref();
        let engine = grafeo::GrafeoDB::open(path.to_str().unwrap_or_default())
            .map_err(|e| SiafuError::EngineError(e.to_string()))?;

        let instance_id = Uuid::new_v4();

        Ok(Self {
            engine,
            fragment: Fragment::new_local(),
            instance_id,
            mutation_tracking: AtomicBool::new(true),
            path: Some(path.to_path_buf()),
            change_log: Arc::new(Mutex::new(ChangeLog::new())),
        })
    }

    /// Create an in-memory database.
    ///
    /// Useful for testing, for ephemeral caches, and for edge deployments
    /// where persistent storage isn't available (e.g., some WASM runtimes).
    /// The graph exists only in memory and is lost when the instance drops.
    pub fn in_memory() -> Result<Self, SiafuError> {
        let engine = grafeo::GrafeoDB::new_in_memory();

        Ok(Self {
            engine,
            fragment: Fragment::new_local(),
            instance_id: Uuid::new_v4(),
            mutation_tracking: AtomicBool::new(true),
            path: None,
            change_log: Arc::new(Mutex::new(ChangeLog::new())),
        })
    }

    /// Serialize the whole graph to a binary snapshot (Grafeo snapshot format).
    ///
    /// This is the persistence primitive for targets without a file system
    /// (browser): the caller encrypts the bytes and stores them in OPFS or
    /// IndexedDB.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, SiafuError> {
        self.engine
            .export_snapshot()
            .map_err(|e| SiafuError::EngineError(e.to_string()))
    }

    /// Create an in-memory database from a snapshot produced by
    /// [`export_snapshot`](Self::export_snapshot).
    pub fn from_snapshot(data: &[u8]) -> Result<Self, SiafuError> {
        let engine = grafeo::GrafeoDB::import_snapshot(data)
            .map_err(|e| SiafuError::EngineError(e.to_string()))?;
        Ok(Self {
            engine,
            fragment: Fragment::new_local(),
            instance_id: Uuid::new_v4(),
            mutation_tracking: AtomicBool::new(true),
            path: None,
            change_log: Arc::new(Mutex::new(ChangeLog::new())),
        })
    }

    /// Execute a graph query (Cypher or GQL) that modifies data.
    ///
    /// This is the write path. Every mutation is:
    /// 1. Executed against the GrafeoDB engine
    /// 2. Captured in the mutation log (if tracking is enabled)
    /// 3. Available for the sync protocol to pick up
    ///
    /// Returns the number of nodes/edges affected.
    pub fn execute(&self, query: &str) -> Result<ExecuteResult, SiafuError> {
        self.execute_with_params(query, serde_json::Map::new())
    }

    /// Execute a write query with bound parameters (`$name` in the query).
    ///
    /// Always prefer this over formatting values into the query string:
    /// bound values can never be parsed as query syntax. Supported parameter
    /// types: string, bool, number, null (arrays/objects are bound as their
    /// JSON text).
    pub fn execute_with_params(
        &self,
        query: &str,
        params: serde_json::Map<String, serde_json::Value>,
    ) -> Result<ExecuteResult, SiafuError> {
        let bound = to_engine_params(&params)?;
        let result = self
            .engine
            .execute_with_params(query, bound)
            .map_err(|e| SiafuError::QueryError(e.to_string()))?;

        let rows_affected = result.rows().len();

        if self.mutation_tracking.load(Ordering::Relaxed)
            && let Ok(mut log) = self.change_log.lock()
        {
            log.append(query.to_string(), rows_affected, params);
        }

        Ok(ExecuteResult { rows_affected })
    }

    /// Execute a graph query that reads data.
    ///
    /// This is the read path. No mutation tracking, no sync implications.
    /// Just query the local fragment and return results.
    pub fn query(&self, query: &str) -> Result<QueryResult, SiafuError> {
        self.query_with_params(query, serde_json::Map::new())
    }

    /// Read query with bound parameters. See [`Self::execute_with_params`].
    pub fn query_with_params(
        &self,
        query: &str,
        params: serde_json::Map<String, serde_json::Value>,
    ) -> Result<QueryResult, SiafuError> {
        let result = self
            .engine
            .execute_with_params(query, to_engine_params(&params)?)
            .map_err(|e| SiafuError::QueryError(e.to_string()))?;

        let rows: Vec<Vec<serde_json::Value>> = result
            .rows()
            .iter()
            .map(|row| row.iter().map(from_engine_value).collect())
            .collect();

        Ok(QueryResult {
            columns: result.columns.clone(),
            rows,
        })
    }

    /// Read query with bound parameters returning engine-typed cells
    /// (bytes, temporal values, etc. are preserved instead of being
    /// flattened to JSON). Used by the FFI layer.
    pub fn query_typed_with_params(
        &self,
        query: &str,
        params: serde_json::Map<String, serde_json::Value>,
    ) -> Result<TypedQueryResult, SiafuError> {
        let result = self
            .engine
            .execute_with_params(query, to_engine_params(&params)?)
            .map_err(|e| SiafuError::QueryError(e.to_string()))?;
        Ok(TypedQueryResult {
            columns: result.columns.clone(),
            rows: result.rows().iter().map(|r| r.to_vec()).collect(),
        })
    }

    /// Get the fragment configuration for this instance.
    ///
    /// The fragment describes what this database holds in relation to
    /// the larger graph. On a personal device, this is the user's
    /// personal context. On a platform server, this might be the
    /// full knowledge graph.
    pub fn fragment(&self) -> &Fragment {
        &self.fragment
    }

    /// Get the unique instance ID.
    ///
    /// Every SiafuDB instance has a UUID that identifies it in the
    /// sync protocol. When mutations propagate between instances,
    /// this ID is how the protocol tracks which instance made which change.
    pub fn instance_id(&self) -> Uuid {
        self.instance_id
    }

    /// Path to the database file on disk, if this instance is persistent.
    ///
    /// Returns `None` for in-memory databases. Useful for diagnostics
    /// and for callers that need to back up, copy, or report the file
    /// location.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Enable or disable mutation tracking.
    ///
    /// Disabling tracking is useful during bulk imports where you don't
    /// want every individual INSERT to be logged for sync. Re-enable
    /// after the import completes.
    ///
    /// Takes `&self` (an atomic flag) so shared handles, such as the
    /// UniFFI object wrapper, can toggle it.
    pub fn set_mutation_tracking(&self, enabled: bool) {
        self.mutation_tracking.store(enabled, Ordering::Relaxed);
    }

    /// Borrow the engine-level change log.
    ///
    /// Returns an `Arc` clone — multiple consumers can read or drain in
    /// parallel via the inner `Mutex`. The sync protocol layer is the
    /// expected primary consumer.
    pub fn change_log(&self) -> SharedChangeLog {
        Arc::clone(&self.change_log)
    }

    // === KV Access Pattern ===
    // These methods let developers use SiafuDB like a key-value store.
    // Underneath, each key-value pair is a node in the graph with a
    // special label. This means KV data participates in the graph —
    // you can query it with Cypher, relate it to other nodes, and
    // sync it through the protocol.

    /// Store a key-value pair.
    ///
    /// Stored as a graph node: (:KV {key: "...", value: "..."})
    /// This means KV data is graph data — it syncs, it's queryable,
    /// it participates in relationships if you want it to.
    pub fn kv_set(&self, key: &str, value: &str) -> Result<(), SiafuError> {
        // MERGE ensures we update if exists, create if not.
        self.execute_with_params(
            "MERGE (n:_KV {key: $key}) SET n.value = $value",
            params([("key", key.into()), ("value", value.into())]),
        )?;
        Ok(())
    }

    /// Retrieve a value by key.
    pub fn kv_get(&self, key: &str) -> Result<Option<String>, SiafuError> {
        let result = self.query_with_params(
            "MATCH (n:_KV {key: $key}) RETURN n.value",
            params([("key", key.into())]),
        )?;
        Ok(result.rows.first().and_then(|row| {
            row.first().and_then(|v| {
                if let serde_json::Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
        }))
    }

    /// Delete a key-value pair.
    pub fn kv_delete(&self, key: &str) -> Result<(), SiafuError> {
        self.execute_with_params(
            "MATCH (n:_KV {key: $key}) DELETE n",
            params([("key", key.into())]),
        )?;
        Ok(())
    }

    // === Document Access Pattern ===
    // These methods let developers store JSON documents in named collections.
    // Each document becomes a subgraph — the document node with properties
    // extracted from the JSON. Nested objects become related nodes.

    /// Insert a JSON document into a named collection.
    ///
    /// The document becomes a node with label matching the collection name,
    /// with all top-level JSON properties stored as node properties.
    pub fn doc_insert(
        &self,
        collection: &str,
        document: serde_json::Value,
    ) -> Result<Uuid, SiafuError> {
        let label = ident(collection)?;
        let doc_id = Uuid::new_v4();

        if let serde_json::Value::Object(map) = &document {
            let mut bound = serde_json::Map::new();
            let mut props = vec!["_id: $_id".to_string()];
            bound.insert("_id".into(), doc_id.to_string().into());
            for (i, (k, v)) in map.iter().enumerate() {
                let k = ident(k)?;
                if k.starts_with('_') {
                    return Err(SiafuError::QueryError(format!(
                        "document keys starting with '_' are reserved: {k:?}"
                    )));
                }
                let p = format!("p{i}");
                props.push(format!("{k}: ${p}"));
                // Nested arrays/objects are stored as JSON text until nested
                // documents are decomposed into subgraphs.
                let v = match v {
                    serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                        serde_json::Value::String(v.to_string())
                    }
                    other => other.clone(),
                };
                bound.insert(p, v);
            }
            let query = format!("CREATE (:_doc_{label} {{{}}})", props.join(", "));
            self.execute_with_params(&query, bound)?;
        }

        Ok(doc_id)
    }

    /// Find documents in a collection whose top-level properties equal every
    /// entry of `filter` (an empty map matches all). Keys must be plain
    /// identifiers; values are bound parameters.
    pub fn doc_find(
        &self,
        collection: &str,
        filter: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<QueryResult, SiafuError> {
        let label = ident(collection)?;
        let mut bound = serde_json::Map::new();
        let mut conds = Vec::new();
        for (i, (k, v)) in filter.iter().enumerate() {
            let k = ident(k)?;
            let p = format!("f{i}");
            conds.push(format!("{k}: ${p}"));
            bound.insert(p, v.clone());
        }
        let query = if conds.is_empty() {
            format!("MATCH (d:_doc_{label}) RETURN d")
        } else {
            format!("MATCH (d:_doc_{label} {{{}}}) RETURN d", conds.join(", "))
        };
        self.query_with_params(&query, bound)
    }
}

/// Accept only `[A-Za-z_][A-Za-z0-9_]*` (max 64) wherever a name is spliced
/// into query text. Values never go through here; they are bound parameters.
fn ident(s: &str) -> Result<&str, SiafuError> {
    let mut chars = s.chars();
    let ok = s.len() <= 64
        && matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(s)
    } else {
        Err(SiafuError::QueryError(format!("invalid identifier: {s:?}")))
    }
}

/// Engine value -> JSON. Types JSON cannot express natively (bytes, temporal,
/// vectors) fall back to their debug text until a typed result API exists.
fn from_engine_value(v: &grafeo::Value) -> serde_json::Value {
    use grafeo::Value as V;
    match v {
        V::Null => serde_json::Value::Null,
        V::Bool(b) => (*b).into(),
        V::Int64(i) => (*i).into(),
        V::Float64(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        V::String(s) => s.as_str().into(),
        V::List(items) => items.iter().map(from_engine_value).collect(),
        other => serde_json::Value::String(format!("{other:?}")),
    }
}

fn params<const N: usize>(
    pairs: [(&str, serde_json::Value); N],
) -> serde_json::Map<String, serde_json::Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn to_engine_params(
    p: &serde_json::Map<String, serde_json::Value>,
) -> Result<std::collections::HashMap<String, grafeo::Value>, SiafuError> {
    use serde_json::Value as J;
    p.iter()
        .map(|(k, v)| {
            let v = match v {
                J::Null => grafeo::Value::Null,
                J::Bool(b) => grafeo::Value::from(*b),
                J::String(s) => grafeo::Value::from(s.as_str()),
                J::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
                    (Some(i), _, _) => grafeo::Value::from(i),
                    // u64 above i64::MAX would silently lose precision as f64.
                    (None, Some(_), _) => {
                        return Err(SiafuError::QueryError(format!(
                            "parameter {k:?}: integer out of range (> i64::MAX)"
                        )));
                    }
                    (None, None, Some(f)) => grafeo::Value::from(f),
                    _ => unreachable!("serde_json numbers are i64, u64 or f64"),
                },
                // No silent coercion: a list/object would otherwise be bound
                // as text and match a different value than the caller meant.
                J::Array(_) | J::Object(_) => {
                    return Err(SiafuError::QueryError(format!(
                        "parameter {k:?}: arrays and objects are not supported as bound values"
                    )));
                }
            };
            Ok((k.clone(), v))
        })
        .collect()
}

/// Result of a write operation.
#[derive(Debug)]
pub struct ExecuteResult {
    pub rows_affected: usize,
}

/// Result of a read query.
#[derive(Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
}

/// Result of a read query with engine-typed cells.
#[derive(Debug)]
pub struct TypedQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<grafeo::Value>>,
}
