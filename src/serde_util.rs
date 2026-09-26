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
//!
//! It also holds the `skip_serializing_if` predicate and the `Unknown`
//! variant serializers that let the Evergreen tagged unions
//! `#[derive(Serialize)]` instead of hand-writing a map per variant.

use serde::de::Deserializer;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

/// Names the resource a lenient helper is deserializing, so its `warn!`
/// lines say *which* field family degraded — the analogue of the vec
/// helper's `type_name`. A field name cannot be threaded through
/// `deserialize_with`, so the resource is the available granularity;
/// call sites select a marker with turbofish in the serde attribute.
pub(crate) trait ResourceName {
    const NAME: &'static str;
}

macro_rules! resource_markers {
    ($($ty:ident => $name:literal),* $(,)?) => {
        $(pub(crate) struct $ty;
        impl ResourceName for $ty {
            const NAME: &'static str = $name;
        })*
    };
}

resource_markers! {
    ForTrigger => "Trigger",
    ForTriggerExecution => "TriggerExecution",
    ForEnvironment => "Environment",
    ForWebhook => "Webhook",
    ForSigningSecret => "SigningSecret",
    ForFileSearchDocument => "FileSearchDocument",
}

#[cfg(test)]
resource_markers! {
    ForTest => "TestWrapper",
}

/// `skip_serializing_if` predicate: true for `None` *and* for `Some` of an
/// empty collection, so an optional list or map the caller set to empty is
/// omitted from the wire rather than sent as `[]` or `{}`.
///
/// Works for any `Option<C>` whose `&C` iterates: `Vec`, `HashMap`,
/// `BTreeMap`, and so on.
#[allow(clippy::ref_option)] // signature dictated by serde's skip_serializing_if
pub(crate) fn is_none_or_empty<T>(value: &Option<T>) -> bool
where
    for<'a> &'a T: IntoIterator,
{
    value
        .as_ref()
        .is_none_or(|collection| collection.into_iter().next().is_none())
}

/// Variant-level `serialize_with` for an `Unknown { <context>_type, data }`
/// variant whose wire form re-merges the type into the object: `"type"`
/// first (from `type_name`, overriding any `"type"` inside `data`), then
/// every other entry of an object `data` in its map order. Non-object data
/// is written under a `"data"` key; null data is omitted, leaving just the
/// type.
///
/// serde passes a struct variant's fields to a variant-level
/// `serialize_with` as one reference argument each, in declaration order,
/// so the variant must declare the type string before `data`. (The first
/// parameter is `&str`; serde's `&String` argument deref-coerces to it.)
pub(crate) fn serialize_unknown_merged<S>(
    type_name: &str,
    data: &serde_json::Value,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(None)?;
    map.serialize_entry("type", type_name)?;
    // Flatten the data fields into the map if it's an object
    if let serde_json::Value::Object(obj) = data {
        for (key, value) in obj {
            if key != "type" {
                map.serialize_entry(key, value)?;
            }
        }
    } else if !data.is_null() {
        map.serialize_entry("data", data)?;
    }
    map.end()
}

/// Variant-level `serialize_with` for an `Unknown { <context>_type, data }`
/// variant whose wire form is `data` exactly as captured; the type string is
/// ignored (it was read out of `data` on deserialize). Same argument order
/// as [`serialize_unknown_merged`].
pub(crate) fn serialize_unknown_data<S>(
    _type_name: &str,
    data: &serde_json::Value,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    data.serialize(serializer)
}

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
pub(crate) fn deserialize_string_i64<'de, D, R>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
    R: ResourceName,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        // A JSON null arrives as `None` (serde routes it through
        // `visit_none`; see `deserialize_lenient_vec`) — the second half
        // of this pattern is belt-and-braces, not a reachable arm. Same
        // for the two siblings below.
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => {
            let parsed = n.as_i64();
            if parsed.is_none() {
                tracing::warn!("Non-i64 JSON number on {}, dropping: {n}", R::NAME);
            }
            Ok(parsed)
        }
        Some(serde_json::Value::String(s)) => {
            let parsed = s.parse().ok();
            if parsed.is_none() {
                tracing::warn!(
                    "Unparseable int64 string on {} from API, dropping: {s:?}",
                    R::NAME
                );
            }
            Ok(parsed)
        }
        Some(other) => {
            tracing::warn!(
                "Unexpected JSON type for int64 field on {}, dropping: {other:?}",
                R::NAME
            );
            Ok(None)
        }
    }
}

/// Deserializes an optional int64 accepting both the plain-number and
/// protobuf-JSON string forms, but *erroring* on anything malformed — the
/// send-side sibling of [`deserialize_string_i64`]. For deserializable
/// request types (config-file loading), where a typo must stay a clean
/// load-time error: there is no page to protect on the send side, and a
/// silently dropped field would change what gets created.
pub(crate) fn deserialize_strict_string_i64<'de, D>(
    deserializer: D,
) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| D::Error::custom(format!("non-i64 JSON number for int64 field: {n}"))),
        Some(serde_json::Value::String(s)) => s
            .parse()
            .map(Some)
            .map_err(|e| D::Error::custom(format!("unparseable int64 string {s:?}: {e}"))),
        Some(other) => Err(D::Error::custom(format!(
            "unexpected JSON type for int64 field: {other}"
        ))),
    }
}

/// Deserializes an optional RFC 3339 timestamp, degrading an unparseable
/// string or unexpected JSON shape to `None` with a `warn!` instead of
/// failing the enclosing struct (and hence an entire list response).
///
/// Used on the trigger resource family, whose wire shape is unverified
/// (protobuf-JSON specifies RFC 3339 strings for `Timestamp`, but this
/// family already diverged from expectations once — int64s arrive as
/// strings); on `Environment` for uniformity with the int64 helpers next
/// door even though its timestamp encoding was live-verified; and on
/// `Webhook`/`SigningSecret` so a divergent timestamp encoding costs one
/// field rather than dropping the whole webhook from a listed page (the
/// element-drop arm of `deserialize_lenient_vec` would otherwise be the
/// thing that caught it).
pub(crate) fn deserialize_lenient_timestamp<'de, D, R>(
    deserializer: D,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, D::Error>
where
    D: Deserializer<'de>,
    R: ResourceName,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => {
            let parsed = chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok();
            if parsed.is_none() {
                tracing::warn!(
                    "Unparseable RFC 3339 timestamp on {} from API, dropping: {s:?}",
                    R::NAME
                );
            }
            Ok(parsed)
        }
        Some(other) => {
            tracing::warn!(
                "Unexpected JSON type for timestamp field on {}, dropping: {other:?}",
                R::NAME
            );
            Ok(None)
        }
    }
}

/// Deserializes a list field leniently: an explicit null or non-array
/// shape degrades to an empty vec, and an undeserializable *element*
/// drops alone — each with a `warn!` — instead of failing the enclosing
/// envelope.
///
/// Used by every resource list envelope, so one malformed resource costs
/// itself rather than the whole page. Lists *inside* a resource stay
/// strict.
pub(crate) fn deserialize_lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        // This is the explicit-null arm: serde maps a JSON null to `None`
        // via `deserialize_option`/`visit_none`, so `Some(Value::Null)`
        // can never be produced here. A merely-absent key also cannot
        // reach this helper — every call site carries a struct-level
        // serde default, which fills a missing key without invoking
        // `deserialize_with`. The element type is the only discriminator
        // available to say *which* envelope's page came back empty.
        None => {
            tracing::warn!(
                "List field of {} was explicit null; degrading to an empty list",
                std::any::type_name::<T>()
            );
            Ok(Vec::new())
        }
        Some(serde_json::Value::Array(items)) => Ok(items
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| {
                serde_json::from_value(item)
                    .map_err(|e| {
                        // The index makes the warn actionable — it names
                        // *which* element of the page vanished, and this
                        // warn is the only signal one did (the caller
                        // just sees a shorter list).
                        tracing::warn!(
                            "Undeserializable {} list element at index {index}, dropping: {e}",
                            std::any::type_name::<T>()
                        );
                    })
                    .ok()
            })
            .collect()),
        Some(other) => {
            tracing::warn!(
                "Unexpected JSON type for {} list field, degrading to empty: {other:?}",
                std::any::type_name::<T>()
            );
            Ok(Vec::new())
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
            deserialize_with = "super::deserialize_string_i64::<_, super::ForTest>"
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

    /// The send-side sibling of `Wrapper`: same accepted wire forms, but
    /// malformed values error instead of degrading.
    #[derive(Debug, PartialEq, Deserialize)]
    struct StrictWrapper {
        #[serde(default, deserialize_with = "super::deserialize_strict_string_i64")]
        v: Option<i64>,
    }

    #[test]
    fn strict_string_i64_accepts_both_forms_and_absence() {
        let parse = |json| serde_json::from_value::<StrictWrapper>(json).map(|w| w.v);
        assert_eq!(parse(serde_json::json!({"v": "42"})).unwrap(), Some(42));
        assert_eq!(parse(serde_json::json!({"v": 42})).unwrap(), Some(42));
        // The two arms that look malformed but must NOT error: an unset
        // optional is not a typo.
        assert_eq!(parse(serde_json::json!({"v": null})).unwrap(), None);
        assert_eq!(parse(serde_json::json!({})).unwrap(), None);
    }

    #[test]
    fn strict_string_i64_errors_on_malformed_values() {
        // The send-side contract, opposite of `Wrapper`'s: there is no
        // page to protect, so garbage is a load-time error rather than a
        // silently unset field.
        let parse = |json| serde_json::from_value::<StrictWrapper>(json);
        assert!(parse(serde_json::json!({"v": "three"})).is_err());
        assert!(parse(serde_json::json!({"v": ""})).is_err());
        assert!(parse(serde_json::json!({"v": 1.5})).is_err());
        assert!(parse(serde_json::json!({"v": true})).is_err());
        assert!(parse(serde_json::json!({"v": [1]})).is_err());
        assert!(parse(serde_json::json!({"v": {"n": 1}})).is_err());
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct TsWrapper {
        #[serde(
            default,
            deserialize_with = "super::deserialize_lenient_timestamp::<_, super::ForTest>"
        )]
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

    #[derive(Debug, PartialEq, Deserialize)]
    struct VecWrapper {
        #[serde(default, deserialize_with = "super::deserialize_lenient_vec")]
        v: Vec<i64>,
    }

    fn vec_roundtrip(json: serde_json::Value) -> Vec<i64> {
        serde_json::from_value::<VecWrapper>(json).unwrap().v
    }

    #[test]
    fn lenient_vec_happy_paths() {
        assert_eq!(vec_roundtrip(serde_json::json!({"v": [1, 2]})), vec![1, 2]);
        assert_eq!(
            vec_roundtrip(serde_json::json!({"v": []})),
            Vec::<i64>::new()
        );
        // Key-absent rides the field-level serde default, not the helper.
        assert_eq!(vec_roundtrip(serde_json::json!({})), Vec::<i64>::new());
    }

    #[test]
    fn lenient_vec_degradation_arms_drop_to_empty() {
        // Same degrade-per-field contract as the siblings above: a null or
        // malformed list key must never fail the envelope it is embedded in.
        assert_eq!(
            vec_roundtrip(serde_json::json!({"v": null})),
            Vec::<i64>::new()
        );
        assert_eq!(
            vec_roundtrip(serde_json::json!({"v": "x"})),
            Vec::<i64>::new()
        );
        assert_eq!(
            vec_roundtrip(serde_json::json!({"v": 5})),
            Vec::<i64>::new()
        );
        assert_eq!(
            vec_roundtrip(serde_json::json!({"v": {"a": 1}})),
            Vec::<i64>::new()
        );
        // Degradation is per-element: one bad element drops alone, and the
        // good entries of the page survive.
        assert_eq!(
            vec_roundtrip(serde_json::json!({"v": [1, "x", 2]})),
            vec![1, 2]
        );
    }

    #[test]
    fn lenient_vec_warns_are_not_silent() {
        // The warns are load-bearing, and value assertions cannot pin
        // them: an explicit-null page and a genuinely empty account both
        // read as an empty vec, and a page that silently lost an element
        // reads like a shorter page. Assert on the log itself. (serde
        // maps JSON null to `None` — never `Some(Value::Null)` — so this
        // also pins that the arm that fires is the one that logs.)
        let messages = crate::test_subscriber::capture_messages(|| {
            assert_eq!(
                vec_roundtrip(serde_json::json!({"v": null})),
                Vec::<i64>::new()
            );
            assert_eq!(
                vec_roundtrip(serde_json::json!({"v": [1, "x", 2]})),
                vec![1, 2]
            );
        });
        assert!(
            messages.iter().any(|m| m.contains("explicit null")),
            "the explicit-null degradation must warn; got: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("dropping")),
            "the element-drop degradation must warn; got: {messages:?}"
        );
    }

    #[test]
    fn is_none_or_empty_skips_none_and_empty_collections() {
        use std::collections::{BTreeMap, HashMap};

        assert!(super::is_none_or_empty::<Vec<i32>>(&None));
        assert!(super::is_none_or_empty::<Vec<i32>>(&Some(vec![])));
        assert!(!super::is_none_or_empty(&Some(vec![1])));

        assert!(super::is_none_or_empty::<HashMap<String, String>>(&None));
        assert!(super::is_none_or_empty(&Some(
            HashMap::<String, String>::new()
        )));
        assert!(!super::is_none_or_empty(&Some(HashMap::from([(
            "k".to_string(),
            "v".to_string()
        )]))));

        assert!(super::is_none_or_empty(&Some(
            BTreeMap::<String, i32>::new()
        )));
        assert!(!super::is_none_or_empty(&Some(BTreeMap::from([(
            "k".to_string(),
            1
        )]))));
    }

    /// The shape the `Unknown` serializers are written for: an internally
    /// tagged enum whose last variant is untagged with a variant-level
    /// `serialize_with`. Going through the derive (rather than calling the
    /// helpers directly) pins the argument order serde passes.
    #[derive(Serialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Merged {
        Known {
            x: i32,
        },
        #[serde(untagged, serialize_with = "super::serialize_unknown_merged")]
        Unknown {
            thing_type: String,
            data: serde_json::Value,
        },
    }

    #[derive(Serialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Verbatim {
        Known {
            x: i32,
        },
        #[serde(untagged, serialize_with = "super::serialize_unknown_data")]
        Unknown {
            thing_type: String,
            data: serde_json::Value,
        },
    }

    fn merged(data: serde_json::Value) -> String {
        serde_json::to_string(&Merged::Unknown {
            thing_type: "zz_new".to_string(),
            data,
        })
        .unwrap()
    }

    fn verbatim(data: serde_json::Value) -> String {
        serde_json::to_string(&Verbatim::Unknown {
            thing_type: "zz_new".to_string(),
            data,
        })
        .unwrap()
    }

    #[test]
    fn unknown_merged_writes_type_first_then_data_entries() {
        assert_eq!(
            serde_json::to_string(&Merged::Known { x: 1 }).unwrap(),
            r#"{"type":"known","x":1}"#
        );
        // The type string wins over a "type" inside data, and comes first;
        // the remaining entries follow in the data map's (sorted) order.
        assert_eq!(
            merged(serde_json::json!({"b": 1, "type": "stale", "a": [true]})),
            r#"{"type":"zz_new","a":[true],"b":1}"#
        );
        // Data without a "type" key still goes out with one.
        assert_eq!(
            merged(serde_json::json!({"a": 1})),
            r#"{"type":"zz_new","a":1}"#
        );
        assert_eq!(merged(serde_json::json!({})), r#"{"type":"zz_new"}"#);
        // Non-object data nests under "data"; null data is omitted.
        assert_eq!(
            merged(serde_json::json!("raw")),
            r#"{"type":"zz_new","data":"raw"}"#
        );
        assert_eq!(
            merged(serde_json::json!([1, 2])),
            r#"{"type":"zz_new","data":[1,2]}"#
        );
        assert_eq!(
            merged(serde_json::json!(7)),
            r#"{"type":"zz_new","data":7}"#
        );
        assert_eq!(merged(serde_json::Value::Null), r#"{"type":"zz_new"}"#);
    }

    #[test]
    fn unknown_data_writes_data_verbatim() {
        assert_eq!(
            serde_json::to_string(&Verbatim::Known { x: 1 }).unwrap(),
            r#"{"type":"known","x":1}"#
        );
        // The type string is ignored: data goes out exactly as captured.
        assert_eq!(
            verbatim(serde_json::json!({"b": 1, "type": "stale", "a": 2})),
            r#"{"a":2,"b":1,"type":"stale"}"#
        );
        assert_eq!(verbatim(serde_json::json!({"a": 1})), r#"{"a":1}"#);
        assert_eq!(verbatim(serde_json::json!("raw")), r#""raw""#);
        assert_eq!(verbatim(serde_json::Value::Null), "null");
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
