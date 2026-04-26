// Copyright (C) 2026 The Bundu Foundation
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Milestone 2 smoke test — change log captures mutations automatically.

use siafudb_core::SiafuDB;

#[test]
fn writes_append_entries_in_order() {
    let db = SiafuDB::in_memory().unwrap();

    db.execute("CREATE (:Person {name: 'Amara'})").unwrap();
    db.execute("CREATE (:Person {name: 'Tatenda'})").unwrap();
    db.execute("CREATE (:Person {name: 'Kofi'})").unwrap();

    let log = db.change_log();
    let snapshot = log.lock().unwrap().snapshot();

    assert_eq!(snapshot.len(), 3);
    assert_eq!(snapshot[0].sequence, 1);
    assert_eq!(snapshot[1].sequence, 2);
    assert_eq!(snapshot[2].sequence, 3);
    assert!(snapshot[0].query.contains("Amara"));
    assert!(snapshot[1].query.contains("Tatenda"));
    assert!(snapshot[2].query.contains("Kofi"));
}

#[test]
fn reads_do_not_append() {
    let db = SiafuDB::in_memory().unwrap();
    db.execute("CREATE (:Person {name: 'Amara'})").unwrap();

    db.query("MATCH (p:Person) RETURN p.name").unwrap();
    db.query("MATCH (p:Person) RETURN p.name").unwrap();

    let log = db.change_log();
    assert_eq!(
        log.lock().unwrap().len(),
        1,
        "only the CREATE should be logged"
    );
}

#[test]
fn disabling_mutation_tracking_skips_capture() {
    let mut db = SiafuDB::in_memory().unwrap();
    db.execute("CREATE (:Person {name: 'Amara'})").unwrap();

    db.set_mutation_tracking(false);
    db.execute("CREATE (:Person {name: 'Tatenda'})").unwrap();
    db.execute("CREATE (:Person {name: 'Kofi'})").unwrap();

    let log = db.change_log();
    let snapshot = log.lock().unwrap().snapshot();
    assert_eq!(snapshot.len(), 1, "only the first write was tracked");
    assert!(snapshot[0].query.contains("Amara"));
}

#[test]
fn since_returns_only_newer_entries() {
    let db = SiafuDB::in_memory().unwrap();
    db.execute("CREATE (:Person {name: 'Amara'})").unwrap();
    db.execute("CREATE (:Person {name: 'Tatenda'})").unwrap();
    db.execute("CREATE (:Person {name: 'Kofi'})").unwrap();

    let log = db.change_log();
    let after_first = log.lock().unwrap().since(1);

    assert_eq!(after_first.len(), 2);
    assert_eq!(after_first[0].sequence, 2);
    assert_eq!(after_first[1].sequence, 3);
}
