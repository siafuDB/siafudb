use grafeo::GrafeoDB;
use grafeo::cdc::ChangeEvent;

fn dump(db: &GrafeoDB, from: &mut u64, what: &str) {
    let evs: Vec<ChangeEvent> = db
        .changes_between(grafeo_epoch(*from), grafeo_epoch(u64::MAX))
        .unwrap();
    println!("--- {what} (cur epoch {:?})", db.current_epoch());
    for e in &evs {
        println!(
            "  {:?} {:?} ep={:?} ts={:?} before={:?} after={:?} labels={:?} bl={:?} et={:?} src={:?} dst={:?} dw={:?}",
            e.entity_id, e.kind, e.epoch, e.timestamp, e.before, e.after, e.labels, e.before_labels, e.edge_type, e.src_id, e.dst_id, e.deleted_with
        );
        *from = e.epoch.0 + 1;
    }
}
fn grafeo_epoch(e: u64) -> grafeo::store::EpochId {
    grafeo::store::EpochId(e)
}

#[test]
fn explore() {
    let db = GrafeoDB::new_in_memory();
    db.set_cdc_enabled(true);
    let mut from = 0u64;
    let run = |q: &str| {
        let r = db.execute(q);
        if let Err(e) = &r {
            println!("ERR {q}: {e}");
        }
        r
    };
    run("CREATE (:Person:Admin {name: 'A', age: 3, f: 1.5, b: true, l: [1,2], n: null})").ok();
    dump(&db, &mut from, "create node");
    run("CREATE (:Person {name: 'B'})").ok();
    run("MATCH (a:Person {name:'A'}), (b:Person {name:'B'}) CREATE (a)-[:KNOWS {since: 2024}]->(b)").ok();
    dump(&db, &mut from, "create edge");
    run("MATCH (a:Person {name:'A'}) SET a.age = 4, a.city = 'X'").ok();
    dump(&db, &mut from, "set");
    run("MATCH (a:Person {name:'A'}) REMOVE a.city").ok();
    dump(&db, &mut from, "remove prop");
    run("MATCH (a:Person {name:'A'}) SET a.age = null").ok();
    dump(&db, &mut from, "set null");
    run("MATCH (a:Person {name:'A'}) SET a:Extra").ok();
    dump(&db, &mut from, "add label");
    run("MATCH (a:Person {name:'A'}) REMOVE a:Admin").ok();
    dump(&db, &mut from, "remove label");
    run("MATCH (:Person {name:'A'})-[r:KNOWS]->() SET r.since = 2025, r.w = 1.0").ok();
    dump(&db, &mut from, "edge set");
    run("MATCH (:Person {name:'A'})-[r:KNOWS]->() REMOVE r.w").ok();
    dump(&db, &mut from, "edge remove prop");
    run("MERGE (n:_KV {key: 'k'}) SET n.value = 'v'").ok();
    run("MERGE (n:_KV {key: 'k'}) SET n.value = 'v2'").ok();
    dump(&db, &mut from, "merge");
    run("MATCH (a:Person {name:'A'}) SET a += {x: 1, y: 2}").ok();
    dump(&db, &mut from, "set +=");
    run("MATCH (a:Person {name:'A'}) SET a = {name: 'A', z: 9}").ok();
    dump(&db, &mut from, "set =");
    run("MATCH (:Person {name:'A'})-[r:KNOWS]->() DELETE r").ok();
    dump(&db, &mut from, "edge delete");
    run("MATCH (a:Person {name:'A'}), (b:Person {name:'B'}) CREATE (a)-[:KNOWS {since: 1}]->(b), (b)-[:LIKES]->(a)").ok();
    run("MATCH (a:Person {name:'A'}) DETACH DELETE a").ok();
    dump(&db, &mut from, "detach delete");
    let r = run("MATCH (n) RETURN id(n), n.name, labels(n)").unwrap();
    println!("id(): {:?} {:?}", r.columns, r.rows());
    let r = run("MATCH (a)-[r]->(b) RETURN id(r), type(r), id(a), id(b)");
    println!("edge id(): {:?}", r.map(|r| r.rows().to_vec()));
    // values
    let s = db.session();
    let id = s.create_node_with_props(&["V"], [("bytes", grafeo::Value::Bytes(vec![1u8,2,3].into()))]).unwrap();
    println!("node {:?}", s.get_node(id));
    run("CREATE (:T {d: date('2024-01-02'), m: {a: 1}})").ok();
    dump(&db, &mut from, "values");
    // read query epoch
    let e0 = db.current_epoch();
    run("MATCH (n) RETURN count(n)").ok();
    println!("epoch before/after read: {:?} {:?}", e0, db.current_epoch());
    // uncaptured session
    let s = db.session_with_cdc(false);
    let mut s = s;
    s.begin_transaction().unwrap();
    let n = s.create_node(&["Hidden"]).unwrap();
    s.set_node_property(n, "p", 1i64.into()).unwrap();
    s.commit().unwrap();
    dump(&db, &mut from, "hidden (expect none)");
    // snapshot
    let snap = db.export_snapshot().unwrap();
    let db2 = GrafeoDB::import_snapshot(&snap).unwrap();
    println!("epoch orig {:?} imported {:?} cdc {}", db.current_epoch(), db2.current_epoch(), db2.is_cdc_enabled());
    let r = db2.execute("MATCH (n) RETURN id(n), labels(n)").unwrap();
    println!("imported: {:?}", r.rows());
    let r = db.execute("MATCH (n) RETURN id(n), labels(n)").unwrap();
    println!("orig: {:?}", r.rows());
    // create+delete same statement and failing statement
    run("CREATE (x:Tmp {a: 1}) DELETE x").ok();
    dump(&db, &mut from, "create+delete");
    println!("count {:?}", db.cdc_log().event_count());
}
