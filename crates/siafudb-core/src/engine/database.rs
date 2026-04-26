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
    mutation_tracking: bool,

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
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, SiafuError> {
        let path = path.as_ref();
        let engine = grafeo::GrafeoDB::open(path.to_str().unwrap_or_default())
            .map_err(|e| SiafuError::EngineError(e.to_string()))?;

        let instance_id = Uuid::new_v4();

        Ok(Self {
            engine,
            fragment: Fragment::new_local(),
            instance_id,
            mutation_tracking: true,
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
            mutation_tracking: true,
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
        let result = self
            .engine
            .execute(query)
            .map_err(|e| SiafuError::QueryError(e.to_string()))?;

        let rows_affected = result.rows.len();

        if self.mutation_tracking
            && let Ok(mut log) = self.change_log.lock()
        {
            log.append(query.to_string(), rows_affected);
        }

        Ok(ExecuteResult { rows_affected })
    }

    /// Execute a graph query that reads data.
    ///
    /// This is the read path. No mutation tracking, no sync implications.
    /// Just query the local fragment and return results.
    pub fn query(&self, query: &str) -> Result<QueryResult, SiafuError> {
        let result = self.engine.execute(query)
            .map_err(|e| SiafuError::QueryError(e.to_string()))?;

        let rows: Vec<Vec<serde_json::Value>> = result
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|val| serde_json::Value::String(format!("{:?}", val)))
                    .collect()
            })
            .collect();

        Ok(QueryResult {
            columns: result.columns.clone(),
            rows,
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
    pub fn set_mutation_tracking(&mut self, enabled: bool) {
        self.mutation_tracking = enabled;
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
        let query = format!(
            "MERGE (n:_KV {{key: '{}'}}) SET n.value = '{}'",
            key.replace('\'', "\\'"),
            value.replace('\'', "\\'")
        );
        self.execute(&query)?;
        Ok(())
    }

    /// Retrieve a value by key.
    pub fn kv_get(&self, key: &str) -> Result<Option<String>, SiafuError> {
        let query = format!(
            "MATCH (n:_KV {{key: '{}'}}) RETURN n.value",
            key.replace('\'', "\\'")
        );
        let result = self.query(&query)?;
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
        let query = format!(
            "MATCH (n:_KV {{key: '{}'}}) DELETE n",
            key.replace('\'', "\\'")
        );
        self.execute(&query)?;
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
        let doc_id = Uuid::new_v4();

        if let serde_json::Value::Object(map) = &document {
            let props: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, value_to_cypher(v)))
                .collect();

            let query = format!(
                "CREATE (:_{} {{_id: '{}', {}}})",
                collection,
                doc_id,
                props.join(", ")
            );
            self.execute(&query)?;
        }

        Ok(doc_id)
    }

    /// Find documents in a collection matching a filter.
    pub fn doc_find(
        &self,
        collection: &str,
        filter: &str,
    ) -> Result<QueryResult, SiafuError> {
        let query = if filter.is_empty() {
            format!("MATCH (d:_{}) RETURN d", collection)
        } else {
            format!("MATCH (d:_{} {{{}}}) RETURN d", collection, filter)
        };
        self.query(&query)
    }
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

/// Convert a serde_json::Value to a Cypher literal.
fn value_to_cypher(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        // For nested objects and arrays, store as JSON string for now.
        // Phase 2 will decompose nested objects into subgraph nodes.
        other => format!("'{}'", other.to_string().replace('\'', "\\'")),
    }
}
