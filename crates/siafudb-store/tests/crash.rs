//! Crash injection: after any single injected failure (failed or torn write,
//! failed sync, failed truncate) followed by power loss with an arbitrary
//! subset of unsynced writes landing (some torn), the container opens to
//! exactly the old or exactly the new checkpoint. Never a mix, never an
//! error, and a commit that returned Ok is always the one that survives.

mod common;

use std::sync::Arc;

use common::*;
use proptest::prelude::*;
use siafudb_store::testing::{FaultKind, FaultyStore};
use siafudb_store::{Container, MemPageStore, PageStore, StoreError};

#[derive(Clone, Debug)]
enum Op {
    Put(u8, usize, u64),
    Delete(u8),
    Append(usize, u64),
    Trim(u64),
}

fn apply<S: PageStore>(c: &mut Container<S>, ops: &[Op]) {
    for op in ops {
        match *op {
            Op::Put(n, len, seed) => c.put_blob(&format!("b{n}"), &bytes(seed, len)).unwrap(),
            Op::Delete(n) => c.delete_blob(&format!("b{n}")).unwrap(),
            Op::Append(len, seed) => {
                c.append_log(&bytes(seed, len)).unwrap();
            }
            Op::Trim(t) => c.truncate_log(t).unwrap(),
        }
    }
}

fn wrap(fs: Arc<FaultyStore>, encrypted: bool) -> Box<dyn PageStore> {
    if encrypted { Box::new(encrypt(fs)) } else { Box::new(fs) }
}

fn reopen(img: MemPageStore, encrypted: bool) -> siafudb_store::Result<Container<Box<dyn PageStore>>> {
    let s: Box<dyn PageStore> = if encrypted {
        Box::new(reopen_encrypted(img)?)
    } else {
        Box::new(img)
    };
    Container::open(s)
}

/// Build a container, commit each of `history`, return the store and the
/// state after the last commit.
fn setup(encrypted: bool, history: &[Vec<Op>]) -> (Arc<FaultyStore>, Container<Box<dyn PageStore>>, State) {
    let fs = Arc::new(FaultyStore::new(if encrypted { PPS } else { LPS }));
    let mut c = Container::create(wrap(fs.clone(), encrypted)).unwrap();
    for ops in history {
        apply(&mut c, ops);
        c.commit().unwrap();
    }
    let s = state(&c);
    (fs, c, s)
}

/// Ops that the commit of `next` performs (deterministic).
fn commit_ops(encrypted: bool, history: &[Vec<Op>], next: &[Op]) -> (u64, State) {
    let (fs, mut c, _) = setup(encrypted, history);
    let before = fs.ops();
    apply(&mut c, next);
    c.commit().unwrap();
    (fs.ops() - before, state(&c))
}

struct Outcome {
    old: usize,
    new: usize,
}

#[allow(clippy::too_many_arguments)]
fn crash_one(
    encrypted: bool,
    history: &[Vec<Op>],
    next: &[Op],
    k: u64,
    kind: FaultKind,
    seed: u64,
    old: &State,
    new: &State,
    out: &mut Outcome,
) {
    let (fs, mut c, s0) = setup(encrypted, history);
    assert_eq!(&s0, old);
    fs.fail_after(k, kind);
    apply(&mut c, next);
    let r = c.commit();
    if r.is_err() {
        assert!(matches!(c.get_blob("b0"), Err(StoreError::Poisoned)));
        assert!(matches!(c.commit(), Err(StoreError::Poisoned)));
    }
    drop(c);
    let img = fs.crash_image(seed);
    let mut c = match reopen(img.clone(), encrypted) {
        Ok(c) => c,
        Err(e) => panic!("k={k} {kind:?} seed={seed} commit={r:?}: open failed: {e}"),
    };
    let got = state(&c);
    if r.is_ok() {
        assert_eq!(&got, new, "k={k} {kind:?} seed={seed}: committed Ok but lost");
    }
    if &got == old {
        out.old += 1;
    } else if &got == new {
        out.new += 1;
    } else {
        panic!("k={k} {kind:?} seed={seed}: opened a state that is neither old nor new");
    }
    // The recovered store keeps working.
    c.put_blob("after", b"recovered").unwrap();
    let seq = c.append_log(b"after").unwrap();
    c.commit().unwrap();
    let mut want = got.clone();
    want.blobs.insert("after".into(), b"recovered".to_vec());
    want.log.push(siafudb_store::LogEntry {
        seq,
        data: b"after".to_vec(),
    });
    drop(c);
    // `img` shares its bytes with the clone the container wrote through.
    let c2 = reopen(img, encrypted).unwrap();
    assert_eq!(state(&c2), want, "k={k} {kind:?} seed={seed}: recovery commit");
}

/// Exhaustive: every op index of the commit, both fault kinds, several
/// power-loss seeds.
fn exhaustive(encrypted: bool, history: Vec<Vec<Op>>, next: Vec<Op>) -> (u64, Outcome) {
    let (_, _, old) = setup(encrypted, &history);
    let (n, new) = commit_ops(encrypted, &history, &next);
    assert_ne!(old, new);
    let mut out = Outcome { old: 0, new: 0 };
    for k in 0..=n {
        for kind in [FaultKind::Fail, FaultKind::Tear] {
            for seed in 0..6 {
                crash_one(encrypted, &history, &next, k, kind, seed, &old, &new, &mut out);
            }
        }
    }
    assert!(out.old > 0 && out.new > 0, "both outcomes reachable: old {} new {}", out.old, out.new);
    eprintln!(
        "encrypted={encrypted}: {n} ops/commit, {} crash runs: {} old, {} new",
        out.old + out.new,
        out.old,
        out.new
    );
    (n, out)
}

fn history() -> Vec<Vec<Op>> {
    vec![
        vec![Op::Put(0, 3 * LPS + 17, 1), Op::Append(40, 2), Op::Append(700, 3)],
        vec![Op::Put(1, 10, 4), Op::Append(5, 5)],
    ]
}

/// Snapshot replacement + log append + log trim in one commit.
fn checkpoint_commit() -> Vec<Op> {
    vec![Op::Put(0, 4 * LPS, 6), Op::Trim(2), Op::Append(100, 7), Op::Delete(1)]
}

#[test]
fn crash_during_checkpoint_plain() {
    exhaustive(false, history(), checkpoint_commit());
}

#[test]
fn crash_during_checkpoint_encrypted() {
    exhaustive(true, history(), checkpoint_commit());
}

#[test]
fn crash_during_log_append_encrypted() {
    exhaustive(true, history(), vec![Op::Append(30, 8), Op::Append(3000, 9)]);
}

#[test]
fn crash_during_first_commit_after_create() {
    exhaustive(true, vec![], vec![Op::Put(0, 2 * LPS, 1), Op::Append(9, 2)]);
}

#[test]
fn crash_during_log_compaction() {
    // 32 one-entry commits fill the segment table; the next commit compacts.
    let hist: Vec<Vec<Op>> = (0..32).map(|i| vec![Op::Append(20, i)]).collect();
    exhaustive(false, hist, vec![Op::Append(20, 99), Op::Trim(3)]);
}

#[test]
fn crash_during_shrinking_commit() {
    // Big blob replaced by a tiny one: the commit ends with a truncate.
    exhaustive(true, vec![vec![Op::Put(0, 12 * LPS, 1)], vec![Op::Put(0, 12 * LPS, 2)]], vec![Op::Put(0, 3, 3)]);
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0u8..3, 0usize..4 * LPS, any::<u64>()).prop_map(|(n, l, s)| Op::Put(n, l, s)),
        1 => (0u8..3).prop_map(Op::Delete),
        4 => (0usize..600, any::<u64>()).prop_map(|(l, s)| Op::Append(l, s)),
        1 => (0u64..12).prop_map(Op::Trim),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Random history, random next commit, random fault point/kind/seed.
    #[test]
    fn prop_crash_opens_old_or_new(
        encrypted in any::<bool>(),
        history in prop::collection::vec(prop::collection::vec(arb_op(), 1..5), 0..5),
        next in prop::collection::vec(arb_op(), 1..6),
        k_frac in 0.0f64..1.0,
        tear in any::<bool>(),
        seed in any::<u64>(),
    ) {
        let (_, _, old) = setup(encrypted, &history);
        let (n, new) = commit_ops(encrypted, &history, &next);
        let k = ((n + 1) as f64 * k_frac) as u64;
        let kind = if tear { FaultKind::Tear } else { FaultKind::Fail };
        let mut out = Outcome { old: 0, new: 0 };
        crash_one(encrypted, &history, &next, k, kind, seed, &old, &new, &mut out);
    }
}
