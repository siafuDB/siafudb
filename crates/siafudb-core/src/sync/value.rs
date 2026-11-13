// siafudb-core/src/sync/value.rs
//
// The typed value carried by a `GraphChange`, and its mapping to and from
// engine values.

use crate::error::SiafuError;
use std::collections::BTreeMap;

/// Map key that marks the lossless encoding of an engine value with no
/// direct [`ChangeValue`] form (see [`ChangeValue`]).
pub const ENGINE_VALUE_KEY: &str = "$engine";

/// A property value in a [`GraphChange`](super::GraphChange).
///
/// Eight shapes only, so a protocol layer can encode it without knowing the
/// engine. Engine values map as follows:
///
/// | Engine value | `ChangeValue` |
/// | --- | --- |
/// | null, bool, 64-bit integer, 64-bit float, string, bytes | the same shape |
/// | list, map | `List` / `Map`, element by element |
/// | everything else (decimal, date, time, timestamp, zoned datetime, duration, vector, counters, ...) | `Map { "$engine": String(json) }` |
///
/// The last row is a **lossless encoding**: `json` is the engine's own serde
/// form of the value, and capture checks that it decodes back to an equal
/// value before reporting it. A value that does not survive that round trip
/// (for example a vector holding NaN) is refused with
/// [`SiafuError::UnsupportedValue`] instead of being changed silently, as is
/// a user map that itself has the key `"$engine"`.
///
/// Note that the engine reads a query literal such as `1.5` as a decimal,
/// not a float, so decimals are common; a bound `f64` parameter is a float.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeValue {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<ChangeValue>),
    Map(BTreeMap<String, ChangeValue>),
}

impl ChangeValue {
    /// Engine value -> change value. See the type's table.
    pub fn from_engine(value: &grafeo::Value) -> Result<Self, SiafuError> {
        use grafeo::Value as E;
        Ok(match value {
            E::Null => Self::Null,
            E::Bool(b) => Self::Bool(*b),
            E::Int64(i) => Self::I64(*i),
            E::Float64(f) => Self::F64(*f),
            E::String(s) => Self::String(s.to_string()),
            E::Bytes(b) => Self::Bytes(b.to_vec()),
            E::List(items) => Self::List(
                items
                    .iter()
                    .map(Self::from_engine)
                    .collect::<Result<_, _>>()?,
            ),
            E::Map(map) => {
                if map.keys().any(|k| k.as_str() == ENGINE_VALUE_KEY) {
                    return Err(SiafuError::UnsupportedValue(format!(
                        "map key {ENGINE_VALUE_KEY:?} is reserved for the encoding of engine values"
                    )));
                }
                Self::Map(
                    map.iter()
                        .map(|(k, v)| Ok((k.as_str().to_string(), Self::from_engine(v)?)))
                        .collect::<Result<_, SiafuError>>()?,
                )
            }
            other => {
                let json = serde_json::to_string(other).map_err(|e| {
                    SiafuError::UnsupportedValue(format!("{other:?} cannot be encoded: {e}"))
                })?;
                let back: Result<grafeo::Value, _> = serde_json::from_str(&json);
                if back.ok().as_ref() != Some(other) {
                    return Err(SiafuError::UnsupportedValue(format!(
                        "{other:?} has no lossless encoding"
                    )));
                }
                Self::Map(BTreeMap::from([(
                    ENGINE_VALUE_KEY.to_string(),
                    Self::String(json),
                )]))
            }
        })
    }

    /// Change value -> engine value; the inverse of [`Self::from_engine`].
    pub fn to_engine(&self) -> Result<grafeo::Value, SiafuError> {
        use grafeo::Value as E;
        Ok(match self {
            Self::Null => E::Null,
            Self::Bool(b) => E::Bool(*b),
            Self::I64(i) => E::Int64(*i),
            Self::F64(f) => E::Float64(*f),
            Self::String(s) => E::from(s.as_str()),
            Self::Bytes(b) => E::from(b.as_slice()),
            Self::List(items) => E::List(
                items
                    .iter()
                    .map(Self::to_engine)
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
            ),
            Self::Map(map) => {
                if let Some(encoded) = map.get(ENGINE_VALUE_KEY) {
                    let Self::String(json) = encoded else {
                        return Err(SiafuError::InvalidChange(format!(
                            "{ENGINE_VALUE_KEY:?} must hold a string"
                        )));
                    };
                    if map.len() != 1 {
                        return Err(SiafuError::InvalidChange(format!(
                            "{ENGINE_VALUE_KEY:?} must be the only key of its map"
                        )));
                    }
                    return serde_json::from_str(json).map_err(|e| {
                        SiafuError::InvalidChange(format!("undecodable engine value: {e}"))
                    });
                }
                E::Map(
                    map.iter()
                        .map(|(k, v)| {
                            Ok((grafeo::store::PropertyKey::from(k.as_str()), v.to_engine()?))
                        })
                        .collect::<Result<BTreeMap<_, _>, SiafuError>>()?
                        .into(),
                )
            }
        })
    }
}
