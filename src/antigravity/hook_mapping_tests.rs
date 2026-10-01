use super::*;
use crate::antigravity::protocol::StepState;
use crate::antigravity::turn::TurnState;
use crate::antigravity::{AgentEvent, AntigravityAgent, Policy, ToolDecision, policy};
use std::sync::Arc;

/// `n` three-choice single-select questions for mapping tests.
fn question_batch(n: usize) -> Vec<AgentQuestion> {
    (0..n)
        .map(|i| AgentQuestion::new(format!("q{i}"), ["a", "b", "c"], false))
        .collect()
}

#[test]
fn on_questions_populates_the_builder_hook() {
    // Plumbing guard: the seven mapping tests would all stay green if
    // the builder simply dropped the hook (silently reverting to the
    // hookless "unanswered" fallback). Pin that on_questions stores
    // the closure it is given — spawn() moves this field to the agent
    // verbatim, so the builder side is the observable half without a
    // live harness.
    let builder = AntigravityAgent::builder().on_questions(|_| QuestionReply::Cancel);
    let hook = builder.questions.as_ref().expect("hook stored");
    assert_eq!(hook(&question_batch(1)), QuestionReply::Cancel);
    assert!(
        AntigravityAgent::builder().questions.is_none(),
        "no hook by default"
    );
}

#[test]
fn question_reply_cancel_sets_only_cancelled() {
    let response = map_question_reply(Some(QuestionReply::Cancel), &question_batch(2));
    assert_eq!(response.cancelled, Some(true));
    assert!(
        response.response.is_none(),
        "cancel must not also emit a response (protocol oneof)"
    );
}

#[test]
fn question_reply_exact_answers_map_to_protocol() {
    let response = map_question_reply(
        Some(QuestionReply::Answers(vec![
            hooks::QuestionAnswer::Choices {
                selected: vec![1, 2],
                freeform: None,
            },
            hooks::QuestionAnswer::Freeform("details".into()),
        ])),
        &question_batch(2),
    );
    assert!(response.cancelled.is_none());
    let answers = response.response.expect("answers present").answers;
    assert_eq!(answers.len(), 2);
    let first = answers[0].multiple_choice_answer.as_ref().unwrap();
    assert_eq!(first.selected_choice_indices, vec![1, 2]);
    assert!(first.freeform_response.is_none());
    let second = answers[1].multiple_choice_answer.as_ref().unwrap();
    assert!(second.selected_choice_indices.is_empty());
    assert_eq!(second.freeform_response.as_deref(), Some("details"));
    assert!(answers[1].unanswered.is_none());
}

#[test]
fn question_reply_short_list_pads_with_unanswered() {
    let response = map_question_reply(
        Some(QuestionReply::Answers(vec![
            hooks::QuestionAnswer::Freeform("only one".into()),
        ])),
        &question_batch(3),
    );
    let answers = response.response.expect("answers present").answers;
    assert_eq!(answers.len(), 3, "padded to the question count");
    assert!(answers[0].multiple_choice_answer.is_some());
    assert_eq!(answers[1].unanswered, Some(true));
    assert_eq!(answers[2].unanswered, Some(true));
}

#[test]
fn question_reply_long_list_truncates() {
    let response = map_question_reply(
        Some(QuestionReply::Answers(vec![
            hooks::QuestionAnswer::Unanswered,
            hooks::QuestionAnswer::Freeform("extra".into()),
        ])),
        &question_batch(1),
    );
    let answers = response.response.expect("answers present").answers;
    assert_eq!(answers.len(), 1, "truncated to the question count");
    assert_eq!(answers[0].unanswered, Some(true));
}

#[test]
fn map_questions_extracts_multiple_choice_fields() {
    let request = protocol::UserQuestionsRequest {
        questions: vec![protocol::UserQuestion {
            multiple_choice: Some(protocol::MultipleChoice {
                question: Some("Pick one".into()),
                choices: vec!["a".into(), "b".into()],
                is_multi_select: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let batch = map_questions(&request);
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].question, "Pick one");
    assert_eq!(batch[0].choices, vec!["a".to_string(), "b".to_string()]);
    assert!(batch[0].is_multi_select);
}

#[test]
fn map_questions_defaults_when_fields_absent() {
    let request = protocol::UserQuestionsRequest {
        questions: vec![
            // No multiple_choice at all: a future question type whose
            // payload rides in `extra`.
            protocol::UserQuestion {
                multiple_choice: None,
                extra: {
                    let mut m = serde_json::Map::new();
                    m.insert("freeText".into(), serde_json::json!({"maxLen": 80}));
                    m
                },
            },
            // multiple_choice present but sparse: is_multi_select unset.
            protocol::UserQuestion {
                multiple_choice: Some(protocol::MultipleChoice {
                    question: Some("Sparse".into()),
                    choices: Vec::new(),
                    is_multi_select: None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            // multiple_choice present with choices but no question text:
            // the hook sees an empty question above real choices (warned).
            protocol::UserQuestion {
                multiple_choice: Some(protocol::MultipleChoice {
                    question: None,
                    choices: vec!["yes".into(), "no".into()],
                    is_multi_select: None,
                    // An unmodeled field *inside* multipleChoice must
                    // also reach the hook (merged into extra).
                    extra: {
                        let mut m = serde_json::Map::new();
                        m.insert("defaultChoiceIndex".into(), serde_json::json!(1));
                        // Same key at both levels: the inner value must
                        // win the merge (the documented precedence).
                        m.insert("collides".into(), serde_json::json!("inner"));
                        m
                    },
                }),
                extra: {
                    let mut m = serde_json::Map::new();
                    m.insert("collides".into(), serde_json::json!("outer"));
                    m
                },
            },
        ],
        ..Default::default()
    };
    let batch = map_questions(&request);
    assert_eq!(batch.len(), 3);
    assert!(batch[0].question.is_empty());
    assert!(batch[0].choices.is_empty());
    assert!(!batch[0].is_multi_select);
    // The unknown shape is programmatically distinguishable from a
    // genuinely empty multiple-choice question, and the raw payload
    // rides along for the hook to inspect.
    assert!(batch[0].is_unknown_type());
    assert_eq!(
        batch[0].extra["freeText"],
        serde_json::json!({"maxLen": 80})
    );
    assert_eq!(batch[1].question, "Sparse");
    assert!(!batch[1].is_multi_select);
    assert!(!batch[1].is_unknown_type());
    // Blank question text above real choices: the substitution is
    // observable to the hook (and warned at map time).
    assert!(batch[2].question.is_empty());
    assert_eq!(batch[2].choices, vec!["yes", "no"]);
    assert!(!batch[2].is_unknown_type());
    // An unmodeled field nested inside multipleChoice merges into the
    // hook-facing extra map (lossless one level down too).
    assert_eq!(batch[2].extra["defaultChoiceIndex"], 1);
    // Collision precedence pinned like the other two extra maps in
    // this PR: the inner (multipleChoice-level) value wins the merge.
    assert_eq!(batch[2].extra["collides"], "inner");
}

#[test]
fn question_reply_hookless_fallback_answers_unanswered() {
    let response = map_question_reply(None, &question_batch(2));
    assert!(response.cancelled.is_none());
    let answers = response.response.expect("answers present").answers;
    assert_eq!(answers.len(), 2);
    assert!(answers.iter().all(|a| a.unanswered == Some(true)));
}

#[test]
fn tool_error_normalization_treats_blank_as_success() {
    // The harness sends `"error": ""` (protobuf's default for an unset
    // string) on calls that SUCCEEDED, so passing it through verbatim
    // reported every successful builtin as a failure to the
    // `is_some()` check the field's docs invite.
    assert_eq!(normalize_tool_error(Some(String::new())), None);
    assert_eq!(normalize_tool_error(Some("   ".to_string())), None);
    assert_eq!(normalize_tool_error(None), None);
    assert_eq!(
        normalize_tool_error(Some("boom".to_string())),
        Some("boom".to_string())
    );
}

/// `PreToolArgs` exactly as harness 0.1.18 sends them (from a
/// `LOUD_WIRE` capture), minus ids.
fn pre_tool_args(tool: &str, server: &str, arguments_json: &str) -> protocol::PreToolArgs {
    protocol::PreToolArgs {
        tool_name: Some(tool.to_string()),
        server_name: Some(server.to_string()),
        arguments_json: Some(arguments_json.to_string()),
        call_id: Some("call_1".to_string()),
        ..Default::default()
    }
}

#[test]
fn test_pre_tool_hook_names_mcp_tools_by_policy_target() {
    // Regression: the harness sends the bare MCP tool name with the
    // server beside it, so `deny("mcp_widgets_lookup_widget_code")`
    // never matched on the hook path — the only gate MCP calls get.
    let invocation = pre_tool_invocation(&pre_tool_args(
        "lookup_widget_code",
        "widgets",
        r#"{"Arguments":{"name":"flange"},"ServerName":"widgets","ToolName":"lookup_widget_code"}"#,
    ));
    assert_eq!(invocation.name, "mcp_widgets_lookup_widget_code");
    // The wrapper is peeled: hooks see what the model passed, the
    // same shape `ToolAction::args` reports for the MCP step.
    assert_eq!(invocation.args, serde_json::json!({"name": "flange"}));
    assert_eq!(invocation.id.as_deref(), Some("call_1"));

    let denied = hooks::decide(
        &engine(vec![
            policy::allow_all(),
            policy::deny("mcp_widgets_lookup_widget_code"),
        ]),
        None,
        &invocation,
    );
    assert!(matches!(denied, PreToolDecision::Deny { .. }));
}

#[test]
fn test_pre_tool_hook_maps_the_subagent_builtin_to_its_public_name() {
    // Regression: the subagent builtin arrives as its step field name.
    let invocation = pre_tool_invocation(&pre_tool_args(
        "invoke_subagent",
        "",
        r#"{"Subagents":[{"TypeName":"haiku-writer"}]}"#,
    ));
    assert_eq!(invocation.name, "start_subagent");
    // Not an MCP call (blank server), so args are left as sent.
    assert_eq!(invocation.args["Subagents"][0]["TypeName"], "haiku-writer");

    // Every other builtin and custom tool passes through untouched.
    for name in ["view_file", "run_command", "antigravity_test_weather"] {
        assert_eq!(
            pre_tool_invocation(&pre_tool_args(name, "", "{}")).name,
            name
        );
    }
}

#[test]
fn test_pre_tool_hook_leaves_unwrapped_mcp_arguments_alone() {
    // Only the harness's wrapper (recognized by its ToolName sibling)
    // is peeled; a tool whose own parameter is called `Arguments` is
    // not mangled.
    let invocation = pre_tool_invocation(&pre_tool_args(
        "t",
        "srv",
        r#"{"Arguments":{"x":1},"other":2}"#,
    ));
    assert_eq!(
        invocation.args,
        serde_json::json!({"Arguments": {"x": 1}, "other": 2})
    );
}

// -------------------------------------------------------------------
// Tool-confirmation decisions (see `confirmation_decision`)
// -------------------------------------------------------------------

/// A waiting step carrying a `run_command` action confirmation.
fn run_command_confirmation_step() -> StepUpdate {
    StepUpdate {
        state: Some(StepState::WaitingForUser),
        request_text: Some("run `ls`?".to_string()),
        tool_confirmation_request: Some(protocol::ToolConfirmationRequest::default()),
        run_command: Some(protocol::ActionRunCommand {
            command_line: Some("ls".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A waiting confirmation step with no action payload at all (a
/// pre-request notification for a host-side tool).
fn pre_request_confirmation_step() -> StepUpdate {
    StepUpdate {
        state: Some(StepState::WaitingForUser),
        tool_confirmation_request: Some(protocol::ToolConfirmationRequest::default()),
        ..Default::default()
    }
}

/// A waiting confirmation step whose action landed in `extra` — the
/// shape a harness builtin newer than this client produces.
fn unknown_action_confirmation_step() -> StepUpdate {
    let mut step = pre_request_confirmation_step();
    step.request_text = Some("do the new thing?".to_string());
    step.extra.insert(
        "deleteEverything".to_string(),
        serde_json::json!({"target": "/"}),
    );
    step
}

fn engine(policies: Vec<Policy>) -> PolicyEngine {
    PolicyEngine::new(policies)
}

/// Whether a confirmation is accepted (the decision maps to `Allow`).
fn confirmation_accepted(
    step: &StepUpdate,
    engine: &PolicyEngine,
    pre_tool: Option<&PreToolHook>,
) -> bool {
    matches!(
        confirmation_decision(step, engine, pre_tool),
        PreToolDecision::Allow
    )
}

#[test]
fn test_confirmation_denied_for_deny_policied_known_tool() {
    let step = run_command_confirmation_step();
    // Exact deny and wildcard deny must both reject (accepted=false).
    assert!(!confirmation_accepted(
        &step,
        &engine(vec![policy::deny("run_command")]),
        None
    ));
    assert!(!confirmation_accepted(
        &step,
        &engine(vec![policy::deny_all()]),
        None
    ));
}

#[test]
fn test_confirmation_allowed_for_allowed_known_tool() {
    let step = run_command_confirmation_step();
    assert!(confirmation_accepted(
        &step,
        &engine(vec![policy::allow_all()]),
        None
    ));
    assert!(confirmation_accepted(
        &step,
        &engine(vec![policy::deny_all(), policy::allow("run_command")]),
        None
    ));
}

#[test]
fn test_confirmation_hook_sees_known_tool_args_and_request_text() {
    let step = run_command_confirmation_step();
    let hook: PreToolHook = Arc::new(|invocation| {
        assert_eq!(invocation.name, "run_command");
        assert_eq!(invocation.args["commandLine"], "ls");
        assert_eq!(invocation.args["request_text"], "run `ls`?");
        PreToolDecision::deny("hook says no")
    });
    assert!(!confirmation_accepted(&step, &engine(vec![]), Some(&hook)));
}

#[test]
fn test_confirmation_pre_request_auto_approved_even_under_deny_all() {
    // No action payload and no unknown fields: a pre-request
    // notification. The concrete call gets its own policy check, so
    // this is approved regardless of policy (mirrors the reference SDK).
    let step = pre_request_confirmation_step();
    assert!(confirmation_accepted(
        &step,
        &engine(vec![policy::deny_all()]),
        None
    ));
    assert!(confirmation_accepted(&step, &engine(vec![]), None));
}

#[test]
fn test_confirmation_unknown_action_allowed_only_under_allow_all() {
    let step = unknown_action_confirmation_step();
    // allow_all (wildcard) approves.
    assert!(confirmation_accepted(
        &step,
        &engine(vec![policy::allow_all()]),
        None
    ));
    // Restrictive policy sets fail closed.
    assert!(!confirmation_accepted(
        &step,
        &engine(vec![policy::deny_all()]),
        None
    ));
    assert!(!confirmation_accepted(
        &step,
        &engine(vec![policy::deny_all(), policy::allow("run_command")]),
        None
    ));
    // No policies and no hook at all: still fail closed — an unknown
    // builtin's confirmation is its only gate.
    assert!(!confirmation_accepted(&step, &engine(vec![]), None));
}

#[test]
fn test_confirmation_unknown_action_matches_exact_rule_by_wire_field() {
    let step = unknown_action_confirmation_step();
    assert!(confirmation_accepted(
        &step,
        &engine(vec![policy::deny_all(), policy::allow("deleteEverything")]),
        None
    ));
    assert!(!confirmation_accepted(
        &step,
        &engine(vec![policy::allow_all(), policy::deny("deleteEverything")]),
        None
    ));
}

#[test]
fn test_confirmation_unknown_action_defers_to_hook() {
    let step = unknown_action_confirmation_step();
    let hook: PreToolHook = Arc::new(|invocation| {
        // The hook sees the unknown wire field as the tool name and the
        // preserved payload as args.
        assert_eq!(invocation.name, "deleteEverything");
        assert_eq!(invocation.args["deleteEverything"]["target"], "/");
        assert_eq!(invocation.args["request_text"], "do the new thing?");
        PreToolDecision::Allow
    });
    assert!(confirmation_accepted(&step, &engine(vec![]), Some(&hook)));

    let deny_hook: PreToolHook = Arc::new(|_| PreToolDecision::deny("nope"));
    assert!(!confirmation_accepted(
        &step,
        &engine(vec![]),
        Some(&deny_hook)
    ));
}

// -------------------------------------------------------------------
// Accepted/denied decision surfacing (Item: ToolAction decision marker)
// -------------------------------------------------------------------

#[test]
fn test_confirmation_decision_carries_denial_reason() {
    let step = run_command_confirmation_step();
    let decision = confirmation_decision(&step, &engine(vec![policy::deny_all()]), None);
    let PreToolDecision::Deny { reason } = decision else {
        panic!("expected a denial");
    };
    assert!(reason.contains("run_command"));
    // The allowed path yields `Allow` (no reason).
    assert_eq!(
        confirmation_decision(&step, &engine(vec![policy::allow_all()]), None),
        PreToolDecision::Allow
    );
}

#[test]
fn test_denied_confirmation_emits_denied_tool_action() {
    // A denied confirmation must surface as a ToolAction event carrying
    // the Denied decision and the trajectory id; an allowed one must not
    // emit at the confirmation step (it surfaces at its terminal step).
    let mut step = run_command_confirmation_step();
    step.trajectory_id = Some("traj-1".to_string());

    let denied = matches!(
        confirmation_decision(&step, &engine(vec![policy::deny_all()]), None),
        PreToolDecision::Deny { .. }
    );
    assert!(denied);
    // Mirror the emission the turn loop performs on a denial.
    let action = streaming::ToolAction::from_step(&step).unwrap();
    let event = AgentEvent::ToolAction {
        action: Box::new(action),
        decision: ToolDecision::Denied {
            reason: "Denied by policy for tool 'run_command'.".to_string(),
        },
        trajectory_id: step.trajectory_id.clone(),
    };
    let AgentEvent::ToolAction {
        decision,
        trajectory_id,
        ..
    } = &event
    else {
        panic!("expected a ToolAction");
    };
    assert!(decision.is_denied());
    assert_eq!(
        decision.denial_reason(),
        Some("Denied by policy for tool 'run_command'.")
    );
    assert_eq!(trajectory_id.as_deref(), Some("traj-1"));

    assert_eq!(
        confirmation_decision(&step, &engine(vec![policy::allow_all()]), None),
        PreToolDecision::Allow
    );
}

#[test]
fn test_denied_action_dedups_terminal_allowed_emission() {
    // The turn-loop invariant: a denied action is announced exactly once
    // (as Denied at its confirmation step) and is NOT re-announced as
    // Allowed when the same step later reaches its terminal delivery.
    // Both call sites route through `TurnState::announce_tool_action`
    // keyed on the same step_key, so exercise it directly (the send-side
    // I/O of `answer_tool_confirmation` is not needed to lock this).
    let mut turn = TurnState::new(None, None);
    let step = run_command_confirmation_step();
    let step_key = (
        step.trajectory_id.clone().unwrap_or_default(),
        step.step_index.unwrap_or_default(),
    );
    let action = || streaming::ToolAction::from_step(&step).unwrap();

    // Confirmation step: denial announced.
    assert!(turn.announce_tool_action(
        &step_key,
        action(),
        ToolDecision::Denied {
            reason: "blocked".to_string(),
        },
        step.trajectory_id.clone(),
    ));
    // Terminal step: the same step's Allowed announcement is suppressed.
    assert!(!turn.announce_tool_action(
        &step_key,
        action(),
        ToolDecision::Allowed,
        step.trajectory_id.clone(),
    ));

    // Exactly one event, and it is the Denied one.
    let actions: Vec<_> = turn
        .queue
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolAction { .. }))
        .collect();
    assert_eq!(actions.len(), 1);
    let AgentEvent::ToolAction { decision, .. } = actions[0] else {
        unreachable!();
    };
    assert!(decision.is_denied());

    // A distinct step (different key) is unaffected by the dedup.
    let other_key = ("traj-other".to_string(), 9);
    assert!(turn.announce_tool_action(
        &other_key,
        action(),
        ToolDecision::Allowed,
        Some("traj-other".to_string()),
    ));
}

// -------------------------------------------------------------------
// ToolOutcome result unwrapping (Item: unwrap the wire envelope)
// -------------------------------------------------------------------

#[test]
fn test_unwrap_result_value_unwraps_scalar_envelope() {
    // {"result": "<string>"} → the inner string, verbatim.
    assert_eq!(
        unwrap_result_value(&serde_json::json!({"result": "hello"})),
        "hello"
    );
    // {"result": <non-string>} → the inner value serialized.
    assert_eq!(
        unwrap_result_value(&serde_json::json!({"result": 42})),
        "42"
    );
    // A verbatim object (not a lone `result` key) passes through.
    assert_eq!(
        unwrap_result_value(&serde_json::json!({"echo": "hi"})),
        r#"{"echo":"hi"}"#
    );
    // A multi-key object that happens to contain `result` is not
    // unwrapped (data preservation).
    let multi = serde_json::json!({"result": 1, "other": 2});
    assert_eq!(unwrap_result_value(&multi), multi.to_string());
}

#[test]
fn test_unwrap_result_string_unwraps_only_the_envelope() {
    assert_eq!(unwrap_result_string(r#"{"result":"inner"}"#), "inner");
    assert_eq!(unwrap_result_string(r#"{"result":7}"#), "7");
    // Plain (non-JSON) harness output is handed back verbatim.
    assert_eq!(unwrap_result_string("plain text"), "plain text");
    // A non-envelope object stays verbatim.
    assert_eq!(unwrap_result_string(r#"{"a":1}"#), r#"{"a":1}"#);
}
