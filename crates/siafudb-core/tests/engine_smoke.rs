// Copyright (C) 2026 The Bundu Foundation
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Milestone 1 smoke test — engine runs end-to-end via GrafeoDB.
//!
//! Proves that opening a SiafuDB, writing nodes via Cypher, and
//! reading them back produces a non-empty result. Not a behavioural
//! contract — just evidence the wiring is live.

use siafudb_core::SiafuDB;

#[test]
fn cypher_round_trip_in_memory() {
    let db = SiafuDB::in_memory().expect("create in-memory db");

    db.execute("CREATE (:Person {name: 'Amara', city: 'Accra'})")
        .expect("CREATE should succeed");
    db.execute("CREATE (:Person {name: 'Tatenda', city: 'Harare'})")
        .expect("CREATE should succeed");

    let result = db
        .query("MATCH (p:Person) RETURN p.name")
        .expect("MATCH should succeed");

    assert_eq!(
        result.rows.len(),
        2,
        "expected two Person nodes, got {} (rows: {:?})",
        result.rows.len(),
        result.rows
    );
}
