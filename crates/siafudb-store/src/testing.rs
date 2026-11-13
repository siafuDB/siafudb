//! Crash simulation for tests (here and in downstream crates).
//!
//! [`FaultyStore`] models a device with a volatile write cache: writes and
//! truncates are visible to reads at once but only become durable at a
//! successful `sync`. A fault can be injected at the Nth mutating operation
//! (write, truncate or sync): the operation fails (a write may first land
//! torn) and the device is "crashed" — every later call fails. Then
//! [`FaultyStore::crash_image`] produces what a reboot would find: the
//! durable image plus an arbitrary, seeded subset of the unsynced operations,
//! in order, each of which may itself be torn. That is harsher than most
//! real disks (it allows reordering within a sync epoch), which is the point.

use std::sync::{Mutex, MutexGuard};

use crate::Result;
use crate::mem::MemPageStore;
use crate::store::{LockGuard, LockMode, PageNo, PageStore, StoreCaps, StoreError, check_len};

/// How the injected fault behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// The operation fails without effect.
    Fail,
    /// If the operation is a write, a prefix of the new page lands (rest
    /// keeps the old bytes) before it fails. Other operations just fail.
    Tear,
}

#[derive(Clone, Debug)]
enum Op {
    Write(PageNo, Vec<u8>),
    Truncate(u64),
}

struct State {
    durable: Vec<Vec<u8>>,
    view: Vec<Vec<u8>>,
    pending: Vec<Op>,
    ops: u64,
    fault_at: Option<(u64, FaultKind)>,
    crashed: bool,
}

/// Fault-injecting in-memory store. See the module docs.
pub struct FaultyStore {
    page_size: usize,
    state: Mutex<State>,
}

/// Small deterministic PRNG (splitmix64), so crash images are reproducible.
#[derive(Clone, Debug)]
pub struct SplitMix(pub u64);

impl SplitMix {
    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `0..n` (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

fn apply(pages: &mut Vec<Vec<u8>>, ps: usize, op: &Op) {
    match op {
        Op::Write(no, data) => {
            let no = *no as usize;
            if pages.len() <= no {
                pages.resize(no + 1, vec![0; ps]);
            }
            pages[no].clone_from(data);
        }
        Op::Truncate(n) => pages.resize(*n as usize, vec![0; ps]),
    }
}

fn torn(old: Option<&Vec<u8>>, new: &[u8], cut: usize) -> Vec<u8> {
    let mut out = old.cloned().unwrap_or_else(|| vec![0; new.len()]);
    out[..cut].copy_from_slice(&new[..cut]);
    out
}

impl FaultyStore {
    /// Empty store.
    pub fn new(page_size: usize) -> Self {
        Self {
            page_size,
            state: Mutex::new(State {
                durable: Vec::new(),
                view: Vec::new(),
                pending: Vec::new(),
                ops: 0,
                fault_at: None,
                crashed: false,
            }),
        }
    }

    /// Store whose durable (and visible) contents are `bytes`.
    pub fn from_bytes(page_size: usize, bytes: &[u8]) -> Self {
        let s = Self::new(page_size);
        {
            let mut st = s.st();
            st.durable = bytes.chunks(page_size).map(<[u8]>::to_vec).collect();
            st.view = st.durable.clone();
        }
        s
    }

    fn st(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Mutating operations (writes, truncates, syncs) performed so far.
    pub fn ops(&self) -> u64 {
        self.st().ops
    }

    /// Inject a fault at the mutating operation with index `ops() + after`.
    pub fn fail_after(&self, after: u64, kind: FaultKind) {
        let mut st = self.st();
        st.fault_at = Some((st.ops + after, kind));
    }

    /// Has the injected fault fired?
    pub fn crashed(&self) -> bool {
        self.st().crashed
    }

    /// Simulated reboot: durable image plus a seeded random subset of the
    /// unsynced operations (each write applied whole, dropped, or torn).
    pub fn crash_image(&self, seed: u64) -> MemPageStore {
        let st = self.st();
        let ps = self.page_size;
        let mut rng = SplitMix(seed);
        let mut pages = st.durable.clone();
        for op in &st.pending {
            match (op, rng.below(3)) {
                (_, 0) => {}
                (Op::Write(no, data), 2) => {
                    let cut = rng.below(ps as u64) as usize;
                    let t = torn(pages.get(*no as usize), data, cut);
                    apply(&mut pages, ps, &Op::Write(*no, t));
                }
                (op, _) => apply(&mut pages, ps, op),
            }
        }
        MemPageStore::from_bytes(ps, pages.concat()).expect("aligned")
    }

    /// Image as if every operation reached the disk (no crash).
    pub fn current_image(&self) -> MemPageStore {
        MemPageStore::from_bytes(self.page_size, self.st().view.concat()).expect("aligned")
    }

    /// Count a mutating op; returns the fault kind if it fires now.
    fn tick(&self, st: &mut State) -> Result<Option<FaultKind>> {
        if st.crashed {
            return Err(StoreError::Io("device crashed".into()));
        }
        let i = st.ops;
        st.ops += 1;
        match st.fault_at {
            Some((at, kind)) if at == i => {
                st.crashed = true;
                Ok(Some(kind))
            }
            _ => Ok(None),
        }
    }
}

impl PageStore for FaultyStore {
    fn page_size(&self) -> usize {
        self.page_size
    }
    fn caps(&self) -> StoreCaps {
        StoreCaps {
            durable_sync: true,
            exclusive_lock: false,
            evictable: false,
        }
    }
    fn page_count(&self) -> Result<u64> {
        let st = self.st();
        if st.crashed {
            return Err(StoreError::Io("device crashed".into()));
        }
        Ok(st.view.len() as u64)
    }
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        let st = self.st();
        if st.crashed {
            return Err(StoreError::Io("device crashed".into()));
        }
        let p = st.view.get(no as usize).ok_or(StoreError::OutOfRange(no))?;
        buf.copy_from_slice(p);
        Ok(())
    }
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()> {
        check_len(self.page_size, buf.len())?;
        let mut st = self.st();
        let ps = self.page_size;
        match self.tick(&mut st)? {
            None => {
                let op = Op::Write(no, buf.to_vec());
                apply(&mut st.view, ps, &op);
                st.pending.push(op);
                Ok(())
            }
            Some(kind) => {
                if kind == FaultKind::Tear {
                    let cut = (no as usize * 7919 + st.ops as usize * 104_729) % ps;
                    let t = torn(st.view.get(no as usize), buf, cut);
                    st.pending.push(Op::Write(no, t));
                }
                Err(StoreError::Io("injected write fault".into()))
            }
        }
    }
    fn truncate(&self, pages: u64) -> Result<()> {
        let mut st = self.st();
        let ps = self.page_size;
        match self.tick(&mut st)? {
            None => {
                let op = Op::Truncate(pages);
                apply(&mut st.view, ps, &op);
                st.pending.push(op);
                Ok(())
            }
            Some(_) => Err(StoreError::Io("injected truncate fault".into())),
        }
    }
    fn sync(&self) -> Result<()> {
        let mut st = self.st();
        match self.tick(&mut st)? {
            None => {
                st.durable = st.view.clone();
                st.pending.clear();
                Ok(())
            }
            Some(_) => Err(StoreError::Io("injected sync fault".into())),
        }
    }
    fn lock(&self, _mode: LockMode) -> Result<LockGuard> {
        Ok(LockGuard::none())
    }
}
