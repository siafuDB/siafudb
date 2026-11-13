// siafudb-core/src/sync/mod.rs
//
// Structured change capture and apply for device-to-device sync.
//
// This is the engine side of sync: it turns what local writes did into a
// list of changes keyed by global ids, and applies such a list coming from
// another device. It knows nothing about transports, signatures or conflict
// resolution; a protocol layer sits on top and only sees `GraphChange`.
//
// How it works (details in `capture.rs`):
// - Capture reads the engine's change-data-capture log (`cdc` feature), which
//   reports every committed write of the default graph, whatever API made it.
// - Engine ids are local integers, so every captured or applied entity gets a
//   random 16-byte global id, stored on it as the reserved property `_gid`
//   (32 hex digits) and indexed in memory in both directions.
// - Remote changes are applied in a session whose commits are not reported
//   to the CDC log, so they can never come back out of `take_changes`.

mod capture;
mod codec;
mod value;

pub(crate) use capture::SyncState;
pub(crate) use codec::{append_trailer, split_trailer};
pub use value::{ChangeValue, ENGINE_VALUE_KEY};

/// A global entity id: 16 random bytes, the same on every device.
pub type Gid = [u8; 16];

/// The reserved property that holds an entity's [`Gid`] as 32 lowercase hex
/// digits. Capture never reports it and apply refuses changes that name it.
pub const GID_PROPERTY: &str = "_gid";

/// One change to the graph, in terms of global ids.
///
/// Lists of changes are ordered: apply them in the order given.
#[derive(Debug, Clone, PartialEq)]
pub enum GraphChange {
    /// The node exists, with this delta applied: labels to add and remove,
    /// properties to set and to remove. For a node the receiver does not
    /// have, it creates the node from `labels_add` and `set`.
    NodePut {
        gid: Gid,
        labels_add: Vec<String>,
        labels_remove: Vec<String>,
        set: Vec<(String, ChangeValue)>,
        unset: Vec<String>,
    },
    /// The node and every edge attached to it are gone.
    NodeDel { gid: Gid },
    /// The edge exists between `src` and `dst` with type `edge_type` (carried
    /// every time), with this property delta applied.
    EdgePut {
        gid: Gid,
        src: Gid,
        dst: Gid,
        edge_type: String,
        set: Vec<(String, ChangeValue)>,
        unset: Vec<String>,
    },
    /// The edge is gone.
    EdgeDel { gid: Gid },
}

/// A gid as 32 lowercase hex digits (the form stored in [`GID_PROPERTY`]).
pub fn gid_to_hex(gid: &Gid) -> String {
    use std::fmt::Write;
    gid.iter().fold(String::with_capacity(32), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Parse 32 hex digits into a gid.
pub fn gid_from_hex(hex: &str) -> Option<Gid> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut gid = [0u8; 16];
    for (i, byte) in gid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(gid)
}
