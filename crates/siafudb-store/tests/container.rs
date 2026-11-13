//! Container: round trips (unit + property), rollback, tamper, wrong key.

mod common;

use std::collections::BTreeMap;

use common::*;
use proptest::prelude::*;
use siafudb_store::{
    Container, EncryptedPageStore, MemPageStore, OpenOptions, PageStore, RawKek, StoreError,
};

#[test]
fn round_trip_plain_and_encrypted() {
    for encrypted in [false, true] {
        let inner = MemPageStore::new(if encrypted { PPS } else { LPS });
        let mut c: Container<Box<dyn PageStore>> = if encrypted {
            Container::create(Box::new(encrypt(inner.clone())) as Box<dyn PageStore>).unwrap()
        } else {
            Container::create(Box::new(inner.clone()) as Box<dyn PageStore>).unwrap()
        };
        assert_eq!(c.commit_seq(), 1);
        c.put_blob("snapshot", &bytes(1, 3000)).unwrap();
        c.put_blob("empty", b"").unwrap();
        for i in 0..10u8 {
            assert_eq!(c.append_log(&[i; 7]).unwrap(), u64::from(i));
        }
        // Read-your-writes before commit.
        assert_eq!(c.get_blob("snapshot").unwrap().unwrap(), bytes(1, 3000));
        assert_eq!(c.read_log(8).unwrap().len(), 2);
        assert_eq!(c.commit().unwrap(), 2);
        assert!(!c.is_dirty());
        assert_eq!(c.commit().unwrap(), 2, "clean commit is a no-op");
        let want = state(&c);
        drop(c);
        let store: Box<dyn PageStore> = if encrypted {
            Box::new(reopen_encrypted(inner.clone()).unwrap())
        } else {
            Box::new(inner.clone())
        };
        let mut c = Container::open(store).unwrap();
        assert_eq!(state(&c), want);
        assert_eq!(c.get_blob("empty").unwrap().unwrap(), b"");
        assert_eq!(c.get_blob("missing").unwrap(), None);
        assert_eq!(c.open_report().seq, 2);
        assert!(!c.open_report().damaged_superblock);

        // Snapshot + log truncation, delete.
        c.checkpoint("snapshot", &bytes(2, 100), 6).unwrap();
        c.delete_blob("empty").unwrap();
        c.commit().unwrap();
        assert_eq!(c.log_bounds(), (6, 10));
        let log = c.read_log(0).unwrap();
        assert_eq!(log.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![6, 7, 8, 9]);
        assert_eq!(c.blob_names(), vec!["snapshot".to_string()]);
    }
}

#[test]
fn space_is_reused_and_trimmed() {
    let inner = MemPageStore::new(LPS);
    let mut c = Container::create(inner.clone()).unwrap();
    for i in 0..50 {
        c.put_blob("snap", &bytes(i, 10 * LPS)).unwrap();
        c.commit().unwrap();
    }
    // Two generations of a 10-page blob + directory + superblocks; never 50.
    assert!(inner.page_count().unwrap() < 30, "{}", inner.page_count().unwrap());
    c.put_blob("snap", b"x").unwrap();
    c.commit().unwrap();
    c.put_blob("snap", b"y").unwrap();
    c.commit().unwrap();
    assert!(inner.page_count().unwrap() < 10, "{}", inner.page_count().unwrap());
}

#[test]
fn many_small_log_commits_compact() {
    let inner = MemPageStore::new(LPS);
    let mut c = Container::create(inner.clone()).unwrap();
    for i in 0..200u32 {
        c.append_log(&i.to_le_bytes()).unwrap();
        c.commit().unwrap();
    }
    let log = c.read_log(0).unwrap();
    assert_eq!(log.len(), 200);
    assert!(log.iter().enumerate().all(|(i, e)| e.seq == i as u64 && e.data == (i as u32).to_le_bytes()));
    assert!(inner.page_count().unwrap() < 80, "{}", inner.page_count().unwrap());
    drop(c);
    assert_eq!(Container::open(inner).unwrap().read_log(150).unwrap().len(), 50);
}

#[test]
fn wrong_key_fails_cleanly() {
    let inner = MemPageStore::new(PPS);
    let mut c = Container::create(encrypt(inner.clone())).unwrap();
    c.put_blob("s", b"secret").unwrap();
    c.commit().unwrap();
    drop(c);
    let r = EncryptedPageStore::open(inner, &RawKek::new([0u8; 32]));
    assert!(matches!(r, Err(StoreError::WrongKey)));
}


/// Restore an older authentic copy of any single physical page (from any
/// earlier commit): open must detect it, or be unaffected (page no longer
/// referenced). Never a silent mix of old and new. The one undetectable
/// case is replaying the newest superblock slot itself, which turns the
/// store into an exact earlier checkpoint (a whole-store rollback); that is
/// caught by `min_seq`.
#[test]
fn replaying_an_older_page_is_detected() {
    let inner = MemPageStore::new(PPS);
    let mut c = Container::create(encrypt(inner.clone())).unwrap();
    let mut history = Vec::new(); // (image, state, seq)
    for i in 0..6u64 {
        c.put_blob("snapshot", &bytes(i, 3 * LPS)).unwrap();
        c.append_log(&bytes(100 + i, 50)).unwrap();
        let seq = c.commit().unwrap();
        history.push((inner.to_bytes(), state(&c), seq));
    }
    let (cur_image, cur, cur_seq) = history.pop().unwrap();
    drop(c);
    let (mut rollback_errors, mut other_errors, mut whole_store) = (0, 0, 0);
    for (old_image, _, _) in &history {
        for p in 0..cur_image.len().min(old_image.len()) / PPS {
            let range = p * PPS..(p + 1) * PPS;
            if cur_image[range.clone()] == old_image[range.clone()] {
                continue;
            }
            let mut img = cur_image.clone();
            img[range.clone()].copy_from_slice(&old_image[range]);
            let store = MemPageStore::from_bytes(PPS, img).unwrap();
            match reopen_encrypted(store.clone()).and_then(Container::open) {
                Err(StoreError::Rollback(_)) => rollback_errors += 1,
                Err(e) => {
                    assert!(matches!(e, StoreError::Integrity(_) | StoreError::Corrupt(_)), "page {p}: {e:?}");
                    other_errors += 1;
                }
                Ok(c) => {
                    let s = state(&c);
                    if s == cur {
                        continue;
                    }
                    // Physical pages 2 and 3 are the superblock slots.
                    assert!(p == 2 || p == 3, "page {p}: silent mix");
                    assert!(history.iter().any(|(_, st, _)| *st == s), "page {p}: not a real checkpoint");
                    whole_store += 1;
                    drop(c);
                    let r = Container::open_with(
                        reopen_encrypted(store).unwrap(),
                        OpenOptions { min_seq: Some(cur_seq), ..OpenOptions::default() },
                    );
                    assert!(matches!(r, Err(StoreError::Rollback(_))), "{r:?}");
                }
            }
        }
    }
    eprintln!("replays: {rollback_errors} rollback errors, {other_errors} other errors, {whole_store} whole-store (min_seq)");
    assert!(rollback_errors >= 3, "stale data/dir pages detected by the root: {rollback_errors}");
}

fn populated_encrypted() -> (MemPageStore, Vec<u8>, State, State) {
    let inner = MemPageStore::new(PPS);
    let mut c = Container::create(encrypt(inner.clone())).unwrap();
    c.put_blob("snapshot", &bytes(1, 3 * LPS)).unwrap();
    c.append_log(b"one").unwrap();
    c.commit().unwrap();
    let prev = state(&c);
    let prev_image = inner.to_bytes();
    c.put_blob("snapshot", &bytes(2, 3 * LPS)).unwrap();
    c.append_log(b"two").unwrap();
    c.commit().unwrap();
    let cur = state(&c);
    (inner, prev_image, prev, cur)
}

#[test]
fn whole_store_rollback_needs_min_seq() {
    let (_inner, prev_image, prev, _) = populated_encrypted();
    let store = reopen_encrypted(MemPageStore::from_bytes(PPS, prev_image.clone()).unwrap()).unwrap();
    let c = Container::open(store).unwrap();
    assert_eq!(state(&c), prev);
    assert_eq!(c.commit_seq(), 2);
    drop(c);
    let store = reopen_encrypted(MemPageStore::from_bytes(PPS, prev_image).unwrap()).unwrap();
    let r = Container::open_with(store, OpenOptions { min_seq: Some(3), ..OpenOptions::default() });
    assert!(matches!(r, Err(StoreError::Rollback(_))));
}

/// Flip bits across every physical page of an encrypted container. Open
/// must fail, or yield exactly the current state, or (if the newest
/// superblock was hit) exactly the previous committed state with the damage
/// reported. Never wrong plaintext.
#[test]
fn bit_flips_anywhere_never_yield_wrong_data() {
    let (inner, _, prev, cur) = populated_encrypted();
    let image = inner.to_bytes();
    let mut rng = siafudb_store::testing::SplitMix(99);
    let (mut errs, mut fallback, mut same) = (0, 0, 0);
    for p in 0..image.len() / PPS {
        // Header fields, nonce, tag and a sample of ciphertext bytes.
        let mut offsets: Vec<usize> = (0..40).chain(PPS - 16..PPS).collect();
        offsets.extend((0..16).map(|_| rng.below(PPS as u64) as usize));
        for off in offsets {
            let mut img = image.clone();
            img[p * PPS + off] ^= 1 << rng.below(8);
            let store = MemPageStore::from_bytes(PPS, img).unwrap();
            match reopen_encrypted(store).and_then(Container::open) {
                Err(_) => errs += 1,
                Ok(c) => {
                    let s = state(&c);
                    if s == cur {
                        same += 1;
                    } else {
                        assert_eq!(s, prev, "page {p} off {off}: wrong data");
                        assert!(c.open_report().damaged_superblock);
                        fallback += 1;
                    }
                }
            }
        }
    }
    assert!(errs > 0 && same > 0 && fallback > 0, "errs {errs} same {same} fallback {fallback}");
}

#[test]
fn plain_container_detects_corruption_too() {
    let inner = MemPageStore::new(LPS);
    let mut c = Container::create(inner.clone()).unwrap();
    c.put_blob("b", &bytes(3, 2 * LPS)).unwrap();
    c.commit().unwrap();
    drop(c);
    let n = inner.page_count().unwrap() as usize;
    let mut detected = 0;
    for p in 0..n {
        for off in [5, LPS / 2, LPS - 1] {
            let img = inner.deep_clone();
            img.with_bytes_mut(|b| b[p * LPS + off] ^= 0x10);
            match Container::open(img) {
                Err(_) => detected += 1,
                // Unreferenced page, the older superblock slot, or the newest
                // slot (falls back to the previous commit, which had no blob).
                Ok(c) => {
                    let got = c.get_blob("b").unwrap();
                    if c.open_report().damaged_superblock && p == 0 {
                        assert_eq!(got, None, "fell back to commit 1");
                    } else {
                        assert_eq!(got.unwrap(), bytes(3, 2 * LPS), "page {p}");
                    }
                }
            }
        }
    }
    assert!(detected >= 3 * 3, "blob pages and directory: {detected}");
}

#[test]
fn second_open_is_locked_out() {
    let inner = MemPageStore::new(LPS);
    let _c = Container::create(inner.clone()).unwrap();
    assert!(matches!(Container::open(inner), Err(StoreError::Locked)));
}

#[test]
fn open_errors() {
    assert!(matches!(Container::open(MemPageStore::new(LPS)), Err(StoreError::NotInitialized)));
    let inner = MemPageStore::new(LPS);
    drop(Container::create(inner.clone()).unwrap());
    assert!(matches!(Container::create(inner.clone()), Err(StoreError::AlreadyInitialized)));
    // Garbage everywhere: no valid superblock.
    let junk = MemPageStore::from_bytes(LPS, bytes(5, 4 * LPS)).unwrap();
    assert!(matches!(Container::open(junk), Err(StoreError::Corrupt(_))));
}

#[derive(Clone, Debug)]
enum Op {
    Put(u8, Vec<u8>),
    Delete(u8),
    Append(Vec<u8>),
    Trim(u8),
    Commit,
    Reopen,
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0u8..4, prop::collection::vec(any::<u8>(), 0..3 * LPS)).prop_map(|(n, b)| Op::Put(n, b)),
        1 => (0u8..4).prop_map(Op::Delete),
        4 => prop::collection::vec(any::<u8>(), 0..300).prop_map(Op::Append),
        1 => (0u8..40).prop_map(Op::Trim),
        3 => Just(Op::Commit),
        1 => Just(Op::Reopen),
    ]
}

/// Model: committed state + staged ops. Reopen drops staged changes.
#[derive(Clone, Default)]
struct Model {
    blobs: BTreeMap<String, Vec<u8>>,
    log: Vec<Vec<u8>>, // index = seq
    first: u64,
}

fn check_against(c: &Container<impl PageStore>, m: &Model) -> Result<(), TestCaseError> {
    let s = state(c);
    prop_assert_eq!(&s.blobs, &m.blobs);
    let next = m.log.len() as u64;
    let first = m.first.min(next);
    prop_assert_eq!(c.log_bounds(), (first, next));
    let want: Vec<(u64, Vec<u8>)> = (first..next).map(|i| (i, m.log[i as usize].clone())).collect();
    let got: Vec<(u64, Vec<u8>)> = s.log.into_iter().map(|e| (e.seq, e.data)).collect();
    prop_assert_eq!(got, want);
    Ok(())
}

fn run_model(encrypted: bool, ops: Vec<Op>) -> Result<(), TestCaseError> {
    let inner = MemPageStore::new(if encrypted { PPS } else { LPS });
    let wrap = |m: MemPageStore, create: bool| -> Box<dyn PageStore> {
        if !encrypted {
            Box::new(m)
        } else if create {
            Box::new(encrypt(m))
        } else {
            Box::new(reopen_encrypted(m).unwrap())
        }
    };
    let mut c = Some(Container::create(wrap(inner.clone(), true)).unwrap());
    let mut committed = Model::default();
    let mut staged = Model::default();
    for op in ops {
        let cc = c.as_mut().unwrap();
        match op {
            Op::Put(n, b) => {
                cc.put_blob(&format!("b{n}"), &b).unwrap();
                staged.blobs.insert(format!("b{n}"), b);
            }
            Op::Delete(n) => {
                cc.delete_blob(&format!("b{n}")).unwrap();
                staged.blobs.remove(&format!("b{n}"));
            }
            Op::Append(e) => {
                prop_assert_eq!(cc.append_log(&e).unwrap(), staged.log.len() as u64);
                staged.log.push(e);
            }
            Op::Trim(t) => {
                cc.truncate_log(u64::from(t)).unwrap();
                staged.first = staged.first.max(u64::from(t));
            }
            Op::Commit => {
                cc.commit().unwrap();
                staged.first = staged.first.min(staged.log.len() as u64);
                committed = staged.clone();
            }
            Op::Reopen => {
                drop(c.take());
                c = Some(Container::open(wrap(inner.clone(), false)).unwrap());
                staged = committed.clone();
            }
        }
        check_against(c.as_ref().unwrap(), &staged)?;
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prop_container_round_trip_plain(ops in prop::collection::vec(arb_op(), 1..40)) {
        run_model(false, ops)?;
    }

    #[test]
    fn prop_container_round_trip_encrypted(ops in prop::collection::vec(arb_op(), 1..40)) {
        run_model(true, ops)?;
    }
}
