//! Lifecycle hook and policy-decision messages: the harness asking this
//! client to run a hook (`callHookRequest`) or judge a dynamic policy rule
//! (`policyDecisionRequest`), and this client's answers.

use super::{
    HookDecision, LifecycleHook, PolicyEvaluationOutcome, StopDecision, StopReason, UserInput,
    flex_num,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// =============================================================================
// Harness -> client
// =============================================================================

/// `CallHookRequest` — the harness invoking a client-side lifecycle hook.
///
/// The `*_args` fields form a oneof (`args`): at most one is set. Session
/// start and end hooks carry no arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallHookRequest {
    /// Correlation id; echo it in the [`CallHookResponse`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Hook name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Which lifecycle point fired.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub hook_type: Option<LifecycleHook>,
    /// Pre-turn arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_turn_args: Option<PreTurnArgs>,
    /// Post-turn arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_turn_args: Option<PostTurnArgs>,
    /// Pre-tool arguments (the hook may deny the call).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_tool_args: Option<PreToolArgs>,
    /// Post-tool arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_tool_args: Option<PostToolArgs>,
    /// Tool-error arguments (the hook may replace the error text).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_tool_error_args: Option<OnToolErrorArgs>,
    /// Compaction arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_compaction_args: Option<OnCompactionArgs>,
    /// Stop arguments (the hook may keep the turn going).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_args: Option<StopArgs>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `PreToolArgs`.
///
/// `tool_name` is the harness's *step field* name for a builtin
/// (`invoke_subagent`, `list_directory`) and the bare tool name for an MCP
/// tool, whose server is in `server_name`. The bridge maps an MCP call onto
/// its `mcp_<server>_<tool>` policy target before evaluating policies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreToolArgs {
    /// The tool about to run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Its arguments, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_json: Option<String>,
    /// The MCP server providing the tool, for MCP tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// The harness's correlation id for the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// The trajectory making the call (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// The step making the call (new in harness 0.1.18).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub step_index: Option<u32>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `PostToolArgs`. `tool_name` follows the same convention as
/// [`PreToolArgs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PostToolArgs {
    /// The tool that ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Its result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// The error, if it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The MCP server providing the tool, for MCP tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// The harness's correlation id for the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// The trajectory that made the call (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// The step that made the call (new in harness 0.1.18).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub step_index: Option<u32>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `PreTurnArgs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreTurnArgs {
    /// The user input starting the turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input: Option<UserInput>,
    /// The trajectory the turn runs on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
}

/// `PostTurnArgs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PostTurnArgs {
    /// The final response text of the turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
    /// The trajectory the turn ran on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
}

/// `OnToolErrorArgs` — a harness-run or custom tool failed. `tool_name`
/// follows the same convention as [`PreToolArgs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OnToolErrorArgs {
    /// The tool that failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// The harness's error text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// The MCP server providing the tool, for MCP tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// The harness's correlation id for the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// The trajectory that made the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// The step that made the call.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub step_index: Option<u32>,
}

/// `OnCompactionArgs` — the harness compacted a trajectory's history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OnCompactionArgs {
    /// The compacted trajectory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// The compaction step's index.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub step_index: Option<u32>,
    /// The summary that replaced the compacted history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// `StopArgs` — the root trajectory is about to go idle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StopArgs {
    /// The turn's final response text so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
    /// The stopping trajectory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// How many times a stop hook has already kept this turn going.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub continuation_count: Option<i32>,
    /// Why the turn is stopping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// The error that ended the turn, if one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// `PolicyDecisionRequest` — the harness asking this client to judge a
/// tool call against a dynamic policy rule, or against the auto policy's
/// flag (`rule_id` `"auto"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDecisionRequest {
    /// Correlation id; echo it in the [`PolicyDecisionResponse`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The rule being judged (`"auto"` for the auto policy).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    /// The tool call, in the pre-tool hook's shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<PreToolArgs>,
    /// Why the harness is asking (the auto policy's assessment).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Decodes the harness's `genai.Struct` encoding into plain JSON.
///
/// `ToolCall.arguments` (and `ToolResponse.response`) are not
/// `google.protobuf.Struct` — whose proto-JSON form *is* plain JSON — but
/// the harness's own `genai.Struct`, which proto-JSON renders
/// structurally: `{"fields": [{"name": "city", "value": {"stringValue":
/// "SF"}}]}`. Returns `None` when `value` is not that shape. A value kind
/// with no JSON equivalent (`contentValue`, `functionValue`) is kept as its
/// raw object rather than dropped.
pub(crate) fn decode_genai_struct(value: &Value) -> Option<Value> {
    fn decode_value(value: &Value) -> Option<Value> {
        let Value::Object(kind) = value else {
            return None;
        };
        // Proto3 omits a oneof arm set to its default, so an empty object
        // is an unset value.
        let Some((arm, inner)) = kind.iter().next() else {
            return Some(Value::Null);
        };
        Some(match arm.as_str() {
            "nullValue" => Value::Null,
            "numberValue" | "stringValue" | "boolValue" => inner.clone(),
            "structValue" => decode_genai_struct(inner)?,
            "listValue" => Value::Array(
                inner
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|values| values.iter().map(decode_value).collect::<Option<_>>())
                    .unwrap_or(Some(Vec::new()))?,
            ),
            _ => value.clone(),
        })
    }

    let Value::Object(object) = value else {
        return None;
    };
    let mut decoded = Map::new();
    let Some(fields) = object.get("fields") else {
        // An empty struct omits its only field.
        return object.is_empty().then_some(Value::Object(decoded));
    };
    for field in fields.as_array()? {
        let name = field.get("name")?.as_str()?;
        let inner = field.get("value").map_or(Some(Value::Null), decode_value)?;
        decoded.insert(name.to_string(), inner);
    }
    Some(Value::Object(decoded))
}

/// Encodes a JSON object in the harness's `genai.Struct` shape, the
/// inverse of [`decode_genai_struct`]: what a pre-tool hook's
/// `modifiedArgs` must carry. A non-object `value` encodes as an empty
/// struct (tool arguments are always an object).
pub(crate) fn encode_genai_struct(value: &Value) -> Value {
    fn encode_value(value: &Value) -> Value {
        match value {
            Value::Null => serde_json::json!({"nullValue": "NULL_VALUE"}),
            Value::Bool(b) => serde_json::json!({"boolValue": b}),
            Value::Number(n) => serde_json::json!({"numberValue": n}),
            Value::String(s) => serde_json::json!({"stringValue": s}),
            Value::Array(items) => serde_json::json!({
                "listValue": {"values": items.iter().map(encode_value).collect::<Vec<_>>()}
            }),
            Value::Object(_) => serde_json::json!({"structValue": encode_genai_struct(value)}),
        }
    }
    let fields: Vec<Value> = value
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(
                    |(name, inner)| serde_json::json!({"name": name, "value": encode_value(inner)}),
                )
                .collect()
        })
        .unwrap_or_default();
    serde_json::json!({ "fields": fields })
}

/// Maps a hook callback's `(tool_name, server_name)` onto the name
/// policies target: `mcp_<server>_<tool>` for an MCP tool, the builtin's
/// wire name or the custom tool's name otherwise. Without the mapping an
/// MCP rule would never match the bare tool name the harness sends.
#[must_use]
pub(crate) fn hook_tool_name(tool_name: &str, server_name: Option<&str>) -> String {
    match server_name.filter(|s| !s.is_empty()) {
        Some(server) => crate::antigravity::policies::mcp_tool_name(server, tool_name),
        None => tool_name.to_string(),
    }
}

// =============================================================================
// Client -> harness
// =============================================================================

/// `CallHookResponse` — the client's reply to a hook callback.
///
/// The result fields form a oneof (`result`): set exactly one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallHookResponse {
    /// Correlates with [`CallHookRequest::request_id`].
    pub request_id: String,
    /// Verdict for a pre-turn hook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_turn_result: Option<HookVerdict>,
    /// Verdict for a pre-tool hook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_tool_result: Option<HookVerdict>,
    /// Acknowledgement for observe-only hooks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_result: Option<EmptyResult>,
    /// Hook execution failed on the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// A tool-error hook's replacement error text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_tool_error_result: Option<OnToolErrorResult>,
    /// A stop hook's verdict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_result: Option<StopResult>,
}

/// `PreToolResult` / `PreTurnResult` — an allow/deny verdict with a reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookVerdict {
    /// The decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
    /// Human-readable reason (surfaced to the model on deny).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Replacement tool arguments, in the harness's `genai.Struct` shape
    /// (`PreToolResult` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_args: Option<Value>,
}

/// `EmptyResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmptyResult {}

/// `OnToolErrorResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OnToolErrorResult {
    /// Replaces the error text the model sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_error_message: Option<String>,
}

/// `StopResult`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    /// Stop, or keep going.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<StopDecision>,
    /// With `CONTINUE`: the instruction the harness injects before the next
    /// model call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `PolicyDecisionResponse` — this client's verdict on a
/// [`PolicyDecisionRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDecisionResponse {
    /// Correlates with [`PolicyDecisionRequest::request_id`].
    pub request_id: String,
    /// Allow, deny, or (for a rule whose condition did not hold) no match,
    /// which lets the harness fall through to the next rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<PolicyEvaluationOutcome>,
    /// The message the model sees on a denial.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_reason: Option<String>,
}
