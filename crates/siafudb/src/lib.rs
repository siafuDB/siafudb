// siafudb — The unified facade crate.
//
// This is what users depend on. It re-exports the core engine plus
// the access patterns and (optionally) the sync protocol, so a caller
// can `use siafudb::SiafuDB` without juggling sub-crates.

pub use siafudb_core::{
    DeploymentProfile, Fragment, FragmentConfig, FragmentKind, NodeAuthority, SiafuConfig,
    SiafuDB, SiafuError,
};

pub mod access {
    pub use siafudb_access::*;
}

pub mod sync {
    pub use siafudb_sync::*;
}
