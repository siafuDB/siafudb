//! Key-encryption keys: wrap and unwrap the data key (DEK).
//!
//! The DEK encrypts pages with a sync Rust AEAD. A KEK protects the DEK at
//! rest: a passphrase (Argon2id), a raw key (tests, or a key already held by
//! the platform keystore), or the host (Secure Enclave, Android Keystore,
//! WebCrypto non-extractable key) through [`CallbackKek`] or, for async
//! hosts, the two-step `KeyHeader::read` + [`UnwrappedKek`] flow.
//!
//! Honest limit: the DEK is in process memory for the life of the session.
//! Code execution in the app or origin can read it; the KEK protects the
//! key at rest, not in use.

use zeroize::Zeroizing;

use crate::Result;
use crate::crypto::{Dek, open_blob, seal_blob};
use crate::store::StoreError;

/// A wrapped DEK as stored in the key header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrappedDek {
    /// Which provider produced it, e.g. `"argon2id/v1"`, `"raw/v1"`,
    /// `"secure-enclave/v1"` (at most 255 bytes).
    pub kek_id: String,
    /// Provider parameters (salt, cost), opaque to the store.
    pub params: Vec<u8>,
    /// Wrapped key bytes, opaque to the store.
    pub ciphertext: Vec<u8>,
}

/// Wraps and unwraps the data key. `context` binds the wrap to one store
/// (suite, database id, epoch); providers must authenticate it (use it as
/// AAD) so a wrapped key cannot be transplanted between databases.
pub trait KekProvider: crate::MaybeSendSync {
    /// Provider id stored in the header.
    fn kek_id(&self) -> &str;
    /// Wrap `dek` for storage.
    fn wrap(&self, dek: &[u8; 32], context: &[u8]) -> Result<WrappedDek>;
    /// Unwrap; `WrongKey` if this KEK did not wrap it.
    fn unwrap(&self, wrapped: &WrappedDek, context: &[u8]) -> Result<Dek>;
}

fn to_dek(bytes: &[u8]) -> Result<Dek> {
    let arr: [u8; 32] = bytes.try_into().map_err(|_| StoreError::WrongKey)?;
    Ok(Zeroizing::new(arr))
}

/// A raw 256-bit KEK (tests; or a key the platform keystore releases).
pub struct RawKek {
    key: Zeroizing<[u8; 32]>,
}

impl RawKek {
    /// KEK id.
    pub const ID: &'static str = "raw/v1";
    /// From key bytes.
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key: Zeroizing::new(key),
        }
    }
}

impl KekProvider for RawKek {
    fn kek_id(&self) -> &str {
        Self::ID
    }
    fn wrap(&self, dek: &[u8; 32], context: &[u8]) -> Result<WrappedDek> {
        Ok(WrappedDek {
            kek_id: Self::ID.into(),
            params: Vec::new(),
            ciphertext: seal_blob(&self.key, context, dek)?,
        })
    }
    fn unwrap(&self, w: &WrappedDek, context: &[u8]) -> Result<Dek> {
        if w.kek_id != Self::ID {
            return Err(StoreError::WrongKey);
        }
        let plain = open_blob(&self.key, context, &w.ciphertext).ok_or(StoreError::WrongKey)?;
        to_dek(&plain)
    }
}

/// Argon2id cost parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argon2Params {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Iterations.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl Argon2Params {
    /// OWASP baseline (19 MiB, t=2, p=1): usable on phones and in browsers.
    pub const DEFAULT: Self = Self {
        m_kib: 19 * 1024,
        t: 2,
        p: 1,
    };
    /// Minimal cost. Tests only.
    pub const INSECURE_TEST: Self = Self {
        m_kib: 8,
        t: 1,
        p: 1,
    };
    /// Upper bound accepted when reading a header (a tampered header must not
    /// make open allocate unbounded memory).
    pub const MAX_M_KIB: u32 = 4 * 1024 * 1024;
}

/// Passphrase KEK: Argon2id(passphrase, random 16-byte salt) -> 256-bit key,
/// which wraps the DEK with XChaCha20-Poly1305. Salt and cost are stored in
/// `WrappedDek::params`; `unwrap` uses the stored cost, so it can be raised
/// for new databases without breaking old ones.
pub struct PassphraseKek {
    passphrase: Zeroizing<Vec<u8>>,
    params: Argon2Params,
}

impl PassphraseKek {
    /// KEK id.
    pub const ID: &'static str = "argon2id/v1";
    /// Passphrase with default cost.
    pub fn new(passphrase: &[u8]) -> Self {
        Self::with_params(passphrase, Argon2Params::DEFAULT)
    }
    /// Passphrase with explicit cost (used for `wrap`).
    pub fn with_params(passphrase: &[u8], params: Argon2Params) -> Self {
        Self {
            passphrase: Zeroizing::new(passphrase.to_vec()),
            params,
        }
    }

    fn derive(&self, p: Argon2Params, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
        if p.m_kib > Argon2Params::MAX_M_KIB || p.t == 0 || p.t > 64 || p.p == 0 || p.p > 16 {
            return Err(StoreError::Crypto("argon2 parameters out of range".into()));
        }
        let params = argon2::Params::new(p.m_kib, p.t, p.p, Some(32))
            .map_err(|e| StoreError::Crypto(format!("argon2: {e}")))?;
        let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut out = Zeroizing::new([0u8; 32]);
        a.hash_password_into(&self.passphrase, salt, &mut out[..])
            .map_err(|e| StoreError::Crypto(format!("argon2: {e}")))?;
        Ok(out)
    }
}

impl KekProvider for PassphraseKek {
    fn kek_id(&self) -> &str {
        Self::ID
    }
    fn wrap(&self, dek: &[u8; 32], context: &[u8]) -> Result<WrappedDek> {
        let mut salt = [0u8; 16];
        crate::crypto::fill_random(&mut salt)?;
        let p = self.params;
        let kek = self.derive(p, &salt)?;
        let mut params = Vec::with_capacity(28);
        params.extend_from_slice(&p.m_kib.to_le_bytes());
        params.extend_from_slice(&p.t.to_le_bytes());
        params.extend_from_slice(&p.p.to_le_bytes());
        params.extend_from_slice(&salt);
        Ok(WrappedDek {
            kek_id: Self::ID.into(),
            ciphertext: seal_blob(&kek, &[context, &params].concat(), dek)?,
            params,
        })
    }
    fn unwrap(&self, w: &WrappedDek, context: &[u8]) -> Result<Dek> {
        if w.kek_id != Self::ID || w.params.len() != 28 {
            return Err(StoreError::WrongKey);
        }
        let u = |i: usize| u32::from_le_bytes(w.params[i..i + 4].try_into().expect("4 bytes"));
        let p = Argon2Params {
            m_kib: u(0),
            t: u(4),
            p: u(8),
        };
        let kek = self.derive(p, &w.params[12..28])?;
        let plain = open_blob(&kek, &[context, &w.params].concat(), &w.ciphertext)
            .ok_or(StoreError::WrongKey)?;
        to_dek(&plain)
    }
}

#[cfg(not(target_arch = "wasm32"))]
type HostWrap = Box<dyn Fn(&[u8; 32], &[u8]) -> Result<WrappedDek> + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type HostWrap = Box<dyn Fn(&[u8; 32], &[u8]) -> Result<WrappedDek>>;
#[cfg(not(target_arch = "wasm32"))]
type HostUnwrap = Box<dyn Fn(&WrappedDek, &[u8]) -> Result<Zeroizing<Vec<u8>>> + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type HostUnwrap = Box<dyn Fn(&WrappedDek, &[u8]) -> Result<Zeroizing<Vec<u8>>>>;

/// Host-supplied synchronous KEK (Secure Enclave / Android Keystore through
/// UniFFI callback interfaces). The store verifies the unwrapped key against
/// its key-check value, so a host returning the wrong bytes fails cleanly.
pub struct CallbackKek {
    id: String,
    wrap: HostWrap,
    unwrap: HostUnwrap,
}

impl CallbackKek {
    /// `wrap(dek, context)` and `unwrap(wrapped, context)` closures.
    pub fn new(
        kek_id: impl Into<String>,
        wrap: impl Fn(&[u8; 32], &[u8]) -> Result<WrappedDek> + crate::MaybeSendSync + 'static,
        unwrap: impl Fn(&WrappedDek, &[u8]) -> Result<Zeroizing<Vec<u8>>>
        + crate::MaybeSendSync
        + 'static,
    ) -> Self {
        Self {
            id: kek_id.into(),
            wrap: Box::new(wrap),
            unwrap: Box::new(unwrap),
        }
    }
}

impl KekProvider for CallbackKek {
    fn kek_id(&self) -> &str {
        &self.id
    }
    fn wrap(&self, dek: &[u8; 32], context: &[u8]) -> Result<WrappedDek> {
        (self.wrap)(dek, context)
    }
    fn unwrap(&self, w: &WrappedDek, context: &[u8]) -> Result<Dek> {
        to_dek(&(self.unwrap)(w, context)?)
    }
}

/// For async hosts (WebCrypto). Open: the host reads the header with
/// [`crate::KeyHeader::read`], unwraps the DEK itself (awaiting
/// `crypto.subtle`), then opens with this provider; the key-check value
/// still rejects a wrong DEK. Create: use
/// [`crate::EncryptedPageStore::create_with_wrapped`] instead.
pub struct UnwrappedKek {
    dek: Dek,
}

impl UnwrappedKek {
    /// The DEK the host already unwrapped.
    pub fn new(dek: Dek) -> Self {
        Self { dek }
    }
}

impl KekProvider for UnwrappedKek {
    fn kek_id(&self) -> &str {
        "host/unwrapped"
    }
    fn wrap(&self, _dek: &[u8; 32], _context: &[u8]) -> Result<WrappedDek> {
        Err(StoreError::InvalidArgument(
            "UnwrappedKek cannot wrap; use EncryptedPageStore::create_with_wrapped".into(),
        ))
    }
    fn unwrap(&self, _w: &WrappedDek, _context: &[u8]) -> Result<Dek> {
        Ok(self.dek.clone())
    }
}
