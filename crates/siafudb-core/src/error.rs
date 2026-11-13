// siafudb-core/src/error.rs
//
// Error types for SiafuDB.
//
// Errors are categorized by where they originate:
// - EngineError: something went wrong in the GrafeoDB engine
// - QueryError: the user's query was malformed or failed
// - FragmentError: something related to fragment management
// - IdentityError: cryptographic identity operations failed
// - SyncError: the sync protocol encountered a problem

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SiafuError {
    /// The underlying graph engine encountered an error.
    #[error("engine error: {0}")]
    EngineError(String),

    /// A query failed to parse or execute.
    #[error("query error: {0}")]
    QueryError(String),

    /// A fragment operation failed.
    #[error("fragment error: {0}")]
    FragmentError(String),

    /// A cryptographic identity operation failed.
    #[error("identity error: {0}")]
    IdentityError(String),

    /// The sync protocol encountered an error.
    #[error("sync error: {0}")]
    SyncError(String),

    /// `apply_changes` met an edge whose endpoint node is not known here.
    /// Nothing of the batch was applied.
    #[error(
        "sync error: edge {} names unknown endpoint {}",
        crate::sync::gid_to_hex(.edge),
        crate::sync::gid_to_hex(.endpoint)
    )]
    UnknownEndpoint {
        /// Gid of the edge.
        edge: [u8; 16],
        /// Gid of the missing node.
        endpoint: [u8; 16],
    },

    /// A change given to `apply_changes` is malformed or contradicts the
    /// graph (a reserved property, an edge re-pointed to other endpoints).
    #[error("invalid change: {0}")]
    InvalidChange(String),

    /// A property value has no lossless form in a `GraphChange`.
    #[error("unsupported value: {0}")]
    UnsupportedValue(String),

    /// An I/O error occurred (file operations, etc).
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    /// A serialization/deserialization error occurred.
    #[error("serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),
}
