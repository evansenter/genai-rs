//! Tool policies and lifecycle hooks for Antigravity agents.
//!
//! Policies are declarative allow/deny/confirm rules over tool names,
//! evaluated **Rust-side before every dispatch decision** (defense in depth:
//! the harness additionally enforces hook verdicts on its side of the wire).
//!
//! # Evaluation order
//!
//! Mirroring the reference SDK's priority model, **exact-name rules beat
//! wildcard rules**; within the same specificity tier the first registered
//! matching rule wins. This makes the natural
//! `[deny_all(), allow("get_weather")]` registration order behave as
//! intended: `get_weather` is allowed, everything else denied.
//!
//! When no rule matches, the call is **allowed** (default-open, matching the
//! reference SDK). Note that [`AgentBuilder::spawn`](super::AgentBuilder)
//! refuses to start an agent with write-capable builtins or MCP servers and
//! no policy or pre-tool hook at all, so default-open only applies once you
//! have opted into a policy set.
//!
//! # Targets
//!
//! - Custom tools and builtins are matched by name (builtins use their wire
//!   names: `run_command`, `edit_file`, `create_file`, `view_file`,
//!   `list_directory`, `search_directory`, `find_file`, `search_web`,
//!   `generate_image`, `start_subagent`, `ask_question`, `finish`).
//! - MCP tools are matched as `mcp_<server>_<tool>` (the harness's naming).
//! - `"*"` matches everything.
//!
//! `ask_question` is policy-gated like the rest on harness 0.1.18 — the
//! call passes through the pre-tool hook before the question is posed, so
//! a `deny_all()` set needs an explicit `allow("ask_question")`. (On 0.1.10
//! question requests bypassed the policy engine.) The answers themselves
//! come from the
//! [`on_questions`](crate::antigravity::AgentBuilder::on_questions) hook.

use super::{PreToolDecision, PreToolHook, ToolInvocation};

/// What a matched policy decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PolicyDecision {
    /// Approve the call.
    Allow,
    /// Reject the call.
    Deny,
    /// Defer to the agent's pre-tool hook. With no hook configured the call
    /// is **denied** (fail closed).
    Confirm,
}

/// One tool policy rule. Build with the [`policy`] constructors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    target: String,
    decision: PolicyDecision,
}

impl Policy {
    /// The tool name (or `"*"`) this rule targets.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The decision this rule produces when matched.
    #[must_use]
    pub fn decision(&self) -> PolicyDecision {
        self.decision
    }

    fn is_wildcard(&self) -> bool {
        self.target == WILDCARD
    }

    fn matches(&self, tool_name: &str) -> bool {
        self.target == WILDCARD || self.target == tool_name
    }
}

const WILDCARD: &str = "*";

/// Constructors for [`Policy`] rules.
pub mod policy {
    use super::{Policy, PolicyDecision, WILDCARD};

    /// Approves calls to the named tool.
    #[must_use]
    pub fn allow(tool: impl Into<String>) -> Policy {
        Policy {
            target: tool.into(),
            decision: PolicyDecision::Allow,
        }
    }

    /// Rejects calls to the named tool.
    #[must_use]
    pub fn deny(tool: impl Into<String>) -> Policy {
        Policy {
            target: tool.into(),
            decision: PolicyDecision::Deny,
        }
    }

    /// Defers calls to the named tool to the agent's `on_pre_tool` hook.
    /// With no hook configured, matched calls are denied (fail closed).
    #[must_use]
    pub fn confirm(tool: impl Into<String>) -> Policy {
        Policy {
            target: tool.into(),
            decision: PolicyDecision::Confirm,
        }
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
}

/// Evaluates an ordered policy set against tool names.
#[derive(Debug, Clone, Default)]
pub(crate) struct PolicyEngine {
    policies: Vec<Policy>,
}

impl PolicyEngine {
    pub(crate) fn new(policies: Vec<Policy>) -> Self {
        Self { policies }
    }

    /// Returns the decision of the highest-priority matching rule, or `None`
    /// when no rule matches (callers treat that as allow — default open).
    pub(crate) fn evaluate(&self, tool_name: &str) -> Option<PolicyDecision> {
        // Exact-name rules beat wildcards; first match wins within a tier.
        self.policies
            .iter()
            .find(|p| !p.is_wildcard() && p.matches(tool_name))
            .or_else(|| self.policies.iter().find(|p| p.is_wildcard()))
            .map(Policy::decision)
    }
}

/// Combines the policy verdict with the optional pre-tool hook into a final
/// allow/deny decision. This is the single decision point used for custom
/// tool calls, harness-side tool confirmations, and pre-tool hook callbacks.
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
    match engine.evaluate(&invocation.name) {
        Some(PolicyDecision::Deny) => {
            PreToolDecision::deny(format!("Denied by policy for tool '{}'.", invocation.name))
        }
        Some(PolicyDecision::Confirm) => match pre_tool {
            Some(hook) => hook(invocation),
            None => PreToolDecision::deny(format!(
                "Tool '{}' requires confirmation but no pre-tool hook is configured \
                 (failing closed).",
                invocation.name
            )),
        },
        Some(PolicyDecision::Allow) => {
            // Policy allows; the hook still gets a say (defense in depth).
            match pre_tool {
                Some(hook) => hook(invocation),
                None => PreToolDecision::Allow,
            }
        }
        None => {
            // No rule matched: the hook decides, else the caller's
            // unmatched outcome applies (Allow for known tools —
            // default open; Deny for unrecognized confirmations).
            match pre_tool {
                Some(hook) => hook(invocation),
                None => unmatched,
            }
        }
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
