//! [`Subagent`]: a static subagent the parent agent can delegate to.

use super::protocol;
use super::{AgentBehavior, BuiltinTool, Capabilities, SubagentSkills, SystemInstructions};

/// A static subagent the parent agent can delegate to.
///
/// Subagents run in their own trajectory with their own instructions and
/// tool set. The parent model invokes them through the `invoke_subagent`
/// builtin — enable it with
/// `Capabilities::read_only().enable(BuiltinTool::InvokeSubagent)` (it is
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
///     .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))
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
    system_instructions: Option<SystemInstructions>,
    capabilities: Capabilities,
    tool_names: Vec<String>,
    agent_behavior: Option<AgentBehavior>,
    model: Option<String>,
    allowed_subagents: Option<Vec<String>>,
    skills: Option<SubagentSkills>,
}

/// What the parent agent's configuration decides for every subagent's wire
/// form.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SubagentWireContext<'a> {
    /// The workspace announcement, added to custom instructions only (the
    /// harness's own subagent prompt lists the workspaces).
    pub(crate) workspace_note: Option<&'a str>,
    /// Whether nesting is configured (a maximum depth of two or more), so a
    /// subagent may invoke subagents itself.
    pub(crate) nesting: bool,
    /// Hides `define_subagent` from subagents that may invoke subagents.
    pub(crate) disable_define_subagent_tool: bool,
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
            model: None,
            allowed_subagents: None,
            skills: None,
        }
    }

    /// Sets the description shown to the parent model (how it decides when
    /// to delegate).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets system instructions for the subagent. A string is **added to**
    /// the harness's own subagent prompt (which lists the workspaces);
    /// [`SystemInstructions::custom`] replaces it, as for the parent.
    #[must_use]
    pub fn with_system_instructions(mut self, instructions: impl Into<SystemInstructions>) -> Self {
        self.system_instructions = Some(instructions.into());
        self
    }

    /// Configures which built-in harness tools the subagent may use
    /// (default: the read-only set). [`BuiltinTool::InvokeSubagent`] takes
    /// effect only when the agent allows nesting
    /// ([`AgentBuilder::with_max_subagent_depth`](super::AgentBuilder::with_max_subagent_depth)
    /// of 2 or more); otherwise it is turned off, since the harness default
    /// is one level.
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

    /// Runs the subagent on another model than the parent's, by name. It
    /// uses the parent's endpoint (key, base URL, headers): only the name
    /// differs.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Limits which subagents this subagent may invoke, when nesting is
    /// configured. Every name must be a subagent added to the agent;
    /// `spawn()` validates this.
    #[must_use]
    pub fn with_allowed_subagents(
        mut self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_subagents = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Sets which skills the subagent sees (default: all of the parent's).
    #[must_use]
    pub fn with_skills(mut self, skills: SubagentSkills) -> Self {
        self.skills = Some(skills);
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

    /// The subagents this one may invoke, when set.
    #[must_use]
    pub fn allowed_subagents(&self) -> Option<&[String]> {
        self.allowed_subagents.as_deref()
    }

    /// Builds the wire `CustomAgent`, resolving custom tool names against
    /// the parent's declarations. Callers must have validated the names
    /// beforehand (`spawn()` does); unresolved names are skipped here.
    pub(crate) fn to_wire(
        &self,
        parent_tools: &[protocol::Tool],
        context: SubagentWireContext<'_>,
    ) -> protocol::CustomAgent {
        let invokes = self.capabilities.is_enabled(BuiltinTool::InvokeSubagent);
        if invokes && !context.nesting {
            tracing::warn!(
                "Subagent '{}' enables invoke_subagent, but the agent allows one level of \
                 subagents (set with_max_subagent_depth(2) or more to nest); disabling it.",
                self.name
            );
        }
        let mut harness_side_tools = self.capabilities.to_harness_side_tools();
        harness_side_tools.subagents = Some(protocol::SubagentsConfig {
            enabled: invokes && context.nesting,
            allowed_subagents: self.allowed_subagents.clone().unwrap_or_default(),
            disable_define_subagent_tool: context.disable_define_subagent_tool,
            ..Default::default()
        });

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

        // A custom prompt has no workspace list; the harness's own one does.
        let system_instructions = self.system_instructions.as_ref().map(|instructions| {
            let note = instructions
                .is_custom()
                .then_some(context.workspace_note)
                .flatten();
            instructions.to_wire(note)
        });

        protocol::CustomAgent {
            name: Some(self.name.clone()),
            description: self.description.clone(),
            system_instructions,
            harness_side_tools: Some(harness_side_tools),
            tools,
            agent_behavior: self.agent_behavior.map(AgentBehavior::to_wire),
            model: self.model.as_ref().map(|name| protocol::ModelConfig {
                name: Some(name.clone()),
                ..Default::default()
            }),
            skills_config: self.skills.as_ref().map(SubagentSkills::to_wire),
        }
    }
}

#[cfg(test)]
#[path = "subagent_tests.rs"]
mod tests;
