use super::{LineAction, StepSource, StepState, StepTarget, ToolCall, ToolResponse, flex_num};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
    /// The trajectory that spawned this step's trajectory — set on
    /// subagent steps, absent on the root conversation's (new in harness
    /// 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_trajectory_id: Option<String>,
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
    /// `read_url_content` action details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_url_content: Option<ActionReadUrlContent>,
    /// A client-executed custom tool call, mirrored into the trajectory.
    /// The call itself arrives separately as an
    /// [`OutputPayload::ToolCall`](super::OutputPayload::ToolCall); this is the harness's record of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_tool: Option<ActionCustomTool>,
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
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — keeps
    /// the question path lossless one level down too.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Offset into the file's content (new in harness 0.1.18).
    #[serde(
        default,
        deserialize_with = "flex_num::opt_i32",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_offset: Option<i32>,
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

/// `ActionInvokeSubagent` — subagent invocation marker.
///
/// The invoked subagent's `name` is modeled as an optional typed field, but
/// **the harness does not populate it** (verified live on 0.1.5, 0.1.10 and
/// 0.1.18 by `test_antigravity_subagent_is_actually_invoked`, which
/// delegates for real and reports the answer rather than asserting the old
/// one): the `invokeSubagent` step action is an empty message on the wire,
/// and the step only carries the generic text `"Invoke subagent"`. The
/// field is here so that a future harness emitting the name surfaces it
/// without an API break; until then it stays `None`, and any unexpected
/// field is preserved in `extra` (Evergreen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionInvokeSubagent {
    /// The invoked subagent's name, when the harness reports it (see the
    /// type docs — `None` on every harness so far).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Where the generated image was written (new in harness 0.1.18).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
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

/// `ActionReadUrlContent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionReadUrlContent {
    /// The fetched URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The page title (populated when the step completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Content summary (populated when the step completes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Where the harness saved the full content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `ActionCustomTool` — the trajectory's record of a client-executed
/// custom tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActionCustomTool {
    /// The call as the model made it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ToolCall>,
    /// The client's response, once sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<ToolResponse>,
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
