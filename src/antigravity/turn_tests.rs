use super::*;

fn trajectory_update(
    trajectory_id: Option<&str>,
    state: TrajectoryState,
    error: Option<&str>,
) -> protocol::TrajectoryStateUpdate {
    protocol::TrajectoryStateUpdate {
        trajectory_id: trajectory_id.map(str::to_string),
        state: Some(state),
        error: error.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn test_main_trajectory_idle_finishes_turn() {
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    turn.final_text = Some("done".to_string());
    let update = trajectory_update(Some("main"), TrajectoryState::Idle, None);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    assert!(turn.finished);
    assert!(matches!(
        turn.queue.pop_front(),
        Some(AgentEvent::Finished(_))
    ));
}

#[test]
fn stall_diagnosis_names_unrecognized_main_trajectory_states() {
    // The scenario nobody reproduces on purpose: the harness renames
    // the terminal state, the Evergreen path absorbs it as Unknown,
    // and the turn runs to its timeout. Pin that the timeout message
    // names the culprit instead of reporting a bare stall.
    let unknown: TrajectoryState =
        serde_json::from_value(serde_json::json!("STATE_SUPER_IDLE")).unwrap();
    assert!(unknown.is_unknown(), "fixture must be an Unknown variant");

    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("main"), unknown, None);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();

    assert!(!turn.finished, "an unknown state must not end the turn");
    let diagnosis = turn.stall_diagnosis("");
    assert!(
        diagnosis.contains("STATE_SUPER_IDLE"),
        "diagnosis must name the state, got: {diagnosis}"
    );
    assert!(
        diagnosis.contains("version mismatch"),
        "diagnosis must point at the likely cause, got: {diagnosis}"
    );
}

#[test]
fn stall_diagnosis_ignores_subagent_states_and_clean_turns() {
    // A clean turn keeps the plain operation name...
    let turn = TurnState::new(None, None);
    assert_eq!(turn.stall_diagnosis(""), "agent turn");

    // ...and a *subagent* trajectory going somewhere unrecognized is
    // not the parent's problem, so it must not pollute the parent's
    // diagnosis (the `is_main` half of the condition).
    let unknown: TrajectoryState =
        serde_json::from_value(serde_json::json!("STATE_SUPER_IDLE")).unwrap();
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("subagent-1"), unknown, None);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    assert_eq!(turn.stall_diagnosis(""), "agent turn");
}

#[test]
fn stall_diagnosis_names_a_harness_side_input_rejection() {
    // What every turn against 0.1.18 looked like before the userInput
    // fix: no trajectory events at all, and the only evidence on the
    // harness's stderr. Verbatim from that run.
    let stderr = "some earlier line\n\
         Failed to send InputEvent: failed to unmarshal InputEvent: proto: syntax \
         error (line 1:14): unexpected token \"hello\"\n\
         another line";
    let turn = TurnState::new(None, None);
    let diagnosis = turn.stall_diagnosis(stderr);
    assert!(
        diagnosis.contains("failed to unmarshal InputEvent"),
        "diagnosis must quote the harness's own line, got: {diagnosis}"
    );
    assert!(diagnosis.contains("version mismatch"), "got: {diagnosis}");
    // Unrelated stderr noise does not produce a diagnosis.
    assert_eq!(turn.stall_diagnosis("Stdin closed"), "agent turn");
}

#[test]
fn test_main_trajectory_cancelled_fails_turn() {
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("main"), TrajectoryState::Cancelled, Some("halted"));
    let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
    assert!(matches!(&err, AntigravityError::Turn(m) if m == "halted"));
}

#[test]
fn test_cancelled_before_any_step_fails_turn() {
    // No main trajectory yet (e.g. a pre-turn hook denial cancels the
    // turn before its first step): treated as the main trajectory.
    let mut turn = TurnState::new(None, None);
    let update = trajectory_update(Some("t-0"), TrajectoryState::Cancelled, None);
    let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
    assert!(matches!(&err, AntigravityError::Turn(m) if m == "turn cancelled"));
}

#[test]
fn test_subagent_trajectory_cancelled_is_not_fatal() {
    // A cancelled subagent trajectory must not fail the parent's turn
    // (mirroring the subagent-idle no-op); subagent failures surface
    // through their step errors instead.
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("sub"), TrajectoryState::Cancelled, Some("denied"));
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    assert!(!turn.finished);
    assert!(turn.queue.is_empty());
}

#[test]
fn test_subagent_trajectory_idle_does_not_finish_turn() {
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("sub"), TrajectoryState::Idle, None);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    assert!(!turn.finished);
    assert!(turn.queue.is_empty());
}

#[test]
fn test_subagent_trajectory_with_parent_never_claims_the_turn() {
    // 0.1.18 marks subagent trajectories with a parent id. Even when a
    // subagent event is the *first* one this turn sees (so the
    // first-seen fallback would have crowned it), it must not become
    // the main trajectory, and its idle must not finish the turn.
    let mut turn = TurnState::new(None, None);
    let mut sub = trajectory_update(Some("sub"), TrajectoryState::Idle, None);
    sub.parent_trajectory_id = Some("root".to_string());
    sub.depth = Some(1);
    AntigravityAgent::process_trajectory_update(&sub, &mut turn).unwrap();
    assert!(!turn.finished, "a subagent's idle ended the parent turn");
    assert_eq!(turn.main_trajectory, None);

    // The root's own update (no parent) establishes and finishes it.
    let root = trajectory_update(Some("root"), TrajectoryState::Idle, None);
    AntigravityAgent::process_trajectory_update(&root, &mut turn).unwrap();
    assert!(turn.finished);
    assert_eq!(turn.main_trajectory.as_deref(), Some("root"));
}

#[test]
fn test_blank_trajectory_error_does_not_fail_the_turn() {
    // 0.1.18 emits unpopulated strings explicitly; an `"error": ""` on
    // the terminal update is a normal completion, not a failure.
    let mut turn = TurnState::new(None, None);
    let update = trajectory_update(Some("root"), TrajectoryState::Idle, Some(""));
    AntigravityAgent::process_trajectory_update(&update, &mut turn)
        .expect("a blank error string is no error");
    assert!(turn.finished);
}

#[test]
fn test_stop_reason_surfaces_on_the_finished_response() {
    let mut turn = TurnState::new(None, None);
    let mut update = trajectory_update(Some("root"), TrajectoryState::Idle, None);
    update.stop_reason = Some(protocol::StopReason::QuotaExhausted);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    let Some(AgentEvent::Finished(response)) = turn.queue.pop_front() else {
        panic!("expected Finished");
    };
    assert_eq!(
        response.stop_reason(),
        Some(&protocol::StopReason::QuotaExhausted)
    );

    // UNSPECIFIED is the wire's "normal completion": not surfaced.
    let mut turn = TurnState::new(None, None);
    let mut update = trajectory_update(Some("root"), TrajectoryState::Idle, None);
    update.stop_reason = Some(protocol::StopReason::Unspecified);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    let Some(AgentEvent::Finished(response)) = turn.queue.pop_front() else {
        panic!("expected Finished");
    };
    assert_eq!(response.stop_reason(), None);

    // A subagent's stop reason is not the parent turn's.
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("root".to_string());
    let mut sub = trajectory_update(Some("sub"), TrajectoryState::Idle, None);
    sub.parent_trajectory_id = Some("root".to_string());
    sub.stop_reason = Some(protocol::StopReason::MaxToolCallsExceeded);
    AntigravityAgent::process_trajectory_update(&sub, &mut turn).unwrap();
    assert_eq!(turn.stop_reason, None);
}

/// The error step harness 0.1.18 sends for a hook-denied call (from a
/// `LOUD_WIRE` capture): the verdict's reason verbatim in
/// `errorMessage`, a decorated copy in the error action, no tool action.
fn hook_denied_step(trajectory: &str, index: u32, reason: &str) -> StepUpdate {
    StepUpdate {
        trajectory_id: Some(trajectory.to_string()),
        step_index: Some(index),
        state: Some(StepState::Error),
        source: Some(StepSource::Model),
        error_message: Some(reason.to_string()),
        error: Some(protocol::ActionError {
            error_message: Some(format!("{reason} (\"denied by pre-tool hook: {reason}\")")),
            http_code: Some(0),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn test_stream_deltas_carry_only_the_models_own_output() {
    // Shapes verbatim from a 0.1.18 capture of one custom-tool turn.
    let step = |source, target, text: &str, thinking: &str| StepUpdate {
        source: Some(source),
        target: Some(target),
        text_delta: Some(text.to_string()),
        thinking_delta: Some(thinking.to_string()),
        ..Default::default()
    };
    // The user's own prompt, echoed back as step 0.
    let echo = step(
        StepSource::User,
        StepTarget::Model,
        "What's the weather?",
        "",
    );
    assert_eq!(stream_deltas(&echo), (None, None));
    // A tool step's summary label, with the model's reasoning before it.
    let tool = step(
        StepSource::Model,
        StepTarget::Environment,
        "Weather check",
        "I should call the tool",
    );
    assert_eq!(stream_deltas(&tool), (Some("I should call the tool"), None));
    // The answer itself.
    let answer = step(StepSource::Model, StepTarget::User, "It is 17C.", "");
    assert_eq!(stream_deltas(&answer), (None, Some("It is 17C.")));
}

#[test]
fn test_post_tool_callback_for_a_dispatched_call_is_not_reported_twice() {
    // 0.1.18 sends a PostTool callback for custom tools too; the
    // dispatch path already fed on_post_tool, and every call doubled.
    let mut turn = TurnState::new(None, None);
    turn.reported_calls.insert("call_101866".to_string());
    assert!(
        turn.take_reported_call(Some("call_101866")),
        "the echo is a duplicate"
    );
    assert!(
        !turn.take_reported_call(Some("call_101866")),
        "consumed once"
    );
    // Builtins were never dispatched here, so their callbacks report.
    assert!(!turn.take_reported_call(Some("call_999")));
    assert!(!turn.take_reported_call(Some("")));
    assert!(!turn.take_reported_call(None));
}

#[test]
fn test_hook_denial_marks_its_error_step_denied() {
    let mut turn = TurnState::new(None, None);
    turn.hook_denials.push(HookDenial {
        trajectory_id: Some("root".to_string()),
        reason: "no widgets".to_string(),
    });

    // A different trajectory's error with the same text is not ours.
    assert_eq!(
        turn.take_hook_denial(&hook_denied_step("sub", 1, "no widgets")),
        None
    );
    // An unrelated error in the right trajectory is not ours either.
    assert_eq!(
        turn.take_hook_denial(&hook_denied_step("root", 1, "quota")),
        None
    );
    // The matching step claims it, exactly once.
    let step = hook_denied_step("root", 1, "no widgets");
    assert_eq!(turn.take_hook_denial(&step).as_deref(), Some("no widgets"));
    assert_eq!(turn.take_hook_denial(&step), None, "claimed once");
}

#[test]
fn test_hook_denied_step_surfaces_as_a_denied_tool_action() {
    // End to end through process_step's announce logic, minus the
    // socket: without the denial marker, a blocked call reads as an
    // executed one (`decision: Allowed`).
    let mut turn = TurnState::new(None, None);
    turn.hook_denials.push(HookDenial {
        trajectory_id: Some("root".to_string()),
        reason: "no widgets".to_string(),
    });
    let step = hook_denied_step("root", 1, "no widgets");
    let decision = match turn.take_hook_denial(&step) {
        Some(reason) => ToolDecision::Denied { reason },
        None => ToolDecision::Allowed,
    };
    let action = streaming::ToolAction::from_step(&step).expect("error action");
    assert!(turn.announce_tool_action(&("root".to_string(), 1), action, decision, None));
    let Some(AgentEvent::ToolAction {
        action, decision, ..
    }) = turn.queue.pop_front()
    else {
        panic!("expected a ToolAction");
    };
    assert_eq!(action.tool_name(), "error");
    assert_eq!(decision.denial_reason(), Some("no widgets"));
}

// -------------------------------------------------------------------
// Error severity classification (Item: classify harness error severity)
// -------------------------------------------------------------------

#[test]
fn test_classify_error_severity() {
    // No code / retryable / non-fatal codes → transient noise.
    assert_eq!(classify_error_severity(None), ErrorSeverity::Transient);
    assert_eq!(classify_error_severity(Some(500)), ErrorSeverity::Transient);
    assert_eq!(classify_error_severity(Some(429)), ErrorSeverity::Transient);
    // Fatal model-backend codes that reached the event path are serious.
    for code in FATAL_HTTP_CODES {
        assert_eq!(classify_error_severity(Some(code)), ErrorSeverity::Severe);
    }
}
