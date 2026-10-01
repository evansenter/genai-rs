use super::{
    LifecycleHook, Modality, StepUpdate, StopReason, TrajectoryState, UserInput, flex_num,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// =============================================================================
// Harness -> client events (OutputEvent oneof)
// =============================================================================

/// `OutputEvent` — the harness-to-client message envelope.
///
/// Carries envelope metadata (`seq_num`, `timestamp_micros`,
/// `usage_metadata`) alongside the oneof payload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputEvent {
    /// Monotonic sequence number.
    pub seq_num: Option<i64>,
    /// Event timestamp in microseconds since the Unix epoch.
    pub timestamp_micros: Option<i64>,
    /// Token usage, attached to some step updates.
    pub usage_metadata: Option<UsageMetadata>,
    /// The event payload (`None` if the oneof was empty).
    pub payload: Option<OutputPayload>,
}

/// The oneof payload of an [`OutputEvent`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum OutputPayload {
    /// Progress on one agent step. Boxed: `StepUpdate` is by far the
    /// largest message in the protocol.
    StepUpdate(Box<StepUpdate>),
    /// A trajectory changed lifecycle state.
    TrajectoryStateUpdate(TrajectoryStateUpdate),
    /// The model called a client-executed custom tool.
    ToolCall(ToolCall),
    /// Reply to the initial `InitializeConversationEvent`.
    InitializeConversationResponse(InitializeConversationResponse),
    /// The harness is invoking a client-side lifecycle hook.
    CallHookRequest(CallHookRequest),
    /// Session-end hooks completed.
    SessionEndResponse(bool),
    /// An event variant this crate does not recognize (Evergreen).
    Unknown {
        /// The unrecognized oneof field name.
        event_type: String,
        /// The raw JSON payload, preserved for roundtrip.
        data: Value,
    },
}

impl OutputPayload {
    /// Check if this is an unknown event.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized oneof field name if this is an unknown event.
    #[must_use]
    pub fn unknown_event_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { event_type, .. } => Some(event_type),
            _ => None,
        }
    }

    /// Returns the preserved raw JSON if this is an unknown event.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

const OUTPUT_PAYLOAD_KEYS: &[&str] = &[
    "stepUpdate",
    "trajectoryStateUpdate",
    "toolCall",
    "initializeConversationResponse",
    "callHookRequest",
    "sessionEndResponse",
];

impl Serialize for OutputEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeMap};
        let mut len = 0;
        len += usize::from(self.seq_num.is_some());
        len += usize::from(self.timestamp_micros.is_some());
        len += usize::from(self.usage_metadata.is_some());
        len += usize::from(self.payload.is_some());
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(seq_num) = self.seq_num {
            map.serialize_entry("seqNum", &seq_num)?;
        }
        if let Some(timestamp_micros) = self.timestamp_micros {
            map.serialize_entry("timestampMicros", &timestamp_micros)?;
        }
        if let Some(payload) = &self.payload {
            let (key, value) = match payload {
                OutputPayload::StepUpdate(v) => (
                    "stepUpdate",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::TrajectoryStateUpdate(v) => (
                    "trajectoryStateUpdate",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::ToolCall(v) => (
                    "toolCall",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::InitializeConversationResponse(v) => (
                    "initializeConversationResponse",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::CallHookRequest(v) => (
                    "callHookRequest",
                    serde_json::to_value(v).map_err(S::Error::custom)?,
                ),
                OutputPayload::SessionEndResponse(b) => ("sessionEndResponse", Value::Bool(*b)),
                OutputPayload::Unknown { event_type, data } => (event_type.as_str(), data.clone()),
            };
            map.serialize_entry(key, &value)?;
        }
        if let Some(usage_metadata) = &self.usage_metadata {
            map.serialize_entry("usageMetadata", usage_metadata)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for OutputEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut map = Map::deserialize(deserializer)?;

        fn take_i64<E: Error>(map: &mut Map<String, Value>, key: &str) -> Result<Option<i64>, E> {
            match map.remove(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Number(n)) => n
                    .as_i64()
                    .map(Some)
                    .ok_or_else(|| E::custom(format!("{key}: number does not fit in i64"))),
                Some(Value::String(s)) => s
                    .parse::<i64>()
                    .map(Some)
                    .map_err(|e| E::custom(format!("{key}: invalid i64 string: {e}"))),
                Some(other) => Err(E::custom(format!("{key}: expected i64, got {other}"))),
            }
        }

        let seq_num = take_i64(&mut map, "seqNum")?;
        let timestamp_micros = take_i64(&mut map, "timestampMicros")?;
        // Harness 0.1.10 replaced the flat `usageMetadata` with
        // `usageUpdate: {agents: [...], total: UsageMetadata}`. Read the
        // aggregate from either spelling so one build drives both
        // revisions; the per-trajectory `agents` breakdown is not
        // modeled yet, and either key must be consumed here or it falls
        // through to the leftover-key arm below and is misreported as an
        // unknown oneof variant.
        // Consume `usageUpdate` unconditionally, before choosing between
        // the two spellings: if a transitional harness ever sent *both*,
        // leaving it in `map` would hand it to the leftover-key arm below
        // and surface a bogus `Unknown` payload — the exact misreport this
        // branch exists to prevent, in the one shape a nested match
        // wouldn't cover.
        let usage_update = map.remove("usageUpdate");
        let usage_metadata = match map.remove("usageMetadata") {
            Some(Value::Null) | None => match usage_update {
                None | Some(Value::Null) => None,
                Some(Value::Object(mut update)) => match update.remove("total") {
                    None | Some(Value::Null) => None,
                    Some(total) => Some(serde_json::from_value(total).map_err(D::Error::custom)?),
                },
                Some(other) => {
                    tracing::warn!("Unexpected JSON type for usageUpdate, dropping usage: {other}");
                    None
                }
            },
            Some(v) => Some(serde_json::from_value(v).map_err(D::Error::custom)?),
        };

        let mut payload = None;
        for key in OUTPUT_PAYLOAD_KEYS {
            if let Some(value) = map.remove(*key) {
                payload = Some(match *key {
                    "stepUpdate" => OutputPayload::StepUpdate(Box::new(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    )),
                    "trajectoryStateUpdate" => OutputPayload::TrajectoryStateUpdate(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    ),
                    "toolCall" => OutputPayload::ToolCall(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    ),
                    "initializeConversationResponse" => {
                        OutputPayload::InitializeConversationResponse(
                            serde_json::from_value(value).map_err(D::Error::custom)?,
                        )
                    }
                    "callHookRequest" => OutputPayload::CallHookRequest(
                        serde_json::from_value(value).map_err(D::Error::custom)?,
                    ),
                    "sessionEndResponse" => {
                        OutputPayload::SessionEndResponse(value.as_bool().unwrap_or_default())
                    }
                    _ => unreachable!("key list is exhaustive"),
                });
                break;
            }
        }
        // Any leftover field is an unrecognized oneof variant: preserve it.
        if payload.is_none()
            && let Some((event_type, data)) = map.into_iter().next()
        {
            tracing::warn!(
                "Unknown OutputEvent variant: '{}'. Preserving in Unknown variant.",
                event_type
            );
            payload = Some(OutputPayload::Unknown { event_type, data });
        }

        Ok(Self {
            seq_num,
            timestamp_micros,
            usage_metadata,
            payload,
        })
    }
}

/// `InitializeConversationResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeConversationResponse {
    /// The conversation id (persist to resume this session later).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cascade_id: Option<String>,
    /// Restored steps when resuming a saved conversation.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub history: Vec<StepUpdate>,
    /// Token usage accumulated by a resumed conversation so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cumulative_usage: Option<UsageMetadata>,
    /// Per-trajectory breakdown of `cumulative_usage`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub trajectory_usage: Vec<TrajectoryUsageEntry>,
    /// Whether the OS command sandbox can be enforced (new in harness
    /// 0.1.18). Only meaningful when `run_command` sandboxing was
    /// requested, which this crate does not do yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_status: Option<SandboxStatus>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `TrajectoryUsageEntry` — token usage attributed to one trajectory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryUsageEntry {
    /// The trajectory the usage belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// Its usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
}

/// `SandboxStatus` — whether the harness can enforce the OS command
/// sandbox.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    /// Whether the sandbox actually enforces isolation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    /// Why not, when `available` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ModalityTokenCount` — a token count for one content modality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModalityTokenCount {
    /// The modality counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modality: Option<Modality>,
    /// Tokens of that modality.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub token_count: Option<u64>,
}

/// `UsageMetadata` — token accounting for a step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Prompt tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub prompt_token_count: Option<u64>,
    /// Cached-content tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub cached_content_token_count: Option<u64>,
    /// Response candidate tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub candidates_token_count: Option<u64>,
    /// Thinking tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub thoughts_token_count: Option<u64>,
    /// Total tokens.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub total_token_count: Option<u64>,
    /// The service tier the backend served the request on (a raw string:
    /// the value set differs between the Gemini API and Vertex AI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Prompt tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub prompt_tokens_details: Vec<ModalityTokenCount>,
    /// Cached tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cache_tokens_details: Vec<ModalityTokenCount>,
    /// Response tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub candidates_tokens_details: Vec<ModalityTokenCount>,
    /// Tool-use prompt tokens by modality (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_use_prompt_tokens_details: Vec<ModalityTokenCount>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `TrajectoryStateUpdate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrajectoryStateUpdate {
    /// The trajectory that changed state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// The new state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<TrajectoryState>,
    /// Error message (e.g. why the turn was cancelled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The trajectory that spawned this one — set for subagent
    /// trajectories, absent for the root conversation (new in harness
    /// 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_trajectory_id: Option<String>,
    /// Subagent nesting depth (`0`/absent for the root conversation).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub depth: Option<i32>,
    /// Why the trajectory stopped, when it was not a normal completion
    /// (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ToolCall` — the model invoking a client-executed custom tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// Correlation id; echo it in the [`ToolResponse`](super::ToolResponse).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The tool name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Arguments, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_json: Option<String>,
    /// Arguments as a structured value (protobuf `Struct`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// The trajectory that made the call — lets a subagent's custom tool
    /// calls be told apart from the parent's (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `CallHookRequest` — the harness invoking a client-side lifecycle hook.
///
/// The `*_args` fields form a oneof (`args`): at most one is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallHookRequest {
    /// Correlation id; echo it in the [`CallHookResponse`](super::CallHookResponse).
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
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `PreToolArgs`.
///
/// `tool_name` is the harness's *step field* name for a builtin (so the
/// subagent builtin arrives as `invoke_subagent`, not `start_subagent`)
/// and the bare tool name for an MCP tool, whose server is in
/// `server_name`. The bridge maps both onto policy targets before
/// evaluating policies (`mcp_<server>_<tool>`, `start_subagent`).
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

/// Maps a hook callback's `(tool_name, server_name)` onto the name
/// policies target: `mcp_<server>_<tool>` for MCP tools, and the builtin's
/// public wire name for builtins.
///
/// The harness names a builtin in hook args by its `StepUpdate` field, and
/// exactly one differs from the public name: `invoke_subagent` is the
/// `start_subagent` builtin (the reference SDK maps it the same way). Left
/// unmapped, `policy::deny("start_subagent")` would never fire on the hook
/// path, and an MCP rule would never match its bare tool name.
#[must_use]
pub(crate) fn hook_tool_name(tool_name: &str, server_name: Option<&str>) -> String {
    match server_name.filter(|s| !s.is_empty()) {
        Some(server) => crate::antigravity::hooks::mcp_tool_name(server, tool_name),
        None if tool_name == "invoke_subagent" => "start_subagent".to_string(),
        None => tool_name.to_string(),
    }
}

/// `PreTurnArgs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreTurnArgs {
    /// The user input starting the turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input: Option<UserInput>,
}

/// `PostTurnArgs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PostTurnArgs {
    /// The final response text of the turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
}
