//! Streaming types for SSE responses (API revision 2026-05-20).

use serde::{Deserialize, Serialize};

use crate::response::{InteractionResponse, InteractionStatus, UsageMetadata};
use crate::steps::{Step, StepDelta};

/// A chunk from the streaming API.
///
/// Under API revision 2026-05-20 the server emits this lifecycle:
/// - `interaction.created` → [`StreamChunk::Created`] (first event, contains ID)
/// - `interaction.status_update` → [`StreamChunk::StatusUpdate`]
/// - `step.start` → [`StreamChunk::StepStart`] (a step begins at an index)
/// - `step.delta` → [`StreamChunk::StepDelta`] (incremental step payload)
/// - `step.stop` → [`StreamChunk::StepStop`] (step finished, per-step usage)
/// - `interaction.completed` → [`StreamChunk::Completed`] (terminal)
/// - `error` → [`StreamChunk::Error`] (terminal)
///
/// All variants implement `Serialize` and `Deserialize` for logging,
/// persistence, and replay of streaming events.
///
/// # Forward Compatibility
///
/// This enum uses `#[non_exhaustive]` to allow adding new chunk types in future
/// versions without breaking existing code. Always include a wildcard arm in
/// match statements. Unknown chunk types deserialize to the `Unknown` variant
/// with their data preserved.
#[derive(Clone, Debug)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum StreamChunk {
    /// Interaction created (first event, contains ID).
    ///
    /// Sent when the interaction is accepted by the API. Provides early access
    /// to the interaction ID before any content is generated. The payload is a
    /// partial interaction resource (some fields present only on the final
    /// response may be absent).
    Created {
        /// The (partial) interaction resource at creation time
        interaction: InteractionResponse,
    },

    /// Status update for in-progress interaction.
    ///
    /// Sent when the interaction status changes during processing.
    /// Useful for tracking progress of background/agent interactions.
    StatusUpdate {
        /// The interaction ID
        interaction_id: String,
        /// The updated status
        status: InteractionStatus,
    },

    /// A step started at the given index.
    StepStart {
        /// Position index for this step
        index: usize,
        /// The step being started (may be partially populated; deltas fill it in)
        step: Step,
    },

    /// Incremental payload for the step at the given index.
    StepDelta {
        /// Position index for the step this delta belongs to
        index: usize,
        /// The delta payload (text fragment, arguments_delta, thought
        /// signature, tool call/result, ...)
        delta: StepDelta,
    },

    /// The step at the given index finished.
    StepStop {
        /// Position index for the completed step
        index: usize,
        /// Cumulative interaction usage, if reported
        usage: Option<UsageMetadata>,
        /// Usage attributable to this step, if reported
        step_usage: Option<UsageMetadata>,
    },

    /// Complete interaction response (terminal event).
    Completed(InteractionResponse),

    /// Error occurred during streaming (terminal event).
    Error {
        /// Human-readable error message
        message: String,
        /// Error code from the API (if provided)
        code: Option<String>,
    },

    /// Unknown chunk type (for forward compatibility).
    ///
    /// Used when the server emits an SSE event type this library doesn't
    /// recognize. The `chunk_type` field contains the unrecognized event type
    /// string, and `data` contains the full event JSON for inspection.
    Unknown {
        /// The unrecognized chunk/event type from the API
        chunk_type: String,
        /// The raw JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl StreamChunk {
    /// Check if this is an unknown chunk type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the chunk type name if this is an unknown chunk type.
    ///
    /// Returns `None` for known chunk types.
    #[must_use]
    pub fn unknown_chunk_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { chunk_type, .. } => Some(chunk_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown chunk type.
    ///
    /// Returns `None` for known chunk types.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Returns the interaction ID if this event contains one.
    ///
    /// Available for `Created`, `StatusUpdate`, and `Completed` variants.
    #[must_use]
    pub fn interaction_id(&self) -> Option<&str> {
        match self {
            Self::Created { interaction } => interaction.id.as_deref(),
            Self::StatusUpdate { interaction_id, .. } => Some(interaction_id),
            Self::Completed(response) => response.id.as_deref(),
            _ => None,
        }
    }

    /// Returns true if this is a terminal event.
    ///
    /// Terminal events indicate the stream has ended (either successfully or with an error).
    /// After receiving a terminal event, no more events will be sent.
    ///
    /// Terminal events are:
    /// - `Completed`: Successful completion
    /// - `Error`: Error occurred
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Error { .. })
    }

    /// Returns the status if this event contains one.
    ///
    /// Available for `Created`, `StatusUpdate`, and `Completed` variants.
    #[must_use]
    pub fn status(&self) -> Option<&InteractionStatus> {
        match self {
            Self::Created { interaction } => Some(&interaction.status),
            Self::StatusUpdate { status, .. } => Some(status),
            Self::Completed(response) => Some(&response.status),
            _ => None,
        }
    }

    /// Returns the delta text fragment, if this is a `StepDelta` carrying text.
    ///
    /// Convenience for the common "print streaming text" loop.
    #[must_use]
    pub fn delta_text(&self) -> Option<&str> {
        match self {
            Self::StepDelta { delta, .. } => delta.as_text(),
            _ => None,
        }
    }

    /// Writes the chunk's fields to a serialization map.
    ///
    /// This is used internally by both `StreamChunk::Serialize` and `StreamEvent::Serialize`
    /// to avoid duplicating the match logic.
    fn write_to_map<M>(&self, map: &mut M) -> Result<(), M::Error>
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
            } => {
                map.serialize_entry("chunk_type", "status_update")?;
                map.serialize_entry(
                    "data",
                    &serde_json::json!({
                        "interaction_id": interaction_id,
                        "status": status,
                    }),
                )?;
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
            Self::StepDelta { index, delta } => {
                map.serialize_entry("chunk_type", "step_delta")?;
                map.serialize_entry(
                    "data",
                    &serde_json::json!({
                        "index": index,
                        "delta": delta,
                    }),
                )?;
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
                if !data.is_null() {
                    map.serialize_entry("data", data)?;
                }
            }
        }
        Ok(())
    }
}

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

        match chunk_type {
            "created" => {
                let interaction: InteractionResponse = serde_json::from_value(data_of(&value))
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
                    .map(String::from)
                    .unwrap_or_else(|| {
                        tracing::warn!("StreamChunk::StatusUpdate is missing interaction_id.");
                        String::new()
                    });
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
                Ok(Self::StepDelta { index, delta })
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
                let response: InteractionResponse = serde_json::from_value(data_of(&value))
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

/// A streaming event with position metadata for resume support.
///
/// This wrapper pairs a [`StreamChunk`] with its `event_id`, enabling stream resumption
/// after network interruptions. To resume a stream, pass the `event_id` from the last
/// successfully received event to resume the stream.
///
/// # Example
///
/// ```ignore
/// let mut last_event_id = None;
/// let mut stream = client.interaction().with_model("gemini-3-flash-preview")
///     .with_text("Count to 100").create_stream();
///
/// while let Some(result) = stream.next().await {
///     let event = result?;
///     last_event_id = event.event_id.clone();  // Track for resume
///     match event.chunk {
///         StreamChunk::StepDelta { delta, .. } => { /* process */ }
///         StreamChunk::Completed(response) => { /* done */ }
///         _ => {}
///     }
/// }
///
/// // If interrupted, resume from last_event_id:
/// let resumed_stream = client.get_interaction_stream(&interaction_id, last_event_id.as_deref());
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StreamEvent {
    /// The chunk content (StepDelta, Completed, Unknown, ...).
    pub chunk: StreamChunk,

    /// Event ID for stream resumption.
    ///
    /// Pass this to `last_event_id` when calling `get_interaction_stream()` to resume
    /// the stream from this point. Events are ordered, so resuming from an event_id
    /// will replay all subsequent events.
    pub event_id: Option<String>,
}

impl StreamEvent {
    /// Creates a new StreamEvent with the given chunk and event_id.
    #[must_use]
    pub fn new(chunk: StreamChunk, event_id: Option<String>) -> Self {
        Self { chunk, event_id }
    }

    /// Returns `true` if the chunk is a StepDelta variant.
    #[must_use]
    pub const fn is_delta(&self) -> bool {
        matches!(self.chunk, StreamChunk::StepDelta { .. })
    }

    /// Returns `true` if the chunk is a Completed variant.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.chunk, StreamChunk::Completed(_))
    }

    /// Returns `true` if the chunk is an Unknown variant.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        self.chunk.is_unknown()
    }

    /// Returns `true` if the chunk is a terminal event (Completed or Error).
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        self.chunk.is_terminal()
    }

    /// Returns the interaction ID from the chunk, if available.
    #[must_use]
    pub fn interaction_id(&self) -> Option<&str> {
        self.chunk.interaction_id()
    }

    /// Returns the status from the chunk, if available.
    #[must_use]
    pub fn status(&self) -> Option<&InteractionStatus> {
        self.chunk.status()
    }

    /// Returns the unrecognized chunk type if this is an Unknown variant.
    #[must_use]
    pub fn unknown_chunk_type(&self) -> Option<&str> {
        self.chunk.unknown_chunk_type()
    }

    /// Returns the preserved JSON data if this is an Unknown variant.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        self.chunk.unknown_data()
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

/// Optional metadata accompanying any streamed event.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StreamMetadata {
    /// Cumulative token usage for the interaction so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_usage: Option<UsageMetadata>,
}

/// Wrapper for SSE streaming events from the Interactions API
/// (revision 2026-05-20 lifecycle).
///
/// The API returns these event types during streaming:
/// - `interaction.created`: Initial event with (partial) interaction data
/// - `interaction.status_update`: Status changes during processing
/// - `step.start`: A step begins at an index
/// - `step.delta`: Incremental step payload
/// - `step.stop`: A step finished (carries `usage` / `step_usage`)
/// - `interaction.completed`: Final complete interaction (terminal)
/// - `error`: Error occurred during streaming (terminal)
#[derive(Clone, Debug)]
pub struct InteractionStreamEvent {
    /// Event type (e.g., "step.delta", "interaction.completed")
    pub event_type: String,

    /// The (partial) interaction data (present in "interaction.created" and
    /// "interaction.completed")
    pub interaction: Option<InteractionResponse>,

    /// Interaction ID (present in "interaction.status_update")
    pub interaction_id: Option<String>,

    /// Status (present in "interaction.status_update")
    pub status: Option<InteractionStatus>,

    /// Position index for step events ("step.start" / "step.delta" / "step.stop")
    pub index: Option<usize>,

    /// The step being started (present in "step.start")
    pub step: Option<Step>,

    /// Incremental step payload (present in "step.delta")
    pub delta: Option<StepDelta>,

    /// Cumulative interaction usage (present in "step.stop")
    pub usage: Option<UsageMetadata>,

    /// Per-step usage (present in "step.stop")
    pub step_usage: Option<UsageMetadata>,

    /// Optional metadata accompanying any event (carries `total_usage`)
    pub metadata: Option<StreamMetadata>,

    /// Error details (present in "error" events)
    pub error: Option<StreamError>,

    /// Event ID for stream resumption.
    ///
    /// Pass this to `last_event_id` when calling `get_interaction_stream()` to resume
    /// the stream from this point after a network interruption.
    pub event_id: Option<String>,

    /// The full raw event JSON, preserved so unknown event types can be
    /// surfaced losslessly (Evergreen).
    pub raw: serde_json::Value,
}

impl<'de> Deserialize<'de> for InteractionStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Fields {
            event_type: Option<String>,
            interaction: Option<InteractionResponse>,
            interaction_id: Option<String>,
            status: Option<InteractionStatus>,
            index: Option<usize>,
            step: Option<Step>,
            delta: Option<StepDelta>,
            usage: Option<UsageMetadata>,
            step_usage: Option<UsageMetadata>,
            metadata: Option<StreamMetadata>,
            error: Option<StreamError>,
            event_id: Option<String>,
        }

        let fields: Fields =
            serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?;

        Ok(Self {
            event_type: fields.event_type.unwrap_or_else(|| {
                tracing::warn!("SSE event missing event_type field.");
                "<missing event_type>".to_string()
            }),
            interaction: fields.interaction,
            interaction_id: fields.interaction_id,
            status: fields.status,
            index: fields.index,
            step: fields.step,
            delta: fields.delta,
            usage: fields.usage,
            step_usage: fields.step_usage,
            metadata: fields.metadata,
            error: fields.error,
            event_id: fields.event_id,
            raw,
        })
    }
}

/// Error details from SSE streaming.
///
/// Represents error information sent in "error" type SSE events.
#[derive(Clone, Deserialize, Debug)]
pub struct StreamError {
    /// Human-readable error message
    #[serde(default)]
    pub message: String,

    /// Error code from the API (if provided). Per spec this is a URI that
    /// identifies the error type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    #[test]
    fn test_stream_chunk_step_delta_roundtrip() {
        let chunk = StreamChunk::StepDelta {
            index: 0,
            delta: StepDelta::Text {
                text: "Hello, world!".to_string(),
            },
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(
            json.contains("step_delta"),
            "Should have step_delta variant"
        );
        assert!(json.contains("Hello, world!"), "Should have content");

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::StepDelta { index, delta } => {
                assert_eq!(index, 0);
                assert_eq!(delta.as_text(), Some("Hello, world!"));
            }
            _ => panic!("Expected StepDelta variant"),
        }
    }

    #[test]
    fn test_stream_chunk_completed_roundtrip() {
        let response = InteractionResponse {
            id: Some("test-interaction-123".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            steps: vec![Step::model_output(vec![Content::text("The answer is 4.")])],
            status: InteractionStatus::Completed,
            ..Default::default()
        };

        let chunk = StreamChunk::Completed(response);

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("completed"), "Should have completed variant");
        assert!(json.contains("test-interaction-123"));
        assert!(json.contains("The answer is 4"));

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Completed(response) => {
                assert_eq!(response.id.as_deref(), Some("test-interaction-123"));
                assert_eq!(response.status, InteractionStatus::Completed);
                assert_eq!(response.as_text(), Some("The answer is 4."));
            }
            _ => panic!("Expected Completed variant"),
        }
    }

    #[test]
    fn test_stream_chunk_unknown_forward_compatibility() {
        // Simulate a future chunk type that doesn't exist yet
        let unknown_json = r#"{"chunk_type": "future_chunk_type", "data": {"key": "value"}}"#;
        let deserialized: StreamChunk =
            serde_json::from_str(unknown_json).expect("Should deserialize unknown variant");

        // Verify it's an Unknown variant
        assert!(deserialized.is_unknown());
        assert_eq!(deserialized.unknown_chunk_type(), Some("future_chunk_type"));

        // Verify data is preserved
        let data = deserialized.unknown_data().expect("Should have data");
        assert_eq!(data["key"], "value");

        // Verify roundtrip serialization
        let reserialized = serde_json::to_string(&deserialized).expect("Should serialize");
        assert!(reserialized.contains("future_chunk_type"));
        assert!(reserialized.contains("value"));
    }

    #[test]
    fn test_stream_chunk_created_roundtrip() {
        let response = InteractionResponse {
            id: Some("test-interaction-456".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            status: InteractionStatus::InProgress,
            ..Default::default()
        };

        let chunk = StreamChunk::Created {
            interaction: response,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("created"), "Should have created variant");

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Created { interaction } => {
                assert_eq!(interaction.id.as_deref(), Some("test-interaction-456"));
                assert_eq!(interaction.status, InteractionStatus::InProgress);
            }
            _ => panic!("Expected Created variant"),
        }
    }

    #[test]
    fn test_stream_chunk_status_update_roundtrip() {
        let chunk = StreamChunk::StatusUpdate {
            interaction_id: "test-interaction-789".to_string(),
            status: InteractionStatus::RequiresAction,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("status_update"));
        assert!(json.contains("test-interaction-789"));

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::StatusUpdate {
                interaction_id,
                status,
            } => {
                assert_eq!(interaction_id, "test-interaction-789");
                assert_eq!(status, InteractionStatus::RequiresAction);
            }
            _ => panic!("Expected StatusUpdate variant"),
        }
    }

    #[test]
    fn test_stream_chunk_step_start_roundtrip() {
        let chunk = StreamChunk::StepStart {
            index: 0,
            step: Step::model_output(vec![]),
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("step_start"));

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::StepStart { index, step } => {
                assert_eq!(index, 0);
                assert!(matches!(step, Step::ModelOutput { .. }));
            }
            _ => panic!("Expected StepStart variant"),
        }
    }

    #[test]
    fn test_stream_chunk_step_stop_roundtrip() {
        let chunk = StreamChunk::StepStop {
            index: 1,
            usage: None,
            step_usage: Some(UsageMetadata {
                total_output_tokens: Some(7),
                ..Default::default()
            }),
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("step_stop"));
        assert!(json.contains("\"index\":1"));

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::StepStop {
                index, step_usage, ..
            } => {
                assert_eq!(index, 1);
                assert_eq!(step_usage.unwrap().total_output_tokens, Some(7));
            }
            _ => panic!("Expected StepStop variant"),
        }
    }

    #[test]
    fn test_stream_chunk_error_roundtrip() {
        let chunk = StreamChunk::Error {
            message: "Rate limit exceeded".to_string(),
            code: Some("RATE_LIMIT".to_string()),
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Error { message, code } => {
                assert_eq!(message, "Rate limit exceeded");
                assert_eq!(code, Some("RATE_LIMIT".to_string()));
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_stream_chunk_helper_methods() {
        let created_chunk = StreamChunk::Created {
            interaction: InteractionResponse {
                id: Some("start-id".to_string()),
                status: InteractionStatus::InProgress,
                ..Default::default()
            },
        };
        assert_eq!(created_chunk.interaction_id(), Some("start-id"));
        assert!(!created_chunk.is_terminal());

        let status_chunk = StreamChunk::StatusUpdate {
            interaction_id: "status-id".to_string(),
            status: InteractionStatus::InProgress,
        };
        assert_eq!(status_chunk.interaction_id(), Some("status-id"));

        let delta_chunk = StreamChunk::StepDelta {
            index: 0,
            delta: StepDelta::Text {
                text: "test".to_string(),
            },
        };
        assert_eq!(delta_chunk.interaction_id(), None);
        assert_eq!(delta_chunk.delta_text(), Some("test"));
        assert!(!delta_chunk.is_terminal());

        let completed_chunk = StreamChunk::Completed(InteractionResponse {
            status: InteractionStatus::Completed,
            ..Default::default()
        });
        assert!(completed_chunk.is_terminal());
        assert_eq!(
            completed_chunk.status(),
            Some(&InteractionStatus::Completed)
        );

        let error_chunk = StreamChunk::Error {
            message: "test".to_string(),
            code: None,
        };
        assert!(error_chunk.is_terminal());
    }

    #[test]
    fn test_stream_event_with_event_id_roundtrip() {
        let event = StreamEvent::new(
            StreamChunk::StepDelta {
                index: 0,
                delta: StepDelta::Text {
                    text: "Hello".to_string(),
                },
            },
            Some("evt_abc123".to_string()),
        );

        // Test helper methods
        assert!(event.is_delta());
        assert!(!event.is_complete());
        assert!(!event.is_unknown());

        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        assert!(json.contains("evt_abc123"), "Should have event_id");
        assert!(json.contains("Hello"), "Should have content");

        let deserialized: StreamEvent =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        assert_eq!(deserialized.event_id.as_deref(), Some("evt_abc123"));
        assert!(deserialized.is_delta());
    }

    #[test]
    fn test_interaction_stream_event_step_delta_wire_format() {
        // Exact wire shape of a revision 2026-05-20 step.delta SSE payload.
        let json = r#"{
            "event_type": "step.delta",
            "index": 0,
            "delta": {"type": "text", "text": "Hello"},
            "event_id": "evt_resume_token_123"
        }"#;

        let event: InteractionStreamEvent = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(event.event_type, "step.delta");
        assert_eq!(event.index, Some(0));
        assert_eq!(event.event_id.as_deref(), Some("evt_resume_token_123"));
        assert_eq!(
            event.delta.as_ref().and_then(|d| d.as_text()),
            Some("Hello")
        );
        assert_eq!(event.raw["event_type"], "step.delta");
    }

    #[test]
    fn test_interaction_stream_event_step_stop_usage() {
        let json = r#"{
            "event_type": "step.stop",
            "index": 2,
            "usage": {"total_tokens": 30},
            "step_usage": {"total_output_tokens": 9},
            "metadata": {"total_usage": {"total_tokens": 30}}
        }"#;
        let event: InteractionStreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.usage.as_ref().unwrap().total_tokens, Some(30));
        assert_eq!(
            event.step_usage.as_ref().unwrap().total_output_tokens,
            Some(9)
        );
        assert_eq!(
            event
                .metadata
                .as_ref()
                .unwrap()
                .total_usage
                .as_ref()
                .unwrap()
                .total_tokens,
            Some(30)
        );
    }
}
