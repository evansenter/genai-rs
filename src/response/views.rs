//! Borrowed view types returned by the
//! [`InteractionResponse`](super::InteractionResponse) accessors, plus
//! [`OwnedFunctionCallInfo`]. See the [parent module](super).

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::content::CodeExecutionLanguage;
use crate::errors::GenaiError;
use crate::steps::FunctionResultPayload;

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
///     .with_model(genai_rs::DEFAULT_MODEL)
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
    pub(super) data: &'a str,
    pub(super) mime_type: Option<&'a str>,
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
///     .with_model(genai_rs::DEFAULT_TTS_MODEL)
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
    pub(super) data: &'a str,
    pub(super) mime_type: Option<&'a str>,
    pub(super) sample_rate: Option<u32>,
    pub(super) channels: Option<u32>,
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
///
/// [`InteractionResponse::function_calls()`]: crate::InteractionResponse::function_calls
/// [`InteractionResponse`]: crate::InteractionResponse
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
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

/// A generic server-side tool call, borrowed from the response.
///
/// This is what an MCP invocation looks like — the API does not identify the
/// server or the tool, so `id` and `signature` are all there is. See
/// [`InteractionResponse::tool_calls`].
///
/// [`InteractionResponse::tool_calls`]: crate::InteractionResponse::tool_calls
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ToolCallInfo<'a> {
    /// Unique identifier for this call.
    pub id: &'a str,
    /// Opaque signature; pass back unchanged when replaying statelessly.
    ///
    /// Skipped when absent so a logged `ToolCallInfo` and a logged
    /// [`Step::ToolCall`] show the same shape for the same missing field —
    /// the hand-written `Serialize` for the step omits the key rather than
    /// emitting `null`, and a byte-for-byte roundtrip test pins that.
    ///
    /// [`Step::ToolCall`]: crate::Step::ToolCall
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<&'a str>,
}

/// Owned version of [`FunctionCallInfo`] for storing beyond response lifetime.
///
/// This type owns all its data, making it suitable for:
/// - Event emission with function call metadata
/// - Trajectory/replay recording
/// - Passing to async tasks or storing in collections
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
///
/// [`InteractionResponse::function_results()`]: crate::InteractionResponse::function_results
/// [`InteractionResponse`]: crate::InteractionResponse
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
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
///
/// [`InteractionResponse::code_execution_calls()`]: crate::InteractionResponse::code_execution_calls
/// [`InteractionResponse`]: crate::InteractionResponse
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
///
/// [`InteractionResponse::code_execution_results()`]: crate::InteractionResponse::code_execution_results
/// [`InteractionResponse`]: crate::InteractionResponse
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
///
/// [`InteractionResponse::url_context_results()`]: crate::InteractionResponse::url_context_results
/// [`InteractionResponse`]: crate::InteractionResponse
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
///
/// [`InteractionResponse::google_maps_results()`]: crate::InteractionResponse::google_maps_results
/// [`InteractionResponse`]: crate::InteractionResponse
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct GoogleMapsResultInfo<'a> {
    /// The ID of the corresponding Google Maps call
    pub call_id: &'a str,
    /// The result items containing place data
    pub items: &'a [crate::GoogleMapsResultItem],
}

#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;
