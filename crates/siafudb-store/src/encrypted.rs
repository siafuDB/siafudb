//! Page encryption, identical on every target.
//!
//! Physical layout of the inner store (`P = page_size + OVERHEAD`):
//!
//! ```text
//! physical 0, 1   key header copies (plaintext; see KeyHeader)
//! physical n + 2  logical page n:
//!                 [suite u16][epoch u32][version u64][nonce 24][ciphertext page_size][tag 16]
//! ```
//!
//! AAD = "SFPG" || suite || db_id || page_no || epoch || version. Binding the
//! suite stops cross-suite reinterpretation, db_id stops moving pages between
//! databases, page_no stops moving pages between slots, epoch separates key
//! generations, and the per-write version makes every write's AAD distinct.
//!
//! Nonces are 24 random bytes per write, not counters: a counter lives in
//! state that can be cloned or restored (device backup restore, VM snapshot,
//! a copied browser profile, two tabs racing), and a restored counter reuses
//! nonces under the same key, which breaks Poly1305 and leaks plaintext XOR.
//! With 192-bit random nonces the collision bound stays negligible for any
//! realistic number of writes, and no state needs to be trusted.
//!
//! What this layer does *not* do on its own is detect replay of an older,
//! authentic page into the same slot: that needs a trusted record of what is
//! current. [`crate::Container`] keeps that record (keyed BLAKE3 root in its
//! superblock) using [`PageStore::commitment_key`], a subkey of the DEK.

use core::sync::atomic::{AtomicU64, Ordering};

use zeroize::Zeroizing;

use crate::Result;
use crate::codec::{Dec, Enc};
use crate::crypto::{AeadSuite, Cipher, Dek, ct_eq, fill_random, generate_dek};
use crate::kek::{KekProvider, WrappedDek};
use crate::store::{
    LockGuard, LockMode, PAGE_SIZE, PageNo, PageStore, StoreCaps, StoreError, check_len,
};

const HDR_SUITE: usize = 0;
const HDR_EPOCH: usize = 2;
const HDR_VERSION: usize = 6;
const HDR_NONCE: usize = 14;
const HDR_BODY: usize = 38;
const TAG_LEN: usize = 16;

/// Bytes added to every logical page: suite (2) + epoch (4) + version (8) +
/// nonce (24) + tag (16).
pub const OVERHEAD: usize = HDR_BODY + TAG_LEN;

/// Physical pages reserved for the key header copies.
const HEADER_PAGES: u64 = 2;
/// Smallest logical page size (the key header must fit in one physical page).
const MIN_PAGE_SIZE: usize = 512;
const KEY_MAGIC: &[u8; 8] = b"SIAFUKEY";
const KEY_FORMAT: u16 = 1;
const CHECKSUM_LEN: usize = 32;

/// Options for [`EncryptedPageStore::create`].
#[derive(Clone, Debug)]
pub struct EncryptOptions {
    /// Logical page size seen by callers (default [`PAGE_SIZE`]).
    pub page_size: usize,
    /// AEAD suite (default XChaCha20-Poly1305).
    pub suite: AeadSuite,
    /// Database id bound into every page's AAD. Random if `None`.
    pub db_id: Option<[u8; 16]>,
}

impl Default for EncryptOptions {
    fn default() -> Self {
        Self {
            page_size: PAGE_SIZE,
            suite: AeadSuite::XChaCha20Poly1305,
            db_id: None,
        }
    }
}

/// The plaintext key header, stored twice (physical pages 0 and 1) with a
/// BLAKE3 checksum each so one torn copy is survivable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyHeader {
    /// Suite registry id.
    pub suite: u16,
    /// Logical page size.
    pub page_size: usize,
    /// Key epoch (generation of the DEK).
    pub epoch: u32,
    /// Database id.
    pub db_id: [u8; 16],
    /// Key-check value: keyed BLAKE3 under a DEK subkey over the wrap
    /// context. Lets a wrong key fail before any page is decrypted, even
    /// when a host KEK returns arbitrary bytes.
    pub kcv: [u8; 32],
    /// The wrapped DEK.
    pub wrapped: WrappedDek,
}

fn wrap_context(suite: u16, db_id: &[u8; 16], epoch: u32, page_size: usize) -> Vec<u8> {
    let mut e = Enc::default();
    e.raw(b"siafudb-store/wrap/v1")
        .u16(suite)
        .raw(db_id)
        .u32(epoch)
        .u32(page_size as u32);
    e.0
}

fn kcv_of(dek: &[u8; 32], ctx: &[u8]) -> [u8; 32] {
    let k = Zeroizing::new(blake3::derive_key("siafudb-store v1 key check", dek));
    *blake3::keyed_hash(&k, ctx).as_bytes()
}

impl KeyHeader {
    /// Context the KEK must authenticate when wrapping/unwrapping this DEK.
    pub fn wrap_context(&self) -> Vec<u8> {
        wrap_context(self.suite, &self.db_id, self.epoch, self.page_size)
    }

    /// Read the first valid header copy from an inner (physical) store, for
    /// hosts that unwrap the DEK asynchronously.
    pub fn read<S: PageStore + ?Sized>(inner: &S) -> Result<Self> {
        let mut copies = read_headers(inner)?;
        Ok(copies.remove(0))
    }

    fn encode(&self, phys: usize) -> Result<Vec<u8>> {
        let w = &self.wrapped;
        if w.kek_id.len() > 255
            || w.params.len() > u16::MAX as usize
            || w.ciphertext.len() > u16::MAX as usize
        {
            return Err(StoreError::InvalidArgument("wrapped key too large".into()));
        }
        let mut e = Enc::default();
        e.raw(KEY_MAGIC)
            .u16(KEY_FORMAT)
            .u16(self.suite)
            .u32(self.page_size as u32)
            .u32(self.epoch)
            .raw(&self.db_id)
            .raw(&self.kcv)
            .u8(w.kek_id.len() as u8)
            .raw(w.kek_id.as_bytes())
            .u16(w.params.len() as u16)
            .raw(&w.params)
            .u16(w.ciphertext.len() as u16)
            .raw(&w.ciphertext);
        let mut page = e.0;
        if page.len() + CHECKSUM_LEN > phys {
            return Err(StoreError::InvalidArgument(
                "key header does not fit in a page".into(),
            ));
        }
        page.resize(phys - CHECKSUM_LEN, 0);
        let sum = blake3::hash(&page);
        page.extend_from_slice(sum.as_bytes());
        Ok(page)
    }

    fn decode(page: &[u8]) -> Result<Self> {
        let bad = || StoreError::Corrupt("key header".into());
        if page.len() < CHECKSUM_LEN {
            return Err(bad());
        }
        let (body, sum) = page.split_at(page.len() - CHECKSUM_LEN);
        if blake3::hash(body).as_bytes() != sum {
            return Err(StoreError::Corrupt("key header checksum".into()));
        }
        let mut d = Dec::new(body, "key header");
        if &d.arr::<8>()? != KEY_MAGIC {
            return Err(bad());
        }
        if d.u16()? != KEY_FORMAT {
            return Err(StoreError::Corrupt("unknown key header format".into()));
        }
        let suite = d.u16()?;
        let page_size = d.u32()? as usize;
        let epoch = d.u32()?;
        let db_id = d.arr()?;
        let kcv = d.arr()?;
        let n = d.u8()? as usize;
        let kek_id = String::from_utf8(d.take(n)?.to_vec()).map_err(|_| bad())?;
        let n = d.u16()? as usize;
        let params = d.take(n)?.to_vec();
        let n = d.u16()? as usize;
        let ciphertext = d.take(n)?.to_vec();
        Ok(Self {
            suite,
            page_size,
            epoch,
            db_id,
            kcv,
            wrapped: WrappedDek {
                kek_id,
                params,
                ciphertext,
            },
        })
    }
}

/// Valid header copies, in slot order. Errors if none is valid.
fn read_headers<S: PageStore + ?Sized>(inner: &S) -> Result<Vec<KeyHeader>> {
    let count = inner.page_count()?;
    if count == 0 {
        return Err(StoreError::NotInitialized);
    }
    let phys = inner.page_size();
    let mut buf = vec![0u8; phys];
    let mut out = Vec::new();
    for slot in 0..HEADER_PAGES.min(count) {
        if inner.read_page(slot, &mut buf).is_ok()
            && let Ok(h) = KeyHeader::decode(&buf)
            && h.page_size + OVERHEAD == phys
        {
            out.push(h);
        }
    }
    if out.is_empty() {
        return Err(StoreError::Corrupt("no valid key header".into()));
    }
    Ok(out)
}

/// Encrypts every page of an inner store. See the module docs for layout.
pub struct EncryptedPageStore<S: PageStore> {
    inner: S,
    suite: AeadSuite,
    db_id: [u8; 16],
    epoch: u32,
    page_size: usize,
    cipher: Cipher,
    commit_key: Zeroizing<[u8; 32]>,
    dek: Dek,
    kcv: [u8; 32],
    /// Per-write version counter (bound into the AAD). Raised to the highest
    /// version seen on read so versions keep increasing across sessions.
    version: AtomicU64,
}

impl<S: PageStore> core::fmt::Debug for EncryptedPageStore<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EncryptedPageStore")
            .field("suite", &self.suite)
            .field("page_size", &self.page_size)
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl<S: PageStore> EncryptedPageStore<S> {
    /// Physical page size the inner store must use for `logical` pages.
    pub const fn physical_page_size(logical: usize) -> usize {
        logical + OVERHEAD
    }

    /// Initialize an empty inner store: fresh DEK, wrapped by `kek`.
    pub fn create(inner: S, kek: &dyn KekProvider, opts: EncryptOptions) -> Result<Self> {
        let dek = generate_dek()?;
        let db_id = match opts.db_id {
            Some(id) => id,
            None => {
                let mut id = [0u8; 16];
                fill_random(&mut id)?;
                id
            }
        };
        let ctx = wrap_context(opts.suite.id(), &db_id, 1, opts.page_size);
        let wrapped = kek.wrap(&dek, &ctx)?;
        Self::init(inner, dek, wrapped, opts, db_id)
    }

    /// Initialize with a DEK the host wrapped itself (async WebCrypto flow):
    /// `dek` from [`generate_dek`], `opts.db_id` set, and `wrapped` produced
    /// over [`EncryptedPageStore::wrap_context_for`]`(&opts)`.
    pub fn create_with_wrapped(
        inner: S,
        dek: Dek,
        wrapped: WrappedDek,
        opts: EncryptOptions,
    ) -> Result<Self> {
        let db_id = opts.db_id.ok_or_else(|| {
            StoreError::InvalidArgument("create_with_wrapped needs opts.db_id".into())
        })?;
        Self::init(inner, dek, wrapped, opts, db_id)
    }

    /// The wrap context a host KEK must authenticate for a store created
    /// with `opts` (which must have `db_id` set).
    pub fn wrap_context_for(opts: &EncryptOptions) -> Result<Vec<u8>> {
        let db_id = opts
            .db_id
            .ok_or_else(|| StoreError::InvalidArgument("opts.db_id must be set".into()))?;
        Ok(wrap_context(opts.suite.id(), &db_id, 1, opts.page_size))
    }

    fn init(
        inner: S,
        dek: Dek,
        wrapped: WrappedDek,
        opts: EncryptOptions,
        db_id: [u8; 16],
    ) -> Result<Self> {
        if opts.page_size < MIN_PAGE_SIZE || opts.page_size > u32::MAX as usize {
            return Err(StoreError::InvalidArgument(format!(
                "page size must be at least {MIN_PAGE_SIZE}"
            )));
        }
        if inner.page_size() != opts.page_size + OVERHEAD {
            return Err(StoreError::InvalidArgument(format!(
                "inner store page size is {}, expected {} (page_size + OVERHEAD)",
                inner.page_size(),
                opts.page_size + OVERHEAD
            )));
        }
        if inner.page_count()? != 0 {
            return Err(StoreError::AlreadyInitialized);
        }
        let epoch = 1;
        let ctx = wrap_context(opts.suite.id(), &db_id, epoch, opts.page_size);
        let header = KeyHeader {
            suite: opts.suite.id(),
            page_size: opts.page_size,
            epoch,
            db_id,
            kcv: kcv_of(&dek, &ctx),
            wrapped,
        };
        let page = header.encode(inner.page_size())?;
        for slot in 0..HEADER_PAGES {
            inner.write_page(slot, &page)?;
        }
        inner.sync()?;
        Self::assemble(inner, header, dek)
    }

    fn assemble(inner: S, h: KeyHeader, dek: Dek) -> Result<Self> {
        let suite = AeadSuite::from_id(h.suite)?;
        let page_key = Zeroizing::new(blake3::derive_key("siafudb-store v1 page key", &dek[..]));
        let commit_key = Zeroizing::new(blake3::derive_key(
            "siafudb-store v1 container commitment",
            &dek[..],
        ));
        Ok(Self {
            cipher: Cipher::new(suite, &page_key),
            inner,
            suite,
            db_id: h.db_id,
            epoch: h.epoch,
            page_size: h.page_size,
            commit_key,
            dek,
            kcv: h.kcv,
            version: AtomicU64::new(0),
        })
    }

    /// Open an initialized store. Fails with `WrongKey` (before any page is
    /// decrypted) if `kek` does not unwrap a DEK matching the key-check value.
    pub fn open(inner: S, kek: &dyn KekProvider) -> Result<Self> {
        let headers = read_headers(&inner)?;
        let mut last = StoreError::WrongKey;
        for h in headers {
            AeadSuite::from_id(h.suite)?;
            let ctx = h.wrap_context();
            match kek.unwrap(&h.wrapped, &ctx) {
                Ok(dek) if ct_eq(&kcv_of(&dek, &ctx), &h.kcv) => {
                    return Self::assemble(inner, h, dek);
                }
                Ok(_) => last = StoreError::WrongKey,
                Err(StoreError::WrongKey) => last = StoreError::WrongKey,
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Re-wrap the DEK under a new KEK (passphrase change). Writes header
    /// copy 0, syncs, then copy 1, syncs: a crash in between leaves one copy
    /// under each KEK, and either opens.
    pub fn rewrap(&self, new_kek: &dyn KekProvider) -> Result<()> {
        let ctx = wrap_context(self.suite.id(), &self.db_id, self.epoch, self.page_size);
        let header = KeyHeader {
            suite: self.suite.id(),
            page_size: self.page_size,
            epoch: self.epoch,
            db_id: self.db_id,
            kcv: self.kcv,
            wrapped: new_kek.wrap(&self.dek, &ctx)?,
        };
        let page = header.encode(self.inner.page_size())?;
        for slot in 0..HEADER_PAGES {
            self.inner.write_page(slot, &page)?;
            self.inner.sync()?;
        }
        Ok(())
    }

    /// Database id.
    pub fn db_id(&self) -> [u8; 16] {
        self.db_id
    }
    /// Suite in use.
    pub fn suite(&self) -> AeadSuite {
        self.suite
    }
    /// Inner store.
    pub fn inner(&self) -> &S {
        &self.inner
    }
    /// Unwrap the inner store (the DEK is zeroized).
    pub fn into_inner(self) -> S {
        self.inner
    }

    fn aad(&self, no: PageNo, version: u64) -> [u8; 42] {
        let mut a = [0u8; 42];
        a[0..4].copy_from_slice(b"SFPG");
        a[4..6].copy_from_slice(&self.suite.id().to_le_bytes());
        a[6..22].copy_from_slice(&self.db_id);
        a[22..30].copy_from_slice(&no.to_le_bytes());
        a[30..34].copy_from_slice(&self.epoch.to_le_bytes());
        a[34..42].copy_from_slice(&version.to_le_bytes());
        a
    }

    fn phys_no(no: PageNo) -> Result<PageNo> {
        no.checked_add(HEADER_PAGES)
            .ok_or(StoreError::OutOfRange(no))
    }
}

impl<S: PageStore> PageStore for EncryptedPageStore<S> {
    fn page_size(&self) -> usize {
        self.page_size
    }
    fn caps(&self) -> StoreCaps {
        self.inner.caps()
    }
    fn page_count(&self) -> Result<u64> {
        Ok(self.inner.page_count()?.saturating_sub(HEADER_PAGES))
    }
    fn read_page(&self, no: PageNo, buf: &mut [u8]) -> Result<()> {
        self.read_page_versioned(no, buf).map(|_| ())
    }
    fn write_page(&self, no: PageNo, buf: &[u8]) -> Result<()> {
        self.write_page_versioned(no, buf).map(|_| ())
    }
    fn observe_version(&self, v: u64) {
        self.version.fetch_max(v, Ordering::Relaxed);
    }
    fn read_page_versioned(&self, no: PageNo, buf: &mut [u8]) -> Result<u64> {
        check_len(self.page_size, buf.len())?;
        let mut phys = vec![0u8; self.page_size + OVERHEAD];
        match self.inner.read_page(Self::phys_no(no)?, &mut phys) {
            Ok(()) => {}
            Err(StoreError::OutOfRange(_)) => return Err(StoreError::OutOfRange(no)),
            Err(e) => return Err(e),
        }
        let suite = u16::from_le_bytes([phys[HDR_SUITE], phys[HDR_SUITE + 1]]);
        let epoch = u32::from_le_bytes(phys[HDR_EPOCH..HDR_VERSION].try_into().expect("4"));
        let version = u64::from_le_bytes(phys[HDR_VERSION..HDR_NONCE].try_into().expect("8"));
        if suite != self.suite.id() || epoch != self.epoch {
            buf.fill(0);
            return Err(StoreError::Integrity(no));
        }
        let nonce: [u8; 24] = phys[HDR_NONCE..HDR_BODY].try_into().expect("24");
        let tag: [u8; 16] = phys[HDR_BODY + self.page_size..].try_into().expect("16");
        let aad = self.aad(no, version);
        let body = &mut phys[HDR_BODY..HDR_BODY + self.page_size];
        if !self.cipher.open(&nonce, &aad, body, &tag) {
            buf.fill(0);
            return Err(StoreError::Integrity(no));
        }
        buf.copy_from_slice(body);
        zeroize::Zeroize::zeroize(&mut phys);
        self.version.fetch_max(version, Ordering::Relaxed);
        Ok(version)
    }
    fn write_page_versioned(&self, no: PageNo, buf: &[u8]) -> Result<u64> {
        check_len(self.page_size, buf.len())?;
        let version = self.version.fetch_add(1, Ordering::Relaxed) + 1;
        let mut phys = vec![0u8; self.page_size + OVERHEAD];
        phys[HDR_SUITE..HDR_EPOCH].copy_from_slice(&self.suite.id().to_le_bytes());
        phys[HDR_EPOCH..HDR_VERSION].copy_from_slice(&self.epoch.to_le_bytes());
        phys[HDR_VERSION..HDR_NONCE].copy_from_slice(&version.to_le_bytes());
        let mut nonce = [0u8; 24];
        fill_random(&mut nonce)?;
        phys[HDR_NONCE..HDR_BODY].copy_from_slice(&nonce);
        let aad = self.aad(no, version);
        let body = &mut phys[HDR_BODY..HDR_BODY + self.page_size];
        body.copy_from_slice(buf);
        let tag = self.cipher.seal(&nonce, &aad, body)?;
        phys[HDR_BODY + self.page_size..].copy_from_slice(&tag);
        self.inner.write_page(Self::phys_no(no)?, &phys)?;
        Ok(version)
    }
    fn truncate(&self, pages: u64) -> Result<()> {
        self.inner
            .truncate(pages.checked_add(HEADER_PAGES).ok_or(StoreError::Quota)?)
    }
    fn sync(&self) -> Result<()> {
        self.inner.sync()
    }
    fn lock(&self, mode: LockMode) -> Result<LockGuard> {
        self.inner.lock(mode)
    }
    fn commitment_key(&self) -> Option<Zeroizing<[u8; 32]>> {
        Some(self.commit_key.clone())
    }
}
