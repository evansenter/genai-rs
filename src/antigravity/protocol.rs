//! Proto-JSON wire types for the localharness WebSocket protocol.
//!
//! Everything after the stdio handshake travels as **proto-JSON** (the
//! protobuf JSON mapping) over a localhost WebSocket:
//!
//! - field names are `camelCase`;
//! - enums are `SCREAMING_SNAKE_CASE` strings;
//! - 64-bit integers are emitted as JSON *strings* by the harness (this
//!   module accepts both strings and numbers, and re-serializes as numbers —
//!   value-preserving, accepted by the harness's parser);
//! - a `oneof` sets at most one of its fields.
//!
//! Types follow the crate's Evergreen philosophy: unrecognized oneof
//! variants deserialize into `Unknown` variants, unrecognized enum strings
//! are preserved, and unrecognized fields on harness-emitted messages are
//! captured in `extra` maps so they roundtrip.
//!
//! Message and field shapes are verified against the descriptor set shipped
//! in the `google-antigravity` 0.1.5 wheel (`localharness.proto`, package
//! `antigravity.localharness`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

// =============================================================================
// Flexible numeric deserialization (proto-JSON int64/uint64 arrive as strings)
// =============================================================================

pub(crate) mod flex_num {
    use serde::{Deserialize, Deserializer, de::Error};
    use serde_json::Value;

    fn value_to_i64<E: Error>(value: &Value) -> Result<i64, E> {
        match value {
            Value::Number(n) => n
                .as_i64()
                .ok_or_else(|| E::custom(format!("number {n} does not fit in i64"))),
            Value::String(s) => s
                .parse::<i64>()
                .map_err(|e| E::custom(format!("invalid i64 string {s:?}: {e}"))),
            other => Err(E::custom(format!("expected i64, got {other}"))),
        }
    }

    fn value_to_u64<E: Error>(value: &Value) -> Result<u64, E> {
        match value {
            Value::Number(n) => n
                .as_u64()
                .ok_or_else(|| E::custom(format!("number {n} does not fit in u64"))),
            Value::String(s) => s
                .parse::<u64>()
                .map_err(|e| E::custom(format!("invalid u64 string {s:?}: {e}"))),
            other => Err(E::custom(format!("expected u64, got {other}"))),
        }
    }

    pub fn opt_u64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => value_to_u64(&v).map(Some),
        }
    }

    pub fn opt_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                let raw = value_to_u64(&v)?;
                u32::try_from(raw)
                    .map(Some)
                    .map_err(|_| Error::custom(format!("value {raw} does not fit in u32")))
            }
        }
    }

    pub fn opt_i32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i32>, D::Error> {
        match Option::<Value>::deserialize(d)? {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                let raw = value_to_i64(&v)?;
                i32::try_from(raw)
                    .map(Some)
                    .map_err(|_| Error::custom(format!("value {raw} does not fit in i32")))
            }
        }
    }
}

// =============================================================================
// String-valued wire enums (Evergreen: unknown values are preserved)
// =============================================================================

/// Generates a proto-JSON string enum with an `Unknown` variant that
/// preserves unrecognized wire values, plus the crate-standard helper
/// methods (`is_unknown`, `unknown_<context>_type`, `unknown_data`).
macro_rules! wire_string_enum {
    (
        $(#[$meta:meta])*
        $name:ident, $ctx:ident, $unknown_type_fn:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $wire:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// A wire value this crate does not recognize (Evergreen).
            Unknown {
                /// The unrecognized enum string from the harness.
                $ctx: String,
                /// The raw JSON value, preserved for roundtrip.
                data: Value,
            },
        }

        impl $name {
            /// Returns the proto-JSON wire string for this value.
            #[must_use]
            pub fn as_wire_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )+
                    Self::Unknown { $ctx, .. } => $ctx,
                }
            }

            /// Check if this is an unknown value.
            #[must_use]
            pub const fn is_unknown(&self) -> bool {
                matches!(self, Self::Unknown { .. })
            }

            /// Returns the unrecognized wire string if this is an unknown value.
            #[must_use]
            pub fn $unknown_type_fn(&self) -> Option<&str> {
                match self {
                    Self::Unknown { $ctx, .. } => Some($ctx),
                    _ => None,
                }
            }

            /// Returns the preserved raw JSON if this is an unknown value.
            #[must_use]
            pub fn unknown_data(&self) -> Option<&Value> {
                match self {
                    Self::Unknown { data, .. } => Some(data),
                    _ => None,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_wire_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = Value::deserialize(deserializer)?;
                if let Value::String(s) = &value {
                    match s.as_str() {
                        $( $wire => return Ok(Self::$variant), )+
                        _ => {}
                    }
                }
                let $ctx = match &value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                tracing::warn!(
                    concat!("Unknown ", stringify!($name), " wire value: '{}'. \
                     Preserving in Unknown variant."),
                    $ctx
                );
                Ok(Self::Unknown { $ctx, data: value })
            }
        }
    };
}

wire_string_enum!(
    /// `StepUpdate.State` — lifecycle state of one agent step.
    StepState, state_type, unknown_state_type {
        /// `STATE_UNSPECIFIED`.
        Unspecified => "STATE_UNSPECIFIED",
        /// The step is executing.
        Active => "STATE_ACTIVE",
        /// The step completed successfully.
        Done => "STATE_DONE",
        /// The step is blocked on client input (confirmation or questions).
        WaitingForUser => "STATE_WAITING_FOR_USER",
        /// The step failed.
        Error => "STATE_ERROR",
    }
);

wire_string_enum!(
    /// `StepUpdate.Source` — who produced a step.
    StepSource, source_type, unknown_source_type {
        /// `SOURCE_UNSPECIFIED`.
        Unspecified => "SOURCE_UNSPECIFIED",
        /// Emitted by the platform (e.g. system errors).
        System => "SOURCE_SYSTEM",
        /// Emitted on behalf of the user.
        User => "SOURCE_USER",
        /// Emitted by the model.
        Model => "SOURCE_MODEL",
    }
);

wire_string_enum!(
    /// `StepUpdate.Target` — who a step is directed at.
    StepTarget, target_type, unknown_target_type {
        /// `TARGET_UNSPECIFIED`.
        Unspecified => "TARGET_UNSPECIFIED",
        /// Directed at the user (e.g. final response text).
        User => "TARGET_USER",
        /// Directed at the model.
        Model => "TARGET_MODEL",
        /// Directed at the environment (e.g. tool executions).
        Environment => "TARGET_ENVIRONMENT",
    }
);

wire_string_enum!(
    /// `TrajectoryStateUpdate.State` — lifecycle state of one trajectory.
    TrajectoryState, state_type, unknown_state_type {
        /// `STATE_UNSPECIFIED`.
        Unspecified => "STATE_UNSPECIFIED",
        /// The trajectory is processing a turn.
        Running => "STATE_RUNNING",
        /// The trajectory finished the turn and is awaiting input.
        Idle => "STATE_IDLE",
        /// The turn was cancelled (halt request or pre-turn hook denial).
        Cancelled => "STATE_CANCELLED",
    }
);

wire_string_enum!(
    /// `ModelType` — the roles a configured model can serve.
    ModelType, model_type, unknown_model_type {
        /// `MODEL_TYPE_UNSPECIFIED`.
        Unspecified => "MODEL_TYPE_UNSPECIFIED",
        /// Text generation model.
        Text => "MODEL_TYPE_TEXT",
        /// Image generation model.
        Image => "MODEL_TYPE_IMAGE",
    }
);

wire_string_enum!(
    /// `LifecycleHook` — hook points the harness can call back into.
    LifecycleHook, hook_type, unknown_hook_type {
        /// `LIFECYCLE_HOOK_UNSPECIFIED`.
        Unspecified => "LIFECYCLE_HOOK_UNSPECIFIED",
        /// Fired when the session starts.
        OnSessionStart => "LIFECYCLE_HOOK_ON_SESSION_START",
        /// Fired when the session ends.
        OnSessionEnd => "LIFECYCLE_HOOK_ON_SESSION_END",
        /// Fired before each user turn.
        PreTurn => "LIFECYCLE_HOOK_PRE_TURN",
        /// Fired after each user turn.
        PostTurn => "LIFECYCLE_HOOK_POST_TURN",
        /// Fired before each tool call (may deny it).
        PreTool => "LIFECYCLE_HOOK_PRE_TOOL",
        /// Fired after each successful tool call.
        PostTool => "LIFECYCLE_HOOK_POST_TOOL",
        /// Fired when a tool call errors.
        OnToolError => "LIFECYCLE_HOOK_ON_TOOL_ERROR",
    }
);

wire_string_enum!(
    /// `PreToolResult.Decision` / `PreTurnResult.Decision` — hook verdicts.
    HookDecision, decision_type, unknown_decision_type {
        /// `DECISION_UNSPECIFIED`.
        Unspecified => "DECISION_UNSPECIFIED",
        /// Allow the operation.
        Allow => "ALLOW",
        /// Deny the operation.
        Deny => "DENY",
    }
);

wire_string_enum!(
    /// `ActionEditFile.DiffLine.LineAction` — per-line diff operation.
    LineAction, action_type, unknown_action_type {
        /// `LINE_ACTION_UNSPECIFIED`.
        Unspecified => "LINE_ACTION_UNSPECIFIED",
        /// The line was inserted.
        Insert => "LINE_ACTION_INSERT",
        /// The line was deleted.
        Delete => "LINE_ACTION_DELETE",
        /// The line is unchanged context.
        None => "LINE_ACTION_NONE",
    }
);

// =============================================================================
// Harness configuration (client -> harness, sent once at init)
// =============================================================================

/// The first WebSocket message: `InitializeConversationEvent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeConversationEvent {
    /// The full agent configuration for this conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<HarnessConfig>,
}

/// `HarnessConfig` — everything the harness needs to run the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HarnessConfig {
    /// Conversation id to resume, when restoring a saved session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cascade_id: Option<String>,
    /// System instructions for the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instructions: Option<SystemInstructions>,
    /// Custom (client-executed) tool declarations.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<Tool>,
    /// Per-builtin enable flags for harness-executed tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_side_tools: Option<HarnessSideTools>,
    /// History compaction threshold in tokens (`0` = harness default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_threshold: Option<u32>,
    /// Workspace directories the agent may operate in.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub workspaces: Vec<Workspace>,
    /// Paths to search for agent skills.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skills_paths: Vec<String>,
    /// JSON schema (as a JSON string) for structured final output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_tool_schema_json: Option<String>,
    /// Serialized trajectory to resume from (base64 bytes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_trajectory: Option<String>,
    /// Directory for harness application data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_data_dir: Option<String>,
    /// MCP servers the harness should connect to.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mcp_servers: Vec<McpServerConfig>,
    /// Model configurations. The harness requires at least one text model.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub models: Vec<ModelConfig>,
    /// Lifecycle hooks the client wants callbacks for.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub enabled_hooks: Vec<LifecycleHook>,
    /// Static subagent configurations.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub custom_subagents: Vec<CustomAgent>,
}

/// `SystemInstructions` (oneof `type`): custom or appended instructions.
///
/// Modeled as a struct of options (proto-JSON sets at most one field).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemInstructions {
    /// Fully custom instructions, replacing the harness's defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<CustomSystemInstructions>,
    /// Sections appended to the harness's default instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appended: Option<AppendedSystemInstructions>,
}

impl SystemInstructions {
    /// Custom instructions consisting of a single text part.
    #[must_use]
    pub fn custom_text(text: impl Into<String>) -> Self {
        Self {
            custom: Some(CustomSystemInstructions {
                part: vec![SystemInstructionPart {
                    text: Some(text.into()),
                }],
            }),
            appended: None,
        }
    }
}

/// `CustomSystemInstructions` — a list of instruction parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CustomSystemInstructions {
    /// Instruction parts. Note the singular wire name (`part`) — the proto
    /// field is a *repeated message* named `part`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub part: Vec<SystemInstructionPart>,
}

/// `CustomSystemInstructions.Part` (oneof `part`, currently text-only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemInstructionPart {
    /// Text content of this part.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// `AppendedSystemInstructions` — identity plus appended sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppendedSystemInstructions {
    /// Custom identity line for the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_identity: Option<String>,
    /// Titled sections appended after the default instructions.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub appended_sections: Vec<InstructionSection>,
}

/// `AppendedSystemInstructions.Section`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSection {
    /// Section title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Section body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// `Tool` — a custom, client-executed tool declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Tool name (what the model calls).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Human/model-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parameter JSON schema, serialized as a JSON *string*.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters_json_schema: Option<String>,
    /// Response JSON schema, serialized as a JSON *string*.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<String>,
}

/// Enable/disable flag for one harness-side tool.
///
/// Several `*ToolConfig` messages exist on the wire; this crate writes only
/// their common `enabled` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolToggle {
    /// Whether the tool is available to the agent.
    pub enabled: bool,
}

impl ToolToggle {
    /// Convenience constructor.
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

/// `HarnessSideTools` — per-builtin enable flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSideTools {
    /// `find_file` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub find: Option<ToolToggle>,
    /// `run_command` builtin (shell access).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<ToolToggle>,
    /// `start_subagent` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<ToolToggle>,
    /// `ask_question` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_questions: Option<ToolToggle>,
    /// `edit_file` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_edit: Option<ToolToggle>,
    /// `view_file` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_file: Option<ToolToggle>,
    /// `create_file` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_to_file: Option<ToolToggle>,
    /// `search_directory` (grep) builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grep_search: Option<ToolToggle>,
    /// `list_directory` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_dir: Option<ToolToggle>,
    /// Workspace path validation enforcement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionsConfig>,
    /// `generate_image` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_image: Option<ToolToggle>,
    /// `search_web` builtin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_web: Option<ToolToggle>,
}

/// `PermissionsConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsConfig {
    /// Reject file operations targeting paths outside configured workspaces.
    pub enforce_workspace_validation: bool,
}

/// `Workspace` (oneof `workspace_type`, currently filesystem-only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    /// A directory on the local filesystem.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem_workspace: Option<FilesystemWorkspace>,
}

impl Workspace {
    /// A filesystem workspace rooted at `directory`.
    #[must_use]
    pub fn filesystem(directory: impl Into<String>) -> Self {
        Self {
            filesystem_workspace: Some(FilesystemWorkspace {
                directory: Some(directory.into()),
            }),
        }
    }
}

/// `FilesystemWorkspace`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemWorkspace {
    /// Absolute directory path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

/// `ModelConfig` — one model the harness may call.
///
/// The endpoint fields form a oneof (`endpoint`): set at most one of
/// `gemini_api_endpoint` / `vertex_endpoint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    /// Model name, e.g. `gemini-3-flash-preview`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Roles this model serves ([`ModelType::Text`] is required for chat).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub types: Vec<ModelType>,
    /// Gemini API endpoint (API-key auth).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_api_endpoint: Option<GeminiApiEndpoint>,
    /// Vertex AI endpoint (project/location auth). Present for wire
    /// completeness; the tested path is the Gemini API endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_endpoint: Option<VertexEndpoint>,
}

/// `GeminiAPIEndpoint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiApiEndpoint {
    /// Override for the Gemini API base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Extra HTTP headers sent with model requests.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub http_headers: BTreeMap<String, String>,
    /// Gemini API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Model options (thinking level).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GeminiModelOptions>,
}

/// `VertexEndpoint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VertexEndpoint {
    /// Override for the Vertex AI base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Extra HTTP headers sent with model requests.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub http_headers: BTreeMap<String, String>,
    /// Google Cloud project id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// Google Cloud location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Model options (thinking level).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GeminiModelOptions>,
}

/// `GeminiModelOptions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiModelOptions {
    /// Thinking level (e.g. `"low"`, `"high"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// `McpServerConfig` — one MCP server the harness connects to.
///
/// `stdio` / `http` form a oneof (`transport`): set at most one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// Server name; also the prefix in policy targets (`mcp_<name>_<tool>`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Stdio transport: the harness spawns the server as a subprocess.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdio: Option<McpStdioTransport>,
    /// Streamable-HTTP transport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<McpHttpTransport>,
    /// Allow-list of tool names (empty = all).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub enabled_tools: Vec<String>,
    /// Deny-list of tool names.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub disabled_tools: Vec<String>,
    /// Per-call timeout in seconds (`0` = harness default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<i32>,
}

/// `McpStdioTransport`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpStdioTransport {
    /// Executable to spawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Command-line arguments.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub args: Vec<String>,
    /// Environment variables for the subprocess.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub env: BTreeMap<String, String>,
}

/// `McpHttpTransport`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpTransport {
    /// Server URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Extra HTTP headers.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub headers: BTreeMap<String, String>,
}

/// `CustomAgent` — a static subagent configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CustomAgent {
    /// Subagent name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Subagent description (shown to the parent model).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Subagent system instructions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instructions: Option<SystemInstructions>,
    /// Subagent builtin-tool flags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_side_tools: Option<HarnessSideTools>,
    /// Custom tools available to the subagent (must also be registered on
    /// the main agent).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<Tool>,
}

// =============================================================================
// Client -> harness events (InputEvent oneof)
// =============================================================================

/// `InputEvent` — the client-to-harness message envelope (oneof `event`).
///
/// Serializes to a single-key proto-JSON object, e.g.
/// `{"userInput": "hello"}` or `{"toolResponse": {...}}`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum InputEvent {
    /// Plain-text user message; starts a new turn.
    UserInput(String),
    /// Multi-part user message (text, media, slash commands).
    ComplexUserInput(UserInput),
    /// Approve/reject a harness-side tool awaiting confirmation.
    ToolConfirmation(ToolConfirmation),
    /// Result of a client-executed custom tool call.
    ToolResponse(ToolResponse),
    /// Answers to a `questions_request`.
    QuestionResponse(UserQuestionsResponse),
    /// Cancel the current turn.
    HaltRequest(bool),
    /// Message injected by a client-side trigger.
    AutomatedTrigger(String),
    /// Reply to a `call_hook_request`.
    CallHookResponse(CallHookResponse),
    /// Ask the harness to run session-end hooks.
    SessionEndRequest(bool),
    /// An event variant this crate does not recognize (Evergreen).
    Unknown {
        /// The unrecognized oneof field name.
        event_type: String,
        /// The raw JSON payload, preserved for roundtrip.
        data: Value,
    },
}

impl InputEvent {
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

    fn oneof_key(&self) -> &str {
        match self {
            Self::UserInput(_) => "userInput",
            Self::ComplexUserInput(_) => "complexUserInput",
            Self::ToolConfirmation(_) => "toolConfirmation",
            Self::ToolResponse(_) => "toolResponse",
            Self::QuestionResponse(_) => "questionResponse",
            Self::HaltRequest(_) => "haltRequest",
            Self::AutomatedTrigger(_) => "automatedTrigger",
            Self::CallHookResponse(_) => "callHookResponse",
            Self::SessionEndRequest(_) => "sessionEndRequest",
            Self::Unknown { event_type, .. } => event_type,
        }
    }

    fn oneof_value(&self) -> Result<Value, serde_json::Error> {
        match self {
            Self::UserInput(s) | Self::AutomatedTrigger(s) => Ok(Value::String(s.clone())),
            Self::ComplexUserInput(v) => serde_json::to_value(v),
            Self::ToolConfirmation(v) => serde_json::to_value(v),
            Self::ToolResponse(v) => serde_json::to_value(v),
            Self::QuestionResponse(v) => serde_json::to_value(v),
            Self::HaltRequest(b) | Self::SessionEndRequest(b) => Ok(Value::Bool(*b)),
            Self::CallHookResponse(v) => serde_json::to_value(v),
            Self::Unknown { data, .. } => Ok(data.clone()),
        }
    }
}

impl Serialize for InputEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let value = self.oneof_value().map_err(serde::ser::Error::custom)?;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.oneof_key(), &value)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for InputEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let map = Map::deserialize(deserializer)?;
        let (key, value) = map
            .into_iter()
            .next()
            .ok_or_else(|| D::Error::custom("InputEvent must have exactly one field set"))?;
        let event = match key.as_str() {
            "userInput" => Self::UserInput(
                value
                    .as_str()
                    .ok_or_else(|| D::Error::custom("userInput must be a string"))?
                    .to_string(),
            ),
            "complexUserInput" => {
                Self::ComplexUserInput(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "toolConfirmation" => {
                Self::ToolConfirmation(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "toolResponse" => {
                Self::ToolResponse(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "questionResponse" => {
                Self::QuestionResponse(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "haltRequest" => Self::HaltRequest(value.as_bool().unwrap_or_default()),
            "automatedTrigger" => Self::AutomatedTrigger(
                value
                    .as_str()
                    .ok_or_else(|| D::Error::custom("automatedTrigger must be a string"))?
                    .to_string(),
            ),
            "callHookResponse" => {
                Self::CallHookResponse(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "sessionEndRequest" => Self::SessionEndRequest(value.as_bool().unwrap_or_default()),
            _ => Self::Unknown {
                event_type: key,
                data: value,
            },
        };
        Ok(event)
    }
}

/// `UserInput` — multi-part user content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserInput {
    /// The content parts, in order.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parts: Vec<UserInputPart>,
}

/// `UserInput.Part` (oneof `part`): text, media, or a slash command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserInputPart {
    /// Plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Inline media (image, document, audio, video).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    /// A named slash command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slash_command: Option<SlashCommand>,
}

impl UserInputPart {
    /// A text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }
}

/// `Media` — inline binary content (proto-JSON encodes `data` as base64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// MIME type of the data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Base64-encoded content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// `UserInput.SlashCommand`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    /// Command name (without the slash).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `ToolConfirmation` — approve or reject a pending harness-side tool step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfirmation {
    /// Trajectory of the waiting step.
    pub trajectory_id: String,
    /// Index of the waiting step.
    pub step_index: u32,
    /// `true` to run the tool, `false` to reject it.
    pub accepted: bool,
}

/// `ToolResponse` — the result of a client-executed custom tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolResponse {
    /// Correlates with [`ToolCall::id`].
    pub id: String,
    /// The result, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json: Option<String>,
    /// Media attachments accompanying the result.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub supplemental_media: Vec<Media>,
}

/// `UserQuestionsResponse` — answers to a `questions_request`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionsResponse {
    /// Trajectory of the asking step.
    pub trajectory_id: String,
    /// Index of the asking step.
    pub step_index: u32,
    /// Set to cancel the questions instead of answering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
    /// The answers (oneof with `cancelled`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<QuestionsResponse>,
}

/// `UserQuestionsResponse.QuestionsResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuestionsResponse {
    /// One answer per question, in order.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub answers: Vec<UserQuestionAnswer>,
}

/// `UserQuestionAnswer` (oneof `answer`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionAnswer {
    /// The question was left unanswered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unanswered: Option<bool>,
    /// A multiple-choice answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple_choice_answer: Option<MultipleChoiceAnswer>,
}

impl UserQuestionAnswer {
    /// An explicit "unanswered" answer.
    #[must_use]
    pub fn unanswered() -> Self {
        Self {
            unanswered: Some(true),
            multiple_choice_answer: None,
        }
    }
}

/// `MultipleChoiceAnswer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MultipleChoiceAnswer {
    /// Zero-based indices of the selected choices.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub selected_choice_indices: Vec<i32>,
    /// Optional freeform text response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeform_response: Option<String>,
}

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
}

/// `EmptyResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmptyResult {}

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
        let usage_metadata = match map.remove("usageMetadata") {
            None | Some(Value::Null) => None,
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
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `StepUpdate` — progress on one agent step.
///
/// The per-tool action fields (`run_command`, `edit_file`, ...) are plain
/// optional fields on the wire (not a oneof); at most one is set in
/// practice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StepUpdate {
    /// Conversation id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cascade_id: Option<String>,
    /// Trajectory this step belongs to (subagents get their own).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    /// Step index within the trajectory.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub step_index: Option<u32>,
    /// Lifecycle state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<StepState>,
    /// Who produced this step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<StepSource>,
    /// Who this step is directed at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<StepTarget>,
    /// Error description when `state` is `Error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Accumulated thinking text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Incremental response text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_delta: Option<String>,
    /// Incremental thinking text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_delta: Option<String>,
    /// Accumulated response text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// `list_directory` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_directory: Option<ActionListDirectory>,
    /// `find_file` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub find_file: Option<ActionFindFile>,
    /// `search_directory` (grep) action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_directory: Option<ActionSearchDirectory>,
    /// `view_file` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_file: Option<ActionViewFile>,
    /// `create_file` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_file: Option<ActionCreateFile>,
    /// `edit_file` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_file: Option<ActionEditFile>,
    /// `run_command` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<ActionRunCommand>,
    /// History compaction marker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<ActionCompaction>,
    /// Subagent invocation marker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoke_subagent: Option<ActionInvokeSubagent>,
    /// `generate_image` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_image: Option<ActionGenerateImage>,
    /// Finish marker with structured output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<ActionFinish>,
    /// Step-level error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ActionError>,
    /// MCP tool invocation details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_tool: Option<ActionMcpTool>,
    /// `search_web` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_web: Option<ActionSearchWeb>,
    /// Free-text description of a pending request (confirmation prompts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_text: Option<String>,
    /// Present while the step waits for a tool confirmation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_confirmation_request: Option<ToolConfirmationRequest>,
    /// Present while the step waits for question answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub questions_request: Option<UserQuestionsRequest>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ToolConfirmationRequest` — currently an empty marker message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfirmationRequest {
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `UserQuestionsRequest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionsRequest {
    /// The questions to answer.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub questions: Vec<UserQuestion>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `UserQuestion` (oneof `question_type`, currently multiple-choice only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestion {
    /// A multiple-choice question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple_choice: Option<MultipleChoice>,
    /// Unrecognized fields (including future question types), preserved
    /// for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `MultipleChoice`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MultipleChoice {
    /// The question text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// The choices.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub choices: Vec<String>,
    /// Whether multiple choices may be selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_multi_select: Option<bool>,
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
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ToolCall` — the model invoking a client-executed custom tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// Correlation id; echo it in the [`ToolResponse`].
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
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `PreToolArgs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreToolArgs {
    /// The tool about to run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Its arguments, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_json: Option<String>,
}

/// `PostToolArgs`.
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

// =============================================================================
// StepUpdate action submessages
// =============================================================================

/// `ActionListDirectory`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionListDirectory {
    /// The listed directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_path: Option<String>,
    /// The entries (populated when the step completes).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub results: Vec<ListDirectoryEntry>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionListDirectory.Result` (oneof `info`: directory flag or file size).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListDirectoryEntry {
    /// Entry name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Set when the entry is a directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_directory: Option<bool>,
    /// Set (byte size) when the entry is a file.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u64",
        skip_serializing_if = "Option::is_none"
    )]
    pub file_size: Option<u64>,
}

/// `ActionFindFile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionFindFile {
    /// The searched directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_path: Option<String>,
    /// The filename query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Raw find output (populated when the step completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionSearchDirectory`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionSearchDirectory {
    /// The searched directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory_path: Option<String>,
    /// The grep query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Result count (populated when the step completes).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub num_results: Option<i32>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionViewFile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionViewFile {
    /// The viewed file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// First viewed line.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub start_line: Option<u32>,
    /// Last viewed line.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub end_line: Option<u32>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionCreateFile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionCreateFile {
    /// The created file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Its contents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionEditFile`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionEditFile {
    /// The edited file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// The applied diff blocks.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diff_block: Vec<DiffBlock>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionEditFile.DiffBlock`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffBlock {
    /// First affected line.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub start_line: Option<i32>,
    /// Last affected line.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub end_line: Option<i32>,
    /// The diff lines.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub lines: Vec<DiffLine>,
}

/// `ActionEditFile.DiffLine`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// The line text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What happened to the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<LineAction>,
}

/// `ActionRunCommand`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionRunCommand {
    /// The shell command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_line: Option<String>,
    /// The working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Exit code (populated when the step completes).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub exit_code: Option<i32>,
    /// Combined stdout/stderr (populated when the step completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combined_output: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionCompaction` — history compaction marker (no fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionCompaction {
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionInvokeSubagent` — subagent invocation marker (no fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionInvokeSubagent {
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionGenerateImage`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionGenerateImage {
    /// The image prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Input image paths (for edits).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub image_paths: Vec<String>,
    /// Output image name (populated when the step completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_name: Option<String>,
    /// Requested aspect ratio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionFinish` — the agent finished with structured output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionFinish {
    /// The structured output, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_string: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionError {
    /// The error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// HTTP status code of the underlying model call, when applicable.
    #[serde(
        default,
        deserialize_with = "flex_num::opt_u32",
        skip_serializing_if = "Option::is_none"
    )]
    pub http_code: Option<u32>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionMcpTool`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionMcpTool {
    /// The MCP server name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// The tool name on that server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Arguments, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments_json: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionSearchWeb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionSearchWeb {
    /// The search query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Restrict results to this domain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Result summary (populated when the step completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -------------------------------------------------------------------
    // Golden proto-JSON fixtures generated with the reference
    // implementation (google-antigravity 0.1.5, protobuf json_format).
    // -------------------------------------------------------------------

    #[test]
    fn test_input_event_user_input_golden() {
        let event = InputEvent::UserInput("hello".to_string());
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"userInput": "hello"})
        );
    }

    #[test]
    fn test_input_event_tool_response_golden() {
        let event = InputEvent::ToolResponse(ToolResponse {
            id: "1".to_string(),
            response_json: Some(r#"{"a":1}"#.to_string()),
            supplemental_media: vec![],
        });
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"toolResponse": {"id": "1", "responseJson": "{\"a\":1}"}})
        );
    }

    #[test]
    fn test_input_event_halt_request_golden() {
        let event = InputEvent::HaltRequest(true);
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"haltRequest": true})
        );
    }

    #[test]
    fn test_input_event_tool_confirmation_golden() {
        let event = InputEvent::ToolConfirmation(ToolConfirmation {
            trajectory_id: "t".to_string(),
            step_index: 2,
            accepted: true,
        });
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"toolConfirmation": {"trajectoryId": "t", "stepIndex": 2, "accepted": true}})
        );
    }

    #[test]
    fn test_input_event_call_hook_response_golden() {
        let event = InputEvent::CallHookResponse(CallHookResponse {
            request_id: "r".to_string(),
            pre_tool_result: Some(HookVerdict {
                decision: Some(HookDecision::Allow),
                reason: Some("ok".to_string()),
            }),
            ..Default::default()
        });
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"callHookResponse": {"requestId": "r", "preToolResult": {"decision": "ALLOW", "reason": "ok"}}})
        );
    }

    #[test]
    fn test_initialize_conversation_event_golden() {
        // Matches the reference SDK's json_format.MessageToJson output for
        // an equivalent HarnessConfig (field-by-field; ordering differs).
        let event = InitializeConversationEvent {
            config: Some(HarnessConfig {
                cascade_id: Some("cid".to_string()),
                system_instructions: Some(SystemInstructions::custom_text("hi")),
                tools: vec![Tool {
                    name: Some("t".to_string()),
                    description: Some("d".to_string()),
                    parameters_json_schema: Some("{}".to_string()),
                    response_json_schema: Some("{}".to_string()),
                }],
                harness_side_tools: Some(HarnessSideTools {
                    run_command: Some(ToolToggle::new(false)),
                    view_file: Some(ToolToggle::new(true)),
                    ..Default::default()
                }),
                workspaces: vec![Workspace::filesystem("/w")],
                mcp_servers: vec![McpServerConfig {
                    name: Some("git".to_string()),
                    stdio: Some(McpStdioTransport {
                        command: Some("uvx".to_string()),
                        args: vec!["x".to_string()],
                        env: BTreeMap::from([("K".to_string(), "V".to_string())]),
                    }),
                    ..Default::default()
                }],
                models: vec![ModelConfig {
                    name: Some("gemini-3-flash-preview".to_string()),
                    types: vec![ModelType::Text],
                    gemini_api_endpoint: Some(GeminiApiEndpoint {
                        api_key: Some("k".to_string()),
                        http_headers: BTreeMap::from([("a".to_string(), "b".to_string())]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                enabled_hooks: vec![LifecycleHook::PreTool],
                ..Default::default()
            }),
        };
        let expected = json!({"config": {
            "cascadeId": "cid",
            "systemInstructions": {"custom": {"part": [{"text": "hi"}]}},
            "tools": [{"name": "t", "description": "d", "parametersJsonSchema": "{}", "responseJsonSchema": "{}"}],
            "harnessSideTools": {"runCommand": {"enabled": false}, "viewFile": {"enabled": true}},
            "workspaces": [{"filesystemWorkspace": {"directory": "/w"}}],
            "mcpServers": [{"name": "git", "stdio": {"command": "uvx", "args": ["x"], "env": {"K": "V"}}}],
            "models": [{"name": "gemini-3-flash-preview", "types": ["MODEL_TYPE_TEXT"], "geminiApiEndpoint": {"httpHeaders": {"a": "b"}, "apiKey": "k"}}],
            "enabledHooks": ["LIFECYCLE_HOOK_PRE_TOOL"],
        }});
        assert_eq!(serde_json::to_value(&event).unwrap(), expected);
    }

    #[test]
    fn test_output_event_step_update_golden() {
        // Verbatim harness-side encoding: int64/uint64 as strings.
        let raw = r#"{"seqNum": "12345678901234", "timestampMicros": "2", "stepUpdate": {"trajectoryId": "traj", "stepIndex": 3, "state": "STATE_DONE", "source": "SOURCE_MODEL", "target": "TARGET_USER", "text": "hello", "runCommand": {"commandLine": "ls", "exitCode": 0, "combinedOutput": "a\n"}}, "usageMetadata": {"promptTokenCount": "10", "totalTokenCount": "20"}}"#;
        let event: OutputEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(event.seq_num, Some(12_345_678_901_234));
        assert_eq!(event.timestamp_micros, Some(2));
        let usage = event.usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, Some(10));
        assert_eq!(usage.total_token_count, Some(20));
        let Some(OutputPayload::StepUpdate(step)) = &event.payload else {
            panic!("expected StepUpdate, got {:?}", event.payload);
        };
        assert_eq!(step.trajectory_id.as_deref(), Some("traj"));
        assert_eq!(step.step_index, Some(3));
        assert_eq!(step.state, Some(StepState::Done));
        assert_eq!(step.source, Some(StepSource::Model));
        assert_eq!(step.target, Some(StepTarget::User));
        assert_eq!(step.text.as_deref(), Some("hello"));
        let run = step.run_command.as_ref().unwrap();
        assert_eq!(run.command_line.as_deref(), Some("ls"));
        assert_eq!(run.exit_code, Some(0));
        assert_eq!(run.combined_output.as_deref(), Some("a\n"));
    }

    #[test]
    fn test_output_event_tool_call_golden() {
        let raw = r#"{"seqNum": "1", "toolCall": {"id": "abc", "name": "get_weather", "argumentsJson": "{\"city\":\"SF\"}"}}"#;
        let event: OutputEvent = serde_json::from_str(raw).unwrap();
        let Some(OutputPayload::ToolCall(call)) = &event.payload else {
            panic!("expected ToolCall");
        };
        assert_eq!(call.id.as_deref(), Some("abc"));
        assert_eq!(call.name.as_deref(), Some("get_weather"));
        assert_eq!(call.arguments_json.as_deref(), Some(r#"{"city":"SF"}"#));
    }

    #[test]
    fn test_output_event_init_response_golden() {
        let raw = r#"{"initializeConversationResponse": {"cascadeId": "cid"}}"#;
        let event: OutputEvent = serde_json::from_str(raw).unwrap();
        let Some(OutputPayload::InitializeConversationResponse(resp)) = &event.payload else {
            panic!("expected InitializeConversationResponse");
        };
        assert_eq!(resp.cascade_id.as_deref(), Some("cid"));
        assert!(resp.history.is_empty());
    }

    // -------------------------------------------------------------------
    // Roundtrips and Evergreen preservation
    // -------------------------------------------------------------------

    fn roundtrip_output(event: &OutputEvent) -> OutputEvent {
        let json = serde_json::to_string(event).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    fn roundtrip_input(event: &InputEvent) -> InputEvent {
        let json = serde_json::to_string(event).unwrap();
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn test_input_event_roundtrip_all_variants() {
        let events = vec![
            InputEvent::UserInput("hi".to_string()),
            InputEvent::ComplexUserInput(UserInput {
                parts: vec![
                    UserInputPart::text("a"),
                    UserInputPart {
                        media: Some(Media {
                            mime_type: Some("image/png".to_string()),
                            description: Some("d".to_string()),
                            data: Some("aGVsbG8=".to_string()),
                        }),
                        ..Default::default()
                    },
                    UserInputPart {
                        slash_command: Some(SlashCommand {
                            name: Some("compact".to_string()),
                        }),
                        ..Default::default()
                    },
                ],
            }),
            InputEvent::ToolConfirmation(ToolConfirmation {
                trajectory_id: "t".to_string(),
                step_index: 7,
                accepted: false,
            }),
            InputEvent::ToolResponse(ToolResponse {
                id: "id".to_string(),
                response_json: Some("{}".to_string()),
                supplemental_media: vec![Media::default()],
            }),
            InputEvent::QuestionResponse(UserQuestionsResponse {
                trajectory_id: "t".to_string(),
                step_index: 1,
                cancelled: None,
                response: Some(QuestionsResponse {
                    answers: vec![
                        UserQuestionAnswer::unanswered(),
                        UserQuestionAnswer {
                            multiple_choice_answer: Some(MultipleChoiceAnswer {
                                selected_choice_indices: vec![0, 2],
                                freeform_response: Some("f".to_string()),
                            }),
                            ..Default::default()
                        },
                    ],
                }),
            }),
            InputEvent::HaltRequest(true),
            InputEvent::AutomatedTrigger("tick".to_string()),
            InputEvent::CallHookResponse(CallHookResponse {
                request_id: "r".to_string(),
                empty_result: Some(EmptyResult {}),
                ..Default::default()
            }),
            InputEvent::SessionEndRequest(true),
            InputEvent::Unknown {
                event_type: "futureEvent".to_string(),
                data: json!({"x": 1}),
            },
        ];
        for event in events {
            assert_eq!(roundtrip_input(&event), event);
        }
    }

    #[test]
    fn test_input_event_unknown_variant_preserves_wire_format() {
        let raw = r#"{"futureEvent": {"payload": 42}}"#;
        let event: InputEvent = serde_json::from_str(raw).unwrap();
        assert!(event.is_unknown());
        assert_eq!(event.unknown_event_type(), Some("futureEvent"));
        assert_eq!(event.unknown_data(), Some(&json!({"payload": 42})));
        // Roundtrips back to the original single-key object.
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"futureEvent": {"payload": 42}})
        );
    }

    #[test]
    fn test_output_event_roundtrip_all_variants() {
        let events = vec![
            OutputEvent {
                seq_num: Some(1),
                timestamp_micros: Some(2),
                usage_metadata: Some(UsageMetadata {
                    prompt_token_count: Some(1),
                    total_token_count: Some(2),
                    ..Default::default()
                }),
                payload: Some(OutputPayload::StepUpdate(Box::new(StepUpdate {
                    trajectory_id: Some("t".to_string()),
                    step_index: Some(1),
                    state: Some(StepState::Active),
                    text_delta: Some("d".to_string()),
                    ..Default::default()
                }))),
            },
            OutputEvent {
                payload: Some(OutputPayload::TrajectoryStateUpdate(
                    TrajectoryStateUpdate {
                        trajectory_id: Some("t".to_string()),
                        state: Some(TrajectoryState::Idle),
                        error: None,
                        extra: Map::new(),
                    },
                )),
                ..Default::default()
            },
            OutputEvent {
                payload: Some(OutputPayload::ToolCall(ToolCall {
                    id: Some("i".to_string()),
                    name: Some("n".to_string()),
                    arguments_json: Some("{}".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            },
            OutputEvent {
                payload: Some(OutputPayload::InitializeConversationResponse(
                    InitializeConversationResponse {
                        cascade_id: Some("c".to_string()),
                        history: vec![StepUpdate::default()],
                        extra: Map::new(),
                    },
                )),
                ..Default::default()
            },
            OutputEvent {
                payload: Some(OutputPayload::CallHookRequest(CallHookRequest {
                    request_id: Some("r".to_string()),
                    hook_type: Some(LifecycleHook::PreTool),
                    pre_tool_args: Some(PreToolArgs {
                        tool_name: Some("run_command".to_string()),
                        arguments_json: Some("{}".to_string()),
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            },
            OutputEvent {
                payload: Some(OutputPayload::SessionEndResponse(true)),
                ..Default::default()
            },
            OutputEvent {
                payload: Some(OutputPayload::Unknown {
                    event_type: "newThing".to_string(),
                    data: json!({"a": [1, 2]}),
                }),
                ..Default::default()
            },
            OutputEvent::default(),
        ];
        for event in events {
            assert_eq!(roundtrip_output(&event), event);
        }
    }

    #[test]
    fn test_output_event_unknown_variant_preserved() {
        let raw = r#"{"seqNum": "5", "brandNewEvent": {"k": "v"}}"#;
        let event: OutputEvent = serde_json::from_str(raw).unwrap();
        assert_eq!(event.seq_num, Some(5));
        let payload = event.payload.as_ref().unwrap();
        assert!(payload.is_unknown());
        assert_eq!(payload.unknown_event_type(), Some("brandNewEvent"));
        assert_eq!(payload.unknown_data(), Some(&json!({"k": "v"})));
        // Reserialization preserves the unknown payload.
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["brandNewEvent"], json!({"k": "v"}));
        assert_eq!(json["seqNum"], json!(5));
    }

    #[test]
    fn test_step_update_unknown_fields_preserved() {
        let raw = r#"{"trajectoryId": "t", "futureField": {"nested": true}}"#;
        let step: StepUpdate = serde_json::from_str(raw).unwrap();
        assert_eq!(
            step.extra.get("futureField"),
            Some(&json!({"nested": true}))
        );
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["futureField"], json!({"nested": true}));
    }

    #[test]
    fn test_wire_enum_unknown_value_preserved() {
        let state: StepState = serde_json::from_value(json!("STATE_HIBERNATING")).unwrap();
        assert!(state.is_unknown());
        assert_eq!(state.unknown_state_type(), Some("STATE_HIBERNATING"));
        assert_eq!(state.unknown_data(), Some(&json!("STATE_HIBERNATING")));
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            json!("STATE_HIBERNATING")
        );
    }

    #[test]
    fn test_wire_enum_known_values_roundtrip() {
        for (value, wire) in [
            (StepState::Unspecified, "STATE_UNSPECIFIED"),
            (StepState::Active, "STATE_ACTIVE"),
            (StepState::Done, "STATE_DONE"),
            (StepState::WaitingForUser, "STATE_WAITING_FOR_USER"),
            (StepState::Error, "STATE_ERROR"),
        ] {
            assert_eq!(serde_json::to_value(&value).unwrap(), json!(wire));
            let parsed: StepState = serde_json::from_value(json!(wire)).unwrap();
            assert_eq!(parsed, value);
            assert!(!parsed.is_unknown());
            assert_eq!(parsed.as_wire_str(), wire);
        }
    }

    #[test]
    fn test_usage_metadata_accepts_numbers_and_strings() {
        let from_strings: UsageMetadata =
            serde_json::from_value(json!({"promptTokenCount": "7", "totalTokenCount": "9"}))
                .unwrap();
        let from_numbers: UsageMetadata =
            serde_json::from_value(json!({"promptTokenCount": 7, "totalTokenCount": 9})).unwrap();
        assert_eq!(from_strings.prompt_token_count, Some(7));
        assert_eq!(from_strings, from_numbers);
    }

    #[test]
    fn test_questions_request_deserializes() {
        let raw = r#"{"questions": [{"multipleChoice": {"question": "q?", "choices": ["a", "b"], "isMultiSelect": true}}, {"holographicQuestion": {"q": 1}}]}"#;
        let req: UserQuestionsRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.questions.len(), 2);
        let mc = req.questions[0].multiple_choice.as_ref().unwrap();
        assert_eq!(mc.question.as_deref(), Some("q?"));
        assert_eq!(mc.choices, vec!["a", "b"]);
        assert_eq!(mc.is_multi_select, Some(true));
        // Unknown question type preserved via extra.
        assert!(req.questions[1].multiple_choice.is_none());
        assert!(req.questions[1].extra.contains_key("holographicQuestion"));
    }

    #[test]
    fn test_edit_file_diff_structure() {
        let raw = r#"{"filePath": "/f", "diffBlock": [{"startLine": 1, "endLine": 2, "lines": [{"text": "x", "action": "LINE_ACTION_INSERT"}]}]}"#;
        let action: ActionEditFile = serde_json::from_str(raw).unwrap();
        assert_eq!(action.file_path.as_deref(), Some("/f"));
        assert_eq!(
            action.diff_block[0].lines[0].action,
            Some(LineAction::Insert)
        );
    }

    #[test]
    fn test_tool_call_arguments_struct_preserved() {
        let raw = r#"{"id": "1", "name": "n", "arguments": {"fields": [{"name": "city", "value": {"stringValue": "SF"}}]}}"#;
        let call: ToolCall = serde_json::from_str(raw).unwrap();
        assert!(call.arguments.is_some());
        let json = serde_json::to_value(&call).unwrap();
        assert_eq!(json["arguments"]["fields"][0]["name"], "city");
    }

    #[test]
    fn test_flex_num_rejects_garbage() {
        let result: Result<UsageMetadata, _> =
            serde_json::from_value(json!({"promptTokenCount": "not-a-number"}));
        assert!(result.is_err());
        let result: Result<UsageMetadata, _> =
            serde_json::from_value(json!({"promptTokenCount": true}));
        assert!(result.is_err());
    }

    #[test]
    fn test_flex_num_null_is_none() {
        let usage: UsageMetadata =
            serde_json::from_value(json!({"promptTokenCount": null})).unwrap();
        assert_eq!(usage.prompt_token_count, None);
    }

    #[test]
    fn test_output_event_seq_num_string_and_number() {
        let a: OutputEvent = serde_json::from_str(r#"{"seqNum": "3"}"#).unwrap();
        let b: OutputEvent = serde_json::from_str(r#"{"seqNum": 3}"#).unwrap();
        assert_eq!(a.seq_num, Some(3));
        assert_eq!(a, b);
    }
}
