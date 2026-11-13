//! The wire-faithfulness half of the fuzz round trip (D-NEXT-J7): a type
//! that promises to write back the JSON it read is checked against its
//! input, not only against its own later writes, so a value lost on the
//! first read is caught too.
//!
//! Both sides go through [`canonical`], the forms every type treats as one,
//! and the input through the type's own documented normalizations
//! ([`super::WireForm::faithful_wire`]).

use serde_json::{Map, Value};

use crate::{Agent, EnvironmentSpec, NetworkConfig, RemoteEnvironment, Tool};

/// The keys of the `f32` fields: `GenerationConfig`'s `temperature` and
/// `top_p`, and the retrieval tool's hybrid-search `alpha`.
const F32_KEYS: &[&str] = &["temperature", "top_p", "alpha"];

/// `value` in the form a comparison of written JSON with its input uses:
///
/// - A member that is `null`, `[]` or `{}` is left out: an `Option` reads
///   `null` as absent, and a field skips an empty collection on write.
/// - A number that is a whole `f64` in `i64`'s range is that integer: an
///   `f64` field writes `1` as `1.0`.
/// - A number under an `f32` field's key is what that field holds: rounded
///   to an `f32`, or left out beyond its range.
/// - With `proto3`, for the Antigravity harness's protobuf JSON: a string
///   that spells an `i64` or `u64` is that number (protobuf JSON writes an
///   int64 as a string, and the harness types read either and write the
///   number), and a member at its scalar default (`""`, `0`, `false`) is
///   left out, as proto3 has no presence for it.
pub(super) fn canonical(value: &Value, proto3: bool) -> Value {
    canonical_member(None, value, proto3)
}

fn canonical_member(key: Option<&str>, value: &Value, proto3: bool) -> Value {
    match value {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .map(|(key, value)| (key.clone(), canonical_member(Some(key), value, proto3)))
                .filter(|(_, value)| !is_empty(value) && !(proto3 && is_scalar_default(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| canonical_member(None, item, proto3))
                .collect(),
        ),
        Value::Number(number) if key.is_some_and(|key| F32_KEYS.contains(&key)) => {
            number.as_f64().map_or(Value::Null, |wide| {
                // What the field holds: `as` rounds to the nearest `f32`.
                let narrow = wide as f32;
                if narrow.is_finite() {
                    canonical_number(f64::from(narrow))
                } else {
                    Value::Null
                }
            })
        }
        Value::Number(number) => match number.as_f64() {
            Some(float) if number.is_f64() => canonical_number(float),
            _ => value.clone(),
        },
        Value::String(text) if proto3 => text
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| text.parse::<u64>().map(Value::from))
            .unwrap_or_else(|_| value.clone()),
        _ => value.clone(),
    }
}

/// A float as the integer it is, when it is a whole number in `i64`'s
/// range.
fn canonical_number(float: f64) -> Value {
    // `i64::MAX as f64` rounds up to 2^63, which is out of range: `<` keeps
    // the cast below exact.
    if float.fract() == 0.0 && float.abs() < i64::MAX as f64 {
        Value::from(float as i64)
    } else {
        Value::from(float)
    }
}

/// A proto3 scalar at its default value.
fn is_scalar_default(value: &Value) -> bool {
    match value {
        Value::String(text) => text.is_empty(),
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::Bool(flag) => !flag,
        _ => false,
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(members) => members.is_empty(),
        _ => false,
    }
}

/// What an [`EnvironmentSpec`] read as `spec` from `input` writes back: an
/// ID or an unknown spec writes what it read, and a remote one is
/// [`remote_environment`].
pub(super) fn environment_spec(spec: &EnvironmentSpec, input: &Value) -> Value {
    match spec {
        EnvironmentSpec::Remote(environment) => remote_environment(environment, input),
        _ => input.clone(),
    }
}

/// What a [`RemoteEnvironment`] writes back for `input`, its documented
/// normalizations applied: the `type` is `"remote"` (a missing or null one
/// included), an `env` echoed as a list of single-key maps is the map a
/// request sends, and its `network` is [`network_config`].
fn remote_environment(environment: &RemoteEnvironment, input: &Value) -> Value {
    let mut object = input.as_object().cloned().unwrap_or_default();
    // Only these read as remote; any other `type` must be written back as
    // it came, which is what the comparison then checks.
    if object.get("type").is_none_or(serde_json::Value::is_null) {
        object.insert("type".into(), "remote".into());
    }
    // An `env` it could not read is kept in `extra`, as it came.
    if !environment.extra.contains_key("env")
        && let Some(Value::Array(items)) = object.get("env")
    {
        let merged: Map<String, Value> = items
            .iter()
            .filter_map(Value::as_object)
            .flatten()
            .map(|(name, var)| (name.clone(), var.clone()))
            .collect();
        object.insert("env".into(), Value::Object(merged));
    }
    if let (Some(network), Some(value)) = (&environment.network, object.get_mut("network")) {
        *value = network_config(network, value);
    }
    Value::Object(object)
}

/// What a [`NetworkConfig`] read as `network` from `input` writes back: the
/// disabled allowlist's echo goes out in the form a request sends, and an
/// allowlist entry's empty `domain` is not written.
pub(super) fn network_config(network: &NetworkConfig, input: &Value) -> Value {
    match network {
        NetworkConfig::AllowlistDisabled => serde_json::json!({"allowlist": "disabled"}),
        NetworkConfig::Allowlist { .. } => {
            let mut value = input.clone();
            if let Some(Value::Array(entries)) = value.get_mut("allowlist") {
                for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
                    if entry.get("domain").is_some_and(|domain| domain == "") {
                        entry.remove("domain");
                    }
                }
            }
            value
        }
        _ => input.clone(),
    }
}

/// What an [`Agent`] read from `input` writes back: its
/// `base_environment` as [`environment_spec`] writes it, and an unknown
/// tool with its type tag merged in, as every unknown tagged union does
/// ([`super::unknown_read_back`]).
pub(super) fn agent(agent: &Agent, input: &Value) -> Value {
    let mut object = input.as_object().cloned().unwrap_or_default();
    if let (Some(spec), Some(value)) = (&agent.base_environment, object.get_mut("base_environment"))
    {
        *value = environment_spec(spec, value);
    }
    if let (Some(tools), Some(Value::Array(values))) = (&agent.tools, object.get_mut("tools")) {
        for (tool, value) in tools.iter().zip(values) {
            if let Tool::Unknown { tool_type, .. } = tool {
                super::unknown_read_back(tool_type, value);
            }
        }
    }
    Value::Object(object)
}

/// What an Antigravity `OutputEvent` read from `input` writes back: with
/// both spellings of its usage, the flat `usageMetadata` is the total of
/// the `usageUpdate` it is written as.
#[cfg(feature = "antigravity")]
pub(super) fn output_event(input: &Value) -> Value {
    let mut object = input.as_object().cloned().unwrap_or_default();
    let flat = object
        .get("usageMetadata")
        .filter(|flat| !flat.is_null())
        .cloned();
    if let (Some(flat), Some(Value::Object(update))) = (flat, object.get_mut("usageUpdate")) {
        update.insert("total".into(), flat);
        object.remove("usageMetadata");
    }
    Value::Object(object)
}
