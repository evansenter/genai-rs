//! The JSON form of [`AutoFunctionStreamChunk`] and [`AutoFunctionStreamEvent`]:
//! a `chunk_type` tag with its payload under `data`, and an event's `chunk`
//! beside its optional `event_id`.

use serde::{Deserialize, Serialize};

use super::{AutoFunctionStreamChunk, AutoFunctionStreamEvent, FunctionExecutionResult};
use crate::{InteractionResponse, StepDelta};

impl Serialize for AutoFunctionStreamChunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Delta(content) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "delta")?;
                map.serialize_entry("data", content)?;
                map.end()
            }
            Self::ExecutingFunctions {
                response,
                pending_calls,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "executing_functions")?;
                // Serialize as nested object with both fields
                let data = serde_json::json!({
                    "response": response,
                    "pending_calls": pending_calls,
                });
                map.serialize_entry("data", &data)?;
                map.end()
            }
            Self::FunctionResults(results) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "function_results")?;
                map.serialize_entry("data", results)?;
                map.end()
            }
            Self::Complete(response) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "complete")?;
                map.serialize_entry("data", response)?;
                map.end()
            }
            Self::MaxLoopsReached(response) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "max_loops_reached")?;
                map.serialize_entry("data", response)?;
                map.end()
            }
            Self::Unknown { chunk_type, data } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", chunk_type)?;
                if !data.is_null() {
                    map.serialize_entry("data", data)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for AutoFunctionStreamChunk {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        // Helper to extract "data" field with consistent warning for missing data
        fn extract_data_field(value: &serde_json::Value, variant_name: &str) -> serde_json::Value {
            match value.get("data").cloned() {
                Some(d) => d,
                None => {
                    tracing::warn!(
                        "AutoFunctionStreamChunk::{} is missing the 'data' field. \
                         This may indicate a malformed API response.",
                        variant_name
                    );
                    serde_json::Value::Null
                }
            }
        }

        let chunk_type = match value.get("chunk_type") {
            Some(serde_json::Value::String(s)) => s.as_str(),
            Some(other) => {
                tracing::warn!(
                    "AutoFunctionStreamChunk received non-string chunk_type: {}. \
                     This may indicate a malformed API response.",
                    other
                );
                "<non-string chunk_type>"
            }
            None => {
                tracing::warn!(
                    "AutoFunctionStreamChunk is missing required chunk_type field. \
                     This may indicate a malformed API response."
                );
                "<missing chunk_type>"
            }
        };

        match chunk_type {
            "delta" => {
                let data = extract_data_field(&value, "Delta");
                let delta: StepDelta = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize AutoFunctionStreamChunk::Delta data: {}",
                        e
                    ))
                })?;
                Ok(Self::Delta(delta))
            }
            "executing_functions" => {
                let data = extract_data_field(&value, "ExecutingFunctions");

                let response = serde_json::from_value(
                    data.get("response")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize ExecutingFunctions response: {}",
                        e
                    ))
                })?;
                let pending_calls = serde_json::from_value(
                    data.get("pending_calls")
                        .cloned()
                        .unwrap_or(serde_json::json!([])),
                )
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize ExecutingFunctions pending_calls: {}",
                        e
                    ))
                })?;

                Ok(Self::ExecutingFunctions {
                    response,
                    pending_calls,
                })
            }
            "function_results" => {
                let data = extract_data_field(&value, "FunctionResults");
                let results: Vec<FunctionExecutionResult> =
                    serde_json::from_value(data).map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize AutoFunctionStreamChunk::FunctionResults data: {}",
                            e
                        ))
                    })?;
                Ok(Self::FunctionResults(results))
            }
            "complete" => {
                let data = extract_data_field(&value, "Complete");
                let response: Box<InteractionResponse> =
                    serde_json::from_value(data).map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize AutoFunctionStreamChunk::Complete data: {}",
                            e
                        ))
                    })?;
                Ok(Self::Complete(response))
            }
            "max_loops_reached" => {
                let data = extract_data_field(&value, "MaxLoopsReached");
                let response: Box<InteractionResponse> = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize AutoFunctionStreamChunk::MaxLoopsReached data: {}",
                        e
                    ))
                })?;
                Ok(Self::MaxLoopsReached(response))
            }
            other => {
                tracing::warn!(
                    "Encountered unknown AutoFunctionStreamChunk type '{}'. \
                     This may indicate a new API feature. \
                     The chunk will be preserved in the Unknown variant.",
                    other
                );
                let data = value
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                Ok(Self::Unknown {
                    chunk_type: other.to_string(),
                    data,
                })
            }
        }
    }
}

impl Serialize for AutoFunctionStreamEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("chunk", &self.chunk)?;
        if let Some(id) = &self.event_id {
            map.serialize_entry("event_id", id)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for AutoFunctionStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        let chunk = match value.get("chunk") {
            Some(chunk_value) => {
                serde_json::from_value(chunk_value.clone()).map_err(serde::de::Error::custom)?
            }
            None => {
                return Err(serde::de::Error::missing_field("chunk"));
            }
        };

        let event_id = value
            .get("event_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(Self { chunk, event_id })
    }
}

#[cfg(test)]
#[path = "serde_impls_tests.rs"]
mod tests;
