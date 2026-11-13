// siafudb-core/src/sync/codec.rs
//
// Private binary form of the sync state appended to a snapshot when change
// capture is on (see `SiafuDB::export_snapshot`). Layout of a snapshot:
//
//   engine snapshot bytes | payload | payload length (u64 LE) | MAGIC
//
// payload = version (u8 = 1) | change count (u32 LE) | changes. It is not a
// wire format: the protocol layer has its own encoding.

use super::{ChangeValue, Gid, GraphChange};
use crate::error::SiafuError;
use std::collections::BTreeMap;

const MAGIC: &[u8; 8] = b"SIAFSYN1";
const VERSION: u8 = 1;
/// Nesting bound for decoded values: a snapshot is input, not trusted.
const MAX_DEPTH: usize = 64;

/// Append the trailer holding `pending` to an engine snapshot.
pub(crate) fn append_trailer(mut snapshot: Vec<u8>, pending: &[GraphChange]) -> Vec<u8> {
    let start = snapshot.len();
    snapshot.push(VERSION);
    put_len(&mut snapshot, pending.len());
    for change in pending {
        put_change(&mut snapshot, change);
    }
    let len = (snapshot.len() - start) as u64;
    snapshot.extend_from_slice(&len.to_le_bytes());
    snapshot.extend_from_slice(MAGIC);
    snapshot
}

/// Split a snapshot into the engine's bytes and, when it has a trailer, the
/// changes that were still queued at export.
pub(crate) fn split_trailer(data: &[u8]) -> Result<(&[u8], Option<Vec<GraphChange>>), SiafuError> {
    let Some(rest) = data.strip_suffix(MAGIC) else {
        return Ok((data, None));
    };
    let bad = || SiafuError::EngineError("snapshot: corrupt sync trailer".to_string());
    let (rest, len) = rest.split_last_chunk::<8>().ok_or_else(bad)?;
    let len = usize::try_from(u64::from_le_bytes(*len)).map_err(|_| bad())?;
    let engine_len = rest.len().checked_sub(len).ok_or_else(bad)?;
    let (engine, payload) = rest.split_at(engine_len);
    let mut r = Reader { data: payload };
    if r.u8()? != VERSION {
        return Err(SiafuError::EngineError(
            "snapshot: unsupported sync trailer version".to_string(),
        ));
    }
    let count = r.len()?;
    let mut pending = Vec::new();
    for _ in 0..count {
        pending.push(r.change()?);
    }
    if !r.data.is_empty() {
        return Err(bad());
    }
    Ok((engine, Some(pending)))
}

fn put_len(out: &mut Vec<u8>, len: usize) {
    // Lengths are of in-memory collections; u32 bounds one snapshot section.
    out.extend_from_slice(&u32::try_from(len).unwrap_or(u32::MAX).to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_len(out, s.len());
    out.extend_from_slice(s.as_bytes());
}

fn put_strs(out: &mut Vec<u8>, items: &[String]) {
    put_len(out, items.len());
    for s in items {
        put_str(out, s);
    }
}

fn put_props(out: &mut Vec<u8>, props: &[(String, ChangeValue)]) {
    put_len(out, props.len());
    for (k, v) in props {
        put_str(out, k);
        put_value(out, v);
    }
}

fn put_value(out: &mut Vec<u8>, value: &ChangeValue) {
    match value {
        ChangeValue::Null => out.push(0),
        ChangeValue::Bool(b) => out.extend_from_slice(&[1, u8::from(*b)]),
        ChangeValue::I64(i) => {
            out.push(2);
            out.extend_from_slice(&i.to_le_bytes());
        }
        ChangeValue::F64(f) => {
            out.push(3);
            out.extend_from_slice(&f.to_bits().to_le_bytes());
        }
        ChangeValue::String(s) => {
            out.push(4);
            put_str(out, s);
        }
        ChangeValue::Bytes(b) => {
            out.push(5);
            put_len(out, b.len());
            out.extend_from_slice(b);
        }
        ChangeValue::List(items) => {
            out.push(6);
            put_len(out, items.len());
            for item in items {
                put_value(out, item);
            }
        }
        ChangeValue::Map(map) => {
            out.push(7);
            put_len(out, map.len());
            for (k, v) in map {
                put_str(out, k);
                put_value(out, v);
            }
        }
    }
}

fn put_change(out: &mut Vec<u8>, change: &GraphChange) {
    match change {
        GraphChange::NodePut {
            gid,
            labels_add,
            labels_remove,
            set,
            unset,
        } => {
            out.push(0);
            out.extend_from_slice(gid);
            put_strs(out, labels_add);
            put_strs(out, labels_remove);
            put_props(out, set);
            put_strs(out, unset);
        }
        GraphChange::NodeDel { gid } => {
            out.push(1);
            out.extend_from_slice(gid);
        }
        GraphChange::EdgePut {
            gid,
            src,
            dst,
            edge_type,
            set,
            unset,
        } => {
            out.push(2);
            out.extend_from_slice(gid);
            out.extend_from_slice(src);
            out.extend_from_slice(dst);
            put_str(out, edge_type);
            put_props(out, set);
            put_strs(out, unset);
        }
        GraphChange::EdgeDel { gid } => {
            out.push(3);
            out.extend_from_slice(gid);
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
}

impl Reader<'_> {
    fn bad() -> SiafuError {
        SiafuError::EngineError("snapshot: corrupt sync trailer".to_string())
    }

    fn take(&mut self, n: usize) -> Result<&[u8], SiafuError> {
        let (head, tail) = self.data.split_at_checked(n).ok_or_else(Self::bad)?;
        self.data = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, SiafuError> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, SiafuError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| Self::bad())?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// A count or length. Every counted item takes at least one byte, so a
    /// count past the bytes left is corrupt (and must not size an allocation).
    fn len(&mut self) -> Result<usize, SiafuError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| Self::bad())?;
        let len = u32::from_le_bytes(bytes) as usize;
        if len > self.data.len() {
            return Err(Self::bad());
        }
        Ok(len)
    }

    fn gid(&mut self) -> Result<Gid, SiafuError> {
        self.take(16)?.try_into().map_err(|_| Self::bad())
    }

    fn str(&mut self) -> Result<String, SiafuError> {
        let len = self.len()?;
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| Self::bad())
    }

    fn strs(&mut self) -> Result<Vec<String>, SiafuError> {
        (0..self.len()?).map(|_| self.str()).collect()
    }

    fn props(&mut self) -> Result<Vec<(String, ChangeValue)>, SiafuError> {
        (0..self.len()?)
            .map(|_| Ok((self.str()?, self.value(0)?)))
            .collect()
    }

    fn value(&mut self, depth: usize) -> Result<ChangeValue, SiafuError> {
        if depth > MAX_DEPTH {
            return Err(Self::bad());
        }
        Ok(match self.u8()? {
            0 => ChangeValue::Null,
            1 => ChangeValue::Bool(self.u8()? != 0),
            2 => ChangeValue::I64(self.u64()? as i64),
            3 => ChangeValue::F64(f64::from_bits(self.u64()?)),
            4 => ChangeValue::String(self.str()?),
            5 => {
                let len = self.len()?;
                ChangeValue::Bytes(self.take(len)?.to_vec())
            }
            6 => ChangeValue::List(
                (0..self.len()?)
                    .map(|_| self.value(depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            7 => ChangeValue::Map(
                (0..self.len()?)
                    .map(|_| Ok((self.str()?, self.value(depth + 1)?)))
                    .collect::<Result<BTreeMap<_, _>, SiafuError>>()?,
            ),
            _ => return Err(Self::bad()),
        })
    }

    fn change(&mut self) -> Result<GraphChange, SiafuError> {
        Ok(match self.u8()? {
            0 => GraphChange::NodePut {
                gid: self.gid()?,
                labels_add: self.strs()?,
                labels_remove: self.strs()?,
                set: self.props()?,
                unset: self.strs()?,
            },
            1 => GraphChange::NodeDel { gid: self.gid()? },
            2 => GraphChange::EdgePut {
                gid: self.gid()?,
                src: self.gid()?,
                dst: self.gid()?,
                edge_type: self.str()?,
                set: self.props()?,
                unset: self.strs()?,
            },
            3 => GraphChange::EdgeDel { gid: self.gid()? },
            _ => return Err(Self::bad()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailer_round_trip_and_absence() {
        let changes = vec![
            GraphChange::NodePut {
                gid: [1; 16],
                labels_add: vec!["A".into()],
                labels_remove: vec!["B".into()],
                set: vec![(
                    "k".into(),
                    ChangeValue::List(vec![
                        ChangeValue::Null,
                        ChangeValue::Bool(true),
                        ChangeValue::I64(-7),
                        ChangeValue::F64(-0.0),
                        ChangeValue::String("s".into()),
                        ChangeValue::Bytes(vec![0, 255]),
                        ChangeValue::Map(BTreeMap::from([("m".into(), ChangeValue::I64(1))])),
                    ]),
                )],
                unset: vec!["gone".into()],
            },
            GraphChange::EdgePut {
                gid: [2; 16],
                src: [1; 16],
                dst: [3; 16],
                edge_type: "T".into(),
                set: vec![],
                unset: vec![],
            },
            GraphChange::EdgeDel { gid: [2; 16] },
            GraphChange::NodeDel { gid: [1; 16] },
        ];
        let engine = vec![9u8, 8, 7];
        let snap = append_trailer(engine.clone(), &changes);
        let (head, pending) = split_trailer(&snap).unwrap();
        assert_eq!(head, engine.as_slice());
        assert_eq!(pending.unwrap(), changes);

        let (head, pending) = split_trailer(&engine).unwrap();
        assert_eq!(head, engine.as_slice());
        assert!(pending.is_none());

        // A truncated or damaged trailer is an error, never a panic.
        let payload = snap.len() - engine.len() - 16;
        for cut in 1..payload {
            // Payload bytes missing, the length and the magic intact.
            let mut damaged = snap[..snap.len() - 16 - cut].to_vec();
            damaged.extend_from_slice(&snap[snap.len() - 16..]);
            assert!(split_trailer(&damaged).is_err(), "cut {cut}");
            // The payload cut short, with its length adjusted to match.
            let mut short = snap[..snap.len() - 16 - cut].to_vec();
            short.extend_from_slice(&((payload - cut) as u64).to_le_bytes());
            short.extend_from_slice(MAGIC);
            assert!(split_trailer(&short).is_err(), "short {cut}");
        }
        assert!(split_trailer(MAGIC).is_err());
    }
}
