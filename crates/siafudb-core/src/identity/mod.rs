// siafudb-core/src/identity
//
// Cryptographic identity for SiafuDB nodes. Every node carries provenance
// tied to its authoritative source — ed25519 signing keys, content hashes,
// and a reference to the fragment that authored the data.
//
// TODO: implementation. This module is referenced by the engine but the
// types live in a future commit (Phase 2: cryptographic identity).

use serde::{Deserialize, Serialize};

/// Cryptographic identity attached to a graph node.
///
/// Placeholder — the real implementation will hold an ed25519 public key,
/// a content hash, and the fragment ID of the authoritative source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub fragment_id: uuid::Uuid,
}
