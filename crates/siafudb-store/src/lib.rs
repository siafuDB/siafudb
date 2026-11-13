//! # siafudb-store
//!
//! The storage layer under SiafuDB. Every target (native, iOS/Android app
//! sandbox, browser, tests) shares this code, so pages are encrypted and laid
//! out identically everywhere.
//!
//! Layers, bottom to top:
//!
//! 1. [`PageStore`]: a synchronous, fixed-size page device. Backends:
//!    [`MemPageStore`], [`FilePageStore`] (feature `native`), and
//!    [`CallbackPageStore`], through which a host (the wasm layer, a UniFFI
//!    binding) supplies read/write/sync closures so OPFS or IndexedDB backends
//!    can live in TypeScript.
//! 2. [`EncryptedPageStore`]: wraps any `PageStore`. XChaCha20-Poly1305 per
//!    page, random 24-byte nonces, suite id / database id / page number /
//!    epoch / per-write version bound into the AAD, a wrapped data key and a
//!    key-check value in a plaintext key header. Inner stores see fixed-size
//!    physical pages of `page_size + OVERHEAD` bytes.
//! 3. [`Container`]: a crash-safe container on any `PageStore`.
//!    Double-buffered superblock (pages 0 and 1), copy-on-write named blobs
//!    (for engine snapshots) and an append-only log (for signed mutations).
//!    The superblock carries a monotonic commit counter and a keyed BLAKE3
//!    root over everything it references, so replaying an older page is
//!    detected at open.
//!
//! [`testing::FaultyStore`] simulates power loss, torn writes and failing
//! syncs for crash tests; it is public so downstream crates can reuse it.

mod codec;
mod container;
mod crypto;
mod encrypted;
mod kek;
mod store;

mod callback;
#[cfg(feature = "native")]
mod file;
mod mem;
pub mod testing;

pub use callback::{CallbackPageStore, CallbackPageStoreBuilder};
pub use container::{Container, LogEntry, OpenOptions, OpenReport};
pub use crypto::{AeadSuite, Dek, generate_dek};
pub use encrypted::{EncryptOptions, EncryptedPageStore, KeyHeader, OVERHEAD};
#[cfg(feature = "native")]
pub use file::FilePageStore;
pub use kek::{
    Argon2Params, CallbackKek, KekProvider, PassphraseKek, RawKek, UnwrappedKek, WrappedDek,
};
pub use mem::MemPageStore;
pub use store::{
    LockGuard, LockMode, MaybeSendSync, PAGE_SIZE, PageNo, PageStore, StoreCaps, StoreError,
};

/// Crate result type.
pub type Result<T, E = StoreError> = core::result::Result<T, E>;
