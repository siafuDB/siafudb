//! # siafudb-gsp
//!
//! The engine adapter between a [`SiafuDB`](siafudb_core::SiafuDB) and a
//! Graph Sync Protocol v0 replica ([`gsp_sync::Replica`], specified in
//! `docs/SYNC-V0.md` of the protocol repository).
//!
//! [`SyncedDb`] owns both and keeps them consistent:
//!
//! - after a local write, [`SyncedDb::commit_local`] takes what the engine
//!   captured, hands it to the replica as local commits (which signs, seals
//!   and queues them in its outbox) and applies the corrections the replica
//!   answers with;
//! - received data goes through [`SyncedDb::ingest_batch`], which first
//!   commits pending local changes and then applies the replica's effects to
//!   the engine;
//! - [`SyncedDb::to_bytes`] writes the engine snapshot and the replica state
//!   into **one** blob, so a store that writes it atomically can never hold a
//!   graph and a replica that disagree.
//!
//! Like the replica, this crate does no network I/O: it builds and reads the
//! `Push`, `Ack`, `Pull` and `Batch` frames and a transport moves the bytes.
//! Time is passed in by the caller (Unix milliseconds). Random bytes (root
//! secret, device seed, nonces) come from the operating system through
//! `getrandom`.
//!
//! The blob holds the fragment's root secret and the device key. Store it
//! encrypted.
//!
//! New, unaudited code, like the protocol crates under it.
#![forbid(unsafe_code)]

mod blob;
mod convert;
mod synced;

pub use convert::{ConvertError, change_from_wire, change_to_wire, value_from_wire, value_to_wire};
pub use gsp_sync::{OriginError, canonical_origin};
pub use synced::{
    BatchOutcome, DeviceStatus, Error, SYNC_STATE_VERSION, SYNC_STATE_VERSION_PREFIX, SyncStatus,
    SyncedDb,
};
