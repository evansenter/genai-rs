use super::*;
use serde_json::json;
use std::sync::Arc;

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
