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
    let diagnosis = turn.stall_diagnosis("", None);
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
    assert_eq!(turn.stall_diagnosis("", None), "agent turn");

    // ...and a *subagent* trajectory going somewhere unrecognized is
    // not the parent's problem, so it must not pollute the parent's
    // diagnosis (the `is_main` half of the condition).
    let unknown: TrajectoryState =
        serde_json::from_value(serde_json::json!("STATE_SUPER_IDLE")).unwrap();
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("subagent-1"), unknown, None);
    AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
    assert_eq!(turn.stall_diagnosis("", None), "agent turn");
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
    let diagnosis = turn.stall_diagnosis(stderr, None);
    assert!(
        diagnosis.contains("failed to unmarshal InputEvent"),
        "diagnosis must quote the harness's own line, got: {diagnosis}"
    );
    assert!(diagnosis.contains("version mismatch"), "got: {diagnosis}");
    // Unrelated stderr noise does not produce a diagnosis.
    assert_eq!(turn.stall_diagnosis("Stdin closed", None), "agent turn");
}

#[test]
fn stall_diagnosis_names_client_work_still_running_at_the_deadline() {
    // A hung custom tool or hook is this side's stall, not the harness's:
    // name it, and don't point at a version mismatch.
    let mut turn = TurnState::new(None, None);
    turn.running = Some("custom tool 'slow_lookup'".to_string());
    let diagnosis = turn.stall_diagnosis("", Some("0.1.18"));
    assert_eq!(
        diagnosis,
        "agent turn (custom tool 'slow_lookup' was still running at the deadline)"
    );
}

#[test]
fn stall_diagnosis_names_a_known_harness_version_mismatch() {
    let turn = TurnState::new(None, None);
    // The supported version, or an unknown one, adds nothing.
    assert_eq!(
        turn.stall_diagnosis("", Some(crate::antigravity::SUPPORTED_HARNESS_VERSION)),
        "agent turn"
    );
    let diagnosis = turn.stall_diagnosis("", Some("0.0.1"));
    assert!(
        diagnosis.contains("google-antigravity 0.0.1")
            && diagnosis.contains(crate::antigravity::SUPPORTED_HARNESS_VERSION),
        "got: {diagnosis}"
    );
}

#[test]
fn test_main_trajectory_cancelled_fails_turn() {
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("main".to_string());
    let update = trajectory_update(Some("main"), TrajectoryState::Cancelled, Some("halted"));
    let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
    let AntigravityError::Turn { kind, message } = &err else {
        panic!("expected a Turn error: {err:?}");
    };
    assert_eq!(*kind, TurnFailure::Cancelled);
    assert_eq!(message, "halted", "the harness's error is the message");
}

#[test]
fn test_cancelled_before_any_step_fails_turn() {
    // No main trajectory yet (e.g. a pre-turn hook denial cancels the
    // turn before its first step): treated as the main trajectory.
    let mut turn = TurnState::new(None, None);
    let update = trajectory_update(Some("t-0"), TrajectoryState::Cancelled, None);
    let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
    assert!(
        matches!(
            &err,
            AntigravityError::Turn {
                kind: TurnFailure::Cancelled,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn test_a_failed_trajectory_names_its_kind() {
    // The main trajectory's failing ends: idle with an error, and
    // cancelled. A turn the pre-turn hook refused is Denied by either.
    let cases = [
        (
            TrajectoryState::Idle,
            Some("MCP load failed"),
            false,
            TurnFailure::Trajectory,
        ),
        (
            TrajectoryState::Cancelled,
            None,
            false,
            TurnFailure::Cancelled,
        ),
        (
            TrajectoryState::Cancelled,
            Some("no-9920"),
            true,
            TurnFailure::Denied,
        ),
        (
            TrajectoryState::Idle,
            Some("no-9921"),
            true,
            TurnFailure::Denied,
        ),
    ];
    for (state, error, denied, expected) in cases {
        let mut turn = TurnState::new(None, None);
        turn.pre_turn_denied = denied;
        let update = trajectory_update(Some("root"), state.clone(), error);
        let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
        let AntigravityError::Turn { kind, .. } = &err else {
            panic!("expected a Turn error: {err:?}");
        };
        assert_eq!(*kind, expected, "{state:?} {error:?} denied={denied}");
    }

    // After the backend failed a model call, the error that ends the turn
    // is that failure, with its status.
    let mut turn = TurnState::new(None, None);
    turn.backend_status = Some(503);
    let update = trajectory_update(Some("root"), TrajectoryState::Idle, Some("unreachable"));
    let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
    assert!(
        matches!(
            err,
            AntigravityError::Turn {
                kind: TurnFailure::ModelBackend { status: 503 },
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn test_an_error_on_a_non_terminal_update_fails_the_turn_at_its_end() {
    // 0.1.23, after a delegation: the main trajectory's error arrives while
    // it waits for subagents, and the idle that follows carries none.
    for (status, expected) in [
        (Some(503), TurnFailure::ModelBackend { status: 503 }),
        (None, TurnFailure::Trajectory),
    ] {
        let mut turn = TurnState::new(None, None);
        turn.backend_status = status;
        let waiting = trajectory_update(
            Some("root"),
            TrajectoryState::WaitingForSubagents,
            Some("model unreachable: Error 503"),
        );
        AntigravityAgent::process_trajectory_update(&waiting, &mut turn).unwrap();
        // A later update without the error does not clear it.
        let running = trajectory_update(Some("root"), TrajectoryState::Running, Some(""));
        AntigravityAgent::process_trajectory_update(&running, &mut turn).unwrap();
        let idle = trajectory_update(Some("root"), TrajectoryState::Idle, None);
        let err = AntigravityAgent::process_trajectory_update(&idle, &mut turn).unwrap_err();
        let AntigravityError::Turn { kind, message } = &err else {
            panic!("expected a Turn error: {err:?}");
        };
        assert_eq!(*kind, expected);
        assert_eq!(message, "model unreachable: Error 503");
        assert!(!turn.finished);
    }

    // A subagent's error is the subagent's: the parent turn still finishes.
    let mut turn = TurnState::new(None, None);
    turn.main_trajectory = Some("root".to_string());
    let mut sub = trajectory_update(Some("sub"), TrajectoryState::Running, Some("sub failed"));
    sub.parent_trajectory_id = Some("root".to_string());
    AntigravityAgent::process_trajectory_update(&sub, &mut turn).unwrap();
    let idle = trajectory_update(Some("root"), TrajectoryState::Idle, None);
    AntigravityAgent::process_trajectory_update(&idle, &mut turn).unwrap();
    assert!(turn.finished);
}

#[test]
fn test_backend_status_reads_http_errors_on_system_steps_only() {
    let step = |source: &str, code: u32| -> StepUpdate {
        serde_json::from_value(serde_json::json!({
            "trajectoryId": "root", "stepIndex": 1, "state": "STATE_ERROR",
            "source": source,
            "error": {"errorMessage": "Agent execution terminated due to error.", "httpCode": code}
        }))
        .unwrap()
    };
    assert_eq!(backend_status(&step("SOURCE_SYSTEM", 503)), Some(503));
    assert_eq!(backend_status(&step("SOURCE_SYSTEM", 400)), Some(400));
    // gRPC's UNKNOWN, which 0.1.23 sends for a failed MCP load.
    assert_eq!(backend_status(&step("SOURCE_SYSTEM", 2)), None);
    assert_eq!(backend_status(&step("SOURCE_MODEL", 503)), None);
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

fn widget_denial() -> HookDenial {
    HookDenial {
        trajectory_id: Some("root".to_string()),
        reason: "no widgets".to_string(),
        name: "mcp_widgets_lookup".to_string(),
        args: serde_json::json!({"widget": "flange"}),
    }
}

#[test]
fn test_hook_denial_marks_its_error_step_denied() {
    let mut turn = TurnState::new(None, None);
    turn.hook_denials.push(widget_denial());

    // A different trajectory's error with the same text is not ours.
    assert!(
        turn.take_hook_denial(&hook_denied_step("sub", 1, "no widgets"))
            .is_none()
    );
    // An unrelated error in the right trajectory is not ours either.
    assert!(
        turn.take_hook_denial(&hook_denied_step("root", 1, "quota"))
            .is_none()
    );
    // The matching step claims it, exactly once.
    let step = hook_denied_step("root", 1, "no widgets");
    let denial = turn.take_hook_denial(&step).expect("our denial");
    assert_eq!(denial.reason, "no widgets");
    assert_eq!(denial.name, "mcp_widgets_lookup");
    assert!(turn.take_hook_denial(&step).is_none(), "claimed once");
}

#[tokio::test]
async fn test_hook_denied_step_surfaces_as_a_named_denied_tool_action() {
    // Through process_step: the harness's error step for a denied call
    // carries no action, so the stream names the call from the denial
    // (WP5 follow-up: it used to read "error").
    let mut turn = TurnState::new(None, None);
    turn.hook_denials.push(widget_denial());
    let step = hook_denied_step("root", 1, "no widgets");
    let mut announced = None;
    if let Some(denial) = turn.take_hook_denial(&step) {
        announced = Some(turn.announce_tool_action(
            &("root".to_string(), 1),
            streaming::ToolAction::Denied {
                name: denial.name,
                args: denial.args,
            },
            ToolDecision::Denied {
                reason: denial.reason,
            },
            step.trajectory_id.clone(),
        ));
    }
    assert_eq!(announced, Some(true));
    let Some(AgentEvent::ToolAction {
        action, decision, ..
    }) = turn.queue.pop_front()
    else {
        panic!("expected a ToolAction");
    };
    assert_eq!(action.tool_name(), "mcp_widgets_lookup");
    assert_eq!(action.args()["widget"], "flange");
    assert_eq!(decision.denial_reason(), Some("no widgets"));
}

#[test]
fn test_error_state_step_drops_text_and_thought_deltas() {
    // #493 item 8: harness 0.1.23 streams a denial's text as the delta of
    // a SOURCE_MODEL / TARGET_USER error step (captured 2026-11-05).
    let step: StepUpdate = serde_json::from_value(serde_json::json!({
        "trajectoryId": "root", "stepIndex": 1, "state": "STATE_ERROR",
        "source": "SOURCE_MODEL", "target": "TARGET_USER",
        "errorMessage": "static-deny-reason-55",
        "textDelta": "static-deny-reason-55 (\"denied by pre-tool hook: static-deny-reason-55\")",
        "thinkingDelta": "partial thought"
    }))
    .unwrap();
    assert_eq!(stream_deltas(&step), (None, None));
    // The same step done streams as before.
    let mut done = step.clone();
    done.state = Some(StepState::Done);
    assert_eq!(
        stream_deltas(&done),
        (
            Some("partial thought"),
            Some("static-deny-reason-55 (\"denied by pre-tool hook: static-deny-reason-55\")")
        )
    );
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
