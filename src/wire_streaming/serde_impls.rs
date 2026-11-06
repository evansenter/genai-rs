//! The wire format of [`StreamChunk`] and [`StreamEvent`]: the
//! `chunk_type`/`data` map this crate writes for logging and replay, and
//! reads back.

use serde::{Deserialize, Serialize};

use super::{StreamChunk, StreamEvent};
use crate::response::{InteractionResponse, InteractionStatus};
use crate::steps::{Step, StepDelta};

impl StreamChunk {
    /// Writes the chunk's fields to a serialization map.
    ///
    /// This is used internally by both `StreamChunk::Serialize` and `StreamEvent::Serialize`
    /// to avoid duplicating the match logic.
    pub(super) fn write_to_map<M>(&self, map: &mut M) -> Result<(), M::Error>
    where
        M: serde::ser::SerializeMap,
    {
        match self {
            Self::Created { interaction } => {
                map.serialize_entry("chunk_type", "created")?;
                map.serialize_entry("data", interaction)?;
            }
            Self::StatusUpdate {
                interaction_id,
                status,
                usage,
                continuation_token,
            } => {
                map.serialize_entry("chunk_type", "status_update")?;
                let mut data = serde_json::Map::new();
                if let Some(id) = interaction_id {
                    data.insert("interaction_id".into(), id.clone().into());
                }
                data.insert(
                    "status".into(),
                    serde_json::to_value(status).map_err(serde::ser::Error::custom)?,
                );
                if let Some(usage) = usage {
                    data.insert(
                        "usage".into(),
                        serde_json::to_value(usage).map_err(serde::ser::Error::custom)?,
                    );
                }
                if let Some(token) = continuation_token {
                    data.insert("continuation_token".into(), token.clone().into());
                }
                map.serialize_entry("data", &data)?;
            }
            Self::StepStart { index, step } => {
                map.serialize_entry("chunk_type", "step_start")?;
                map.serialize_entry(
                    "data",
                    &serde_json::json!({
                        "index": index,
                        "step": step,
                    }),
                )?;
            }
            Self::StepDelta {
                index,
                delta,
                total_usage,
            } => {
                map.serialize_entry("chunk_type", "step_delta")?;
                let mut data = serde_json::json!({
                    "index": index,
                    "delta": delta,
                });
                if let Some(usage) = total_usage {
                    data["total_usage"] =
                        serde_json::to_value(usage).map_err(serde::ser::Error::custom)?;
                }
                map.serialize_entry("data", &data)?;
            }
            Self::StepStop {
                index,
                usage,
                step_usage,
            } => {
                map.serialize_entry("chunk_type", "step_stop")?;
                let mut data = serde_json::json!({ "index": index });
                if let Some(u) = usage {
                    data["usage"] = serde_json::to_value(u).map_err(serde::ser::Error::custom)?;
                }
                if let Some(su) = step_usage {
                    data["step_usage"] =
                        serde_json::to_value(su).map_err(serde::ser::Error::custom)?;
                }
                map.serialize_entry("data", &data)?;
            }
            Self::Completed(response) => {
                map.serialize_entry("chunk_type", "completed")?;
                map.serialize_entry("data", response)?;
            }
            Self::Error { message, code } => {
                map.serialize_entry("chunk_type", "error")?;
                map.serialize_entry(
                    "data",
                    &serde_json::json!({
                        "message": message,
                        "code": code,
                    }),
                )?;
            }
            Self::Unknown { chunk_type, data } => {
                map.serialize_entry("chunk_type", chunk_type)?;
                // An unknown type spelled like a known `chunk_type` (a wire
                // event named `created`, say) would read back as that known
                // chunk; the marker keeps it unknown. Found by fuzzing.
                if KNOWN_CHUNK_TYPES.contains(&chunk_type.as_str()) {
                    map.serialize_entry("unknown", &true)?;
                }
                if !data.is_null() {
                    map.serialize_entry("data", data)?;
                }
            }
        }
        Ok(())
    }
}

/// The `chunk_type` values the known [`StreamChunk`] variants serialize as.
pub(super) const KNOWN_CHUNK_TYPES: [&str; 7] = [
    "created",
    "status_update",
    "step_start",
    "step_delta",
    "step_stop",
    "completed",
    "error",
];

impl Serialize for StreamChunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        self.write_to_map(&mut map)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for StreamChunk {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        let chunk_type = match value.get("chunk_type") {
            Some(serde_json::Value::String(s)) => s.as_str(),
            Some(other) => {
                tracing::warn!(
                    "StreamChunk received non-string chunk_type: {}. \
                     This may indicate a malformed payload.",
                    other
                );
                "<non-string chunk_type>"
            }
            None => {
                tracing::warn!(
                    "StreamChunk is missing required chunk_type field. \
                     This may indicate a malformed payload."
                );
                "<missing chunk_type>"
            }
        };

        fn data_of(value: &serde_json::Value) -> serde_json::Value {
            value
                .get("data")
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        }

        fn index_of(data: &serde_json::Value) -> usize {
            data.get("index")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or_else(|| {
                    tracing::warn!(
                        "StreamChunk step event missing index. Defaulting to 0. \
                         This may indicate a malformed payload."
                    );
                    0
                })
        }

        // Written by an `Unknown` whose type collides with a known one.
        let marked_unknown = value.get("unknown") == Some(&serde_json::Value::Bool(true));

        match chunk_type {
            chunk_type if marked_unknown => Ok(Self::Unknown {
                chunk_type: chunk_type.to_string(),
                data: data_of(&value),
            }),
            "created" => {
                let interaction: Box<InteractionResponse> = serde_json::from_value(data_of(&value))
                    .map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize StreamChunk::Created data: {}",
                            e
                        ))
                    })?;
                Ok(Self::Created { interaction })
            }
            "status_update" => {
                let data = data_of(&value);
                let interaction_id = data
                    .get("interaction_id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let usage = data
                    .get("usage")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize StreamChunk::StatusUpdate usage: {e}"
                        ))
                    })?;
                let continuation_token = data
                    .get("continuation_token")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let status: InteractionStatus = data
                    .get("status")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize StreamChunk::StatusUpdate status: {}",
                            e
                        ))
                    })?
                    .unwrap_or_else(|| {
                        tracing::warn!("StreamChunk::StatusUpdate is missing status.");
                        InteractionStatus::InProgress
                    });
                Ok(Self::StatusUpdate {
                    interaction_id,
                    status,
                    usage,
                    continuation_token,
                })
            }
            "step_start" => {
                let data = data_of(&value);
                let index = index_of(&data);
                let step: Step = serde_json::from_value(
                    data.get("step").cloned().unwrap_or(serde_json::Value::Null),
                )
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize StreamChunk::StepStart step: {}",
                        e
                    ))
                })?;
                Ok(Self::StepStart { index, step })
            }
            "step_delta" => {
                let data = data_of(&value);
                let index = index_of(&data);
                let delta: StepDelta = serde_json::from_value(
                    data.get("delta")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize StreamChunk::StepDelta delta: {}",
                        e
                    ))
                })?;
                let total_usage = data
                    .get("total_usage")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::StepDelta {
                    index,
                    delta,
                    total_usage,
                })
            }
            "step_stop" => {
                let data = data_of(&value);
                let index = index_of(&data);
                let usage = data
                    .get("usage")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                let step_usage = data
                    .get("step_usage")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(serde::de::Error::custom)?;
                Ok(Self::StepStop {
                    index,
                    usage,
                    step_usage,
                })
            }
            "completed" => {
                let response: Box<InteractionResponse> = serde_json::from_value(data_of(&value))
                    .map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize StreamChunk::Completed data: {}",
                            e
                        ))
                    })?;
                Ok(Self::Completed(response))
            }
            "error" => {
                let data = data_of(&value);
                let message = data
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        tracing::warn!("StreamChunk::Error is missing message.");
                        "Unknown error".to_string()
                    });
                let code = data.get("code").and_then(|v| v.as_str()).map(String::from);
                Ok(Self::Error { message, code })
            }
            other => {
                tracing::warn!(
                    "Encountered unknown StreamChunk type '{}'. \
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

impl Serialize for StreamEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        self.chunk.write_to_map(&mut map)?;

        if let Some(event_id) = &self.event_id {
            map.serialize_entry("event_id", event_id)?;
        }

        map.end()
    }
}

impl<'de> Deserialize<'de> for StreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        // Extract event_id first
        let event_id = value
            .get("event_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Deserialize the chunk from the same value
        let chunk: StreamChunk = serde_json::from_value(value).map_err(serde::de::Error::custom)?;

        Ok(Self { chunk, event_id })
    }
}
