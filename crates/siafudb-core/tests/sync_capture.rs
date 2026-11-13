// Copyright (C) 2026 The Bundu Foundation
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Structured change capture and apply (`siafudb_core::sync`): what local
//! writes did on one database, applied to another, gives the same graph.

use siafudb_core::sync::{gid_from_hex, gid_to_hex};
use siafudb_core::{ChangeValue, EngineValue, GID_PROPERTY, Gid, GraphChange, SiafuDB, SiafuError};
use std::collections::BTreeMap;

type Props = BTreeMap<String, ChangeValue>;

/// A graph in canonical form: every node and edge keyed by its global id, so
/// two databases can be compared whatever their local ids are.
#[derive(Debug, PartialEq, Default)]
struct Dump {
    /// gid -> (sorted labels, properties without `_gid`)
    nodes: BTreeMap<String, (Vec<String>, Props)>,
    /// gid -> (source gid, destination gid, type, properties without `_gid`)
    edges: BTreeMap<String, (String, String, String, Props)>,
}

fn text(v: &EngineValue) -> String {
    match v {
        EngineValue::String(s) => s.to_string(),
        other => panic!("expected a string (is an entity missing its gid?), got {other:?}"),
    }
}

fn props(v: &EngineValue) -> Props {
    let EngineValue::Map(map) = v else {
        panic!("expected a property map, got {v:?}");
    };
    map.iter()
        .filter(|(k, _)| k.as_str() != GID_PROPERTY)
        .map(|(k, v)| (k.as_str().to_string(), ChangeValue::from_engine(v).unwrap()))
        .collect()
}

/// Canonical dump keyed by gid. Panics if any node or edge has no gid.
fn dump(db: &SiafuDB) -> Dump {
    let none = serde_json::Map::new;
    let mut out = Dump::default();
    let nodes = db
        .query_typed_with_params("MATCH (n) RETURN n._gid, labels(n), properties(n)", none())
        .unwrap();
    for row in &nodes.rows {
        let EngineValue::List(labels) = &row[1] else {
            panic!("labels: {:?}", row[1]);
        };
        let mut labels: Vec<String> = labels.iter().map(text).collect();
        labels.sort();
        let old = out.nodes.insert(text(&row[0]), (labels, props(&row[2])));
        assert!(old.is_none(), "duplicate node gid");
    }
    let edges = db
        .query_typed_with_params(
            "MATCH (a)-[r]->(b) RETURN r._gid, a._gid, b._gid, type(r), properties(r)",
            none(),
        )
        .unwrap();
    for row in &edges.rows {
        let old = out.edges.insert(
            text(&row[0]),
            (text(&row[1]), text(&row[2]), text(&row[3]), props(&row[4])),
        );
        assert!(old.is_none(), "duplicate edge gid");
    }
    out
}

fn capturing() -> SiafuDB {
    let db = SiafuDB::in_memory().unwrap();
    db.enable_change_capture().unwrap();
    db
}

fn run(db: &SiafuDB, script: &[&str]) {
    for q in script {
        db.execute(q).unwrap_or_else(|e| panic!("{q}: {e}"));
    }
}

/// A script that exercises every kind of write.
fn varied_script(a: &SiafuDB) {
    run(
        a,
        &[
            "CREATE (:Person:Admin {name: 'Amara', age: 31, score: 1.5, active: true, tags: ['a', 'b']})",
            "CREATE (:Person {name: 'Tatenda', city: 'Harare', born: date('1990-04-16')})",
            "CREATE (:Person:Guest {name: 'Kofi'})",
            "CREATE (:City {name: 'Accra', pop: 2500000})",
            "MATCH (a:Person {name: 'Amara'}), (t:Person {name: 'Tatenda'}) CREATE (a)-[:KNOWS {since: 2024, close: true}]->(t)",
            "MATCH (a:Person {name: 'Amara'}), (c:City {name: 'Accra'}) CREATE (a)-[:LIVES_IN]->(c)",
            "MATCH (k:Person {name: 'Kofi'}), (a:Person {name: 'Amara'}) CREATE (k)-[:KNOWS {since: 2020}]->(a), (k)-[:FOLLOWS]->(a)",
            // properties: overwrite, add, remove, set to null
            "MATCH (a:Person {name: 'Amara'}) SET a.age = 32, a.city = 'Accra'",
            "MATCH (a:Person {name: 'Amara'}) REMOVE a.active",
            "MATCH (t:Person {name: 'Tatenda'}) SET t.city = null",
            "MATCH (t:Person {name: 'Tatenda'}) SET t += {nick: 'T', level: 3}",
            // labels
            "MATCH (t:Person {name: 'Tatenda'}) SET t:Admin:Staff",
            "MATCH (a:Person {name: 'Amara'}) REMOVE a:Admin",
            // edge properties, edge delete
            "MATCH (:Person {name: 'Amara'})-[r:KNOWS]->() SET r.since = 2025, r.weight = 0.25",
            "MATCH (:Person {name: 'Amara'})-[r:KNOWS]->() REMOVE r.close",
            "MATCH (:Person {name: 'Kofi'})-[r:FOLLOWS]->() DELETE r",
            // a node with edges goes away
            "MATCH (k:Person {name: 'Kofi'}) DETACH DELETE k",
            // created and deleted again
            "CREATE (:Temp {n: 1})",
            "MATCH (t:Temp) DELETE t",
        ],
    );
    // bound parameters (a float stays a float, unlike a literal)
    let mut p = serde_json::Map::new();
    p.insert("ratio".into(), serde_json::json!(0.75));
    p.insert("name".into(), serde_json::json!("O'Brien \"q\""));
    a.execute_with_params("CREATE (:Person {name: $name, ratio: $ratio})", p)
        .unwrap();
    // KV and document helpers
    a.kv_set("session:token", "abc").unwrap();
    a.kv_set("session:token", "def").unwrap();
    a.kv_set("gone", "x").unwrap();
    a.kv_delete("gone").unwrap();
    a.doc_insert(
        "messages",
        serde_json::json!({"from": "Amara", "text": "Hello!", "n": 3, "nested": {"a": [1, 2]}}),
    )
    .unwrap();
}

#[test]
fn capture_is_off_by_default() {
    let db = SiafuDB::in_memory().unwrap();
    assert!(!db.change_capture_enabled());
    db.execute("CREATE (:Person {name: 'Amara'})").unwrap();
    assert!(db.take_changes().unwrap().is_empty());
    let r = db.query("MATCH (n) RETURN n._gid").unwrap();
    assert_eq!(r.rows, vec![vec![serde_json::Value::Null]]);
    // A plain snapshot is the engine's, with no trailer.
    let snap = db.export_snapshot().unwrap();
    assert!(
        !SiafuDB::from_snapshot(&snap)
            .unwrap()
            .change_capture_enabled()
    );
}

#[test]
fn a_to_b_varied_script() {
    let (a, b) = (capturing(), capturing());
    varied_script(&a);
    let changes = a.take_changes().unwrap();
    assert!(!changes.is_empty());
    // Reserved properties never leave the database.
    for change in &changes {
        if let GraphChange::NodePut { set, unset, .. } | GraphChange::EdgePut { set, unset, .. } =
            change
        {
            assert!(set.iter().all(|(k, _)| k != GID_PROPERTY));
            assert!(unset.iter().all(|k| k != GID_PROPERTY));
        }
    }
    b.apply_changes(&changes).unwrap();

    let (da, db_) = (dump(&a), dump(&b));
    assert_eq!(da, db_);
    // The script's survivors: Amara, Tatenda, Accra, O'Brien, a KV pair and
    // a document; two edges.
    assert_eq!(da.nodes.len(), 6);
    assert_eq!(da.edges.len(), 2);
    // Taken once.
    assert!(a.take_changes().unwrap().is_empty());
    // The helpers read the same on both sides.
    assert_eq!(b.kv_get("session:token").unwrap().as_deref(), Some("def"));
    assert_eq!(b.kv_get("gone").unwrap(), None);
    let docs = b.doc_find("messages", &serde_json::Map::new()).unwrap();
    assert_eq!(docs.rows.len(), 1);
}

#[test]
fn reapply_is_idempotent() {
    let (a, b) = (capturing(), capturing());
    varied_script(&a);
    let changes = a.take_changes().unwrap();
    b.apply_changes(&changes).unwrap();
    let once = dump(&b);
    b.apply_changes(&changes).unwrap();
    assert_eq!(dump(&b), once);
    assert_eq!(once, dump(&a));
    // Also in pieces, and one change at a time.
    let c = capturing();
    for change in &changes {
        c.apply_changes(std::slice::from_ref(change)).unwrap();
        c.apply_changes(std::slice::from_ref(change)).unwrap();
    }
    assert_eq!(dump(&c), once);
    assert!(b.take_changes().unwrap().is_empty());
    assert!(c.take_changes().unwrap().is_empty());
}

#[test]
fn applied_changes_do_not_echo_and_later_local_writes_are_captured() {
    let (a, b) = (capturing(), capturing());
    varied_script(&a);
    b.apply_changes(&a.take_changes().unwrap()).unwrap();
    assert!(b.take_changes().unwrap().is_empty(), "apply must not echo");

    // B now writes to entities it only knows through apply.
    run(
        &b,
        &[
            "MATCH (a:Person {name: 'Amara'}) SET a.age = 40, a:Elder",
            "MATCH (:Person {name: 'Amara'})-[r:KNOWS]->() SET r.since = 1999",
            "MATCH (a:Person {name: 'Amara'}), (c:City) CREATE (c)-[:HOME_OF]->(a)",
            "MATCH (:Person)-[r:LIVES_IN]->() DELETE r",
            "MATCH (t:Person {name: 'Tatenda'}) DETACH DELETE t",
        ],
    );
    b.kv_set("session:token", "ghi").unwrap();
    let back = b.take_changes().unwrap();
    // The update of Amara names the gid A gave her.
    let amara = a
        .query("MATCH (a:Person {name: 'Amara'}) RETURN a._gid")
        .unwrap();
    let amara: Gid = gid_from_hex(amara.rows[0][0].as_str().unwrap()).unwrap();
    assert!(matches!(
        &back[0],
        GraphChange::NodePut { gid, labels_add, set, .. }
            if *gid == amara && labels_add.is_empty() && set.len() == 1
    ));
    a.apply_changes(&back).unwrap();
    assert_eq!(dump(&a), dump(&b));
    assert!(a.take_changes().unwrap().is_empty());
    assert_eq!(a.kv_get("session:token").unwrap().as_deref(), Some("ghi"));
}

#[test]
fn bidirectional_ping_pong() {
    let (a, b) = (capturing(), capturing());
    for round in 0..6 {
        run(
            &a,
            &[
                &format!("CREATE (:Ping {{round: {round}, who: 'a'}})"),
                "MATCH (p:Pong) SET p.seen_by_a = true",
            ],
        );
        b.apply_changes(&a.take_changes().unwrap()).unwrap();
        run(
            &b,
            &[
                &format!("CREATE (:Pong {{round: {round}, who: 'b'}})"),
                &format!(
                    "MATCH (i:Ping {{round: {round}}}), (o:Pong {{round: {round}}}) CREATE (i)-[:ANSWERED_BY {{round: {round}}}]->(o)"
                ),
                "MATCH (p:Ping) SET p.hits = 1",
            ],
        );
        if round == 3 {
            b.execute("MATCH (p:Ping {round: 1}) DETACH DELETE p")
                .unwrap();
        }
        a.apply_changes(&b.take_changes().unwrap()).unwrap();
        assert_eq!(dump(&a), dump(&b), "round {round}");
    }
    assert!(a.take_changes().unwrap().is_empty());
    assert!(b.take_changes().unwrap().is_empty());
    let d = dump(&a);
    assert_eq!((d.nodes.len(), d.edges.len()), (11, 5));
}

#[test]
fn snapshot_round_trip_in_the_middle() {
    let (a, b) = (capturing(), capturing());
    run(
        &a,
        &[
            "CREATE (:Person {name: 'Amara'})",
            "CREATE (:Person {name: 'Tatenda'})",
            "MATCH (a:Person {name: 'Amara'}), (t:Person {name: 'Tatenda'}) CREATE (a)-[:KNOWS]->(t)",
        ],
    );
    b.apply_changes(&a.take_changes().unwrap()).unwrap();
    // Written but not yet taken when the snapshot is made.
    a.execute("MATCH (a:Person {name: 'Amara'}) SET a.city = 'Accra'")
        .unwrap();
    let before = dump(&a);

    let a = SiafuDB::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
    assert!(a.change_capture_enabled(), "capture comes back on");
    assert_eq!(dump(&a), before, "gids and data are unchanged");

    run(
        &a,
        &[
            "MATCH (t:Person {name: 'Tatenda'}) SET t.city = 'Harare'",
            "MATCH (:Person)-[r:KNOWS]->() SET r.since = 2024",
            "CREATE (:Person {name: 'Kofi'})",
        ],
    );
    let changes = a.take_changes().unwrap();
    // Only what B has not seen: the queued write and the three new ones,
    // none of the writes taken before the snapshot.
    assert_eq!(changes.len(), 4, "{changes:#?}");
    b.apply_changes(&changes).unwrap();
    assert_eq!(dump(&a), dump(&b));

    // B round-trips too: what it applied is not reported after a reload,
    // and it still knows the gids (an update lands on the same node).
    let b = SiafuDB::from_snapshot(&b.export_snapshot().unwrap()).unwrap();
    assert!(b.take_changes().unwrap().is_empty());
    a.execute("MATCH (k:Person {name: 'Kofi'}) SET k.age = 7")
        .unwrap();
    b.apply_changes(&a.take_changes().unwrap()).unwrap();
    assert_eq!(dump(&a), dump(&b));
    assert_eq!(dump(&b).nodes.len(), 3);

    // And an idle round trip reports nothing.
    let a = SiafuDB::from_snapshot(&a.export_snapshot().unwrap()).unwrap();
    assert!(a.take_changes().unwrap().is_empty());
}

#[test]
fn unknown_endpoint_is_a_distinct_error_and_applies_nothing() {
    let b = capturing();
    let (node, ghost, edge) = ([1u8; 16], [2u8; 16], [3u8; 16]);
    let changes = vec![
        GraphChange::NodePut {
            gid: node,
            labels_add: vec!["Person".into()],
            labels_remove: vec![],
            set: vec![("name".into(), ChangeValue::String("Amara".into()))],
            unset: vec![],
        },
        GraphChange::EdgePut {
            gid: edge,
            src: node,
            dst: ghost,
            edge_type: "KNOWS".into(),
            set: vec![],
            unset: vec![],
        },
    ];
    match b.apply_changes(&changes) {
        Err(SiafuError::UnknownEndpoint { edge: e, endpoint }) => {
            assert_eq!((e, endpoint), (edge, ghost));
        }
        other => panic!("expected UnknownEndpoint, got {other:?}"),
    }
    // The whole batch was rolled back, the node included.
    assert_eq!(dump(&b), Dump::default());
    // And the node's gid was not left behind: the batch works once complete.
    let mut fixed = changes.clone();
    fixed.insert(
        1,
        GraphChange::NodePut {
            gid: ghost,
            labels_add: vec![],
            labels_remove: vec![],
            set: vec![],
            unset: vec![],
        },
    );
    b.apply_changes(&fixed).unwrap();
    let d = dump(&b);
    assert_eq!((d.nodes.len(), d.edges.len()), (2, 1));
    assert_eq!(d.edges[&gid_to_hex(&edge)].1, gid_to_hex(&ghost));
    assert!(b.take_changes().unwrap().is_empty());
}

#[test]
fn malformed_changes_are_refused() {
    let b = capturing();
    let put = |set: Vec<(String, ChangeValue)>| GraphChange::NodePut {
        gid: [9; 16],
        labels_add: vec![],
        labels_remove: vec![],
        set,
        unset: vec![],
    };
    let reserved = put(vec![(GID_PROPERTY.into(), ChangeValue::String("x".into()))]);
    assert!(matches!(
        b.apply_changes(&[reserved]),
        Err(SiafuError::InvalidChange(_))
    ));
    // An edge cannot move to other endpoints under the same gid.
    let node = |gid| GraphChange::NodePut {
        gid,
        labels_add: vec![],
        labels_remove: vec![],
        set: vec![],
        unset: vec![],
    };
    let edge = |src, dst| GraphChange::EdgePut {
        gid: [7; 16],
        src,
        dst,
        edge_type: "T".into(),
        set: vec![],
        unset: vec![],
    };
    b.apply_changes(&[node([1; 16]), node([2; 16]), edge([1; 16], [2; 16])])
        .unwrap();
    assert!(matches!(
        b.apply_changes(&[edge([2; 16], [1; 16])]),
        Err(SiafuError::InvalidChange(_))
    ));
    // Deletes of things never seen are fine.
    b.apply_changes(&[
        GraphChange::EdgeDel { gid: [42; 16] },
        GraphChange::NodeDel { gid: [43; 16] },
    ])
    .unwrap();
    assert_eq!(dump(&b).nodes.len(), 2);
}

#[test]
fn every_value_shape_survives() {
    let (a, b) = (capturing(), capturing());
    // Shapes Cypher literals cannot write arrive through apply; A then
    // re-captures them when a local write copies them.
    let mut nested = BTreeMap::new();
    nested.insert(
        "k".to_string(),
        ChangeValue::List(vec![ChangeValue::I64(1)]),
    );
    let values = vec![
        ("b".to_string(), ChangeValue::Bool(false)),
        (
            "bytes".to_string(),
            ChangeValue::Bytes(vec![0, 1, 254, 255]),
        ),
        ("f".to_string(), ChangeValue::F64(-0.5)),
        ("i".to_string(), ChangeValue::I64(i64::MIN)),
        (
            "l".to_string(),
            ChangeValue::List(vec![ChangeValue::String("x".into()), ChangeValue::I64(2)]),
        ),
        ("m".to_string(), ChangeValue::Map(nested)),
        (
            "s".to_string(),
            ChangeValue::String("héllo \u{1F41C}".into()),
        ),
    ];
    a.apply_changes(&[GraphChange::NodePut {
        gid: [5; 16],
        labels_add: vec!["V".into()],
        labels_remove: vec![],
        set: values.clone(),
        unset: vec![],
    }])
    .unwrap();
    run(
        &a,
        &[
            "MATCH (v:V) CREATE (c:Copy) SET c.bytes = v.bytes, c.f = v.f, c.i = v.i, c.l = v.l, c.m = v.m, c.s = v.s, c.b = v.b",
            // Engine types with no direct shape travel in the lossless encoding.
            "CREATE (:T {d: date('2024-02-29'), dec: 12.50, dur: duration('P1DT2H'), ts: datetime('2024-02-29T10:00:00Z')})",
        ],
    );
    let changes = a.take_changes().unwrap();
    let GraphChange::NodePut { set, .. } = &changes[0] else {
        panic!("{changes:?}");
    };
    assert_eq!(set, &values, "captured values equal the applied ones");
    let GraphChange::NodePut { set, .. } = &changes[1] else {
        panic!("{changes:?}");
    };
    for (key, value) in set {
        let ChangeValue::Map(map) = value else {
            panic!("{key} should be encoded, got {value:?}");
        };
        assert_eq!(map.keys().collect::<Vec<_>>(), ["$engine"]);
    }
    // B gets the first node from A's peer too, then A's own writes.
    b.apply_changes(&[GraphChange::NodePut {
        gid: [5; 16],
        labels_add: vec!["V".into()],
        labels_remove: vec![],
        set: values,
        unset: vec![],
    }])
    .unwrap();
    b.apply_changes(&changes).unwrap();
    assert_eq!(dump(&a), dump(&b));
    // The encoded values are the engine's own types again on B.
    let r = b
        .query_typed_with_params("MATCH (t:T) RETURN t.d, t.dec", serde_json::Map::new())
        .unwrap();
    assert!(matches!(r.rows[0][0], EngineValue::Date(_)), "{:?}", r.rows);
    assert!(
        matches!(r.rows[0][1], EngineValue::Decimal(_)),
        "{:?}",
        r.rows
    );
}

#[test]
fn entities_written_before_capture_are_reported_whole_when_touched() {
    let a = SiafuDB::in_memory().unwrap();
    run(
        &a,
        &[
            "CREATE (:Person:Admin {name: 'Amara', age: 31})",
            "CREATE (:Person {name: 'Tatenda'})",
            "CREATE (:Person {name: 'Untouched'})",
            "MATCH (a:Person {name: 'Amara'}), (t:Person {name: 'Tatenda'}) CREATE (a)-[:KNOWS {since: 2024}]->(t)",
        ],
    );
    a.enable_change_capture().unwrap();
    assert!(a.take_changes().unwrap().is_empty());
    run(
        &a,
        &[
            "MATCH (:Person)-[r:KNOWS]->() SET r.since = 2025",
            "MATCH (a:Person {name: 'Amara'}) SET a.age = 32",
        ],
    );
    let b = capturing();
    b.apply_changes(&a.take_changes().unwrap()).unwrap();
    let d = dump(&b);
    assert_eq!((d.nodes.len(), d.edges.len()), (2, 1));
    let amara = d
        .nodes
        .values()
        .find(|(_, p)| p["name"] == ChangeValue::String("Amara".into()))
        .unwrap();
    assert_eq!(amara.0, ["Admin", "Person"]);
    assert_eq!(amara.1["age"], ChangeValue::I64(32));
    assert_eq!(
        d.edges.values().next().unwrap().3["since"],
        ChangeValue::I64(2025)
    );
}

#[test]
fn the_whole_graph_is_reported_once_as_puts() {
    // Data written before capture, and data captured but never taken.
    let a = SiafuDB::in_memory().unwrap();
    varied_script(&a);
    a.enable_change_capture().unwrap();
    run(
        &a,
        &[
            "CREATE (:Late {name: 'captured'})",
            "CREATE (:Gone {name: 'never seen'})",
            "MATCH (g:Gone) DELETE g",
        ],
    );

    let graph = a.take_graph_as_changes().unwrap();
    // Nodes first, puts only, every gid once.
    let first_edge = graph
        .iter()
        .position(|c| matches!(c, GraphChange::EdgePut { .. }))
        .unwrap();
    assert!(
        graph[..first_edge]
            .iter()
            .all(|c| matches!(c, GraphChange::NodePut { .. }))
    );
    assert!(
        graph[first_edge..]
            .iter()
            .all(|c| matches!(c, GraphChange::EdgePut { .. }))
    );
    let mut gids: Vec<Gid> = graph
        .iter()
        .map(|c| match c {
            GraphChange::NodePut { gid, .. } | GraphChange::EdgePut { gid, .. } => *gid,
            other => panic!("not a put: {other:?}"),
        })
        .collect();
    let count = gids.len();
    gids.sort_unstable();
    gids.dedup();
    assert_eq!(gids.len(), count);

    // An empty database that applies it holds the same graph.
    let b = capturing();
    b.apply_changes(&graph).unwrap();
    assert_eq!(dump(&a), dump(&b));
    assert!(!dump(&a).nodes.is_empty() && !dump(&a).edges.is_empty());

    // The queue was replaced: nothing from before comes out again, later
    // writes do, and a second call names the same gids.
    assert!(a.take_changes().unwrap().is_empty());
    run(&a, &["MATCH (l:Late) SET l.n = 1"]);
    let later = a.take_changes().unwrap();
    assert_eq!(later.len(), 1);
    b.apply_changes(&later).unwrap();
    assert_eq!(dump(&a), dump(&b));
    let again = a.take_graph_as_changes().unwrap();
    assert_eq!(again.len(), count);
    b.apply_changes(&again).unwrap();
    assert_eq!(dump(&a), dump(&b));

    // On a database without capture it turns capture on; an empty graph is
    // an empty list.
    let c = SiafuDB::in_memory().unwrap();
    assert!(c.take_graph_as_changes().unwrap().is_empty());
    assert!(c.change_capture_enabled());
}

#[test]
fn user_writes_to_the_reserved_property_do_not_break_ids() {
    let (a, b) = (capturing(), capturing());
    run(&a, &["CREATE (:Person {name: 'Amara'})"]);
    let gid = dump(&a).nodes.keys().next().unwrap().clone();
    run(
        &a,
        &[
            "MATCH (a:Person) SET a._gid = 'mine'",
            "MATCH (a:Person) REMOVE a._gid",
            "MATCH (a:Person) SET a.age = 1",
        ],
    );
    b.apply_changes(&a.take_changes().unwrap()).unwrap();
    assert_eq!(dump(&a).nodes.keys().next(), Some(&gid));
    assert_eq!(dump(&a), dump(&b));
}

#[cfg(feature = "native")]
#[test]
fn persistent_database_keeps_its_gids_across_reopen() {
    let dir = std::env::temp_dir().join(format!("siafu-sync-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("db.siafu");
    let b = capturing();
    let before = {
        let a = SiafuDB::open(&path).unwrap();
        a.enable_change_capture().unwrap();
        run(
            &a,
            &[
                "CREATE (:Person {name: 'Amara'})",
                "CREATE (:Person {name: 'Tatenda'})",
                "MATCH (a:Person {name: 'Amara'}), (t:Person {name: 'Tatenda'}) CREATE (a)-[:KNOWS]->(t)",
            ],
        );
        b.apply_changes(&a.take_changes().unwrap()).unwrap();
        dump(&a)
    };
    let a = SiafuDB::open(&path).unwrap();
    a.enable_change_capture().unwrap();
    assert_eq!(dump(&a), before);
    assert!(a.take_changes().unwrap().is_empty());
    run(
        &a,
        &[
            "MATCH (a:Person {name: 'Amara'}) SET a.age = 3",
            "MATCH (:Person)-[r:KNOWS]->() SET r.w = 1",
            "MATCH (t:Person {name: 'Tatenda'}) DETACH DELETE t",
        ],
    );
    b.apply_changes(&a.take_changes().unwrap()).unwrap();
    assert_eq!(dump(&a), dump(&b));
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Small deterministic generator (no extra dependency).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// One random write against `db`. Nodes are addressed by a stable `k`.
fn random_write(db: &SiafuDB, rng: &mut Rng, next_k: &mut u64) {
    const LABELS: [&str; 4] = ["A", "B", "C", "D"];
    const KEYS: [&str; 4] = ["p", "q", "r", "s"];
    const TYPES: [&str; 3] = ["X", "Y", "Z"];
    let k = rng.below((*next_k).max(1));
    let k2 = rng.below((*next_k).max(1));
    let key = KEYS[rng.below(4) as usize];
    let label = LABELS[rng.below(4) as usize];
    let ty = TYPES[rng.below(3) as usize];
    let value = match rng.below(5) {
        0 => format!("{}", rng.below(1000) as i64 - 500),
        1 => format!("'s{}'", rng.below(50)),
        2 => "true".to_string(),
        3 => format!("{}.{}", rng.below(100), rng.below(100)),
        _ => format!("[{}, 's{}']", rng.below(10), rng.below(10)),
    };
    let q = match rng.below(12) {
        0 | 1 => {
            *next_k += 1;
            format!("CREATE (:N:{label} {{k: {}, {key}: {value}}})", *next_k - 1)
        }
        2 | 3 => format!("MATCH (n:N {{k: {k}}}) SET n.{key} = {value}"),
        4 => format!("MATCH (n:N {{k: {k}}}) REMOVE n.{key}"),
        5 => format!("MATCH (n:N {{k: {k}}}) SET n:{label}"),
        6 => format!("MATCH (n:N {{k: {k}}}) REMOVE n:{label}"),
        7 | 8 => format!(
            "MATCH (a:N {{k: {k}}}), (b:N {{k: {k2}}}) CREATE (a)-[:{ty} {{{key}: {value}}}]->(b)"
        ),
        9 => format!("MATCH (:N {{k: {k}}})-[e:{ty}]->() SET e.{key} = {value}"),
        10 => format!("MATCH (:N {{k: {k}}})-[e:{ty}]->() DELETE e"),
        _ => format!("MATCH (n:N {{k: {k}}}) DETACH DELETE n"),
    };
    db.execute(&q).unwrap_or_else(|e| panic!("{q}: {e}"));
}

#[test]
fn randomised_scripts_converge() {
    for seed in 1..=12u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (a, b, c) = (capturing(), capturing(), capturing());
        let mut next_k = 0;
        let mut all = Vec::new();
        for step in 0..150 {
            random_write(&a, &mut rng, &mut next_k);
            // Sync at random points, so batches of every size are applied.
            if rng.below(7) == 0 {
                let changes = a.take_changes().unwrap();
                b.apply_changes(&changes).unwrap();
                all.extend(changes);
                assert_eq!(dump(&a), dump(&b), "seed {seed} step {step}");
            }
        }
        let changes = a.take_changes().unwrap();
        b.apply_changes(&changes).unwrap();
        all.extend(changes);
        assert_eq!(dump(&a), dump(&b), "seed {seed}");
        assert!(b.take_changes().unwrap().is_empty());
        // The whole history in one batch, twice, gives the same graph.
        c.apply_changes(&all).unwrap();
        c.apply_changes(&all).unwrap();
        assert_eq!(dump(&a), dump(&c), "seed {seed} (replay)");
    }
}

/// Timing on a 10,000-node graph. Run with
/// `cargo test -p siafudb-core --release --test sync_capture -- --ignored --nocapture`.
#[test]
#[ignore = "timing; run explicitly"]
fn timing_10k_nodes() {
    use std::time::Instant;
    const N: usize = 10_000;
    let (a, b) = (capturing(), capturing());

    let t = Instant::now();
    for chunk in 0..N / 100 {
        a.execute(&format!(
            "UNWIND range({}, {}) AS i CREATE (:Item {{k: i, name: 'item', score: 1}})",
            chunk * 100,
            chunk * 100 + 99
        ))
        .unwrap();
    }
    let create = t.elapsed();

    let t = Instant::now();
    let changes = a.take_changes().unwrap();
    let take = t.elapsed();
    assert_eq!(changes.len(), N);

    let t = Instant::now();
    b.apply_changes(&changes).unwrap();
    let apply_create = t.elapsed();

    // Updates against the 10,000 existing nodes: one batch, then one by one
    // (each call is its own transaction and gid lookup).
    let updates: Vec<GraphChange> = changes
        .iter()
        .map(|c| match c {
            GraphChange::NodePut { gid, .. } => GraphChange::NodePut {
                gid: *gid,
                labels_add: vec!["Seen".into()],
                labels_remove: vec![],
                set: vec![("score".into(), ChangeValue::I64(2))],
                unset: vec!["name".into()],
            },
            other => other.clone(),
        })
        .collect();
    let t = Instant::now();
    b.apply_changes(&updates).unwrap();
    let apply_update = t.elapsed();
    let t = Instant::now();
    for change in updates.iter().take(1000) {
        b.apply_changes(std::slice::from_ref(change)).unwrap();
    }
    let single = t.elapsed();

    // A ring of edges, and the cost of capturing single local writes.
    let gids: Vec<Gid> = changes
        .iter()
        .map(|c| match c {
            GraphChange::NodePut { gid, .. } => *gid,
            _ => unreachable!(),
        })
        .collect();
    let edges: Vec<GraphChange> = (0..N)
        .map(|i| {
            let mut gid = [0u8; 16];
            gid[..8].copy_from_slice(&(i as u64).to_le_bytes());
            GraphChange::EdgePut {
                gid,
                src: gids[i],
                dst: gids[(i + 1) % N],
                edge_type: "NEXT".into(),
                set: vec![("i".into(), ChangeValue::I64(i as i64))],
                unset: vec![],
            }
        })
        .collect();
    let t = Instant::now();
    b.apply_changes(&edges).unwrap();
    let apply_edges = t.elapsed();

    let t = Instant::now();
    for i in 0..1000 {
        b.execute(&format!("MATCH (n:Item {{k: {i}}}) SET n.local = {i}"))
            .unwrap();
    }
    let local = t.elapsed();
    let plain = SiafuDB::in_memory().unwrap();
    plain
        .execute(&format!(
            "UNWIND range(0, {}) AS i CREATE (:Item {{k: i}})",
            N - 1
        ))
        .unwrap();
    let t = Instant::now();
    for i in 0..1000 {
        plain
            .execute(&format!("MATCH (n:Item {{k: {i}}}) SET n.local = {i}"))
            .unwrap();
    }
    let local_plain = t.elapsed();
    assert_eq!(b.take_changes().unwrap().len(), 1000);

    let t = Instant::now();
    let snap = b.export_snapshot().unwrap();
    let export = t.elapsed();
    let t = Instant::now();
    let b2 = SiafuDB::from_snapshot(&snap).unwrap();
    let import = t.elapsed();
    assert!(b2.change_capture_enabled());

    let t = Instant::now();
    b.apply_changes(&[GraphChange::NodeDel { gid: gids[0] }])
        .unwrap();
    let del = t.elapsed();

    println!("10k timing ({} nodes)", N);
    println!("  A: create via Cypher, captured  {create:?}");
    println!("  A: take_changes                 {take:?}");
    println!(
        "  B: apply 10k NodePut (create)   {apply_create:?}  ({:?}/change)",
        apply_create / N as u32
    );
    println!(
        "  B: apply 10k NodePut (update)   {apply_update:?}  ({:?}/change)",
        apply_update / N as u32
    );
    println!(
        "  B: apply 1k single-change calls {single:?}  ({:?}/call)",
        single / 1000
    );
    println!(
        "  B: apply 10k EdgePut (create)   {apply_edges:?}  ({:?}/change)",
        apply_edges / N as u32
    );
    println!(
        "  B: 1k local SET, capture on     {local:?}  ({:?}/write)",
        local / 1000
    );
    println!(
        "     1k local SET, capture off    {local_plain:?}  ({:?}/write)",
        local_plain / 1000
    );
    println!(
        "  B: export_snapshot              {export:?} ({} bytes)",
        snap.len()
    );
    println!("  B: from_snapshot + gid index    {import:?}");
    println!("  B: apply 1 NodeDel              {del:?}");
}
