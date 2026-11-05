use super::*;
use serde_json::json;
use std::sync::Arc;

fn invocation(name: &str) -> ToolInvocation {
    ToolInvocation::new(name, json!({}))
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
        // Within a tier deny beats ask beats allow, whatever the order
        // (the harness evaluator's order, verified on 0.1.23).
        (vec![policy::deny("t"), policy::allow("t")], "t", Some(Deny)),
        (vec![policy::allow("t"), policy::deny("t")], "t", Some(Deny)),
        (
            vec![policy::allow("t"), policy::confirm("t")],
            "t",
            Some(Confirm),
        ),
        (
            vec![policy::allow_all(), policy::deny_all()],
            "t",
            Some(Deny),
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

fn call(name: &str, args: serde_json::Value) -> ToolInvocation {
    ToolInvocation::new(name, args)
}

#[test]
fn test_when_condition_falls_through_to_the_next_rule() {
    // Deny `rm` commands, allow the rest: the conditional deny outranks the
    // allow, and a call it rejects falls through to it.
    let engine = engine(vec![
        policy::allow("run_command"),
        policy::deny("run_command")
            .when(|c| {
                c.args["CommandLine"]
                    .as_str()
                    .is_none_or(|cmd| cmd.contains("rm "))
            })
            .with_reason("no deletions"),
    ]);
    assert_eq!(
        decide(
            &engine,
            None,
            &call("run_command", json!({"CommandLine": "rm -rf /"}))
        ),
        PreToolDecision::deny("no deletions")
    );
    assert_eq!(
        decide(
            &engine,
            None,
            &call("run_command", json!({"CommandLine": "ls"}))
        ),
        PreToolDecision::Allow
    );
    // No condition matched and nothing else applies: default open.
    let only_conditional = PolicyEngine::new(vec![policy::deny("run_command").when(|_| false)]);
    assert!(
        only_conditional
            .verdict(&invocation("run_command"))
            .is_none()
    );
    assert!(only_conditional.has_callbacks());
}

#[test]
fn test_a_panicking_condition_fails_closed() {
    let engine = engine(vec![
        policy::allow_all(),
        policy::allow("view_file")
            .with_name("fragile")
            .when(|_| panic!("condition bug")),
    ]);
    let PreToolDecision::Deny { reason } = decide(&engine, None, &invocation("view_file")) else {
        panic!("a panicking condition must deny");
    };
    assert!(reason.contains("'fragile' panicked"), "{reason}");
}

#[test]
fn test_ask_user_handler_decides_with_the_rule_reason() {
    let engine = engine(vec![
        policy::ask_user("run_command", |c, reason| {
            assert_eq!(reason, Some("shell needs approval"));
            c.args["CommandLine"] == "ls"
        })
        .with_reason("shell needs approval"),
    ]);
    assert_eq!(
        decide(
            &engine,
            None,
            &call("run_command", json!({"CommandLine": "ls"}))
        ),
        PreToolDecision::Allow
    );
    assert_eq!(
        decide(
            &engine,
            None,
            &call("run_command", json!({"CommandLine": "rm"}))
        ),
        PreToolDecision::deny("shell needs approval")
    );
    // An approval still lets the pre-tool hook veto (defense in depth).
    let veto: PreToolHook = Arc::new(|_| PreToolDecision::deny("hook veto"));
    assert_eq!(
        decide(
            &engine,
            Some(&veto),
            &call("run_command", json!({"CommandLine": "ls"}))
        ),
        PreToolDecision::deny("hook veto")
    );
}

#[test]
fn test_rule_name_and_reason_shape_the_denial() {
    let named = engine(vec![policy::deny("t").with_name("no-t")]);
    assert_eq!(
        decide(&named, None, &invocation("t")),
        PreToolDecision::deny("Denied by policy 'no-t' for tool 't'.")
    );
    let reasoned = engine(vec![policy::deny_all().with_reason("read-only agent")]);
    assert_eq!(
        decide(&reasoned, None, &invocation("anything")),
        PreToolDecision::deny("read-only agent")
    );
}

#[test]
fn test_server_wide_mcp_rules_sit_between_named_and_wildcard() {
    use PolicyDecision::{Allow, Deny};
    let rules = || {
        vec![
            policy::allow_all(),
            policy::deny("mcp_git_*"),
            policy::allow(mcp_tool_name("git", "status")),
        ]
    };
    assert_eq!(engine(rules()).evaluate("mcp_git_status"), Some(Allow));
    assert_eq!(engine(rules()).evaluate("mcp_git_push"), Some(Deny));
    assert_eq!(engine(rules()).evaluate("mcp_github_push"), Some(Allow));
    assert_eq!(engine(rules()).evaluate("view_file"), Some(Allow));
    // `mcp_*` covers every MCP tool, at the server-rule tier.
    let all_mcp = || vec![policy::allow_all(), policy::deny("mcp_*")];
    assert_eq!(engine(all_mcp()).evaluate("mcp_git_x"), Some(Deny));
    assert_eq!(engine(all_mcp()).evaluate("view_file"), Some(Allow));
    // Any other `*` suffix is a literal name.
    assert_eq!(
        engine(vec![policy::deny("view_*")]).evaluate("view_file"),
        None
    );
}

#[test]
fn test_start_subagent_targets_invoke_subagent() {
    let rule = policy::deny("start_subagent");
    assert_eq!(rule.target(), "invoke_subagent");
    assert_eq!(
        engine(vec![rule]).evaluate("invoke_subagent"),
        Some(PolicyDecision::Deny)
    );
}

#[test]
fn test_auto_policy_is_kept_out_of_client_evaluation() {
    let engine = engine(vec![policy::auto(), policy::allow("view_file")]);
    assert!(engine.auto().is_some_and(Policy::is_auto));
    assert_eq!(engine.evaluate("run_command"), None, "auto is harness-side");
    // A flagged call is denied with the assessment unless a handler approves.
    let flagged = call("run_command", json!({"CommandLine": "rm -rf build"}));
    let PreToolDecision::Deny { reason } =
        decide_flagged(&engine, &flagged, Some("deletes a directory"))
    else {
        panic!("a flagged call without a handler must be denied");
    };
    assert!(reason.contains("deletes a directory"), "{reason}");
    let approving = PolicyEngine::new(vec![policy::auto().with_handler(|c, reason| {
        reason == Some("deletes a directory") && c.name == "run_command"
    })]);
    assert_eq!(
        decide_flagged(&approving, &flagged, Some("deletes a directory")),
        PreToolDecision::Allow
    );
}

#[test]
fn test_policy_debug_omits_closures() {
    let rendered = format!("{:?}", policy::deny("t").when(|_| true));
    assert!(rendered.contains("when: true"), "{rendered}");
}
