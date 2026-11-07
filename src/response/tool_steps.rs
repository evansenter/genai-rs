//! [`InteractionResponse`] accessors for built-in tool steps: code
//! execution, Google Search, URL context, generic tool calls, file search
//! and Google Maps. See the [parent module](super).

use super::InteractionResponse;
use super::views::{
    CodeExecutionCallInfo, CodeExecutionResultInfo, GoogleMapsResultInfo, ToolCallInfo,
    UrlContextResultInfo,
};
use crate::content::GoogleSearchResultItem;
use crate::steps::Step;

impl InteractionResponse {
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
                        result,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the first successful code execution output, if any.
    /// [`CodeExecutionOutput::text`](crate::CodeExecutionOutput::text) reads
    /// it as text.
    #[must_use]
    pub fn successful_code_output(&self) -> Option<&crate::CodeExecutionOutput> {
        self.steps.iter().find_map(|step| {
            if let Step::CodeExecutionResult {
                is_error: false,
                result,
                ..
            } = step
            {
                Some(result)
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

    /// Extract all Google Search queries from steps (flattened across calls).
    #[must_use]
    pub fn google_search_calls(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter_map(|step| {
                if let Step::GoogleSearchCall { queries, .. } = step {
                    Some(queries.iter().map(String::as_str))
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
    // Generic Tool Call Step Helpers
    // =========================================================================

    /// Whether the response contains generic `tool_call` steps.
    ///
    /// The oldest MCP shape arrived as these; MCP calls now arrive as
    /// `mcp_server_tool_call` steps (as `function_call` pairs in the last
    /// streams seen), both of which [`mcp_calls`](Self::mcp_calls) reads.
    /// See [`tool_calls`](Self::tool_calls).
    #[must_use]
    pub fn has_tool_calls(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::ToolCall { .. }))
    }

    /// The generic `tool_call` steps, in order.
    ///
    /// These are server-side tool invocations the API does not further
    /// identify: the oldest MCP shape, carrying only an `id` and an optional
    /// `signature`. MCP calls now arrive as `mcp_server_tool_call` steps
    /// in a non-streamed or stored response (in between, and in the last
    /// streams seen, as answered `function_call` steps named
    /// `<server>:<tool>`), which [`mcp_calls`](Self::mcp_calls) reads with
    /// the server and tool named.
    ///
    /// Returns a borrowed view carrying both fields, matching
    /// [`function_calls`](Self::function_calls) — the `signature` is what a
    /// caller needs for stateless replay, so returning bare ids would not do.
    /// See #433.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<ToolCallInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                Step::ToolCall { id, signature, .. } => Some(ToolCallInfo {
                    id: id.as_str(),
                    signature: signature.as_deref(),
                }),
                _ => None,
            })
            .collect()
    }

    // =========================================================================
    // File Search Step Helpers
    // =========================================================================

    /// Whether the response contains a `file_search_result` step, that is,
    /// whether a file search ran.
    ///
    /// The step carries no results: the retrieved chunks
    /// are folded into the answer, which cites them with `file_citation`
    /// [`Annotation`](crate::Annotation)s. Read those with
    /// [`all_annotations`](Self::all_annotations).
    #[must_use]
    pub fn has_file_search_results(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::FileSearchResult { .. }))
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
}

#[cfg(test)]
#[path = "tool_steps_tests.rs"]
mod tests;
