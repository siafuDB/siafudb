// siafudb-core/src/fragment/types.rs
//
// Data structures for the fragment model.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Describes the scope and authority of this SiafuDB instance's data.
///
/// Every SiafuDB instance is a fragment. The Fragment struct tells the
/// sync protocol what this instance holds and how to handle it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fragment {
    /// Unique identifier for this fragment.
    pub id: Uuid,

    /// What kind of fragment this is.
    pub kind: FragmentKind,

    /// The identity that owns this fragment's authoritative data.
    /// For a personal device, this is the user's cryptographic identity.
    /// For a platform fragment, this is the platform's signing key.
    pub owner_identity: Option<String>,

    /// Configuration for how this fragment participates in sync.
    pub config: FragmentConfig,
}

/// The kind of fragment this instance represents.
///
/// This determines the default sync behavior and the authority model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FragmentKind {
    /// A user's personal graph on their device.
    ///
    /// Contains: messages, preferences, history, personal content.
    /// Authority: this fragment is authoritative for personal data.
    /// Sync direction: pushes personal data to pod; pulls platform data in.
    /// Isolation: completely separate from any Honeycomb network data.
    Personal,

    /// A user's sovereign pod (Web3 persistent storage).
    ///
    /// Contains: the complete personal graph, replicated from devices.
    /// Authority: the canonical persistent store for personal data.
    /// Sync direction: receives from devices; serves to devices.
    Pod,

    /// A fragment of the Honeycomb network (user opted in to hosting).
    ///
    /// Contains: a subset of platform/network data assigned by the network.
    /// Authority: this fragment caches network data; the platform is authoritative.
    /// Sync direction: receives network data; serves it to nearby nodes.
    /// Isolation: completely separate from the user's personal data.
    Network,

    /// The platform's authoritative graph.
    ///
    /// Contains: places, businesses, news, public content, the knowledge graph.
    /// Authority: this is the source of truth for platform data.
    /// Sync direction: pushes to edge/device fragments that reference its data.
    Platform,

    /// A standalone local database with no sync participation.
    ///
    /// This is the "just use it like SQLite" mode. No sync, no fragments,
    /// no network participation. Just a local graph database.
    Local,
}

/// Configuration for how a fragment participates in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentConfig {
    /// Whether this fragment pushes its authoritative mutations outward.
    pub sync_push: bool,

    /// Whether this fragment pulls referenced data from other fragments.
    pub sync_pull: bool,

    /// How long to keep referenced (non-authoritative) data before eviction.
    /// None means keep forever (or until storage pressure requires eviction).
    pub reference_ttl_seconds: Option<u64>,

    /// Maximum storage size for this fragment.
    /// The fragment will evict non-authoritative data to stay within this limit.
    /// Authoritative data is never evicted (it would be data loss).
    pub max_storage_bytes: Option<u64>,
}

/// Describes the authority relationship of a specific node in the graph.
///
/// When the sync protocol encounters a mutation, it checks the node's
/// authority to determine how to handle it:
/// - Authoritative nodes: mutations are trusted and propagated outward.
/// - Referenced nodes: mutations are validated against the authoritative source.
/// - Local nodes: mutations stay local and don't participate in sync.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeAuthority {
    /// This fragment is the authoritative source for this node.
    /// Mutations originating here are trusted and propagated.
    Authoritative,

    /// This node is referenced from another fragment.
    /// Contains a reference to the authoritative source.
    Referenced {
        /// The fragment ID of the authoritative source.
        source_fragment: Uuid,
        /// When this reference was last refreshed.
        last_refreshed: Option<u64>,
    },

    /// This node is local-only and doesn't participate in sync.
    /// Used for temporary data, caches, and session state.
    Local,
}

impl Fragment {
    /// Create a new local fragment (no sync, just a database).
    /// This is the default for someone who just wants to use SiafuDB
    /// like SQLite — open a file, store data, query it.
    pub fn new_local() -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: FragmentKind::Local,
            owner_identity: None,
            config: FragmentConfig {
                sync_push: false,
                sync_pull: false,
                reference_ttl_seconds: None,
                max_storage_bytes: None,
            },
        }
    }

    /// Create a personal fragment for a user's device.
    pub fn new_personal(owner_identity: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: FragmentKind::Personal,
            owner_identity: Some(owner_identity),
            config: FragmentConfig {
                sync_push: true,
                sync_pull: true,
                reference_ttl_seconds: Some(7 * 24 * 3600), // 7 days for referenced data
                max_storage_bytes: None,
            },
        }
    }

    /// Create a network fragment for a Honeycomb node.
    pub fn new_network() -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: FragmentKind::Network,
            owner_identity: None,
            config: FragmentConfig {
                sync_push: false, // Network fragments don't author data
                sync_pull: true,  // They receive data from the platform
                reference_ttl_seconds: Some(24 * 3600), // 24 hours
                max_storage_bytes: Some(512 * 1024 * 1024), // 512MB cap
            },
        }
    }

    /// Create a platform fragment (the authoritative knowledge graph).
    pub fn new_platform(platform_identity: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: FragmentKind::Platform,
            owner_identity: Some(platform_identity),
            config: FragmentConfig {
                sync_push: true,
                sync_pull: false, // Platform is the source of truth
                reference_ttl_seconds: None,
                max_storage_bytes: None,
            },
        }
    }
}
