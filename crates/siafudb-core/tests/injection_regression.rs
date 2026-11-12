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
