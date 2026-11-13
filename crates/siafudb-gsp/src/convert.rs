//! `GraphChange` <-> `gsp_sync::Change` and `ChangeValue` <-> `gsp_wire::Value`.
//!
//! Both directions are lossless or fail; nothing is rounded, truncated or
//! dropped. Two things are not bit-exact and cannot be: the protocol encodes
//! every NaN as one canonical NaN, and it orders map entries itself.

use gsp_sync::Change;
use gsp_wire::Value;
use siafudb_core::{ChangeValue, GraphChange};
use std::collections::{BTreeMap, BTreeSet};

/// Why a change or value has no form on the other side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// An integer from the wire outside the engine's 64-bit range.
    IntRange,
    /// A map from the wire that holds one key twice.
    DuplicateKey,
    /// A label, property key or `unset` entry listed twice in one change.
    Duplicate(&'static str),
    /// A property both set and unset, or a label both added and removed, in
    /// one change.
    Conflict(&'static str),
}

impl core::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ConvertError::IntRange => write!(f, "integer outside the 64-bit range"),
            ConvertError::DuplicateKey => write!(f, "map with a duplicate key"),
            ConvertError::Duplicate(what) => write!(f, "{what} listed twice in one change"),
            ConvertError::Conflict(what) => write!(f, "{what} both written and removed"),
        }
    }
}
impl core::error::Error for ConvertError {}

/// Engine-side value -> wire value. Total: every `ChangeValue` has a wire
/// form (nesting depth and size are checked when the op is encoded).
pub fn value_to_wire(value: &ChangeValue) -> Value {
    match value {
        ChangeValue::Null => Value::Null,
        ChangeValue::Bool(b) => Value::Bool(*b),
        ChangeValue::I64(i) => Value::Int(i128::from(*i)),
        ChangeValue::F64(f) => Value::Float(*f),
        ChangeValue::String(s) => Value::Text(s.clone()),
        ChangeValue::Bytes(b) => Value::Bytes(b.clone()),
        ChangeValue::List(items) => Value::Array(items.iter().map(value_to_wire).collect()),
        ChangeValue::Map(map) => Value::Map(
            map.iter()
                .map(|(k, v)| (k.clone(), value_to_wire(v)))
                .collect(),
        ),
    }
}

/// Wire value -> engine-side value.
pub fn value_from_wire(value: &Value) -> Result<ChangeValue, ConvertError> {
    Ok(match value {
        Value::Null => ChangeValue::Null,
        Value::Bool(b) => ChangeValue::Bool(*b),
        Value::Int(i) => ChangeValue::I64(i64::try_from(*i).map_err(|_| ConvertError::IntRange)?),
        Value::Float(f) => ChangeValue::F64(*f),
        Value::Text(s) => ChangeValue::String(s.clone()),
        Value::Bytes(b) => ChangeValue::Bytes(b.clone()),
        Value::Array(items) => ChangeValue::List(
            items
                .iter()
                .map(value_from_wire)
                .collect::<Result<_, _>>()?,
        ),
        Value::Map(entries) => {
            let mut map = BTreeMap::new();
            for (k, v) in entries {
                if map.insert(k.clone(), value_from_wire(v)?).is_some() {
                    return Err(ConvertError::DuplicateKey);
                }
            }
            ChangeValue::Map(map)
        }
    })
}

fn names(list: &[String], what: &'static str) -> Result<BTreeSet<String>, ConvertError> {
    let mut out = BTreeSet::new();
    for name in list {
        if !out.insert(name.clone()) {
            return Err(ConvertError::Duplicate(what));
        }
    }
    Ok(out)
}

fn props(
    set: &[(String, ChangeValue)],
    unset: &[String],
) -> Result<(BTreeMap<String, Value>, BTreeSet<String>), ConvertError> {
    let mut out = BTreeMap::new();
    for (key, value) in set {
        if out.insert(key.clone(), value_to_wire(value)).is_some() {
            return Err(ConvertError::Duplicate("property"));
        }
    }
    let unset = names(unset, "removed property")?;
    if unset.iter().any(|k| out.contains_key(k)) {
        return Err(ConvertError::Conflict("property"));
    }
    Ok((out, unset))
}

/// Engine-side change -> protocol change.
pub fn change_to_wire(change: &GraphChange) -> Result<Change, ConvertError> {
    Ok(match change {
        GraphChange::NodePut {
            gid,
            labels_add,
            labels_remove,
            set,
            unset,
        } => {
            let labels_add = names(labels_add, "label")?;
            let labels_remove = names(labels_remove, "removed label")?;
            if labels_add.intersection(&labels_remove).next().is_some() {
                return Err(ConvertError::Conflict("label"));
            }
            let (set, unset) = props(set, unset)?;
            Change::NodePut {
                gid: *gid,
                labels_add,
                labels_remove,
                set,
                unset,
            }
        }
        GraphChange::NodeDel { gid } => Change::NodeDel { gid: *gid },
        GraphChange::EdgePut {
            gid,
            src,
            dst,
            edge_type,
            set,
            unset,
        } => {
            let (set, unset) = props(set, unset)?;
            Change::EdgePut {
                gid: *gid,
                src: *src,
                dst: *dst,
                ty: edge_type.clone(),
                set,
                unset,
            }
        }
        GraphChange::EdgeDel { gid } => Change::EdgeDel { gid: *gid },
    })
}

fn props_from(set: &BTreeMap<String, Value>) -> Result<Vec<(String, ChangeValue)>, ConvertError> {
    set.iter()
        .map(|(k, v)| Ok((k.clone(), value_from_wire(v)?)))
        .collect()
}

/// Protocol change -> engine-side change.
pub fn change_from_wire(change: &Change) -> Result<GraphChange, ConvertError> {
    Ok(match change {
        Change::NodePut {
            gid,
            labels_add,
            labels_remove,
            set,
            unset,
        } => GraphChange::NodePut {
            gid: *gid,
            labels_add: labels_add.iter().cloned().collect(),
            labels_remove: labels_remove.iter().cloned().collect(),
            set: props_from(set)?,
            unset: unset.iter().cloned().collect(),
        },
        Change::NodeDel { gid } => GraphChange::NodeDel { gid: *gid },
        Change::EdgePut {
            gid,
            src,
            dst,
            ty,
            set,
            unset,
        } => GraphChange::EdgePut {
            gid: *gid,
            src: *src,
            dst: *dst,
            edge_type: ty.clone(),
            set: props_from(set)?,
            unset: unset.iter().cloned().collect(),
        },
        Change::EdgeDel { gid } => GraphChange::EdgeDel { gid: *gid },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_shape() -> ChangeValue {
        ChangeValue::Map(BTreeMap::from([
            ("null".to_string(), ChangeValue::Null),
            ("bool".to_string(), ChangeValue::Bool(true)),
            ("min".to_string(), ChangeValue::I64(i64::MIN)),
            ("max".to_string(), ChangeValue::I64(i64::MAX)),
            ("float".to_string(), ChangeValue::F64(-0.0)),
            ("big".to_string(), ChangeValue::F64(1.0e300)),
            ("inf".to_string(), ChangeValue::F64(f64::NEG_INFINITY)),
            (
                "text".to_string(),
                ChangeValue::String("O'Brien \u{0} 🐜".into()),
            ),
            ("bytes".to_string(), ChangeValue::Bytes(vec![0, 255, 7])),
            (
                "list".to_string(),
                ChangeValue::List(vec![
                    ChangeValue::I64(1),
                    ChangeValue::List(vec![]),
                    ChangeValue::Map(BTreeMap::new()),
                ]),
            ),
            (
                "$engine".to_string(),
                ChangeValue::String("{\"Date\":7}".into()),
            ),
        ]))
    }

    #[test]
    fn values_round_trip_through_the_wire_encoding() {
        let value = every_shape();
        let wire = value_to_wire(&value);
        assert_eq!(value_from_wire(&wire).unwrap(), value);
        // And through the bytes a relay would carry.
        let decoded = Value::decode(&wire.encode().unwrap()).unwrap();
        assert_eq!(value_from_wire(&decoded).unwrap(), value);
        // -0.0 keeps its sign.
        let ChangeValue::F64(z) = value_from_wire(&value_to_wire(&ChangeValue::F64(-0.0))).unwrap()
        else {
            panic!("not a float");
        };
        assert!(z.is_sign_negative());
    }

    #[test]
    fn changes_round_trip() {
        let (a, b, e) = ([1u8; 16], [2u8; 16], [3u8; 16]);
        let changes = vec![
            GraphChange::NodePut {
                gid: a,
                labels_add: vec!["Admin".into(), "Person".into()],
                labels_remove: vec!["Guest".into()],
                set: vec![
                    ("age".into(), ChangeValue::I64(31)),
                    ("all".into(), every_shape()),
                ],
                unset: vec!["city".into(), "nick".into()],
            },
            GraphChange::EdgePut {
                gid: e,
                src: a,
                dst: b,
                edge_type: "KNOWS".into(),
                set: vec![("since".into(), ChangeValue::I64(2024))],
                unset: vec!["close".into()],
            },
            GraphChange::EdgeDel { gid: e },
            GraphChange::NodeDel { gid: b },
        ];
        for change in &changes {
            let wire = change_to_wire(change).unwrap();
            assert_eq!(&change_from_wire(&wire).unwrap(), change);
        }
        // The whole list survives the op payload encoding too.
        let wire: Vec<Change> = changes.iter().map(|c| change_to_wire(c).unwrap()).collect();
        let payload = gsp_sync::encode_payload(&wire).unwrap();
        let back =
            gsp_sync::decode_payload(&Value::decode(&payload.encode().unwrap()).unwrap()).unwrap();
        let back: Vec<GraphChange> = back.iter().map(|c| change_from_wire(c).unwrap()).collect();
        assert_eq!(back, changes);
    }

    #[test]
    fn what_has_no_other_form_is_an_error() {
        assert_eq!(
            value_from_wire(&Value::Int(i128::from(i64::MAX) + 1)),
            Err(ConvertError::IntRange)
        );
        assert_eq!(
            value_from_wire(&Value::Array(vec![Value::Int(i128::from(i64::MIN) - 1)])),
            Err(ConvertError::IntRange)
        );
        assert_eq!(
            value_from_wire(&Value::Map(vec![
                ("k".into(), Value::Null),
                ("k".into(), Value::Bool(true))
            ])),
            Err(ConvertError::DuplicateKey)
        );
        let node = |labels_add: &[&str], labels_remove: &[&str], set: &[&str], unset: &[&str]| {
            let s = |l: &[&str]| l.iter().map(|x| x.to_string()).collect::<Vec<_>>();
            change_to_wire(&GraphChange::NodePut {
                gid: [9; 16],
                labels_add: s(labels_add),
                labels_remove: s(labels_remove),
                set: set
                    .iter()
                    .map(|k| (k.to_string(), ChangeValue::Null))
                    .collect(),
                unset: s(unset),
            })
        };
        assert!(node(&["A"], &["B"], &["k"], &["j"]).is_ok());
        assert_eq!(
            node(&["A", "A"], &[], &[], &[]),
            Err(ConvertError::Duplicate("label"))
        );
        assert_eq!(
            node(&["A"], &["A"], &[], &[]),
            Err(ConvertError::Conflict("label"))
        );
        assert_eq!(
            node(&[], &[], &["k", "k"], &[]),
            Err(ConvertError::Duplicate("property"))
        );
        assert_eq!(
            node(&[], &[], &["k"], &["k"]),
            Err(ConvertError::Conflict("property"))
        );
        assert_eq!(
            node(&[], &[], &[], &["k", "k"]),
            Err(ConvertError::Duplicate("removed property"))
        );
    }
}
