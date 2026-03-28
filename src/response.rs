//! Response types for the Interactions API.
//!
//! This module contains `InteractionResponse` and related types for handling
//! API responses, including helper methods for extracting content.

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fmt;

use crate::content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleSearchResultItem,
};
use crate::errors::GenaiError;
use crate::request::Turn;
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
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum InteractionStatus {
    Completed,
    InProgress,
    RequiresAction,
    Failed,
    Cancelled,
    /// Interaction ended before completion (e.g., token limit reached).
    Incomplete,
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
    /// Total number of reasoning tokens (populated for thinking models like gemini-2.0-flash-thinking)
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_token_count"
    )]
    pub total_reasoning_tokens: Option<u32>,
    /// Total number of thought tokens (thinking model internal reasoning, distinct from reasoning_tokens)
    ///
    /// This field appears in API responses for models using the thinking/reasoning features.
    /// Note: This may overlap with or complement `total_reasoning_tokens` - both are included
    /// to accurately reflect the wire format returned by the API.
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
}

impl UsageMetadata {
    /// Returns true if any usage data is present
    #[must_use]
    pub fn has_data(&self) -> bool {
        self.total_tokens.is_some()
            || self.total_input_tokens.is_some()
            || self.total_output_tokens.is_some()
            || self.total_cached_tokens.is_some()
            || self.total_reasoning_tokens.is_some()
            || self.total_thought_tokens.is_some()
            || self.total_tool_use_tokens.is_some()
            || self.input_tokens_by_modality.is_some()
            || self.output_tokens_by_modality.is_some()
            || self.cached_tokens_by_modality.is_some()
            || self.tool_use_tokens_by_modality.is_some()
    }

    /// Returns total thought tokens (thinking model internal reasoning)
    ///
    /// This may be populated for thinking models. See also `total_reasoning_tokens`
    /// which may contain related but distinct token counts.
    #[must_use]
    pub fn thought_tokens(&self) -> Option<u32> {
        self.total_thought_tokens
    }

    /// Returns the input token count for a specific modality.
    ///
    /// # Arguments
    ///
    /// * `modality` - The modality name (e.g., "TEXT", "IMAGE", "AUDIO")
    ///
    /// # Returns
    ///
    /// The token count for the specified modality, or `None` if the modality
    /// is not present in the breakdown or if modality data is unavailable.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::UsageMetadata;
    /// # let usage: UsageMetadata = Default::default();
    /// if let Some(image_tokens) = usage.input_tokens_for_modality("IMAGE") {
    ///     println!("Image input cost: {} tokens", image_tokens);
    /// }
    /// ```
    #[must_use]
    pub fn input_tokens_for_modality(&self, modality: &str) -> Option<u32> {
        self.input_tokens_by_modality
            .as_ref()?
            .iter()
            .find(|m| m.modality == modality)
            .map(|m| m.tokens)
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
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::UsageMetadata;
    /// # let usage: UsageMetadata = Default::default();
    /// if let Some(rate) = usage.cache_hit_rate() {
    ///     println!("Cache hit rate: {:.1}%", rate * 100.0);
    /// }
    /// ```
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
    /// Note: `*_by_modality` fields are not accumulated (would require complex merging).
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
        add_option(
            &mut self.total_reasoning_tokens,
            other.total_reasoning_tokens,
        );
        add_option(&mut self.total_thought_tokens, other.total_thought_tokens);
        add_option(&mut self.total_tool_use_tokens, other.total_tool_use_tokens);
        // Note: *_by_modality fields are not accumulated as they would require
        // complex merging logic. If needed, callers can handle these separately.
    }
}

// =============================================================================
// Metadata Types (Google Search grounding, URL context)
// =============================================================================

/// Grounding metadata returned when using the GoogleSearch tool.
///
/// Contains search queries executed by the model and web sources that
/// ground the response in real-time information.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// if let Some(metadata) = response.google_search_metadata() {
///     println!("Search queries: {:?}", metadata.web_search_queries);
///     for chunk in &metadata.grounding_chunks {
///         println!("Source: {} - {}", chunk.web.title, chunk.web.uri);
///     }
/// }
/// ```
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct GroundingMetadata {
    /// Search queries that were executed by the model
    pub web_search_queries: Vec<String>,

    /// Web sources referenced in the response
    pub grounding_chunks: Vec<GroundingChunk>,
}

/// A web source referenced in grounding.
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
pub struct GroundingChunk {
    /// Web resource information
    #[serde(default)]
    pub web: WebSource,
}

/// Web source details (URI, title, and domain).
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct WebSource {
    /// URI of the web page
    pub uri: String,
    /// Title of the source
    pub title: String,
    /// Domain of the web page (e.g., "wikipedia.org")
    pub domain: String,
}

/// Metadata returned when using the UrlContext tool.
///
/// Contains retrieval status for each URL that was processed.
/// This is useful for verification and debugging URL fetches.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// if let Some(metadata) = response.url_context_metadata() {
///     for entry in &metadata.url_metadata {
///         println!("URL: {} - Status: {:?}", entry.retrieved_url, entry.url_retrieval_status);
///     }
/// }
/// ```
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UrlContextMetadata {
    /// Metadata for each URL that was processed
    pub url_metadata: Vec<UrlMetadataEntry>,
}

/// Retrieval status for a single URL processed by the UrlContext tool.
#[derive(Clone, Deserialize, Serialize, Debug, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct UrlMetadataEntry {
    /// The URL that was retrieved
    pub retrieved_url: String,
    /// Status of the retrieval attempt
    pub url_retrieval_status: UrlRetrievalStatus,
}

/// Status of a URL retrieval attempt.
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// This enum is marked `#[non_exhaustive]`, which means:
/// - Match statements must include a wildcard arm (`_ => ...`)
/// - New variants may be added in minor version updates without breaking your code
///
/// When the API returns a status value that this library doesn't recognize,
/// it will be captured as `UrlRetrievalStatus::Unknown` rather than causing a
/// deserialization error. This follows the
/// [Evergreen spec](https://github.com/google-deepmind/evergreen-spec)
/// philosophy of graceful degradation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum UrlRetrievalStatus {
    /// Status not specified
    #[default]
    Unspecified,
    /// URL content was successfully retrieved
    Success,
    /// URL failed safety/content moderation checks
    Unsafe,
    /// URL retrieval failed for other reasons
    Error,
    /// Unknown status (for forward compatibility).
    ///
    /// This variant captures any unrecognized status values from the API,
    /// allowing the library to handle new statuses gracefully.
    ///
    /// The `status_type` field contains the unrecognized status string,
    /// and `data` contains the full JSON value for debugging.
    Unknown {
        /// The unrecognized status string from the API
        status_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl UrlRetrievalStatus {
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

    /// Returns true if retrieval was successful.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    /// Returns true if retrieval failed for any reason.
    ///
    /// **Note:** This returns `true` only for `Error` and `Unsafe` variants.
    /// The `Unknown` variant is NOT treated as an error because:
    /// 1. URL retrieval failures are non-critical (graceful degradation)
    /// 2. An `Unknown` status may represent a new success state from the API
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error | Self::Unsafe)
    }
}

impl Serialize for UrlRetrievalStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Unspecified => serializer.serialize_str("URL_RETRIEVAL_STATUS_UNSPECIFIED"),
            Self::Success => serializer.serialize_str("URL_RETRIEVAL_STATUS_SUCCESS"),
            Self::Unsafe => serializer.serialize_str("URL_RETRIEVAL_STATUS_UNSAFE"),
            Self::Error => serializer.serialize_str("URL_RETRIEVAL_STATUS_ERROR"),
            Self::Unknown { status_type, .. } => serializer.serialize_str(status_type),
        }
    }
}

impl<'de> Deserialize<'de> for UrlRetrievalStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.as_str() {
            Some("URL_RETRIEVAL_STATUS_UNSPECIFIED") => Ok(Self::Unspecified),
            Some("URL_RETRIEVAL_STATUS_SUCCESS") => Ok(Self::Success),
            Some("URL_RETRIEVAL_STATUS_UNSAFE") => Ok(Self::Unsafe),
            Some("URL_RETRIEVAL_STATUS_ERROR") => Ok(Self::Error),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown UrlRetrievalStatus '{}'. \
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
                    "UrlRetrievalStatus received non-string value: {}. \
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

impl fmt::Display for UrlRetrievalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => write!(f, "URL_RETRIEVAL_STATUS_UNSPECIFIED"),
            Self::Success => write!(f, "URL_RETRIEVAL_STATUS_SUCCESS"),
            Self::Unsafe => write!(f, "URL_RETRIEVAL_STATUS_UNSAFE"),
            Self::Error => write!(f, "URL_RETRIEVAL_STATUS_ERROR"),
            Self::Unknown { status_type, .. } => write!(f, "{}", status_type),
        }
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
            Some(mime) if mime.starts_with("audio/L16") => "pcm",
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
///     println!("Function: {} with args: {}", call.name, call.args);
///     if let Some(id) = call.id {
///         println!("  Call ID: {}", id);
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionCallInfo<'a> {
    /// Unique identifier for this function call (used when sending results back)
    pub id: Option<&'a str>,
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
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// // Store function calls for later processing
    /// let owned_calls: Vec<_> = response.function_calls()
    ///     .into_iter()
    ///     .map(|call| call.to_owned())
    ///     .collect();
    /// ```
    #[must_use]
    pub fn to_owned(&self) -> OwnedFunctionCallInfo {
        OwnedFunctionCallInfo {
            id: self.id.map(String::from),
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
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// let owned_calls: Vec<_> = response.function_calls()
///     .into_iter()
///     .map(|call| call.to_owned())
///     .collect();
///
/// // owned_calls can now outlive `response`
/// for call in owned_calls {
///     println!("Function: {} with args: {}", call.name, call.args);
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedFunctionCallInfo {
    /// Unique identifier for this function call (used when sending results back)
    pub id: Option<String>,
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
/// It implements [`Serialize`] for logging and debugging purposes, but not `Deserialize`
/// since it's not meant to be constructed directly—use the response helper methods instead.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// for result in response.function_results() {
///     if let Some(name) = result.name {
///         println!("Function {} returned: {}", name, result.result);
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionResultInfo<'a> {
    /// Name of the function that was called (optional per API spec)
    pub name: Option<&'a str>,
    /// The call_id from the FunctionCall this result responds to
    pub call_id: &'a str,
    /// The result returned by the function
    pub result: &'a serde_json::Value,
    /// Whether this result indicates an error
    pub is_error: Option<bool>,
}

/// Information about a code execution call requested by the model.
///
/// Returned by [`InteractionResponse::code_execution_calls()`] for convenient access
/// to code execution details.
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
/// for call in response.code_execution_calls() {
///     println!("Executing {} code (id: {:?})", call.language, call.id);
///     println!("Code: {}", call.code);
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CodeExecutionCallInfo<'a> {
    /// Unique identifier for this code execution call (optional per API spec)
    pub id: Option<&'a str>,
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
/// It implements [`Serialize`] for logging and debugging purposes, but not `Deserialize`
/// since it's not meant to be constructed directly—use the response helper methods instead.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// for result in response.code_execution_results() {
///     println!("Call {:?} completed: is_error={}", result.call_id, result.is_error);
///     if !result.is_error {
///         println!("Output: {}", result.result);
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CodeExecutionResultInfo<'a> {
    /// The call_id matching the CodeExecutionCall this result is for (optional per API spec)
    pub call_id: Option<&'a str>,
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
/// It implements [`Serialize`] for logging and debugging purposes, but not `Deserialize`
/// since it's not meant to be constructed directly—use the response helper methods instead.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// for result in response.url_context_results() {
///     println!("Call ID: {}", result.call_id);
///     for item in result.items {
///         println!("  URL: {} - Status: {}", item.url, item.status);
///     }
/// }
/// ```
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

/// Response from creating or retrieving an interaction
#[derive(Clone, Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
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

    /// The input that was provided (array of content objects)
    #[serde(default)]
    pub input: Vec<Content>,

    /// The outputs generated by the model/agent (array of content objects)
    #[serde(default)]
    pub outputs: Vec<Content>,

    /// Current status of the interaction
    pub status: InteractionStatus,

    /// Token usage information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,

    /// Tools that were available for this interaction
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Grounding metadata when using GoogleSearch tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grounding_metadata: Option<GroundingMetadata>,

    /// URL context metadata when using UrlContext tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_context_metadata: Option<UrlContextMetadata>,

    /// Previous interaction ID if this was a follow-up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,

    /// Timestamp when the interaction was created (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,

    /// Timestamp when the interaction was last updated (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<DateTime<Utc>>,
}

impl InteractionResponse {
    // =========================================================================
    // Text Content Helpers
    // =========================================================================

    /// Extract the first text content from outputs
    ///
    /// Returns the first text found in the outputs vector.
    /// Useful for simple queries where you expect a single text response.
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
        self.outputs.iter().find_map(|content| {
            if let Content::Text { text: Some(t), .. } = content {
                Some(t.as_str())
            } else {
                None
            }
        })
    }

    /// Extract all text contents concatenated
    ///
    /// Combines all text outputs into a single string.
    /// Useful when the model returns multiple text chunks.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// let full_text = response.all_text();
    /// println!("Complete response: {}", full_text);
    /// ```
    #[must_use]
    pub fn all_text(&self) -> String {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::Text { text: Some(t), .. } = content {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }

    // =========================================================================
    // Annotation Helpers (Citation Support)
    // =========================================================================

    /// Check if response contains annotations (citations).
    ///
    /// Returns `true` if any text output contains source annotations.
    /// Annotations are typically present when grounding tools like
    /// `GoogleSearch` or `UrlContext` were used.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_annotations() {
    ///     println!("Response includes {} citations", response.all_annotations().count());
    /// }
    /// ```
    #[must_use]
    pub fn has_annotations(&self) -> bool {
        self.outputs.iter().any(|c| c.annotations().is_some())
    }

    /// Returns all annotations from text outputs.
    ///
    /// Collects all [`Annotation`] references from all text outputs in the response.
    /// Annotations link specific text spans to their sources, enabling citation tracking.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// let text = response.all_text();
    /// for annotation in response.all_annotations() {
    ///     if let Some(span) = annotation.extract_span(&text) {
    ///         println!("'{}' sourced from: {:?}", span, annotation.source);
    ///     }
    /// }
    /// ```
    pub fn all_annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.outputs
            .iter()
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
    ///     .with_text("A sunset over mountains")
    ///     .with_image_output()
    ///     .create()
    ///     .await?;
    ///
    /// if let Some(bytes) = response.first_image_bytes()? {
    ///     std::fs::write("sunset.png", &bytes)?;
    ///     println!("Saved {} bytes", bytes.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn first_image_bytes(&self) -> Result<Option<Vec<u8>>, GenaiError> {
        for output in &self.outputs {
            if let Content::Image {
                data: Some(base64_data),
                ..
            } = output
            {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(base64_data)
                    .map_err(|e| {
                        GenaiError::InvalidInput(format!("Invalid base64 image data: {}", e))
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
    ///     .with_text("Generate 3 variations of a cat")
    ///     .with_image_output()
    ///     .create()
    ///     .await?;
    ///
    /// for (i, image) in response.images().enumerate() {
    ///     let bytes = image.bytes()?;
    ///     let filename = format!("cat_{}.{}", i, image.extension());
    ///     std::fs::write(&filename, bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn images(&self) -> impl Iterator<Item = ImageInfo<'_>> {
        self.outputs.iter().filter_map(|output| {
            if let Content::Image {
                data: Some(base64_data),
                mime_type,
                ..
            } = output
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
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// # let response = client.interaction().with_model("gemini-3-flash-preview")
    /// #     .with_text("A cat").with_image_output().create().await?;
    /// if response.has_images() {
    ///     for image in response.images() {
    ///         let bytes = image.bytes()?;
    ///         // process images...
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn has_images(&self) -> bool {
        self.outputs
            .iter()
            .any(|output| matches!(output, Content::Image { data: Some(_), .. }))
    }

    // =========================================================================
    // Audio Helpers
    // =========================================================================

    /// Returns the first audio content in the response.
    ///
    /// This is a convenience method for the common case of extracting a single
    /// generated audio. For multiple audio outputs, use [`audios()`](Self::audios).
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
    /// if let Some(audio) = response.first_audio() {
    ///     let bytes = audio.bytes()?;
    ///     std::fs::write("speech.wav", &bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn first_audio(&self) -> Option<AudioInfo<'_>> {
        self.audios().next()
    }

    /// Returns an iterator over all audio content in the response.
    ///
    /// Each [`AudioInfo`] provides methods for accessing the audio data,
    /// MIME type, and a suitable file extension.
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
    ///     .with_text("Generate multiple audio segments")
    ///     .with_audio_output()
    ///     .create()
    ///     .await?;
    ///
    /// for (i, audio) in response.audios().enumerate() {
    ///     let bytes = audio.bytes()?;
    ///     let filename = format!("audio_{}.{}", i, audio.extension());
    ///     std::fs::write(&filename, bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn audios(&self) -> impl Iterator<Item = AudioInfo<'_>> {
        self.outputs.iter().filter_map(|output| {
            if let Content::Audio {
                data: Some(base64_data),
                mime_type,
                ..
            } = output
            {
                Some(AudioInfo {
                    data: base64_data.as_str(),
                    mime_type: mime_type.as_deref(),
                })
            } else {
                None
            }
        })
    }

    /// Check if the response contains any audio content.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// # let response = client.interaction().with_model("gemini-2.5-pro-preview-tts")
    /// #     .with_text("Hello").with_audio_output().create().await?;
    /// if response.has_audio() {
    ///     for audio in response.audios() {
    ///         let bytes = audio.bytes()?;
    ///         // process audio...
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn has_audio(&self) -> bool {
        self.outputs
            .iter()
            .any(|output| matches!(output, Content::Audio { data: Some(_), .. }))
    }

    // =========================================================================
    // Function Calling Helpers
    // =========================================================================

    /// Extract function calls from outputs
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
    ///     if let Some(id) = call.id {
    ///         // Use call.id when sending results back to the model
    ///         println!("  Call ID: {}", id);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn function_calls(&self) -> Vec<FunctionCallInfo<'_>> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::FunctionCall { id, name, args } = content {
                    Some(FunctionCallInfo {
                        id: id.as_ref().map(|s| s.as_str()),
                        name: name.as_str(),
                        args,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if response contains text
    ///
    /// Returns true if any output contains text content.
    #[must_use]
    pub fn has_text(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::Text { text: Some(_), .. }))
    }

    /// Check if response contains function calls
    ///
    /// Returns true if any output contains a function call.
    #[must_use]
    pub fn has_function_calls(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::FunctionCall { .. }))
    }

    /// Check if response contains function results
    ///
    /// Returns true if any output contains a function result.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_function_results() {
    ///     for result in response.function_results() {
    ///         println!("Function {:?} returned data", result.name);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn has_function_results(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::FunctionResult { .. }))
    }

    /// Extract function results from outputs
    ///
    /// Returns a vector of [`FunctionResultInfo`] structs with named fields for
    /// convenient access to function result details.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for result in response.function_results() {
    ///     println!("Function {:?} (call_id: {}) returned: {}",
    ///         result.name, result.call_id, result.result);
    /// }
    /// ```
    #[must_use]
    pub fn function_results(&self) -> Vec<FunctionResultInfo<'_>> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::FunctionResult {
                    name,
                    call_id,
                    result,
                    is_error,
                } = content
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

    /// Check if response contains thoughts (internal reasoning)
    ///
    /// Returns true if any output contains thought content with a signature.
    #[must_use]
    pub fn has_thoughts(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::Thought { signature: Some(_) }))
    }

    /// Get an iterator over all thought signatures (internal reasoning verification).
    ///
    /// Returns the cryptographic signature of each `Thought` variant in the outputs.
    /// These signatures are used to verify the model's reasoning process when
    /// thinking mode is enabled via `with_thinking_level()`.
    ///
    /// **Note:** The actual thought text is not exposed by the API - only signatures
    /// for verification purposes.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for signature in response.thought_signatures() {
    ///     println!("Thought signature: {}", signature);
    /// }
    /// ```
    pub fn thought_signatures(&self) -> impl Iterator<Item = &str> {
        self.outputs.iter().filter_map(|c| match c {
            Content::Thought { signature: Some(s) } => Some(s.as_str()),
            _ => None,
        })
    }

    // =========================================================================
    // Unknown Content Helpers (Evergreen Forward Compatibility)
    // =========================================================================

    /// Check if response contains unknown content types.
    ///
    /// Returns `true` if any output contains an [`Content::Unknown`] variant.
    /// This indicates the API returned content types that this library version doesn't
    /// recognize.
    ///
    /// # When to Use
    ///
    /// Call this after receiving a response to detect if you might be missing content:
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_unknown() {
    ///     eprintln!("Warning: Response contains unknown content types");
    ///     for (content_type, data) in response.unknown_content() {
    ///         eprintln!("  - {}: {:?}", content_type, data);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn has_unknown(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::Unknown { .. }))
    }

    /// Get all unknown content as (content_type, data) tuples.
    ///
    /// Returns a vector of references to the type names and JSON data for all
    /// [`Content::Unknown`] variants in the outputs.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for (content_type, data) in response.unknown_content() {
    ///     println!("Unknown type '{}': {}", content_type, data);
    /// }
    /// ```
    #[must_use]
    pub fn unknown_content(&self) -> Vec<(&str, &serde_json::Value)> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::Unknown { content_type, data } = content {
                    Some((content_type.as_str(), data))
                } else {
                    None
                }
            })
            .collect()
    }

    // =========================================================================
    // Google Search Metadata Helpers
    // =========================================================================

    /// Check if response has grounding metadata from Google Search.
    ///
    /// Returns true if the response was grounded using the GoogleSearch tool.
    ///
    /// This checks for both:
    /// - Explicit `grounding_metadata` field (future API support)
    /// - GoogleSearchCall/GoogleSearchResult outputs (current Interactions API)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_google_search_metadata() {
    ///     println!("Response is grounded with web sources");
    /// }
    /// ```
    #[must_use]
    pub fn has_google_search_metadata(&self) -> bool {
        self.grounding_metadata.is_some()
            || self.has_google_search_calls()
            || self.has_google_search_results()
    }

    /// Get Google Search grounding metadata if explicitly present.
    ///
    /// **Note:** The Interactions API embeds Google Search data in outputs rather than
    /// a top-level `grounding_metadata` field. Use [`google_search_calls()`](Self::google_search_calls)
    /// and [`google_search_results()`](Self::google_search_results) to access the search
    /// data from outputs. This method returns `None` for Interactions API responses.
    ///
    /// Returns the grounding metadata containing search queries and web sources
    /// when the GoogleSearch tool was used and the API provides explicit metadata.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// // For Interactions API, use direct output accessors:
    /// for query in response.google_search_calls() {
    ///     println!("Search query: {}", query);
    /// }
    /// for result in response.google_search_results() {
    ///     println!("Source: {} - {}", result.title, result.url);
    /// }
    /// ```
    #[must_use]
    pub fn google_search_metadata(&self) -> Option<&GroundingMetadata> {
        self.grounding_metadata.as_ref()
    }

    // =========================================================================
    // URL Context Metadata Helpers
    // =========================================================================

    /// Check if response has URL context metadata.
    ///
    /// Returns true if the UrlContext tool was used and metadata is available.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_url_context_metadata() {
    ///     println!("Response includes URL context");
    /// }
    /// ```
    #[must_use]
    pub fn has_url_context_metadata(&self) -> bool {
        self.url_context_metadata.is_some()
    }

    /// Get URL context metadata if present.
    ///
    /// Returns metadata about URLs that were fetched when the UrlContext tool was used,
    /// including retrieval status for each URL.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(metadata) = response.url_context_metadata() {
    ///     for entry in &metadata.url_metadata {
    ///         println!("URL: {} - Status: {:?}", entry.retrieved_url, entry.url_retrieval_status);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn url_context_metadata(&self) -> Option<&UrlContextMetadata> {
        self.url_context_metadata.as_ref()
    }

    // =========================================================================
    // Code Execution Tool Helpers
    // =========================================================================

    /// Check if response contains code execution calls
    #[must_use]
    pub fn has_code_execution_calls(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::CodeExecutionCall { .. }))
    }

    /// Get the first code execution call, if any.
    ///
    /// Convenience method for the common case where you just want to see
    /// the first code the model wants to execute.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(call) = response.code_execution_call() {
    ///     println!("Model wants to run {} code (id: {:?}):\n{}", call.language, call.id, call.code);
    /// }
    /// ```
    #[must_use]
    pub fn code_execution_call(&self) -> Option<CodeExecutionCallInfo<'_>> {
        self.outputs.iter().find_map(|content| {
            if let Content::CodeExecutionCall { id, language, code } = content {
                Some(CodeExecutionCallInfo {
                    id: id.as_deref(),
                    language: language.clone(),
                    code: code.as_str(),
                })
            } else {
                None
            }
        })
    }

    /// Extract all code execution calls from outputs
    ///
    /// Returns a vector of [`CodeExecutionCallInfo`] structs with named fields for
    /// convenient access to code execution details.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{InteractionResponse, CodeExecutionLanguage};
    /// # let response: InteractionResponse = todo!();
    /// for call in response.code_execution_calls() {
    ///     match call.language {
    ///         CodeExecutionLanguage::Python => println!("Python (id: {:?}):\n{}", call.id, call.code),
    ///         _ => println!("Other (id: {:?}):\n{}", call.id, call.code),
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn code_execution_calls(&self) -> Vec<CodeExecutionCallInfo<'_>> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::CodeExecutionCall { id, language, code } = content {
                    Some(CodeExecutionCallInfo {
                        id: id.as_deref(),
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
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::CodeExecutionResult { .. }))
    }

    /// Extract code execution results from outputs
    ///
    /// Returns a vector of [`CodeExecutionResultInfo`] structs with named fields for
    /// convenient access to code execution results.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for result in response.code_execution_results() {
    ///     if !result.is_error {
    ///         println!("Code output (call_id: {:?}): {}", result.call_id, result.result);
    ///     } else {
    ///         eprintln!("Code failed: {}", result.result);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn code_execution_results(&self) -> Vec<CodeExecutionResultInfo<'_>> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::CodeExecutionResult {
                    call_id,
                    is_error,
                    result,
                } = content
                {
                    Some(CodeExecutionResultInfo {
                        call_id: call_id.as_deref(),
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
    ///
    /// This is a convenience method for the common case where you just want the
    /// output from successful code execution without handling errors.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(output) = response.successful_code_output() {
    ///     println!("Result: {}", output);
    /// }
    /// ```
    #[must_use]
    pub fn successful_code_output(&self) -> Option<&str> {
        self.outputs.iter().find_map(|content| {
            if let Content::CodeExecutionResult {
                is_error, result, ..
            } = content
            {
                if !is_error {
                    Some(result.as_str())
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    // =========================================================================
    // Google Search Output Content Helpers
    // =========================================================================

    /// Check if response contains Google Search calls
    ///
    /// Returns true if the model performed any Google Search queries.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_google_search_calls() {
    ///     println!("Model searched: {:?}", response.google_search_calls());
    /// }
    /// ```
    #[must_use]
    pub fn has_google_search_calls(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::GoogleSearchCall { .. }))
    }

    /// Get the first Google Search query, if any.
    ///
    /// Convenience method for the common case where you just want to see
    /// the first search query performed by the model.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(query) = response.google_search_call() {
    ///     println!("Model searched for: {}", query);
    /// }
    /// ```
    #[must_use]
    pub fn google_search_call(&self) -> Option<&str> {
        self.outputs.iter().find_map(|content| {
            if let Content::GoogleSearchCall { queries, .. } = content {
                // Return first non-empty query
                queries.iter().find(|q| !q.is_empty()).map(|q| q.as_str())
            } else {
                None
            }
        })
    }

    /// Extract all Google Search queries from outputs
    ///
    /// Returns a vector of search query strings (flattened from all search calls).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for query in response.google_search_calls() {
    ///     println!("Searched for: {}", query);
    /// }
    /// ```
    #[must_use]
    pub fn google_search_calls(&self) -> Vec<&str> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::GoogleSearchCall { queries, .. } = content {
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
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::GoogleSearchResult { .. }))
    }

    /// Extract Google Search result items from outputs
    ///
    /// Returns a vector of references to the search result items with title/URL info.
    #[must_use]
    pub fn google_search_results(&self) -> Vec<&GoogleSearchResultItem> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::GoogleSearchResult { result, .. } = content {
                    Some(result.iter())
                } else {
                    None
                }
            })
            .flatten()
            .collect()
    }

    // =========================================================================
    // URL Context Output Content Helpers
    // =========================================================================

    /// Check if response contains URL context calls
    ///
    /// Returns true if the model requested any URLs for context.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_url_context_calls() {
    ///     let urls = response.url_context_call_urls();
    ///     println!("Model fetched: {:?}", urls);
    /// }
    /// ```
    #[must_use]
    pub fn has_url_context_calls(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::UrlContextCall { .. }))
    }

    /// Get the ID of the first URL context call, if any.
    ///
    /// Convenience method for the common case where you just want to get
    /// the call ID for the first URL context call.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(call_id) = response.url_context_call_id() {
    ///     println!("URL context call ID: {}", call_id);
    /// }
    /// ```
    #[must_use]
    pub fn url_context_call_id(&self) -> Option<&str> {
        self.outputs.iter().find_map(|content| {
            if let Content::UrlContextCall { id, .. } = content {
                Some(id.as_str())
            } else {
                None
            }
        })
    }

    /// Extract URL context call URLs from outputs
    ///
    /// Returns a vector of all URLs that were requested for fetching across all UrlContextCalls.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for url in response.url_context_call_urls() {
    ///     println!("Requested URL: {}", url);
    /// }
    /// ```
    #[must_use]
    pub fn url_context_call_urls(&self) -> Vec<&str> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::UrlContextCall { urls, .. } = content {
                    Some(urls.iter().map(String::as_str).collect::<Vec<_>>())
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
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::UrlContextResult { .. }))
    }

    /// Extract URL context results from outputs
    ///
    /// Returns a vector of [`UrlContextResultInfo`] structs with named fields for
    /// convenient access to URL context results.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for result in response.url_context_results() {
    ///     println!("Call ID: {}", result.call_id);
    ///     for item in result.items {
    ///         println!("  URL: {} - Status: {}", item.url, item.status);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn url_context_results(&self) -> Vec<UrlContextResultInfo<'_>> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::UrlContextResult { call_id, result } = content {
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
    // File Search Output Content Helpers
    // =========================================================================

    /// Check if response contains file search results
    ///
    /// Returns true if the model returned any file search results from semantic retrieval.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if response.has_file_search_results() {
    ///     println!("Found {} search matches", response.file_search_results().len());
    /// }
    /// ```
    #[must_use]
    pub fn has_file_search_results(&self) -> bool {
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::FileSearchResult { .. }))
    }

    /// Extract file search result items from outputs
    ///
    /// Returns a vector of references to the file search result items with title/text/store info.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for result in response.file_search_results() {
    ///     println!("{}: {}", result.title, result.text);
    /// }
    /// ```
    #[must_use]
    pub fn file_search_results(&self) -> Vec<&FileSearchResultItem> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::FileSearchResult { result, .. } = content {
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
        self.outputs
            .iter()
            .any(|c| matches!(c, Content::GoogleMapsResult { .. }))
    }

    /// Extract Google Maps results from outputs.
    ///
    /// Returns a vector of [`GoogleMapsResultInfo`] structs with call ID and
    /// place data for each Google Maps result in the response.
    #[must_use]
    pub fn google_maps_results(&self) -> Vec<GoogleMapsResultInfo<'_>> {
        self.outputs
            .iter()
            .filter_map(|content| {
                if let Content::GoogleMapsResult {
                    call_id, result, ..
                } = content
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

    /// Get a summary of content types present in outputs.
    ///
    /// Returns a [`ContentSummary`] with counts for each content type.
    /// Useful for debugging, logging, or detecting unexpected content.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// let summary = response.content_summary();
    /// println!("Response has {} text outputs", summary.text_count);
    /// if summary.unknown_count > 0 {
    ///     println!("Warning: {} unknown types: {:?}",
    ///         summary.unknown_count, summary.unknown_types);
    /// }
    /// ```
    #[must_use]
    pub fn content_summary(&self) -> ContentSummary {
        let mut summary = ContentSummary::default();
        let mut unknown_types_set = BTreeSet::new();

        for content in &self.outputs {
            match content {
                Content::Text { .. } => summary.text_count += 1,
                Content::Thought { .. } => summary.thought_count += 1,
                Content::ThoughtSignature { .. } => {
                    // ThoughtSignature typically only appears during streaming,
                    // not in final outputs. Count with thoughts if present.
                    summary.thought_count += 1
                }
                Content::Image { .. } => summary.image_count += 1,
                Content::Audio { .. } => summary.audio_count += 1,
                Content::Video { .. } => summary.video_count += 1,
                Content::Document { .. } => summary.document_count += 1,
                Content::FunctionCall { .. } => summary.function_call_count += 1,
                Content::FunctionResult { .. } => summary.function_result_count += 1,
                Content::CodeExecutionCall { .. } => summary.code_execution_call_count += 1,
                Content::CodeExecutionResult { .. } => summary.code_execution_result_count += 1,
                Content::GoogleSearchCall { .. } => summary.google_search_call_count += 1,
                Content::GoogleSearchResult { .. } => summary.google_search_result_count += 1,
                Content::UrlContextCall { .. } => summary.url_context_call_count += 1,
                Content::UrlContextResult { .. } => summary.url_context_result_count += 1,
                Content::FileSearchResult { .. } => summary.file_search_result_count += 1,
                Content::GoogleMapsCall { .. } => summary.google_maps_call_count += 1,
                Content::GoogleMapsResult { .. } => summary.google_maps_result_count += 1,
                Content::ComputerUseCall { .. } => summary.computer_use_call_count += 1,
                Content::ComputerUseResult { .. } => summary.computer_use_result_count += 1,
                Content::Unknown { content_type, .. } => {
                    summary.unknown_count += 1;
                    unknown_types_set.insert(content_type.clone());
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
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(tokens) = response.input_tokens() {
    ///     println!("Input tokens: {}", tokens);
    /// }
    /// ```
    #[must_use]
    pub fn input_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_input_tokens)
    }

    /// Get the number of output tokens generated.
    ///
    /// Returns `None` if usage metadata is not available.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(tokens) = response.output_tokens() {
    ///     println!("Output tokens: {}", tokens);
    /// }
    /// ```
    #[must_use]
    pub fn output_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_output_tokens)
    }

    /// Get the total number of tokens used (input + output).
    ///
    /// Returns `None` if usage metadata is not available.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(tokens) = response.total_tokens() {
    ///     println!("Total tokens: {}", tokens);
    /// }
    /// ```
    #[must_use]
    pub fn total_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_tokens)
    }

    /// Get the number of reasoning tokens used (for thinking models).
    ///
    /// Reasoning tokens are used when thinking mode is enabled
    /// (e.g., via `with_thinking_level()` on supported models).
    /// Returns `None` if usage metadata is not available or thinking wasn't used.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(tokens) = response.reasoning_tokens() {
    ///     println!("Reasoning tokens: {}", tokens);
    /// }
    /// ```
    #[must_use]
    pub fn reasoning_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_reasoning_tokens)
    }

    /// Get the number of cached tokens used (from context caching).
    ///
    /// Cached tokens reduce billing costs when reusing context.
    /// Returns `None` if usage metadata is not available or caching wasn't used.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(tokens) = response.cached_tokens() {
    ///     println!("Cached tokens: {} (reduces cost)", tokens);
    /// }
    /// ```
    #[must_use]
    pub fn cached_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_cached_tokens)
    }

    /// Get the number of tool use tokens consumed.
    ///
    /// Tool use tokens represent overhead from function calling.
    /// Returns `None` if usage metadata is not available or tools weren't used.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(tokens) = response.tool_use_tokens() {
    ///     println!("Tool use overhead: {} tokens", tokens);
    /// }
    /// ```
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
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(created) = response.created() {
    ///     println!("Created at: {}", created.to_rfc3339());
    /// }
    /// ```
    #[must_use]
    pub fn created(&self) -> Option<DateTime<Utc>> {
        self.created
    }

    /// Get the timestamp when this interaction was last updated.
    ///
    /// Returns `None` if the interaction was created with `store=false` or
    /// if the API didn't include timestamp information.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// if let Some(updated) = response.updated() {
    ///     println!("Last updated: {}", updated.to_rfc3339());
    /// }
    /// ```
    #[must_use]
    pub fn updated(&self) -> Option<DateTime<Utc>> {
        self.updated
    }

    // =========================================================================
    // Multi-Turn Helpers
    // =========================================================================

    /// Converts this response's outputs to a model turn for multi-turn conversations.
    ///
    /// This enables seamless multi-turn patterns by allowing response outputs to be
    /// directly included in subsequent requests as conversation history.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Client, Turn};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // First turn
    /// let response1 = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What is 2+2?")
    ///     .create()
    ///     .await?;
    ///
    /// // Use response as model turn in follow-up
    /// let response2 = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_history(vec![
    ///         Turn::user("What is 2+2?"),
    ///         response1.as_model_turn(),
    ///         Turn::user("Now multiply that by 3"),
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_model_turn(&self) -> Turn {
        Turn::model(self.outputs.clone())
    }
}

/// Summary of content types present in an interaction response.
///
/// Returned by [`InteractionResponse::content_summary`]. Provides a quick overview
/// of what content types are present, including any unknown types.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::InteractionResponse;
/// # let response: InteractionResponse = todo!();
/// let summary = response.content_summary();
///
/// // Check for unexpected content
/// if summary.unknown_count > 0 {
///     tracing::warn!(
///         "Response contains {} unknown content types: {:?}",
///         summary.unknown_count,
///         summary.unknown_types
///     );
/// }
///
/// // Log content breakdown
/// tracing::debug!(
///     "Content: {} text, {} thoughts, {} function calls",
///     summary.text_count,
///     summary.thought_count,
///     summary.function_call_count
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentSummary {
    /// Number of text content items
    pub text_count: usize,
    /// Number of thought content items
    pub thought_count: usize,
    /// Number of image content items
    pub image_count: usize,
    /// Number of audio content items
    pub audio_count: usize,
    /// Number of video content items
    pub video_count: usize,
    /// Number of document content items (PDF files)
    pub document_count: usize,
    /// Number of function call content items
    pub function_call_count: usize,
    /// Number of function result content items
    pub function_result_count: usize,
    /// Number of code execution call content items
    pub code_execution_call_count: usize,
    /// Number of code execution result content items
    pub code_execution_result_count: usize,
    /// Number of Google Search call content items
    pub google_search_call_count: usize,
    /// Number of Google Search result content items
    pub google_search_result_count: usize,
    /// Number of URL context call content items
    pub url_context_call_count: usize,
    /// Number of URL context result content items
    pub url_context_result_count: usize,
    /// Number of file search result content items
    pub file_search_result_count: usize,
    /// Number of Google Maps call content items
    pub google_maps_call_count: usize,
    /// Number of Google Maps result content items
    pub google_maps_result_count: usize,
    /// Number of computer use call content items
    pub computer_use_call_count: usize,
    /// Number of computer use result content items
    pub computer_use_result_count: usize,
    /// Number of unknown content items
    pub unknown_count: usize,
    /// List of unique unknown type names encountered (sorted alphabetically)
    pub unknown_types: Vec<String>,
}

impl fmt::Display for ContentSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        if self.text_count > 0 {
            parts.push(format!("{} text", self.text_count));
        }
        if self.thought_count > 0 {
            parts.push(format!("{} thought", self.thought_count));
        }
        if self.image_count > 0 {
            parts.push(format!("{} image", self.image_count));
        }
        if self.audio_count > 0 {
            parts.push(format!("{} audio", self.audio_count));
        }
        if self.video_count > 0 {
            parts.push(format!("{} video", self.video_count));
        }
        if self.function_call_count > 0 {
            parts.push(format!("{} function_call", self.function_call_count));
        }
        if self.function_result_count > 0 {
            parts.push(format!("{} function_result", self.function_result_count));
        }
        if self.code_execution_call_count > 0 {
            parts.push(format!(
                "{} code_execution_call",
                self.code_execution_call_count
            ));
        }
        if self.code_execution_result_count > 0 {
            parts.push(format!(
                "{} code_execution_result",
                self.code_execution_result_count
            ));
        }
        if self.google_search_call_count > 0 {
            parts.push(format!(
                "{} google_search_call",
                self.google_search_call_count
            ));
        }
        if self.google_search_result_count > 0 {
            parts.push(format!(
                "{} google_search_result",
                self.google_search_result_count
            ));
        }
        if self.url_context_call_count > 0 {
            parts.push(format!("{} url_context_call", self.url_context_call_count));
        }
        if self.url_context_result_count > 0 {
            parts.push(format!(
                "{} url_context_result",
                self.url_context_result_count
            ));
        }
        if self.file_search_result_count > 0 {
            parts.push(format!(
                "{} file_search_result",
                self.file_search_result_count
            ));
        }
        if self.google_maps_call_count > 0 {
            parts.push(format!("{} google_maps_call", self.google_maps_call_count));
        }
        if self.google_maps_result_count > 0 {
            parts.push(format!(
                "{} google_maps_result",
                self.google_maps_result_count
            ));
        }
        if self.computer_use_call_count > 0 {
            parts.push(format!(
                "{} computer_use_call",
                self.computer_use_call_count
            ));
        }
        if self.computer_use_result_count > 0 {
            parts.push(format!(
                "{} computer_use_result",
                self.computer_use_result_count
            ));
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
            id: None,
            model: None,
            agent: None,
            input: vec![],
            outputs: vec![],
            status: InteractionStatus::Completed,
            usage,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        }
    }

    #[test]
    fn test_token_helpers_with_usage() {
        let response = minimal_response(Some(UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            total_tokens: Some(150),
            total_cached_tokens: Some(25),
            total_reasoning_tokens: Some(10),
            total_tool_use_tokens: Some(5),
            ..Default::default()
        }));

        assert_eq!(response.input_tokens(), Some(100));
        assert_eq!(response.output_tokens(), Some(50));
        assert_eq!(response.total_tokens(), Some(150));
        assert_eq!(response.cached_tokens(), Some(25));
        assert_eq!(response.reasoning_tokens(), Some(10));
        assert_eq!(response.tool_use_tokens(), Some(5));
    }

    #[test]
    fn test_token_helpers_without_usage() {
        let response = minimal_response(None);

        assert_eq!(response.input_tokens(), None);
        assert_eq!(response.output_tokens(), None);
        assert_eq!(response.total_tokens(), None);
        assert_eq!(response.cached_tokens(), None);
        assert_eq!(response.reasoning_tokens(), None);
        assert_eq!(response.tool_use_tokens(), None);
    }

    #[test]
    fn test_token_helpers_with_partial_usage() {
        // Test case where only some token counts are available
        let response = minimal_response(Some(UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            total_tokens: Some(150),
            total_cached_tokens: None,
            total_reasoning_tokens: None,
            total_tool_use_tokens: None,
            ..Default::default()
        }));

        assert_eq!(response.input_tokens(), Some(100));
        assert_eq!(response.output_tokens(), Some(50));
        assert_eq!(response.total_tokens(), Some(150));
        assert_eq!(response.cached_tokens(), None);
        assert_eq!(response.reasoning_tokens(), None);
        assert_eq!(response.tool_use_tokens(), None);
    }

    // =========================================================================
    // ModalityTokens Tests
    // =========================================================================

    #[test]
    fn test_modality_tokens_serialization() {
        let tokens = ModalityTokens {
            modality: "TEXT".to_string(),
            tokens: 100,
        };

        let json = serde_json::to_string(&tokens).unwrap();
        assert!(json.contains("\"modality\":\"TEXT\""));
        assert!(json.contains("\"tokens\":100"));

        // Roundtrip
        let deserialized: ModalityTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.modality, "TEXT");
        assert_eq!(deserialized.tokens, 100);
    }

    #[test]
    fn test_modality_tokens_deserialization() {
        let json = r#"{"modality": "IMAGE", "tokens": 500}"#;
        let tokens: ModalityTokens = serde_json::from_str(json).unwrap();
        assert_eq!(tokens.modality, "IMAGE");
        assert_eq!(tokens.tokens, 500);
    }

    #[test]
    fn test_input_tokens_for_modality() {
        let usage = UsageMetadata {
            input_tokens_by_modality: Some(vec![
                ModalityTokens {
                    modality: "TEXT".to_string(),
                    tokens: 100,
                },
                ModalityTokens {
                    modality: "IMAGE".to_string(),
                    tokens: 500,
                },
                ModalityTokens {
                    modality: "AUDIO".to_string(),
                    tokens: 200,
                },
            ]),
            ..Default::default()
        };

        assert_eq!(usage.input_tokens_for_modality("TEXT"), Some(100));
        assert_eq!(usage.input_tokens_for_modality("IMAGE"), Some(500));
        assert_eq!(usage.input_tokens_for_modality("AUDIO"), Some(200));
        assert_eq!(usage.input_tokens_for_modality("VIDEO"), None);
    }

    #[test]
    fn test_input_tokens_for_modality_none() {
        let usage = UsageMetadata::default();
        assert_eq!(usage.input_tokens_for_modality("TEXT"), None);
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

        // 100% cache hit rate
        let usage = UsageMetadata {
            total_input_tokens: Some(100),
            total_cached_tokens: Some(100),
            ..Default::default()
        };
        let rate = usage.cache_hit_rate().unwrap();
        assert!((rate - 1.0).abs() < f32::EPSILON);

        // 0% cache hit rate
        let usage = UsageMetadata {
            total_input_tokens: Some(100),
            total_cached_tokens: Some(0),
            ..Default::default()
        };
        let rate = usage.cache_hit_rate().unwrap();
        assert!((rate - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cache_hit_rate_none_cases() {
        // Missing cached tokens
        let usage = UsageMetadata {
            total_input_tokens: Some(100),
            total_cached_tokens: None,
            ..Default::default()
        };
        assert!(usage.cache_hit_rate().is_none());

        // Missing input tokens
        let usage = UsageMetadata {
            total_input_tokens: None,
            total_cached_tokens: Some(25),
            ..Default::default()
        };
        assert!(usage.cache_hit_rate().is_none());

        // Zero input tokens (avoid division by zero)
        let usage = UsageMetadata {
            total_input_tokens: Some(0),
            total_cached_tokens: Some(0),
            ..Default::default()
        };
        assert!(usage.cache_hit_rate().is_none());
    }

    #[test]
    fn test_has_data_with_modality_breakdowns() {
        // Only modality breakdowns present
        let usage = UsageMetadata {
            input_tokens_by_modality: Some(vec![ModalityTokens {
                modality: "TEXT".to_string(),
                tokens: 100,
            }]),
            ..Default::default()
        };
        assert!(usage.has_data());

        // Empty default
        let usage = UsageMetadata::default();
        assert!(!usage.has_data());
    }

    #[test]
    fn test_usage_metadata_with_modality_breakdowns_serialization() {
        let usage = UsageMetadata {
            total_input_tokens: Some(600),
            total_output_tokens: Some(100),
            input_tokens_by_modality: Some(vec![
                ModalityTokens {
                    modality: "TEXT".to_string(),
                    tokens: 100,
                },
                ModalityTokens {
                    modality: "IMAGE".to_string(),
                    tokens: 500,
                },
            ]),
            output_tokens_by_modality: Some(vec![ModalityTokens {
                modality: "TEXT".to_string(),
                tokens: 100,
            }]),
            ..Default::default()
        };

        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("input_tokens_by_modality"));
        assert!(json.contains("output_tokens_by_modality"));

        // Roundtrip
        let deserialized: UsageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_input_tokens, Some(600));
        assert_eq!(
            deserialized
                .input_tokens_by_modality
                .as_ref()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            deserialized
                .output_tokens_by_modality
                .as_ref()
                .unwrap()
                .len(),
            1
        );
    }

    // =========================================================================
    // Token Count Deserialization Edge Cases
    // =========================================================================

    #[test]
    fn test_negative_token_count_clamped_to_zero() {
        // When API returns negative token counts (shouldn't happen but be defensive)
        let json = r#"{"total_input_tokens": -100, "total_output_tokens": 50}"#;
        let usage: UsageMetadata = serde_json::from_str(json).unwrap();

        // Negative values are clamped to 0
        assert_eq!(usage.total_input_tokens, Some(0));
        assert_eq!(usage.total_output_tokens, Some(50));
    }

    #[test]
    fn test_modality_tokens_negative_clamped() {
        let json = r#"{"modality": "TEXT", "tokens": -50}"#;
        let tokens: ModalityTokens = serde_json::from_str(json).unwrap();

        assert_eq!(tokens.modality, "TEXT");
        assert_eq!(tokens.tokens, 0);
    }

    #[test]
    fn test_large_token_count_clamped_to_u32_max() {
        // Value larger than u32::MAX (4,294,967,295)
        let json = r#"{"total_input_tokens": 5000000000}"#;
        let usage: UsageMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(usage.total_input_tokens, Some(u32::MAX));
    }

    #[test]
    fn test_valid_token_counts_unchanged() {
        let json = r#"{
            "total_input_tokens": 100,
            "total_output_tokens": 50,
            "total_tokens": 150,
            "total_cached_tokens": 25
        }"#;
        let usage: UsageMetadata = serde_json::from_str(json).unwrap();

        assert_eq!(usage.total_input_tokens, Some(100));
        assert_eq!(usage.total_output_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(150));
        assert_eq!(usage.total_cached_tokens, Some(25));
    }

    // =========================================================================
    // Image Helper Tests
    // =========================================================================

    fn make_response_with_image(base64_data: &str, mime_type: Option<&str>) -> InteractionResponse {
        InteractionResponse {
            id: Some("test-id".to_string()),
            model: Some("test-model".to_string()),
            agent: None,
            input: vec![],
            outputs: vec![Content::Image {
                data: Some(base64_data.to_string()),
                mime_type: mime_type.map(String::from),
                uri: None,
                resolution: None,
            }],
            status: InteractionStatus::Completed,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        }
    }

    fn make_response_no_images() -> InteractionResponse {
        InteractionResponse {
            id: Some("test-id".to_string()),
            model: Some("test-model".to_string()),
            agent: None,
            input: vec![],
            outputs: vec![Content::Text {
                text: Some("Hello".to_string()),
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
        }
    }

    #[test]
    fn test_first_image_bytes_success() {
        // Base64 for "test"
        let base64_data = "dGVzdA==";
        let response = make_response_with_image(base64_data, Some("image/png"));

        let result = response.first_image_bytes();
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert!(bytes.is_some());
        assert_eq!(bytes.unwrap(), b"test");
    }

    #[test]
    fn test_first_image_bytes_no_images() {
        let response = make_response_no_images();

        let result = response.first_image_bytes();
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_first_image_bytes_invalid_base64() {
        let response = make_response_with_image("not-valid-base64!!!", Some("image/png"));

        let result = response.first_image_bytes();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid base64"));
    }

    #[test]
    fn test_images_iterator() {
        // Create response with multiple images
        let response = InteractionResponse {
            id: Some("test-id".to_string()),
            model: Some("test-model".to_string()),
            agent: None,
            input: vec![],
            outputs: vec![
                Content::Image {
                    data: Some("dGVzdDE=".to_string()), // "test1"
                    mime_type: Some("image/png".to_string()),
                    uri: None,
                    resolution: None,
                },
                Content::Text {
                    text: Some("text between".to_string()),
                    annotations: None,
                },
                Content::Image {
                    data: Some("dGVzdDI=".to_string()), // "test2"
                    mime_type: Some("image/jpeg".to_string()),
                    uri: None,
                    resolution: None,
                },
            ],
            status: InteractionStatus::Completed,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        };

        let images: Vec<_> = response.images().collect();
        assert_eq!(images.len(), 2);

        assert_eq!(images[0].bytes().unwrap(), b"test1");
        assert_eq!(images[0].mime_type(), Some("image/png"));
        assert_eq!(images[0].extension(), "png");

        assert_eq!(images[1].bytes().unwrap(), b"test2");
        assert_eq!(images[1].mime_type(), Some("image/jpeg"));
        assert_eq!(images[1].extension(), "jpg");
    }

    #[test]
    fn test_has_images() {
        let response_with = make_response_with_image("dGVzdA==", Some("image/png"));
        assert!(response_with.has_images());

        let response_without = make_response_no_images();
        assert!(!response_without.has_images());
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

    #[test]
    fn test_image_info_bytes_invalid_base64() {
        let info = ImageInfo {
            data: "not-valid-base64!!!",
            mime_type: Some("image/png"),
        };
        let result = info.bytes();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid base64"));
    }

    #[test]
    fn test_image_info_extension_unknown_mime_type() {
        // This test documents Evergreen-compliant behavior:
        // Unknown MIME types default to "png" and log a warning (not verified here)
        // to surface API evolution without breaking user code.
        let info = ImageInfo {
            data: "",
            mime_type: Some("image/future-format"),
        };
        assert_eq!(info.extension(), "png");

        // Completely novel MIME type also defaults gracefully
        let info2 = ImageInfo {
            data: "",
            mime_type: Some("application/octet-stream"),
        };
        assert_eq!(info2.extension(), "png");
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
        check(Some("audio/L16"), "pcm");
        check(Some("audio/unknown"), "wav"); // default
        check(None, "wav"); // default
    }

    #[test]
    fn test_audio_info_bytes_valid_base64() {
        // Base64 for "test"
        let info = AudioInfo {
            data: "dGVzdA==",
            mime_type: Some("audio/wav"),
        };
        let result = info.bytes();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"test");
    }

    #[test]
    fn test_audio_info_bytes_invalid_base64() {
        let info = AudioInfo {
            data: "not-valid-base64!!!",
            mime_type: Some("audio/wav"),
        };
        let result = info.bytes();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid base64"));
    }

    #[test]
    fn test_audio_info_extension_unknown_mime_type() {
        // Evergreen-compliant behavior: unknown MIME types default to "wav"
        // and log a warning to surface API evolution.
        let info = AudioInfo {
            data: "",
            mime_type: Some("audio/future-format"),
        };
        assert_eq!(info.extension(), "wav");
    }

    #[test]
    fn test_usage_metadata_accumulate_both_have_values() {
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            total_tokens: Some(150),
            ..Default::default()
        };
        let usage2 = UsageMetadata {
            total_input_tokens: Some(200),
            total_output_tokens: Some(75),
            total_tokens: Some(275),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        assert_eq!(usage1.total_input_tokens, Some(300));
        assert_eq!(usage1.total_output_tokens, Some(125));
        assert_eq!(usage1.total_tokens, Some(425));
    }

    #[test]
    fn test_usage_metadata_accumulate_self_has_none() {
        let mut usage1 = UsageMetadata::default();
        let usage2 = UsageMetadata {
            total_input_tokens: Some(200),
            total_output_tokens: Some(75),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        assert_eq!(usage1.total_input_tokens, Some(200));
        assert_eq!(usage1.total_output_tokens, Some(75));
    }

    #[test]
    fn test_usage_metadata_accumulate_other_has_none() {
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            ..Default::default()
        };
        let usage2 = UsageMetadata::default();

        usage1.accumulate(&usage2);

        // Values should remain unchanged
        assert_eq!(usage1.total_input_tokens, Some(100));
        assert_eq!(usage1.total_output_tokens, Some(50));
    }

    #[test]
    fn test_usage_metadata_accumulate_all_fields() {
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(50),
            total_tokens: Some(150),
            total_cached_tokens: Some(20),
            total_reasoning_tokens: Some(10),
            total_thought_tokens: Some(5),
            total_tool_use_tokens: Some(15),
            ..Default::default()
        };
        let usage2 = UsageMetadata {
            total_input_tokens: Some(200),
            total_output_tokens: Some(100),
            total_tokens: Some(300),
            total_cached_tokens: Some(40),
            total_reasoning_tokens: Some(20),
            total_thought_tokens: Some(10),
            total_tool_use_tokens: Some(30),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        assert_eq!(usage1.total_input_tokens, Some(300));
        assert_eq!(usage1.total_output_tokens, Some(150));
        assert_eq!(usage1.total_tokens, Some(450));
        assert_eq!(usage1.total_cached_tokens, Some(60));
        assert_eq!(usage1.total_reasoning_tokens, Some(30));
        assert_eq!(usage1.total_thought_tokens, Some(15));
        assert_eq!(usage1.total_tool_use_tokens, Some(45));
    }

    #[test]
    fn test_usage_metadata_accumulate_zero_values() {
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(0),
            total_output_tokens: Some(50),
            ..Default::default()
        };
        let usage2 = UsageMetadata {
            total_input_tokens: Some(100),
            total_output_tokens: Some(0),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        assert_eq!(usage1.total_input_tokens, Some(100));
        assert_eq!(usage1.total_output_tokens, Some(50));
    }

    #[test]
    fn test_usage_metadata_accumulate_saturating() {
        // Test that we don't overflow - use saturating_add
        let mut usage1 = UsageMetadata {
            total_input_tokens: Some(u32::MAX - 10),
            ..Default::default()
        };
        let usage2 = UsageMetadata {
            total_input_tokens: Some(100),
            ..Default::default()
        };

        usage1.accumulate(&usage2);

        // Should saturate at u32::MAX, not wrap around
        assert_eq!(usage1.total_input_tokens, Some(u32::MAX));
    }

    // =========================================================================
    // InteractionStatus Tests
    // =========================================================================

    #[test]
    fn test_interaction_status_incomplete_roundtrip() {
        // Deserialize from wire format
        let json = r#""incomplete""#;
        let status: InteractionStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, InteractionStatus::Incomplete);

        // Serialize back to wire format
        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(serialized, r#""incomplete""#);

        // Roundtrip
        let roundtripped: InteractionStatus = serde_json::from_str(&serialized).unwrap();
        assert_eq!(roundtripped, InteractionStatus::Incomplete);
    }
}
