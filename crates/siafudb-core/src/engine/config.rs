// siafudb-core/src/engine/config.rs
//
// Configuration for a SiafuDB instance.
//
// The config determines how this instance behaves — whether it's a personal
// database on a phone, an edge cache, a Honeycomb node fragment, or a
// full platform database in the cloud. The engine is the same; the config
// shapes how it operates.

use serde::{Deserialize, Serialize};

/// Configuration for a SiafuDB instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiafuConfig {
    /// How this instance identifies itself in the sync protocol.
    /// Defaults to a random UUID if not specified.
    pub instance_name: Option<String>,

    /// The deployment profile for this instance.
    /// Affects default memory limits, sync behavior, and feature availability.
    pub profile: DeploymentProfile,

    /// Whether the sync protocol mutation log is enabled.
    /// Default: true. Disable for bulk import performance.
    pub mutation_tracking: bool,

    /// Maximum memory budget for the database engine.
    /// On mobile devices, this should be conservative (e.g., 64MB).
    /// On servers, this can be generous (e.g., 4GB).
    /// None means let the engine decide based on available system memory.
    pub memory_limit_bytes: Option<usize>,
}

/// The deployment context for this SiafuDB instance.
///
/// Each profile adjusts defaults for its environment. The engine is
/// the same everywhere — profiles just tune the knobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentProfile {
    /// Running on a user's mobile device.
    /// Optimized for: small memory footprint, fast startup, battery efficiency.
    /// Mutation tracking: enabled (for personal pod sync).
    /// Default memory limit: 64MB.
    Mobile,

    /// Running in an edge compute environment (Cloudflare Workers, WASM runtime).
    /// Optimized for: minimal binary size, fast cold start, ephemeral storage.
    /// Mutation tracking: configurable.
    /// Default memory limit: 32MB.
    Edge,

    /// Running as a Honeycomb network node (user has opted in).
    /// Optimized for: serving network fragments, participating in sync.
    /// Mutation tracking: enabled (for network sync).
    /// Default memory limit: 128MB.
    HoneycombNode,

    /// Running as a development/testing server.
    /// Optimized for: developer experience, full feature availability.
    /// Mutation tracking: enabled.
    /// Default memory limit: 1GB.
    Server,

    /// Running in a cloud deployment (the platform database).
    /// Optimized for: throughput, large graphs, full query capabilities.
    /// Mutation tracking: enabled (for downstream sync to edge/device).
    /// Default memory limit: system-dependent.
    Cloud,
}

impl Default for SiafuConfig {
    fn default() -> Self {
        Self {
            instance_name: None,
            profile: DeploymentProfile::Server,
            mutation_tracking: true,
            memory_limit_bytes: None,
        }
    }
}

impl SiafuConfig {
    /// Create a config optimized for mobile deployment.
    pub fn mobile() -> Self {
        Self {
            instance_name: None,
            profile: DeploymentProfile::Mobile,
            mutation_tracking: true,
            memory_limit_bytes: Some(64 * 1024 * 1024), // 64MB
        }
    }

    /// Create a config optimized for edge deployment.
    pub fn edge() -> Self {
        Self {
            instance_name: None,
            profile: DeploymentProfile::Edge,
            mutation_tracking: false,
            memory_limit_bytes: Some(32 * 1024 * 1024), // 32MB
        }
    }

    /// Create a config for a Honeycomb network node.
    pub fn honeycomb_node() -> Self {
        Self {
            instance_name: None,
            profile: DeploymentProfile::HoneycombNode,
            mutation_tracking: true,
            memory_limit_bytes: Some(128 * 1024 * 1024), // 128MB
        }
    }
}
