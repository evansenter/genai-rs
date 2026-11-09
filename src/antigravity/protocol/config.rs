use super::{
    AgentBehavior, AutoPolicyConfig, BudgetConfig, CompactionConfig, LifecycleHook, ModelType,
    PolicyRule, RetryConfig, RunCommandToolConfig, SessionContinuationMode, SkillsConfig,
    SubagentSkillsConfig, SubagentsConfig, ToolOutputTruncation, WorkspaceContainment,
};
use crate::redact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// How the harness frames the task (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_behavior: Option<AgentBehavior>,
    /// Harness-side policy. Its presence alone turns on workspace
    /// containment: the harness confines file tools to `workspaces` only
    /// when a policy config is sent (verified on harness 0.1.18 and
    /// 0.1.23).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_config: Option<PolicyConfig>,
    /// Cuts long tool output to a token budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output_truncation: Option<ToolOutputTruncation>,
    /// Whether `cascade_id` must exist, must not, or either.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_continuation_mode: Option<SessionContinuationMode>,
    /// Model-call and model-output retries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_config: Option<RetryConfig>,
    /// Session caps on model calls, tool calls and tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_config: Option<BudgetConfig>,
    /// History compaction (`compaction_threshold` is its legacy spelling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction_config: Option<CompactionConfig>,
    /// Inline skills.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_config: Option<SkillsConfig>,
    /// Size past which an MCP tool's output is written to a file instead of
    /// the model context (`0` = the harness default, 4096 bytes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_max_output_bytes: Option<i32>,
}

/// `PolicyConfig` — the harness-side policy evaluator's configuration.
///
/// This crate sends it for workspace containment and for the auto policy;
/// its own tool policies run client-side through the pre-tool hook (D-022).
/// With `rules` present the evaluator fails closed on a call no rule
/// matches (verified on 0.1.23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PolicyConfig {
    /// Harness-evaluated rules.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rules: Vec<PolicyRule>,
    /// Whether file tools are confined to the workspaces (unset: contained).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_containment: Option<WorkspaceContainment>,
    /// The model-judged safety assessment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_config: Option<AutoPolicyConfig>,
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
                    title: None,
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
    /// Prompt-section title: the harness renders the text under this tag
    /// instead of `<System>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    /// `run_command` builtin (shell access) and its settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_command: Option<RunCommandToolConfig>,
    /// The subagent builtins (`invoke_subagent` and its companions) and
    /// their settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<SubagentsConfig>,
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
    /// `read_url_content` builtin (fetches a URL).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_url_content: Option<ToolToggle>,
    /// Deferred tool loading via a tool-search builtin. Never written by
    /// this crate; modeled so a config roundtrips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_search_config: Option<ToolToggle>,
    /// `manage_task` builtin — lists and kills background tasks started by
    /// `run_command` or `schedule` (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manage_task: Option<ToolToggle>,
    /// `schedule` builtin — the agent schedules its own future turns as a
    /// timer or cron job (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ToolToggle>,
    /// `run_workflow` builtin, which also brings four skill tools (new in
    /// harness 0.1.23; earlier harnesses reject the field at init).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_workflow: Option<ToolToggle>,
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
    /// Model name, e.g. the crate's [`DEFAULT_MODEL`](crate::DEFAULT_MODEL).
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
///
/// `Debug` redacts the API key and the header values, keeping the header
/// names.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
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
    /// Model options (thinking level, service tier, output cap).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GeminiModelOptions>,
}

/// `VertexEndpoint`.
///
/// `Debug` redacts the header values, keeping their names.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
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

impl std::fmt::Debug for GeminiApiEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            base_url,
            http_headers,
            api_key,
            options,
        } = self;
        f.debug_struct("GeminiApiEndpoint")
            .field("base_url", base_url)
            .field("http_headers", &redact::values(http_headers))
            .field("api_key", &redact::optional(api_key.as_ref()))
            .field("options", options)
            .finish()
    }
}

impl std::fmt::Debug for VertexEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            base_url,
            http_headers,
            project,
            location,
            options,
        } = self;
        f.debug_struct("VertexEndpoint")
            .field("base_url", base_url)
            .field("http_headers", &redact::values(http_headers))
            .field("project", project)
            .field("location", location)
            .field("options", options)
            .finish()
    }
}

/// `GeminiModelOptions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GeminiModelOptions {
    /// Thinking level (e.g. `"low"`, `"high"`); the harness sends it as
    /// `generationConfig.thinkingConfig.thinkingLevel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
    /// Service tier (`"standard"`, `"priority"`, `"flex"`); the harness
    /// sends it as the request's `serviceTier`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Output cap per model call (harness default 65535).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
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
    /// Declares every tool to the model up front instead of behind the
    /// lazy `call_mcp_tool` indirection.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub force_all_tools_eager: bool,
}

/// `McpStdioTransport`.
///
/// `Debug` redacts the environment variables' values, keeping their names.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
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
///
/// `Debug` redacts the header values, keeping their names.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpHttpTransport {
    /// Server URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Extra HTTP headers.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub headers: BTreeMap<String, String>,
}

impl std::fmt::Debug for McpStdioTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { command, args, env } = self;
        f.debug_struct("McpStdioTransport")
            .field("command", command)
            .field("args", args)
            .field("env", &redact::values(env))
            .finish()
    }
}

impl std::fmt::Debug for McpHttpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { url, headers } = self;
        f.debug_struct("McpHttpTransport")
            .field("url", url)
            .field("headers", &redact::values(headers))
            .finish()
    }
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
    /// How the harness frames the subagent's task (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_behavior: Option<AgentBehavior>,
    /// The model the subagent runs on. Only `name` is read: a subagent
    /// always uses the agent-level endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelConfig>,
    /// Which skills the subagent sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_config: Option<SubagentSkillsConfig>,
}
