//! Spawn-time validation of an [`AgentBuilder`].

use std::collections::HashSet;

use super::AgentBuilder;
use crate::antigravity::tools::ToolDispatcher;
use crate::antigravity::{AgentBehavior, AntigravityError, BuiltinTool};

impl AgentBuilder {
    /// Everything `spawn()` checks before it starts a harness: errors for
    /// configurations that cannot work or would be unsafe, `warn!`s for the
    /// likeliest harmless misconfigurations.
    pub(super) fn validate(&self, dispatcher: &ToolDispatcher) -> Result<(), AntigravityError> {
        // An explicit containment opt-in that can't be honored is an error,
        // not a warning: without a workspace the harness has nothing to
        // confine to, and the agent could read any file.
        if self.workspace_containment == Some(true) && self.workspaces.is_empty() {
            return Err(AntigravityError::Config(
                "with_workspace_containment(true) needs a workspace: add one with \
                 `.add_workspace(..)`, or drop the call to run unconfined"
                    .to_string(),
            ));
        }
        // Safety parity with the reference SDK: refuse to run write-capable
        // agents with no policy and no pre-tool hook.
        if (self.capabilities.has_write_tools() || !self.mcp_servers.is_empty())
            && self.policies.is_empty()
            && self.pre_tool.is_none()
        {
            return Err(AntigravityError::Config(
                "write-capable built-in tools or MCP servers are enabled without a safety \
                 policy. Add `.add_policy(policy::allow_all())` to approve all tool calls, \
                 `.add_policy(policy::deny_all())` plus specific `policy::allow(..)` rules \
                 to selectively allow tools, or an `.on_pre_tool(..)` hook."
                    .to_string(),
            ));
        }
        if self.model.is_some() && self.api_key().is_none() {
            return Err(AntigravityError::Config(
                "a model is configured without an API key; call with_api_key(..) with a \
                 non-empty key"
                    .to_string(),
            ));
        }
        if self.gemini_base_url.is_some() && self.api_key().is_none() {
            return Err(AntigravityError::Config(
                "with_gemini_base_url(..) needs with_api_key(..) with a non-empty key: \
                 without one the harness sends its environment's GEMINI_API_KEY to the \
                 custom host"
                    .to_string(),
            ));
        }
        if let Some(trigger) = self.triggers.iter().find(|t| t.interval.is_zero()) {
            return Err(AntigravityError::Config(format!(
                "trigger '{}' has a zero interval; trigger intervals must be non-zero",
                trigger.message
            )));
        }
        // Not an error (the combination is harmless), but the likeliest
        // first-time misconfiguration: a questions hook with the builtin
        // disabled never fires, and without this there is no signal
        // distinguishing "the agent chose not to ask" from "asking was
        // never enabled".
        if self.questions.is_some() && !self.capabilities.is_enabled(BuiltinTool::AskQuestion) {
            tracing::warn!(
                "on_questions hook set but BuiltinTool::AskQuestion is not enabled - the agent \
                 will never ask questions and the hook will never run. Enable it with \
                 Capabilities::read_only().enable(BuiltinTool::AskQuestion)."
            );
        }
        // Same failure shape one layer down (reference-SDK parity): the
        // tool is declared, but the default autonomous prompt tells the
        // model nobody will answer, so it asks in prose instead.
        if self.capabilities.is_enabled(BuiltinTool::AskQuestion)
            && self.agent_behavior != Some(AgentBehavior::Interactive)
        {
            tracing::warn!(
                "BuiltinTool::AskQuestion is enabled but agent behavior is not Interactive - \
                 the harness's autonomous prompt tells the model the user will not respond, \
                 so it rarely calls ask_question. Add \
                 .with_agent_behavior(AgentBehavior::Interactive)."
            );
        }
        self.validate_subagents(dispatcher)
    }

    /// Subagent validation: unique names, and every referenced custom tool
    /// registered on the parent (the harness dispatches subagent custom-tool
    /// calls through the parent's registry).
    fn validate_subagents(&self, dispatcher: &ToolDispatcher) -> Result<(), AntigravityError> {
        let declared: HashSet<String> = dispatcher
            .harness_declarations()
            .into_iter()
            .filter_map(|tool| tool.name)
            .collect();
        let mut subagent_names = HashSet::new();
        for subagent in &self.subagents {
            if !subagent_names.insert(subagent.name().to_string()) {
                return Err(AntigravityError::Config(format!(
                    "duplicate subagent name '{}'; subagent names must be unique",
                    subagent.name()
                )));
            }
            if let Some(missing) = subagent
                .tool_names()
                .iter()
                .find(|name| !declared.contains(name.as_str()))
            {
                return Err(AntigravityError::Config(format!(
                    "subagent '{}' references custom tool '{missing}' which is not \
                     registered on the agent; custom tools used by subagents must also \
                     be added via add_tool(..) or with_tool_service(..)",
                    subagent.name()
                )));
            }
        }
        if !self.subagents.is_empty() && !self.capabilities.is_enabled(BuiltinTool::StartSubagent) {
            tracing::warn!(
                "Subagents are configured but the start_subagent builtin is disabled; \
                 the parent agent cannot invoke them. Enable it with \
                 with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))."
            );
        }
        Ok(())
    }
}
