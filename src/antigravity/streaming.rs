//! Streaming event types for Antigravity agent turns.

use futures_util::Stream;
use serde_json::Value;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::protocol::{
    ActionCompaction, ActionCreateFile, ActionEditFile, ActionError, ActionFindFile, ActionFinish,
    ActionGenerateImage, ActionInvokeSubagent, ActionListDirectory, ActionMcpTool,
    ActionRunCommand, ActionSearchDirectory, ActionSearchWeb, ActionViewFile, StepUpdate,
};
use super::{AntigravityError, ChatResponse};

/// One event observed while an agent turn runs.
///
/// This enum is `#[non_exhaustive]`: new event kinds may be added in future
/// releases, so `match` statements must include a wildcard arm.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AgentEvent {
    /// Incremental response text.
    TextDelta(String),
    /// Incremental thinking text.
    ThinkingDelta(String),
    /// A harness-side tool action completed (details in the action).
    ToolAction(Box<ToolAction>),
    /// A custom (client-executed) tool was dispatched and its result
    /// returned to the harness.
    ToolCallDispatched {
        /// The tool name.
        name: String,
        /// The harness's correlation id for this call.
        id: String,
    },
    /// The turn finished; carries the assembled response.
    Finished(Box<ChatResponse>),
    /// The harness reported an error step. Fatal configuration errors
    /// (HTTP 400/401/403 from the model backend) abort the turn with
    /// [`AntigravityError::Turn`] instead.
    Error(String),
    /// An event this crate does not recognize (Evergreen).
    Unknown {
        /// The unrecognized oneof field name.
        event_type: String,
        /// The raw JSON payload.
        data: Value,
    },
}

impl AgentEvent {
    /// Check if this is an unknown event.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized event type if this is an unknown event.
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

/// A structured view of one completed harness-side tool action.
///
/// This enum is `#[non_exhaustive]`: the harness grows new actions over
/// time; unrecognized ones surface through the `StepUpdate`'s preserved
/// `extra` fields rather than a dedicated variant here.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ToolAction {
    /// `run_command` — shell execution.
    RunCommand(ActionRunCommand),
    /// `edit_file`.
    EditFile(ActionEditFile),
    /// `create_file`.
    CreateFile(ActionCreateFile),
    /// `view_file`.
    ViewFile(ActionViewFile),
    /// `list_directory`.
    ListDirectory(ActionListDirectory),
    /// `find_file`.
    FindFile(ActionFindFile),
    /// `search_directory` (grep).
    SearchDirectory(ActionSearchDirectory),
    /// An MCP tool call executed by the harness.
    McpTool(ActionMcpTool),
    /// `search_web`.
    SearchWeb(ActionSearchWeb),
    /// `generate_image`.
    GenerateImage(ActionGenerateImage),
    /// `start_subagent`.
    InvokeSubagent(ActionInvokeSubagent),
    /// History compaction.
    Compaction(ActionCompaction),
    /// `finish` with structured output.
    Finish(ActionFinish),
    /// An error step.
    Error(ActionError),
}

impl ToolAction {
    /// Extracts the action from a step update, if one is present.
    pub(crate) fn from_step(step: &StepUpdate) -> Option<Self> {
        if let Some(a) = &step.run_command {
            return Some(Self::RunCommand(a.clone()));
        }
        if let Some(a) = &step.edit_file {
            return Some(Self::EditFile(a.clone()));
        }
        if let Some(a) = &step.create_file {
            return Some(Self::CreateFile(a.clone()));
        }
        if let Some(a) = &step.view_file {
            return Some(Self::ViewFile(a.clone()));
        }
        if let Some(a) = &step.list_directory {
            return Some(Self::ListDirectory(a.clone()));
        }
        if let Some(a) = &step.find_file {
            return Some(Self::FindFile(a.clone()));
        }
        if let Some(a) = &step.search_directory {
            return Some(Self::SearchDirectory(a.clone()));
        }
        if let Some(a) = &step.mcp_tool {
            return Some(Self::McpTool(a.clone()));
        }
        if let Some(a) = &step.search_web {
            return Some(Self::SearchWeb(a.clone()));
        }
        if let Some(a) = &step.generate_image {
            return Some(Self::GenerateImage(a.clone()));
        }
        if let Some(a) = &step.invoke_subagent {
            return Some(Self::InvokeSubagent(a.clone()));
        }
        if let Some(a) = &step.compaction {
            return Some(Self::Compaction(a.clone()));
        }
        if let Some(a) = &step.finish {
            return Some(Self::Finish(a.clone()));
        }
        if let Some(a) = &step.error {
            return Some(Self::Error(a.clone()));
        }
        None
    }

    /// The policy/confirmation tool name for this action (the builtin wire
    /// name, or `mcp_<server>_<tool>` for MCP actions).
    #[must_use]
    pub fn tool_name(&self) -> String {
        match self {
            Self::RunCommand(_) => "run_command".to_string(),
            Self::EditFile(_) => "edit_file".to_string(),
            Self::CreateFile(_) => "create_file".to_string(),
            Self::ViewFile(_) => "view_file".to_string(),
            Self::ListDirectory(_) => "list_directory".to_string(),
            Self::FindFile(_) => "find_file".to_string(),
            Self::SearchDirectory(_) => "search_directory".to_string(),
            Self::McpTool(a) => super::hooks::mcp_tool_name(
                a.server_name.as_deref().unwrap_or_default(),
                a.tool_name.as_deref().unwrap_or_default(),
            ),
            Self::SearchWeb(_) => "search_web".to_string(),
            Self::GenerateImage(_) => "generate_image".to_string(),
            Self::InvokeSubagent(_) => "start_subagent".to_string(),
            Self::Compaction(_) => "compaction".to_string(),
            Self::Finish(_) => "finish".to_string(),
            Self::Error(_) => "error".to_string(),
        }
    }

    /// The action's arguments as JSON (for policy predicates and hooks).
    #[must_use]
    pub fn args(&self) -> Value {
        let result = match self {
            Self::RunCommand(a) => serde_json::to_value(a),
            Self::EditFile(a) => serde_json::to_value(a),
            Self::CreateFile(a) => serde_json::to_value(a),
            Self::ViewFile(a) => serde_json::to_value(a),
            Self::ListDirectory(a) => serde_json::to_value(a),
            Self::FindFile(a) => serde_json::to_value(a),
            Self::SearchDirectory(a) => serde_json::to_value(a),
            Self::McpTool(a) => a
                .arguments_json
                .as_deref()
                .map(serde_json::from_str)
                .unwrap_or_else(|| Ok(Value::Object(serde_json::Map::new()))),
            Self::SearchWeb(a) => serde_json::to_value(a),
            Self::GenerateImage(a) => serde_json::to_value(a),
            Self::InvokeSubagent(a) => serde_json::to_value(a),
            Self::Compaction(a) => serde_json::to_value(a),
            Self::Finish(a) => serde_json::to_value(a),
            Self::Error(a) => serde_json::to_value(a),
        };
        result.unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
    }
}

/// A stream of [`AgentEvent`]s for one turn, returned by
/// [`AntigravityAgent::send_streaming`](super::AntigravityAgent::send_streaming).
///
/// The stream mutably borrows the agent for the duration of the turn; drive
/// it to completion (until [`AgentEvent::Finished`] or `None`) before
/// sending the next message. To cancel mid-turn, grab a
/// [`CancelHandle`](super::CancelHandle) before starting the stream.
pub struct AgentEventStream<'a> {
    inner: Pin<Box<dyn Stream<Item = Result<AgentEvent, AntigravityError>> + Send + 'a>>,
}

impl<'a> AgentEventStream<'a> {
    pub(crate) fn new(
        inner: Pin<Box<dyn Stream<Item = Result<AgentEvent, AntigravityError>> + Send + 'a>>,
    ) -> Self {
        Self { inner }
    }
}

impl std::fmt::Debug for AgentEventStream<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentEventStream").finish_non_exhaustive()
    }
}

impl Stream for AgentEventStream<'_> {
    type Item = Result<AgentEvent, AntigravityError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_action_from_step_extracts_run_command() {
        let step = StepUpdate {
            run_command: Some(ActionRunCommand {
                command_line: Some("ls".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let action = ToolAction::from_step(&step).unwrap();
        assert_eq!(action.tool_name(), "run_command");
        assert_eq!(action.args()["commandLine"], "ls");
    }

    #[test]
    fn test_tool_action_from_step_none_when_no_action() {
        let step = StepUpdate {
            text: Some("hi".to_string()),
            ..Default::default()
        };
        assert!(ToolAction::from_step(&step).is_none());
    }

    #[test]
    fn test_tool_action_mcp_naming_and_args() {
        let step = StepUpdate {
            mcp_tool: Some(ActionMcpTool {
                server_name: Some("git".to_string()),
                tool_name: Some("status".to_string()),
                arguments_json: Some(r#"{"repo": "."}"#.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let action = ToolAction::from_step(&step).unwrap();
        assert_eq!(action.tool_name(), "mcp_git_status");
        assert_eq!(action.args()["repo"], ".");
    }

    #[test]
    fn test_agent_event_unknown_helpers() {
        let event = AgentEvent::Unknown {
            event_type: "novel".to_string(),
            data: serde_json::json!({"x": 1}),
        };
        assert!(event.is_unknown());
        assert_eq!(event.unknown_event_type(), Some("novel"));
        assert_eq!(event.unknown_data(), Some(&serde_json::json!({"x": 1})));
        assert!(!AgentEvent::TextDelta("t".to_string()).is_unknown());
        assert_eq!(
            AgentEvent::TextDelta("t".to_string()).unknown_event_type(),
            None
        );
        assert_eq!(AgentEvent::TextDelta("t".to_string()).unknown_data(), None);
    }
}
