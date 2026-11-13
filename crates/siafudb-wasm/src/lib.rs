//! Browser bindings for SiafuDB.
//!
//! The JS-facing [`Database`] class is a thin shell over [`Core`], which holds
//! all logic in plain Rust types so it is unit-testable on the native target.
//! Parameters cross the boundary as a JSON object string and are always bound
//! with `execute_with_params` / `query_with_params`; user values are never
//! interpolated into query text.

use serde_json::{Map, Value, json};
use siafudb_core::SiafuDB;
use siafudb_gsp::SyncedDb;
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

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Plain-Rust core of the binding (no wasm types), so it can be tested natively.
///
/// The database and its optional sync replica live in one [`SyncedDb`]; see
/// `siafudb-gsp` for what each sync call does. Errors are strings that never
/// hold a pairing code or a relay token.
pub struct Core {
    db: SyncedDb,
}

impl Core {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            db: SyncedDb::new(SiafuDB::in_memory().map_err(|e| e.to_string())?),
        })
    }
    /// From `export_snapshot` bytes: the combined blob, or a bare engine
    /// snapshot written before sync existed.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self {
            db: SyncedDb::from_bytes(bytes).map_err(|e| e.to_string())?,
        })
    }
    /// The engine snapshot and, in a sync group, the replica state and the
    /// transport note, in one blob.
    pub fn export_snapshot(&self) -> Result<Vec<u8>, String> {
        Ok(self.db.to_bytes().map_err(|e| e.to_string())?.to_vec())
    }
    /// Returns `{"rowsAffected": n}` as JSON.
    pub fn execute(&self, q: &str, params: Option<String>) -> Result<String, String> {
        let r = self
            .db
            .db()
            .execute_with_params(q, parse_params(params)?)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "rowsAffected": r.rows_affected }).to_string())
    }
    /// Returns `{"columns": [...], "rows": [[...]]}` with typed JSON cells.
    pub fn query(&self, q: &str, params: Option<String>) -> Result<String, String> {
        let r = self
            .db
            .db()
            .query_with_params(q, parse_params(params)?)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "columns": r.columns, "rows": r.rows }).to_string())
    }
    /// Change-log entries with `sequence > since`, as a JSON array.
    pub fn changes_since(&self, since: u64) -> Result<String, String> {
        let log = self.db.db().change_log();
        let entries = log
            .lock()
            .map_err(|_| "change log poisoned".to_string())?
            .since(since);
        serde_json::to_string(&entries).map_err(|e| e.to_string())
    }

    // --- sync ---

    pub fn sync_create(&mut self, now_ms: u64) -> Result<String, String> {
        let code = self.db.sync_create(now_ms).map_err(|e| e.to_string())?;
        Ok(code.as_str().to_owned())
    }
    pub fn sync_join(&mut self, code: &str, now_ms: u64) -> Result<(), String> {
        self.db.sync_join(code, now_ms).map_err(|e| e.to_string())
    }
    pub fn sync_leave(&mut self) {
        self.db.sync_leave();
    }
    pub fn sync_enabled(&self) -> bool {
        self.db.sync_enabled()
    }
    pub fn sync_commit(&mut self, now_ms: u64) -> Result<usize, String> {
        self.db.commit_local(now_ms).map_err(|e| e.to_string())
    }
    /// JSON object of sizes and counters, or `null` when not in a sync group.
    pub fn sync_status(&self) -> String {
        match self.db.status() {
            None => "null".to_string(),
            Some(s) => json!({
                "groupId": hex(&s.fragment_id),
                "outbox": s.outbox,
                "cursor": s.cursor,
                "localOps": s.local_ops,
                "remoteOps": s.remote_ops,
                "pending": s.pending,
                "devices": s.devices,
                "rejected": s.rejected,
                "droppedLocal": s.dropped_local,
                "skippedRemote": s.skipped_remote,
                "lastProblem": s.last_problem,
            })
            .to_string(),
        }
    }
    pub fn sync_pairing_code(&self) -> Option<String> {
        self.db.pairing_code().map(|c| c.as_str().to_owned())
    }
    pub fn sync_relay_token(&self) -> Option<String> {
        self.db.relay_token().map(|t| t.as_str().to_owned())
    }
    pub fn sync_push_frame(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.db.push_frame().map_err(|e| e.to_string())
    }
    pub fn sync_accept_ack(&mut self, reply: &[u8]) -> Result<usize, String> {
        self.db.accept_ack(reply).map_err(|e| e.to_string())
    }
    pub fn sync_pull_frame(&self) -> Result<Vec<u8>, String> {
        self.db.pull_frame().map_err(|e| e.to_string())
    }
    /// Returns `{"items": n, "ops": n, "changes": n, "more": bool, "stalled": bool}` as JSON.
    pub fn sync_ingest_batch(&mut self, reply: &[u8], now_ms: u64) -> Result<String, String> {
        let o = self
            .db
            .ingest_batch(reply, now_ms)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "items": o.items,
            "ops": o.ops,
            "changes": o.changes,
            "more": o.more,
            "stalled": o.stalled,
        })
        .to_string())
    }
    pub fn sync_poll(&mut self, now_ms: u64) -> Result<usize, String> {
        self.db.poll(now_ms).map_err(|e| e.to_string())
    }
    pub fn sync_meta(&self) -> String {
        String::from_utf8_lossy(self.db.meta()).into_owned()
    }
    pub fn set_sync_meta(&mut self, meta: &str) {
        self.db.set_meta(meta.as_bytes().to_vec());
    }
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

/// An in-memory SiafuDB instance. Persistence is the caller's job:
/// `exportSnapshot()` / `Database.fromSnapshot()`.
///
/// The `sync*` methods implement device-to-device sync (GSP sync v0) without
/// doing any I/O: they build and read relay frames and the caller moves the
/// bytes. Methods that take `now_ms` want `Date.now()`.
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

    /// Serialise the whole database (plaintext; encrypt before storing). In a
    /// sync group the bytes also hold the sync state, which includes the
    /// group's secret: one blob, to be written in one step.
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

    /// Create a new sync group with this database as its first device.
    /// Returns the pairing code (a secret) to enter on another device. Data
    /// already in the database is queued for upload.
    #[wasm_bindgen(js_name = syncCreate)]
    pub fn sync_create(&mut self, now_ms: f64) -> Result<String, JsError> {
        self.core.sync_create(now_ms as u64).map_err(js)
    }

    /// Join the sync group of a pairing code.
    #[wasm_bindgen(js_name = syncJoin)]
    pub fn sync_join(&mut self, code: &str, now_ms: f64) -> Result<(), JsError> {
        self.core.sync_join(code, now_ms as u64).map_err(js)
    }

    /// Leave the sync group. The data stays; the sync state is dropped.
    #[wasm_bindgen(js_name = syncLeave)]
    pub fn sync_leave(&mut self) {
        self.core.sync_leave();
    }

    /// Whether the database is in a sync group.
    #[wasm_bindgen(js_name = syncEnabled)]
    pub fn sync_enabled(&self) -> bool {
        self.core.sync_enabled()
    }

    /// Call after every `execute` or `query` while in a sync group: queues
    /// what the call wrote for upload. Returns the number of ops queued.
    #[wasm_bindgen(js_name = syncCommit)]
    pub fn sync_commit(&mut self, now_ms: f64) -> Result<u32, JsError> {
        self.core
            .sync_commit(now_ms as u64)
            .map(|n| n as u32)
            .map_err(js)
    }

    /// JSON: `null` when not in a sync group, else `{"groupId", "outbox",
    /// "cursor", "localOps", "remoteOps", "pending", "devices", "rejected",
    /// "droppedLocal", "skippedRemote", "lastProblem"}`.
    #[wasm_bindgen(js_name = syncStatus)]
    pub fn sync_status(&self) -> String {
        self.core.sync_status()
    }

    /// The group's pairing code (a secret), to add another device.
    #[wasm_bindgen(js_name = syncPairingCode)]
    pub fn sync_pairing_code(&self) -> Option<String> {
        self.core.sync_pairing_code()
    }

    /// The relay bearer token (base64url) for `Authorization: Bearer ...`.
    #[wasm_bindgen(js_name = syncRelayToken)]
    pub fn sync_relay_token(&self) -> Option<String> {
        self.core.sync_relay_token()
    }

    /// The next `Push` frame for `POST /v0/push`, or `undefined` when there
    /// is nothing to push. Pass the reply body to `syncAcceptAck`.
    #[wasm_bindgen(js_name = syncPushFrame)]
    pub fn sync_push_frame(&mut self) -> Result<Option<Vec<u8>>, JsError> {
        self.core.sync_push_frame().map_err(js)
    }

    /// Take the relay's `Ack`. Returns how many items left the outbox.
    #[wasm_bindgen(js_name = syncAcceptAck)]
    pub fn sync_accept_ack(&mut self, reply: &[u8]) -> Result<u32, JsError> {
        self.core
            .sync_accept_ack(reply)
            .map(|n| n as u32)
            .map_err(js)
    }

    /// The `Pull` frame for the current cursor, for `POST /v0/pull`.
    #[wasm_bindgen(js_name = syncPullFrame)]
    pub fn sync_pull_frame(&self) -> Result<Vec<u8>, JsError> {
        self.core.sync_pull_frame().map_err(js)
    }

    /// Take the relay's `Batch` and apply it. Returns JSON `{"items": n,
    /// "ops": n, "changes": n, "more": bool, "stalled": bool}`; `changes` is
    /// the number of changes applied to the graph. Persist when `items > 0`.
    /// With `stalled`, wait before pulling again.
    #[wasm_bindgen(js_name = syncIngestBatch)]
    pub fn sync_ingest_batch(&mut self, reply: &[u8], now_ms: f64) -> Result<String, JsError> {
        self.core
            .sync_ingest_batch(reply, now_ms as u64)
            .map_err(js)
    }

    /// Apply received changes that were held because their clock was ahead.
    /// Returns the number of changes applied to the graph.
    #[wasm_bindgen(js_name = syncPoll)]
    pub fn sync_poll(&mut self, now_ms: f64) -> Result<u32, JsError> {
        self.core
            .sync_poll(now_ms as u64)
            .map(|n| n as u32)
            .map_err(js)
    }

    /// A string the transport keeps with the database (the relay URL).
    #[wasm_bindgen(js_name = syncMeta)]
    pub fn sync_meta(&self) -> String {
        self.core.sync_meta()
    }

    /// Set the string returned by `syncMeta`. Persisted by `exportSnapshot`.
    #[wasm_bindgen(js_name = setSyncMeta)]
    pub fn set_sync_meta(&mut self, meta: &str) {
        self.core.set_sync_meta(meta);
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
        assert!(
            v(&c.changes_since(2).unwrap())
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn hostile_params_are_data_not_syntax() {
        let c = Core::new().unwrap();
        let evil = "x' }) DETACH DELETE n // \\ \"q\" \\n";
        let p = json!({ "n": evil }).to_string();
        c.execute("CREATE (:P {name: $n})", Some(p.clone()))
            .unwrap();
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
        assert!(
            v(&d.changes_since(0).unwrap())
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn bad_params_are_rejected() {
        let c = Core::new().unwrap();
        assert!(c.query("RETURN 1", Some("[1]".into())).is_err());
        assert!(c.query("RETURN 1", Some("{".into())).is_err());
    }

    #[test]
    fn two_cores_sync_through_frames() {
        // A stand-in relay: one log of sealed items.
        let mut log: Vec<gsp_wire::SealedOp> = Vec::new();
        let mut exchange = |c: &mut Core, now: u64| -> u64 {
            use gsp_wire::Message;
            while let Some(frame) = c.sync_push_frame().unwrap() {
                let Message::Push {
                    fragment_id,
                    sealed,
                } = Message::from_bytes(&frame).unwrap()
                else {
                    panic!("not a Push");
                };
                let count = sealed.len() as u32;
                log.extend(sealed);
                let ack = Message::Ack {
                    fragment_id,
                    cursor: log.len() as u64,
                    count,
                };
                c.sync_accept_ack(&ack.to_bytes().unwrap()).unwrap();
            }
            let Message::Pull {
                fragment_id, after, ..
            } = Message::from_bytes(&c.sync_pull_frame().unwrap()).unwrap()
            else {
                panic!("not a Pull");
            };
            let batch = Message::Batch {
                fragment_id,
                next: log.len() as u64,
                more: false,
                sealed: log[after as usize..].to_vec(),
            };
            let out = v(&c
                .sync_ingest_batch(&batch.to_bytes().unwrap(), now)
                .unwrap());
            out["changes"].as_u64().unwrap()
        };

        let mut a = Core::new().unwrap();
        a.execute("CREATE (:P {name: 'before sync'})", None)
            .unwrap();
        assert_eq!(a.sync_status(), "null");
        assert_eq!(a.sync_commit(1).unwrap(), 0);
        let code = a.sync_create(1_000).unwrap();
        assert!(code.starts_with("gsp0-"));
        a.set_sync_meta("{\"relayUrl\":\"http://relay.test\"}");

        let mut b = Core::new().unwrap();
        // A malformed code is refused and the error does not repeat it.
        let bad = format!("{}!", &code[..code.len() - 1]);
        let err = b.sync_join(&bad, 1_000).unwrap_err();
        assert!(!err.contains(&bad[5..20]), "{err}");
        assert!(!b.sync_enabled());
        b.sync_join(&code, 1_000).unwrap();
        assert_eq!(b.sync_relay_token(), a.sync_relay_token());
        assert_eq!(b.sync_relay_token().unwrap().len(), 43);

        exchange(&mut a, 2_000);
        assert_eq!(exchange(&mut b, 2_000), 1);
        b.execute("MATCH (p:P) SET p.seen = true", None).unwrap();
        assert_eq!(b.sync_commit(3_000).unwrap(), 1);
        exchange(&mut b, 3_000);
        assert_eq!(exchange(&mut a, 3_000), 1);
        let r = v(&a.query("MATCH (p:P) RETURN p.name, p.seen", None).unwrap());
        assert_eq!(r["rows"], json!([["before sync", true]]));

        // The exported blob carries the sync state and the transport note.
        let restored = Core::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
        assert!(restored.sync_enabled());
        assert_eq!(restored.sync_pairing_code().unwrap(), code);
        assert_eq!(v(&restored.sync_meta())["relayUrl"], "http://relay.test");
        let status = v(&restored.sync_status());
        assert_eq!(status["outbox"], 0);
        assert_eq!(status["devices"], 2);
        assert_eq!(status["cursor"], v(&a.sync_status())["cursor"]);
        assert_eq!(status["groupId"].as_str().unwrap().len(), 32);
        // Nothing in the status is secret.
        let text = restored.sync_status();
        assert!(!text.contains(&code[5..]));
        assert!(!text.contains(restored.sync_relay_token().unwrap().as_str()));
    }
}
