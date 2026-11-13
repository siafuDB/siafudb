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
use crate::sync::{GraphChange, SyncState};
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

    /// Structured change capture for sync (`enable_change_capture`). `None`
    /// until enabled; `capture` mirrors that so the write path pays one
    /// atomic load while it is off.
    sync: Mutex<Option<SyncState>>,
    capture: AtomicBool,
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
            sync: Mutex::new(None),
            capture: AtomicBool::new(false),
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
            sync: Mutex::new(None),
            capture: AtomicBool::new(false),
        })
    }

    /// Serialize the whole graph to a binary snapshot (Grafeo snapshot format).
    ///
    /// This is the persistence primitive for targets without a file system
    /// (browser): the caller encrypts the bytes and stores them in OPFS or
    /// IndexedDB.
    ///
    /// With change capture on, the snapshot also carries the changes that
    /// were captured but not yet taken (as a trailer after the engine's
    /// bytes, read back by [`from_snapshot`](Self::from_snapshot)), so a
    /// write made before the export is still reported after a reload.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, SiafuError> {
        let export = || {
            self.engine
                .export_snapshot()
                .map_err(|e| SiafuError::EngineError(e.to_string()))
        };
        let mut guard = self.sync_state()?;
        let Some(state) = guard.as_mut() else {
            return export();
        };
        // A write that commits while the snapshot is taken would be in the
        // graph but not among the queued changes: take both again.
        for _ in 0..8 {
            state.drain(&self.engine)?;
            let snapshot = export()?;
            if !state.behind(&self.engine) {
                return Ok(crate::sync::append_trailer(snapshot, state.pending()));
            }
        }
        Err(SiafuError::SyncError(
            "export_snapshot: writes kept arriving during the export".to_string(),
        ))
    }

    /// Create an in-memory database from a snapshot produced by
    /// [`export_snapshot`](Self::export_snapshot).
    ///
    /// A snapshot exported with change capture on comes back with capture
    /// on, the same global ids, and the changes that were queued at export;
    /// nothing written before the export is reported again.
    pub fn from_snapshot(data: &[u8]) -> Result<Self, SiafuError> {
        let (data, pending) = crate::sync::split_trailer(data)?;
        let engine = grafeo::GrafeoDB::import_snapshot(data)
            .map_err(|e| SiafuError::EngineError(e.to_string()))?;
        let db = Self {
            engine,
            fragment: Fragment::new_local(),
            instance_id: Uuid::new_v4(),
            mutation_tracking: AtomicBool::new(true),
            path: None,
            change_log: Arc::new(Mutex::new(ChangeLog::new())),
            sync: Mutex::new(None),
            capture: AtomicBool::new(false),
        };
        if let Some(pending) = pending {
            db.start_capture(pending)?;
        }
        Ok(db)
    }

    // === Structured change capture (sync) ===
    // See `crate::sync` for the model. Off by default.

    /// Start capturing what local writes do, as [`GraphChange`]s keyed by
    /// global ids, for [`take_changes`](Self::take_changes). Idempotent.
    ///
    /// Capture sees every committed write to the default graph made through
    /// this handle from now on: `execute`, `query` (a "read" query that
    /// writes), and the KV and document helpers. Entities written earlier
    /// are not reported until a later write touches them, at which point
    /// they are reported whole. Capture is per handle and in memory: enable
    /// it again after [`open`](Self::open) (a snapshot restores it itself).
    ///
    /// Enabling scans the graph once for existing global ids.
    pub fn enable_change_capture(&self) -> Result<(), SiafuError> {
        self.start_capture(Vec::new())
    }

    /// Whether change capture is on.
    pub fn change_capture_enabled(&self) -> bool {
        self.capture.load(Ordering::Acquire)
    }

    fn start_capture(&self, pending: Vec<GraphChange>) -> Result<(), SiafuError> {
        let mut guard = self.sync_state()?;
        if guard.is_none() {
            // The log records from here on; the scan below then indexes the
            // ids of everything written before.
            self.engine.set_cdc_enabled(true);
            let state = SyncState::load(&self.engine, pending)
                .inspect_err(|_| self.engine.set_cdc_enabled(false))?;
            *guard = Some(state);
            self.capture.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn sync_state(&self) -> Result<std::sync::MutexGuard<'_, Option<SyncState>>, SiafuError> {
        self.sync
            .lock()
            .map_err(|_| SiafuError::SyncError("sync state lock poisoned".to_string()))
    }

    /// After a statement ran: move what it did from the engine's log into
    /// the queue of captured changes. A failure here is not the statement's;
    /// it is retried and reported by `take_changes`.
    fn capture_writes(&self) {
        if self.capture.load(Ordering::Acquire)
            && let Ok(mut guard) = self.sync.lock()
            && let Some(state) = guard.as_mut()
        {
            let _ = state.drain(&self.engine);
        }
    }

    /// Everything local writes did since the last call, in commit order.
    ///
    /// Every node and edge named has a stable global id. Changes applied
    /// with [`apply_changes`](Self::apply_changes) are never returned.
    /// Returns an empty list while capture is off.
    ///
    /// # Errors
    ///
    /// [`SiafuError::UnsupportedValue`] when a written property value has no
    /// lossless form (see [`ChangeValue`](crate::sync::ChangeValue)): that
    /// property was left out of its change. The changes stay queued and the
    /// next call returns them. Other errors leave the queue untouched too.
    pub fn take_changes(&self) -> Result<Vec<GraphChange>, SiafuError> {
        let mut guard = self.sync_state()?;
        let Some(state) = guard.as_mut() else {
            return Ok(Vec::new());
        };
        state.drain(&self.engine)?;
        state.take()
    }

    /// The whole current default graph as puts: every node, then every edge
    /// (so an edge never precedes its endpoints), each with all its labels
    /// and properties. Turns change capture on.
    ///
    /// This is how a protocol layer uploads a database that had data before
    /// sync was set up: capture alone reports an older entity only when a
    /// later write touches it. Entities without a global id get one here;
    /// entities that have one keep it.
    ///
    /// The call **replaces the queue** of [`take_changes`](Self::take_changes):
    /// what was captured and not yet taken describes writes the returned
    /// graph already shows, so it is dropped. Writes made after the call are
    /// captured as usual.
    ///
    /// # Errors
    ///
    /// As [`take_changes`](Self::take_changes): on
    /// [`SiafuError::UnsupportedValue`] the property was left out, the puts
    /// stay queued, and the next `take_changes` returns them.
    pub fn take_graph_as_changes(&self) -> Result<Vec<GraphChange>, SiafuError> {
        self.start_capture(Vec::new())?;
        let mut guard = self.sync_state()?;
        match guard.as_mut() {
            Some(state) => state.take_graph(&self.engine),
            None => Err(SiafuError::SyncError("change capture is off".to_string())),
        }
    }

    /// Apply changes captured on another device, in order, as one
    /// transaction: on an error nothing is applied.
    ///
    /// Idempotent: applying a list again leaves the same graph. The writes
    /// are not captured, so they never show up in
    /// [`take_changes`](Self::take_changes). A `NodePut` for an unknown
    /// global id creates the node; `NodeDel` removes the node with its
    /// edges; deletes of unknown ids are ignored. Turns change capture on.
    ///
    /// # Errors
    ///
    /// [`SiafuError::UnknownEndpoint`] for an `EdgePut` whose source or
    /// destination node is not known here; [`SiafuError::InvalidChange`] for
    /// a change that names a reserved property or re-points an existing
    /// edge; engine errors (for example a constraint violation).
    pub fn apply_changes(&self, changes: &[GraphChange]) -> Result<(), SiafuError> {
        self.start_capture(Vec::new())?;
        let mut guard = self.sync_state()?;
        match guard.as_mut() {
            Some(state) => state.apply(&self.engine, changes),
            None => Err(SiafuError::SyncError("change capture is off".to_string())),
        }
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
        self.capture_writes();

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
        self.capture_writes();

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
        self.capture_writes();
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
