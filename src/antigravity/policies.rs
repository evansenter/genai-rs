//! Tool policies for Antigravity agents.
//!
//! Policies are declarative allow/deny/ask rules over tool names, with
//! optional conditions on the arguments. This client evaluates them in the
//! harness's pre-tool callback, which gates every tool call (builtin,
//! custom, MCP, subagent) before it runs; the harness enforces the verdict.
//! The exception is [`policy::auto`], the harness's own model-judged
//! assessment, which runs harness-side and asks this client only about the
//! calls it flags (D-022).
//!
//! # Evaluation order
//!
//! The harness's own evaluator's order, which the Python SDK documents and
//! harness 0.1.23 follows (verified 2026-11-05):
//!
//! 1. **Specificity**: a rule naming the tool beats a server-wide MCP rule
//!    (`mcp_<server>_*`), which beats the wildcard (`"*"`).
//! 2. **Within a tier, safety**: deny beats ask (`confirm`, `ask_user`),
//!    which beats allow — whatever the registration order.
//! 3. Then registration order.
//!
//! A rule with a [`when`](Policy::when) condition applies only to calls the
//! condition accepts; the others fall through to the next rule. So
//! `[deny_all(), allow("get_weather")]` allows only `get_weather`, and
//! `[allow("run_command"), deny("run_command").when(rm)]` denies the
//! commands `rm` matches and allows the rest.
//!
//! When no rule matches, the call is **allowed** (default open), still
//! subject to the pre-tool hook. [`AgentBuilder::spawn`](super::AgentBuilder)
//! refuses to start an agent with write-capable builtins or MCP servers and
//! no policy or pre-tool hook at all, so default-open only applies once you
//! have opted into a policy set.
//!
//! # Targets
//!
//! - Custom tools and builtins are matched by name (builtins use their wire
//!   names: `run_command`, `edit_file`, `create_file`, `view_file`,
//!   `list_directory`, `search_directory`, `find_file`, `search_web`,
//!   `read_url_content`, `generate_image`, `invoke_subagent`,
//!   `ask_question`, `finish`). `start_subagent`, the subagent builtin's
//!   name before harness 0.1.18, still targets `invoke_subagent`.
//! - MCP tools are matched as `mcp_<server>_<tool>`, every tool of a
//!   server as `mcp_<server>_*`, and every MCP tool as `mcp_*`.
//! - `"*"` matches everything.
//!
//! `ask_question` is policy-gated like the rest on harness 0.1.18 and later:
//! the call passes through the pre-tool hook before the question is posed,
//! so a `deny_all()` set needs an explicit `allow("ask_question")`. The
//! answers come from the
//! [`on_questions`](crate::antigravity::AgentBuilder::on_questions) hook.

use std::sync::Arc;

use super::{PreToolDecision, PreToolHook, ToolInvocation};

/// What a matched policy decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PolicyDecision {
    /// Approve the call.
    Allow,
    /// Reject the call.
    Deny,
    /// Ask: the rule's [handler](Policy::with_handler) decides, or without
    /// one the agent's pre-tool hook. With neither the call is **denied**
    /// (fail closed).
    Confirm,
}

/// A condition on a tool call: the rule applies only when it returns true.
pub type PolicyCondition = Arc<dyn Fn(&ToolInvocation) -> bool + Send + Sync>;

/// Approves or refuses a call a rule asks about: handed the call and the
/// reason the asker gives (the rule's [reason](Policy::with_reason), or the
/// auto policy's assessment). `true` approves.
pub type AskUserHandler = Arc<dyn Fn(&ToolInvocation, Option<&str>) -> bool + Send + Sync>;

/// One tool policy rule. Build with the [`policy`] constructors.
#[derive(Clone)]
pub struct Policy {
    target: String,
    decision: PolicyDecision,
    auto: bool,
    condition: Option<PolicyCondition>,
    handler: Option<AskUserHandler>,
    reason: Option<String>,
    name: Option<String>,
}

impl std::fmt::Debug for Policy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Policy")
            .field("target", &self.target)
            .field("decision", &self.decision)
            .field("auto", &self.auto)
            .field("when", &self.condition.is_some())
            .field("handler", &self.handler.is_some())
            .field("reason", &self.reason)
            .field("name", &self.name)
            .finish()
    }
}

impl Policy {
    fn rule(target: impl Into<String>, decision: PolicyDecision) -> Self {
        let target = target.into();
        // The subagent builtin's pre-0.1.18 name, kept as a policy alias
        // (as the Python SDK keeps `START_SUBAGENT`).
        let target = if target == "start_subagent" {
            "invoke_subagent".to_string()
        } else {
            target
        };
        Self {
            target,
            decision,
            auto: false,
            condition: None,
            handler: None,
            reason: None,
            name: None,
        }
    }

    /// The tool name (or `"*"`, or `mcp_<server>_*`) this rule targets.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The decision this rule produces when matched.
    #[must_use]
    pub fn decision(&self) -> PolicyDecision {
        self.decision
    }

    /// Whether this is the [auto](policy::auto) policy.
    #[must_use]
    pub fn is_auto(&self) -> bool {
        self.auto
    }

    /// The rule's name, when set.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Applies the rule only to calls `condition` accepts (the Python SDK's
    /// `when=`); other calls fall through to the next rule. A condition that
    /// panics denies the call (fail closed).
    ///
    /// The condition reads the call's arguments in the tool's own schema
    /// (`CommandLine`, `AbsolutePath`), and should treat a missing argument
    /// as a match for a deny rule, or a renamed argument waves calls
    /// through.
    ///
    /// ```
    /// use genai_rs::antigravity::{ToolInvocation, policy};
    ///
    /// let no_rm = policy::deny("run_command").when(|call| {
    ///     call.args["CommandLine"].as_str().is_none_or(|cmd| cmd.contains("rm "))
    /// });
    /// # let _ = no_rm;
    /// ```
    #[must_use]
    pub fn when(
        mut self,
        condition: impl Fn(&ToolInvocation) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.condition = Some(Arc::new(condition));
        self
    }

    /// The message the model sees when this rule denies a call (and the
    /// reason an ask rule's handler is handed).
    #[must_use]
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// A label for the rule, quoted in its default denial message.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the handler that answers an ask rule (or the auto policy's
    /// flagged calls) instead of the pre-tool hook. Ignored on allow and
    /// deny rules.
    #[must_use]
    pub fn with_handler(
        mut self,
        handler: impl Fn(&ToolInvocation, Option<&str>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Specificity tier: 0 for a named tool, 1 for a server-wide MCP rule,
    /// 2 for the wildcard.
    fn tier(&self) -> u8 {
        if self.target == WILDCARD {
            2
        } else if self.server_prefix().is_some() {
            1
        } else {
            0
        }
    }

    /// `mcp_<server>_` for a server-wide `mcp_<server>_*` target.
    fn server_prefix(&self) -> Option<&str> {
        self.target
            .strip_suffix('*')
            .filter(|prefix| prefix.starts_with("mcp_") && prefix.ends_with('_'))
    }

    fn matches_target(&self, tool_name: &str) -> bool {
        match self.tier() {
            2 => true,
            1 => self
                .server_prefix()
                .is_some_and(|prefix| tool_name.starts_with(prefix)),
            _ => self.target == tool_name,
        }
    }

    /// Whether the rule applies to `invocation`: the target matches and the
    /// condition, if any, accepts it. `Err` when the condition panicked.
    fn applies_to(&self, invocation: &ToolInvocation) -> Result<bool, String> {
        if !self.matches_target(&invocation.name) {
            return Ok(false);
        }
        let Some(condition) = &self.condition else {
            return Ok(true);
        };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| condition(invocation))).map_err(
            |_| {
                format!(
                    "Tool '{}' denied: the condition of policy '{}' panicked (failing closed).",
                    invocation.name,
                    self.label()
                )
            },
        )
    }

    fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.target)
    }

    /// The message the model sees when this rule denies `tool_name`.
    fn deny_message(&self, tool_name: &str) -> String {
        if let Some(reason) = &self.reason {
            return reason.clone();
        }
        match &self.name {
            Some(name) => format!("Denied by policy '{name}' for tool '{tool_name}'."),
            None => format!("Denied by policy for tool '{tool_name}'."),
        }
    }
}

const WILDCARD: &str = "*";

/// Constructors for [`Policy`] rules.
pub mod policy {
    use super::{Policy, PolicyDecision, WILDCARD};
    use crate::antigravity::ToolInvocation;

    /// Approves calls to the named tool.
    #[must_use]
    pub fn allow(tool: impl Into<String>) -> Policy {
        Policy::rule(tool, PolicyDecision::Allow)
    }

    /// Rejects calls to the named tool.
    #[must_use]
    pub fn deny(tool: impl Into<String>) -> Policy {
        Policy::rule(tool, PolicyDecision::Deny)
    }

    /// Defers calls to the named tool to the agent's `on_pre_tool` hook.
    /// With no hook configured, matched calls are denied (fail closed).
    #[must_use]
    pub fn confirm(tool: impl Into<String>) -> Policy {
        Policy::rule(tool, PolicyDecision::Confirm)
    }

    /// Asks `handler` about calls to the named tool (the Python SDK's
    /// `ask_user`): `true` approves. The handler runs on a thread of its
    /// own, bounded by the turn deadline like every hook, so answer from
    /// state you already have rather than waiting on a person.
    #[must_use]
    pub fn ask_user(
        tool: impl Into<String>,
        handler: impl Fn(&ToolInvocation, Option<&str>) -> bool + Send + Sync + 'static,
    ) -> Policy {
        confirm(tool).with_handler(handler)
    }

    /// Approves every tool call. Intended for autonomous agents and local
    /// development.
    #[must_use]
    pub fn allow_all() -> Policy {
        allow(WILDCARD)
    }

    /// Rejects every tool call. Use as a base rule with specific
    /// [`allow`] overrides for a deny-by-default posture.
    #[must_use]
    pub fn deny_all() -> Policy {
        deny(WILDCARD)
    }

    /// The harness's model-judged safety assessment (the Python SDK's
    /// `policy.auto()`): before a risky call (`run_command`,
    /// `read_url_content`, …) runs, the harness has the model vet it, and
    /// a call it flags is **denied** unless a
    /// [handler](Policy::with_handler) approves it. The handler is handed
    /// the assessment as its reason.
    ///
    /// It runs harness-side, after this client's rules and hooks have
    /// allowed the call, and costs a model call (or more) per assessed
    /// tool call. At most one per agent.
    #[must_use]
    pub fn auto() -> Policy {
        let mut policy = Policy::rule(WILDCARD, PolicyDecision::Confirm).with_name("auto");
        policy.auto = true;
        policy
    }
}

/// Evaluates an ordered policy set against tool calls.
#[derive(Debug, Clone, Default)]
pub(crate) struct PolicyEngine {
    /// The client-evaluated rules, sorted into evaluation order.
    rules: Vec<Policy>,
    /// The auto policy, if one was added (harness-evaluated).
    auto: Option<Policy>,
}

/// A rule's verdict on one call.
#[derive(Debug)]
pub(crate) enum Verdict<'a> {
    /// A rule matched.
    Rule(&'a Policy),
    /// A rule's condition panicked: deny with this message.
    Broken(String),
}

impl PolicyEngine {
    pub(crate) fn new(policies: Vec<Policy>) -> Self {
        let (auto, mut rules): (Vec<Policy>, Vec<Policy>) =
            policies.into_iter().partition(|p| p.auto);
        // A stable sort keeps registration order within each rank.
        rules.sort_by_key(|p| {
            let decision = match p.decision {
                PolicyDecision::Deny => 0,
                PolicyDecision::Confirm => 1,
                PolicyDecision::Allow => 2,
            };
            (p.tier(), decision)
        });
        Self {
            rules,
            auto: auto.into_iter().next(),
        }
    }

    /// The auto policy, if one was added.
    pub(crate) fn auto(&self) -> Option<&Policy> {
        self.auto.as_ref()
    }

    /// Whether there are client-evaluated rules.
    pub(crate) fn has_rules(&self) -> bool {
        !self.rules.is_empty()
    }

    /// Whether evaluating may run user code (a condition or a handler), so
    /// the caller runs it on a hook thread.
    pub(crate) fn has_callbacks(&self) -> bool {
        self.rules
            .iter()
            .any(|rule| rule.condition.is_some() || rule.handler.is_some())
    }

    /// The first rule that applies to `invocation`, in evaluation order, or
    /// `None` when none does (callers treat that as allow: default open).
    pub(crate) fn verdict(&self, invocation: &ToolInvocation) -> Option<Verdict<'_>> {
        for rule in &self.rules {
            match rule.applies_to(invocation) {
                Ok(true) => return Some(Verdict::Rule(rule)),
                Ok(false) => {}
                Err(message) => return Some(Verdict::Broken(message)),
            }
        }
        None
    }

    /// The decision of the first rule that applies, by tool name only (no
    /// arguments, so conditions see an empty object).
    pub(crate) fn evaluate(&self, tool_name: &str) -> Option<PolicyDecision> {
        match self.verdict(&ToolInvocation::new(
            tool_name,
            serde_json::Value::Object(Default::default()),
        ))? {
            Verdict::Rule(rule) => Some(rule.decision),
            Verdict::Broken(_) => Some(PolicyDecision::Deny),
        }
    }
}

/// Combines the policy verdict with the optional pre-tool hook into a final
/// decision. This is the single decision point used for harness pre-tool
/// callbacks and tool confirmations.
///
/// When no policy rule matches and no hook is configured, the call is
/// allowed (default open — see the module docs).
pub(crate) fn decide(
    engine: &PolicyEngine,
    pre_tool: Option<&PreToolHook>,
    invocation: &ToolInvocation,
) -> PreToolDecision {
    decide_with_default(engine, pre_tool, invocation, PreToolDecision::Allow)
}

/// Like [`decide`], but with an explicit `unmatched` outcome for the
/// no-rule-matched / no-hook case. Used for *unrecognized* harness tool
/// confirmations, which fail closed instead of default-open: an unknown
/// builtin's confirmation is its only gate, so silently approving it would
/// bypass a restrictive policy set entirely.
pub(crate) fn decide_with_default(
    engine: &PolicyEngine,
    pre_tool: Option<&PreToolHook>,
    invocation: &ToolInvocation,
    unmatched: PreToolDecision,
) -> PreToolDecision {
    let hook_or = |fallback: PreToolDecision| match pre_tool {
        Some(hook) => hook(invocation),
        None => fallback,
    };
    let rule = match engine.verdict(invocation) {
        None => return hook_or(unmatched),
        Some(Verdict::Broken(message)) => return PreToolDecision::deny(message),
        Some(Verdict::Rule(rule)) => rule,
    };
    match rule.decision {
        PolicyDecision::Deny => PreToolDecision::deny(rule.deny_message(&invocation.name)),
        PolicyDecision::Confirm => match (&rule.handler, pre_tool) {
            (Some(handler), _) => {
                if handler(invocation, rule.reason.as_deref()) {
                    // Approved; the hook still gets a say, as on allow.
                    hook_or(PreToolDecision::Allow)
                } else {
                    PreToolDecision::deny(rule.reason.clone().unwrap_or_else(|| {
                        format!(
                            "Tool '{}' was not approved (policy '{}').",
                            invocation.name,
                            rule.label()
                        )
                    }))
                }
            }
            (None, Some(hook)) => hook(invocation),
            (None, None) => PreToolDecision::deny(format!(
                "Tool '{}' requires confirmation but no pre-tool hook is configured \
                 (failing closed).",
                invocation.name
            )),
        },
        // Policy allows; the hook still gets a say (defense in depth).
        PolicyDecision::Allow => hook_or(PreToolDecision::Allow),
    }
}

/// The verdict on a call the auto policy flagged: its handler's, or a
/// denial carrying the harness's assessment.
pub(crate) fn decide_flagged(
    engine: &PolicyEngine,
    invocation: &ToolInvocation,
    assessment: Option<&str>,
) -> PreToolDecision {
    let assessment = assessment.filter(|a| !a.trim().is_empty());
    let deny = || {
        PreToolDecision::deny(assessment.map_or_else(
            || format!("Tool '{}' was flagged by the auto policy.", invocation.name),
            |reason| {
                format!(
                    "Tool '{}' was flagged by the auto policy: {reason}",
                    invocation.name
                )
            },
        ))
    };
    match engine.auto().and_then(|auto| auto.handler.as_ref()) {
        Some(handler) if handler(invocation, assessment) => PreToolDecision::Allow,
        _ => deny(),
    }
}

/// Builds the harness-facing tool name for an MCP tool
/// (`mcp_<server>_<tool>` — the naming the harness itself uses).
#[must_use]
pub(crate) fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp_{server}_{tool}")
}

#[cfg(test)]
#[path = "policies_tests.rs"]
mod tests;
