//! AEAD suite registry, data keys, randomness.

use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::{Key, KeyInit, Tag, XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::Result;
use crate::store::StoreError;

/// Data-encryption key (256-bit). Zeroized on drop.
pub type Dek = Zeroizing<[u8; 32]>;

/// AEAD suite registry. Ids are numeric (never strings) and stored in every
/// page header and in the AAD, so a page cannot be reinterpreted under
/// another suite.
///
/// | id | suite | status |
/// |----|-------|--------|
/// | 0 | — | invalid |
/// | 1 | XChaCha20-Poly1305, 24-byte random nonce, 16-byte tag | default |
/// | 2 | AES-256-GCM | reserved (12-byte nonces are too short for random nonces at scale; would need SIV or a nonce-derivation step) |
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
#[non_exhaustive]
pub enum AeadSuite {
    /// XChaCha20-Poly1305 (RustCrypto `chacha20poly1305`).
    XChaCha20Poly1305 = 1,
}

impl AeadSuite {
    /// Registry id.
    pub fn id(self) -> u16 {
        self as u16
    }
    /// Look up a registry id.
    pub fn from_id(id: u16) -> Result<Self> {
        match id {
            1 => Ok(Self::XChaCha20Poly1305),
            other => Err(StoreError::UnsupportedSuite(other)),
        }
    }
    /// Nonce length in bytes.
    pub fn nonce_len(self) -> usize {
        24
    }
    /// Tag length in bytes.
    pub fn tag_len(self) -> usize {
        16
    }
}

/// Fill `buf` from the OS / browser CSPRNG.
pub(crate) fn fill_random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| StoreError::Crypto(format!("rng: {e}")))
}

/// Fresh random data key (for hosts that wrap it themselves, e.g. WebCrypto).
pub fn generate_dek() -> Result<Dek> {
    let mut k = Zeroizing::new([0u8; 32]);
    fill_random(&mut k[..])?;
    Ok(k)
}

/// A keyed AEAD instance for one suite.
pub(crate) struct Cipher {
    inner: XChaCha20Poly1305,
}

impl Cipher {
    pub(crate) fn new(suite: AeadSuite, key: &[u8; 32]) -> Self {
        match suite {
            AeadSuite::XChaCha20Poly1305 => Self {
                inner: XChaCha20Poly1305::new(&Key::from(*key)),
            },
        }
    }

    /// Encrypt `buf` in place; returns the tag.
    pub(crate) fn seal(&self, nonce: &[u8; 24], aad: &[u8], buf: &mut [u8]) -> Result<[u8; 16]> {
        let tag = self
            .inner
            .encrypt_inout_detached(&XNonce::from(*nonce), aad, buf.into())
            .map_err(|_| StoreError::Crypto("encrypt".into()))?;
        Ok(tag.into())
    }

    /// Decrypt `buf` in place. On failure `buf` is zeroed (never leak
    /// unauthenticated plaintext) and `false` is returned.
    pub(crate) fn open(
        &self,
        nonce: &[u8; 24],
        aad: &[u8],
        buf: &mut [u8],
        tag: &[u8; 16],
    ) -> bool {
        let ok = self
            .inner
            .decrypt_inout_detached(&XNonce::from(*nonce), aad, buf.into(), &Tag::from(*tag))
            .is_ok();
        if !ok {
            buf.fill(0);
        }
        ok
    }
}

/// Seal a short secret as `nonce || ciphertext || tag` (key wrapping).
pub(crate) fn seal_blob(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    let c = Cipher::new(AeadSuite::XChaCha20Poly1305, key);
    let mut nonce = [0u8; 24];
    fill_random(&mut nonce)?;
    let mut body = plain.to_vec();
    let tag = c.seal(&nonce, aad, &mut body)?;
    let mut out = Vec::with_capacity(24 + body.len() + 16);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    out.extend_from_slice(&tag);
    zeroize::Zeroize::zeroize(&mut body);
    Ok(out)
}

/// Inverse of [`seal_blob`]. `None` if it does not authenticate.
pub(crate) fn open_blob(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < 40 {
        return None;
    }
    let c = Cipher::new(AeadSuite::XChaCha20Poly1305, key);
    let nonce: [u8; 24] = sealed[..24].try_into().ok()?;
    let tag: [u8; 16] = sealed[sealed.len() - 16..].try_into().ok()?;
    let mut body = Zeroizing::new(sealed[24..sealed.len() - 16].to_vec());
    c.open(&nonce, aad, &mut body, &tag).then_some(body)
}

/// Constant-time equality for fixed-size secrets.
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
