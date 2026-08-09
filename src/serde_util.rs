//! Shared serde helpers for protobuf-JSON wire conventions.
//!
//! The Interactions API family serializes int64 fields as JSON *strings*
//! (protobuf JSON convention) — live-verified on the environments resource
//! (`file_count`/`size_bytes`, 2026-08-08). These helpers absorb that on
//! deserialize (accepting plain numbers too) and reproduce it on serialize
//! where roundtrip fidelity to captured wire matters.
//!
//! The wire-form fidelity claim is scoped to the int64s: the lenient
//! timestamps keep chrono's default `Serialize`, which emits valid
//! RFC 3339 but not necessarily the exact spelling captured from the live
//! wire (offset form, fractional-second width). A parsed `DateTime` cannot
//! remember the original formatting, and nothing in the crate re-sends a
//! deserialized resource, so the asymmetry is deliberate.

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

/// Deserializes an optional RFC 3339 timestamp, degrading an unparseable
/// string or unexpected JSON shape to `None` with a `warn!` instead of
/// failing the enclosing struct (and hence an entire list response).
///
/// Used on the trigger resource family, whose wire shape is unverified
/// (protobuf-JSON specifies RFC 3339 strings for `Timestamp`, but this
/// family already diverged from expectations once — int64s arrive as
/// strings), and on `Environment` for uniformity with the int64 helpers
/// next door even though its timestamp encoding was live-verified.
pub(crate) fn deserialize_lenient_timestamp<'de, D>(
    deserializer: D,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => {
            let parsed = chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok();
            if parsed.is_none() {
                tracing::warn!("Unparseable RFC 3339 timestamp from API, dropping: {s:?}");
            }
            Ok(parsed)
        }
        Some(other) => {
            tracing::warn!("Unexpected JSON type for timestamp field, dropping: {other:?}");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    /// Deliberately no `skip_serializing_if`, so the `None` serialize arm
    /// is reachable here even though production fields skip it.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Wrapper {
        #[serde(
            default,
            serialize_with = "super::serialize_string_i64",
            deserialize_with = "super::deserialize_string_i64"
        )]
        v: Option<i64>,
    }

    fn roundtrip(json: serde_json::Value) -> Option<i64> {
        serde_json::from_value::<Wrapper>(json).unwrap().v
    }

    #[test]
    fn happy_paths_accept_string_and_number() {
        assert_eq!(roundtrip(serde_json::json!({"v": "42"})), Some(42));
        assert_eq!(roundtrip(serde_json::json!({"v": 42})), Some(42));
        assert_eq!(roundtrip(serde_json::json!({"v": null})), None);
        assert_eq!(roundtrip(serde_json::json!({})), None);
    }

    #[test]
    fn degradation_arms_drop_to_none_instead_of_erroring() {
        // The degrade-per-field contract: one bad int must never fail the
        // struct (and hence an entire list response) it is embedded in.
        assert_eq!(roundtrip(serde_json::json!({"v": "12abc"})), None);
        assert_eq!(roundtrip(serde_json::json!({"v": ""})), None);
        assert_eq!(roundtrip(serde_json::json!({"v": 1.5})), None);
        assert_eq!(roundtrip(serde_json::json!({"v": true})), None);
        assert_eq!(roundtrip(serde_json::json!({"v": [1]})), None);
        assert_eq!(roundtrip(serde_json::json!({"v": {"n": 1}})), None);
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct TsWrapper {
        #[serde(default, deserialize_with = "super::deserialize_lenient_timestamp")]
        t: Option<chrono::DateTime<chrono::Utc>>,
    }

    fn ts_roundtrip(json: serde_json::Value) -> Option<chrono::DateTime<chrono::Utc>> {
        serde_json::from_value::<TsWrapper>(json).unwrap().t
    }

    #[test]
    fn lenient_timestamp_happy_paths() {
        let t = ts_roundtrip(serde_json::json!({"t": "2026-08-08T12:30:00Z"}));
        assert_eq!(t.unwrap().to_rfc3339(), "2026-08-08T12:30:00+00:00");
        // Offset forms normalize to UTC.
        let t = ts_roundtrip(serde_json::json!({"t": "2026-08-08T05:30:00-07:00"}));
        assert_eq!(t.unwrap().to_rfc3339(), "2026-08-08T12:30:00+00:00");
        assert_eq!(ts_roundtrip(serde_json::json!({"t": null})), None);
        assert_eq!(ts_roundtrip(serde_json::json!({})), None);
    }

    #[test]
    fn lenient_timestamp_degradation_arms_drop_to_none() {
        // Same degrade-per-field contract as the int64s: one bad timestamp
        // must never fail the struct it is embedded in.
        assert_eq!(ts_roundtrip(serde_json::json!({"t": "not-a-time"})), None);
        assert_eq!(ts_roundtrip(serde_json::json!({"t": ""})), None);
        // Epoch-number and object encodings (the realistic divergence
        // shapes) degrade rather than erroring.
        assert_eq!(ts_roundtrip(serde_json::json!({"t": 1754656200})), None);
        assert_eq!(
            ts_roundtrip(serde_json::json!({"t": {"seconds": 1754656200}})),
            None
        );
        assert_eq!(ts_roundtrip(serde_json::json!({"t": true})), None);
    }

    #[test]
    fn serializes_protobuf_json_string_form() {
        assert_eq!(
            serde_json::to_value(Wrapper { v: Some(5) }).unwrap(),
            serde_json::json!({"v": "5"})
        );
        assert_eq!(
            serde_json::to_value(Wrapper { v: None }).unwrap(),
            serde_json::json!({"v": null})
        );
    }
}
