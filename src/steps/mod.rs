//! Step types for the Interactions API (revision 2026-05-20).
//!
//! Under API revision 2026-05-20 the response body carries a `steps` array
//! instead of the launch-era `outputs` array. Each [`Step`] is a typed record
//! of one unit of work in the interaction: user input, model output, internal
//! reasoning (thoughts), and calls/results for client-side functions and
//! server-side tools.
//!
//! Content (text, images, audio, video, documents) is nested inside the
//! `user_input` / `model_output` steps as [`Content`] blocks.
//!
//! # Forward Compatibility
//!
//! [`Step`], [`StepDelta`] and [`FunctionResultPayload`] follow the
//! [Evergreen](https://github.com/google-deepmind/evergreen-spec) philosophy:
//! unrecognized wire data is preserved in `Unknown` variants rather than
//! rejected.

use serde::{Deserialize, Serialize};

use crate::content::{
    CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, UrlContextResultItem,
};

mod accumulator;
mod delta;
mod delta_serde;
mod step_serde;

pub(crate) use accumulator::StepAccumulator;
pub use delta::StepDelta;

// =============================================================================
// StepError (wire: google.rpc.Status-like {code, message, details})
// =============================================================================

/// Error details attached to a `model_output` step.
///
/// Mirrors the API's status shape: `{code, message, details}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct StepError {
    /// Numeric error code, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    /// Human-readable error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Additional structured error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
}

// =============================================================================
// FunctionResultPayload
// =============================================================================

/// The `result` of a `function_result` (or `mcp_server_tool_result`) step.
///
/// The wire format is a union: a plain string, an arbitrary JSON object, or a
/// list of content blocks (text/image). This type models all three shapes and
/// preserves anything else (numbers, booleans, mixed arrays) in the
/// [`FunctionResultPayload::Json`] catch-all so no data is lost.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum FunctionResultPayload {
    /// A plain string result.
    Text(String),
    /// A structured JSON result (object or any other non-string,
    /// non-content-list JSON value). This doubles as the Evergreen catch-all
    /// for shapes this library does not recognize.
    Json(serde_json::Value),
    /// A list of content blocks (e.g. text and images).
    Contents(Vec<Content>),
}

impl FunctionResultPayload {
    /// Returns the string result, if this is a `Text` payload.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            _ => None,
        }
    }

    /// Returns the JSON value, if this is a `Json` payload.
    #[must_use]
    pub fn as_json(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Json(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the content blocks, if this is a `Contents` payload.
    #[must_use]
    pub fn as_contents(&self) -> Option<&[Content]> {
        match self {
            Self::Contents(c) => Some(c),
            _ => None,
        }
    }

    /// Converts the payload to a `serde_json::Value` (lossless).
    #[must_use]
    pub fn to_value(&self) -> serde_json::Value {
        match self {
            Self::Text(t) => serde_json::Value::String(t.clone()),
            Self::Json(v) => v.clone(),
            Self::Contents(c) => serde_json::to_value(c).unwrap_or(serde_json::Value::Null),
        }
    }
}

impl From<serde_json::Value> for FunctionResultPayload {
    /// Converts a function's return value into a payload the API accepts.
    ///
    /// Strings become [`FunctionResultPayload::Text`] and objects
    /// [`FunctionResultPayload::Json`]. Any other value (array, number, bool,
    /// null) is wrapped as `{"result": value}`, because the API rejects a
    /// top-level array. For content blocks, convert from `Vec<Content>`.
    ///
    /// This is the send-side conversion; deserializing a response uses
    /// [`FunctionResultPayload::from_value`], which keeps the wire shape.
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::String(s) => Self::Text(s),
            object @ serde_json::Value::Object(_) => Self::Json(object),
            other => Self::Json(serde_json::json!({ "result": other })),
        }
    }
}

impl From<&str> for FunctionResultPayload {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for FunctionResultPayload {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Vec<Content>> for FunctionResultPayload {
    fn from(value: Vec<Content>) -> Self {
        Self::Contents(value)
    }
}

impl Serialize for FunctionResultPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Text(t) => serializer.serialize_str(t),
            Self::Json(v) => v.serialize(serializer),
            Self::Contents(c) => c.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionResultPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::from_value(value))
    }
}

impl FunctionResultPayload {
    /// Classifies a raw JSON value into the appropriate payload variant.
    ///
    /// Arrays where every element is a JSON object with a `type` field are
    /// treated as content-block lists; all other arrays stay as raw JSON.
    #[must_use]
    pub fn from_value(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::String(s) => Self::Text(s),
            serde_json::Value::Array(items) => {
                let looks_like_contents = !items.is_empty()
                    && items
                        .iter()
                        .all(|v| v.is_object() && v.get("type").is_some_and(|t| t.is_string()));
                if looks_like_contents {
                    match serde_json::from_value::<Vec<Content>>(serde_json::Value::Array(
                        items.clone(),
                    )) {
                        Ok(contents) => Self::Contents(contents),
                        Err(_) => Self::Json(serde_json::Value::Array(items)),
                    }
                } else {
                    Self::Json(serde_json::Value::Array(items))
                }
            }
            other => Self::Json(other),
        }
    }
}

// =============================================================================
// Step
// =============================================================================

/// A single step in an interaction (API revision 2026-05-20).
///
/// The `steps` array on [`InteractionResponse`](crate::InteractionResponse)
/// replaces the launch-era `outputs` array. Steps also form the canonical
/// representation of conversation history when sending stateless multi-turn
/// requests (`input: [Step, ...]`).
///
/// # Forward Compatibility
///
/// This enum is `#[non_exhaustive]`. Unrecognized step types deserialize into
/// [`Step::Unknown`] with the full JSON preserved for roundtrip.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{InteractionResponse, Step};
/// # let response: InteractionResponse = todo!();
/// for step in &response.steps {
///     match step {
///         Step::ModelOutput { content, .. } => println!("{} content blocks", content.len()),
///         Step::FunctionCall { name, .. } => println!("Call function: {}", name),
///         Step::Unknown { step_type, .. } => println!("Unknown step: {}", step_type),
///         _ => {}
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Step {
    /// Input provided by the user (`type: "user_input"`).
    UserInput {
        /// Content blocks that make up the user input.
        content: Vec<Content>,
    },
    /// Output generated by the model (`type: "model_output"`).
    ModelOutput {
        /// Content blocks generated by the model.
        content: Vec<Content>,
        /// Error details, if the model output failed.
        error: Option<StepError>,
    },
    /// Internal reasoning (`type: "thought"`).
    Thought {
        /// Opaque signature validating the reasoning process. Pass it back
        /// unchanged when replaying history statelessly.
        signature: Option<String>,
        /// Optional human-readable summary of the reasoning (text/image
        /// content blocks).
        summary: Vec<Content>,
    },
    /// A client-side function call requested by the model
    /// (`type: "function_call"`).
    FunctionCall {
        /// Unique ID for this specific tool call.
        id: String,
        /// Name of the function to call.
        name: String,
        /// Arguments to pass to the function (JSON object).
        arguments: serde_json::Value,
        /// Opaque signature for backend validation. Returned by the API and
        /// required when replaying this step in stateless history (verified
        /// live 2026-07; the generated SDK bindings omit it).
        signature: Option<String>,
    },
    /// The result of a client-side function call
    /// (`type: "function_result"`).
    FunctionResult {
        /// The `id` of the [`Step::FunctionCall`] this responds to.
        call_id: String,
        /// Function name (optional per spec).
        name: Option<String>,
        /// The result: string, JSON, or content blocks.
        result: FunctionResultPayload,
        /// Whether the function execution errored.
        is_error: Option<bool>,
        /// Opaque signature hash for backend validation; pass through
        /// unchanged when replaying history.
        signature: Option<String>,
    },
    /// Server-side code execution call (`type: "code_execution_call"`).
    ///
    /// Wire format nests `language`/`code` inside an `arguments` object.
    CodeExecutionCall {
        /// Unique ID for this call.
        id: String,
        /// Programming language (currently only Python).
        language: CodeExecutionLanguage,
        /// Source code to execute.
        code: String,
        /// Opaque signature; pass through unchanged when replaying history.
        signature: Option<String>,
    },
    /// Server-side code execution result (`type: "code_execution_result"`).
    CodeExecutionResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Output (stdout on success, error message on failure).
        result: String,
        /// Whether execution errored.
        is_error: bool,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// URL context fetch call (`type: "url_context_call"`).
    UrlContextCall {
        /// Unique ID for this call.
        id: String,
        /// URLs to fetch (wire: nested under `arguments.urls`).
        urls: Vec<String>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// URL context fetch result (`type: "url_context_result"`).
    UrlContextResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Per-URL retrieval results.
        result: Vec<UrlContextResultItem>,
        /// Whether the fetch errored.
        is_error: Option<bool>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Google Search call (`type: "google_search_call"`).
    GoogleSearchCall {
        /// Unique ID for this call.
        id: String,
        /// Search queries (wire: nested under `arguments.queries`).
        queries: Vec<String>,
        /// Which search backend was used (e.g. `web_search`).
        search_type: Option<crate::tools::SearchType>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Google Search result (`type: "google_search_result"`).
    GoogleSearchResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Search results with source information.
        result: Vec<GoogleSearchResultItem>,
        /// Whether the search errored.
        is_error: Option<bool>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// A server-side tool call the API does not further identify
    /// (`type: "tool_call"`).
    ///
    /// This is what an MCP tool invocation actually looks like on the wire:
    /// only `{id, signature}`, with no server name, tool name, or arguments
    /// (verified live 2026-08-16, `gemini-3.7-flash`, `mcp.deepwiki.com`).
    /// Before it was modeled, these landed in [`Step::Unknown`], so a
    /// successful MCP interaction reported zero tool calls.
    ///
    /// The call *happened* and its cost is real — `usage.total_tool_use_tokens`
    /// is non-zero — but which server or tool ran is not recoverable from the
    /// response. See #433.
    ///
    /// # Roundtrip cost of modeling this
    ///
    /// [`Step::Unknown`] preserved the whole JSON object; this variant
    /// captures exactly `id` and `signature`, and the deserialize shadow enum
    /// does not deny unknown fields. So if the endpoint starts attaching
    /// sibling keys — `name`, `server_name`, `arguments`, the very fields the
    /// spec puts on `mcp_server_tool_call` — they are dropped on a
    /// deserialize/re-serialize roundtrip, where before they survived.
    ///
    /// That is the crate-wide convention (no `Step` variant flattens an
    /// extras map) and the probe showed exactly three keys, but it is more
    /// load-bearing here than elsewhere: this variant's premise is that its
    /// full shape is unproven, and this is the mechanism by which the missing
    /// identity would *stay* missing even once the API began sending it.
    /// The recurring SDK-bindings sweep (#421) is the intended detector.
    ToolCall {
        /// Unique ID for this call.
        id: String,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// MCP server tool call (`type: "mcp_server_tool_call"`).
    ///
    /// **Spec-present, never observed.** MCP activity arrives as the generic
    /// [`Step::ToolCall`] above instead (verified live 2026-08-16), so a
    /// match on this variant never fires today. Modeled for parity, like
    /// `Tool::Retrieval` — kept rather than removed because nothing
    /// *rejects* it, so the API may begin emitting it. Tracked in #459.
    McpServerToolCall {
        /// Unique ID for this call.
        id: String,
        /// Tool name on the MCP server.
        name: String,
        /// Name of the MCP server.
        server_name: String,
        /// Arguments passed to the tool.
        arguments: serde_json::Value,
    },
    /// MCP server tool result (`type: "mcp_server_tool_result"`).
    ///
    /// **Spec-present, never observed** — see [`Step::McpServerToolCall`].
    McpServerToolResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Tool name (optional per spec).
        name: Option<String>,
        /// Server name (optional per spec).
        server_name: Option<String>,
        /// The result: string, JSON, or content blocks.
        result: FunctionResultPayload,
    },
    /// File Search call (`type: "file_search_call"`).
    FileSearchCall {
        /// Unique ID for this call.
        id: String,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// File Search result (`type: "file_search_result"`).
    FileSearchResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Retrieved chunks. Note: the 2026-05-20 spec does not document a
        /// `result` field on this step; it is kept here (deserialized when
        /// present) because earlier revisions returned it.
        result: Vec<FileSearchResultItem>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Google Maps call (`type: "google_maps_call"`).
    GoogleMapsCall {
        /// Unique ID for this call.
        id: String,
        /// Location queries (wire: nested under `arguments.queries`).
        queries: Vec<String>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Google Maps result (`type: "google_maps_result"`).
    GoogleMapsResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Places and widget data.
        result: Vec<GoogleMapsResultItem>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Server-initiated media processing call (`type: "processing_call"`).
    ///
    /// Emitted before the model answers when video uses
    /// [`VideoProcessing::Agentic`](crate::VideoProcessing::Agentic)
    /// (verified live 2026-09-24, `gemini-3.8-flash`). The signature is large
    /// (~36KB) and **required** for stateless replay: omitting it yields
    /// `400 Processing call step is missing signature`.
    ProcessingCall {
        /// Unique ID for this call.
        id: String,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Result of a [`Step::ProcessingCall`] (`type: "processing_result"`).
    ProcessingResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Retrieval tool call (`type: "retrieval_call"`).
    ///
    /// Produced by [`Tool::Retrieval`](crate::Tool::Retrieval), which the
    /// Gemini API rejects as Vertex-only; modeled for spec parity.
    RetrievalCall {
        /// Unique ID for this call.
        id: String,
        /// Retrieval queries (wire: nested under `arguments.queries`).
        queries: Vec<String>,
        /// Which retrieval backend handled the call.
        retrieval_type: Option<crate::tools::RetrievalType>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Retrieval tool result (`type: "retrieval_result"`); Vertex-only, see
    /// [`Step::RetrievalCall`].
    RetrievalResult {
        /// The `id` of the corresponding call.
        call_id: String,
        /// Whether the retrieval errored.
        is_error: Option<bool>,
        /// Opaque signature; pass through unchanged.
        signature: Option<String>,
    },
    /// Unknown step type for forward compatibility.
    ///
    /// Captures step types this library doesn't recognize yet. Roundtrips
    /// losslessly: serializing an `Unknown` step reproduces the original
    /// fields with `step_type` as the `"type"` tag.
    Unknown {
        /// The unrecognized type name from the API.
        step_type: String,
        /// The full JSON data for this step, preserved for debugging.
        data: serde_json::Value,
    },
}

impl Step {
    // =========================================================================
    // Unknown helpers (Evergreen pattern)
    // =========================================================================

    /// Check if this is an unknown step type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the step type name if this is an unknown step.
    #[must_use]
    pub fn unknown_step_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { step_type, .. } => Some(step_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown step.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    // =========================================================================
    // Constructors
    // =========================================================================

    /// Creates a `user_input` step from a plain text message.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::UserInput {
            content: vec![Content::text(text)],
        }
    }

    /// Creates a `user_input` step from content blocks.
    #[must_use]
    pub fn user_input(content: Vec<Content>) -> Self {
        Self::UserInput { content }
    }

    /// Creates a `model_output` step from a plain text message.
    ///
    /// Useful when constructing conversation history manually.
    pub fn model_text(text: impl Into<String>) -> Self {
        Self::ModelOutput {
            content: vec![Content::text(text)],
            error: None,
        }
    }

    /// Creates a `model_output` step from content blocks.
    #[must_use]
    pub fn model_output(content: Vec<Content>) -> Self {
        Self::ModelOutput {
            content,
            error: None,
        }
    }

    /// Creates a `thought` step carrying only a signature.
    pub fn thought(signature: impl Into<String>) -> Self {
        Self::Thought {
            signature: Some(signature.into()),
            summary: Vec::new(),
        }
    }

    /// Creates a `function_call` step.
    pub fn function_call(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
    ) -> Self {
        Self::FunctionCall {
            id: id.into(),
            name: name.into(),
            arguments,
            signature: None,
        }
    }

    /// Creates a `function_result` step for a successful execution.
    ///
    /// The result can be a string, a JSON value, or content blocks — anything
    /// convertible into [`FunctionResultPayload`].
    pub fn function_result(
        name: impl Into<String>,
        call_id: impl Into<String>,
        result: impl Into<FunctionResultPayload>,
    ) -> Self {
        Self::FunctionResult {
            call_id: call_id.into(),
            name: Some(name.into()),
            result: result.into(),
            is_error: None,
            signature: None,
        }
    }

    /// Creates a `function_result` step marked as an error.
    pub fn function_result_error(
        name: impl Into<String>,
        call_id: impl Into<String>,
        result: impl Into<FunctionResultPayload>,
    ) -> Self {
        Self::FunctionResult {
            call_id: call_id.into(),
            name: Some(name.into()),
            result: result.into(),
            is_error: Some(true),
            signature: None,
        }
    }

    // =========================================================================
    // Accessors
    // =========================================================================

    /// Returns the content blocks if this step carries content
    /// (`user_input` / `model_output`).
    #[must_use]
    pub fn content(&self) -> Option<&[Content]> {
        match self {
            Self::UserInput { content } | Self::ModelOutput { content, .. } => Some(content),
            _ => None,
        }
    }

    /// Returns the first text block if this step carries text content.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        self.content()?.iter().find_map(Content::as_text)
    }

    /// Returns the opaque signature carried by this step, if any.
    ///
    /// Thought steps, function call/result steps, and built-in tool
    /// call/result steps carry signatures that must be passed back unchanged
    /// when replaying history statelessly.
    #[must_use]
    pub fn signature(&self) -> Option<&str> {
        match self {
            Self::Thought { signature, .. }
            | Self::FunctionCall { signature, .. }
            | Self::FunctionResult { signature, .. }
            | Self::CodeExecutionCall { signature, .. }
            | Self::CodeExecutionResult { signature, .. }
            | Self::UrlContextCall { signature, .. }
            | Self::UrlContextResult { signature, .. }
            | Self::GoogleSearchCall { signature, .. }
            | Self::GoogleSearchResult { signature, .. }
            | Self::FileSearchCall { signature, .. }
            | Self::FileSearchResult { signature, .. }
            | Self::GoogleMapsCall { signature, .. }
            | Self::GoogleMapsResult { signature, .. }
            | Self::ToolCall { signature, .. }
            | Self::ProcessingCall { signature, .. }
            | Self::ProcessingResult { signature, .. }
            | Self::RetrievalCall { signature, .. }
            | Self::RetrievalResult { signature, .. } => signature.as_deref(),
            _ => None,
        }
    }

    /// Returns the wire `type` tag for this step.
    #[must_use]
    pub fn step_type(&self) -> &str {
        match self {
            Self::UserInput { .. } => "user_input",
            Self::ModelOutput { .. } => "model_output",
            Self::Thought { .. } => "thought",
            Self::FunctionCall { .. } => "function_call",
            Self::FunctionResult { .. } => "function_result",
            Self::CodeExecutionCall { .. } => "code_execution_call",
            Self::CodeExecutionResult { .. } => "code_execution_result",
            Self::UrlContextCall { .. } => "url_context_call",
            Self::UrlContextResult { .. } => "url_context_result",
            Self::GoogleSearchCall { .. } => "google_search_call",
            Self::GoogleSearchResult { .. } => "google_search_result",
            Self::ToolCall { .. } => "tool_call",
            Self::McpServerToolCall { .. } => "mcp_server_tool_call",
            Self::McpServerToolResult { .. } => "mcp_server_tool_result",
            Self::FileSearchCall { .. } => "file_search_call",
            Self::FileSearchResult { .. } => "file_search_result",
            Self::GoogleMapsCall { .. } => "google_maps_call",
            Self::GoogleMapsResult { .. } => "google_maps_result",
            Self::ProcessingCall { .. } => "processing_call",
            Self::ProcessingResult { .. } => "processing_result",
            Self::RetrievalCall { .. } => "retrieval_call",
            Self::RetrievalResult { .. } => "retrieval_result",
            Self::Unknown { step_type, .. } => step_type,
        }
    }
}

#[cfg(test)]
#[path = "steps_tests.rs"]
mod tests;
