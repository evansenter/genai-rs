//! Field-level round-trip support for the hand-written Evergreen unions.
//!
//! `Step`, `StepDelta`, `Content` and `Annotation` model the fields they know
//! and keep every other key of a known variant in that variant's `extra` map,
//! so a field the API adds survives a deserialize/re-serialize round trip
//! (#491). The call steps that reshape a nested `arguments` object keep its
//! unmodeled keys in `arguments_extra` the same way.
//!
//! [`WireFields`] reads a wire object one field at a time and records each
//! key it reads, so [`WireFields::extra`] is exactly what the variant did not
//! model, with no second key list to keep in step. [`ModeledMap`] writes the
//! modeled fields, recording each key whether or not it was written, then the
//! extras minus `type` and every recorded key. A hand-built `extra` can
//! therefore never duplicate a modeled field or change the variant's `type`.

use serde::Deserialize;
use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

/// Reads a JSON object field by field, remembering which keys were read.
///
/// Every error names the field, with its path from the tagged object
/// (`arguments.language`), so the cause survives into the warning and the
/// `strict-unknown` error for a known type that falls back to `Unknown`.
pub(crate) struct WireFields<'a> {
    object: &'a Map<String, Value>,
    /// This object's path inside the tagged one, ending in `.`, or empty.
    prefix: String,
    modeled: Vec<&'static str>,
}

impl<'a> WireFields<'a> {
    /// A tagged union's object: `type` counts as modeled.
    pub(crate) fn tagged(object: &'a Map<String, Value>) -> Self {
        Self {
            object,
            prefix: String::new(),
            modeled: vec!["type"],
        }
    }

    /// The object's `type`, if it is a string.
    pub(crate) fn tag(&self) -> Option<&'a str> {
        self.object.get("type").and_then(Value::as_str)
    }

    /// An error about `key`, naming its full path.
    fn field_error(&self, key: &str, cause: impl std::fmt::Display) -> serde_json::Error {
        serde_json::Error::custom(format!("field `{}{key}`: {cause}", self.prefix))
    }

    /// A field the variant requires. Absent is an error.
    pub(crate) fn required<T: Deserialize<'a>>(
        &mut self,
        key: &'static str,
    ) -> Result<T, serde_json::Error> {
        self.modeled.push(key);
        match self.object.get(key) {
            Some(value) => T::deserialize(value).map_err(|e| self.field_error(key, e)),
            None => Err(self.field_error(key, "missing")),
        }
    }

    /// An optional field. Absent and `null` both read as `None`; any other
    /// value must have the field's type.
    pub(crate) fn optional<T: Deserialize<'a>>(
        &mut self,
        key: &'static str,
    ) -> Result<Option<T>, serde_json::Error> {
        self.modeled.push(key);
        match self.object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => T::deserialize(value)
                .map(Some)
                .map_err(|e| self.field_error(key, e)),
        }
    }

    /// An optional field that falls back to its type's default.
    pub(crate) fn or_default<T: Deserialize<'a> + Default>(
        &mut self,
        key: &'static str,
    ) -> Result<T, serde_json::Error> {
        Ok(self.optional(key)?.unwrap_or_default())
    }

    /// An optional nested object (such as a call step's `arguments`), read
    /// with its own [`WireFields`], where a `type` key is ordinary data. A
    /// present, non-null value that is not an object is an error: the
    /// variant cannot model it, and failing sends the whole object to the
    /// union's `Unknown` variant, which keeps it.
    pub(crate) fn object(
        &mut self,
        key: &'static str,
    ) -> Result<Option<WireFields<'a>>, serde_json::Error> {
        self.modeled.push(key);
        match self.object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(inner)) => Ok(Some(WireFields {
                object: inner,
                prefix: format!("{}{key}.", self.prefix),
                modeled: Vec::new(),
            })),
            Some(other) => Err(self.field_error(key, format!("expected an object, got {other}"))),
        }
    }

    /// Every entry the variant did not read.
    pub(crate) fn extra(&self) -> Map<String, Value> {
        self.object
            .iter()
            .filter(|(key, _)| !self.modeled.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}

/// Why a tagged union's value did not read as a known variant. The union
/// keeps the value whole as its `Unknown` variant, or, where
/// `strict-unknown` applies (`Step`, `Content`), fails with the cause.
#[derive(Debug)]
pub(crate) enum Fallback {
    /// Not an object with a string `type` that the crate models.
    UnknownType,
    /// A known `type` with a modeled field that could not be read. The
    /// error names the field and says why.
    Malformed(serde_json::Error),
}

impl From<serde_json::Error> for Fallback {
    fn from(error: serde_json::Error) -> Self {
        Self::Malformed(error)
    }
}

impl Fallback {
    /// Reads `value` with a union's known-variant reader. A value that is
    /// not an object has no type to know.
    pub(crate) fn read<T>(
        value: &Value,
        read_known: impl FnOnce(&Map<String, Value>) -> Result<T, Self>,
    ) -> Result<T, Self> {
        value.as_object().map_or(Err(Self::UnknownType), read_known)
    }

    /// The cause, for the warning logged when the value becomes `Unknown`.
    pub(crate) fn describe(&self, union: &str, type_name: &str) -> String {
        match self {
            Self::UnknownType => format!("unknown {union} type '{type_name}'"),
            Self::Malformed(error) => {
                format!("{union} '{type_name}' has a field this crate cannot read: {error}")
            }
        }
    }

    /// The `strict-unknown` error: an unknown type, or a known one with a
    /// field the crate cannot read, with that field and why.
    #[cfg(feature = "strict-unknown")]
    pub(crate) fn strict_message(&self, union: &str, type_name: &str) -> String {
        const GUIDANCE: &str = "Either update the library or disable strict mode.";
        match self {
            Self::UnknownType => format!(
                "Unknown {union} type '{type_name}'. Strict mode is enabled via the \
                 'strict-unknown' feature flag. {GUIDANCE}"
            ),
            Self::Malformed(error) => {
                let cause = error.to_string();
                // A nested value that strict mode rejected (an unknown enum
                // value or content type) already says so; say it once.
                if cause.contains("'strict-unknown'") {
                    self.describe(union, type_name)
                } else {
                    format!(
                        "{}. Strict mode is enabled via the 'strict-unknown' feature \
                         flag, which rejects what would otherwise be kept whole as \
                         {union}::Unknown. {GUIDANCE}",
                        self.describe(union, type_name)
                    )
                }
            }
        }
    }
}

/// Writes a variant's modeled fields, then its extras.
pub(crate) struct ModeledMap<M> {
    map: M,
    modeled: Vec<&'static str>,
}

impl<M: SerializeMap> ModeledMap<M> {
    /// Opens a map and writes `"type": type_name`.
    pub(crate) fn tagged<S>(serializer: S, type_name: &str) -> Result<Self, S::Error>
    where
        S: Serializer<SerializeMap = M, Error = M::Error>,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", type_name)?;
        Ok(Self {
            map,
            modeled: vec!["type"],
        })
    }

    /// Writes a modeled field.
    pub(crate) fn field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), M::Error> {
        self.modeled.push(key);
        self.map.serialize_entry(key, value)
    }

    /// Writes a modeled field when it is `Some`; the key counts as modeled
    /// either way.
    pub(crate) fn opt<T: Serialize>(
        &mut self,
        key: &'static str,
        value: Option<&T>,
    ) -> Result<(), M::Error> {
        self.modeled.push(key);
        match value {
            Some(v) => self.map.serialize_entry(key, v),
            None => Ok(()),
        }
    }

    /// Writes a modeled field unless `skip`; the key counts as modeled
    /// either way.
    pub(crate) fn field_unless<T: Serialize + ?Sized>(
        &mut self,
        skip: bool,
        key: &'static str,
        value: &T,
    ) -> Result<(), M::Error> {
        self.modeled.push(key);
        if skip {
            Ok(())
        } else {
            self.map.serialize_entry(key, value)
        }
    }

    /// Writes the extras that do not name a modeled field, and closes the map.
    pub(crate) fn end(mut self, extra: &Map<String, Value>) -> Result<M::Ok, M::Error> {
        for (key, value) in extra {
            if !self.modeled.contains(&key.as_str()) {
                self.map.serialize_entry(key, value)?;
            }
        }
        self.map.end()
    }
}

/// Builds a reshaped `arguments` object: the modeled entries that are
/// `Some`, then the extras that do not name a modeled key, set or not.
pub(crate) fn merged_arguments<const N: usize>(
    modeled: [(&'static str, Option<Value>); N],
    extra: &Map<String, Value>,
) -> Map<String, Value> {
    let keys: [&str; N] = std::array::from_fn(|i| modeled[i].0);
    let mut arguments: Map<String, Value> = modeled
        .into_iter()
        .filter_map(|(key, value)| Some((key.to_string(), value?)))
        .collect();
    for (key, value) in extra {
        if !keys.contains(&key.as_str()) {
            arguments.insert(key.clone(), value.clone());
        }
    }
    arguments
}

#[cfg(test)]
#[path = "wire_fields_tests.rs"]
mod tests;
