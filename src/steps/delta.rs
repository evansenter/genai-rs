use crate::content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, Resolution, UrlContextResultItem,
};

use super::FunctionResultPayload;

// =============================================================================
// StepDelta (streaming `step.delta` payloads)
// =============================================================================

/// The payload of a `step.delta` SSE event.
///
/// Deltas incrementally build the step announced by the matching `step.start`
/// event: text arrives in fragments, function-call arguments stream as JSON
/// string fragments (`arguments_delta`), thought summaries and signatures
/// arrive separately, and built-in tool calls/results are pushed as they
/// resolve.
///
/// # Forward Compatibility
///
/// `#[non_exhaustive]`; unrecognized delta types deserialize into
/// [`StepDelta::Unknown`] with data preserved.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum StepDelta {
    /// Text fragment (`type: "text"`).
    Text {
        /// The text fragment.
        text: String,
    },
    /// Image data (`type: "image"`).
    Image {
        /// Base64 image data.
        data: Option<String>,
        /// URI reference.
        uri: Option<String>,
        /// MIME type.
        mime_type: Option<String>,
        /// Media resolution.
        resolution: Option<Resolution>,
    },
    /// Audio data (`type: "audio"`).
    Audio {
        /// Base64 audio data.
        data: Option<String>,
        /// URI reference.
        uri: Option<String>,
        /// MIME type.
        mime_type: Option<String>,
        /// Sample rate in Hz (legacy field name).
        rate: Option<u32>,
        /// Sample rate in Hz.
        sample_rate: Option<u32>,
        /// Number of audio channels.
        channels: Option<u32>,
    },
    /// Video data (`type: "video"`).
    Video {
        /// Base64 video data.
        data: Option<String>,
        /// URI reference.
        uri: Option<String>,
        /// MIME type.
        mime_type: Option<String>,
        /// Media resolution.
        resolution: Option<Resolution>,
    },
    /// Document data (`type: "document"`).
    Document {
        /// Base64 document data.
        data: Option<String>,
        /// URI reference.
        uri: Option<String>,
        /// MIME type.
        mime_type: Option<String>,
    },
    /// Thought summary content (`type: "thought_summary"`).
    ThoughtSummary {
        /// The summary content block (text or image).
        content: Option<Content>,
    },
    /// Thought signature fragment (`type: "thought_signature"`).
    ThoughtSignature {
        /// The signature fragment.
        signature: Option<String>,
    },
    /// Text annotations (`type: "text_annotation_delta"`).
    TextAnnotation {
        /// Citation annotations for previously streamed text.
        annotations: Vec<Annotation>,
    },
    /// Function-call arguments fragment (`type: "arguments_delta"`).
    ///
    /// Arguments for a `function_call` step stream incrementally as raw JSON
    /// string fragments; concatenate them and parse when the step stops.
    ArgumentsDelta {
        /// The raw JSON fragment.
        arguments: String,
    },
    /// Function result (`type: "function_result"`).
    FunctionResult {
        /// The `id` of the call this result responds to. Absent from the
        /// 2.25 bindings, so it may stop arriving; the step's `call_id` from
        /// `step.start` is kept when it does.
        call_id: Option<String>,
        /// Function name.
        name: Option<String>,
        /// The result payload.
        result: FunctionResultPayload,
        /// Whether execution errored.
        is_error: Option<bool>,
    },
    /// Code execution call delta (`type: "code_execution_call"`).
    CodeExecutionCall {
        /// Programming language.
        language: Option<CodeExecutionLanguage>,
        /// Source code fragment.
        code: Option<String>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Code execution result delta (`type: "code_execution_result"`).
    CodeExecutionResult {
        /// Execution output.
        result: String,
        /// Whether execution errored.
        is_error: Option<bool>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// URL context call delta (`type: "url_context_call"`).
    UrlContextCall {
        /// URLs to fetch.
        urls: Vec<String>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// URL context result delta (`type: "url_context_result"`).
    UrlContextResult {
        /// Per-URL retrieval results.
        result: Vec<UrlContextResultItem>,
        /// Whether the fetch errored.
        is_error: Option<bool>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Google Search call delta (`type: "google_search_call"`).
    GoogleSearchCall {
        /// Search queries.
        queries: Vec<String>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Google Search result delta (`type: "google_search_result"`).
    GoogleSearchResult {
        /// Search results.
        result: Vec<GoogleSearchResultItem>,
        /// Whether the search errored.
        is_error: Option<bool>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// MCP server tool call delta (`type: "mcp_server_tool_call"`).
    ///
    /// Not currently emitted by the API — see
    /// [`Step::McpServerToolCall`](crate::Step::McpServerToolCall) for the verified wire behaviour
    /// and #459.
    McpServerToolCall {
        /// Tool name.
        name: String,
        /// MCP server name.
        server_name: String,
        /// Tool arguments.
        arguments: serde_json::Value,
    },
    /// MCP server tool result delta (`type: "mcp_server_tool_result"`).
    ///
    /// Not currently emitted by the API — see
    /// [`Step::McpServerToolCall`](crate::Step::McpServerToolCall) for the verified wire behaviour
    /// and #459.
    McpServerToolResult {
        /// Tool name.
        name: Option<String>,
        /// MCP server name.
        server_name: Option<String>,
        /// The result payload.
        result: FunctionResultPayload,
    },
    /// File search call delta (`type: "file_search_call"`).
    FileSearchCall {
        /// Opaque signature.
        signature: Option<String>,
    },
    /// File search result delta (`type: "file_search_result"`).
    FileSearchResult {
        /// Retrieved chunks.
        result: Vec<FileSearchResultItem>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Google Maps call delta (`type: "google_maps_call"`).
    GoogleMapsCall {
        /// Location queries.
        queries: Vec<String>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Google Maps result delta (`type: "google_maps_result"`).
    GoogleMapsResult {
        /// Places and widget data.
        result: Vec<GoogleMapsResultItem>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Processing call delta (`type: "processing_call"`). Carries the
    /// signature that `step.start` announces as `""`.
    ProcessingCall {
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Processing result delta (`type: "processing_result"`).
    ProcessingResult {
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Retrieval call delta (`type: "retrieval_call"`); Vertex-only.
    RetrievalCall {
        /// Retrieval queries.
        queries: Vec<String>,
        /// Which retrieval backend handled the call.
        retrieval_type: Option<crate::tools::RetrievalType>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Retrieval result delta (`type: "retrieval_result"`); Vertex-only.
    RetrievalResult {
        /// Whether the retrieval errored.
        is_error: Option<bool>,
        /// Opaque signature.
        signature: Option<String>,
    },
    /// Unknown delta type for forward compatibility.
    Unknown {
        /// The unrecognized type name from the API.
        delta_type: String,
        /// The full JSON data, preserved for debugging and roundtrip.
        data: serde_json::Value,
    },
}

impl StepDelta {
    /// Check if this is an unknown delta type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the delta type name if this is an unknown delta.
    #[must_use]
    pub fn unknown_delta_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { delta_type, .. } => Some(delta_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown delta.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Returns the text fragment if this is a `Text` delta.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Returns the raw arguments fragment if this is an `ArgumentsDelta`.
    #[must_use]
    pub fn as_arguments_delta(&self) -> Option<&str> {
        match self {
            Self::ArgumentsDelta { arguments } => Some(arguments),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "delta_tests.rs"]
mod tests;
