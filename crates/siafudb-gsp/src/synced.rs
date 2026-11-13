//! [`SyncedDb`]: a database and its replica, kept consistent.

use crate::blob;
use crate::convert::{change_from_wire, change_to_wire};
use gsp_sync::{
    Change, Config, IngestStatus, MAX_CHANGES, MAX_PAYLOAD_BYTES, Pairing, PairingError, Replica,
    base64url, encode_payload,
};
use gsp_wire::{Message, SealedOp};
use siafudb_core::{GraphChange, SiafuDB, SiafuError};
use zeroize::Zeroizing;

/// Most sealed items in one `Push` frame (the relay's default limit).
const MAX_PUSH_ITEMS: usize = 64;
/// Most item bytes in one `Push` frame; the relay's default request limit is
/// 1 MiB and one item is at most 256 KiB. At least one item is always sent.
const MAX_PUSH_BYTES: usize = 720 * 1024;
/// Budget for the encoded changes of one op: the protocol's payload limit
/// less room for the array header.
const OP_BUDGET: usize = MAX_PAYLOAD_BYTES - 16;

/// Why a call failed.
#[derive(Debug)]
pub enum Error {
    /// The engine refused.
    Db(SiafuError),
    /// The replica refused (state bytes, encoding, sealing).
    Replica(gsp_sync::Error),
    /// The pairing code is not well formed. Never carries the code.
    Pairing(PairingError),
    /// The operating system gave no random bytes.
    Random,
    /// Bytes from the relay are not the frame expected here (what was wrong).
    Frame(&'static str),
    /// The persisted blob is damaged or from a newer version (what was wrong).
    Blob(&'static str),
    /// The database is not in a sync group.
    NotEnabled,
    /// The database is already in a sync group.
    AlreadyEnabled,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Db(e) => write!(f, "{e}"),
            Error::Replica(e) => write!(f, "{e}"),
            Error::Pairing(e) => write!(f, "{e}"),
            Error::Random => write!(f, "no random bytes available"),
            Error::Frame(what) => write!(f, "unexpected reply from the relay: {what}"),
            Error::Blob(what) => write!(f, "stored database is not readable: {what}"),
            Error::NotEnabled => write!(f, "sync is not set up for this database"),
            Error::AlreadyEnabled => write!(f, "sync is already set up for this database"),
        }
    }
}
impl core::error::Error for Error {}

impl From<SiafuError> for Error {
    fn from(e: SiafuError) -> Self {
        Error::Db(e)
    }
}
impl From<gsp_sync::Error> for Error {
    fn from(e: gsp_sync::Error) -> Self {
        Error::Replica(e)
    }
}

/// What [`SyncedDb::ingest_batch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatchOutcome {
    /// Sealed items in the frame.
    pub items: usize,
    /// Ops of other devices that became applied (this includes ops that were
    /// waiting for an item of this frame).
    pub ops: usize,
    /// Changes applied to the engine: what a query can now see differently.
    pub changes: usize,
    /// The relay has more after this frame: pull again without waiting.
    pub more: bool,
    /// An item could not be taken because too many received ops are already
    /// waiting for earlier ones; the cursor stays before it. Pulling again
    /// at once would return the same item: wait before retrying.
    pub stalled: bool,
}

/// Sizes and counters of a database in a sync group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncStatus {
    /// Public routing id of the group on the relay.
    pub fragment_id: [u8; 16],
    /// Sealed items waiting to be pushed.
    pub outbox: usize,
    /// Pull cursor (relay position).
    pub cursor: u64,
    /// Local commits made.
    pub local_ops: u64,
    /// Ops of other devices applied.
    pub remote_ops: u64,
    /// Received ops waiting for something they depend on.
    pub pending: usize,
    /// Devices known in the group, this one included.
    pub devices: usize,
    /// Received items dropped: failed authentication, invalid, unsupported,
    /// or equivocating.
    pub rejected: u64,
    /// Local changes that could not be handed to the protocol (for example a
    /// property over the 64 KiB op limit) and so stay on this device only.
    pub dropped_local: u64,
    /// Remote changes the engine refused; this device does not show them.
    pub skipped_remote: u64,
    /// The latest such problem, in words. Never holds a secret.
    pub last_problem: Option<String>,
}

/// A [`SiafuDB`] with an optional GSP replica. See the crate documentation.
pub struct SyncedDb {
    db: SiafuDB,
    replica: Option<Replica>,
    /// Opaque note of the transport (for example the relay URL), persisted
    /// with the rest.
    meta: Vec<u8>,
    /// Items in the `Push` frame last built and not yet acknowledged.
    in_flight: usize,
    dropped_local: u64,
    skipped_remote: u64,
    last_problem: Option<String>,
}

fn random<const N: usize>() -> Result<Zeroizing<[u8; N]>, Error> {
    let mut bytes = Zeroizing::new([0u8; N]);
    getrandom::fill(&mut bytes[..]).map_err(|_| Error::Random)?;
    Ok(bytes)
}

/// Make a put that is too large for one op smaller. With two or more
/// properties it becomes two puts that together say the same. With one, the
/// property is taken out (`None` in second place) and the caller counts it
/// as not synced. `None` when there is nothing left to take out.
fn split(change: &Change) -> Option<(Change, Option<Change>)> {
    use std::collections::BTreeMap;
    type Set = BTreeMap<String, gsp_wire::Value>;
    fn halves(set: &Set) -> Option<(Set, Option<Set>)> {
        match set.len() {
            0 => None,
            1 => Some((Set::new(), None)),
            n => {
                let mut first = set.clone();
                let middle = first.keys().nth(n / 2)?.clone();
                let second = first.split_off(&middle);
                Some((first, Some(second)))
            }
        }
    }
    match change {
        Change::NodePut {
            gid,
            labels_add,
            labels_remove,
            set,
            unset,
        } => {
            let (first, second) = halves(set)?;
            Some((
                Change::NodePut {
                    gid: *gid,
                    labels_add: labels_add.clone(),
                    labels_remove: labels_remove.clone(),
                    set: first,
                    unset: unset.clone(),
                },
                second.map(|set| Change::NodePut {
                    gid: *gid,
                    labels_add: Default::default(),
                    labels_remove: Default::default(),
                    set,
                    unset: Default::default(),
                }),
            ))
        }
        Change::EdgePut {
            gid,
            src,
            dst,
            ty,
            set,
            unset,
        } => {
            let (first, second) = halves(set)?;
            let edge = |set, unset| Change::EdgePut {
                gid: *gid,
                src: *src,
                dst: *dst,
                ty: ty.clone(),
                set,
                unset,
            };
            Some((
                edge(first, unset.clone()),
                second.map(|set| edge(set, Default::default())),
            ))
        }
        Change::NodeDel { .. } | Change::EdgeDel { .. } => None,
    }
}

impl SyncedDb {
    /// Wrap a database that is not in a sync group.
    pub fn new(db: SiafuDB) -> SyncedDb {
        SyncedDb {
            db,
            replica: None,
            meta: Vec::new(),
            in_flight: 0,
            dropped_local: 0,
            skipped_remote: 0,
            last_problem: None,
        }
    }

    /// The database, for queries and writes. After a write, call
    /// [`commit_local`](Self::commit_local).
    pub fn db(&self) -> &SiafuDB {
        &self.db
    }

    /// Whether the database is in a sync group.
    pub fn sync_enabled(&self) -> bool {
        self.replica.is_some()
    }

    // ----------------------------------------------------------- persistence

    /// Everything in one blob: the engine snapshot (with any captured change
    /// not yet committed), the replica state and the transport note. Write
    /// it in one atomic step, encrypted: it holds the group's root secret.
    ///
    /// A database that is not in a sync group and has no transport note is
    /// written as a bare engine snapshot, readable by builds without sync.
    pub fn to_bytes(&self) -> Result<Zeroizing<Vec<u8>>, Error> {
        let engine = Zeroizing::new(self.db.export_snapshot()?);
        let Some(replica) = &self.replica else {
            if self.meta.is_empty() {
                return Ok(engine);
            }
            return Ok(Zeroizing::new(blob::encode(&blob::Parts {
                engine: &engine,
                replica: &[],
                meta: &self.meta,
            })));
        };
        let state = replica.to_bytes()?;
        Ok(Zeroizing::new(blob::encode(&blob::Parts {
            engine: &engine,
            replica: &state,
            meta: &self.meta,
        })))
    }

    /// Restore from [`to_bytes`](Self::to_bytes), or from a bare engine
    /// snapshot (`SiafuDB::export_snapshot`) written before sync existed.
    pub fn from_bytes(bytes: &[u8]) -> Result<SyncedDb, Error> {
        let Some(parts) = blob::decode(bytes).map_err(Error::Blob)? else {
            return Ok(SyncedDb::new(SiafuDB::from_snapshot(bytes)?));
        };
        let mut synced = SyncedDb::new(SiafuDB::from_snapshot(parts.engine)?);
        synced.meta = parts.meta.to_vec();
        if !parts.replica.is_empty() {
            synced.replica = Some(Replica::from_bytes(parts.replica, Config::default())?);
            synced.db.enable_change_capture()?;
        }
        Ok(synced)
    }

    /// The transport's note (empty if none was set).
    pub fn meta(&self) -> &[u8] {
        &self.meta
    }

    /// Set the transport's note: opaque bytes persisted with the database,
    /// such as the relay URL. Not a place for secrets beyond those the blob
    /// already holds.
    pub fn set_meta(&mut self, meta: Vec<u8>) {
        self.meta = meta;
    }

    // -------------------------------------------------------------- pairing

    /// Create a new sync group with this database as its first device and
    /// return the pairing code to show to another device. What the database
    /// already holds is queued for upload.
    pub fn sync_create(&mut self, now_ms: u64) -> Result<Zeroizing<String>, Error> {
        if self.replica.is_some() {
            return Err(Error::AlreadyEnabled);
        }
        let root = random::<32>()?;
        self.enable(&root, now_ms)?;
        self.pairing_code().ok_or(Error::NotEnabled)
    }

    /// Join the sync group of a pairing code. What the database already
    /// holds is queued for upload and merges with what the group has.
    ///
    /// A well-formed code of a group that does not exist cannot be told
    /// apart from a new, empty group.
    pub fn sync_join(&mut self, code: &str, now_ms: u64) -> Result<(), Error> {
        if self.replica.is_some() {
            return Err(Error::AlreadyEnabled);
        }
        let pairing = Pairing::parse(code.trim()).map_err(Error::Pairing)?;
        self.enable(pairing.root(), now_ms)
    }

    fn enable(&mut self, root: &[u8; 32], now_ms: u64) -> Result<(), Error> {
        let seed = random::<32>()?;
        let nonce = random::<24>()?;
        let replica = Replica::create(*root, *seed, *nonce, Config::default())?;
        // Everything the engine holds, as puts; the capture queue restarts
        // from here.
        let graph = self.take(|db| db.take_graph_as_changes())?;
        self.replica = Some(replica);
        self.in_flight = 0;
        self.commit(graph, now_ms);
        Ok(())
    }

    /// Leave the sync group: the replica state (keys, merge history, outbox)
    /// is dropped. The data stays. Joining again later uploads it as a new
    /// device would.
    pub fn sync_leave(&mut self) {
        self.replica = None;
        self.in_flight = 0;
    }

    /// The pairing code of the group, to add another device.
    pub fn pairing_code(&self) -> Option<Zeroizing<String>> {
        self.replica.as_ref().map(|r| r.pairing().to_text())
    }

    /// The relay bearer token (unpadded base64url), for the `Authorization`
    /// header of push and pull.
    pub fn relay_token(&self) -> Option<Zeroizing<String>> {
        self.replica
            .as_ref()
            .map(|r| Zeroizing::new(base64url(&r.relay_token()[..])))
    }

    /// Sizes and counters; `None` when not in a sync group.
    pub fn status(&self) -> Option<SyncStatus> {
        let replica = self.replica.as_ref()?;
        let stats = replica.stats();
        Some(SyncStatus {
            fragment_id: *replica.fragment_id(),
            outbox: stats.outbox,
            cursor: replica.cursor(),
            local_ops: stats.local,
            remote_ops: stats.applied,
            pending: stats.pending,
            devices: stats.authors,
            rejected: stats.invalid + stats.undecryptable + stats.unsupported + stats.equivocations,
            dropped_local: self.dropped_local,
            skipped_remote: self.skipped_remote,
            last_problem: self.last_problem.clone(),
        })
    }

    // ---------------------------------------------------------- local writes

    fn problem(&mut self, what: impl core::fmt::Display) {
        self.last_problem = Some(what.to_string());
    }

    /// Take captured changes from the engine. A value with no lossless form
    /// is left out by the engine and reported once; the rest comes with the
    /// second call.
    fn take(
        &mut self,
        take: impl Fn(&SiafuDB) -> Result<Vec<GraphChange>, SiafuError>,
    ) -> Result<Vec<GraphChange>, Error> {
        match take(&self.db) {
            Ok(changes) => Ok(changes),
            Err(e @ SiafuError::UnsupportedValue(_)) => {
                self.dropped_local += 1;
                self.problem(format_args!("a local change is not synced: {e}"));
                Ok(self.db.take_changes()?)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Hand what local writes did since the last call to the replica: it is
    /// signed, sealed and queued for push, and any correction the merge rules
    /// demand is applied to the engine. Call it after every write. Returns
    /// the number of ops queued (0 when nothing was written or the database
    /// is not in a sync group).
    ///
    /// A change the protocol cannot carry is left out and counted in
    /// [`SyncStatus::dropped_local`]; it then exists on this device only.
    pub fn commit_local(&mut self, now_ms: u64) -> Result<usize, Error> {
        if self.replica.is_none() {
            // Nobody will take them: keep the capture queue from growing.
            if self.db.change_capture_enabled() {
                let _ = self.db.take_changes();
            }
            return Ok(0);
        }
        let changes = self.take(|db| db.take_changes())?;
        Ok(self.commit(changes, now_ms))
    }

    /// Local commits for `changes`, split into as many ops as their size
    /// needs. Returns the number of ops made.
    fn commit(&mut self, changes: Vec<GraphChange>, now_ms: u64) -> usize {
        // Convert and measure every change on its own; what cannot be an op
        // payload is dropped here, so a chunk can only fail as a whole for
        // reasons that have nothing to do with its content.
        let mut sized: Vec<(Change, usize)> = Vec::with_capacity(changes.len());
        let mut queue: Vec<Change> = Vec::new();
        for change in &changes {
            match change_to_wire(change) {
                Ok(change) => queue.push(change),
                Err(e) => self.drop_local(&e),
            }
            // Depth first, so the halves of a split stay in order.
            while let Some(change) = queue.pop() {
                let size = encode_payload(core::slice::from_ref(&change))
                    .and_then(|p| p.encode().map_err(gsp_sync::PayloadError::Encode))
                    .map(|bytes| bytes.len());
                match size {
                    Ok(size) if size <= OP_BUDGET => sized.push((change, size)),
                    Ok(_) | Err(gsp_sync::PayloadError::TooLarge) => {
                        const WHY: &str = "larger than one op may be (64 KiB)";
                        match split(&change) {
                            Some((first, second)) => {
                                match second {
                                    Some(second) => queue.push(second),
                                    // One property was taken out.
                                    None => self.drop_local(&WHY),
                                }
                                queue.push(first);
                            }
                            None => self.drop_local(&WHY),
                        }
                    }
                    Err(e) => self.drop_local(&e),
                }
            }
        }
        let mut ops = 0;
        let mut chunk: Vec<Change> = Vec::new();
        let mut bytes = 0;
        let mut sized = sized.into_iter().peekable();
        while let Some((change, size)) = sized.next() {
            chunk.push(change);
            bytes += size;
            let full = match sized.peek() {
                Some((_, next)) => chunk.len() == MAX_CHANGES || bytes + next > OP_BUDGET,
                None => true,
            };
            if full {
                ops += self.commit_op(&chunk, now_ms);
                chunk.clear();
                bytes = 0;
            }
        }
        ops
    }

    fn drop_local(&mut self, why: &dyn core::fmt::Display) {
        self.dropped_local += 1;
        self.problem(format_args!("a local change is not synced: {why}"));
    }

    /// One op. Returns 1 if it was made.
    fn commit_op(&mut self, changes: &[Change], now_ms: u64) -> usize {
        let Some(replica) = self.replica.as_mut() else {
            return 0;
        };
        let made = random::<24>().and_then(|nonce| {
            replica
                .local_commit(changes, now_ms, *nonce)
                .map_err(Error::Replica)
        });
        match made {
            Ok(corrections) => {
                self.apply(&corrections);
                1
            }
            Err(e) => {
                self.dropped_local += changes.len() as u64;
                self.problem(format_args!("local changes are not synced: {e}"));
                0
            }
        }
    }

    // ------------------------------------------------------------- effects

    /// Apply what a replica emitted to the engine. Returns the number of
    /// changes applied. A change the engine refuses is skipped and counted,
    /// the others still apply.
    fn apply(&mut self, effects: &[Change]) -> usize {
        if effects.is_empty() {
            return 0;
        }
        let mut changes = Vec::with_capacity(effects.len());
        for effect in effects {
            match change_from_wire(effect) {
                Ok(change) => changes.push(change),
                Err(e) => self.skip_remote(&e),
            }
        }
        if self.db.apply_changes(&changes).is_ok() {
            return changes.len();
        }
        // The list is one transaction: find what the engine refuses.
        let mut applied = 0;
        for change in &changes {
            match self.db.apply_changes(core::slice::from_ref(change)) {
                Ok(()) => applied += 1,
                Err(e) => self.skip_remote(&e),
            }
        }
        applied
    }

    fn skip_remote(&mut self, why: &dyn core::fmt::Display) {
        self.skipped_remote += 1;
        self.problem(format_args!("a remote change was not applied: {why}"));
    }

    // ------------------------------------------------------------ transport

    /// The next `Push` frame from the outbox, or `None` when the outbox is
    /// empty. Send it to `POST /v0/push` and give the reply to
    /// [`accept_ack`](Self::accept_ack). Building a frame removes nothing.
    pub fn push_frame(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let replica = self.replica.as_ref().ok_or(Error::NotEnabled)?;
        let mut sealed = Vec::new();
        let mut bytes = 0;
        for item in replica.outbox_peek(MAX_PUSH_ITEMS) {
            if !sealed.is_empty() && bytes + item.len() > MAX_PUSH_BYTES {
                break;
            }
            bytes += item.len();
            sealed.push(SealedOp::from_bytes(item).map_err(|_| Error::Frame("outbox item"))?);
        }
        if sealed.is_empty() {
            self.in_flight = 0;
            return Ok(None);
        }
        self.in_flight = sealed.len();
        let frame = Message::Push {
            fragment_id: replica.fragment_id().to_vec(),
            sealed,
        };
        Ok(Some(frame.to_bytes().map_err(gsp_sync::Error::Encode)?))
    }

    /// Take the relay's `Ack` for the frame last built by
    /// [`push_frame`](Self::push_frame): the acknowledged items leave the
    /// outbox. Returns how many did.
    pub fn accept_ack(&mut self, reply: &[u8]) -> Result<usize, Error> {
        let replica = self.replica.as_mut().ok_or(Error::NotEnabled)?;
        let Ok(Message::Ack {
            fragment_id, count, ..
        }) = Message::from_bytes(reply)
        else {
            return Err(Error::Frame("not an Ack"));
        };
        if fragment_id != replica.fragment_id() {
            return Err(Error::Frame("Ack for another group"));
        }
        let count = (count as usize).min(core::mem::take(&mut self.in_flight));
        Ok(replica.outbox_ack(count))
    }

    /// The `Pull` frame for the current cursor, for
    /// `POST /v0/pull?wait_ms=...`. Give the reply to
    /// [`ingest_batch`](Self::ingest_batch).
    pub fn pull_frame(&self) -> Result<Vec<u8>, Error> {
        let replica = self.replica.as_ref().ok_or(Error::NotEnabled)?;
        let frame = Message::Pull {
            fragment_id: replica.fragment_id().to_vec(),
            after: replica.cursor(),
            // As many as the relay allows.
            limit: 0,
        };
        Ok(frame.to_bytes().map_err(gsp_sync::Error::Encode)?)
    }

    /// Take the relay's `Batch`: commit pending local changes first, then
    /// hand every item to the replica and apply its effects to the engine.
    ///
    /// Items the replica rejects (not from this group, damaged, invalid) are
    /// skipped; they do not fail the call. Persist after a call that
    /// returned `items > 0`, so the cursor and the data move together.
    pub fn ingest_batch(&mut self, reply: &[u8], now_ms: u64) -> Result<BatchOutcome, Error> {
        let Some(replica) = self.replica.as_ref() else {
            return Err(Error::NotEnabled);
        };
        let Ok(Message::Batch {
            fragment_id,
            next,
            more,
            sealed,
        }) = Message::from_bytes(reply)
        else {
            return Err(Error::Frame("not a Batch"));
        };
        if fragment_id != replica.fragment_id() {
            return Err(Error::Frame("Batch for another group"));
        }
        // SYNC-V0 section 5: local changes go to the replica before any
        // remote effect reaches the engine.
        self.commit_local(now_ms)?;
        let mut outcome = BatchOutcome {
            items: sealed.len(),
            more,
            ..BatchOutcome::default()
        };
        // Items are in cursor order and `next` is the position of the last.
        let count = sealed.len() as u64;
        let first = next.checked_sub(count).map(|before| before + 1);
        for (i, item) in sealed.iter().enumerate() {
            let Ok(bytes) = item.to_bytes() else {
                continue;
            };
            let relay_seq = first.map_or(0, |first| first + i as u64);
            let Some(replica) = self.replica.as_mut() else {
                break;
            };
            let ingested = replica.ingest(&bytes, relay_seq, now_ms);
            outcome.ops += ingested.applied;
            outcome.changes += self.apply(&ingested.effects);
            if ingested.status == IngestStatus::PendingFull {
                // Not kept and the cursor stays before it: do not go on, or
                // later items would be applied without it.
                self.problem("too many received changes are waiting for earlier ones");
                outcome.more = false;
                outcome.stalled = true;
                break;
            }
        }
        if let Some(replica) = self.replica.as_mut() {
            if count == 0 && next < replica.cursor() {
                // The relay holds less than this device has seen (it lost
                // its data or is another relay): read it from the start.
                // Ingesting an item again changes nothing.
                replica.set_cursor(0);
                outcome.more = next > 0;
            } else if first.is_none() && count > 0 {
                // A `next` that cannot be right; trust it only as a cursor.
                replica.set_cursor(next.max(replica.cursor()));
            }
        }
        Ok(outcome)
    }

    /// Without receiving anything: commit pending local changes, and apply
    /// received ops that were held because their clock was ahead of this
    /// device's and no longer is. Returns the changes applied to the engine.
    pub fn poll(&mut self, now_ms: u64) -> Result<usize, Error> {
        self.commit_local(now_ms)?;
        let Some(replica) = self.replica.as_mut() else {
            return Ok(0);
        };
        let effects = replica.poll(now_ms);
        Ok(self.apply(&effects))
    }
}
