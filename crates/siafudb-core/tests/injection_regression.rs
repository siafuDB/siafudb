//! Regression: values must never be parsed as query syntax.
use siafudb_core::SiafuDB;

#[test]
fn kv_values_with_quotes_and_backslashes_round_trip() {
    let db = SiafuDB::in_memory().unwrap();
    db.execute("CREATE (:Victim {secret: 'keep-me'})").unwrap();
    for v in [
        r"x\' }) WITH n MATCH (v:Victim) DETACH DELETE v //",
        r"\\'",
        "it's fine",
        "'; MATCH (v:Victim) DETACH DELETE v; //",
    ] {
        db.kv_set("k", v).unwrap();
        assert_eq!(db.kv_get("k").unwrap().as_deref(), Some(v));
        db.kv_set(v, "val").unwrap();
        assert_eq!(db.kv_get(v).unwrap().as_deref(), Some("val"));
    }
    let victims = db.query("MATCH (v:Victim) RETURN v.secret").unwrap();
    assert_eq!(victims.rows.len(), 1, "Victim node must survive");
}

#[test]
fn doc_identifiers_are_validated_and_values_bound() {
    let db = SiafuDB::in_memory().unwrap();
    assert!(
        db.doc_insert("a) DETACH DELETE (x", serde_json::json!({"a": 1}))
            .is_err()
    );
    assert!(
        db.doc_insert("notes", serde_json::json!({"a b": 1}))
            .is_err()
    );
    db.doc_insert("notes", serde_json::json!({"text": "it's \\' ok", "n": 3}))
        .unwrap();
    let mut f = serde_json::Map::new();
    f.insert("n".into(), 3.into());
    assert_eq!(db.doc_find("notes", &f).unwrap().rows.len(), 1);
}

#[test]
fn change_log_keeps_params_separate_from_query_text() {
    let db = SiafuDB::in_memory().unwrap();
    db.kv_set("k", "a'b").unwrap();
    let e = db.change_log().lock().unwrap().snapshot().pop().unwrap();
    assert!(!e.query.contains("a'b"));
    assert_eq!(e.params["value"], "a'b");
}

#[test]
fn documents_cannot_collide_with_kv_or_spoof_ids() {
    let db = SiafuDB::in_memory().unwrap();
    db.kv_set("session:token", "real-secret").unwrap();
    // A collection named "KV" must not alias the internal KV label.
    assert!(
        db.doc_find("KV", &serde_json::Map::new())
            .unwrap()
            .rows
            .is_empty()
    );
    db.doc_insert(
        "KV",
        serde_json::json!({"key": "session:token", "value": "attacker"}),
    )
    .unwrap();
    assert_eq!(
        db.kv_get("session:token").unwrap().as_deref(),
        Some("real-secret")
    );
    // Reserved keys cannot be set by callers.
    assert!(
        db.doc_insert("notes", serde_json::json!({"_id": "spoofed"}))
            .is_err()
    );
}

#[test]
fn bound_params_reject_ambiguous_types() {
    let db = SiafuDB::in_memory().unwrap();
    let mut p = serde_json::Map::new();
    p.insert("x".into(), serde_json::json!(["admin"]));
    assert!(
        db.query_with_params("MATCH (n) WHERE n.a = $x RETURN n", p)
            .is_err()
    );
    let mut p = serde_json::Map::new();
    p.insert("x".into(), serde_json::json!(u64::MAX));
    assert!(
        db.query_with_params("MATCH (n) WHERE n.a = $x RETURN n", p)
            .is_err()
    );
}
