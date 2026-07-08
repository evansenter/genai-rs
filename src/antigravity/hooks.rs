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

use serde_json::Value;
use std::sync::Arc;

/// A tool call the agent is about to make (or asking permission for).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ToolInvocation {
    /// The tool name (see the module docs for naming of builtins/MCP tools).
    pub name: String,
    /// The tool arguments.
    pub args: Value,
    /// The harness's correlation id for custom tool calls, when available.
    pub id: Option<String>,
}

/// The outcome of a completed tool call, passed to post-tool hooks.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ToolOutcome {
    /// The tool name.
    pub name: String,
    /// The result, when the call succeeded.
    pub result: Option<String>,
    /// The error, when the call failed.
    pub error: Option<String>,
}

/// A pre-tool hook's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreToolDecision {
    /// Let the tool run.
    Allow,
    /// Block the tool; the reason is surfaced to the model.
    Deny {
        /// Why the call was blocked.
        reason: String,
    },
}

impl PreToolDecision {
    /// A deny decision with the given reason.
    #[must_use]
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }
}

/// A synchronous pre-tool hook: inspect the call, return a verdict.
pub type PreToolHook = Arc<dyn Fn(&ToolInvocation) -> PreToolDecision + Send + Sync>;

/// A synchronous post-tool hook: observe completed tool calls.
pub type PostToolHook = Arc<dyn Fn(&ToolOutcome) + Send + Sync>;

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
mod tests {
    use super::*;
    use serde_json::json;

    fn invocation(name: &str) -> ToolInvocation {
        ToolInvocation {
            name: name.to_string(),
            args: json!({}),
            id: None,
        }
    }

    fn engine(policies: Vec<Policy>) -> PolicyEngine {
        PolicyEngine::new(policies)
    }

    #[test]
    fn test_policy_decision_table() {
        use PolicyDecision::{Allow, Confirm, Deny};
        // (policies, tool, expected)
        let cases: Vec<(Vec<Policy>, &str, Option<PolicyDecision>)> = vec![
            // Empty set: no decision (default open).
            (vec![], "anything", None),
            // Simple exact matches.
            (vec![policy::allow("a")], "a", Some(Allow)),
            (vec![policy::deny("a")], "a", Some(Deny)),
            (vec![policy::confirm("a")], "a", Some(Confirm)),
            (vec![policy::allow("a")], "b", None),
            // Wildcards.
            (vec![policy::allow_all()], "x", Some(Allow)),
            (vec![policy::deny_all()], "x", Some(Deny)),
            // Specific beats wildcard, regardless of registration order.
            (
                vec![policy::deny_all(), policy::allow("get_weather")],
                "get_weather",
                Some(Allow),
            ),
            (
                vec![policy::deny_all(), policy::allow("get_weather")],
                "run_command",
                Some(Deny),
            ),
            (
                vec![policy::allow_all(), policy::deny("run_command")],
                "run_command",
                Some(Deny),
            ),
            (
                vec![policy::allow_all(), policy::deny("run_command")],
                "view_file",
                Some(Allow),
            ),
            // First match wins within a tier.
            (vec![policy::deny("t"), policy::allow("t")], "t", Some(Deny)),
            (
                vec![policy::allow("t"), policy::deny("t")],
                "t",
                Some(Allow),
            ),
            (
                vec![policy::deny_all(), policy::allow_all()],
                "t",
                Some(Deny),
            ),
            // Confirm on specific, allow-all fallback.
            (
                vec![policy::confirm("run_command"), policy::allow_all()],
                "run_command",
                Some(Confirm),
            ),
            (
                vec![policy::confirm("run_command"), policy::allow_all()],
                "edit_file",
                Some(Allow),
            ),
            // MCP tool naming.
            (
                vec![
                    policy::deny_all(),
                    policy::allow(mcp_tool_name("git", "status")),
                ],
                "mcp_git_status",
                Some(Allow),
            ),
            (
                vec![
                    policy::deny_all(),
                    policy::allow(mcp_tool_name("git", "status")),
                ],
                "mcp_git_push",
                Some(Deny),
            ),
        ];

        for (policies, tool, expected) in cases {
            let engine = engine(policies.clone());
            assert_eq!(
                engine.evaluate(tool),
                expected,
                "policies {policies:?} evaluating {tool:?}"
            );
        }
    }

    #[test]
    fn test_decide_no_policies_no_hook_allows() {
        let engine = engine(vec![]);
        assert_eq!(
            decide(&engine, None, &invocation("t")),
            PreToolDecision::Allow
        );
    }

    #[test]
    fn test_decide_policy_deny_short_circuits_hook() {
        let engine = engine(vec![policy::deny_all()]);
        let hook: PreToolHook = Arc::new(|_| PreToolDecision::Allow);
        let decision = decide(&engine, Some(&hook), &invocation("t"));
        assert!(matches!(decision, PreToolDecision::Deny { .. }));
    }

    #[test]
    fn test_decide_policy_allow_still_consults_hook() {
        let engine = engine(vec![policy::allow_all()]);
        let hook: PreToolHook = Arc::new(|inv| {
            if inv.name == "blocked" {
                PreToolDecision::deny("hook says no")
            } else {
                PreToolDecision::Allow
            }
        });
        assert_eq!(
            decide(&engine, Some(&hook), &invocation("fine")),
            PreToolDecision::Allow
        );
        assert_eq!(
            decide(&engine, Some(&hook), &invocation("blocked")),
            PreToolDecision::deny("hook says no")
        );
    }

    #[test]
    fn test_decide_confirm_without_hook_fails_closed() {
        let engine = engine(vec![policy::confirm("danger")]);
        let decision = decide(&engine, None, &invocation("danger"));
        let PreToolDecision::Deny { reason } = decision else {
            panic!("confirm without hook must deny");
        };
        assert!(reason.contains("failing closed"));
    }

    #[test]
    fn test_decide_confirm_defers_to_hook() {
        let engine = engine(vec![policy::confirm("danger")]);
        let approve: PreToolHook = Arc::new(|_| PreToolDecision::Allow);
        let reject: PreToolHook = Arc::new(|_| PreToolDecision::deny("nope"));
        assert_eq!(
            decide(&engine, Some(&approve), &invocation("danger")),
            PreToolDecision::Allow
        );
        assert_eq!(
            decide(&engine, Some(&reject), &invocation("danger")),
            PreToolDecision::deny("nope")
        );
    }

    #[test]
    fn test_decide_hook_receives_invocation_details() {
        let engine = engine(vec![]);
        let hook: PreToolHook = Arc::new(|inv| {
            assert_eq!(inv.name, "run_command");
            assert_eq!(inv.args["commandLine"], "ls");
            assert_eq!(inv.id.as_deref(), Some("call-7"));
            PreToolDecision::Allow
        });
        let inv = ToolInvocation {
            name: "run_command".to_string(),
            args: json!({"commandLine": "ls"}),
            id: Some("call-7".to_string()),
        };
        assert_eq!(decide(&engine, Some(&hook), &inv), PreToolDecision::Allow);
    }

    #[test]
    fn test_decide_with_default_deny_applies_only_when_unmatched() {
        let closed = || PreToolDecision::deny("unrecognized");
        // No rules, no hook: the unmatched outcome applies.
        assert!(matches!(
            decide_with_default(&engine(vec![]), None, &invocation("t"), closed()),
            PreToolDecision::Deny { .. }
        ));
        // A non-matching exact rule is still unmatched for this tool.
        assert!(matches!(
            decide_with_default(
                &engine(vec![policy::allow("other")]),
                None,
                &invocation("t"),
                closed()
            ),
            PreToolDecision::Deny { .. }
        ));
        // A matching wildcard allow wins over the unmatched outcome.
        assert_eq!(
            decide_with_default(
                &engine(vec![policy::allow_all()]),
                None,
                &invocation("t"),
                closed()
            ),
            PreToolDecision::Allow
        );
        // A hook still decides when no rule matches.
        let hook: PreToolHook = Arc::new(|_| PreToolDecision::Allow);
        assert_eq!(
            decide_with_default(&engine(vec![]), Some(&hook), &invocation("t"), closed()),
            PreToolDecision::Allow
        );
    }

    #[test]
    fn test_mcp_tool_name_format() {
        assert_eq!(mcp_tool_name("git", "status"), "mcp_git_status");
    }
}
