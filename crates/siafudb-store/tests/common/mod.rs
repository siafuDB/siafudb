#![allow(dead_code)]

use std::collections::BTreeMap;

use siafudb_store::{
    Container, EncryptOptions, EncryptedPageStore, LogEntry, PageStore, RawKek, Result,
};

pub const KEY: [u8; 32] = [7u8; 32];
/// Small logical pages keep the tests fast while still spanning many pages.
pub const LPS: usize = 512;
pub const PPS: usize = LPS + siafudb_store::OVERHEAD;

pub fn kek() -> RawKek {
    RawKek::new(KEY)
}

pub fn opts() -> EncryptOptions {
    EncryptOptions {
        page_size: LPS,
        db_id: Some([9u8; 16]),
        ..EncryptOptions::default()
    }
}

pub fn encrypt<S: PageStore>(inner: S) -> EncryptedPageStore<S> {
    EncryptedPageStore::create(inner, &kek(), opts()).expect("create encrypted")
}

pub fn reopen_encrypted<S: PageStore>(inner: S) -> Result<EncryptedPageStore<S>> {
    EncryptedPageStore::open(inner, &kek())
}

/// Observable state of a container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub blobs: BTreeMap<String, Vec<u8>>,
    pub log_first: u64,
    pub log: Vec<LogEntry>,
}

pub fn state<S: PageStore>(c: &Container<S>) -> State {
    let mut blobs = BTreeMap::new();
    for n in c.blob_names() {
        blobs.insert(n.clone(), c.get_blob(&n).unwrap().unwrap());
    }
    State {
        blobs,
        log_first: c.log_bounds().0,
        log: c.read_log(0).unwrap(),
    }
}

/// A deterministic byte pattern.
pub fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut r = siafudb_store::testing::SplitMix(seed);
    (0..len).map(|_| r.next_u64() as u8).collect()
}
