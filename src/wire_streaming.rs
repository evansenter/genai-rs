//! Streaming types for SSE responses.

use serde::{Deserialize, Serialize};

use crate::content::Content;
use crate::response::{InteractionResponse, InteractionStatus};

/// A chunk from the streaming API
///
/// During streaming, the API sends different types of events:
/// - `Start`: Initial interaction event (first event, contains ID)
/// - `StatusUpdate`: Status changes during processing
/// - `ContentStart`: Content generation begins for an output
/// - `Delta`: Incremental content updates (text, thought, function_call, etc.)
/// - `ContentStop`: Content generation ends for an output
/// - `Complete`: The final complete interaction response
/// - `Error`: Error occurred during streaming
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
    /// Interaction started (first event, contains ID).
    ///
    /// Sent when the interaction is accepted by the API. Provides early access
    /// to the interaction ID before any content is generated.
    Start {
        /// The full interaction response at start time
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

    /// Content generation started for an output.
    ///
    /// Sent when a new content block begins generation.
    /// The `index` indicates which output position this content will occupy.
    ContentStart {
        /// Position index for this content block
        index: usize,
        /// The content type being started (e.g., "text", "thought")
        content_type: Option<String>,
    },

    /// Incremental content update
    Delta(Content),

    /// Content generation stopped for an output.
    ///
    /// Sent when a content block finishes generation.
    ContentStop {
        /// Position index for the completed content block
        index: usize,
    },

    /// Complete interaction response (final event)
    Complete(InteractionResponse),

    /// Error occurred during streaming.
    ///
    /// Indicates a terminal error condition. The stream will end after this event.
    Error {
        /// Human-readable error message
        message: String,
        /// Error code from the API (if provided)
        code: Option<String>,
    },

    /// Unknown chunk type (for forward compatibility).
    ///
    /// This variant is used when deserializing JSON that contains an unrecognized
    /// `chunk_type`. This allows the library to gracefully handle new chunk types
    /// added by the API in future versions without failing deserialization.
    ///
    /// The `chunk_type` field contains the unrecognized type string, and `data`
    /// contains the full JSON data for inspection or debugging.
    Unknown {
        /// The unrecognized chunk type from the API
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
    /// Available for `Start`, `StatusUpdate`, and `Complete` variants.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::StreamChunk;
    /// # fn example(chunk: StreamChunk) {
    /// if let Some(id) = chunk.interaction_id() {
    ///     println!("Interaction ID: {}", id);
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn interaction_id(&self) -> Option<&str> {
        match self {
            Self::Start { interaction } => interaction.id.as_deref(),
            Self::StatusUpdate { interaction_id, .. } => Some(interaction_id),
            Self::Complete(response) => response.id.as_deref(),
            _ => None,
        }
    }

    /// Returns true if this is a terminal event.
    ///
    /// Terminal events indicate the stream has ended (either successfully or with an error).
    /// After receiving a terminal event, no more events will be sent.
    ///
    /// Terminal events are:
    /// - `Complete`: Successful completion
    /// - `Error`: Error occurred
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::StreamChunk;
    /// # fn example(chunk: StreamChunk) {
    /// if chunk.is_terminal() {
    ///     println!("Stream has ended");
    /// }
    /// # }
    /// ```
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Complete(_) | Self::Error { .. })
    }

    /// Returns the status if this event contains one.
    ///
    /// Available for `StatusUpdate` and `Complete` variants.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::StreamChunk;
    /// # fn example(chunk: StreamChunk) {
    /// if let Some(status) = chunk.status() {
    ///     println!("Status: {:?}", status);
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn status(&self) -> Option<&InteractionStatus> {
        match self {
            Self::StatusUpdate { status, .. } => Some(status),
            Self::Complete(response) => Some(&response.status),
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
            Self::Start { interaction } => {
                map.serialize_entry("chunk_type", "start")?;
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
            Self::ContentStart {
                index,
                content_type,
            } => {
                map.serialize_entry("chunk_type", "content_start")?;
                map.serialize_entry(
                    "data",
                    &serde_json::json!({
                        "index": index,
                        "content_type": content_type,
                    }),
                )?;
            }
            Self::Delta(content) => {
                map.serialize_entry("chunk_type", "delta")?;
                map.serialize_entry("data", content)?;
            }
            Self::ContentStop { index } => {
                map.serialize_entry("chunk_type", "content_stop")?;
                map.serialize_entry("data", &serde_json::json!({ "index": index }))?;
            }
            Self::Complete(response) => {
                map.serialize_entry("chunk_type", "complete")?;
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
                     This may indicate a malformed API response.",
                    other
                );
                "<non-string chunk_type>"
            }
            None => {
                tracing::warn!(
                    "StreamChunk is missing required chunk_type field. \
                     This may indicate a malformed API response."
                );
                "<missing chunk_type>"
            }
        };

        match chunk_type {
            "start" => {
                let data = match value.get("data").cloned() {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            "StreamChunk::Start is missing the 'data' field. \
                             This may indicate a malformed API response."
                        );
                        serde_json::Value::Null
                    }
                };
                let interaction: InteractionResponse =
                    serde_json::from_value(data).map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize StreamChunk::Start data: {}",
                            e
                        ))
                    })?;
                Ok(Self::Start { interaction })
            }
            "status_update" => {
                let data = value
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let interaction_id = data
                    .get("interaction_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        tracing::warn!(
                            "StreamChunk::StatusUpdate is missing interaction_id. \
                             This may indicate a malformed API response."
                        );
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
                        tracing::warn!(
                            "StreamChunk::StatusUpdate is missing status. \
                             This may indicate a malformed API response."
                        );
                        InteractionStatus::InProgress
                    });
                Ok(Self::StatusUpdate {
                    interaction_id,
                    status,
                })
            }
            "content_start" => {
                let data = value
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let index = data
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or_else(|| {
                        tracing::warn!(
                            "StreamChunk::ContentStart is missing index. \
                             This may indicate a malformed API response."
                        );
                        0
                    });
                let content_type = data
                    .get("content_type")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                Ok(Self::ContentStart {
                    index,
                    content_type,
                })
            }
            "delta" => {
                let data = match value.get("data").cloned() {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            "StreamChunk::Delta is missing the 'data' field. \
                             This may indicate a malformed API response."
                        );
                        serde_json::Value::Null
                    }
                };
                let content: Content = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize StreamChunk::Delta data: {}",
                        e
                    ))
                })?;
                Ok(Self::Delta(content))
            }
            "content_stop" => {
                let data = value
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let index = data
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or_else(|| {
                        tracing::warn!(
                            "StreamChunk::ContentStop is missing index. \
                             This may indicate a malformed API response."
                        );
                        0
                    });
                Ok(Self::ContentStop { index })
            }
            "complete" => {
                let data = match value.get("data").cloned() {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            "StreamChunk::Complete is missing the 'data' field. \
                             This may indicate a malformed API response."
                        );
                        serde_json::Value::Null
                    }
                };
                let response: InteractionResponse = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize StreamChunk::Complete data: {}",
                        e
                    ))
                })?;
                Ok(Self::Complete(response))
            }
            "error" => {
                let data = value
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let message = data
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        tracing::warn!(
                            "StreamChunk::Error is missing message. \
                             This may indicate a malformed API response."
                        );
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
///         StreamChunk::Delta(content) => { /* process */ }
///         StreamChunk::Complete(response) => { /* done */ }
///         _ => {}
///     }
/// }
///
/// // If interrupted, resume from last_event_id:
/// let resumed_stream = client.get_interaction_stream(&interaction_id, last_event_id.as_deref(), false);
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StreamEvent {
    /// The chunk content (Delta, Complete, or Unknown).
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

    /// Returns `true` if the chunk is a Delta variant.
    #[must_use]
    pub const fn is_delta(&self) -> bool {
        matches!(self.chunk, StreamChunk::Delta(_))
    }

    /// Returns `true` if the chunk is a Complete variant.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.chunk, StreamChunk::Complete(_))
    }

    /// Returns `true` if the chunk is an Unknown variant.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        self.chunk.is_unknown()
    }

    /// Returns `true` if the chunk is a terminal event (Complete or Error).
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

/// Wrapper for SSE streaming events from the Interactions API
///
/// The API returns different event types during streaming:
/// - `interaction.start`: Initial event with interaction data
/// - `interaction.status_update`: Status changes during processing
/// - `content.start`: Content generation begins
/// - `content.delta`: Incremental content updates
/// - `content.stop`: Content generation ends
/// - `interaction.complete`: Final complete interaction
/// - `error`: Error occurred during streaming
#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct InteractionStreamEvent {
    /// Event type (e.g., "content.delta", "interaction.complete")
    pub event_type: String,

    /// The full interaction data (present in "interaction.start" and "interaction.complete")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction: Option<InteractionResponse>,

    /// Incremental content delta (present in "content.delta" events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Content>,

    /// Interaction ID (present in various events like "interaction.status_update")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,

    /// Status (present in "interaction.status_update" events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<InteractionStatus>,

    /// Position index for content blocks (present in "content.start" and "content.stop")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Content object being started (present in "content.start" events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,

    /// Error details (present in "error" events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StreamError>,

    /// Event ID for stream resumption.
    ///
    /// Pass this to `last_event_id` when calling `get_interaction_stream()` to resume
    /// the stream from this point after a network interruption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

/// Error details from SSE streaming.
///
/// Represents error information sent in "error" type SSE events.
#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct StreamError {
    /// Human-readable error message
    #[serde(default)]
    pub message: String,

    /// Error code from the API (if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_chunk_delta_roundtrip() {
        let chunk = StreamChunk::Delta(Content::Text {
            text: Some("Hello, world!".to_string()),
            annotations: None,
        });

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(json.contains("delta"), "Should have delta variant");
        assert!(json.contains("Hello, world!"), "Should have content");

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Delta(content) => {
                assert_eq!(content.as_text(), Some("Hello, world!"));
            }
            _ => panic!("Expected Delta variant"),
        }
    }

    #[test]
    fn test_stream_chunk_complete_roundtrip() {
        let response = InteractionResponse {
            id: Some("test-interaction-123".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            agent: None,
            input: vec![Content::Text {
                text: Some("What is 2+2?".to_string()),
                annotations: None,
            }],
            outputs: vec![Content::Text {
                text: Some("The answer is 4.".to_string()),
                annotations: None,
            }],
            status: InteractionStatus::Completed,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        };

        let chunk = StreamChunk::Complete(response);

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(json.contains("complete"), "Should have complete variant");
        assert!(
            json.contains("test-interaction-123"),
            "Should have interaction id"
        );
        assert!(
            json.contains("The answer is 4"),
            "Should have response text"
        );

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Complete(response) => {
                assert_eq!(response.id.as_deref(), Some("test-interaction-123"));
                assert_eq!(response.status, InteractionStatus::Completed);
                assert_eq!(response.as_text(), Some("The answer is 4."));
            }
            _ => panic!("Expected Complete variant"),
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
    fn test_stream_chunk_unknown_without_data() {
        // Test unknown chunk type without data field
        let unknown_json = r#"{"chunk_type": "no_data_chunk"}"#;
        let deserialized: StreamChunk =
            serde_json::from_str(unknown_json).expect("Should deserialize unknown variant");

        assert!(deserialized.is_unknown());
        assert_eq!(deserialized.unknown_chunk_type(), Some("no_data_chunk"));

        // Data should be null when not provided
        let data = deserialized.unknown_data().expect("Should have data field");
        assert!(data.is_null());
    }

    #[test]
    fn test_stream_chunk_start_roundtrip() {
        let response = InteractionResponse {
            id: Some("test-interaction-456".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            agent: None,
            input: vec![Content::Text {
                text: Some("Hello".to_string()),
                annotations: None,
            }],
            outputs: vec![],
            status: InteractionStatus::InProgress,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        };

        let chunk = StreamChunk::Start {
            interaction: response,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(json.contains("start"), "Should have start variant");
        assert!(
            json.contains("test-interaction-456"),
            "Should have interaction id"
        );

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Start { interaction } => {
                assert_eq!(interaction.id.as_deref(), Some("test-interaction-456"));
                assert_eq!(interaction.status, InteractionStatus::InProgress);
            }
            _ => panic!("Expected Start variant"),
        }
    }

    #[test]
    fn test_stream_chunk_status_update_roundtrip() {
        let chunk = StreamChunk::StatusUpdate {
            interaction_id: "test-interaction-789".to_string(),
            status: InteractionStatus::RequiresAction,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(
            json.contains("status_update"),
            "Should have status_update variant"
        );
        assert!(
            json.contains("test-interaction-789"),
            "Should have interaction id"
        );

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
    fn test_stream_chunk_content_start_roundtrip() {
        let chunk = StreamChunk::ContentStart {
            index: 0,
            content_type: Some("text".to_string()),
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(
            json.contains("content_start"),
            "Should have content_start variant"
        );
        assert!(json.contains("\"index\":0"), "Should have index");
        assert!(json.contains("text"), "Should have content_type");

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::ContentStart {
                index,
                content_type,
            } => {
                assert_eq!(index, 0);
                assert_eq!(content_type, Some("text".to_string()));
            }
            _ => panic!("Expected ContentStart variant"),
        }
    }

    #[test]
    fn test_stream_chunk_content_stop_roundtrip() {
        let chunk = StreamChunk::ContentStop { index: 1 };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(
            json.contains("content_stop"),
            "Should have content_stop variant"
        );
        assert!(json.contains("\"index\":1"), "Should have index");

        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::ContentStop { index } => {
                assert_eq!(index, 1);
            }
            _ => panic!("Expected ContentStop variant"),
        }
    }

    #[test]
    fn test_stream_chunk_error_roundtrip() {
        let chunk = StreamChunk::Error {
            message: "Rate limit exceeded".to_string(),
            code: Some("RATE_LIMIT".to_string()),
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json.contains("chunk_type"), "Should have chunk_type tag");
        assert!(json.contains("error"), "Should have error variant");
        assert!(json.contains("Rate limit exceeded"), "Should have message");
        assert!(json.contains("RATE_LIMIT"), "Should have code");

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
    fn test_stream_chunk_error_without_code() {
        let chunk = StreamChunk::Error {
            message: "Unknown error".to_string(),
            code: None,
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let deserialized: StreamChunk =
            serde_json::from_str(&json).expect("Deserialization should succeed");

        match deserialized {
            StreamChunk::Error { message, code } => {
                assert_eq!(message, "Unknown error");
                assert!(code.is_none());
            }
            _ => panic!("Expected Error variant"),
        }
    }

    #[test]
    fn test_stream_chunk_helper_methods() {
        // Test interaction_id()
        let start_chunk = StreamChunk::Start {
            interaction: InteractionResponse {
                id: Some("start-id".to_string()),
                model: None,
                agent: None,
                input: vec![],
                outputs: vec![],
                status: InteractionStatus::InProgress,
                usage: None,
                tools: None,
                grounding_metadata: None,
                url_context_metadata: None,
                previous_interaction_id: None,
                created: None,
                updated: None,
            },
        };
        assert_eq!(start_chunk.interaction_id(), Some("start-id"));

        let status_chunk = StreamChunk::StatusUpdate {
            interaction_id: "status-id".to_string(),
            status: InteractionStatus::InProgress,
        };
        assert_eq!(status_chunk.interaction_id(), Some("status-id"));

        let delta_chunk = StreamChunk::Delta(Content::Text {
            text: Some("test".to_string()),
            annotations: None,
        });
        assert_eq!(delta_chunk.interaction_id(), None);

        // Test is_terminal()
        let complete_chunk = StreamChunk::Complete(InteractionResponse {
            id: None,
            model: None,
            agent: None,
            input: vec![],
            outputs: vec![],
            status: InteractionStatus::Completed,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        });
        assert!(complete_chunk.is_terminal());

        let error_chunk = StreamChunk::Error {
            message: "test".to_string(),
            code: None,
        };
        assert!(error_chunk.is_terminal());

        assert!(!delta_chunk.is_terminal());
        assert!(!start_chunk.is_terminal());

        // Test status()
        assert_eq!(status_chunk.status(), Some(&InteractionStatus::InProgress));
        assert_eq!(complete_chunk.status(), Some(&InteractionStatus::Completed));
        assert_eq!(delta_chunk.status(), None);
    }

    #[test]
    fn test_stream_event_with_event_id_roundtrip() {
        let event = StreamEvent::new(
            StreamChunk::Delta(Content::Text {
                text: Some("Hello".to_string()),
                annotations: None,
            }),
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
    fn test_stream_event_without_event_id() {
        let event = StreamEvent::new(
            StreamChunk::Complete(InteractionResponse {
                id: Some("interaction-123".to_string()),
                model: Some("gemini-3-flash-preview".to_string()),
                agent: None,
                input: vec![],
                outputs: vec![Content::Text {
                    text: Some("Response".to_string()),
                    annotations: None,
                }],
                status: InteractionStatus::Completed,
                usage: None,
                tools: None,
                grounding_metadata: None,
                url_context_metadata: None,
                previous_interaction_id: None,
                created: None,
                updated: None,
            }),
            None,
        );

        assert!(event.is_complete());
        assert!(!event.is_delta());
        assert!(event.event_id.is_none());

        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        assert!(!json.contains("event_id"), "Should not have event_id field");

        let deserialized: StreamEvent =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        assert!(deserialized.event_id.is_none());
        assert!(deserialized.is_complete());
    }

    #[test]
    fn test_interaction_stream_event_with_event_id() {
        let json = r#"{
            "event_type": "content.delta",
            "delta": {"type": "text", "text": "Hello"},
            "event_id": "evt_resume_token_123"
        }"#;

        let event: InteractionStreamEvent = serde_json::from_str(json).expect("Should deserialize");
        assert_eq!(event.event_type, "content.delta");
        assert_eq!(event.event_id.as_deref(), Some("evt_resume_token_123"));
        assert!(event.delta.is_some());
    }
}
