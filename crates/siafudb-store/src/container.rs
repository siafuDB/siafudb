//! Crash-safe container: double-buffered superblock, copy-on-write named
//! blobs (engine snapshots) and an append-only log (signed mutations).
//!
//! ## Layout (logical pages of any [`PageStore`])
//!
//! ```text
//! page 0, 1   superblock slots; commit `seq` lives in slot `seq % 2`
//! page 2..    extents: the directory, blob bodies, log segments; free pages
//! ```
//!
//! The superblock holds the monotonic commit counter `seq`, the directory's
//! extent and its keyed BLAKE3 hash, and a keyed BLAKE3 MAC over itself. The
//! directory lists every blob and log segment with its extent, length and
//! keyed BLAKE3 hash. Together these form a hash tree whose root is in the
//! superblock: every live page is committed to by content, so substituting
//! any page (an older authentic copy, a page from another slot, a torn
//! write) is detected when it is read, and [`Container::open`] reads and
//! checks everything by default. The key is [`PageStore::commitment_key`]
//! (a subkey of the DEK for encrypted stores; a fixed public key otherwise,
//! which still detects corruption but not deliberate forgery).
//!
//! ## Commit protocol (shadow paging)
//!
//! 1. Write new blob bodies, the new log segment and the new directory into
//!    pages that are free under the *current* commit (never in place).
//! 2. `sync`.
//! 3. Write superblock `seq + 1` into slot `(seq + 1) % 2`; `sync`.
//! 4. Trim trailing free pages (best effort).
//!
//! A crash before step 3 lands leaves the current superblock and every page
//! it references untouched. A torn step 3 fails its MAC (or AEAD) and open
//! falls back to the other slot, which is the previous commit. Once a
//! superblock authenticates, everything it references was synced before it
//! was written, so a mismatch below a valid superblock is reported as
//! tampering/rollback, never papered over by falling back.
//!
//! ## Rollback
//!
//! Every page write gets a version from the store
//! ([`PageStore::write_page_versioned`]; for [`crate::EncryptedPageStore`]
//! the per-write version bound into the page's AAD). The directory lists the
//! version of every blob and log page, and the superblock carries a keyed
//! BLAKE3 root over the whole page-version vector (directory pages included)
//! plus the highest version. At open the vector is recomputed from the
//! authenticated directory and checked against the root; every later read
//! checks the page's authenticated version against the vector. This catches
//! a restored older copy even when its plaintext equals the current one,
//! which the content hashes alone cannot.
//!
//! Mixing pages from different points in time is detected (hash tree). A
//! rollback of the *whole* store to an earlier consistent state (restore an
//! old backup, replay both superblock slots) cannot be detected from the
//! store alone; pass [`OpenOptions::min_seq`] from a monotonic source outside
//! the store (keychain, server, another device) to catch it.

use std::collections::BTreeMap;

use zeroize::Zeroizing;

use crate::Result;
use crate::codec::{Dec, Enc};
use crate::crypto::ct_eq;
use crate::store::{LockGuard, LockMode, PageNo, PageStore, StoreError};

const SB_MAGIC: &[u8; 8] = b"SIAFUCTR";
const SB_FORMAT: u16 = 1;
const DIR_FORMAT: u32 = 1;
const MAC_LEN: usize = 32;
const FIRST_DATA_PAGE: u64 = 2;
const MIN_PAGE_SIZE: usize = 256;
/// Max blob name length in bytes.
const MAX_NAME: usize = 1024;
/// Log segments beyond this are compacted into one at commit.
const MAX_SEGMENTS: usize = 32;

const D_SB: &[u8] = b"siafudb-store/sb/v1";
const D_DIR: &[u8] = b"siafudb-store/dir/v1";
const D_BLOB: &[u8] = b"siafudb-store/blob/v1";
const D_SEG: &[u8] = b"siafudb-store/seg/v1";
const D_VER: &[u8] = b"siafudb-store/versions/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Extent {
    start: PageNo,
    pages: u64,
}

impl Extent {
    fn end(&self) -> u64 {
        self.start + self.pages
    }
}

#[derive(Clone, Debug)]
struct BlobRef {
    ext: Extent,
    len: u64,
    hash: [u8; 32],
    /// Per-page write versions (`ext.pages` entries).
    versions: Vec<u64>,
}

#[derive(Clone, Debug)]
struct Segment {
    first_seq: u64,
    count: u32,
    ext: Extent,
    len: u64,
    hash: [u8; 32],
    /// Per-page write versions (`ext.pages` entries).
    versions: Vec<u64>,
}

impl Segment {
    fn end_seq(&self) -> u64 {
        self.first_seq + u64::from(self.count)
    }
}

#[derive(Clone, Debug, Default)]
struct Dir {
    blobs: BTreeMap<String, BlobRef>,
    /// First retained log seq.
    log_first: u64,
    /// Seq the next appended entry gets.
    log_next: u64,
    segments: Vec<Segment>,
}

struct Superblock {
    seq: u64,
    dir: Extent,
    dir_len: u64,
    dir_hash: [u8; 32],
    /// Highest page version referenced by this commit.
    max_version: u64,
    /// Keyed BLAKE3 over the page-version vector: `(page_no, version)` for
    /// every live page (directory, then blobs by name, then log segments).
    ver_root: [u8; 32],
}

/// Options for [`Container::open_with`].
#[derive(Clone, Copy, Debug)]
pub struct OpenOptions {
    /// Read and check every blob and log segment at open (default true).
    /// When false, content is still checked on every read.
    pub verify_all: bool,
    /// Reject the store as rolled back if its commit counter is below this
    /// (value from a monotonic source outside the store).
    pub min_seq: Option<u64>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            verify_all: true,
            min_seq: None,
        }
    }
}

/// What [`Container::open`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenReport {
    /// Commit counter of the checkpoint that was opened.
    pub seq: u64,
    /// One superblock slot did not authenticate. After a crash this is a
    /// torn commit and the previous checkpoint was opened; it can also be
    /// damage or tampering, which is indistinguishable. Surface it.
    pub damaged_superblock: bool,
}

/// One log entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Sequence number (dense, starting at 0).
    pub seq: u64,
    /// Entry bytes.
    pub data: Vec<u8>,
}

/// Crash-safe container. Mutations are staged in memory and become durable
/// atomically at [`Container::commit`]; reads see staged changes.
pub struct Container<S: PageStore> {
    store: S,
    ps: usize,
    key: Zeroizing<[u8; 32]>,
    dir: Dir,
    dir_ext: Extent,
    seq: u64,
    staged_blobs: BTreeMap<String, Option<Vec<u8>>>,
    staged_log: Vec<Vec<u8>>,
    staged_trim: u64,
    poisoned: bool,
    report: OpenReport,
    _lock: LockGuard,
}

impl<S: PageStore> core::fmt::Debug for Container<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Container")
            .field("seq", &self.seq)
            .field("page_size", &self.ps)
            .field("blobs", &self.dir.blobs.len())
            .field("log", &(self.dir.log_first, self.dir.log_next))
            .field("dirty", &self.is_dirty())
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

fn pages_for(len: u64, ps: usize) -> u64 {
    len.div_ceil(ps as u64)
}

fn content_hash(key: &[u8; 32], domain: &[u8], name: &[u8], len: u64, pages: &[u8]) -> [u8; 32] {
    let mut h = blake3::Hasher::new_keyed(key);
    h.update(domain);
    h.update(&(name.len() as u32).to_le_bytes());
    h.update(name);
    h.update(&len.to_le_bytes());
    h.update(pages);
    *h.finalize().as_bytes()
}

fn public_key() -> Zeroizing<[u8; 32]> {
    Zeroizing::new(blake3::derive_key(
        "siafudb-store v1 public commitment",
        b"",
    ))
}

fn decode_versions(d: &mut Dec<'_>, pages: u64) -> Result<Vec<u64>> {
    // Bound by the bytes actually present before allocating.
    if pages
        .checked_mul(8)
        .is_none_or(|n| n > d.remaining() as u64)
    {
        return Err(StoreError::Corrupt(
            "directory: version list truncated".into(),
        ));
    }
    (0..pages).map(|_| d.u64()).collect()
}

/// Keyed BLAKE3 over the page-version vector of a commit.
fn version_root(key: &[u8; 32], dir_ext: Extent, dir_versions: &[u64], dir: &Dir) -> [u8; 32] {
    let mut h = blake3::Hasher::new_keyed(key);
    h.update(D_VER);
    let mut add = |ext: Extent, vs: &[u64]| {
        for (i, v) in vs.iter().enumerate() {
            h.update(&(ext.start + i as u64).to_le_bytes());
            h.update(&v.to_le_bytes());
        }
    };
    add(dir_ext, dir_versions);
    for (ext, vs) in dir.versioned() {
        add(ext, vs);
    }
    *h.finalize().as_bytes()
}

impl Dir {
    fn encode(&self) -> Vec<u8> {
        let mut e = Enc::default();
        e.u32(DIR_FORMAT).u64(self.log_first).u64(self.log_next);
        e.u32(self.blobs.len() as u32);
        for (name, b) in &self.blobs {
            e.u16(name.len() as u16)
                .raw(name.as_bytes())
                .u64(b.ext.start)
                .u64(b.ext.pages)
                .u64(b.len)
                .raw(&b.hash);
            for v in &b.versions {
                e.u64(*v);
            }
        }
        e.u32(self.segments.len() as u32);
        for s in &self.segments {
            e.u64(s.first_seq)
                .u32(s.count)
                .u64(s.ext.start)
                .u64(s.ext.pages)
                .u64(s.len)
                .raw(&s.hash);
            for v in &s.versions {
                e.u64(*v);
            }
        }
        e.0
    }

    fn decode(buf: &[u8]) -> Result<Self> {
        let mut d = Dec::new(buf, "directory");
        if d.u32()? != DIR_FORMAT {
            return Err(StoreError::Corrupt("unknown directory format".into()));
        }
        let mut dir = Dir {
            log_first: d.u64()?,
            log_next: d.u64()?,
            ..Dir::default()
        };
        for _ in 0..d.u32()? {
            let n = d.u16()? as usize;
            let name = String::from_utf8(d.take(n)?.to_vec())
                .map_err(|_| StoreError::Corrupt("blob name is not UTF-8".into()))?;
            let ext = Extent {
                start: d.u64()?,
                pages: d.u64()?,
            };
            let b = BlobRef {
                ext,
                len: d.u64()?,
                hash: d.arr()?,
                versions: decode_versions(&mut d, ext.pages)?,
            };
            if dir.blobs.insert(name, b).is_some() {
                return Err(StoreError::Corrupt("duplicate blob name".into()));
            }
        }
        for _ in 0..d.u32()? {
            let first_seq = d.u64()?;
            let count = d.u32()?;
            let ext = Extent {
                start: d.u64()?,
                pages: d.u64()?,
            };
            dir.segments.push(Segment {
                first_seq,
                count,
                ext,
                len: d.u64()?,
                hash: d.arr()?,
                versions: decode_versions(&mut d, ext.pages)?,
            });
        }
        if d.remaining() != 0 {
            return Err(StoreError::Corrupt("trailing bytes in directory".into()));
        }
        Ok(dir)
    }

    /// `(extent, versions)` of every blob and segment, in canonical order.
    fn versioned(&self) -> impl Iterator<Item = (Extent, &[u64])> + '_ {
        self.blobs
            .values()
            .map(|b| (b.ext, b.versions.as_slice()))
            .chain(self.segments.iter().map(|s| (s.ext, s.versions.as_slice())))
    }

    fn max_version(&self) -> u64 {
        self.versioned()
            .flat_map(|(_, v)| v.iter().copied())
            .max()
            .unwrap_or(0)
    }

    fn extents(&self) -> impl Iterator<Item = Extent> + '_ {
        self.blobs
            .values()
            .map(|b| b.ext)
            .chain(self.segments.iter().map(|s| s.ext))
            .filter(|e| e.pages > 0)
    }

    /// Structural checks so a malformed (but authentic-looking) directory
    /// cannot make the allocator hand out live pages.
    fn validate(&self, dir_ext: Extent, ps: usize, page_count: u64) -> Result<()> {
        let bad = |m: &str| Err(StoreError::Corrupt(format!("directory: {m}")));
        let check = |e: Extent, len: u64| -> Result<()> {
            if e.pages != pages_for(len, ps) {
                return bad("extent size does not match length");
            }
            if e.pages > 0
                && (e.start < FIRST_DATA_PAGE
                    || e.start
                        .checked_add(e.pages)
                        .is_none_or(|end| end > page_count))
            {
                return bad("extent out of range");
            }
            Ok(())
        };
        for (ext, vs) in self.versioned() {
            if vs.len() as u64 != ext.pages {
                return bad("version list length");
            }
        }
        for b in self.blobs.values() {
            check(b.ext, b.len)?;
        }
        for s in &self.segments {
            check(s.ext, s.len)?;
            if s.count == 0 {
                return bad("empty segment");
            }
        }
        let mut all: Vec<Extent> = self.extents().chain(core::iter::once(dir_ext)).collect();
        all.sort_by_key(|e| e.start);
        if all.windows(2).any(|w| w[0].end() > w[1].start) {
            return bad("overlapping extents");
        }
        if self.log_first > self.log_next {
            return bad("log bounds");
        }
        match (self.segments.first(), self.segments.last()) {
            (Some(f), Some(l)) => {
                if f.first_seq > self.log_first || l.end_seq() != self.log_next {
                    return bad("log segments do not cover the log");
                }
                if self
                    .segments
                    .windows(2)
                    .any(|w| w[0].end_seq() != w[1].first_seq)
                {
                    return bad("log segments not contiguous");
                }
            }
            _ => {
                if self.log_first != self.log_next {
                    return bad("log has entries but no segments");
                }
            }
        }
        Ok(())
    }
}

/// First-fit allocator over pages free under the committed directory.
struct Allocator {
    free: Vec<Extent>,
    end: u64,
}

impl Allocator {
    fn new(mut used: Vec<Extent>, page_count: u64) -> Self {
        used.sort_by_key(|e| e.start);
        let mut free = Vec::new();
        let mut cur = FIRST_DATA_PAGE;
        for e in &used {
            if e.start > cur {
                free.push(Extent {
                    start: cur,
                    pages: e.start - cur,
                });
            }
            cur = cur.max(e.end());
        }
        let end = cur.max(page_count).max(FIRST_DATA_PAGE);
        if end > cur {
            free.push(Extent {
                start: cur,
                pages: end - cur,
            });
        }
        Self { free, end }
    }

    fn alloc(&mut self, pages: u64) -> Extent {
        if pages == 0 {
            return Extent::default();
        }
        if let Some(i) = self.free.iter().position(|f| f.pages >= pages) {
            let f = &mut self.free[i];
            let e = Extent {
                start: f.start,
                pages,
            };
            f.start += pages;
            f.pages -= pages;
            if f.pages == 0 {
                self.free.remove(i);
            }
            return e;
        }
        let e = Extent {
            start: self.end,
            pages,
        };
        self.end += pages;
        e
    }
}

fn encode_entries<'a>(entries: impl Iterator<Item = &'a [u8]>) -> (Vec<u8>, u32) {
    let mut e = Enc::default();
    let mut n = 0u32;
    for x in entries {
        e.u32(x.len() as u32).raw(x);
        n += 1;
    }
    (e.0, n)
}

fn decode_entries(buf: &[u8], count: u32) -> Result<Vec<Vec<u8>>> {
    let mut d = Dec::new(buf, "log segment");
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let n = d.u32()? as usize;
        out.push(d.take(n)?.to_vec());
    }
    if d.remaining() != 0 {
        return Err(StoreError::Corrupt("trailing bytes in log segment".into()));
    }
    Ok(out)
}

impl<S: PageStore> Container<S> {
    /// Initialize an empty store (`page_count() == 0`).
    pub fn create(store: S) -> Result<Self> {
        let lock = store.lock(LockMode::Exclusive)?;
        let ps = Self::check_ps(&store)?;
        if store.page_count()? != 0 {
            return Err(StoreError::AlreadyInitialized);
        }
        let key = store.commitment_key().unwrap_or_else(public_key);
        let mut c = Self {
            store,
            ps,
            key,
            dir: Dir::default(),
            dir_ext: Extent::default(),
            seq: 1,
            staged_blobs: BTreeMap::new(),
            staged_log: Vec::new(),
            staged_trim: 0,
            poisoned: false,
            report: OpenReport {
                seq: 1,
                damaged_superblock: false,
            },
            _lock: lock,
        };
        let bytes = c.dir.encode();
        let ext = Extent {
            start: FIRST_DATA_PAGE,
            pages: pages_for(bytes.len() as u64, ps),
        };
        let (hash, versions) = c.write_extent(ext, D_DIR, b"", &bytes)?;
        c.store.sync()?;
        let ver_root = version_root(&c.key, ext, &versions, &c.dir);
        for seq in 0..2 {
            let sb = Superblock {
                seq,
                dir: ext,
                dir_len: bytes.len() as u64,
                dir_hash: hash,
                max_version: versions.iter().copied().max().unwrap_or(0),
                ver_root,
            };
            c.store.write_page(seq % 2, &c.encode_sb(&sb))?;
        }
        c.store.sync()?;
        c.dir_ext = ext;
        Ok(c)
    }

    /// Open with default options (full verification).
    pub fn open(store: S) -> Result<Self> {
        Self::open_with(store, OpenOptions::default())
    }

    /// Open an existing store, or create one if it is empty.
    pub fn open_or_create(store: S) -> Result<Self> {
        if store.page_count()? == 0 {
            Self::create(store)
        } else {
            Self::open(store)
        }
    }

    /// Open the newest checkpoint whose superblock authenticates.
    pub fn open_with(store: S, opts: OpenOptions) -> Result<Self> {
        let lock = store.lock(LockMode::Exclusive)?;
        let ps = Self::check_ps(&store)?;
        let count = store.page_count()?;
        if count == 0 {
            return Err(StoreError::NotInitialized);
        }
        let key = store.commitment_key().unwrap_or_else(public_key);
        let mut best: Option<Superblock> = None;
        let mut damaged = false;
        let mut page = vec![0u8; ps];
        for slot in 0..2u64 {
            let sb = store
                .read_page(slot, &mut page)
                .ok()
                .and_then(|()| Self::decode_sb(&key, ps, &page).ok())
                .filter(|sb| sb.seq % 2 == slot);
            match sb {
                Some(sb) if best.as_ref().is_none_or(|b| sb.seq > b.seq) => best = Some(sb),
                Some(_) => {}
                None => damaged = true,
            }
        }
        let sb = best.ok_or_else(|| StoreError::Corrupt("no valid superblock".into()))?;
        if let Some(min) = opts.min_seq
            && sb.seq < min
        {
            return Err(StoreError::Rollback(format!(
                "store is at commit {}, expected at least {min}",
                sb.seq
            )));
        }
        let mut c = Self {
            store,
            ps,
            key,
            dir: Dir::default(),
            dir_ext: sb.dir,
            seq: sb.seq,
            staged_blobs: BTreeMap::new(),
            staged_log: Vec::new(),
            staged_trim: 0,
            poisoned: false,
            report: OpenReport {
                seq: sb.seq,
                damaged_superblock: damaged,
            },
            _lock: lock,
        };
        if sb.dir.pages != pages_for(sb.dir_len, ps)
            || sb.dir.start < FIRST_DATA_PAGE
            || sb.dir.end() > count
        {
            return Err(StoreError::Corrupt("superblock directory extent".into()));
        }
        let (bytes, dir_versions) = c.read_extent(
            sb.dir,
            sb.dir_len,
            D_DIR,
            b"",
            &sb.dir_hash,
            None,
            "directory",
        )?;
        let dir = Dir::decode(&bytes)?;
        dir.validate(sb.dir, ps, count)?;
        // Rollback check on the page-version vector: the directory's own
        // page versions (just read, authenticated by the store) plus the
        // versions it lists must hash to the root in the superblock.
        if !ct_eq(
            &version_root(&c.key, sb.dir, &dir_versions, &dir),
            &sb.ver_root,
        ) || dir.max_version() > sb.max_version
            || dir_versions.iter().any(|v| *v > sb.max_version)
        {
            return Err(StoreError::Rollback(
                "page-version vector does not match the superblock (older page restored)".into(),
            ));
        }
        c.store.observe_version(sb.max_version);
        c.dir = dir;
        if opts.verify_all {
            c.verify()?;
        }
        Ok(c)
    }

    fn check_ps(store: &S) -> Result<usize> {
        let ps = store.page_size();
        if ps < MIN_PAGE_SIZE {
            return Err(StoreError::InvalidArgument(format!(
                "container needs pages of at least {MIN_PAGE_SIZE} bytes"
            )));
        }
        Ok(ps)
    }

    fn encode_sb(&self, sb: &Superblock) -> Vec<u8> {
        let mut e = Enc::default();
        e.raw(SB_MAGIC)
            .u16(SB_FORMAT)
            .u16(0)
            .u32(self.ps as u32)
            .u64(sb.seq)
            .u64(sb.dir.start)
            .u64(sb.dir.pages)
            .u64(sb.dir_len)
            .raw(&sb.dir_hash)
            .u64(sb.max_version)
            .raw(&sb.ver_root);
        let mut page = e.0;
        page.resize(self.ps - MAC_LEN, 0);
        let mac = content_hash(&self.key, D_SB, b"", 0, &page);
        page.extend_from_slice(&mac);
        page
    }

    fn decode_sb(key: &[u8; 32], ps: usize, page: &[u8]) -> Result<Superblock> {
        let (body, mac) = page.split_at(ps - MAC_LEN);
        if !ct_eq(&content_hash(key, D_SB, b"", 0, body), mac) {
            return Err(StoreError::Corrupt("superblock MAC".into()));
        }
        let mut d = Dec::new(body, "superblock");
        if &d.arr::<8>()? != SB_MAGIC || d.u16()? != SB_FORMAT {
            return Err(StoreError::Corrupt("superblock magic".into()));
        }
        let _flags = d.u16()?;
        if d.u32()? as usize != ps {
            return Err(StoreError::Corrupt("superblock page size".into()));
        }
        Ok(Superblock {
            seq: d.u64()?,
            dir: Extent {
                start: d.u64()?,
                pages: d.u64()?,
            },
            dir_len: d.u64()?,
            dir_hash: d.arr()?,
            max_version: d.u64()?,
            ver_root: d.arr()?,
        })
    }

    /// Write `bytes` into `ext`; returns the content hash and the version of
    /// every page written.
    fn write_extent(
        &self,
        ext: Extent,
        domain: &[u8],
        name: &[u8],
        bytes: &[u8],
    ) -> Result<([u8; 32], Vec<u64>)> {
        let mut h = blake3::Hasher::new_keyed(&self.key);
        h.update(domain);
        h.update(&(name.len() as u32).to_le_bytes());
        h.update(name);
        h.update(&(bytes.len() as u64).to_le_bytes());
        let mut page = vec![0u8; self.ps];
        let mut versions = Vec::with_capacity(ext.pages as usize);
        for i in 0..ext.pages {
            let off = (i as usize) * self.ps;
            let n = self.ps.min(bytes.len() - off);
            page[..n].copy_from_slice(&bytes[off..off + n]);
            page[n..].fill(0);
            h.update(&page);
            versions.push(self.store.write_page_versioned(ext.start + i, &page)?);
        }
        Ok((*h.finalize().as_bytes(), versions))
    }

    #[allow(clippy::too_many_arguments)]
    fn read_extent(
        &self,
        ext: Extent,
        len: u64,
        domain: &[u8],
        name: &[u8],
        expected: &[u8; 32],
        expected_versions: Option<&[u64]>,
        what: &str,
    ) -> Result<(Vec<u8>, Vec<u64>)> {
        let ps = self.ps;
        let mut out =
            vec![
                0u8;
                usize::try_from(ext.pages).map_err(|_| StoreError::Corrupt("extent".into()))? * ps
            ];
        let mut versions = Vec::with_capacity(ext.pages as usize);
        for i in 0..ext.pages {
            let off = (i as usize) * ps;
            let v = self
                .store
                .read_page_versioned(ext.start + i, &mut out[off..off + ps])?;
            if let Some(want) = expected_versions
                && want.get(i as usize) != Some(&v)
            {
                return Err(StoreError::Rollback(format!(
                    "{what} page {} has version {v}, the committed page-version vector says {:?}",
                    ext.start + i,
                    want.get(i as usize)
                )));
            }
            versions.push(v);
        }
        if !ct_eq(&content_hash(&self.key, domain, name, len, &out), expected) {
            return Err(StoreError::Rollback(format!(
                "{what} does not match the committed root (older or foreign page substituted)"
            )));
        }
        out.truncate(len as usize);
        Ok((out, versions))
    }

    fn check(&self) -> Result<()> {
        if self.poisoned {
            Err(StoreError::Poisoned)
        } else {
            Ok(())
        }
    }

    /// Read and check every blob and log segment against the committed root.
    pub fn verify(&self) -> Result<()> {
        self.check()?;
        for (name, b) in &self.dir.blobs {
            self.read_extent(
                b.ext,
                b.len,
                D_BLOB,
                name.as_bytes(),
                &b.hash,
                Some(&b.versions),
                "blob",
            )?;
        }
        for s in &self.dir.segments {
            let (bytes, _) = self.read_extent(
                s.ext,
                s.len,
                D_SEG,
                b"",
                &s.hash,
                Some(&s.versions),
                "log segment",
            )?;
            decode_entries(&bytes, s.count)?;
        }
        Ok(())
    }

    /// Stage `bytes` under `name` (replaces any previous blob).
    pub fn put_blob(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.check()?;
        if name.len() > MAX_NAME {
            return Err(StoreError::InvalidArgument("blob name too long".into()));
        }
        self.staged_blobs
            .insert(name.to_owned(), Some(bytes.to_vec()));
        Ok(())
    }

    /// Stage removal of `name`.
    pub fn delete_blob(&mut self, name: &str) -> Result<()> {
        self.check()?;
        self.staged_blobs.insert(name.to_owned(), None);
        Ok(())
    }

    /// Blob contents (staged or committed), checked against the root.
    pub fn get_blob(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        if let Some(staged) = self.staged_blobs.get(name) {
            return Ok(staged.clone());
        }
        match self.dir.blobs.get(name) {
            None => Ok(None),
            Some(b) => self
                .read_extent(
                    b.ext,
                    b.len,
                    D_BLOB,
                    name.as_bytes(),
                    &b.hash,
                    Some(&b.versions),
                    "blob",
                )
                .map(|(bytes, _)| Some(bytes)),
        }
    }

    /// Names of all blobs (staged view), sorted.
    pub fn blob_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .dir
            .blobs
            .keys()
            .filter(|n| !matches!(self.staged_blobs.get(*n), Some(None)))
            .cloned()
            .collect();
        for (n, v) in &self.staged_blobs {
            if v.is_some() && !self.dir.blobs.contains_key(n) {
                names.push(n.clone());
            }
        }
        names.sort();
        names
    }

    /// Stage a log entry; returns its sequence number.
    pub fn append_log(&mut self, entry: &[u8]) -> Result<u64> {
        self.check()?;
        if entry.len() > u32::MAX as usize {
            return Err(StoreError::InvalidArgument("log entry too large".into()));
        }
        let seq = self.dir.log_next + self.staged_log.len() as u64;
        self.staged_log.push(entry.to_vec());
        Ok(seq)
    }

    /// Stage dropping every log entry with `seq < before` (after a snapshot
    /// covers them).
    pub fn truncate_log(&mut self, before: u64) -> Result<()> {
        self.check()?;
        self.staged_trim = self.staged_trim.max(before);
        Ok(())
    }

    /// `(first retained seq, next seq)` of the staged view.
    pub fn log_bounds(&self) -> (u64, u64) {
        let next = self.dir.log_next + self.staged_log.len() as u64;
        (self.dir.log_first.max(self.staged_trim).min(next), next)
    }

    /// Entries with `seq >= from_seq` (staged view), checked against the root.
    pub fn read_log(&self, from_seq: u64) -> Result<Vec<LogEntry>> {
        self.check()?;
        let start = from_seq.max(self.log_bounds().0);
        let mut out = Vec::new();
        for s in &self.dir.segments {
            if s.end_seq() <= start {
                continue;
            }
            let (bytes, _) = self.read_extent(
                s.ext,
                s.len,
                D_SEG,
                b"",
                &s.hash,
                Some(&s.versions),
                "log segment",
            )?;
            for (i, data) in decode_entries(&bytes, s.count)?.into_iter().enumerate() {
                let seq = s.first_seq + i as u64;
                if seq >= start {
                    out.push(LogEntry { seq, data });
                }
            }
        }
        for (i, data) in self.staged_log.iter().enumerate() {
            let seq = self.dir.log_next + i as u64;
            if seq >= start {
                out.push(LogEntry {
                    seq,
                    data: data.clone(),
                });
            }
        }
        Ok(out)
    }

    /// Anything staged?
    pub fn is_dirty(&self) -> bool {
        !self.staged_blobs.is_empty()
            || !self.staged_log.is_empty()
            || self.staged_trim > self.dir.log_first
    }

    /// Drop staged changes.
    pub fn discard(&mut self) {
        self.staged_blobs.clear();
        self.staged_log.clear();
        self.staged_trim = 0;
    }

    /// Store a snapshot and drop the log entries it covers, atomically.
    pub fn checkpoint(&mut self, name: &str, snapshot: &[u8], log_before: u64) -> Result<u64> {
        self.put_blob(name, snapshot)?;
        self.truncate_log(log_before)?;
        self.commit()
    }

    /// Commit counter of the current durable checkpoint.
    pub fn commit_seq(&self) -> u64 {
        self.seq
    }

    /// What open found.
    pub fn open_report(&self) -> OpenReport {
        self.report
    }

    /// The underlying store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Release the lock and return the store (staged changes are dropped).
    pub fn into_store(self) -> S {
        self.store
    }

    /// Make staged changes durable atomically; returns the new commit seq.
    /// On error the container is poisoned: drop it and reopen the store,
    /// which yields either the previous or the new checkpoint.
    pub fn commit(&mut self) -> Result<u64> {
        self.check()?;
        if !self.is_dirty() {
            return Ok(self.seq);
        }
        match self.commit_inner() {
            Ok(()) => Ok(self.seq),
            Err(e) => {
                self.poisoned = true;
                Err(e)
            }
        }
    }

    fn commit_inner(&mut self) -> Result<()> {
        let ps = self.ps;
        let count = self.store.page_count()?;
        let used: Vec<Extent> = self
            .dir
            .extents()
            .chain(core::iter::once(self.dir_ext))
            .collect();
        let mut alloc = Allocator::new(used, count);
        let mut new = self.dir.clone();

        for (name, v) in &self.staged_blobs {
            match v {
                None => {
                    new.blobs.remove(name);
                }
                Some(bytes) => {
                    let len = bytes.len() as u64;
                    let ext = alloc.alloc(pages_for(len, ps));
                    let (hash, versions) =
                        self.write_extent(ext, D_BLOB, name.as_bytes(), bytes)?;
                    new.blobs.insert(
                        name.clone(),
                        BlobRef {
                            ext,
                            len,
                            hash,
                            versions,
                        },
                    );
                }
            }
        }

        // Log: trim, append the staged entries as one segment, compact if long.
        let next = self.dir.log_next + self.staged_log.len() as u64;
        let first = self.dir.log_first.max(self.staged_trim).min(next);
        new.segments.retain(|s| s.end_seq() > first);
        let staged_from = self.dir.log_next.max(first);
        let fresh: Vec<&[u8]> = self
            .staged_log
            .iter()
            .enumerate()
            .filter(|(i, _)| self.dir.log_next + *i as u64 >= first)
            .map(|(_, e)| e.as_slice())
            .collect();
        let mut seg_entries: Option<(u64, Vec<Vec<u8>>)> = None;
        if new.segments.len() + usize::from(!fresh.is_empty()) > MAX_SEGMENTS {
            let mut all = Vec::new();
            let seg_first = new.segments[0].first_seq.max(first);
            for s in &new.segments {
                let (bytes, _) = self.read_extent(
                    s.ext,
                    s.len,
                    D_SEG,
                    b"",
                    &s.hash,
                    Some(&s.versions),
                    "log segment",
                )?;
                for (i, d) in decode_entries(&bytes, s.count)?.into_iter().enumerate() {
                    if s.first_seq + i as u64 >= first {
                        all.push(d);
                    }
                }
            }
            all.extend(fresh.iter().map(|e| e.to_vec()));
            new.segments.clear();
            seg_entries = Some((seg_first, all));
        } else if !fresh.is_empty() {
            seg_entries = Some((staged_from, fresh.iter().map(|e| e.to_vec()).collect()));
        }
        if let Some((first_seq, entries)) = seg_entries
            && !entries.is_empty()
        {
            let (bytes, n) = encode_entries(entries.iter().map(|e| e.as_slice()));
            let len = bytes.len() as u64;
            let ext = alloc.alloc(pages_for(len, ps));
            let (hash, versions) = self.write_extent(ext, D_SEG, b"", &bytes)?;
            new.segments.push(Segment {
                first_seq,
                count: n,
                ext,
                len,
                hash,
                versions,
            });
        }
        new.log_first = first;
        new.log_next = next;

        let bytes = new.encode();
        let dir_ext = alloc.alloc(pages_for(bytes.len() as u64, ps));
        let (dir_hash, dir_versions) = self.write_extent(dir_ext, D_DIR, b"", &bytes)?;
        self.store.sync()?;

        let seq = self.seq + 1;
        let sb = Superblock {
            seq,
            dir: dir_ext,
            dir_len: bytes.len() as u64,
            dir_hash,
            max_version: new
                .max_version()
                .max(dir_versions.iter().copied().max().unwrap_or(0)),
            ver_root: version_root(&self.key, dir_ext, &dir_versions, &new),
        };
        self.store.write_page(seq % 2, &self.encode_sb(&sb))?;
        self.store.sync()?;

        // Durable. Adopt the new state.
        self.dir = new;
        self.dir_ext = dir_ext;
        self.seq = seq;
        self.staged_blobs.clear();
        self.staged_log.clear();
        self.staged_trim = 0;

        // Best effort: give back trailing free pages. Only pages free under
        // the new (durable) commit are dropped.
        let hi = self
            .dir
            .extents()
            .chain(core::iter::once(self.dir_ext))
            .map(|e| e.end())
            .max()
            .unwrap_or(FIRST_DATA_PAGE)
            .max(FIRST_DATA_PAGE);
        if let Ok(n) = self.store.page_count()
            && hi < n
        {
            let _ = self.store.truncate(hi);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::MemPageStore;

    fn sample_dir() -> Dir {
        let mut d = Dir {
            log_first: 1,
            log_next: 4,
            ..Dir::default()
        };
        d.blobs.insert(
            "snap".into(),
            BlobRef {
                ext: Extent { start: 3, pages: 2 },
                len: 300,
                hash: [1; 32],
                versions: vec![7, 8],
            },
        );
        d.segments.push(Segment {
            first_seq: 1,
            count: 3,
            ext: Extent { start: 5, pages: 1 },
            len: 20,
            hash: [2; 32],
            versions: vec![9],
        });
        d
    }

    #[test]
    fn dir_round_trip_and_validation() {
        let d = sample_dir();
        let back = Dir::decode(&d.encode()).unwrap();
        assert_eq!(back.encode(), d.encode());
        assert_eq!(back.max_version(), 9);
        back.validate(Extent { start: 2, pages: 1 }, 256, 6)
            .unwrap();
        // Overlap with the directory extent.
        assert!(
            back.validate(Extent { start: 4, pages: 1 }, 256, 6)
                .is_err()
        );
        // Out of range.
        assert!(
            back.validate(Extent { start: 2, pages: 1 }, 256, 5)
                .is_err()
        );
        // Version list length must match the extent.
        let mut bad = sample_dir();
        bad.blobs.get_mut("snap").unwrap().versions.pop();
        assert!(bad.validate(Extent { start: 2, pages: 1 }, 256, 6).is_err());
        // Truncated encodings are rejected, never panic.
        let enc = d.encode();
        for n in 0..enc.len() {
            assert!(Dir::decode(&enc[..n]).is_err(), "prefix {n}");
        }
    }

    #[test]
    fn huge_version_count_does_not_allocate() {
        let mut e = Enc::default();
        e.u32(DIR_FORMAT)
            .u64(0)
            .u64(0)
            .u32(1)
            .u16(1)
            .raw(b"x")
            .u64(2)
            .u64(u64::MAX / 2)
            .u64(1)
            .raw(&[0; 32]);
        assert!(matches!(Dir::decode(&e.0), Err(StoreError::Corrupt(_))));
    }

    #[test]
    fn allocator_reuses_holes_then_extends() {
        let used = vec![Extent { start: 2, pages: 1 }, Extent { start: 5, pages: 2 }];
        let mut a = Allocator::new(used, 7);
        assert_eq!(a.alloc(2), Extent { start: 3, pages: 2 });
        assert_eq!(a.alloc(1), Extent { start: 7, pages: 1 });
        assert_eq!(a.alloc(0), Extent::default());
    }

    #[test]
    fn version_root_binds_page_numbers_and_versions() {
        let k = [3u8; 32];
        let d = sample_dir();
        let ext = Extent { start: 2, pages: 1 };
        let r = version_root(&k, ext, &[1], &d);
        assert_ne!(r, version_root(&k, ext, &[2], &d));
        assert_ne!(r, version_root(&k, Extent { start: 6, pages: 1 }, &[1], &d));
        let mut d2 = sample_dir();
        d2.segments[0].versions[0] = 10;
        assert_ne!(r, version_root(&k, ext, &[1], &d2));
        assert_ne!(r, version_root(&[4u8; 32], ext, &[1], &d));
    }

    #[test]
    fn superblock_mac_rejects_any_change() {
        let c = Container::create(MemPageStore::new(256)).unwrap();
        let sb = Superblock {
            seq: 5,
            dir: Extent { start: 2, pages: 1 },
            dir_len: 10,
            dir_hash: [1; 32],
            max_version: 3,
            ver_root: [2; 32],
        };
        let page = c.encode_sb(&sb);
        let back = Container::<MemPageStore>::decode_sb(&c.key, 256, &page).unwrap();
        assert_eq!((back.seq, back.max_version, back.ver_root), (5, 3, [2; 32]));
        for i in 0..page.len() {
            let mut p = page.clone();
            p[i] ^= 0x80;
            assert!(
                Container::<MemPageStore>::decode_sb(&c.key, 256, &p).is_err(),
                "byte {i}"
            );
        }
    }
}
