use super::*;
use serde_json::json;

fn args<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("wire args")
}

#[test]
fn test_turn_start_joins_text_parts_from_the_0_1_23_wire() {
    // Captured from harness 0.1.23 (2026-11-05).
    let pre_turn: protocol::PreTurnArgs = args(json!({
        "userInput": {"parts": [{"text": "first"}, {"text": "second"}]},
        "trajectoryId": "t-1"
    }));
    assert_eq!(
        turn_start(&pre_turn),
        TurnStart {
            text: "first\nsecond".to_string(),
            trajectory_id: Some("t-1".to_string()),
        }
    );
}

#[test]
fn test_pre_turn_verdicts() {
    assert_eq!(
        serde_json::to_value(pre_turn_verdict(TurnDecision::Allow)).unwrap(),
        json!({"decision": "ALLOW"})
    );
    assert_eq!(
        serde_json::to_value(pre_turn_verdict(TurnDecision::deny("closed"))).unwrap(),
        json!({"decision": "DENY", "reason": "closed"})
    );
}

#[test]
fn test_tool_failure_maps_names_and_blank_ids() {
    let builtin: protocol::OnToolErrorArgs = args(json!({
        "toolName": "view_file",
        "errorMessage": "path does not exist",
        "callId": "",
        "trajectoryId": "t",
        "stepIndex": 2
    }));
    let failure = tool_failure(&builtin);
    assert_eq!(failure.name, "view_file");
    assert_eq!(failure.error, "path does not exist");
    assert_eq!(failure.call_id, None);
    let mcp: protocol::OnToolErrorArgs = args(json!({
        "toolName": "lookup", "serverName": "widgets", "errorMessage": "boom", "callId": "c"
    }));
    assert_eq!(tool_failure(&mcp).name, "mcp_widgets_lookup");
}

#[test]
fn test_stop_request_and_result_mapping() {
    let wire: protocol::StopArgs = args(json!({
        "responseText": "done",
        "trajectoryId": "t",
        "continuationCount": 1,
        "stopReason": "STOP_REASON_UNSPECIFIED",
        "errorMessage": ""
    }));
    let request = stop_request(&wire);
    assert_eq!(request.response_text, "done");
    assert_eq!(request.continuation_count, 1);
    assert_eq!(request.stop_reason, None, "UNSPECIFIED is a normal stop");
    assert_eq!(request.error, None);
    let capped: protocol::StopArgs =
        args(json!({"stopReason": "STOP_REASON_MAX_TOOL_CALLS_EXCEEDED"}));
    assert_eq!(
        stop_request(&capped).stop_reason,
        Some(protocol::StopReason::MaxToolCallsExceeded)
    );

    assert_eq!(
        serde_json::to_value(stop_result(StopDecision::Stop)).unwrap(),
        json!({"decision": "ALLOW_STOP"})
    );
    assert_eq!(
        serde_json::to_value(stop_result(StopDecision::continue_with(" run the tests "))).unwrap(),
        json!({"decision": "CONTINUE", "reason": "run the tests"})
    );
}
