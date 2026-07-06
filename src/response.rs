//! Response types for the Interactions API.
//!
//! This module contains `InteractionResponse` and related types for handling
//! API responses, including helper methods for extracting content from the
//! `steps` array (API revision 2026-05-20).

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fmt;

use crate::content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleSearchResultItem,
};
use crate::errors::GenaiError;
use crate::request::InteractionInput;
use crate::steps::{FunctionResultPayload, Step};
use crate::tools::Tool;

// =============================================================================
// Token Count Deserialization Helpers
// =============================================================================

/// Deserializes a token count as `u32`, warning if the JSON value is negative.
///
/// Token counts should never be negative, but we handle this gracefully per
/// Evergreen principles. Negative values are clamped to 0 with a warning log.
fn deserialize_token_count<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = i64::deserialize(deserializer)?;
    if value < 0 {
        tracing::warn!(
            "Received negative token count from API: {}. Clamping to 0.",
            value
        );
        Ok(0)
    } else if value > u32::MAX as i64 {
        tracing::warn!(
            "Token count exceeds u32::MAX: {}. Clamping to u32::MAX.",
            value
        );
        Ok(u32::MAX)
    } else {
        Ok(value as u32)
    }
}

/// Deserializes an optional token count as `Option<u32>`, warning if negative.
fn deserialize_optional_token_count<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<i64> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(v) if v < 0 => {
            tracing::warn!(
                "Received negative token count from API: {}. Clamping to 0.",
                v
            );
            Ok(Some(0))
        }
        Some(v) if v > u32::MAX as i64 => {
            tracing::warn!("Token count exceeds u32::MAX: {}. Clamping to u32::MAX.", v);
            Ok(Some(u32::MAX))
        }
        Some(v) => Ok(Some(v as u32)),
    }
}

/// Status of an interaction.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New status values may be added by the API in future versions.
///
/// # Unknown Status Handling
///
/// When the API returns a status value that this library doesn't recognize,
/// it will be captured in the `Unknown` variant with the original status
/// string preserved. This follows the Evergreen philosophy of graceful
/// degradation and data preservation.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum InteractionStatus {
    /// Interaction completed successfully.
    Completed,
    /// Interaction is still being processed.
    ///
    /// This is the `Default` (used when a hand-constructed response omits a
    /// status; the wire always carries one).
    #[default]
    InProgress,
    /// Interaction requires client action (e.g., function results).
    RequiresAction,
    /// Interaction failed.
    Failed,
    /// Interaction was cancelled.
    Cancelled,
    /// Interaction ended before completion (e.g., token limit reached).
    Incomplete,
    /// Interaction stopped because the configured budget was exceeded.
    BudgetExceeded,
    /// Unknown status (for forward compatibility).
    ///
    /// This variant captures any unrecognized status values from the API,
    /// allowing the library to handle new statuses gracefully.
    ///
    /// The `status_type` field contains the unrecognized status string,
    /// and `data` contains the JSON value (typically the same string).
    Unknown {
        /// The unrecognized status string from the API
        status_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl InteractionStatus {
    /// Check if this is an unknown status.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the status type name if this is an unknown status.
    ///
    /// Returns `None` for known statuses.
    #[must_use]
    pub fn unknown_status_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { status_type, .. } => Some(status_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown status.
    ///
    /// Returns `None` for known statuses.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for InteractionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Completed => serializer.serialize_str("completed"),
            Self::InProgress => serializer.serialize_str("in_progress"),
            Self::RequiresAction => serializer.serialize_str("requires_action"),
            Self::Failed => serializer.serialize_str("failed"),
            Self::Cancelled => serializer.serialize_str("cancelled"),
            Self::Incomplete => serializer.serialize_str("incomplete"),
            Self::BudgetExceeded => serializer.serialize_str("budget_exceeded"),
            Self::Unknown { status_type, .. } => serializer.serialize_str(status_type),
        }
    }
}

impl<'de> Deserialize<'de> for InteractionStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.as_str() {
            Some("completed") => Ok(Self::Completed),
            Some("in_progress") => Ok(Self::InProgress),
            Some("requires_action") => Ok(Self::RequiresAction),
            Some("failed") => Ok(Self::Failed),
            Some("cancelled") => Ok(Self::Cancelled),
            Some("incomplete") => Ok(Self::Incomplete),
            Some("budget_exceeded") => Ok(Self::BudgetExceeded),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown InteractionStatus '{}'. \
                     This may indicate a new API feature. \
                     The status will be preserved in the Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    status_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                // Non-string value - preserve it in Unknown
                let status_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "InteractionStatus received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    status_type,
                    data: value,
                })
            }
        }
    }
}

/// Token count for a specific modality.
///
/// Used in per-modality breakdowns like [`UsageMetadata::input_tokens_by_modality`].
///
/// # Example
///
/// ```no_run
/// # use genai_rs::UsageMetadata;
/// # let usage: UsageMetadata = Default::default();
/// if let Some(breakdown) = &usage.input_tokens_by_modality {
///     for modality_tokens in breakdown {
///         println!("{}: {} tokens", modality_tokens.modality, modality_tokens.tokens);
///     }
/// }
/// ```
#[derive(Clone, Deserialize, Serialize, Debug, PartialEq)]
pub struct ModalityTokens {
    /// The modality type (e.g., "text", "image", "audio").
    ///
    /// Uses string for forward compatibility with new modalities per Evergreen principles.
    pub modality: String,
    /// Token count for this modality.
    ///
    /// Uses `u32` since token counts are never negative. If the API returns a negative
    /// value (which would be a bug), it's clamped to 0 with a warning log.
    #[serde(deserialize_with = "deserialize_token_count")]
    pub tokens: u32,
}

/// Per-tool grounding invocation count.
///
/// Reported in [`UsageMetadata::grounding_tool_count`]. Known `tool_type`
/// values are `google_search`, `google_maps`, and `retrieval`; the field is a
/// plain string for Evergreen forward compatibility.
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct GroundingToolCount {
    /// The grounding tool type (wire field: `type`).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,
    /// Number of invocations of this tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// Token usage information from the Interactions API.
///
/// All token counts use `u32` since they're never negative. If the API returns
/// a negative value (which would be a bug), it's clamped to 0 with a warning log.
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
#[serde(default)]
pub struct UsageMetadata {
    /// Total number of input tokens (prompt tokens sent to the model)
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_input_tokens: Option<u32>,
    /// Total number of output tokens (tokens generated by the model)
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_output_tokens: Option<u32>,
    /// Total number of tokens (input + output)
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_tokens: Option<u32>,
    /// Total number of cached tokens (from context caching, reduces billing)
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_cached_tokens: Option<u32>,
    /// Total number of thought tokens (thinking model internal reasoning)
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_thought_tokens: Option<u32>,
    /// Total number of tokens used for tool/function calling overhead
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_tool_use_tokens: Option<u32>,

    // =========================================================================
    // Per-Modality Breakdowns
    // =========================================================================
    /// Input token counts broken down by modality (text, image, audio).
    ///
    /// Useful for understanding cost distribution in multi-modal prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens_by_modality: Option<Vec<ModalityTokens>>,

    /// Output token counts broken down by modality.
    ///
    /// Useful for understanding output cost distribution in multi-modal responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_by_modality: Option<Vec<ModalityTokens>>,

    /// Cached token counts broken down by modality.
    ///
    /// Shows which modalities benefit from context caching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens_by_modality: Option<Vec<ModalityTokens>>,

    /// Tool use token counts broken down by modality.
    ///
    /// Shows tool invocation overhead per modality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_tokens_by_modality: Option<Vec<ModalityTokens>>,

    /// Per-tool grounding invocation counts (e.g., how many Google Search
    /// calls were made while grounding this interaction).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_tool_count: Option<Vec<GroundingToolCount>>,
}

impl UsageMetadata {
    /// Returns true if any usage data is present
    #[must_use]
    pub fn has_data(&self) -> bool {
        self.total_tokens.is_some()
            || self.total_input_tokens.is_some()
            || self.total_output_tokens.is_some()
            || self.total_cached_tokens.is_some()
            || self.total_thought_tokens.is_some()
            || self.total_tool_use_tokens.is_some()
            || self.input_tokens_by_modality.is_some()
            || self.output_tokens_by_modality.is_some()
            || self.cached_tokens_by_modality.is_some()
            || self.tool_use_tokens_by_modality.is_some()
            || self.grounding_tool_count.is_some()
    }

    /// Returns total thought tokens (thinking model internal reasoning)
    #[must_use]
    pub fn thought_tokens(&self) -> Option<u32> {
        self.total_thought_tokens
    }

    /// Returns the input token count for a specific modality.
    ///
    /// # Arguments
    ///
    /// * `modality` - The modality name (e.g., "text", "image", "audio")
    ///
    /// # Returns
    ///
    /// The token count for the specified modality, or `None` if the modality
    /// is not present in the breakdown or if modality data is unavailable.
    #[must_use]
    pub fn input_tokens_for_modality(&self, modality: &str) -> Option<u32> {
        self.input_tokens_by_modality
            .as_ref()?
            .iter()
            .find(|m| m.modality == modality)
            .map(|m| m.tokens)
    }

    /// Returns the grounding invocation count for a specific tool type.
    ///
    /// # Arguments
    ///
    /// * `tool_type` - The tool type (e.g., "google_search", "google_maps", "retrieval")
    #[must_use]
    pub fn grounding_count_for_tool(&self, tool_type: &str) -> Option<u32> {
        self.grounding_tool_count
            .as_ref()?
            .iter()
            .find(|g| g.tool_type.as_deref() == Some(tool_type))
            .and_then(|g| g.count)
    }

    /// Returns the cache hit rate as a fraction (0.0 to 1.0).
    ///
    /// The cache hit rate is the ratio of cached tokens to total input tokens.
    /// A higher rate indicates better cache utilization and lower costs.
    ///
    /// # Returns
    ///
    /// - `Some(rate)` where `rate` is between 0.0 and 1.0
    /// - `None` if either `total_cached_tokens` or `total_input_tokens` is unavailable,
    ///   or if `total_input_tokens` is zero
    #[must_use]
    pub fn cache_hit_rate(&self) -> Option<f32> {
        let cached = self.total_cached_tokens? as f32;
        let total = self.total_input_tokens? as f32;
        if total > 0.0 {
            Some(cached / total)
        } else {
            None
        }
    }

    /// Accumulates usage from another `UsageMetadata` into this one.
    ///
    /// This is useful for aggregating token counts across multiple API calls,
    /// such as in auto-function calling loops where each iteration reports
    /// its own usage.
    ///
    /// For each field, if the other has a value:
    /// - If self has a value, adds the other's value
    /// - If self has None, takes the other's value
    ///
    /// Note: `*_by_modality` and `grounding_tool_count` fields are not
    /// accumulated (would require complex merging).
    pub(crate) fn accumulate(&mut self, other: &UsageMetadata) {
        fn add_option(a: &mut Option<u32>, b: Option<u32>) {
            if let Some(b_val) = b {
                *a = Some(a.unwrap_or(0).saturating_add(b_val));
            }
        }

        add_option(&mut self.total_input_tokens, other.total_input_tokens);
        add_option(&mut self.total_output_tokens, other.total_output_tokens);
        add_option(&mut self.total_tokens, other.total_tokens);
        add_option(&mut self.total_cached_tokens, other.total_cached_tokens);
        add_option(&mut self.total_thought_tokens, other.total_thought_tokens);
        add_option(&mut self.total_tool_use_tokens, other.total_tool_use_tokens);
        // Note: *_by_modality and grounding_tool_count fields are not
        // accumulated as they would require complex merging logic.
    }
}

// =============================================================================
// Image Info Type
// =============================================================================

/// Information about an image in the response.
///
/// This is a view type that provides convenient access to image data
/// in the response, with automatic base64 decoding.
///
/// # Example
///
/// ```no_run
/// use genai_rs::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("api-key".to_string());
///
/// let response = client
///     .interaction()
///     .with_model("gemini-3-flash-preview")
///     .with_text("A cat playing with yarn")
///     .with_image_output()
///     .create()
///     .await?;
///
/// for image in response.images() {
///     let bytes = image.bytes()?;
///     let filename = format!("image.{}", image.extension());
///     std::fs::write(&filename, bytes)?;
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ImageInfo<'a> {
    data: &'a str,
    mime_type: Option<&'a str>,
}

impl ImageInfo<'_> {
    /// Decodes and returns the image bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 data is invalid.
    #[must_use = "this `Result` should be used to handle potential decode errors"]
    pub fn bytes(&self) -> Result<Vec<u8>, GenaiError> {
        base64::engine::general_purpose::STANDARD
            .decode(self.data)
            .map_err(|e| GenaiError::InvalidInput(format!("Invalid base64 image data: {}", e)))
    }

    /// Returns the MIME type of the image, if available.
    #[must_use]
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type
    }

    /// Returns a file extension suitable for this image's MIME type.
    ///
    /// Returns "png" as default if MIME type is unknown or unrecognized.
    /// Logs a warning for unrecognized MIME types to surface API evolution
    /// (following the project's Evergreen philosophy).
    #[must_use]
    pub fn extension(&self) -> &str {
        match self.mime_type {
            Some("image/jpeg") | Some("image/jpg") => "jpg",
            Some("image/png") => "png",
            Some("image/webp") => "webp",
            Some("image/gif") => "gif",
            Some(unknown) => {
                tracing::warn!(
                    "Unknown image MIME type '{}', defaulting to 'png' extension. \
                     Consider updating genai-rs to handle this type.",
                    unknown
                );
                "png"
            }
            None => "png", // No MIME type provided, default to png
        }
    }
}

// =============================================================================
// Audio Info Type
// =============================================================================

/// Information about audio content in the response.
///
/// This is a view type that provides convenient access to audio data
/// in the response, with automatic base64 decoding.
///
/// # Example
///
/// ```no_run
/// use genai_rs::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("api-key".to_string());
///
/// let response = client
///     .interaction()
///     .with_model("gemini-2.5-pro-preview-tts")
///     .with_text("Hello, world!")
///     .with_audio_output()
///     .with_voice("Kore")
///     .create()
///     .await?;
///
/// for audio in response.audios() {
///     let bytes = audio.bytes()?;
///     let filename = format!("audio.{}", audio.extension());
///     std::fs::write(&filename, bytes)?;
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct AudioInfo<'a> {
    data: &'a str,
    mime_type: Option<&'a str>,
    sample_rate: Option<u32>,
    channels: Option<u32>,
}

impl AudioInfo<'_> {
    /// Decodes and returns the audio bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 data is invalid.
    #[must_use = "this `Result` should be used to handle potential decode errors"]
    pub fn bytes(&self) -> Result<Vec<u8>, GenaiError> {
        base64::engine::general_purpose::STANDARD
            .decode(self.data)
            .map_err(|e| GenaiError::InvalidInput(format!("Invalid base64 audio data: {}", e)))
    }

    /// Returns the MIME type of the audio, if available.
    #[must_use]
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type
    }

    /// Returns the sample rate in Hz, if reported by the API.
    #[must_use]
    pub fn sample_rate(&self) -> Option<u32> {
        self.sample_rate
    }

    /// Returns the number of audio channels, if reported by the API.
    #[must_use]
    pub fn channels(&self) -> Option<u32> {
        self.channels
    }

    /// Returns a file extension suitable for this audio's MIME type.
    ///
    /// Returns "wav" as default if MIME type is unknown or unrecognized.
    /// Logs a warning for unrecognized MIME types to surface API evolution
    /// (following the project's Evergreen philosophy).
    #[must_use]
    pub fn extension(&self) -> &str {
        match self.mime_type {
            Some("audio/wav") | Some("audio/x-wav") => "wav",
            Some("audio/mp3") | Some("audio/mpeg") => "mp3",
            Some("audio/ogg") => "ogg",
            Some("audio/flac") => "flac",
            Some("audio/aac") => "aac",
            Some("audio/webm") => "webm",
            // PCM/L16 format from TTS - raw audio data
            Some(mime) if mime.starts_with("audio/L16") || mime.starts_with("audio/l16") => "pcm",
            Some(unknown) => {
                tracing::warn!(
                    "Unknown audio MIME type '{}', defaulting to 'wav' extension. \
                     Consider updating genai-rs to handle this type.",
                    unknown
                );
                "wav"
            }
            None => "wav", // No MIME type provided, default to wav
        }
    }
}

// =============================================================================
// Function Call/Result Info Types
// =============================================================================

/// Information about a function call requested by the model.
///
/// Returned by [`InteractionResponse::function_calls()`] for convenient access
/// to function call details.
///
/// This is a **view type** that borrows data from the underlying [`InteractionResponse`].
/// It implements [`Serialize`] for logging and debugging purposes, but not `Deserialize`
/// since it's not meant to be constructed directly—use the response helper methods instead.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// for call in response.function_calls() {
///     println!("Function: {} ({}) with args: {}", call.name, call.id, call.args);
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionCallInfo<'a> {
    /// Unique identifier for this function call (used when sending results back)
    pub id: &'a str,
    /// Name of the function to call
    pub name: &'a str,
    /// Arguments to pass to the function
    pub args: &'a serde_json::Value,
}

impl FunctionCallInfo<'_> {
    /// Convert to an owned version that doesn't borrow from the response.
    ///
    /// Use this when you need to store function call data beyond the lifetime
    /// of the response, such as for event emission, trajectory recording,
    /// or passing to async tasks.
    #[must_use]
    pub fn to_owned(&self) -> OwnedFunctionCallInfo {
        OwnedFunctionCallInfo {
            id: self.id.to_string(),
            name: self.name.to_string(),
            args: self.args.clone(),
        }
    }
}

/// Owned version of [`FunctionCallInfo`] for storing beyond response lifetime.
///
/// This type owns all its data, making it suitable for:
/// - Event emission with function call metadata
/// - Trajectory/replay recording
/// - Passing to async tasks or storing in collections
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedFunctionCallInfo {
    /// Unique identifier for this function call (used when sending results back)
    pub id: String,
    /// Name of the function to call
    pub name: String,
    /// Arguments to pass to the function
    pub args: serde_json::Value,
}

/// Information about a function result in the response.
///
/// Returned by [`InteractionResponse::function_results()`] for convenient access
/// to function result details.
///
/// This is a **view type** that borrows data from the underlying [`InteractionResponse`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionResultInfo<'a> {
    /// Name of the function that was called (optional per API spec)
    pub name: Option<&'a str>,
    /// The call_id from the FunctionCall this result responds to
    pub call_id: &'a str,
    /// The result returned by the function
    pub result: &'a FunctionResultPayload,
    /// Whether this result indicates an error
    pub is_error: Option<bool>,
}

/// Information about a code execution call requested by the model.
///
/// Returned by [`InteractionResponse::code_execution_calls()`] for convenient access
/// to code execution details.
///
/// This is a **view type** that borrows data from the underlying [`InteractionResponse`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CodeExecutionCallInfo<'a> {
    /// Unique identifier for this code execution call
    pub id: &'a str,
    /// Programming language (currently only Python is supported)
    pub language: CodeExecutionLanguage,
    /// Source code to execute
    pub code: &'a str,
}

/// Information about a code execution result.
///
/// Returned by [`InteractionResponse::code_execution_results()`] for convenient access
/// to code execution results.
///
/// This is a **view type** that borrows data from the underlying [`InteractionResponse`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CodeExecutionResultInfo<'a> {
    /// The call_id matching the CodeExecutionCall this result is for
    pub call_id: &'a str,
    /// Whether the code execution resulted in an error
    pub is_error: bool,
    /// The output of the code execution (stdout for success, error message for failure)
    pub result: &'a str,
}

/// Information about a URL context result.
///
/// Returned by [`InteractionResponse::url_context_results()`] for convenient access
/// to URL context results.
///
/// This is a **view type** that borrows data from the underlying [`InteractionResponse`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct UrlContextResultInfo<'a> {
    /// The ID of the corresponding UrlContextCall
    pub call_id: &'a str,
    /// The result items containing URL and status for each fetched URL
    pub items: &'a [crate::UrlContextResultItem],
}

/// Information about a Google Maps result.
///
/// Returned by [`InteractionResponse::google_maps_results()`] for convenient access
/// to Google Maps results with place data.
///
/// This is a **view type** that borrows data from the underlying [`InteractionResponse`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct GoogleMapsResultInfo<'a> {
    /// The ID of the corresponding Google Maps call
    pub call_id: &'a str,
    /// The result items containing place data
    pub items: &'a [crate::GoogleMapsResultItem],
}

/// Response from creating or retrieving an interaction.
///
/// Under API revision 2026-05-20 the response carries a `steps` array; use the
/// convenience helpers (`as_text()`, `function_calls()`, `images()`, ...) or
/// iterate [`InteractionResponse::steps`] directly.
#[derive(Clone, Deserialize, Serialize, Debug, Default)]
#[serde(default)]
pub struct InteractionResponse {
    /// Unique identifier for this interaction.
    ///
    /// This field is `None` when the interaction was created with `store=false`,
    /// since non-stored interactions are not assigned an ID by the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Model name if a model was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Agent name if an agent was used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// The input that was provided.
    ///
    /// Only populated when the interaction is retrieved with
    /// `include_input=true` (see [`Client::get_interaction_with_input`](crate::Client::get_interaction_with_input)).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InteractionInput>,

    /// The steps produced during this interaction (revision 2026-05-20).
    ///
    /// Replaces the launch-era `outputs` array. Model content is nested in
    /// `model_output` steps; tool calls and results are typed steps.
    pub steps: Vec<Step>,

    /// Current status of the interaction
    pub status: InteractionStatus,

    /// Token usage information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,

    /// Tools that were available for this interaction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Previous interaction ID if this was a follow-up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,

    /// ID of the environment this interaction executed in, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,

    /// Convenience field: concatenated output text, when provided by the API.
    ///
    /// Prefer [`as_text()`](Self::as_text) / [`all_text()`](Self::all_text),
    /// which fall back to this field when steps are absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_text: Option<String>,

    /// Timestamp when the interaction was created (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    /// Timestamp when the interaction was last updated (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<DateTime<Utc>>,
}

impl InteractionResponse {
    // =========================================================================
    // Step / Content Iteration Helpers
    // =========================================================================

    /// Iterates over all content blocks in `model_output` steps.
    ///
    /// This is the step-model equivalent of iterating the launch-era
    /// `outputs` array.
    pub fn output_contents(&self) -> impl Iterator<Item = &Content> {
        self.steps.iter().flat_map(|step| match step {
            Step::ModelOutput { content, .. } => content.as_slice(),
            _ => &[],
        })
    }

    /// Returns the steps as owned values, suitable for replaying as
    /// conversation history in a stateless follow-up request.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Client, Step};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("key".to_string());
    /// let first = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What is 2+2?")
    ///     .create().await?;
    ///
    /// let mut history = vec![Step::user_text("What is 2+2?")];
    /// history.extend(first.output_steps());
    /// history.push(Step::user_text("Now multiply that by 3"));
    ///
    /// let second = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_history(history)
    ///     .create().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn output_steps(&self) -> Vec<Step> {
        self.steps.clone()
    }

    // =========================================================================
    // Text Content Helpers
    // =========================================================================

    /// Extract the first text content from the model output steps.
    ///
    /// Falls back to the API-provided `output_text` convenience field when no
    /// text-bearing steps are present.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(text) = response.as_text() {
    ///     println!("Response: {}", text);
    /// }
    /// ```
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        self.output_contents()
            .find_map(Content::as_text)
            .or(self.output_text.as_deref())
    }

    /// Extract all text contents concatenated.
    ///
    /// Combines all text blocks from model output steps into a single string.
    #[must_use]
    pub fn all_text(&self) -> String {
        let text: String = self
            .output_contents()
            .filter_map(Content::as_text)
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            self.output_text.clone().unwrap_or_default()
        } else {
            text
        }
    }

    /// Check if response contains text
    #[must_use]
    pub fn has_text(&self) -> bool {
        self.output_contents().any(|c| c.as_text().is_some()) || self.output_text.is_some()
    }

    // =========================================================================
    // Annotation Helpers (Citation Support)
    // =========================================================================

    /// Check if response contains annotations (citations).
    ///
    /// Returns `true` if any model output text contains source annotations.
    /// Annotations are typically present when grounding tools like
    /// `GoogleSearch` or `UrlContext` were used.
    #[must_use]
    pub fn has_annotations(&self) -> bool {
        self.output_contents().any(|c| c.annotations().is_some())
    }

    /// Returns all annotations from model output text.
    ///
    /// Collects all [`Annotation`] references from all text blocks in the
    /// response. Annotations link specific text spans to their sources,
    /// enabling citation tracking.
    pub fn all_annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.output_contents()
            .filter_map(|c| c.annotations())
            .flatten()
    }

    // =========================================================================
    // Image Content Helpers
    // =========================================================================

    /// Returns the decoded bytes of the first image in the response.
    ///
    /// This is a convenience method for the common case of extracting a single
    /// generated image. For multiple images, use [`images()`](Self::images).
    ///
    /// # Errors
    ///
    /// Returns an error if the base64 data is invalid.
    pub fn first_image_bytes(&self) -> Result<Option<Vec<u8>>, GenaiError> {
        for content in self.output_contents() {
            if let Content::Image {
                data: Some(base64_data),
                ..
            } = content
            {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(base64_data)
                    .map_err(|e| {
                        GenaiError::MalformedResponse(format!("Invalid base64 image data: {}", e))
                    })?;
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    /// Returns an iterator over all images in the response.
    ///
    /// Each item is an [`ImageInfo`] that provides access to the image data,
    /// MIME type, and convenience methods for decoding.
    pub fn images(&self) -> impl Iterator<Item = ImageInfo<'_>> {
        self.output_contents().filter_map(|content| {
            if let Content::Image {
                data: Some(base64_data),
                mime_type,
                ..
            } = content
            {
                Some(ImageInfo {
                    data: base64_data.as_str(),
                    mime_type: mime_type.as_deref(),
                })
            } else {
                None
            }
        })
    }

    /// Check if the response contains any images.
    #[must_use]
    pub fn has_images(&self) -> bool {
        self.output_contents()
            .any(|c| matches!(c, Content::Image { data: Some(_), .. }))
    }

    // =========================================================================
    // Audio Helpers
    // =========================================================================

    /// Returns the first audio content in the response.
    ///
    /// This is a convenience method for the common case of extracting a single
    /// generated audio. For multiple audio outputs, use [`audios()`](Self::audios).
    #[must_use]
    pub fn first_audio(&self) -> Option<AudioInfo<'_>> {
        self.audios().next()
    }

    /// Returns an iterator over all audio content in the response.
    ///
    /// Each [`AudioInfo`] provides methods for accessing the audio data,
    /// MIME type, sample rate, channels, and a suitable file extension.
    pub fn audios(&self) -> impl Iterator<Item = AudioInfo<'_>> {
        self.output_contents().filter_map(|content| {
            if let Content::Audio {
                data: Some(base64_data),
                mime_type,
                sample_rate,
                channels,
                ..
            } = content
            {
                Some(AudioInfo {
                    data: base64_data.as_str(),
                    mime_type: mime_type.as_deref(),
                    sample_rate: *sample_rate,
                    channels: *channels,
                })
            } else {
                None
            }
        })
    }

    /// Check if the response contains any audio content.
    #[must_use]
    pub fn has_audio(&self) -> bool {
        self.output_contents()
            .any(|c| matches!(c, Content::Audio { data: Some(_), .. }))
    }

    // =========================================================================
    // Function Calling Helpers
    // =========================================================================

    /// Extract function calls from steps.
    ///
    /// Returns a vector of [`FunctionCallInfo`] structs with named fields for
    /// convenient access to function call details.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for call in response.function_calls() {
    ///     println!("Function: {} with args: {}", call.name, call.args);
    ///     // Use call.id when sending results back to the model
    /// }
    /// ```
    #[must_use]
    pub fn function_calls(&self) -> Vec<FunctionCallInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::FunctionCall {
                    id,
                    name,
                    arguments,
                } = step
                {
                    Some(FunctionCallInfo {
                        id: id.as_str(),
                        name: name.as_str(),
                        args: arguments,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if response contains function calls
    #[must_use]
    pub fn has_function_calls(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::FunctionCall { .. }))
    }

    /// Check if response contains function results
    #[must_use]
    pub fn has_function_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::FunctionResult { .. }))
    }

    /// Extract function results from steps.
    #[must_use]
    pub fn function_results(&self) -> Vec<FunctionResultInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::FunctionResult {
                    name,
                    call_id,
                    result,
                    is_error,
                } = step
                {
                    Some(FunctionResultInfo {
                        name: name.as_deref(),
                        call_id: call_id.as_str(),
                        result,
                        is_error: *is_error,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    // =========================================================================
    // Thinking/Reasoning Helpers
    // =========================================================================

    /// Check if response contains thought steps with signatures.
    #[must_use]
    pub fn has_thoughts(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(
                s,
                Step::Thought {
                    signature: Some(_),
                    ..
                }
            )
        })
    }

    /// Get an iterator over all thought signatures.
    ///
    /// Signatures are opaque values validating the model's reasoning process;
    /// pass them back unchanged when replaying history statelessly.
    pub fn thought_signatures(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().filter_map(|s| match s {
            Step::Thought {
                signature: Some(sig),
                ..
            } => Some(sig.as_str()),
            _ => None,
        })
    }

    /// Get an iterator over all thought summary content blocks.
    ///
    /// Populated when thinking summaries are enabled
    /// (`with_thinking_summaries(ThinkingSummaries::Auto)`).
    pub fn thought_summaries(&self) -> impl Iterator<Item = &Content> {
        self.steps.iter().flat_map(|s| match s {
            Step::Thought { summary, .. } => summary.as_slice(),
            _ => &[],
        })
    }

    // =========================================================================
    // Unknown Step Helpers (Evergreen Forward Compatibility)
    // =========================================================================

    /// Check if response contains unknown step types.
    ///
    /// Returns `true` if any step is a [`Step::Unknown`] variant, indicating
    /// the API returned step types this library version doesn't recognize.
    #[must_use]
    pub fn has_unknown(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Unknown { .. }))
    }

    /// Get all unknown steps as (step_type, data) tuples.
    #[must_use]
    pub fn unknown_steps(&self) -> Vec<(&str, &serde_json::Value)> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::Unknown { step_type, data } = step {
                    Some((step_type.as_str(), data))
                } else {
                    None
                }
            })
            .collect()
    }

    // =========================================================================
    // Code Execution Tool Helpers
    // =========================================================================

    /// Check if response contains code execution calls
    #[must_use]
    pub fn has_code_execution_calls(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::CodeExecutionCall { .. }))
    }

    /// Get the first code execution call, if any.
    #[must_use]
    pub fn code_execution_call(&self) -> Option<CodeExecutionCallInfo<'_>> {
        self.code_execution_calls().into_iter().next()
    }

    /// Extract all code execution calls from steps.
    #[must_use]
    pub fn code_execution_calls(&self) -> Vec<CodeExecutionCallInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::CodeExecutionCall {
                    id, language, code, ..
                } = step
                {
                    Some(CodeExecutionCallInfo {
                        id: id.as_str(),
                        language: language.clone(),
                        code: code.as_str(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if response contains code execution results
    #[must_use]
    pub fn has_code_execution_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::CodeExecutionResult { .. }))
    }

    /// Extract code execution results from steps.
    #[must_use]
    pub fn code_execution_results(&self) -> Vec<CodeExecutionResultInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::CodeExecutionResult {
                    call_id,
                    is_error,
                    result,
                    ..
                } = step
                {
                    Some(CodeExecutionResultInfo {
                        call_id: call_id.as_str(),
                        is_error: *is_error,
                        result: result.as_str(),
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the first successful code execution output, if any.
    #[must_use]
    pub fn successful_code_output(&self) -> Option<&str> {
        self.steps.iter().find_map(|step| {
            if let Step::CodeExecutionResult {
                is_error: false,
                result,
                ..
            } = step
            {
                Some(result.as_str())
            } else {
                None
            }
        })
    }

    // =========================================================================
    // Google Search Step Helpers
    // =========================================================================

    /// Check if response contains Google Search calls
    #[must_use]
    pub fn has_google_search_calls(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::GoogleSearchCall { .. }))
    }

    /// Get the first Google Search query, if any.
    #[must_use]
    pub fn google_search_call(&self) -> Option<&str> {
        self.steps.iter().find_map(|step| {
            if let Step::GoogleSearchCall { queries, .. } = step {
                queries.iter().find(|q| !q.is_empty()).map(|q| q.as_str())
            } else {
                None
            }
        })
    }

    /// Extract all Google Search queries from steps (flattened across calls).
    #[must_use]
    pub fn google_search_calls(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::GoogleSearchCall { queries, .. } = step {
                    Some(queries.iter().map(|q| q.as_str()))
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    /// Check if response contains Google Search results
    #[must_use]
    pub fn has_google_search_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::GoogleSearchResult { .. }))
    }

    /// Extract Google Search result items from steps.
    #[must_use]
    pub fn google_search_results(&self) -> Vec<&GoogleSearchResultItem> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::GoogleSearchResult { result, .. } = step {
                    Some(result.iter())
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    // =========================================================================
    // URL Context Step Helpers
    // =========================================================================

    /// Check if response contains URL context calls
    #[must_use]
    pub fn has_url_context_calls(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::UrlContextCall { .. }))
    }

    /// Get the ID of the first URL context call, if any.
    #[must_use]
    pub fn url_context_call_id(&self) -> Option<&str> {
        self.steps.iter().find_map(|step| {
            if let Step::UrlContextCall { id, .. } = step {
                Some(id.as_str())
            } else {
                None
            }
        })
    }

    /// Extract URL context call URLs from steps (flattened across calls).
    #[must_use]
    pub fn url_context_call_urls(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::UrlContextCall { urls, .. } = step {
                    Some(urls.iter().map(String::as_str))
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    /// Check if response contains URL context results
    #[must_use]
    pub fn has_url_context_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::UrlContextResult { .. }))
    }

    /// Extract URL context results from steps.
    #[must_use]
    pub fn url_context_results(&self) -> Vec<UrlContextResultInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::UrlContextResult {
                    call_id, result, ..
                } = step
                {
                    Some(UrlContextResultInfo {
                        call_id: call_id.as_str(),
                        items: result,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    // =========================================================================
    // File Search Step Helpers
    // =========================================================================

    /// Check if response contains file search results
    #[must_use]
    pub fn has_file_search_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::FileSearchResult { .. }))
    }

    /// Extract file search result items from steps.
    #[must_use]
    pub fn file_search_results(&self) -> Vec<&FileSearchResultItem> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::FileSearchResult { result, .. } = step {
                    Some(result.iter())
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    // =========================================================================
    // Google Maps Results
    // =========================================================================

    /// Returns `true` if the response contains Google Maps results.
    #[must_use]
    pub fn has_google_maps_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::GoogleMapsResult { .. }))
    }

    /// Extract Google Maps results from steps.
    #[must_use]
    pub fn google_maps_results(&self) -> Vec<GoogleMapsResultInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::GoogleMapsResult {
                    call_id, result, ..
                } = step
                {
                    Some(GoogleMapsResultInfo {
                        call_id: call_id.as_str(),
                        items: result,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    // =========================================================================
    // Summary and Diagnostics
    // =========================================================================

    /// Get a summary of step and content types present in the response.
    ///
    /// Returns a [`StepSummary`] with counts for each step type plus content
    /// block counts within `model_output` steps. Useful for debugging,
    /// logging, or detecting unexpected content.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// let summary = response.step_summary();
    /// println!("Response has {} text blocks", summary.text_count);
    /// if summary.unknown_count > 0 {
    ///     println!("Warning: {} unknown step types: {:?}",
    ///         summary.unknown_count, summary.unknown_types);
    /// }
    /// ```
    #[must_use]
    pub fn step_summary(&self) -> StepSummary {
        let mut summary = StepSummary::default();
        let mut unknown_types_set = BTreeSet::new();

        for step in &self.steps {
            match step {
                Step::UserInput { .. } => summary.user_input_count += 1,
                Step::ModelOutput { content, .. } => {
                    summary.model_output_count += 1;
                    for c in content {
                        match c {
                            Content::Text { .. } => summary.text_count += 1,
                            Content::Image { .. } => summary.image_count += 1,
                            Content::Audio { .. } => summary.audio_count += 1,
                            Content::Video { .. } => summary.video_count += 1,
                            Content::Document { .. } => summary.document_count += 1,
                            Content::Unknown { content_type, .. } => {
                                summary.unknown_count += 1;
                                unknown_types_set.insert(content_type.clone());
                            }
                            // Content is #[non_exhaustive]; count future
                            // variants as unknown-free content.
                            #[allow(unreachable_patterns)]
                            _ => {}
                        }
                    }
                }
                Step::Thought { .. } => summary.thought_count += 1,
                Step::FunctionCall { .. } => summary.function_call_count += 1,
                Step::FunctionResult { .. } => summary.function_result_count += 1,
                Step::CodeExecutionCall { .. } => summary.code_execution_call_count += 1,
                Step::CodeExecutionResult { .. } => summary.code_execution_result_count += 1,
                Step::GoogleSearchCall { .. } => summary.google_search_call_count += 1,
                Step::GoogleSearchResult { .. } => summary.google_search_result_count += 1,
                Step::UrlContextCall { .. } => summary.url_context_call_count += 1,
                Step::UrlContextResult { .. } => summary.url_context_result_count += 1,
                Step::McpServerToolCall { .. } => summary.mcp_server_tool_call_count += 1,
                Step::McpServerToolResult { .. } => summary.mcp_server_tool_result_count += 1,
                Step::FileSearchCall { .. } => summary.file_search_call_count += 1,
                Step::FileSearchResult { .. } => summary.file_search_result_count += 1,
                Step::GoogleMapsCall { .. } => summary.google_maps_call_count += 1,
                Step::GoogleMapsResult { .. } => summary.google_maps_result_count += 1,
                Step::Unknown { step_type, .. } => {
                    summary.unknown_count += 1;
                    unknown_types_set.insert(step_type.clone());
                }
            }
        }

        // BTreeSet maintains sorted order, so no need to sort
        summary.unknown_types = unknown_types_set.into_iter().collect();
        summary
    }

    // =========================================================================
    // Token Usage Helpers
    // =========================================================================

    /// Get the number of input (prompt) tokens used.
    ///
    /// Returns `None` if usage metadata is not available.
    #[must_use]
    pub fn input_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_input_tokens)
    }

    /// Get the number of output tokens generated.
    ///
    /// Returns `None` if usage metadata is not available.
    #[must_use]
    pub fn output_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_output_tokens)
    }

    /// Get the total number of tokens used (input + output).
    ///
    /// Returns `None` if usage metadata is not available.
    #[must_use]
    pub fn total_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_tokens)
    }

    /// Get the number of thought tokens used (for thinking models).
    ///
    /// Thought tokens are used when thinking mode is enabled
    /// (e.g., via `with_thinking_level()` on supported models).
    /// Returns `None` if usage metadata is not available or thinking wasn't used.
    #[must_use]
    pub fn thought_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_thought_tokens)
    }

    /// Get the number of cached tokens used (from context caching).
    ///
    /// Cached tokens reduce billing costs when reusing context.
    /// Returns `None` if usage metadata is not available or caching wasn't used.
    #[must_use]
    pub fn cached_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_cached_tokens)
    }

    /// Get the number of tool use tokens consumed.
    ///
    /// Tool use tokens represent overhead from function calling.
    /// Returns `None` if usage metadata is not available or tools weren't used.
    #[must_use]
    pub fn tool_use_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_tool_use_tokens)
    }

    // =========================================================================
    // Timestamp Helpers
    // =========================================================================

    /// Get the timestamp when this interaction was created.
    ///
    /// Returns `None` if the interaction was created with `store=false` or
    /// if the API didn't include timestamp information.
    #[must_use]
    pub fn created(&self) -> Option<DateTime<Utc>> {
        self.created
    }

    /// Get the timestamp when this interaction was last updated.
    ///
    /// Returns `None` if the interaction was created with `store=false` or
    /// if the API didn't include timestamp information.
    #[must_use]
    pub fn updated(&self) -> Option<DateTime<Utc>> {
        self.updated
    }
}

/// Summary of step and content types present in an interaction response.
///
/// Returned by [`InteractionResponse::step_summary`]. Provides a quick
/// overview of what step types are present, including any unknown types.
///
/// Content counts (`text_count`, `image_count`, ...) tally content blocks
/// inside `model_output` steps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepSummary {
    /// Number of `user_input` steps
    pub user_input_count: usize,
    /// Number of `model_output` steps
    pub model_output_count: usize,
    /// Number of text content blocks in model output
    pub text_count: usize,
    /// Number of image content blocks in model output
    pub image_count: usize,
    /// Number of audio content blocks in model output
    pub audio_count: usize,
    /// Number of video content blocks in model output
    pub video_count: usize,
    /// Number of document content blocks in model output
    pub document_count: usize,
    /// Number of `thought` steps
    pub thought_count: usize,
    /// Number of `function_call` steps
    pub function_call_count: usize,
    /// Number of `function_result` steps
    pub function_result_count: usize,
    /// Number of `code_execution_call` steps
    pub code_execution_call_count: usize,
    /// Number of `code_execution_result` steps
    pub code_execution_result_count: usize,
    /// Number of `google_search_call` steps
    pub google_search_call_count: usize,
    /// Number of `google_search_result` steps
    pub google_search_result_count: usize,
    /// Number of `url_context_call` steps
    pub url_context_call_count: usize,
    /// Number of `url_context_result` steps
    pub url_context_result_count: usize,
    /// Number of `mcp_server_tool_call` steps
    pub mcp_server_tool_call_count: usize,
    /// Number of `mcp_server_tool_result` steps
    pub mcp_server_tool_result_count: usize,
    /// Number of `file_search_call` steps
    pub file_search_call_count: usize,
    /// Number of `file_search_result` steps
    pub file_search_result_count: usize,
    /// Number of `google_maps_call` steps
    pub google_maps_call_count: usize,
    /// Number of `google_maps_result` steps
    pub google_maps_result_count: usize,
    /// Number of unknown steps/content blocks
    pub unknown_count: usize,
    /// List of unique unknown type names encountered (sorted alphabetically)
    pub unknown_types: Vec<String>,
}

impl fmt::Display for StepSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        let fields: [(&str, usize); 22] = [
            ("user_input", self.user_input_count),
            ("model_output", self.model_output_count),
            ("text", self.text_count),
            ("image", self.image_count),
            ("audio", self.audio_count),
            ("video", self.video_count),
            ("document", self.document_count),
            ("thought", self.thought_count),
            ("function_call", self.function_call_count),
            ("function_result", self.function_result_count),
            ("code_execution_call", self.code_execution_call_count),
            ("code_execution_result", self.code_execution_result_count),
            ("google_search_call", self.google_search_call_count),
            ("google_search_result", self.google_search_result_count),
            ("url_context_call", self.url_context_call_count),
            ("url_context_result", self.url_context_result_count),
            ("mcp_server_tool_call", self.mcp_server_tool_call_count),
            ("mcp_server_tool_result", self.mcp_server_tool_result_count),
            ("file_search_call", self.file_search_call_count),
            ("file_search_result", self.file_search_result_count),
            ("google_maps_call", self.google_maps_call_count),
            ("google_maps_result", self.google_maps_result_count),
        ];

        for (name, count) in fields {
            if count > 0 {
                parts.push(format!("{count} {name}"));
            }
        }

        if self.unknown_count > 0 {
            parts.push(format!(
                "{} unknown ({:?})",
                self.unknown_count, self.unknown_types
            ));
        }

        if parts.is_empty() {
            write!(f, "empty")
        } else {
            write!(f, "{}", parts.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_response(usage: Option<UsageMetadata>) -> InteractionResponse {
        InteractionResponse {
            status: InteractionStatus::Completed,
            usage,
            ..Default::default()
        }
    }

    fn text_response(text: &str) -> InteractionResponse {
        InteractionResponse {
            status: InteractionStatus::Completed,
            steps: vec![Step::model_text(text)],
            ..Default::default()
        }
    }

    #[test]
    fn test_token_helpers_with_usage() {
        let response = minimal_response(Some(UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            total_tokens: Some(150),
            total_cached_tokens: Some(25),
            total_thought_tokens: Some(10),
            total_tool_use_tokens: Some(5),
            ..Default::default()
        }));

        assert_eq!(response.input_tokens(), Some(100));
        assert_eq!(response.output_tokens(), Some(50));
        assert_eq!(response.total_tokens(), Some(150));
        assert_eq!(response.cached_tokens(), Some(25));
        assert_eq!(response.thought_tokens(), Some(10));
        assert_eq!(response.tool_use_tokens(), Some(5));
    }

    #[test]
    fn test_token_helpers_without_usage() {
        let response = minimal_response(None);

        assert_eq!(response.input_tokens(), None);
        assert_eq!(response.output_tokens(), None);
        assert_eq!(response.total_tokens(), None);
        assert_eq!(response.cached_tokens(), None);
        assert_eq!(response.thought_tokens(), None);
        assert_eq!(response.tool_use_tokens(), None);
    }

    // =========================================================================
    // Steps-based response deserialization (wire fixtures)
    // =========================================================================

    #[test]
    fn test_response_deserializes_steps_wire_fixture() {
        // Representative revision 2026-05-20 response shape.
        let json = r#"{
            "id": "interactions/abc123",
            "model": "gemini-3-flash-preview",
            "status": "completed",
            "steps": [
                {"type": "thought", "signature": "sig-1"},
                {"type": "model_output", "content": [
                    {"type": "text", "text": "The answer is 4."}
                ]}
            ],
            "usage": {
                "total_input_tokens": 10,
                "total_output_tokens": 8,
                "total_tokens": 18,
                "grounding_tool_count": [{"type": "google_search", "count": 2}]
            },
            "previous_interaction_id": "interactions/prev",
            "environment_id": "environments/env1",
            "created": "2026-05-21T10:00:00Z"
        }"#;

        let response: InteractionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id.as_deref(), Some("interactions/abc123"));
        assert_eq!(response.status, InteractionStatus::Completed);
        assert_eq!(response.steps.len(), 2);
        assert_eq!(response.as_text(), Some("The answer is 4."));
        assert_eq!(
            response.thought_signatures().collect::<Vec<_>>(),
            vec!["sig-1"]
        );
        assert_eq!(
            response.environment_id.as_deref(),
            Some("environments/env1")
        );
        let usage = response.usage.as_ref().unwrap();
        assert_eq!(usage.grounding_count_for_tool("google_search"), Some(2));
        assert!(response.created().is_some());
    }

    #[test]
    fn test_response_serializes_snake_case() {
        let response = InteractionResponse {
            previous_interaction_id: Some("interactions/prev".into()),
            status: InteractionStatus::Completed,
            ..Default::default()
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["previous_interaction_id"], "interactions/prev");
        assert!(json.get("previousInteractionId").is_none());
    }

    #[test]
    fn test_budget_exceeded_status_roundtrip() {
        let status: InteractionStatus = serde_json::from_str("\"budget_exceeded\"").unwrap();
        assert_eq!(status, InteractionStatus::BudgetExceeded);
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            "\"budget_exceeded\""
        );
    }

    #[test]
    fn test_function_calls_over_steps() {
        let response = InteractionResponse {
            status: InteractionStatus::RequiresAction,
            steps: vec![
                Step::thought("sig"),
                Step::function_call(
                    "call_1",
                    "get_weather",
                    serde_json::json!({"city": "Tokyo"}),
                ),
            ],
            ..Default::default()
        };
        let calls = response.function_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "get_weather");
        assert_eq!(calls[0].args["city"], "Tokyo");
        assert!(response.has_function_calls());
        assert!(response.has_thoughts());
    }

    #[test]
    fn test_as_text_falls_back_to_output_text() {
        let response = InteractionResponse {
            status: InteractionStatus::Completed,
            output_text: Some("fallback".into()),
            ..Default::default()
        };
        assert_eq!(response.as_text(), Some("fallback"));
        assert_eq!(response.all_text(), "fallback");
        assert!(response.has_text());
    }

    #[test]
    fn test_step_summary_counts() {
        let response = InteractionResponse {
            status: InteractionStatus::Completed,
            steps: vec![
                Step::user_text("hi"),
                Step::thought("sig"),
                Step::model_output(vec![Content::text("a"), Content::text("b")]),
                Step::Unknown {
                    step_type: "future".into(),
                    data: serde_json::Value::Null,
                },
            ],
            ..Default::default()
        };
        let summary = response.step_summary();
        assert_eq!(summary.user_input_count, 1);
        assert_eq!(summary.model_output_count, 1);
        assert_eq!(summary.text_count, 2);
        assert_eq!(summary.thought_count, 1);
        assert_eq!(summary.unknown_count, 1);
        assert_eq!(summary.unknown_types, vec!["future".to_string()]);
        let display = summary.to_string();
        assert!(display.contains("2 text"));
        assert!(display.contains("1 unknown"));
    }

    #[test]
    fn test_unknown_steps_helper() {
        let response = InteractionResponse {
            status: InteractionStatus::Completed,
            steps: vec![Step::Unknown {
                step_type: "quantum".into(),
                data: serde_json::json!({"type": "quantum", "x": 1}),
            }],
            ..Default::default()
        };
        assert!(response.has_unknown());
        let unknown = response.unknown_steps();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].0, "quantum");
    }

    // =========================================================================
    // ModalityTokens / GroundingToolCount
    // =========================================================================

    #[test]
    fn test_modality_tokens_serialization() {
        let tokens = ModalityTokens {
            modality: "text".to_string(),
            tokens: 100,
        };

        let json = serde_json::to_string(&tokens).unwrap();
        assert!(json.contains("\"modality\":\"text\""));
        assert!(json.contains("\"tokens\":100"));

        // Roundtrip
        let deserialized: ModalityTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.modality, "text");
        assert_eq!(deserialized.tokens, 100);
    }

    #[test]
    fn test_grounding_tool_count_wire_format() {
        let json = r#"{"type": "google_maps", "count": 3}"#;
        let count: GroundingToolCount = serde_json::from_str(json).unwrap();
        assert_eq!(count.tool_type.as_deref(), Some("google_maps"));
        assert_eq!(count.count, Some(3));

        let out = serde_json::to_value(&count).unwrap();
        assert_eq!(out["type"], "google_maps");
        assert_eq!(out["count"], 3);
    }

    #[test]
    fn test_input_tokens_for_modality() {
        let usage = UsageMetadata {
            input_tokens_by_modality: Some(vec![
                ModalityTokens {
                    modality: "text".to_string(),
                    tokens: 100,
                },
                ModalityTokens {
                    modality: "image".to_string(),
                    tokens: 500,
                },
            ]),
            ..Default::default()
        };

        assert_eq!(usage.input_tokens_for_modality("text"), Some(100));
        assert_eq!(usage.input_tokens_for_modality("image"), Some(500));
        assert_eq!(usage.input_tokens_for_modality("video"), None);
    }

    #[test]
    fn test_cache_hit_rate() {
        // 25% cache hit rate
        let usage = UsageMetadata {
            total_input_tokens: Some(100),
            total_cached_tokens: Some(25),
            ..Default::default()
        };
        let rate = usage.cache_hit_rate().unwrap();
        assert!((rate - 0.25).abs() < f32::EPSILON);

        // Zero input tokens (avoid division by zero)
        let usage = UsageMetadata {
            total_input_tokens: Some(0),
            total_cached_tokens: Some(0),
            ..Default::default()
        };
        assert!(usage.cache_hit_rate().is_none());
    }

    #[test]
    fn test_has_data_with_grounding_tool_count() {
        let usage = UsageMetadata {
            grounding_tool_count: Some(vec![GroundingToolCount {
                tool_type: Some("retrieval".into()),
                count: Some(1),
            }]),
            ..Default::default()
        };
        assert!(usage.has_data());
        assert!(!UsageMetadata::default().has_data());
    }

    // =========================================================================
    // Token Count Deserialization Edge Cases
    // =========================================================================

    #[test]
    fn test_negative_token_count_clamped_to_zero() {
        let json = r#"{"total_input_tokens": -100, "total_output_tokens": 50}"#;
        let usage: UsageMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(usage.total_input_tokens, Some(0));
        assert_eq!(usage.total_output_tokens, Some(50));
    }

    #[test]
    fn test_large_token_count_clamped_to_u32_max() {
        let json = r#"{"total_input_tokens": 5000000000}"#;
        let usage: UsageMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(usage.total_input_tokens, Some(u32::MAX));
    }

    // =========================================================================
    // Image Helper Tests
    // =========================================================================

    fn make_response_with_image(base64_data: &str, mime_type: Option<&str>) -> InteractionResponse {
        InteractionResponse {
            id: Some("test-id".to_string()),
            model: Some("test-model".to_string()),
            steps: vec![Step::model_output(vec![Content::Image {
                data: Some(base64_data.to_string()),
                mime_type: mime_type.map(String::from),
                uri: None,
                resolution: None,
            }])],
            status: InteractionStatus::Completed,
            ..Default::default()
        }
    }

    #[test]
    fn test_first_image_bytes_success() {
        // Base64 for "test"
        let response = make_response_with_image("dGVzdA==", Some("image/png"));

        let bytes = response.first_image_bytes().unwrap();
        assert_eq!(bytes.unwrap(), b"test");
    }

    #[test]
    fn test_first_image_bytes_no_images() {
        let response = text_response("Hello");
        assert!(response.first_image_bytes().unwrap().is_none());
    }

    #[test]
    fn test_first_image_bytes_invalid_base64() {
        let response = make_response_with_image("not-valid-base64!!!", Some("image/png"));
        let err = response.first_image_bytes().unwrap_err().to_string();
        assert!(err.contains("Invalid base64"));
    }

    #[test]
    fn test_images_iterator() {
        let response = InteractionResponse {
            status: InteractionStatus::Completed,
            steps: vec![Step::model_output(vec![
                Content::image_data("dGVzdDE=", "image/png"),
                Content::text("text between"),
                Content::image_data("dGVzdDI=", "image/jpeg"),
            ])],
            ..Default::default()
        };

        let images: Vec<_> = response.images().collect();
        assert_eq!(images.len(), 2);

        assert_eq!(images[0].bytes().unwrap(), b"test1");
        assert_eq!(images[0].mime_type(), Some("image/png"));
        assert_eq!(images[0].extension(), "png");

        assert_eq!(images[1].bytes().unwrap(), b"test2");
        assert_eq!(images[1].extension(), "jpg");
    }

    #[test]
    fn test_has_images() {
        assert!(make_response_with_image("dGVzdA==", Some("image/png")).has_images());
        assert!(!text_response("no images").has_images());
    }

    #[test]
    fn test_image_info_extension() {
        let check = |mime: Option<&str>, expected: &str| {
            let info = ImageInfo {
                data: "",
                mime_type: mime,
            };
            assert_eq!(info.extension(), expected);
        };

        check(Some("image/jpeg"), "jpg");
        check(Some("image/jpg"), "jpg");
        check(Some("image/png"), "png");
        check(Some("image/webp"), "webp");
        check(Some("image/gif"), "gif");
        check(Some("image/unknown"), "png"); // default
        check(None, "png"); // default
    }

    // =========================================================================
    // AudioInfo Tests
    // =========================================================================

    #[test]
    fn test_audio_info_extension() {
        let check = |mime: Option<&str>, expected: &str| {
            let info = AudioInfo {
                data: "",
                mime_type: mime,
                sample_rate: None,
                channels: None,
            };
            assert_eq!(info.extension(), expected);
        };

        check(Some("audio/wav"), "wav");
        check(Some("audio/x-wav"), "wav");
        check(Some("audio/mp3"), "mp3");
        check(Some("audio/mpeg"), "mp3");
        check(Some("audio/ogg"), "ogg");
        check(Some("audio/flac"), "flac");
        check(Some("audio/aac"), "aac");
        check(Some("audio/webm"), "webm");
        // PCM/L16 format from TTS API
        check(Some("audio/L16;codec=pcm;rate=24000"), "pcm");
        check(Some("audio/l16"), "pcm");
        check(Some("audio/unknown"), "wav"); // default
        check(None, "wav"); // default
    }

    #[test]
    fn test_audio_channels_and_sample_rate_exposed() {
        let response = InteractionResponse {
            status: InteractionStatus::Completed,
            steps: vec![Step::model_output(vec![Content::Audio {
                data: Some("dGVzdA==".into()),
                uri: None,
                mime_type: Some("audio/l16".into()),
                sample_rate: Some(24000),
                channels: Some(1),
            }])],
            ..Default::default()
        };
        let audio = response.first_audio().unwrap();
        assert_eq!(audio.sample_rate(), Some(24000));
        assert_eq!(audio.channels(), Some(1));
        assert!(response.has_audio());
    }

    // =========================================================================
    // Usage accumulation
    // =========================================================================

    #[test]
    fn test_usage_metadata_accumulate_all_fields() {
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            total_tokens: Some(150),
            total_cached_tokens: Some(20),
            total_thought_tokens: Some(5),
            total_tool_use_tokens: Some(15),
            ..Default::default()
        };
        let usage2 = UsageMetadata {
            total_input_tokens: Some(200),
            total_output_tokens: Some(100),
            total_tokens: Some(300),
            total_cached_tokens: Some(40),
            total_thought_tokens: Some(10),
            total_tool_use_tokens: Some(30),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        assert_eq!(usage1.total_input_tokens, Some(300));
        assert_eq!(usage1.total_output_tokens, Some(150));
        assert_eq!(usage1.total_tokens, Some(450));
        assert_eq!(usage1.total_cached_tokens, Some(60));
        assert_eq!(usage1.total_thought_tokens, Some(15));
        assert_eq!(usage1.total_tool_use_tokens, Some(45));
    }

    #[test]
    fn test_usage_metadata_accumulate_saturating() {
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(u32::MAX - 10),
            ..Default::default()
        };
        let usage2 = UsageMetadata {
            total_input_tokens: Some(100),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        assert_eq!(usage1.total_input_tokens, Some(u32::MAX));
    }

    // =========================================================================
    // InteractionStatus Tests
    // =========================================================================

    #[test]
    fn test_interaction_status_incomplete_roundtrip() {
        let json = r#""incomplete""#;
        let status: InteractionStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, InteractionStatus::Incomplete);

        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(serialized, r#""incomplete""#);
    }

    #[test]
    fn test_interaction_status_unknown_preserved() {
        let status: InteractionStatus = serde_json::from_str("\"hibernating\"").unwrap();
        assert!(status.is_unknown());
        assert_eq!(status.unknown_status_type(), Some("hibernating"));
        assert!(status.unknown_data().is_some());
        assert_eq!(serde_json::to_string(&status).unwrap(), "\"hibernating\"");
    }
}
