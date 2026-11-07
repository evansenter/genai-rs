//! Spawn-time validation of an [`AgentBuilder`].

use std::collections::HashSet;

use super::AgentBuilder;
use crate::antigravity::process::API_KEY_ENV_VARS;
use crate::antigravity::tools::ToolDispatcher;
use crate::antigravity::{
    AgentBehavior, AntigravityError, Budget, BuiltinTool, McpServer, Policy, PolicyDecision,
    SessionContinuation,
};

/// A configuration error.
fn config(message: impl Into<String>) -> AntigravityError {
    AntigravityError::Config(message.into())
}

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
        if let Some(setter) = self.options.needs_model_config()
            && self.api_key().is_none()
        {
            return Err(config(format!(
                "{setter}(..) needs with_api_key(..) with a non-empty key: model options \
                 travel in the model config, which is sent only with a key"
            )));
        }
        if self
            .gemini_base_url
            .as_ref()
            .and(
                API_KEY_ENV_VARS
                    .iter()
                    .find(|var| self.options.harness_env.contains_key(**var)),
            )
            .is_some()
        {
            return Err(config(
                "add_harness_env(..) sets an API key variable while with_gemini_base_url(..) \
                 is set; the harness would send it to the custom host",
            ));
        }
        self.validate_options()?;
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
        let invokes = self.capabilities.is_enabled(BuiltinTool::InvokeSubagent);
        if !self.subagents.is_empty() && !invokes {
            tracing::warn!(
                "Subagents are configured but the invoke_subagent builtin is disabled; \
                 the parent agent cannot invoke them. Enable it with \
                 with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))."
            );
        }
        self.refuse_ungated_subagents(invokes)?;
        if !invokes
            && let Some(setter) = [
                (
                    self.options.max_subagent_depth.is_some(),
                    "with_max_subagent_depth",
                ),
                (
                    self.options.allowed_subagents.is_some(),
                    "with_allowed_subagents",
                ),
            ]
            .into_iter()
            .find_map(|(set, setter)| set.then_some(setter))
        {
            return Err(config(format!(
                "{setter}(..) needs the invoke_subagent builtin: enable \
                 BuiltinTool::InvokeSubagent in with_capabilities(..)"
            )));
        }
        if self.options.max_subagent_depth == Some(0) {
            return Err(config("with_max_subagent_depth(..) must be at least 1"));
        }
        let lists = std::iter::once(("the agent", self.options.allowed_subagents.as_deref()))
            .chain(
                self.subagents
                    .iter()
                    .map(|s| (s.name(), s.allowed_subagents())),
            );
        for (owner, allowed) in lists {
            if let Some(unknown) = allowed
                .unwrap_or_default()
                .iter()
                .find(|name| !subagent_names.contains(name.as_str()))
            {
                return Err(config(format!(
                    "allowed subagents of {owner} name '{unknown}', which is not a subagent \
                     added with add_subagent(..)"
                )));
            }
        }
        Ok(())
    }

    /// Refuses an agent that offers subagents while a gate is set, unless
    /// the caller opted in with `with_ungated_subagents(true)` (#520,
    /// D-NEXT-WP22): a subagent's own tool calls bypass every gate (harness
    /// 0.1.23, pinned by
    /// `test_scripted_harness_does_not_gate_a_subagents_tool_calls`).
    ///
    /// `define_subagent` counts on its own: the harness offers it only
    /// beside `invoke_subagent` (0.1.23), which a later release may change.
    fn refuse_ungated_subagents(&self, invokes: bool) -> Result<(), AntigravityError> {
        let offered: Vec<&str> = [
            (invokes, "invoke_subagent (BuiltinTool::InvokeSubagent)"),
            (
                self.options.define_subagent_tool,
                "define_subagent (with_define_subagent_tool)",
            ),
        ]
        .into_iter()
        .filter_map(|(on, tool)| on.then_some(tool))
        .collect();
        if offered.is_empty() || self.ungated_subagents {
            return Ok(());
        }
        let gates = self.subagent_bypassed_gates();
        if gates.is_empty() {
            return Ok(());
        }
        Err(config(format!(
            "{} would let subagents bypass this agent's gates: {}. A subagent's own tool \
             calls, the harness's built-in research subagent's included, reach none of \
             them; only the subagent's capabilities limit it. Call \
             with_ungated_subagents(true) to run subagents anyway, or remove the gates or \
             the subagent tools",
            offered.join(" and "),
            gates.join(", ")
        )))
    }

    /// The gates set on this agent that a subagent's own tool calls bypass.
    /// A policy set that only allows is not one: it refuses nothing, so
    /// there is nothing for a subagent to get around.
    fn subagent_bypassed_gates(&self) -> Vec<&'static str> {
        let refusing_policy = self
            .policies
            .iter()
            .any(|p| !p.is_auto() && p.decision() != PolicyDecision::Allow);
        let auto_policy = self.policies.iter().any(Policy::is_auto);
        [
            (self.pre_tool.is_some(), "a pre-tool hook (on_pre_tool)"),
            (refusing_policy, "a deny or confirm policy (add_policy)"),
            (auto_policy, "the auto policy (policy::auto)"),
            (
                self.contains_workspaces(),
                "workspace containment (on by default with a workspace)",
            ),
            (
                self.capabilities.run_command().sandbox(),
                "the run_command sandbox (RunCommandOptions::with_sandbox)",
            ),
        ]
        .into_iter()
        .filter_map(|(set, gate)| set.then_some(gate))
        .collect()
    }

    /// Range and combination checks on the harness options (the Python
    /// SDK's pydantic validators, where the harness has the same limits).
    fn validate_options(&self) -> Result<(), AntigravityError> {
        let options = &self.options;
        if let Some(cap) = options.budget.as_ref().and_then(Budget::zero_cap) {
            return Err(config(format!("Budget {cap} must be at least 1")));
        }
        if options.max_output_tokens == Some(0) {
            return Err(config("with_max_output_tokens(..) must be at least 1"));
        }
        if options.compaction_threshold == Some(0) {
            return Err(config("with_compaction_threshold(..) must be at least 1"));
        }
        if options
            .mcp_max_output_bytes
            .is_some_and(|bytes| bytes == 0 || i32::try_from(bytes).is_err())
        {
            return Err(config(
                "with_mcp_max_output_bytes(..) must be between 1 and i32::MAX",
            ));
        }
        if options
            .tool_output_truncation
            .is_some_and(|tokens| i32::try_from(tokens).is_err())
        {
            return Err(config(
                "with_tool_output_truncation(..) must be at most i32::MAX",
            ));
        }
        if !options.inline_skills.is_empty() && !self.skills_paths.is_empty() {
            return Err(config(
                "add_inline_skill(..) and add_skills_path(..) cannot be combined: the harness \
                 takes skills from one source or the other",
            ));
        }
        let mut skill_names = HashSet::new();
        if let Some(duplicate) = options
            .inline_skills
            .iter()
            .find(|skill| !skill_names.insert(skill.name()))
        {
            return Err(config(format!(
                "duplicate inline skill name '{}'; skill names must be unique",
                duplicate.name()
            )));
        }
        if self.policies.iter().filter(|p| p.is_auto()).count() > 1 {
            return Err(config("at most one policy::auto() rule may be added"));
        }
        for (short, long) in overlapping_mcp_servers(self.mcp_servers.iter().map(McpServer::name)) {
            tracing::warn!(
                "MCP servers '{short}' and '{long}': a rule naming a tool as \
                 mcp_{short}_<tool> can also match a tool of '{long}' (tool \
                 names are read the same both ways). Server-wide rules \
                 (mcp_{short}_*) match on the server and are not affected; \
                 rename a server to rule this out."
            );
        }
        if options.session_continuation == Some(SessionContinuation::Resume)
            && self.conversation_id.is_none()
        {
            return Err(config(
                "SessionContinuation::Resume needs with_conversation_id(..)",
            ));
        }
        Ok(())
    }
}

/// Pairs of MCP server names where the first, followed by `_`, starts the
/// second (`git`, `git_admin`): their `mcp_<server>_<tool>` names overlap.
pub(super) fn overlapping_mcp_servers<'a>(
    names: impl Iterator<Item = &'a str> + Clone,
) -> Vec<(&'a str, &'a str)> {
    names
        .clone()
        .flat_map(|short| {
            names
                .clone()
                .filter(move |long| {
                    long.strip_prefix(short)
                        .is_some_and(|rest| rest.starts_with('_'))
                })
                .map(move |long| (short, long))
        })
        .collect()
}
