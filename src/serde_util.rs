//! Shared serde helpers for protobuf-JSON wire conventions.
//!
//! The Interactions API family serializes int64 fields as JSON *strings*
//! (protobuf JSON convention) — live-verified on the environments resource
//! (`file_count`/`size_bytes`, 2026-08-08). These helpers absorb that on
//! deserialize (accepting plain numbers too) and reproduce it on serialize
//! where roundtrip fidelity to captured wire matters.

use serde::Deserialize;
use serde::de::Deserializer;

/// Serializes an optional int64 in the protobuf-JSON string form the API
/// uses on the wire, keeping deserialize-then-serialize roundtrips faithful
/// to captured responses.
#[allow(clippy::ref_option)] // signature dictated by serde's serialize_with
pub(crate) fn serialize_string_i64<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(n) => serializer.serialize_str(&n.to_string()),
        None => serializer.serialize_none(),
    }
}

/// Deserializes an optional int64 that the API serializes as a JSON string
/// (protobuf JSON convention), accepting a plain number too.
pub(crate) fn deserialize_string_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => {
            let parsed = n.as_i64();
            if parsed.is_none() {
                tracing::warn!("Non-i64 JSON number for int64 field, dropping: {n}");
            }
            Ok(parsed)
        }
        Some(serde_json::Value::String(s)) => {
            let parsed = s.parse().ok();
            if parsed.is_none() {
                tracing::warn!("Unparseable int64 string from API, dropping: {s:?}");
            }
            Ok(parsed)
        }
        Some(other) => {
            tracing::warn!("Unexpected JSON type for int64 field, dropping: {other:?}");
            Ok(None)
        }
    }
}
