//! PageStore backends: contract tests + read-after-write property tests.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use proptest::prelude::*;
use siafudb_store::{
    CallbackPageStore, EncryptedPageStore, LockMode, MemPageStore, PageStore, StoreCaps, StoreError,
};

fn contract(s: &dyn PageStore) {
    let ps = s.page_size();
    assert_eq!(s.page_count().unwrap(), 0);
    let mut buf = vec![0u8; ps];
    assert!(matches!(
        s.read_page(0, &mut buf),
        Err(StoreError::OutOfRange(0))
    ));
    s.write_page(2, &vec![2u8; ps]).unwrap();
    assert_eq!(s.page_count().unwrap(), 3);
    s.read_page(2, &mut buf).unwrap();
    assert_eq!(buf, vec![2u8; ps]);
    s.write_page(0, &vec![1u8; ps]).unwrap();
    s.sync().unwrap();
    s.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, vec![1u8; ps]);
    // Wrong buffer size is rejected, not truncated.
    assert!(matches!(
        s.write_page(0, &[0u8; 3]),
        Err(StoreError::InvalidArgument(_))
    ));
    s.truncate(1).unwrap();
    assert_eq!(s.page_count().unwrap(), 1);
    assert!(matches!(
        s.read_page(2, &mut buf),
        Err(StoreError::OutOfRange(2))
    ));
}

#[test]
fn mem_contract() {
    contract(&MemPageStore::new(256));
}

#[test]
fn encrypted_contract_and_page_accounting() {
    let inner = MemPageStore::new(PPS);
    let e = encrypt(inner.clone());
    assert_eq!(e.page_size(), LPS);
    assert_eq!(inner.page_size(), LPS + siafudb_store::OVERHEAD);
    assert_eq!(siafudb_store::OVERHEAD, 54);
    contract(&e);
    // Inner store: 2 key-header pages + 1 logical page, all physical-size.
    assert_eq!(inner.page_count().unwrap(), 3);
    assert_eq!(inner.to_bytes().len(), 3 * PPS);
}

#[test]
fn encrypted_rejects_mismatched_inner_page_size() {
    let r = EncryptedPageStore::create(MemPageStore::new(LPS), &kek(), opts());
    assert!(matches!(r, Err(StoreError::InvalidArgument(_))));
}

#[test]
fn callback_contract() {
    let pages: Arc<Mutex<BTreeMap<u64, Vec<u8>>>> = Arc::default();
    let synced = Arc::new(Mutex::new(0usize));
    let (p1, p2, p3, p4, s1) = (
        pages.clone(),
        pages.clone(),
        pages.clone(),
        pages.clone(),
        synced.clone(),
    );
    let store = CallbackPageStore::builder(256)
        .caps(StoreCaps {
            durable_sync: true,
            exclusive_lock: false,
            evictable: true,
        })
        .read(move |no, buf| {
            buf.copy_from_slice(p1.lock().unwrap().get(&no).map_or(&[0u8; 256][..], |v| v));
            Ok(())
        })
        .write(move |no, buf| {
            p2.lock().unwrap().insert(no, buf.to_vec());
            Ok(())
        })
        .page_count(move || Ok(p3.lock().unwrap().keys().next_back().map_or(0, |k| k + 1)))
        .truncate(move |n| {
            p4.lock().unwrap().retain(|k, _| *k < n);
            Ok(())
        })
        .sync(move || {
            *s1.lock().unwrap() += 1;
            Ok(())
        })
        .build()
        .unwrap();
    contract(&store);
    assert!(*synced.lock().unwrap() >= 1);
    assert!(store.caps().evictable);
    assert!(CallbackPageStore::builder(256).build().is_err());
}

#[test]
fn mem_lock_is_exclusive() {
    let s = MemPageStore::new(256);
    let g = s.lock(LockMode::Exclusive).unwrap();
    assert!(matches!(
        s.clone().lock(LockMode::Shared),
        Err(StoreError::Locked)
    ));
    drop(g);
    let a = s.lock(LockMode::Shared).unwrap();
    let _b = s.lock(LockMode::Shared).unwrap();
    assert!(matches!(
        s.lock(LockMode::Exclusive),
        Err(StoreError::Locked)
    ));
    drop(a);
}

#[cfg(feature = "native")]
mod file {
    use super::*;
    use siafudb_store::{Container, FilePageStore};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("siafudb-store-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d
    }

    #[test]
    fn file_contract() {
        let p = tmp("contract");
        contract(&FilePageStore::open(&p, 256).unwrap());
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn file_lock_excludes_second_opener() {
        let p = tmp("lock");
        let a = FilePageStore::open(&p, 256).unwrap();
        let b = FilePageStore::open(&p, 256).unwrap();
        let g = a.lock(LockMode::Exclusive).unwrap();
        assert!(matches!(
            b.lock(LockMode::Exclusive),
            Err(StoreError::Locked)
        ));
        assert!(matches!(b.lock(LockMode::Shared), Err(StoreError::Locked)));
        drop(g);
        let _g = b.lock(LockMode::Exclusive).unwrap();
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn encrypted_container_on_file_survives_reopen() {
        let p = tmp("container");
        {
            let e = encrypt(FilePageStore::open(&p, PPS).unwrap());
            let mut c = Container::create(e).unwrap();
            c.put_blob("snapshot", &bytes(1, 5000)).unwrap();
            c.append_log(b"m1").unwrap();
            c.commit().unwrap();
            // A second opener is locked out while the container is open.
            let other = reopen_encrypted(FilePageStore::open(&p, PPS).unwrap()).unwrap();
            assert!(matches!(Container::open(other), Err(StoreError::Locked)));
        }
        let c = Container::open(reopen_encrypted(FilePageStore::open(&p, PPS).unwrap()).unwrap())
            .unwrap();
        assert_eq!(c.get_blob("snapshot").unwrap().unwrap(), bytes(1, 5000));
        assert_eq!(c.read_log(0).unwrap()[0].data, b"m1");
        // Plaintext never reaches the file.
        let raw = std::fs::read(&p).unwrap();
        assert!(!raw.windows(8).any(|w| w == &bytes(1, 5000)[100..108]));
        std::fs::remove_file(p).unwrap();
    }
}

fn arb_ops(max_page: u64, ps: usize) -> impl Strategy<Value = Vec<(u64, Vec<u8>)>> {
    prop::collection::vec(
        (0..max_page, prop::collection::vec(any::<u8>(), ps..=ps)),
        1..24,
    )
}

fn read_after_write(s: &dyn PageStore, ops: &[(u64, Vec<u8>)]) -> Result<(), TestCaseError> {
    let mut model: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    for (no, data) in ops {
        s.write_page(*no, data).unwrap();
        model.insert(*no, data.clone());
        let mut buf = vec![0u8; s.page_size()];
        for (k, v) in &model {
            s.read_page(*k, &mut buf).unwrap();
            prop_assert_eq!(&buf, v);
        }
        prop_assert_eq!(
            s.page_count().unwrap(),
            model.keys().next_back().unwrap() + 1
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn prop_mem_read_after_write(ops in arb_ops(16, 256)) {
        read_after_write(&MemPageStore::new(256), &ops)?;
    }

    #[test]
    fn prop_encrypted_read_after_write(ops in arb_ops(16, LPS)) {
        let inner = MemPageStore::new(PPS);
        let e = encrypt(inner.clone());
        read_after_write(&e, &ops)?;
        // And after reopening with the key.
        let e2 = reopen_encrypted(inner).unwrap();
        let mut last: BTreeMap<u64, &Vec<u8>> = BTreeMap::new();
        for (no, d) in &ops { last.insert(*no, d); }
        let mut buf = vec![0u8; LPS];
        for (no, d) in last {
            e2.read_page(no, &mut buf).unwrap();
            prop_assert_eq!(&buf, d);
        }
    }
}
