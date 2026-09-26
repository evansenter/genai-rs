//! Response types for the Interactions API.
//!
//! This module contains `InteractionResponse` and related types for handling
//! API responses, including helper methods for extracting content from the
//! `steps` array (API revision 2026-05-20).

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::content::{Annotation, Content};
use crate::errors::GenaiError;
use crate::request::{InteractionInput, ServiceTier};
use crate::steps::Step;
use crate::tools::Tool;
use crate::wire_enum::wire_enum;

mod step_summary;
mod tool_steps;
mod usage;
mod views;

pub use step_summary::StepSummary;
pub use usage::{GroundingToolCount, ModalityTokens, UsageMetadata};
pub use views::{
    AudioInfo, CodeExecutionCallInfo, CodeExecutionResultInfo, FunctionCallInfo,
    FunctionResultInfo, GoogleMapsResultInfo, ImageInfo, OwnedFunctionCallInfo, ToolCallInfo,
    UrlContextResultInfo,
};

wire_enum! {
    /// Status of an interaction.
    #[derive(Default)]
    pub enum InteractionStatus {
        /// Interaction completed successfully.
        Completed = "completed",
        /// Interaction is still being processed.
        ///
        /// This is the `Default` (used when a hand-constructed response omits a
        /// status; the wire always carries one).
        #[default]
        InProgress = "in_progress",
        /// Interaction requires client action (e.g., function results).
        RequiresAction = "requires_action",
        /// Interaction failed.
        Failed = "failed",
        /// Interaction was cancelled.
        Cancelled = "cancelled",
        /// Interaction ended before completion (e.g., token limit reached).
        Incomplete = "incomplete",
        /// Interaction stopped because the configured budget was exceeded.
        BudgetExceeded = "budget_exceeded",
    }
    unknown(status_type, unknown_status_type)
}

/// Response from creating or retrieving an interaction.
///
/// Under API revision 2026-05-20 the response carries a `steps` array; use the
/// convenience helpers (`as_text()`, `function_calls()`, `images()`, ...) or
/// iterate [`InteractionResponse::steps`] directly.
#[derive(Clone, Deserialize, Serialize, Debug, Default)]
#[serde(default)]
#[non_exhaustive]
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

    /// Resource type discriminator returned by the API.
    ///
    /// The live API returns `"interaction"` on every response (verified
    /// 2026-07 against Api-Revision 2026-05-20). Preserved for lossless
    /// roundtrip per Evergreen principles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,

    /// The service tier that actually processed this request.
    ///
    /// The live API echoes the effective tier (e.g. `"standard"`) on every
    /// response, including when the request did not set `service_tier`
    /// (verified 2026-07 against Api-Revision 2026-05-20).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,

    /// The per-request webhook routing config, echoed back by the API.
    ///
    /// The live API echoes the request's `webhook_config` (`uris` +
    /// `user_metadata`) verbatim on the create response (verified 2026-07
    /// against Api-Revision 2026-05-20). Preserved for lossless roundtrip
    /// per Evergreen principles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_config: Option<crate::webhooks::WebhookConfig>,

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

    /// The system instruction the interaction ran with, as echoed by the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,

    /// The request's labels, as echoed by the API (verified live
    /// 2026-09-24).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::BTreeMap<String, String>>,

    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen): e.g. the `environment`, `generation_config`
    /// and `agent_config` echoes.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What is 2+2?")
    ///     .create().await?;
    ///
    /// let mut history = vec![Step::user_text("What is 2+2?")];
    /// history.extend(first.output_steps());
    /// history.push(Step::user_text("Now multiply that by 3"));
    ///
    /// let second = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
                    ..
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
                    ..
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
    /// Declaring a tool without using it does not count — measured
    /// 2026-08-16 — so a non-zero value means a tool was actually invoked.
    /// It is a single aggregate across all tools, though, so it identifies
    /// *which* tool only when one is declared.
    /// Returns `None` when usage metadata is absent, or when the API omitted
    /// the field — not when tools went unused, which yields `Some(0)`.
    #[must_use]
    pub fn tool_use_tokens(&self) -> Option<u32> {
        self.usage.as_ref().and_then(|u| u.total_tool_use_tokens)
    }
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
