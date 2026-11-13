//! Two glued databases exchanging through an in-memory relay that stores
//! sealed items and nothing else.

use gsp_wire::Message;
use siafudb_core::SiafuDB;
use siafudb_gsp::{Error, SyncedDb};
use std::collections::BTreeSet;

const MARKER: &str = "PLAINTEXT-MARKER-PANGOLIN-4471";
/// Where the stand-in relay "lives"; the bearer token is bound to it.
const ORIGIN: &str = "https://relay.test";

/// What a relay does, without HTTP: per-fragment log of sealed items in
/// arrival order, exact-bytes dedupe, token registered by the first push.
#[derive(Default)]
struct FakeRelay {
    fragment: Option<(Vec<u8>, String)>,
    items: Vec<Vec<u8>>,
    seen: BTreeSet<Vec<u8>>,
    /// Most items per `Batch`, to exercise `more`.
    batch: usize,
}

impl FakeRelay {
    fn new(batch: usize) -> FakeRelay {
        FakeRelay {
            batch,
            ..FakeRelay::default()
        }
    }

    fn known(&mut self, fragment_id: &[u8], token: &str, register: bool) -> bool {
        match &self.fragment {
            Some((id, t)) => id == fragment_id && t == token,
            None => {
                if register {
                    self.fragment = Some((fragment_id.to_vec(), token.to_string()));
                }
                register
            }
        }
    }

    fn push(&mut self, token: &str, frame: &[u8]) -> Vec<u8> {
        let Message::Push {
            fragment_id,
            sealed,
        } = Message::from_bytes(frame).unwrap()
        else {
            panic!("not a Push");
        };
        assert!(self.known(&fragment_id, token, true), "401");
        assert!(sealed.len() <= 64, "413");
        let count = sealed.len() as u32;
        for item in sealed {
            let bytes = item.to_bytes().unwrap();
            if self.seen.insert(bytes.clone()) {
                self.items.push(bytes);
            }
        }
        Message::Ack {
            fragment_id,
            cursor: self.items.len() as u64,
            count,
        }
        .to_bytes()
        .unwrap()
    }

    fn pull(&mut self, token: &str, frame: &[u8]) -> Vec<u8> {
        let Message::Pull {
            fragment_id, after, ..
        } = Message::from_bytes(frame).unwrap()
        else {
            panic!("not a Pull");
        };
        // Another fragment (or a wrong token) sees nothing.
        let items: &[Vec<u8>] = if self.known(&fragment_id, token, false) {
            &self.items
        } else {
            &[]
        };
        let after = (after as usize).min(items.len());
        let end = (after + self.batch).min(items.len());
        Message::Batch {
            fragment_id,
            next: if end > after { end } else { items.len() } as u64,
            more: end < items.len(),
            sealed: items[after..end]
                .iter()
                .map(|b| gsp_wire::SealedOp::from_bytes(b).unwrap())
                .collect(),
        }
        .to_bytes()
        .unwrap()
    }
}

/// A device: a glued database and its own clock.
struct Device {
    db: SyncedDb,
    now: u64,
}

impl Device {
    fn new() -> Device {
        Device {
            db: SyncedDb::new(SiafuDB::in_memory().unwrap()),
            now: 1_800_000_000_000,
        }
    }

    fn tick(&mut self) -> u64 {
        self.now += 7;
        self.now
    }

    /// A local write followed by the commit every binding makes.
    fn run(&mut self, query: &str) {
        self.db
            .db()
            .execute(query)
            .unwrap_or_else(|e| panic!("{query}: {e}"));
        let now = self.tick();
        self.db.commit_local(now).unwrap();
    }

    fn push(&mut self, relay: &mut FakeRelay) -> usize {
        let token = self.db.relay_token(ORIGIN).unwrap();
        let mut pushed = 0;
        while let Some(frame) = self.db.push_frame().unwrap() {
            let ack = relay.push(&token, &frame);
            pushed += self.db.accept_ack(&ack).unwrap();
        }
        pushed
    }

    /// Pull until the relay has no more. Returns the changes applied.
    fn pull(&mut self, relay: &mut FakeRelay) -> usize {
        let token = self.db.relay_token(ORIGIN).unwrap();
        let mut changes = 0;
        loop {
            let frame = self.db.pull_frame().unwrap();
            let batch = relay.pull(&token, &frame);
            let now = self.tick();
            let outcome = self.db.ingest_batch(&batch, now).unwrap();
            changes += outcome.changes;
            if !outcome.more {
                return changes;
            }
        }
    }

    fn sync(&mut self, relay: &mut FakeRelay) -> usize {
        self.push(relay);
        self.pull(relay)
    }

    /// Through the persisted blob and back, as a reload does.
    fn reload(&mut self) {
        let blob = self.db.to_bytes().unwrap();
        self.db = SyncedDb::from_bytes(&blob).unwrap();
    }

    /// The graph in canonical form, keyed by global id.
    fn dump(&self) -> String {
        dump(self.db.db())
    }

    fn count(&self, query: &str) -> i64 {
        let result = self.db.db().query(query).unwrap();
        result.rows[0][0].as_i64().unwrap()
    }

    fn clean(&self) {
        let status = self.db.status().unwrap();
        assert_eq!(
            (status.dropped_local, status.skipped_remote, status.rejected),
            (0, 0, 0),
            "{:?}",
            status.last_problem
        );
        assert_eq!(status.pending, 0);
    }
}

fn dump(db: &SiafuDB) -> String {
    let mut lines = Vec::new();
    let nodes = db
        .query("MATCH (n) RETURN n._gid, labels(n), properties(n)")
        .unwrap();
    for row in &nodes.rows {
        let mut labels: Vec<String> = row[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_string())
            .collect();
        labels.sort();
        assert!(row[0].is_string(), "a node without a global id: {row:?}");
        // serde_json maps are ordered by key.
        lines.push(format!("node {} {labels:?} {}", row[0], row[2]));
    }
    let edges = db
        .query("MATCH (a)-[r]->(b) RETURN r._gid, a._gid, b._gid, type(r), properties(r)")
        .unwrap();
    for row in &edges.rows {
        assert!(row[0].is_string(), "an edge without a global id: {row:?}");
        lines.push(format!(
            "edge {} {}->{} {} {}",
            row[0], row[1], row[2], row[3], row[4]
        ));
    }
    lines.sort();
    lines.join("\n")
}

/// A creates a group, B joins it.
fn pair() -> (Device, Device, FakeRelay) {
    let (mut a, mut b) = (Device::new(), Device::new());
    let now = a.tick();
    let code = a.db.sync_create(now).unwrap();
    let now = b.tick();
    b.db.sync_join(&code, now).unwrap();
    (a, b, FakeRelay::new(5))
}

fn converge(a: &mut Device, b: &mut Device, relay: &mut FakeRelay) {
    // Two rounds: the second carries nothing if the first was enough.
    for _ in 0..2 {
        a.sync(relay);
        b.sync(relay);
    }
    assert_eq!(a.sync(relay) + b.sync(relay), 0, "still changing");
    assert_eq!(a.dump(), b.dump());
    assert_eq!(a.db.status().unwrap().outbox, 0);
    assert_eq!(b.db.status().unwrap().outbox, 0);
}

#[test]
fn concurrent_edits_deletes_and_edge_changes_converge() {
    let (mut a, mut b, mut relay) = pair();
    a.run("CREATE (:Person:Admin {name: 'Amara', age: 31, tags: ['a', 'b'], born: date('1990-04-16')})");
    a.run("CREATE (:Person {name: 'Tatenda', city: 'Harare'})");
    a.run("CREATE (:Person {name: 'Kofi'})");
    a.run("CREATE (:City {name: 'Accra', pop: 2500000})");
    a.run("MATCH (a:Person {name: 'Amara'}), (t:Person {name: 'Tatenda'}) CREATE (a)-[:KNOWS {since: 2024}]->(t)");
    a.run("MATCH (k:Person {name: 'Kofi'}), (a:Person {name: 'Amara'}) CREATE (k)-[:FOLLOWS]->(a)");
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(b.count("MATCH (n) RETURN count(n)"), 4);
    assert_eq!(b.count("MATCH ()-[r]->() RETURN count(r)"), 2);
    assert_eq!(
        b.count(
            "MATCH (:Person {name: 'Amara'})-[r:KNOWS]->(:Person {name: 'Tatenda'}) RETURN r.since"
        ),
        2024
    );

    // Both sides write without seeing each other; B's clock is the later.
    b.now = a.now + 60_000;
    // The same property on the same node: one value must win on both.
    a.run("MATCH (p:Person {name: 'Amara'}) SET p.age = 32");
    b.run("MATCH (p:Person {name: 'Amara'}) SET p.age = 40");
    // Different properties and labels of one node: both survive.
    a.run("MATCH (p:Person {name: 'Tatenda'}) SET p.nick = 'T' REMOVE p.city");
    b.run("MATCH (p:Person {name: 'Tatenda'}) SET p:Staff, p.level = 3");
    // A deletes a node that B is writing to and attaching an edge to.
    a.run("MATCH (k:Person {name: 'Kofi'}) DETACH DELETE k");
    b.run("MATCH (k:Person {name: 'Kofi'}) SET k.note = 'still here?'");
    b.run("MATCH (k:Person {name: 'Kofi'}), (c:City) CREATE (k)-[:LIVES_IN]->(c)");
    // Edge changes: a property on one side, a delete and a new edge on the other.
    a.run("MATCH ()-[r:KNOWS]->() SET r.since = 2025, r.close = true");
    b.run("MATCH (t:Person {name: 'Tatenda'}), (c:City) CREATE (t)-[:VISITED {year: 2023}]->(c)");
    b.run("MATCH ()-[r:KNOWS]->() SET r.weight = 2");
    // New entities on both sides.
    a.run("CREATE (:Note {text: 'from a'})");
    b.run("CREATE (:Note {text: 'from b'})");
    assert_ne!(a.dump(), b.dump());

    converge(&mut a, &mut b, &mut relay);
    a.clean();
    b.clean();
    // Deletes win: Kofi and both his edges are gone everywhere.
    assert_eq!(
        a.count("MATCH (k:Person {name: 'Kofi'}) RETURN count(k)"),
        0
    );
    assert_eq!(a.count("MATCH ()-[r:LIVES_IN]->() RETURN count(r)"), 0);
    assert_eq!(a.count("MATCH ()-[r:FOLLOWS]->() RETURN count(r)"), 0);
    // The later write wins.
    assert_eq!(a.count("MATCH (p:Person {name: 'Amara'}) RETURN p.age"), 40);
    assert_eq!(
        a.count("MATCH (p:Person:Staff {name: 'Tatenda', nick: 'T', level: 3}) WHERE p.city IS NULL RETURN count(p)"),
        1
    );
    assert_eq!(
        a.count("MATCH ()-[r:KNOWS {since: 2025, close: true, weight: 2}]->() RETURN count(r)"),
        1
    );
    assert_eq!(a.count("MATCH ()-[r:VISITED]->() RETURN count(r)"), 1);
    assert_eq!(a.count("MATCH (n:Note) RETURN count(n)"), 2);

    // A delete of an edge and of a node with edges, after convergence.
    b.run("MATCH ()-[r:VISITED]->() DELETE r");
    a.run("MATCH (c:City) DETACH DELETE c");
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(b.count("MATCH (c:City) RETURN count(c)"), 0);
    assert_eq!(a.count("MATCH ()-[r]->() RETURN count(r)"), 1);
    a.clean();
    b.clean();
}

#[test]
fn data_from_before_sync_is_uploaded_on_enable() {
    let (mut a, mut b) = (Device::new(), Device::new());
    // Written with capture off: the engine never reported any of it.
    for i in 0..40 {
        a.run(&format!("CREATE (:Item {{n: {i}, text: 'a{i}'}})"));
    }
    a.run("MATCH (x:Item {n: 0}), (y:Item {n: 1}) CREATE (x)-[:NEXT {w: 1}]->(y)");
    b.run("CREATE (:Item {n: 100, text: 'b only'})");
    b.run("CREATE (:Other {k: 'v'})-[:REL]->(:Other {k: 'w'})");
    assert!(a.db.status().is_none());

    let now = a.tick();
    let code = a.db.sync_create(now).unwrap();
    assert!(code.starts_with("gsp0-"));
    assert_eq!(a.db.pairing_code().unwrap().as_str(), code.as_str());
    // Genesis plus at least one op are waiting.
    assert!(a.db.status().unwrap().outbox >= 2);
    let now = b.tick();
    b.db.sync_join(&code, now).unwrap();
    assert!(matches!(
        b.db.sync_join(&code, now),
        Err(Error::AlreadyEnabled)
    ));

    let mut relay = FakeRelay::new(5);
    converge(&mut a, &mut b, &mut relay);
    // The union, each entity once.
    for d in [&a, &b] {
        assert_eq!(d.count("MATCH (n:Item) RETURN count(n)"), 41);
        assert_eq!(d.count("MATCH (n:Other) RETURN count(n)"), 2);
        assert_eq!(d.count("MATCH ()-[r]->() RETURN count(r)"), 2);
        assert_eq!(
            d.count("MATCH (:Item {n: 0})-[r:NEXT]->(:Item {n: 1}) RETURN r.w"),
            1
        );
        d.clean();
    }
    // A later write to old data travels as a delta to the same entity.
    a.run("MATCH (x:Item {n: 7}) SET x.text = 'edited'");
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(
        b.count("MATCH (x:Item {n: 7, text: 'edited'}) RETURN count(x)"),
        1
    );
    assert_eq!(b.count("MATCH (n:Item) RETURN count(n)"), 41);
}

#[test]
fn a_large_graph_is_split_into_several_ops_and_an_oversized_value_is_counted() {
    let mut a = Device::new();
    a.db.db()
        .execute("UNWIND range(1, 2500) AS i CREATE (:Bulk {i: i, pad: 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'})")
        .unwrap();
    // A node whose properties together exceed one op, each fitting alone.
    let big = "y".repeat(30_000);
    a.db.db()
        .execute(&format!(
            "CREATE (:Wide {{p1: '{big}', p2: '{big}', p3: '{big}', p4: '{big}', p5: '{big}'}})"
        ))
        .unwrap();
    let now = a.tick();
    let code = a.db.sync_create(now).unwrap();
    let status = a.db.status().unwrap();
    assert!(status.local_ops >= 4, "{status:?}");
    assert_eq!(status.dropped_local, 0, "{:?}", status.last_problem);

    let mut b = Device::new();
    let now = b.tick();
    b.db.sync_join(&code, now).unwrap();
    let mut relay = FakeRelay::new(3);
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(b.count("MATCH (n:Bulk) RETURN count(n)"), 2500);
    assert_eq!(b.count("MATCH (n:Bulk) RETURN sum(n.i)"), 2500 * 2501 / 2);
    assert_eq!(
        b.count(
            "MATCH (w:Wide) RETURN size(w.p1) + size(w.p2) + size(w.p3) + size(w.p4) + size(w.p5)"
        ),
        150_000
    );

    // One value larger than an op can be: it stays local and is counted,
    // the rest of the same write still syncs.
    let huge = "z".repeat(70_000);
    a.run(&format!("CREATE (:Huge {{small: 1, blob: '{huge}'}})"));
    let status = a.db.status().unwrap();
    assert_eq!(status.dropped_local, 1);
    assert!(status.last_problem.unwrap().contains("64 KiB"));
    a.sync(&mut relay);
    b.sync(&mut relay);
    assert_eq!(
        b.count("MATCH (h:Huge {small: 1}) WHERE h.blob IS NULL RETURN count(h)"),
        1
    );
}

#[test]
fn the_combined_blob_round_trips_in_the_middle_of_a_sync() {
    let (mut a, mut b, mut relay) = pair();
    a.db.set_meta(b"https://relay.example".to_vec());
    for i in 0..12 {
        a.run(&format!("CREATE (:Item {{n: {i}}})"));
    }
    a.run("MATCH (x:Item {n: 0}), (y:Item {n: 1}) CREATE (x)-[:NEXT]->(y)");
    // A pushed part of its outbox, B pulled part of that, then both stop.
    let token = a.db.relay_token(ORIGIN).unwrap();
    let frame = a.db.push_frame().unwrap().unwrap();
    let ack = relay.push(&token, &frame);
    a.db.accept_ack(&ack).unwrap();
    let frame = b.db.pull_frame().unwrap();
    let batch = relay.pull(&b.db.relay_token(ORIGIN).unwrap(), &frame);
    let now = b.tick();
    let first = b.db.ingest_batch(&batch, now).unwrap();
    assert!(first.more && first.items == 5 && first.changes > 0);
    // Writes that are captured but not committed yet ride in the blob too.
    a.db.db().execute("CREATE (:Item {n: 99})").unwrap();
    b.run("CREATE (:Item {n: 50})");
    let (cursor, outbox) = {
        let s = b.db.status().unwrap();
        (s.cursor, s.outbox)
    };
    assert!(cursor > 0 && outbox > 0);
    let before = b.dump();

    a.reload();
    b.reload();
    assert_eq!(a.db.meta(), b"https://relay.example");
    assert_eq!(b.db.meta(), b"");
    assert_eq!(b.dump(), before);
    let status = b.db.status().unwrap();
    assert_eq!((status.cursor, status.outbox), (cursor, outbox));
    assert_eq!(a.db.pairing_code().unwrap(), b.db.pairing_code().unwrap());
    assert_eq!(a.db.relay_token(ORIGIN).unwrap(), token);

    converge(&mut a, &mut b, &mut relay);
    for d in [&a, &b] {
        // Nothing lost, nothing twice.
        assert_eq!(d.count("MATCH (n:Item) RETURN count(n)"), 14);
        assert_eq!(d.count("MATCH (n:Item) RETURN count(DISTINCT n.n)"), 14);
        assert_eq!(d.count("MATCH ()-[r:NEXT]->() RETURN count(r)"), 1);
        d.clean();
    }
    // A second reload with nothing in flight changes nothing, and pulling
    // everything again from the start applies nothing.
    b.reload();
    assert_eq!(b.sync(&mut relay), 0);
    assert_eq!(a.dump(), b.dump());
}

#[test]
fn a_bare_snapshot_from_before_sync_still_loads() {
    // Written by a build without sync: a plain engine snapshot.
    let old = SiafuDB::in_memory().unwrap();
    old.execute("CREATE (:Note {title: 'old'})-[:LINKS]->(:Note {title: 'older'})")
        .unwrap();
    let legacy = old.export_snapshot().unwrap();
    assert!(!legacy.starts_with(b"SIAFGSP\0"));

    let mut a = Device::new();
    a.db = SyncedDb::from_bytes(&legacy).unwrap();
    assert!(!a.db.sync_enabled());
    assert!(a.db.status().is_none());
    assert!(matches!(a.db.relay_token(ORIGIN), Err(Error::NotEnabled)));
    assert!(matches!(a.db.pull_frame(), Err(Error::NotEnabled)));
    assert_eq!(a.count("MATCH (n:Note) RETURN count(n)"), 2);
    // Without a replica the blob stays a bare snapshot, byte for byte what
    // the engine writes.
    assert_eq!(
        a.db.to_bytes().unwrap().as_slice(),
        a.db.db().export_snapshot().unwrap().as_slice()
    );

    // One with change capture on (a trailer of queued changes) loads too.
    let capturing = SiafuDB::in_memory().unwrap();
    capturing.enable_change_capture().unwrap();
    capturing
        .execute("CREATE (:Note {title: 'queued'})")
        .unwrap();
    let with_trailer = capturing.export_snapshot().unwrap();
    let c = SyncedDb::from_bytes(&with_trailer).unwrap();
    assert!(!c.sync_enabled());

    // And sync can then be set up on the old data.
    let now = a.tick();
    let code = a.db.sync_create(now).unwrap();
    let mut b = Device::new();
    let now = b.tick();
    b.db.sync_join(&code, now).unwrap();
    let mut relay = FakeRelay::new(5);
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(
        b.count("MATCH (:Note {title: 'old'})-[:LINKS]->(:Note {title: 'older'}) RETURN count(*)"),
        1
    );

    // Damaged combined blobs are refused, not misread as snapshots.
    let blob = a.db.to_bytes().unwrap();
    assert!(blob.starts_with(b"SIAFGSP\0"));
    assert!(matches!(
        SyncedDb::from_bytes(&blob[..blob.len() - 1]),
        Err(Error::Blob(_))
    ));
    let mut newer = blob.to_vec();
    newer[8] = 9;
    assert!(matches!(SyncedDb::from_bytes(&newer), Err(Error::Blob(_))));
}

#[test]
fn the_relay_holds_no_plaintext_and_a_wrong_code_sees_nothing() {
    let (mut a, mut b, mut relay) = pair();
    a.run(&format!("CREATE (:Secret {{{MARKER_KEY}: '{MARKER}'}})"));
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(b.count("MATCH (s:Secret) RETURN count(s)"), 1);
    let needles: [&[u8]; 3] = [MARKER.as_bytes(), MARKER_KEY.as_bytes(), b"Secret"];
    for item in &relay.items {
        for needle in needles {
            assert!(!item.windows(needle.len()).any(|w| w == needle));
        }
    }
    let code = a.db.pairing_code().unwrap();
    assert!(!relay.items.iter().any(|i| {
        i.windows(code.len() - 5)
            .any(|w| w == &code.as_bytes()[5..])
    }));

    // A third device with another (well-formed) code: an empty group.
    let mut c = Device::new();
    let mut other = Device::new();
    let now = other.tick();
    let wrong = other.db.sync_create(now).unwrap();
    let now = c.tick();
    c.db.sync_join(&wrong, now).unwrap();
    assert_eq!(c.pull(&mut relay), 0);
    assert_eq!(c.count("MATCH (n) RETURN count(n)"), 0);
    // Even handed the group's items directly, it can open none of them.
    let foreign = Message::Batch {
        fragment_id: c.db.status().unwrap().fragment_id.to_vec(),
        next: relay.items.len() as u64,
        more: false,
        sealed: relay
            .items
            .iter()
            .map(|b| gsp_wire::SealedOp::from_bytes(b).unwrap())
            .collect(),
    }
    .to_bytes()
    .unwrap();
    let now = c.tick();
    let outcome = c.db.ingest_batch(&foreign, now).unwrap();
    assert_eq!((outcome.ops, outcome.changes), (0, 0));
    assert_eq!(c.count("MATCH (n) RETURN count(n)"), 0);
    assert_eq!(c.db.status().unwrap().rejected, relay.items.len() as u64);

    // Malformed codes are refused, and the error does not repeat the code.
    let mut d = Device::new();
    for bad in [
        "",
        "gsp0-",
        "nonsense",
        &code[..code.len() - 1],
        &code.replace("gsp0-", "gsp1-"),
    ] {
        let Err(e) = d.db.sync_join(bad, 1) else {
            panic!("accepted {bad:?}");
        };
        assert!(matches!(e, Error::Pairing(_)));
        assert!(bad.len() < 8 || !e.to_string().contains(&bad[5..]));
    }
    assert!(!d.db.sync_enabled());
}

const MARKER_KEY: &str = "markerkeyaardvark";

#[test]
fn frames_that_are_not_what_was_asked_for_are_refused() {
    let (mut a, _b, mut relay) = pair();
    a.run("CREATE (:Item {n: 1})");
    let token = a.db.relay_token(ORIGIN).unwrap();
    let push = a.db.push_frame().unwrap().unwrap();
    let pull = a.db.pull_frame().unwrap();
    let ack = relay.push(&token, &push);
    let batch = relay.pull(&token, &pull);
    assert!(matches!(a.db.accept_ack(&batch), Err(Error::Frame(_))));
    assert!(matches!(a.db.ingest_batch(&ack, 1), Err(Error::Frame(_))));
    assert!(matches!(
        a.db.ingest_batch(b"\x00junk", 1),
        Err(Error::Frame(_))
    ));
    let other = Message::Ack {
        fragment_id: vec![7; 16],
        cursor: 9,
        count: 9,
    }
    .to_bytes()
    .unwrap();
    assert!(matches!(a.db.accept_ack(&other), Err(Error::Frame(_))));
    // Nothing left the outbox on the way; the real Ack empties it, and a
    // second copy of it removes nothing more.
    let outbox = a.db.status().unwrap().outbox;
    assert!(outbox >= 2);
    let _ = a.db.push_frame().unwrap().unwrap();
    assert_eq!(a.db.accept_ack(&ack).unwrap(), outbox);
    a.run("CREATE (:Item {n: 2})");
    assert_eq!(a.db.accept_ack(&ack).unwrap(), 0);
    assert_eq!(a.db.status().unwrap().outbox, 1);

    // Leaving keeps the data and forgets the group.
    a.db.sync_leave();
    assert!(a.db.status().is_none());
    assert_eq!(a.count("MATCH (n:Item) RETURN count(n)"), 2);
    a.run("CREATE (:Item {n: 3})");
    assert_eq!(a.db.commit_local(5).unwrap(), 0);
}

#[test]
fn the_relay_token_is_bound_to_the_relay_origin() {
    let (a, b, _relay) = pair();
    let token = a.db.relay_token("https://relay.example").unwrap();
    assert_eq!(token.len(), 43);
    // Same group, same relay: same token, however the origin is spelled.
    assert_eq!(
        b.db.relay_token("HTTPS://Relay.Example:443/").unwrap(),
        token
    );
    // Another relay, another token.
    for other in [
        "http://relay.example",
        "https://relay.example:8443",
        "https://other.example",
        "http://localhost:8787",
    ] {
        assert_ne!(a.db.relay_token(other).unwrap(), token, "{other}");
    }
    // What is not an origin is refused, and says so.
    for bad in [
        "",
        "relay.example",
        "ftp://relay.example",
        "https://relay.example/path",
        "https://user@relay.example",
        "https://relay.example?x=1",
        "https://relay.example:0",
        "https://",
    ] {
        let Err(e) = a.db.relay_token(bad) else {
            panic!("accepted {bad:?}");
        };
        assert!(matches!(e, Error::Origin(_)), "{bad:?}: {e:?}");
        assert!(e.to_string().contains("relay address"), "{e}");
    }
    assert_eq!(
        siafudb_gsp::canonical_origin("HTTP://LOCALHOST:080/").unwrap(),
        "http://localhost"
    );
}

/// A device of the group that is not a SiafuDB: a bare replica, which can
/// write things the engine will not accept.
struct RawPeer {
    replica: gsp_sync::Replica,
    now: u64,
    nonce: u8,
}

impl RawPeer {
    fn join(code: &str) -> RawPeer {
        let root = *gsp_sync::Pairing::parse(code).unwrap().root();
        RawPeer {
            replica: gsp_sync::Replica::create(
                root,
                [0x5e; 32],
                [0xa0; 24],
                gsp_sync::Config::default(),
            )
            .unwrap(),
            now: 1_800_000_100_000,
            nonce: 0,
        }
    }

    fn commit(&mut self, changes: &[gsp_sync::Change]) {
        self.now += 10;
        self.nonce += 1;
        self.replica
            .local_commit(changes, self.now, [self.nonce; 24])
            .unwrap();
    }

    fn push(&mut self, relay: &mut FakeRelay) {
        let token = gsp_sync::base64url(&self.replica.relay_token(ORIGIN).unwrap()[..]);
        let sealed: Vec<gsp_wire::SealedOp> = self
            .replica
            .outbox_peek(64)
            .into_iter()
            .map(|b| gsp_wire::SealedOp::from_bytes(b).unwrap())
            .collect();
        let count = sealed.len();
        let frame = Message::Push {
            fragment_id: self.replica.fragment_id().to_vec(),
            sealed,
        };
        relay.push(&token, &frame.to_bytes().unwrap());
        self.replica.outbox_ack(count);
    }
}

#[test]
fn a_remote_change_the_engine_refuses_is_counted_and_everything_else_applies() {
    use gsp_sync::Change;
    use gsp_wire::Value;
    let (mut a, mut b, mut relay) = pair();
    a.run("CREATE (:Item {n: 1})");
    converge(&mut a, &mut b, &mut relay);

    let mut peer = RawPeer::join(&a.db.pairing_code().unwrap());
    let gid = |n: u8| [n; 16];
    let node = |n: u8, key: &str, value: Value| {
        let mut put = Change::node(gid(n));
        if let Change::NodePut {
            labels_add, set, ..
        } = &mut put
        {
            labels_add.insert("Peer".to_string());
            set.insert(key.to_string(), value);
        }
        put
    };
    peer.commit(&[
        node(1, "name", Value::Text("fine".into())),
        // The engine's reserved property: `apply_changes` refuses it.
        node(2, "_gid", Value::Text("00".repeat(16))),
        // An integer the engine's 64-bit type cannot hold: no conversion.
        node(3, "big", Value::Int(i128::from(i64::MAX) + 1)),
    ]);
    peer.commit(&[
        node(4, "name", Value::Text("also fine".into())),
        Change::edge(gid(10), gid(1), gid(4), "GOOD"),
        // An endpoint the engine does not have (its node was refused).
        Change::edge(gid(11), gid(1), gid(2), "BAD"),
    ]);
    peer.push(&mut relay);

    for d in [&mut a, &mut b] {
        // Four changes arrive: two nodes and one edge are refused.
        assert_eq!(d.pull(&mut relay), 3);
        let status = d.db.status().unwrap();
        assert_eq!(status.skipped_remote, 3, "{:?}", status.last_problem);
        assert!(
            status
                .last_problem
                .unwrap()
                .contains("a remote change was not applied")
        );
        // The replica itself accepted both ops; nothing is stuck behind them.
        assert_eq!((status.rejected, status.pending), (0, 0));
        assert_eq!(status.devices, 3);
        assert_eq!(d.count("MATCH (p:Peer) RETURN count(p)"), 2);
        assert_eq!(
            d.count(
                "MATCH (:Peer {name: 'fine'})-[r:GOOD]->(:Peer {name: 'also fine'}) RETURN count(r)"
            ),
            1
        );
        assert_eq!(d.count("MATCH ()-[r:BAD]->() RETURN count(r)"), 0);
        assert_eq!(d.count("MATCH (n:Item) RETURN count(n)"), 1);
    }
    // Later ops of that device, and of the others, still apply everywhere.
    peer.commit(&[node(1, "name", Value::Text("renamed".into()))]);
    peer.push(&mut relay);
    a.run("MATCH (p:Peer {name: 'also fine'}) SET p.seen = true");
    b.run("CREATE (:Item {n: 2})");
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(
        b.count("MATCH (p:Peer) WHERE p.name = 'renamed' OR p.seen RETURN count(p)"),
        2
    );
    assert_eq!(a.count("MATCH (n:Item) RETURN count(n)"), 2);
    assert_eq!(a.db.status().unwrap().skipped_remote, 3);
}

#[test]
fn a_rolled_back_device_is_forked_keeps_its_writes_and_rejoins_as_a_new_device() {
    let (mut a, mut b, mut relay) = pair();
    a.run("CREATE (:Doc {k: 'shared', v: 0})");
    converge(&mut a, &mut b, &mut relay);
    let backup = a.db.to_bytes().unwrap();

    // A writes and uploads, then loses that state: the backup comes back.
    a.run("CREATE (:Doc {k: 'w1'})");
    a.push(&mut relay);
    a.db = SyncedDb::from_bytes(&backup).unwrap();
    assert_eq!(a.count("MATCH (d:Doc {k: 'w1'}) RETURN count(d)"), 0);
    assert!(!a.db.forked());

    // Not knowing, it writes again (reusing the sequence number of `w1`)...
    a.run("CREATE (:Doc {k: 'w2'})");
    assert_eq!(a.db.status().unwrap().outbox, 1);
    // ...and finds out when its own earlier op comes back from the relay.
    a.sync(&mut relay);
    assert!(a.db.forked());
    assert!(a.db.status().unwrap().forked);
    // It follows what it had published: `w1` is back.
    assert_eq!(a.count("MATCH (d:Doc {k: 'w1'}) RETURN count(d)"), 1);

    // Writes still work locally and are not handed to the replica.
    let ops = a.db.status().unwrap().local_ops;
    a.run("CREATE (:Doc {k: 'w3'})");
    a.run("MATCH (d:Doc {k: 'shared'}) SET d.v = 3, d.by = 'a'");
    assert_eq!(a.db.status().unwrap().local_ops, ops);
    assert_eq!(a.count("MATCH (d:Doc) RETURN count(d)"), 4);
    // Meanwhile B writes the same property; A still receives it.
    b.now = a.now + 1_000;
    b.run("MATCH (d:Doc {k: 'shared'}) SET d.v = 7");
    b.run("CREATE (:Doc {k: 'from-b'})");
    b.sync(&mut relay);
    a.sync(&mut relay);
    assert_eq!(a.count("MATCH (d:Doc {k: 'from-b'}) RETURN count(d)"), 1);
    // B has none of what A wrote after the rollback, and holds the proof
    // that A's old identity signed one position twice.
    b.sync(&mut relay);
    assert_eq!(
        b.count("MATCH (d:Doc) WHERE d.k IN ['w2', 'w3'] RETURN count(d)"),
        0
    );
    assert_eq!(b.count("MATCH (d:Doc {k: 'w1'}) RETURN count(d)"), 1);
    assert!(
        b.db.status()
            .unwrap()
            .device_list
            .iter()
            .any(|d| d.equivocated)
    );

    // The forked state and the waiting writes survive a reload.
    a.reload();
    assert!(a.db.forked());
    assert_eq!(a.count("MATCH (d:Doc) RETURN count(d)"), 5);
    assert!(matches!(
        b.db.rejoin_as_new_device(1),
        Err(Error::NotForked)
    ));

    a.now = b.now + 1_000;
    let now = a.tick();
    let queued = a.db.rejoin_as_new_device(now).unwrap();
    assert!(queued >= 1);
    assert!(!a.db.forked());
    // Its local writes win over what B wrote while they waited, in its own
    // engine too.
    assert_eq!(a.count("MATCH (d:Doc {k: 'shared'}) RETURN d.v"), 3);

    converge(&mut a, &mut b, &mut relay);
    for d in [&a, &b] {
        assert_eq!(
            d.count("MATCH (d:Doc) WHERE d.k IN ['w1', 'w2', 'w3', 'from-b', 'shared'] RETURN count(DISTINCT d.k)"),
            5
        );
        assert_eq!(d.count("MATCH (d:Doc) RETURN count(d)"), 5);
        assert_eq!(
            d.count("MATCH (d:Doc {k: 'shared', by: 'a'}) RETURN d.v"),
            3
        );
    }
    // B sees one device more; A can write again.
    assert_eq!(b.db.status().unwrap().devices, 3);
    a.run("CREATE (:Doc {k: 'after'})");
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(b.count("MATCH (d:Doc {k: 'after'}) RETURN count(d)"), 1);
    assert_eq!(a.db.status().unwrap().dropped_local, 0);
}

#[test]
fn a_relay_that_lost_its_log_is_noticed_and_what_was_in_flight_is_pushed_again() {
    let (mut a, mut b, mut relay) = pair();
    a.run("CREATE (:Item {n: 1})");
    converge(&mut a, &mut b, &mut relay);

    // Acknowledged by the relay, read back by nobody, and then the relay is
    // replaced by an empty one.
    a.run("CREATE (:Item {n: 2})");
    assert_eq!(a.push(&mut relay), 1);
    assert_eq!(a.db.status().unwrap().unechoed, 1);
    relay = FakeRelay::new(5);

    let frame = a.db.pull_frame().unwrap();
    let batch = relay.pull(&a.db.relay_token(ORIGIN).unwrap(), &frame);
    let now = a.tick();
    let outcome = a.db.ingest_batch(&batch, now).unwrap();
    assert!(outcome.relay_reset);
    let status = a.db.status().unwrap();
    assert_eq!(
        (status.relay_resets, status.cursor, status.outbox),
        (1, 0, 1)
    );

    a.sync(&mut relay);
    assert_eq!(b.pull(&mut relay), 1);
    assert_eq!(b.db.status().unwrap().relay_resets, 1);
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(b.count("MATCH (n:Item) RETURN count(n)"), 2);
    // Detected once, not on every pull.
    assert_eq!(a.db.status().unwrap().relay_resets, 1);

    // History is gone: a device that joins now cannot read what A and B
    // write (their key logs were only on the old relay), and says so.
    let mut c = Device::new();
    let now = c.tick();
    c.db.sync_join(&a.db.pairing_code().unwrap(), now).unwrap();
    c.run("CREATE (:Item {n: 3})");
    c.sync(&mut relay);
    assert_eq!(c.count("MATCH (n:Item) RETURN count(n)"), 1);
    assert!(c.db.status().unwrap().pending >= 1);
    // The others do receive what the new device writes.
    converge(&mut a, &mut b, &mut relay);
    assert_eq!(a.count("MATCH (n:Item) RETURN count(n)"), 3);
}

#[test]
fn sync_state_from_an_older_format_version_is_refused_by_name() {
    let engine = SiafuDB::in_memory().unwrap().export_snapshot().unwrap();
    let state = |version: i128| {
        gsp_wire::Value::Array(vec![
            gsp_wire::Value::Text("gsp-sync-state".into()),
            gsp_wire::Value::Int(version),
        ])
        .encode()
        .unwrap()
    };
    let blob = |replica: &[u8]| {
        let mut out = b"SIAFGSP\0\x01".to_vec();
        for section in [engine.as_slice(), replica, b""] {
            out.extend_from_slice(&(section.len() as u64).to_le_bytes());
            out.extend_from_slice(section);
        }
        out
    };
    let Err(e) = SyncedDb::from_bytes(&blob(&state(0))) else {
        panic!("version 0 state was accepted");
    };
    assert!(matches!(e, Error::SyncStateVersion { found: 0 }), "{e:?}");
    let text = e.to_string();
    assert!(
        text.starts_with(siafudb_gsp::SYNC_STATE_VERSION_PREFIX),
        "{text}"
    );
    assert!(
        text.contains("version 0") && text.contains("reads version 1"),
        "{text}"
    );
    assert!(matches!(
        SyncedDb::from_bytes(&blob(&state(7))),
        Err(Error::SyncStateVersion { found: 7 })
    ));
    // Damage is a different error.
    assert!(matches!(
        SyncedDb::from_bytes(&blob(b"junk")),
        Err(Error::Replica(_))
    ));
}
