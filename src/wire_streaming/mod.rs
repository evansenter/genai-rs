//! Streaming types for SSE responses (API revision 2026-05-20).

use serde::{Deserialize, Serialize};

use crate::response::{InteractionResponse, InteractionStatus, UsageMetadata};
use crate::steps::{Step, StepDelta};

mod serde_impls;

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
pub enum StreamChunk {
    /// Interaction created (first event, contains ID).
    ///
    /// Sent when the interaction is accepted by the API. Provides early access
    /// to the interaction ID before any content is generated. The payload is a
    /// partial interaction resource (some fields present only on the final
    /// response may be absent).
    Created {
        /// The (partial) interaction resource at creation time. Boxed, like
        /// every large payload here, so that the common event, a delta, stays
        /// small (#490).
        interaction: Box<InteractionResponse>,
    },

    /// Status update for in-progress interaction.
    ///
    /// Sent when the interaction status changes during processing.
    /// Useful for tracking progress of background/agent interactions.
    StatusUpdate {
        /// The interaction ID. Absent on a `store: false` stream, which has
        /// no stored interaction to name.
        interaction_id: Option<String>,
        /// The updated status
        status: InteractionStatus,
        /// Usage so far. Background streams send it mid-run (google-genai
        /// 2.34.0).
        usage: Option<Box<UsageMetadata>>,
        /// A continuation token (google-genai 2.34.0; not observed live,
        /// where it arrives on the completed interaction instead).
        continuation_token: Option<String>,
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
        /// Cumulative interaction usage so far, from the event's
        /// `metadata.total_usage` (google-genai 2.34.0 and the spec). Not
        /// observed live: text, TTS and agent streams send no `metadata` on
        /// a `step.delta`, so expect `None`.
        total_usage: Option<Box<UsageMetadata>>,
    },

    /// The step at the given index finished.
    StepStop {
        /// Position index for the completed step
        index: usize,
        /// Cumulative interaction usage, if reported
        usage: Option<Box<UsageMetadata>>,
        /// Usage attributable to this step, if reported
        step_usage: Option<Box<UsageMetadata>>,
    },

    /// Complete interaction response (terminal event).
    Completed(Box<InteractionResponse>),

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
            Self::StatusUpdate { interaction_id, .. } => interaction_id.as_deref(),
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
}

/// A streaming event with position metadata for resume support.
///
/// This wrapper pairs a [`StreamChunk`] with its `event_id`. Only a
/// **background** interaction's stream carries event ids and can be
/// resumed: a foreground `create_stream()` carries none, and the server
/// cancels it when the connection drops.
///
/// # Example
///
/// ```no_run
/// use futures_util::StreamExt;
/// use genai_rs::{Client, StreamChunk};
///
/// # async fn example(client: Client) -> Result<(), genai_rs::GenaiError> {
/// // A background interaction, followed by streaming its events.
/// let created = client.interaction().with_model(genai_rs::DEFAULT_MODEL)
///     .with_text("Count to 100").with_background(true).create().await?;
/// let id = created.id.clone().unwrap_or_default();
///
/// let mut last_event_id = None;
/// let mut stream = client.interactions().stream(&id);
/// while let Some(result) = stream.next().await {
///     let event = result?;
///     if event.event_id.is_some() {
///         last_event_id = event.event_id.clone(); // Track for resume
///     }
///     match event.chunk {
///         StreamChunk::StepDelta { delta, .. } => { /* process */ }
///         StreamChunk::Completed(response) => { /* done */ }
///         _ => {}
///     }
/// }
///
/// // If interrupted, resume from last_event_id:
/// if let Some(last) = &last_event_id {
///     let resumed_stream = client.interactions().resume_stream(&id, last);
/// #   drop(resumed_stream);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StreamEvent {
    /// The chunk content (StepDelta, Completed, Unknown, ...).
    pub chunk: StreamChunk,

    /// Event ID for stream resumption.
    ///
    /// Pass this to [`Interactions::resume_stream`](crate::Interactions::resume_stream)
    /// to resume the stream after this point. Only background interactions' streams carry
    /// it (and only they can be resumed); otherwise it is `None`.
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

/// Optional metadata accompanying any streamed event.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct StreamMetadata {
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
pub(crate) struct InteractionStreamEvent {
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

    /// Continuation token (in "interaction.status_update", per the
    /// google-genai 2.34.0 bindings)
    pub continuation_token: Option<String>,

    /// Event ID for stream resumption.
    ///
    /// Pass this to `Interactions::resume_stream` to resume
    /// the stream from this point after a network interruption.
    pub event_id: Option<String>,

    /// The full raw event JSON of an event type this crate does not
    /// dispatch, preserved so it can be surfaced losslessly (Evergreen).
    /// `None` for the known types, which are parsed by value (#498).
    pub raw: Option<serde_json::Value>,
}

/// The event types `dispatch_stream_event` handles, whose raw JSON is not
/// kept.
const KNOWN_EVENT_TYPES: &[&str] = &[
    "interaction.created",
    "interaction.status_update",
    "step.start",
    "step.delta",
    "step.stop",
    "interaction.completed",
    "error",
];

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
            continuation_token: Option<String>,
            event_id: Option<String>,
        }

        // A known event is parsed by value; only an unknown one keeps a copy
        // of its JSON for `StreamChunk::Unknown`.
        let known = raw
            .get("event_type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|event_type| KNOWN_EVENT_TYPES.contains(&event_type));
        let (fields, raw) = if known {
            (Fields::deserialize(raw), None)
        } else {
            (Fields::deserialize(&raw), Some(raw))
        };
        let fields = fields.map_err(serde::de::Error::custom)?;

        // A bare `{"error": {...}}` line (see `http::sse_parser`) carries no
        // event_type; it is the stream's error event.
        let event_type = match fields.event_type {
            Some(event_type) => event_type,
            None if fields.error.is_some() => "error".to_string(),
            None => {
                tracing::warn!("SSE event missing event_type field.");
                "<missing event_type>".to_string()
            }
        };

        Ok(Self {
            event_type,
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
            continuation_token: fields.continuation_token,
            event_id: fields.event_id,
            raw,
        })
    }
}

/// Error details from SSE streaming.
///
/// Represents error information sent in "error" type SSE events.
#[derive(Clone, Deserialize, Debug)]
pub(crate) struct StreamError {
    /// Human-readable error message
    #[serde(default)]
    pub message: String,

    /// Error code from the API (if provided). Per spec this is a URI that
    /// identifies the error type.
    #[serde(default)]
    pub code: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Content;

    /// A delta is the event a stream sends most, and every event is as big
    /// as the largest variant, so the large, rare payloads are boxed: the
    /// interaction responses, the usage fields and a thought summary's
    /// content (#490). Unboxed, an event was 1.9 KB.
    #[test]
    fn stream_event_size_is_bounded() {
        use std::mem::size_of;
        assert!(
            size_of::<StreamEvent>() <= 256,
            "StreamEvent is {} bytes",
            size_of::<StreamEvent>()
        );
        assert!(
            size_of::<crate::AutoFunctionStreamEvent>() <= 256,
            "AutoFunctionStreamEvent is {} bytes",
            size_of::<crate::AutoFunctionStreamEvent>()
        );
    }

    #[test]
    fn test_stream_chunk_step_delta_roundtrip() {
        let chunk = StreamChunk::StepDelta {
            index: 0,
            delta: StepDelta::Text {
                text: "Hello, world!".to_string(),
                extra: Default::default(),
            },
            total_usage: None,
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
            StreamChunk::StepDelta { index, delta, .. } => {
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
            model: Some("test-model".to_string()),
            steps: vec![Step::model_output(vec![Content::text("The answer is 4.")])],
            status: InteractionStatus::Completed,
            ..Default::default()
        };

        let chunk = StreamChunk::Completed(Box::new(response));

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
        assert!(!reserialized.contains("\"unknown\""), "{reserialized}");
    }

    #[test]
    fn test_unknown_chunk_named_like_a_known_one_stays_unknown() {
        // Found by fuzzing (fuzz/, `stream_event`).
        for name in serde_impls::KNOWN_CHUNK_TYPES {
            let chunk = StreamChunk::Unknown {
                chunk_type: name.to_string(),
                data: serde_json::json!({"event_type": name}),
            };
            let value = serde_json::to_value(&chunk).unwrap();
            assert_eq!(value["unknown"], true, "{value}");
            let back: StreamChunk = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(back.unknown_chunk_type(), Some(name), "{back:?}");
            assert_eq!(serde_json::to_value(&back).unwrap(), value);
        }
        // Without the marker, the known name means the known chunk.
        let known: StreamChunk = serde_json::from_value(
            serde_json::json!({"chunk_type": "error", "data": {"message": "m"}}),
        )
        .unwrap();
        assert!(matches!(known, StreamChunk::Error { .. }), "{known:?}");
    }

    /// The `chunk_type` each known variant actually writes.
    fn written_chunk_types() -> std::collections::BTreeSet<String> {
        let samples = [
            StreamChunk::Created {
                interaction: Box::default(),
            },
            StreamChunk::StatusUpdate {
                interaction_id: None,
                status: InteractionStatus::InProgress,
                usage: None,
                continuation_token: None,
            },
            StreamChunk::StepStart {
                index: 0,
                step: Step::model_output(vec![]),
            },
            StreamChunk::StepDelta {
                index: 0,
                delta: StepDelta::Text {
                    text: String::new(),
                    extra: Default::default(),
                },
                total_usage: None,
            },
            StreamChunk::StepStop {
                index: 0,
                usage: None,
                step_usage: None,
            },
            StreamChunk::Completed(Box::default()),
            StreamChunk::Error {
                message: String::new(),
                code: None,
            },
        ];
        for sample in &samples {
            // No wildcard: a new variant stops this compiling until it has a
            // sample above.
            match sample {
                StreamChunk::Created { .. }
                | StreamChunk::StatusUpdate { .. }
                | StreamChunk::StepStart { .. }
                | StreamChunk::StepDelta { .. }
                | StreamChunk::StepStop { .. }
                | StreamChunk::Completed(_)
                | StreamChunk::Error { .. } => {}
                StreamChunk::Unknown { .. } => unreachable!("samples are known chunks"),
            }
        }
        let written: std::collections::BTreeSet<String> = samples
            .iter()
            .map(|sample| {
                serde_json::to_value(sample).unwrap()["chunk_type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(written.len(), samples.len(), "two variants share a name");
        written
    }

    #[test]
    fn test_known_chunk_types_are_the_serializers_names() {
        // The collision marker is only as good as this hand-kept list.
        let known: std::collections::BTreeSet<String> = serde_impls::KNOWN_CHUNK_TYPES
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(known, written_chunk_types());
    }

    #[test]
    fn test_enum_wire_formats_doc_lists_the_serializers_names() {
        // The StreamChunk table in docs/ENUM_WIRE_FORMATS.md is hand-kept.
        let doc = include_str!("../../docs/ENUM_WIRE_FORMATS.md");
        let section = doc
            .split("\n### StreamChunk\n")
            .nth(1)
            .expect("a `### StreamChunk` section");
        let section = section.split("\n#").next().unwrap();
        let documented: std::collections::BTreeSet<String> = section
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|rest| rest.split_once('`'))
            .map(|(name, _)| name.to_string())
            .filter(|name| name != "chunk_type")
            .collect();
        assert_eq!(documented, written_chunk_types());
    }

    #[test]
    fn test_stream_chunk_created_roundtrip() {
        let response = InteractionResponse {
            id: Some("test-interaction-456".to_string()),
            model: Some("test-model".to_string()),
            status: InteractionStatus::InProgress,
            ..Default::default()
        };

        let chunk = StreamChunk::Created {
            interaction: Box::new(response),
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
            interaction_id: Some("test-interaction-789".to_string()),
            status: InteractionStatus::RequiresAction,
            usage: Some(Box::new(UsageMetadata {
                total_tokens: Some(12),
                ..Default::default()
            })),
            continuation_token: None,
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
                usage,
                continuation_token,
            } => {
                assert_eq!(interaction_id.as_deref(), Some("test-interaction-789"));
                assert_eq!(status, InteractionStatus::RequiresAction);
                assert_eq!(usage.and_then(|u| u.total_tokens), Some(12));
                assert_eq!(continuation_token, None);
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
            step_usage: Some(Box::new(UsageMetadata {
                total_output_tokens: Some(7),
                ..Default::default()
            })),
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
            interaction: Box::new(InteractionResponse {
                id: Some("start-id".to_string()),
                status: InteractionStatus::InProgress,
                ..Default::default()
            }),
        };
        assert_eq!(created_chunk.interaction_id(), Some("start-id"));
        assert!(!created_chunk.is_terminal());

        let status_chunk = StreamChunk::StatusUpdate {
            interaction_id: Some("status-id".to_string()),
            status: InteractionStatus::InProgress,
            usage: None,
            continuation_token: None,
        };
        assert_eq!(status_chunk.interaction_id(), Some("status-id"));

        let delta_chunk = StreamChunk::StepDelta {
            index: 0,
            delta: StepDelta::Text {
                text: "test".to_string(),
                extra: Default::default(),
            },
            total_usage: None,
        };
        assert_eq!(delta_chunk.interaction_id(), None);
        assert_eq!(delta_chunk.delta_text(), Some("test"));
        assert!(!delta_chunk.is_terminal());

        let completed_chunk = StreamChunk::Completed(Box::new(InteractionResponse {
            status: InteractionStatus::Completed,
            ..Default::default()
        }));
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
                    extra: Default::default(),
                },
                total_usage: None,
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
        // Known events are parsed by value, without a copy of their JSON.
        assert!(event.raw.is_none());
    }

    #[test]
    fn test_unknown_stream_event_keeps_its_raw_json() {
        let json = r#"{"event_type": "step.pause", "index": 1, "reason": "x"}"#;
        let event: InteractionStreamEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "step.pause");
        assert_eq!(
            event.raw,
            Some(serde_json::json!({"event_type": "step.pause", "index": 1, "reason": "x"}))
        );
    }

    #[test]
    fn test_step_delta_total_usage_round_trips() {
        let chunk = StreamChunk::StepDelta {
            index: 2,
            delta: StepDelta::Text {
                text: "hi".to_string(),
                extra: Default::default(),
            },
            total_usage: Some(Box::new(UsageMetadata {
                total_tokens: Some(40),
                ..Default::default()
            })),
        };
        let value = serde_json::to_value(&chunk).unwrap();
        assert_eq!(value["data"]["total_usage"]["total_tokens"], 40);
        let back: StreamChunk = serde_json::from_value(value).unwrap();
        match back {
            StreamChunk::StepDelta { total_usage, .. } => {
                assert_eq!(total_usage.and_then(|u| u.total_tokens), Some(40));
            }
            other => panic!("Expected StepDelta, got {other:?}"),
        }
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
