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
pub(crate) struct WireFields<'a> {
    object: &'a Map<String, Value>,
    modeled: Vec<&'static str>,
}

impl<'a> WireFields<'a> {
    /// A tagged union's object: `type` counts as modeled.
    pub(crate) fn tagged(object: &'a Map<String, Value>) -> Self {
        Self {
            object,
            modeled: vec!["type"],
        }
    }

    /// A nested object (such as a call step's `arguments`), where a `type`
    /// key is ordinary data.
    pub(crate) fn nested(object: &'a Map<String, Value>) -> Self {
        Self {
            object,
            modeled: Vec::new(),
        }
    }

    /// A field the variant requires. Absent is an error.
    pub(crate) fn required<T: Deserialize<'a>>(
        &mut self,
        key: &'static str,
    ) -> Result<T, serde_json::Error> {
        self.modeled.push(key);
        match self.object.get(key) {
            Some(value) => T::deserialize(value),
            None => Err(serde_json::Error::missing_field(key)),
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
            Some(value) => T::deserialize(value).map(Some),
        }
    }

    /// An optional field that falls back to its type's default.
    pub(crate) fn or_default<T: Deserialize<'a> + Default>(
        &mut self,
        key: &'static str,
    ) -> Result<T, serde_json::Error> {
        Ok(self.optional(key)?.unwrap_or_default())
    }

    /// An optional nested object, read with its own [`WireFields`]. A
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
            Some(Value::Object(inner)) => Ok(Some(WireFields::nested(inner))),
            Some(other) => Err(serde_json::Error::custom(format!(
                "expected `{key}` to be an object, got {other}"
            ))),
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

/// The JSON object behind a tagged union, or an error for any other value.
pub(crate) fn as_object(value: &Value) -> Result<&Map<String, Value>, serde_json::Error> {
    value
        .as_object()
        .ok_or_else(|| serde_json::Error::custom(format!("expected an object, got {value}")))
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
