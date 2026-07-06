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
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, Resolution, UrlContextResultItem,
};

// =============================================================================
// StepError (wire: google.rpc.Status-like {code, message, details})
// =============================================================================

/// Error details attached to a `model_output` step.
///
/// Mirrors the API's status shape: `{code, message, details}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
    /// Converts an arbitrary JSON value into the appropriate payload variant.
    ///
    /// Strings become [`FunctionResultPayload::Text`]; everything else becomes
    /// [`FunctionResultPayload::Json`].
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::String(s) => Self::Text(s),
            other => Self::Json(other),
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
    /// MCP server tool call (`type: "mcp_server_tool_call"`).
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
    /// Thought steps and built-in tool call/result steps carry signatures
    /// that must be passed back unchanged when replaying history statelessly.
    #[must_use]
    pub fn signature(&self) -> Option<&str> {
        match self {
            Self::Thought { signature, .. }
            | Self::CodeExecutionCall { signature, .. }
            | Self::CodeExecutionResult { signature, .. }
            | Self::UrlContextCall { signature, .. }
            | Self::UrlContextResult { signature, .. }
            | Self::GoogleSearchCall { signature, .. }
            | Self::GoogleSearchResult { signature, .. }
            | Self::FileSearchCall { signature, .. }
            | Self::FileSearchResult { signature, .. }
            | Self::GoogleMapsCall { signature, .. }
            | Self::GoogleMapsResult { signature, .. } => signature.as_deref(),
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
            Self::McpServerToolCall { .. } => "mcp_server_tool_call",
            Self::McpServerToolResult { .. } => "mcp_server_tool_result",
            Self::FileSearchCall { .. } => "file_search_call",
            Self::FileSearchResult { .. } => "file_search_result",
            Self::GoogleMapsCall { .. } => "google_maps_call",
            Self::GoogleMapsResult { .. } => "google_maps_result",
            Self::Unknown { step_type, .. } => step_type,
        }
    }
}

impl Serialize for Step {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::UserInput { content } => {
                map.serialize_entry("type", "user_input")?;
                map.serialize_entry("content", content)?;
            }
            Self::ModelOutput { content, error } => {
                map.serialize_entry("type", "model_output")?;
                map.serialize_entry("content", content)?;
                if let Some(e) = error {
                    map.serialize_entry("error", e)?;
                }
            }
            Self::Thought { signature, summary } => {
                map.serialize_entry("type", "thought")?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
                if !summary.is_empty() {
                    map.serialize_entry("summary", summary)?;
                }
            }
            Self::FunctionCall {
                id,
                name,
                arguments,
            } => {
                map.serialize_entry("type", "function_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::FunctionResult {
                call_id,
                name,
                result,
                is_error,
            } => {
                map.serialize_entry("type", "function_result")?;
                map.serialize_entry("call_id", call_id)?;
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
                }
                map.serialize_entry("result", result)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
            }
            Self::CodeExecutionCall {
                id,
                language,
                code,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry(
                    "arguments",
                    &serde_json::json!({ "language": language, "code": code }),
                )?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::CodeExecutionResult {
                call_id,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                map.serialize_entry("is_error", is_error)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::UrlContextCall {
                id,
                urls,
                signature,
            } => {
                map.serialize_entry("type", "url_context_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "urls": urls }))?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::UrlContextResult {
                call_id,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "url_context_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleSearchCall {
                id,
                queries,
                search_type,
                signature,
            } => {
                map.serialize_entry("type", "google_search_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                if let Some(st) = search_type {
                    map.serialize_entry("search_type", st)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleSearchResult {
                call_id,
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "google_search_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(e) = is_error {
                    map.serialize_entry("is_error", e)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::McpServerToolCall {
                id,
                name,
                server_name,
                arguments,
            } => {
                map.serialize_entry("type", "mcp_server_tool_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("server_name", server_name)?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::McpServerToolResult {
                call_id,
                name,
                server_name,
                result,
            } => {
                map.serialize_entry("type", "mcp_server_tool_result")?;
                map.serialize_entry("call_id", call_id)?;
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
                }
                if let Some(sn) = server_name {
                    map.serialize_entry("server_name", sn)?;
                }
                map.serialize_entry("result", result)?;
            }
            Self::FileSearchCall { id, signature } => {
                map.serialize_entry("type", "file_search_call")?;
                map.serialize_entry("id", id)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::FileSearchResult {
                call_id,
                result,
                signature,
            } => {
                map.serialize_entry("type", "file_search_result")?;
                map.serialize_entry("call_id", call_id)?;
                if !result.is_empty() {
                    map.serialize_entry("result", result)?;
                }
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleMapsCall {
                id,
                queries,
                signature,
            } => {
                map.serialize_entry("type", "google_maps_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::GoogleMapsResult {
                call_id,
                result,
                signature,
            } => {
                map.serialize_entry("type", "google_maps_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
            }
            Self::Unknown { step_type, data } => {
                map.serialize_entry("type", step_type)?;
                match data {
                    serde_json::Value::Object(obj) => {
                        for (key, value) in obj {
                            if key != "type" {
                                map.serialize_entry(key, value)?;
                            }
                        }
                    }
                    other if !other.is_null() => {
                        map.serialize_entry("data", other)?;
                    }
                    _ => {}
                }
            }
        }
        map.end()
    }
}

/// Extracts a `Vec<String>` from `arguments.<key>` (used by call steps whose
/// arguments nest a string array).
fn string_vec_from_arguments(arguments: Option<&serde_json::Value>, key: &str) -> Vec<String> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[cfg(feature = "strict-unknown")]
        use serde::de::Error as _;

        let value = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum KnownStep {
            UserInput {
                #[serde(default)]
                content: Vec<Content>,
            },
            ModelOutput {
                #[serde(default)]
                content: Vec<Content>,
                #[serde(default)]
                error: Option<StepError>,
            },
            Thought {
                #[serde(default)]
                signature: Option<String>,
                #[serde(default)]
                summary: Vec<Content>,
            },
            FunctionCall {
                id: String,
                name: String,
                #[serde(default)]
                arguments: serde_json::Value,
            },
            FunctionResult {
                call_id: String,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
                #[serde(default)]
                is_error: Option<bool>,
            },
            CodeExecutionCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            CodeExecutionResult {
                call_id: String,
                #[serde(default)]
                result: String,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextResult {
                call_id: String,
                #[serde(default)]
                result: Vec<UrlContextResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                search_type: Option<crate::tools::SearchType>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchResult {
                call_id: String,
                #[serde(default)]
                result: Vec<GoogleSearchResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            McpServerToolCall {
                id: String,
                name: String,
                server_name: String,
                #[serde(default)]
                arguments: serde_json::Value,
            },
            McpServerToolResult {
                call_id: String,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                server_name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
            },
            FileSearchCall {
                id: String,
                #[serde(default)]
                signature: Option<String>,
            },
            FileSearchResult {
                call_id: String,
                #[serde(default)]
                result: Vec<FileSearchResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsResult {
                call_id: String,
                #[serde(default)]
                result: Vec<GoogleMapsResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
        }

        match serde_json::from_value::<KnownStep>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownStep::UserInput { content } => Step::UserInput { content },
                KnownStep::ModelOutput { content, error } => Step::ModelOutput { content, error },
                KnownStep::Thought { signature, summary } => Step::Thought { signature, summary },
                KnownStep::FunctionCall {
                    id,
                    name,
                    arguments,
                } => Step::FunctionCall {
                    id,
                    name,
                    arguments,
                },
                KnownStep::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                } => Step::FunctionResult {
                    call_id,
                    name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                    is_error,
                },
                KnownStep::CodeExecutionCall {
                    id,
                    arguments,
                    signature,
                } => {
                    let language = arguments
                        .as_ref()
                        .and_then(|a| a.get("language"))
                        .and_then(|l| {
                            serde_json::from_value::<CodeExecutionLanguage>(l.clone()).ok()
                        })
                        .unwrap_or_default();
                    let code = arguments
                        .as_ref()
                        .and_then(|a| a.get("code"))
                        .and_then(|c| c.as_str())
                        .unwrap_or_default()
                        .to_string();
                    Step::CodeExecutionCall {
                        id,
                        language,
                        code,
                        signature,
                    }
                }
                KnownStep::CodeExecutionResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                } => Step::CodeExecutionResult {
                    call_id,
                    result,
                    is_error: is_error.unwrap_or(false),
                    signature,
                },
                KnownStep::UrlContextCall {
                    id,
                    arguments,
                    signature,
                } => Step::UrlContextCall {
                    id,
                    urls: string_vec_from_arguments(arguments.as_ref(), "urls"),
                    signature,
                },
                KnownStep::UrlContextResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                } => Step::UrlContextResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                },
                KnownStep::GoogleSearchCall {
                    id,
                    arguments,
                    search_type,
                    signature,
                } => Step::GoogleSearchCall {
                    id,
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    search_type,
                    signature,
                },
                KnownStep::GoogleSearchResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                } => Step::GoogleSearchResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                },
                KnownStep::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                } => Step::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                },
                KnownStep::McpServerToolResult {
                    call_id,
                    name,
                    server_name,
                    result,
                } => Step::McpServerToolResult {
                    call_id,
                    name,
                    server_name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                },
                KnownStep::FileSearchCall { id, signature } => {
                    Step::FileSearchCall { id, signature }
                }
                KnownStep::FileSearchResult {
                    call_id,
                    result,
                    signature,
                } => Step::FileSearchResult {
                    call_id,
                    result,
                    signature,
                },
                KnownStep::GoogleMapsCall {
                    id,
                    arguments,
                    signature,
                } => Step::GoogleMapsCall {
                    id,
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    signature,
                },
                KnownStep::GoogleMapsResult {
                    call_id,
                    result,
                    signature,
                } => Step::GoogleMapsResult {
                    call_id,
                    result,
                    signature,
                },
            }),
            Err(parse_error) => {
                let step_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Encountered unknown Step type '{}'. Parse error: {}. \
                     This may indicate a new API feature or a malformed response. \
                     The step will be preserved in the Unknown variant.",
                    step_type,
                    parse_error
                );

                #[cfg(feature = "strict-unknown")]
                {
                    Err(D::Error::custom(format!(
                        "Unknown Step type '{}'. \
                         Strict mode is enabled via the 'strict-unknown' feature flag. \
                         Either update the library or disable strict mode.",
                        step_type
                    )))
                }

                #[cfg(not(feature = "strict-unknown"))]
                {
                    Ok(Step::Unknown {
                        step_type,
                        data: value,
                    })
                }
            }
        }
    }
}

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
        /// The `id` of the call this result responds to.
        call_id: String,
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
    McpServerToolCall {
        /// Tool name.
        name: String,
        /// MCP server name.
        server_name: String,
        /// Tool arguments.
        arguments: serde_json::Value,
    },
    /// MCP server tool result delta (`type: "mcp_server_tool_result"`).
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

impl Serialize for StepDelta {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        /// Serialize an optional entry only when present.
        macro_rules! opt_entry {
            ($map:expr, $key:literal, $val:expr) => {
                if let Some(v) = $val {
                    $map.serialize_entry($key, v)?;
                }
            };
        }

        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::Text { text } => {
                map.serialize_entry("type", "text")?;
                map.serialize_entry("text", text)?;
            }
            Self::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                map.serialize_entry("type", "image")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
                opt_entry!(map, "resolution", resolution);
            }
            Self::Audio {
                data,
                uri,
                mime_type,
                rate,
                sample_rate,
                channels,
            } => {
                map.serialize_entry("type", "audio")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
                opt_entry!(map, "rate", rate);
                opt_entry!(map, "sample_rate", sample_rate);
                opt_entry!(map, "channels", channels);
            }
            Self::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                map.serialize_entry("type", "video")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
                opt_entry!(map, "resolution", resolution);
            }
            Self::Document {
                data,
                uri,
                mime_type,
            } => {
                map.serialize_entry("type", "document")?;
                opt_entry!(map, "data", data);
                opt_entry!(map, "uri", uri);
                opt_entry!(map, "mime_type", mime_type);
            }
            Self::ThoughtSummary { content } => {
                map.serialize_entry("type", "thought_summary")?;
                opt_entry!(map, "content", content);
            }
            Self::ThoughtSignature { signature } => {
                map.serialize_entry("type", "thought_signature")?;
                opt_entry!(map, "signature", signature);
            }
            Self::TextAnnotation { annotations } => {
                map.serialize_entry("type", "text_annotation_delta")?;
                map.serialize_entry("annotations", annotations)?;
            }
            Self::ArgumentsDelta { arguments } => {
                map.serialize_entry("type", "arguments_delta")?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::FunctionResult {
                call_id,
                name,
                result,
                is_error,
            } => {
                map.serialize_entry("type", "function_result")?;
                map.serialize_entry("call_id", call_id)?;
                opt_entry!(map, "name", name);
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
            }
            Self::CodeExecutionCall {
                language,
                code,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_call")?;
                map.serialize_entry(
                    "arguments",
                    &serde_json::json!({ "language": language, "code": code }),
                )?;
                opt_entry!(map, "signature", signature);
            }
            Self::CodeExecutionResult {
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "code_execution_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::UrlContextCall { urls, signature } => {
                map.serialize_entry("type", "url_context_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "urls": urls }))?;
                opt_entry!(map, "signature", signature);
            }
            Self::UrlContextResult {
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "url_context_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleSearchCall { queries, signature } => {
                map.serialize_entry("type", "google_search_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleSearchResult {
                result,
                is_error,
                signature,
            } => {
                map.serialize_entry("type", "google_search_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "is_error", is_error);
                opt_entry!(map, "signature", signature);
            }
            Self::McpServerToolCall {
                name,
                server_name,
                arguments,
            } => {
                map.serialize_entry("type", "mcp_server_tool_call")?;
                map.serialize_entry("name", name)?;
                map.serialize_entry("server_name", server_name)?;
                map.serialize_entry("arguments", arguments)?;
            }
            Self::McpServerToolResult {
                name,
                server_name,
                result,
            } => {
                map.serialize_entry("type", "mcp_server_tool_result")?;
                opt_entry!(map, "name", name);
                opt_entry!(map, "server_name", server_name);
                map.serialize_entry("result", result)?;
            }
            Self::FileSearchCall { signature } => {
                map.serialize_entry("type", "file_search_call")?;
                opt_entry!(map, "signature", signature);
            }
            Self::FileSearchResult { result, signature } => {
                map.serialize_entry("type", "file_search_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleMapsCall { queries, signature } => {
                map.serialize_entry("type", "google_maps_call")?;
                map.serialize_entry("arguments", &serde_json::json!({ "queries": queries }))?;
                opt_entry!(map, "signature", signature);
            }
            Self::GoogleMapsResult { result, signature } => {
                map.serialize_entry("type", "google_maps_result")?;
                map.serialize_entry("result", result)?;
                opt_entry!(map, "signature", signature);
            }
            Self::Unknown { delta_type, data } => {
                map.serialize_entry("type", delta_type)?;
                match data {
                    serde_json::Value::Object(obj) => {
                        for (key, value) in obj {
                            if key != "type" {
                                map.serialize_entry(key, value)?;
                            }
                        }
                    }
                    other if !other.is_null() => {
                        map.serialize_entry("data", other)?;
                    }
                    _ => {}
                }
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for StepDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum KnownDelta {
            Text {
                #[serde(default)]
                text: String,
            },
            Image {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                resolution: Option<Resolution>,
            },
            Audio {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                rate: Option<u32>,
                #[serde(default)]
                sample_rate: Option<u32>,
                #[serde(default)]
                channels: Option<u32>,
            },
            Video {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                resolution: Option<Resolution>,
            },
            Document {
                #[serde(default)]
                data: Option<String>,
                #[serde(default)]
                uri: Option<String>,
                #[serde(default)]
                mime_type: Option<String>,
            },
            ThoughtSummary {
                #[serde(default)]
                content: Option<Content>,
            },
            ThoughtSignature {
                #[serde(default)]
                signature: Option<String>,
            },
            #[serde(rename = "text_annotation_delta")]
            TextAnnotation {
                #[serde(default)]
                annotations: Vec<Annotation>,
            },
            ArgumentsDelta {
                #[serde(default)]
                arguments: String,
            },
            FunctionResult {
                call_id: String,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
                #[serde(default)]
                is_error: Option<bool>,
            },
            CodeExecutionCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            CodeExecutionResult {
                #[serde(default)]
                result: String,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            UrlContextResult {
                #[serde(default)]
                result: Vec<UrlContextResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleSearchResult {
                #[serde(default)]
                result: Vec<GoogleSearchResultItem>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                signature: Option<String>,
            },
            McpServerToolCall {
                name: String,
                server_name: String,
                #[serde(default)]
                arguments: serde_json::Value,
            },
            McpServerToolResult {
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                server_name: Option<String>,
                #[serde(default)]
                result: Option<FunctionResultPayload>,
            },
            FileSearchCall {
                #[serde(default)]
                signature: Option<String>,
            },
            FileSearchResult {
                #[serde(default)]
                result: Vec<FileSearchResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsCall {
                #[serde(default)]
                arguments: Option<serde_json::Value>,
                #[serde(default)]
                signature: Option<String>,
            },
            GoogleMapsResult {
                #[serde(default)]
                result: Vec<GoogleMapsResultItem>,
                #[serde(default)]
                signature: Option<String>,
            },
        }

        match serde_json::from_value::<KnownDelta>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownDelta::Text { text } => StepDelta::Text { text },
                KnownDelta::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                } => StepDelta::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                },
                KnownDelta::Audio {
                    data,
                    uri,
                    mime_type,
                    rate,
                    sample_rate,
                    channels,
                } => StepDelta::Audio {
                    data,
                    uri,
                    mime_type,
                    rate,
                    sample_rate,
                    channels,
                },
                KnownDelta::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                } => StepDelta::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                },
                KnownDelta::Document {
                    data,
                    uri,
                    mime_type,
                } => StepDelta::Document {
                    data,
                    uri,
                    mime_type,
                },
                KnownDelta::ThoughtSummary { content } => StepDelta::ThoughtSummary { content },
                KnownDelta::ThoughtSignature { signature } => {
                    StepDelta::ThoughtSignature { signature }
                }
                KnownDelta::TextAnnotation { annotations } => {
                    StepDelta::TextAnnotation { annotations }
                }
                KnownDelta::ArgumentsDelta { arguments } => StepDelta::ArgumentsDelta { arguments },
                KnownDelta::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                } => StepDelta::FunctionResult {
                    call_id,
                    name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                    is_error,
                },
                KnownDelta::CodeExecutionCall {
                    arguments,
                    signature,
                } => {
                    StepDelta::CodeExecutionCall {
                        language: arguments.as_ref().and_then(|a| a.get("language")).and_then(
                            |l| serde_json::from_value::<CodeExecutionLanguage>(l.clone()).ok(),
                        ),
                        code: arguments
                            .as_ref()
                            .and_then(|a| a.get("code"))
                            .and_then(|c| c.as_str())
                            .map(String::from),
                        signature,
                    }
                }
                KnownDelta::CodeExecutionResult {
                    result,
                    is_error,
                    signature,
                } => StepDelta::CodeExecutionResult {
                    result,
                    is_error,
                    signature,
                },
                KnownDelta::UrlContextCall {
                    arguments,
                    signature,
                } => StepDelta::UrlContextCall {
                    urls: string_vec_from_arguments(arguments.as_ref(), "urls"),
                    signature,
                },
                KnownDelta::UrlContextResult {
                    result,
                    is_error,
                    signature,
                } => StepDelta::UrlContextResult {
                    result,
                    is_error,
                    signature,
                },
                KnownDelta::GoogleSearchCall {
                    arguments,
                    signature,
                } => StepDelta::GoogleSearchCall {
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    signature,
                },
                KnownDelta::GoogleSearchResult {
                    result,
                    is_error,
                    signature,
                } => StepDelta::GoogleSearchResult {
                    result,
                    is_error,
                    signature,
                },
                KnownDelta::McpServerToolCall {
                    name,
                    server_name,
                    arguments,
                } => StepDelta::McpServerToolCall {
                    name,
                    server_name,
                    arguments,
                },
                KnownDelta::McpServerToolResult {
                    name,
                    server_name,
                    result,
                } => StepDelta::McpServerToolResult {
                    name,
                    server_name,
                    result: result.unwrap_or(FunctionResultPayload::Json(serde_json::Value::Null)),
                },
                KnownDelta::FileSearchCall { signature } => StepDelta::FileSearchCall { signature },
                KnownDelta::FileSearchResult { result, signature } => {
                    StepDelta::FileSearchResult { result, signature }
                }
                KnownDelta::GoogleMapsCall {
                    arguments,
                    signature,
                } => StepDelta::GoogleMapsCall {
                    queries: string_vec_from_arguments(arguments.as_ref(), "queries"),
                    signature,
                },
                KnownDelta::GoogleMapsResult { result, signature } => {
                    StepDelta::GoogleMapsResult { result, signature }
                }
            }),
            Err(parse_error) => {
                let delta_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Encountered unknown StepDelta type '{}'. Parse error: {}. \
                     The delta will be preserved in the Unknown variant.",
                    delta_type,
                    parse_error
                );

                Ok(StepDelta::Unknown {
                    delta_type,
                    data: value,
                })
            }
        }
    }
}

// =============================================================================
// Streaming step accumulation
// =============================================================================

/// Accumulates `step.start` / `step.delta` / `step.stop` events into complete
/// [`Step`]s, so streaming consumers get a fully-populated `steps` array on
/// the final response even when the server's `interaction.completed` payload
/// omits it.
#[derive(Debug, Default)]
pub(crate) struct StepAccumulator {
    steps: std::collections::BTreeMap<usize, AccumulatedStep>,
}

#[derive(Debug)]
struct AccumulatedStep {
    step: Step,
    /// Raw buffer for `arguments_delta` fragments (function calls).
    args_buffer: String,
}

impl StepAccumulator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Records a `step.start` event.
    pub(crate) fn start(&mut self, index: usize, step: Step) {
        self.steps.insert(
            index,
            AccumulatedStep {
                step,
                args_buffer: String::new(),
            },
        );
    }

    /// Applies a `step.delta` event to the step at `index`.
    pub(crate) fn apply_delta(&mut self, index: usize, delta: &StepDelta) {
        let entry = self.steps.entry(index).or_insert_with(|| AccumulatedStep {
            // Deltas without a preceding step.start most commonly belong to a
            // model_output step; start one so the content is not dropped.
            step: Step::ModelOutput {
                content: Vec::new(),
                error: None,
            },
            args_buffer: String::new(),
        });

        match delta {
            StepDelta::Text { text } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    if let Some(Content::Text { text: Some(t), .. }) = content.last_mut() {
                        t.push_str(text);
                    } else {
                        content.push(Content::text(text.clone()));
                    }
                }
            }
            StepDelta::TextAnnotation { annotations } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                    && let Some(Content::Text {
                        annotations: annots,
                        ..
                    }) = content.last_mut()
                {
                    annots
                        .get_or_insert_with(Vec::new)
                        .extend(annotations.iter().cloned());
                }
            }
            StepDelta::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    content.push(Content::Image {
                        data: data.clone(),
                        uri: uri.clone(),
                        mime_type: mime_type.clone(),
                        resolution: resolution.clone(),
                    });
                }
            }
            StepDelta::Audio {
                data,
                uri,
                mime_type,
                rate,
                sample_rate,
                channels,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    // Audio may stream in multiple chunks; append base64 data to
                    // the previous audio block when present.
                    if let (
                        Some(Content::Audio {
                            data: Some(existing),
                            ..
                        }),
                        Some(new_data),
                    ) = (content.last_mut(), data.as_ref())
                    {
                        existing.push_str(new_data);
                    } else {
                        content.push(Content::Audio {
                            data: data.clone(),
                            uri: uri.clone(),
                            mime_type: mime_type.clone(),
                            sample_rate: sample_rate.or(*rate),
                            channels: *channels,
                        });
                    }
                }
            }
            StepDelta::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    content.push(Content::Video {
                        data: data.clone(),
                        uri: uri.clone(),
                        mime_type: mime_type.clone(),
                        resolution: resolution.clone(),
                    });
                }
            }
            StepDelta::Document {
                data,
                uri,
                mime_type,
            } => {
                if let Step::UserInput { content } | Step::ModelOutput { content, .. } =
                    &mut entry.step
                {
                    content.push(Content::Document {
                        data: data.clone(),
                        uri: uri.clone(),
                        mime_type: mime_type.clone(),
                    });
                }
            }
            StepDelta::ThoughtSummary { content } => {
                if let Step::Thought { summary, .. } = &mut entry.step
                    && let Some(c) = content
                {
                    // Consecutive text summaries merge; other content appends.
                    if let (
                        Some(Content::Text { text: Some(t), .. }),
                        Content::Text {
                            text: Some(new), ..
                        },
                    ) = (summary.last_mut(), c)
                    {
                        t.push_str(new);
                    } else {
                        summary.push(c.clone());
                    }
                }
            }
            StepDelta::ThoughtSignature { signature } => {
                if let Step::Thought { signature: sig, .. } = &mut entry.step
                    && let Some(fragment) = signature
                {
                    sig.get_or_insert_with(String::new).push_str(fragment);
                }
            }
            StepDelta::ArgumentsDelta { arguments } => {
                entry.args_buffer.push_str(arguments);
            }
            StepDelta::FunctionResult {
                call_id,
                name,
                result,
                is_error,
            } => {
                entry.step = Step::FunctionResult {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    result: result.clone(),
                    is_error: *is_error,
                };
            }
            StepDelta::CodeExecutionCall {
                language,
                code,
                signature,
            } => {
                if let Step::CodeExecutionCall {
                    language: lang,
                    code: existing_code,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    if let Some(l) = language {
                        *lang = l.clone();
                    }
                    if let Some(c) = code {
                        existing_code.push_str(c);
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::CodeExecutionResult {
                result,
                is_error,
                signature,
            } => {
                if let Step::CodeExecutionResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.push_str(result);
                    if let Some(e) = is_error {
                        *err = *e;
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::UrlContextCall { urls, signature } => {
                if let Step::UrlContextCall {
                    urls: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(urls.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::UrlContextResult {
                result,
                is_error,
                signature,
            } => {
                if let Step::UrlContextResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if is_error.is_some() {
                        *err = *is_error;
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleSearchCall { queries, signature } => {
                if let Step::GoogleSearchCall {
                    queries: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(queries.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleSearchResult {
                result,
                is_error,
                signature,
            } => {
                if let Step::GoogleSearchResult {
                    result: existing,
                    is_error: err,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if is_error.is_some() {
                        *err = *is_error;
                    }
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::McpServerToolCall {
                name,
                server_name,
                arguments,
            } => {
                if let Step::McpServerToolCall {
                    name: n,
                    server_name: sn,
                    arguments: args,
                    ..
                } = &mut entry.step
                {
                    *n = name.clone();
                    *sn = server_name.clone();
                    *args = arguments.clone();
                }
            }
            StepDelta::McpServerToolResult {
                name,
                server_name,
                result,
            } => {
                if let Step::McpServerToolResult {
                    name: n,
                    server_name: sn,
                    result: r,
                    ..
                } = &mut entry.step
                {
                    if name.is_some() {
                        *n = name.clone();
                    }
                    if server_name.is_some() {
                        *sn = server_name.clone();
                    }
                    *r = result.clone();
                }
            }
            StepDelta::FileSearchCall { signature } => {
                if let Step::FileSearchCall { signature: sig, .. } = &mut entry.step
                    && signature.is_some()
                {
                    *sig = signature.clone();
                }
            }
            StepDelta::FileSearchResult { result, signature } => {
                if let Step::FileSearchResult {
                    result: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleMapsCall { queries, signature } => {
                if let Step::GoogleMapsCall {
                    queries: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(queries.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::GoogleMapsResult { result, signature } => {
                if let Step::GoogleMapsResult {
                    result: existing,
                    signature: sig,
                    ..
                } = &mut entry.step
                {
                    existing.extend(result.iter().cloned());
                    if signature.is_some() {
                        *sig = signature.clone();
                    }
                }
            }
            StepDelta::Unknown { delta_type, .. } => {
                tracing::debug!(
                    "Skipping unknown StepDelta type '{}' during accumulation",
                    delta_type
                );
            }
        }
    }

    /// Finalizes the step at `index` (called on `step.stop`).
    ///
    /// Parses any buffered `arguments_delta` fragments into the function
    /// call's `arguments`.
    pub(crate) fn stop(&mut self, index: usize) {
        if let Some(entry) = self.steps.get_mut(&index) {
            Self::finalize_entry(entry);
        }
    }

    fn finalize_entry(entry: &mut AccumulatedStep) {
        if entry.args_buffer.is_empty() {
            return;
        }
        if let Step::FunctionCall { arguments, .. } = &mut entry.step {
            match serde_json::from_str::<serde_json::Value>(&entry.args_buffer) {
                Ok(parsed) => *arguments = parsed,
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse accumulated arguments_delta buffer as JSON: {}. \
                         Preserving raw string.",
                        e
                    );
                    *arguments = serde_json::Value::String(std::mem::take(&mut entry.args_buffer));
                }
            }
        }
        entry.args_buffer.clear();
    }

    /// Consumes the accumulator and returns the ordered steps.
    pub(crate) fn finish(mut self) -> Vec<Step> {
        for entry in self.steps.values_mut() {
            Self::finalize_entry(entry);
        }
        self.steps.into_values().map(|e| e.step).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // Step wire fixtures (shapes derived from google-genai 2.10 generated
    // bindings for API revision 2026-05-20)
    // =========================================================================

    #[test]
    fn test_step_user_input_roundtrip() {
        let json_str = r#"{"type":"user_input","content":[{"type":"text","text":"Hello"}]}"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        assert!(matches!(&step, Step::UserInput { content } if content.len() == 1));
        assert_eq!(step.as_text(), Some("Hello"));

        let out = serde_json::to_value(&step).unwrap();
        assert_eq!(out["type"], "user_input");
        assert_eq!(out["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_step_model_output_with_error() {
        let json_str = r#"{
            "type": "model_output",
            "content": [{"type": "text", "text": "Partial"}],
            "error": {"code": 8, "message": "quota exhausted"}
        }"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        match &step {
            Step::ModelOutput { content, error } => {
                assert_eq!(content.len(), 1);
                let error = error.as_ref().unwrap();
                assert_eq!(error.code, Some(8));
                assert_eq!(error.message.as_deref(), Some("quota exhausted"));
            }
            other => panic!("Expected ModelOutput, got {other:?}"),
        }
    }

    #[test]
    fn test_step_thought_roundtrip() {
        let json_str = r#"{
            "type": "thought",
            "signature": "sig-abc",
            "summary": [{"type": "text", "text": "Thinking about it"}]
        }"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        match &step {
            Step::Thought { signature, summary } => {
                assert_eq!(signature.as_deref(), Some("sig-abc"));
                assert_eq!(summary.len(), 1);
            }
            other => panic!("Expected Thought, got {other:?}"),
        }
        assert_eq!(step.signature(), Some("sig-abc"));

        let out = serde_json::to_value(&step).unwrap();
        assert_eq!(out["type"], "thought");
        assert_eq!(out["signature"], "sig-abc");
    }

    #[test]
    fn test_step_function_call_roundtrip() {
        let json_str = r#"{
            "type": "function_call",
            "id": "call_1",
            "name": "get_weather",
            "arguments": {"city": "Tokyo"}
        }"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        match &step {
            Step::FunctionCall {
                id,
                name,
                arguments,
            } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "get_weather");
                assert_eq!(arguments["city"], "Tokyo");
            }
            other => panic!("Expected FunctionCall, got {other:?}"),
        }

        let out = serde_json::to_value(&step).unwrap();
        assert_eq!(out["type"], "function_call");
        assert_eq!(out["id"], "call_1");
        assert_eq!(out["arguments"]["city"], "Tokyo");
    }

    #[test]
    fn test_step_function_result_payload_union() {
        // String result
        let s: Step = serde_json::from_str(
            r#"{"type":"function_result","call_id":"c1","result":"22 degrees"}"#,
        )
        .unwrap();
        match &s {
            Step::FunctionResult { result, .. } => {
                assert_eq!(result.as_text(), Some("22 degrees"));
            }
            other => panic!("Expected FunctionResult, got {other:?}"),
        }

        // Object result
        let s: Step = serde_json::from_str(
            r#"{"type":"function_result","call_id":"c2","result":{"temp":22},"is_error":false}"#,
        )
        .unwrap();
        match &s {
            Step::FunctionResult {
                result, is_error, ..
            } => {
                assert_eq!(result.as_json().unwrap()["temp"], 22);
                assert_eq!(*is_error, Some(false));
            }
            other => panic!("Expected FunctionResult, got {other:?}"),
        }

        // Content-block list result
        let s: Step = serde_json::from_str(
            r#"{"type":"function_result","call_id":"c3","result":[{"type":"text","text":"hi"}]}"#,
        )
        .unwrap();
        match &s {
            Step::FunctionResult { result, .. } => {
                let contents = result.as_contents().unwrap();
                assert_eq!(contents.len(), 1);
                assert_eq!(contents[0].as_text(), Some("hi"));
            }
            other => panic!("Expected FunctionResult, got {other:?}"),
        }
    }

    #[test]
    fn test_step_code_execution_nested_arguments_roundtrip() {
        let json_str = r#"{
            "type": "code_execution_call",
            "id": "exec_1",
            "arguments": {"language": "python", "code": "print(42)"},
            "signature": "sig-1"
        }"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        match &step {
            Step::CodeExecutionCall {
                id,
                language,
                code,
                signature,
            } => {
                assert_eq!(id, "exec_1");
                assert_eq!(*language, CodeExecutionLanguage::Python);
                assert_eq!(code, "print(42)");
                assert_eq!(signature.as_deref(), Some("sig-1"));
            }
            other => panic!("Expected CodeExecutionCall, got {other:?}"),
        }

        // Serialization nests language/code back under arguments.
        let out = serde_json::to_value(&step).unwrap();
        assert_eq!(out["arguments"]["language"], "python");
        assert_eq!(out["arguments"]["code"], "print(42)");
        assert_eq!(out["signature"], "sig-1");
    }

    #[test]
    fn test_step_google_search_call_with_search_type() {
        let json_str = r#"{
            "type": "google_search_call",
            "id": "s1",
            "arguments": {"queries": ["rust serde"]},
            "search_type": "web_search",
            "signature": "sig"
        }"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        match &step {
            Step::GoogleSearchCall {
                queries,
                search_type,
                ..
            } => {
                assert_eq!(queries, &["rust serde".to_string()]);
                assert!(matches!(
                    search_type,
                    Some(crate::tools::SearchType::WebSearch)
                ));
            }
            other => panic!("Expected GoogleSearchCall, got {other:?}"),
        }
    }

    #[test]
    fn test_step_mcp_server_tool_call_roundtrip() {
        let json_str = r#"{
            "type": "mcp_server_tool_call",
            "id": "m1",
            "name": "read_file",
            "server_name": "fs",
            "arguments": {"path": "/tmp/x"}
        }"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        match &step {
            Step::McpServerToolCall {
                name, server_name, ..
            } => {
                assert_eq!(name, "read_file");
                assert_eq!(server_name, "fs");
            }
            other => panic!("Expected McpServerToolCall, got {other:?}"),
        }
        let out = serde_json::to_value(&step).unwrap();
        assert_eq!(out["server_name"], "fs");
    }

    #[test]
    #[cfg(not(feature = "strict-unknown"))]
    fn test_step_unknown_preserves_data_and_roundtrips() {
        let json_str = r#"{"type":"quantum_step","novel_field":42}"#;
        let step: Step = serde_json::from_str(json_str).unwrap();
        assert!(step.is_unknown());
        assert_eq!(step.unknown_step_type(), Some("quantum_step"));
        assert_eq!(step.unknown_data().unwrap()["novel_field"], 42);

        let out = serde_json::to_value(&step).unwrap();
        assert_eq!(out["type"], "quantum_step");
        assert_eq!(out["novel_field"], 42);
    }

    #[test]
    fn test_step_type_accessor() {
        assert_eq!(Step::user_text("x").step_type(), "user_input");
        assert_eq!(
            Step::function_call("id", "f", json!({})).step_type(),
            "function_call"
        );
        assert_eq!(
            Step::Unknown {
                step_type: "future".into(),
                data: serde_json::Value::Null
            }
            .step_type(),
            "future"
        );
    }

    // =========================================================================
    // StepDelta wire fixtures
    // =========================================================================

    #[test]
    fn test_step_delta_text() {
        let delta: StepDelta = serde_json::from_str(r#"{"type":"text","text":"Hel"}"#).unwrap();
        assert_eq!(delta.as_text(), Some("Hel"));
        let out = serde_json::to_value(&delta).unwrap();
        assert_eq!(out, json!({"type": "text", "text": "Hel"}));
    }

    #[test]
    fn test_step_delta_arguments_delta() {
        let delta: StepDelta =
            serde_json::from_str(r#"{"type":"arguments_delta","arguments":"{\"city\": \"To"}"#)
                .unwrap();
        assert_eq!(delta.as_arguments_delta(), Some("{\"city\": \"To"));
    }

    #[test]
    fn test_step_delta_audio_with_rate_and_channels() {
        let delta: StepDelta = serde_json::from_str(
            r#"{"type":"audio","data":"QUJD","mime_type":"audio/l16","rate":24000,"sample_rate":24000,"channels":1}"#,
        )
        .unwrap();
        match &delta {
            StepDelta::Audio {
                sample_rate,
                channels,
                rate,
                ..
            } => {
                assert_eq!(*sample_rate, Some(24000));
                assert_eq!(*rate, Some(24000));
                assert_eq!(*channels, Some(1));
            }
            other => panic!("Expected Audio, got {other:?}"),
        }
    }

    #[test]
    fn test_step_delta_thought_summary_and_signature() {
        let summary: StepDelta = serde_json::from_str(
            r#"{"type":"thought_summary","content":{"type":"text","text":"Analyzing"}}"#,
        )
        .unwrap();
        assert!(matches!(summary, StepDelta::ThoughtSummary { .. }));

        let sig: StepDelta =
            serde_json::from_str(r#"{"type":"thought_signature","signature":"abc123"}"#).unwrap();
        assert!(matches!(
            sig,
            StepDelta::ThoughtSignature { signature: Some(s) } if s == "abc123"
        ));
    }

    #[test]
    fn test_step_delta_text_annotation() {
        let delta: StepDelta = serde_json::from_str(
            r#"{"type":"text_annotation_delta","annotations":[
                {"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":5}
            ]}"#,
        )
        .unwrap();
        match &delta {
            StepDelta::TextAnnotation { annotations } => {
                assert_eq!(annotations.len(), 1);
                assert!(matches!(annotations[0], Annotation::UrlCitation { .. }));
            }
            other => panic!("Expected TextAnnotation, got {other:?}"),
        }
    }

    #[test]
    fn test_step_delta_unknown_roundtrip() {
        let delta: StepDelta = serde_json::from_str(r#"{"type":"hologram","frames":3}"#).unwrap();
        assert!(delta.is_unknown());
        assert_eq!(delta.unknown_delta_type(), Some("hologram"));
        assert_eq!(delta.unknown_data().unwrap()["frames"], 3);
        let out = serde_json::to_value(&delta).unwrap();
        assert_eq!(out["type"], "hologram");
        assert_eq!(out["frames"], 3);
    }

    // =========================================================================
    // FunctionResultPayload
    // =========================================================================

    #[test]
    fn test_function_result_payload_from_value() {
        assert!(matches!(
            FunctionResultPayload::from_value(json!("text")),
            FunctionResultPayload::Text(_)
        ));
        assert!(matches!(
            FunctionResultPayload::from_value(json!({"a": 1})),
            FunctionResultPayload::Json(_)
        ));
        assert!(matches!(
            FunctionResultPayload::from_value(json!([{"type": "text", "text": "hi"}])),
            FunctionResultPayload::Contents(_)
        ));
        // Non-content array stays raw JSON
        assert!(matches!(
            FunctionResultPayload::from_value(json!([1, 2, 3])),
            FunctionResultPayload::Json(_)
        ));
    }

    #[test]
    fn test_function_result_payload_roundtrip() {
        for payload in [
            FunctionResultPayload::Text("hello".into()),
            FunctionResultPayload::Json(json!({"k": [1, 2]})),
            FunctionResultPayload::Contents(vec![Content::text("block")]),
        ] {
            let serialized = serde_json::to_string(&payload).unwrap();
            let back: FunctionResultPayload = serde_json::from_str(&serialized).unwrap();
            assert_eq!(payload, back);
        }
    }

    // =========================================================================
    // StepAccumulator
    // =========================================================================

    #[test]
    fn test_accumulator_text_stream() {
        let mut acc = StepAccumulator::new();
        acc.start(0, Step::model_output(vec![]));
        acc.apply_delta(
            0,
            &StepDelta::Text {
                text: "Hello ".into(),
            },
        );
        acc.apply_delta(
            0,
            &StepDelta::Text {
                text: "world".into(),
            },
        );
        acc.stop(0);
        let steps = acc.finish();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].as_text(), Some("Hello world"));
    }

    #[test]
    fn test_accumulator_function_call_arguments_delta() {
        let mut acc = StepAccumulator::new();
        acc.start(
            0,
            Step::FunctionCall {
                id: "c1".into(),
                name: "get_weather".into(),
                arguments: serde_json::Value::Null,
            },
        );
        acc.apply_delta(
            0,
            &StepDelta::ArgumentsDelta {
                arguments: "{\"city\": ".into(),
            },
        );
        acc.apply_delta(
            0,
            &StepDelta::ArgumentsDelta {
                arguments: "\"Tokyo\"}".into(),
            },
        );
        acc.stop(0);
        let steps = acc.finish();
        match &steps[0] {
            Step::FunctionCall { arguments, .. } => assert_eq!(arguments["city"], "Tokyo"),
            other => panic!("Expected FunctionCall, got {other:?}"),
        }
    }

    #[test]
    fn test_accumulator_thought_summary_and_signature() {
        let mut acc = StepAccumulator::new();
        acc.start(
            0,
            Step::Thought {
                signature: None,
                summary: vec![],
            },
        );
        acc.apply_delta(
            0,
            &StepDelta::ThoughtSummary {
                content: Some(Content::text("Consider ")),
            },
        );
        acc.apply_delta(
            0,
            &StepDelta::ThoughtSummary {
                content: Some(Content::text("the problem")),
            },
        );
        acc.apply_delta(
            0,
            &StepDelta::ThoughtSignature {
                signature: Some("sig-xyz".into()),
            },
        );
        let steps = acc.finish();
        match &steps[0] {
            Step::Thought { signature, summary } => {
                assert_eq!(signature.as_deref(), Some("sig-xyz"));
                assert_eq!(summary[0].as_text(), Some("Consider the problem"));
            }
            other => panic!("Expected Thought, got {other:?}"),
        }
    }

    #[test]
    fn test_accumulator_delta_without_start_creates_model_output() {
        let mut acc = StepAccumulator::new();
        acc.apply_delta(
            0,
            &StepDelta::Text {
                text: "orphan".into(),
            },
        );
        let steps = acc.finish();
        assert_eq!(steps[0].as_text(), Some("orphan"));
    }

    #[test]
    fn test_accumulator_orders_steps_by_index() {
        let mut acc = StepAccumulator::new();
        acc.start(2, Step::model_text("second"));
        acc.start(1, Step::thought("sig"));
        let steps = acc.finish();
        assert!(matches!(steps[0], Step::Thought { .. }));
        assert!(matches!(steps[1], Step::ModelOutput { .. }));
    }
}
