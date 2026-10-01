//! [`StepSummary`]: per-type counts of the steps and content blocks in an
//! interaction response. See the [parent module](super).

use std::collections::BTreeSet;
use std::fmt;

use super::InteractionResponse;
use crate::content::Content;
use crate::steps::Step;

/// Summary of step and content types present in an interaction response.
///
/// Returned by [`InteractionResponse::step_summary`]. Provides a quick
/// overview of what step types are present, including any unknown types.
///
/// Content counts (`text_count`, `image_count`, ...) tally content blocks
/// inside `model_output` steps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
// Closed so that counters for new step types are additive; build one with
// `StepSummary::default()` and field assignment.
#[non_exhaustive]
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
    /// Number of generic `tool_call` steps — server-side tool invocations
    /// the API does not further identify.
    ///
    /// **This is where MCP calls land.** The endpoint emits `tool_call`
    /// rather than `mcp_server_tool_call` (verified live 2026-08-16), so
    /// [`mcp_server_tool_call_count`](Self::mcp_server_tool_call_count)
    /// reads 0 on a successful MCP interaction while this reads non-zero.
    /// Check this one. See #433.
    pub tool_call_count: usize,
    /// Number of `mcp_server_tool_call` steps.
    ///
    /// **Expect 0.** The API emits generic `tool_call` steps for MCP; see
    /// [`tool_call_count`](Self::tool_call_count). Retained because the
    /// step type is spec-defined and may start arriving.
    ///
    /// [`InteractionResponse::tool_use_tokens`] is the other signal that
    /// MCP ran, with the caveat that it is a single aggregate across all
    /// tools — so it isolates the MCP server only when MCP is the sole
    /// declared tool.
    pub mcp_server_tool_call_count: usize,
    /// Number of `mcp_server_tool_result` steps.
    ///
    /// **Expect 0**, for the same reason as
    /// [`mcp_server_tool_call_count`](Self::mcp_server_tool_call_count) —
    /// the API emits neither of the pair today — and with the same
    /// consequence, since 0 here reads as "the MCP call returned nothing"
    /// rather than "we do not model what came back".
    pub mcp_server_tool_result_count: usize,
    /// Number of `file_search_call` steps
    pub file_search_call_count: usize,
    /// Number of `file_search_result` steps
    pub file_search_result_count: usize,
    /// Number of `google_maps_call` steps
    pub google_maps_call_count: usize,
    /// Number of `google_maps_result` steps
    pub google_maps_result_count: usize,
    /// Number of `processing_call` steps (agentic video processing)
    pub processing_call_count: usize,
    /// Number of `processing_result` steps
    pub processing_result_count: usize,
    /// Number of `retrieval_call` steps (Vertex-only retrieval tool)
    pub retrieval_call_count: usize,
    /// Number of `retrieval_result` steps
    pub retrieval_result_count: usize,
    /// Number of unknown steps/content blocks
    pub unknown_count: usize,
    /// List of unique unknown type names encountered (sorted alphabetically)
    pub unknown_types: Vec<String>,
}

impl fmt::Display for StepSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();

        let fields: [(&str, usize); 27] = [
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
            ("tool_call", self.tool_call_count),
            ("mcp_server_tool_call", self.mcp_server_tool_call_count),
            ("mcp_server_tool_result", self.mcp_server_tool_result_count),
            ("file_search_call", self.file_search_call_count),
            ("file_search_result", self.file_search_result_count),
            ("google_maps_call", self.google_maps_call_count),
            ("google_maps_result", self.google_maps_result_count),
            ("processing_call", self.processing_call_count),
            ("processing_result", self.processing_result_count),
            ("retrieval_call", self.retrieval_call_count),
            ("retrieval_result", self.retrieval_result_count),
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

impl InteractionResponse {
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
                Step::ToolCall { .. } => summary.tool_call_count += 1,
                Step::McpServerToolCall { .. } => summary.mcp_server_tool_call_count += 1,
                Step::McpServerToolResult { .. } => summary.mcp_server_tool_result_count += 1,
                Step::FileSearchCall { .. } => summary.file_search_call_count += 1,
                Step::FileSearchResult { .. } => summary.file_search_result_count += 1,
                Step::GoogleMapsCall { .. } => summary.google_maps_call_count += 1,
                Step::GoogleMapsResult { .. } => summary.google_maps_result_count += 1,
                Step::ProcessingCall { .. } => summary.processing_call_count += 1,
                Step::ProcessingResult { .. } => summary.processing_result_count += 1,
                Step::RetrievalCall { .. } => summary.retrieval_call_count += 1,
                Step::RetrievalResult { .. } => summary.retrieval_result_count += 1,
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
}

#[cfg(test)]
#[path = "step_summary_tests.rs"]
mod tests;
