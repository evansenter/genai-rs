//! Recovering which MCP server and tool ran (#459).

use serde::Serialize;

use super::InteractionResponse;
use crate::steps::{FunctionResultPayload, Step};

/// One MCP tool call the server ran, with the server and tool it named.
///
/// Returned by [`InteractionResponse::mcp_calls`]. This is a **view type**
/// that borrows from the response.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct McpCallInfo<'a> {
    /// The call's id (the `call_id` of its result).
    pub id: &'a str,
    /// The MCP server: the `name` the request gave
    /// [`McpServerConfig`](crate::McpServerConfig).
    pub server: &'a str,
    /// The tool on that server.
    pub tool: &'a str,
    /// The arguments the model passed to the tool.
    pub arguments: &'a serde_json::Value,
    /// What the tool returned, when the response carries the result. On the
    /// live `function_result` shape (observed 2026-11-04) this is an object
    /// with the tool's text under `result` and the raw MCP `CallToolResult`
    /// as a JSON string under `call_tool_result_json`. A failed call adds
    /// `is_error: true`; a successful one may add `structured_content`, the
    /// tool's structured output as an object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<&'a FunctionResultPayload>,
    /// Whether the tool call failed: `true` when either the result step's
    /// `is_error` or the result object's own `is_error` key is `true`. On
    /// failed calls the live API set the step's flag to `false` and only the
    /// result object's key to `true` (2026-11-04), so the step's flag alone
    /// misses them. `None` when neither says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Whether an MCP call failed, from the result step's `is_error` and the
/// `is_error` key of its result object. Either one being `true` is a
/// failure; the live API has sent the step's as `false` on a failed call
/// (2026-11-04).
fn call_failed(step_flag: Option<bool>, result: Option<&FunctionResultPayload>) -> Option<bool> {
    let payload_flag = result
        .and_then(FunctionResultPayload::as_json)
        .and_then(|value| value.get("is_error"))
        .and_then(serde_json::Value::as_bool);
    match (step_flag, payload_flag) {
        (None, None) => None,
        (step, payload) => Some(step == Some(true) || payload == Some(true)),
    }
}

/// Splits a `<server>:<tool>` function name at its first colon.
fn split_mcp_name(name: &str) -> Option<(&str, &str)> {
    name.split_once(':')
        .filter(|(server, tool)| !server.is_empty() && !tool.is_empty())
}

impl InteractionResponse {
    /// The MCP tool calls the server ran in this response, in order, with
    /// the server and tool each one named.
    ///
    /// Reads both shapes that identify the server:
    ///
    /// - **Live (since 2026-10-01, re-verified 2026-11-04 on
    ///   `gemini-3.8-flash`, streamed and not):** a `function_call` named
    ///   `<server>:<tool>`, where `<server>` is the name the request gave
    ///   [`McpServerConfig`](crate::McpServerConfig), answered by a
    ///   `function_result` with the same id in the same response. A call
    ///   counts only when it was answered here: a client function may also
    ///   be named `a:b` (the API accepted `weather:get` as a declared name on
    ///   2026-11-04), but the client answers its own calls in a later
    ///   request, so a call answered within the response was run by the
    ///   server. The name splits at its first colon. A failed call is
    ///   marked in the result object (`is_error: true`) while the step's
    ///   own `is_error` stays `false`, which [`McpCallInfo::is_error`]
    ///   accounts for.
    /// - **Spec-only:** `mcp_server_tool_call` steps, which carry
    ///   `server_name` and `name` directly, paired with
    ///   `mcp_server_tool_result` by id. Never observed.
    ///
    /// The 2026-08-16 shape, a bare [`Step::ToolCall`], names neither and
    /// is not returned; see [`tool_calls`](Self::tool_calls).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::InteractionResponse;
    /// # let response: InteractionResponse = todo!();
    /// for call in response.mcp_calls() {
    ///     println!("{} ran {} with {}", call.server, call.tool, call.arguments);
    /// }
    /// ```
    #[must_use]
    pub fn mcp_calls(&self) -> Vec<McpCallInfo<'_>> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                Step::FunctionCall {
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    let (server, tool) = split_mcp_name(name)?;
                    let (result, is_error) = self.steps.iter().find_map(|s| match s {
                        Step::FunctionResult {
                            call_id,
                            result,
                            is_error,
                            ..
                        } if call_id == id => Some((result, *is_error)),
                        _ => None,
                    })?;
                    Some(McpCallInfo {
                        id,
                        server,
                        tool,
                        arguments,
                        result: Some(result),
                        is_error: call_failed(is_error, Some(result)),
                    })
                }
                Step::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                    ..
                } => {
                    let result = self.steps.iter().find_map(|s| match s {
                        Step::McpServerToolResult {
                            call_id, result, ..
                        } if call_id == id => Some(result),
                        _ => None,
                    });
                    Some(McpCallInfo {
                        id,
                        server: server_name,
                        tool: name,
                        arguments,
                        result,
                        is_error: call_failed(None, result),
                    })
                }
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
