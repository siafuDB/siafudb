//! Browser bindings for SiafuDB.
//!
//! The JS-facing [`Database`] class is a thin shell over [`Core`], which holds
//! all logic in plain Rust types so it is unit-testable on the native target.
//! Parameters cross the boundary as a JSON object string and are always bound
//! with `execute_with_params` / `query_with_params`; user values are never
//! interpolated into query text.

use serde_json::{Map, Value, json};
use siafudb_core::SiafuDB;
use wasm_bindgen::prelude::*;

type Params = Map<String, Value>;

fn parse_params(params_json: Option<String>) -> Result<Params, String> {
    match params_json.as_deref() {
        None | Some("") | Some("null") => Ok(Map::new()),
        Some(s) => match serde_json::from_str::<Value>(s).map_err(|e| format!("params: {e}"))? {
            Value::Object(m) => Ok(m),
            _ => Err("params must be a JSON object".into()),
        },
    }
}

/// Plain-Rust core of the binding (no wasm types), so it can be tested natively.
pub struct Core {
    db: SiafuDB,
}

impl Core {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            db: SiafuDB::in_memory().map_err(|e| e.to_string())?,
        })
    }
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self {
            db: SiafuDB::from_snapshot(bytes).map_err(|e| e.to_string())?,
        })
    }
    pub fn export_snapshot(&self) -> Result<Vec<u8>, String> {
        self.db.export_snapshot().map_err(|e| e.to_string())
    }
    /// Returns `{"rowsAffected": n}` as JSON.
    pub fn execute(&self, q: &str, params: Option<String>) -> Result<String, String> {
        let r = self
            .db
            .execute_with_params(q, parse_params(params)?)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "rowsAffected": r.rows_affected }).to_string())
    }
    /// Returns `{"columns": [...], "rows": [[...]]}` with typed JSON cells.
    pub fn query(&self, q: &str, params: Option<String>) -> Result<String, String> {
        let r = self
            .db
            .query_with_params(q, parse_params(params)?)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "columns": r.columns, "rows": r.rows }).to_string())
    }
    /// Change-log entries with `sequence > since`, as a JSON array.
    pub fn changes_since(&self, since: u64) -> Result<String, String> {
        let log = self.db.change_log();
        let entries = log
            .lock()
            .map_err(|_| "change log poisoned".to_string())?
            .since(since);
        serde_json::to_string(&entries).map_err(|e| e.to_string())
    }
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

/// An in-memory SiafuDB instance. Persistence is the caller's job:
/// `exportSnapshot()` / `Database.fromSnapshot()`.
#[wasm_bindgen]
pub struct Database {
    core: Core,
}

#[wasm_bindgen]
impl Database {
    /// Create an empty database.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Database, JsError> {
        Ok(Database {
            core: Core::new().map_err(js)?,
        })
    }

    /// Restore a database from bytes produced by `exportSnapshot()`.
    #[wasm_bindgen(js_name = fromSnapshot)]
    pub fn from_snapshot(bytes: &[u8]) -> Result<Database, JsError> {
        Ok(Database {
            core: Core::from_snapshot(bytes).map_err(js)?,
        })
    }

    /// Serialise the whole database (plaintext; encrypt before storing).
    #[wasm_bindgen(js_name = exportSnapshot)]
    pub fn export_snapshot(&self) -> Result<Vec<u8>, JsError> {
        self.core.export_snapshot().map_err(js)
    }

    /// Run a write query. `params_json` is a JSON object string of `$name` bindings.
    /// Returns JSON `{"rowsAffected": n}`.
    pub fn execute(&self, query: &str, params_json: Option<String>) -> Result<String, JsError> {
        self.core.execute(query, params_json).map_err(js)
    }

    /// Run a read query. Returns JSON `{"columns": [...], "rows": [[...]]}`.
    pub fn query(&self, query: &str, params_json: Option<String>) -> Result<String, JsError> {
        self.core.query(query, params_json).map_err(js)
    }

    /// Change-log entries with `sequence > seq`, as a JSON array.
    #[wasm_bindgen(js_name = changesSince)]
    pub fn changes_since(&self, seq: f64) -> Result<String, JsError> {
        self.core.changes_since(seq as u64).map_err(js)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn change_log_records_params_and_since_filters() {
        let c = Core::new().unwrap();
        c.execute("CREATE (:P {name: $n})", Some(r#"{"n":"a"}"#.into()))
            .unwrap();
        c.execute("CREATE (:P {name: $n})", Some(r#"{"n":"b"}"#.into()))
            .unwrap();
        c.query("MATCH (p:P) RETURN p.name", None).unwrap(); // reads are not logged
        let all = v(&c.changes_since(0).unwrap());
        assert_eq!(all.as_array().unwrap().len(), 2);
        assert_eq!(all[0]["sequence"], 1);
        assert_eq!(all[1]["sequence"], 2);
        assert_eq!(all[1]["params"]["n"], "b");
        assert_eq!(all[0]["query"], "CREATE (:P {name: $n})");
        let tail = v(&c.changes_since(1).unwrap());
        assert_eq!(tail.as_array().unwrap().len(), 1);
        assert_eq!(tail[0]["sequence"], 2);
        assert!(v(&c.changes_since(2).unwrap()).as_array().unwrap().is_empty());
    }

    #[test]
    fn hostile_params_are_data_not_syntax() {
        let c = Core::new().unwrap();
        let evil = "x' }) DETACH DELETE n // \\ \"q\" \\n";
        let p = json!({ "n": evil }).to_string();
        c.execute("CREATE (:P {name: $n})", Some(p.clone())).unwrap();
        let r = v(&c
            .query("MATCH (p:P {name: $n}) RETURN p.name", Some(p))
            .unwrap());
        assert_eq!(r["rows"].as_array().unwrap().len(), 1);
        assert_eq!(r["rows"][0][0], evil);
    }

    #[test]
    fn snapshot_roundtrip_starts_a_fresh_change_log() {
        let c = Core::new().unwrap();
        c.execute("CREATE (:P {name: 'z'})", None).unwrap();
        let snap = c.export_snapshot().unwrap();
        let d = Core::from_snapshot(&snap).unwrap();
        let r = v(&d.query("MATCH (p:P) RETURN p.name", None).unwrap());
        assert_eq!(r["rows"][0][0], "z");
        assert!(v(&d.changes_since(0).unwrap()).as_array().unwrap().is_empty());
    }

    #[test]
    fn bad_params_are_rejected() {
        let c = Core::new().unwrap();
        assert!(c.query("RETURN 1", Some("[1]".into())).is_err());
        assert!(c.query("RETURN 1", Some("{".into())).is_err());
    }
}
