use siafudb_ffi::{Database, DbError, Value};
use std::collections::HashMap;

fn p<const N: usize>(a: [(&str, Value); N]) -> HashMap<String, Value> {
    a.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
fn t(s: &str) -> Value {
    Value::Text { v: s.into() }
}

#[test]
fn typed_round_trip() {
    let db = Database::in_memory().unwrap();
    db.execute(
        "CREATE (:T {i: $i, f: $f, b: $b, s: $s})".into(),
        p([
            ("i", Value::Int { v: -42 }),
            ("f", Value::Float { v: 2.5 }),
            ("b", Value::Bool { v: true }),
            ("s", t("héllo")),
        ]),
    )
    .unwrap();
    let r = db
        .query(
            "MATCH (x:T) RETURN x.i, x.f, x.b, x.s".into(),
            HashMap::new(),
        )
        .unwrap();
    assert_eq!(r.columns.len(), 4);
    assert_eq!(
        r.rows,
        vec![vec![
            Value::Int { v: -42 },
            Value::Float { v: 2.5 },
            Value::Bool { v: true },
            t("héllo"),
        ]]
    );
}

#[test]
fn hostile_strings_are_inert() {
    let db = Database::in_memory().unwrap();
    db.execute(
        "CREATE (:Victim {secret: 'keep-me'})".into(),
        HashMap::new(),
    )
    .unwrap();
    let hostile = [
        r"x\' }) WITH n MATCH (v:Victim) DETACH DELETE v //",
        "'; MATCH (v:Victim) DETACH DELETE v; //",
        "\" OR 1=1 //",
        "emoji \u{1F41C} and \u{202e}rtl",
        "line1\nline2\ttab",
    ];
    for h in hostile {
        db.execute("CREATE (:Note {text: $t})".into(), p([("t", t(h))]))
            .unwrap();
        let r = db
            .query(
                "MATCH (n:Note {text: $t}) RETURN n.text".into(),
                p([("t", t(h))]),
            )
            .unwrap();
        assert!(r.rows.contains(&vec![t(h)]), "round trip failed for {h:?}");
        db.kv_set(h.into(), h.into()).unwrap();
        assert_eq!(db.kv_get(h.into()).unwrap().as_deref(), Some(h));
    }
    let v = db
        .query("MATCH (v:Victim) RETURN v.secret".into(), HashMap::new())
        .unwrap();
    assert_eq!(v.rows, vec![vec![t("keep-me")]]);
}

#[test]
fn kv_and_delete() {
    let db = Database::in_memory().unwrap();
    assert_eq!(db.kv_get("a".into()).unwrap(), None);
    db.kv_set("a".into(), "1".into()).unwrap();
    db.kv_set("a".into(), "2".into()).unwrap();
    assert_eq!(db.kv_get("a".into()).unwrap().as_deref(), Some("2"));
    db.kv_delete("a".into()).unwrap();
    assert_eq!(db.kv_get("a".into()).unwrap(), None);
}

#[test]
fn errors_are_distinct() {
    let db = Database::in_memory().unwrap();
    assert!(matches!(
        db.query("THIS IS NOT A QUERY".into(), HashMap::new()),
        Err(DbError::Query { .. })
    ));
    assert!(matches!(
        db.execute("RETURN $x".into(), p([("x", Value::Bytes { v: vec![1] })])),
        Err(DbError::InvalidArgument { .. })
    ));
    assert!(matches!(
        db.execute("RETURN $x".into(), p([("x", Value::Float { v: f64::NAN })])),
        Err(DbError::InvalidArgument { .. })
    ));
    assert!(matches!(
        Database::from_snapshot(vec![1, 2, 3, 4]),
        Err(DbError::Corrupt { .. })
    ));
    assert!(matches!(
        Database::open(String::new()),
        Err(DbError::InvalidArgument { .. })
    ));
}

#[test]
fn snapshot_round_trip_and_changes() {
    let db = Database::in_memory().unwrap();
    db.kv_set("k".into(), "v".into()).unwrap();
    db.execute(
        "CREATE (:P {n: $n})".into(),
        p([("n", Value::Int { v: 7 })]),
    )
    .unwrap();
    let all = db.changes_since(0);
    assert_eq!(all.len(), 2);
    assert_eq!(db.changes_since(all[0].sequence).len(), 1);

    db.set_mutation_tracking(false);
    db.kv_set("quiet".into(), "x".into()).unwrap();
    assert_eq!(db.changes_since(0).len(), 2);
    db.set_mutation_tracking(true);

    let snap = db.export_snapshot().unwrap();
    let db2 = Database::from_snapshot(snap).unwrap();
    assert_eq!(db2.kv_get("k".into()).unwrap().as_deref(), Some("v"));
    let r = db2
        .query("MATCH (x:P) RETURN x.n".into(), HashMap::new())
        .unwrap();
    assert_eq!(r.rows, vec![vec![Value::Int { v: 7 }]]);
}

#[test]
fn file_backed_open_and_lock() {
    let dir = std::env::temp_dir().join(format!("siafu-ffi-{}.siafu", std::process::id()));
    let path = dir.to_string_lossy().to_string();
    let db = Database::open(path.clone()).unwrap();
    db.kv_set("k".into(), "persist".into()).unwrap();
    let second = Database::open(path);
    eprintln!("second open: {:?}", second.as_ref().err());
    assert!(matches!(second, Err(DbError::Locked { .. })));
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn shared_across_threads() {
    let db = Database::in_memory().unwrap();
    let hs: Vec<_> = (0..4)
        .map(|i| {
            let db = db.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    db.kv_set(format!("k{i}-{j}"), "v".into()).unwrap();
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(db.changes_since(0).len(), 40);
}
