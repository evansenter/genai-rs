//! [`Subagent`]: a static subagent the parent agent can delegate to.

use super::protocol;
use super::{AgentBehavior, BuiltinTool, Capabilities};

/// A static subagent the parent agent can delegate to.
///
/// Subagents run in their own trajectory with their own instructions and
/// tool set. The parent model invokes them through the `start_subagent`
/// builtin — enable it with
/// `Capabilities::read_only().enable(BuiltinTool::StartSubagent)` (it is
/// write-capable, so the spawn-time safety gate requires a policy or
/// pre-tool hook).
///
/// ```rust,ignore
/// use genai_rs::antigravity::{AntigravityAgent, BuiltinTool, Capabilities, Subagent, policy};
///
/// let agent = AntigravityAgent::builder()
///     .add_tool(severity_classifier_declaration())   // parent registration
///     .add_subagent(
///         Subagent::new("auditor")
///             .with_description("Audits one file for security issues.")
///             .with_system_instructions("Focus on injection vectors.")
///             .add_tool("severity_classifier"),      // reference by name
///     )
///     .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))
///     .add_policy(policy::allow_all())
///     // ...
///     ;
/// ```
///
/// Custom tools are referenced **by name** and must also be registered on
/// the parent agent (`add_tool` / `with_tool_service`) — the harness
/// dispatches a subagent's custom tool calls through the parent's tool
/// registry. `spawn()` validates this and fails with
/// [`AntigravityError::Config`](super::AntigravityError::Config) on a
/// dangling reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subagent {
    name: String,
    description: Option<String>,
    system_instructions: Option<String>,
    capabilities: Capabilities,
    tool_names: Vec<String>,
    agent_behavior: Option<AgentBehavior>,
}

impl Subagent {
    /// Creates a subagent with the given (unique) name and the default
    /// read-only builtin set (matching the reference SDK).
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            system_instructions: None,
            capabilities: Capabilities::read_only(),
            tool_names: Vec::new(),
            agent_behavior: None,
        }
    }

    /// Sets the description shown to the parent model (how it decides when
    /// to delegate).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets system instructions for the subagent. These are **appended** to
    /// the harness's default subagent instructions (reference SDK
    /// semantics), not a full replacement.
    #[must_use]
    pub fn with_system_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.system_instructions = Some(instructions.into());
        self
    }

    /// Configures which built-in harness tools the subagent may use
    /// (default: the read-only set). `start_subagent` is force-disabled:
    /// harness 0.1.18 can nest subagents, but only up to a
    /// `max_nesting_depth` this crate does not configure yet, and the
    /// harness default is flat (depth 1).
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Sets how the harness frames the subagent's task (default: the
    /// harness's, which is [`AgentBehavior::Autonomous`]).
    #[must_use]
    pub fn with_agent_behavior(mut self, behavior: AgentBehavior) -> Self {
        self.agent_behavior = Some(behavior);
        self
    }

    /// Adds a custom tool, referenced by name. The tool must also be
    /// registered on the parent agent; `spawn()` validates this.
    #[must_use]
    pub fn add_tool(mut self, name: impl Into<String>) -> Self {
        self.tool_names.push(name.into());
        self
    }

    /// The subagent's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The custom tool names this subagent references.
    #[must_use]
    pub fn tool_names(&self) -> &[String] {
        &self.tool_names
    }

    /// Builds the wire `CustomAgent`, resolving custom tool names against
    /// the parent's declarations. Callers must have validated the names
    /// beforehand (`spawn()` does); unresolved names are skipped here.
    ///
    /// `workspace_note`, when present, is appended as an extra instruction
    /// section so the subagent learns the workspace root(s) — its trajectory
    /// does not inherit the parent's context.
    pub(crate) fn to_wire(
        &self,
        parent_tools: &[protocol::Tool],
        workspace_note: Option<&str>,
    ) -> protocol::CustomAgent {
        if self.capabilities.is_enabled(BuiltinTool::StartSubagent) {
            tracing::warn!(
                "Subagent '{}' enables start_subagent, but this crate does not configure \
                 subagent nesting depth (harness default: flat); disabling it.",
                self.name
            );
        }
        let mut harness_side_tools = self.capabilities.to_harness_side_tools();
        // Nesting needs `SubagentsConfig.max_nesting_depth`, which is not
        // modeled; the harness default depth is 1 (flat).
        harness_side_tools.subagents = Some(protocol::ToolToggle::new(false));

        let tools = self
            .tool_names
            .iter()
            .filter_map(|name| {
                parent_tools
                    .iter()
                    .find(|tool| tool.name.as_deref() == Some(name))
                    .cloned()
            })
            .collect();

        // Reference SDK semantics: subagent instructions are appended
        // sections (title "System"), never a full replacement. The
        // workspace note (when present) is appended as its own section.
        let mut appended_sections = Vec::new();
        if let Some(text) = &self.system_instructions {
            appended_sections.push(protocol::InstructionSection {
                title: Some("System".to_string()),
                content: Some(text.clone()),
            });
        }
        if let Some(note) = workspace_note {
            appended_sections.push(protocol::InstructionSection {
                title: Some("Workspace".to_string()),
                content: Some(note.to_string()),
            });
        }
        let system_instructions =
            (!appended_sections.is_empty()).then_some(protocol::SystemInstructions {
                custom: None,
                appended: Some(protocol::AppendedSystemInstructions {
                    custom_identity: None,
                    appended_sections,
                }),
            });

        protocol::CustomAgent {
            name: Some(self.name.clone()),
            description: self.description.clone(),
            system_instructions,
            harness_side_tools: Some(harness_side_tools),
            tools,
            agent_behavior: self.agent_behavior.map(AgentBehavior::to_wire),
        }
    }
}

#[cfg(test)]
#[path = "subagent_tests.rs"]
mod tests;
