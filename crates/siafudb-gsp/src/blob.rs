//! The combined persisted form: engine snapshot, replica state and the
//! transport's own note in one blob (SYNC-V0 section 7: one atomic write).
//!
//! ```text
//! blob    = MAGIC version:u8 section section section
//! section = len:u64le bytes          ; engine snapshot, replica state, meta
//! ```
//!
//! An empty replica section means "no replica". A blob that does not start
//! with `MAGIC` is a bare engine snapshot, as written before sync existed.

/// First bytes of a combined blob.
pub(crate) const MAGIC: &[u8; 8] = b"SIAFGSP\0";
/// The only version this build writes and reads.
pub(crate) const VERSION: u8 = 1;

pub(crate) struct Parts<'a> {
    pub engine: &'a [u8],
    pub replica: &'a [u8],
    pub meta: &'a [u8],
}

pub(crate) fn encode(parts: &Parts<'_>) -> Vec<u8> {
    let sections = [parts.engine, parts.replica, parts.meta];
    let len = MAGIC.len() + 1 + sections.iter().map(|s| 8 + s.len()).sum::<usize>();
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    for section in sections {
        out.extend_from_slice(&(section.len() as u64).to_le_bytes());
        out.extend_from_slice(section);
    }
    out
}

/// `Ok(None)`: not a combined blob (treat as a bare engine snapshot).
pub(crate) fn decode(blob: &[u8]) -> Result<Option<Parts<'_>>, &'static str> {
    let Some(rest) = blob.strip_prefix(MAGIC) else {
        return Ok(None);
    };
    let (version, mut rest) = rest.split_first().ok_or("truncated")?;
    if *version != VERSION {
        return Err("unknown version");
    }
    let mut section = || -> Result<&[u8], &'static str> {
        let (len, tail) = rest.split_first_chunk::<8>().ok_or("truncated")?;
        let len = usize::try_from(u64::from_le_bytes(*len)).map_err(|_| "truncated")?;
        let (bytes, tail) = tail.split_at_checked(len).ok_or("truncated")?;
        rest = tail;
        Ok(bytes)
    };
    let parts = Parts {
        engine: section()?,
        replica: section()?,
        meta: section()?,
    };
    if !rest.is_empty() {
        return Err("trailing bytes");
    }
    Ok(Some(parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_strictness() {
        let blob = encode(&Parts {
            engine: b"engine",
            replica: b"",
            meta: b"m",
        });
        let parts = decode(&blob).unwrap().unwrap();
        assert_eq!(
            (parts.engine, parts.replica, parts.meta),
            (&b"engine"[..], &b""[..], &b"m"[..])
        );
        assert!(decode(b"anything else").unwrap().is_none());
        assert!(decode(b"").unwrap().is_none());
        for cut in MAGIC.len()..blob.len() {
            assert_eq!(
                decode(&blob[..cut]).err(),
                Some("truncated"),
                "cut at {cut}"
            );
        }
        let mut long = blob.clone();
        long.push(0);
        assert_eq!(decode(&long).err(), Some("trailing bytes"));
        let mut other = blob.clone();
        other[MAGIC.len()] = 2;
        assert_eq!(decode(&other).err(), Some("unknown version"));
        // A length that would overflow must not panic or allocate.
        let mut huge = blob;
        huge[MAGIC.len() + 1..MAGIC.len() + 9].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode(&huge).err(), Some("truncated"));
    }
}
