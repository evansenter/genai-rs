//! Unit tests for streaming types (StreamChunk, InteractionStreamEvent, etc.)

use super::*;

#[test]
fn test_deserialize_streaming_text_delta() {
    // Streaming deltas use the StepDelta wire form (tagged by "type")
    let delta_json = r#"{"type": "text", "text": "Hello world"}"#;
    let delta: StepDelta = serde_json::from_str(delta_json).expect("Deserialization failed");

    match &delta {
        StepDelta::Text { text } => {
            assert_eq!(text, "Hello world");
        }
        other => panic!("Expected Text delta, got {:?}", other),
    }

    assert_eq!(delta.as_text(), Some("Hello world"));
    assert!(!delta.is_unknown());
}

#[test]
fn test_deserialize_streaming_thought_signature_delta() {
    // Thought signatures stream as their own delta type
    let delta_json = r#"{"type": "thought_signature", "signature": "EosFCogFAXLI2..."}"#;
    let delta: StepDelta = serde_json::from_str(delta_json).expect("Deserialization failed");

    match &delta {
        StepDelta::ThoughtSignature { signature } => {
            assert_eq!(signature.as_deref(), Some("EosFCogFAXLI2..."));
        }
        other => panic!("Expected ThoughtSignature delta, got {:?}", other),
    }

    // as_text() returns None for non-text deltas
    assert_eq!(delta.as_text(), None);
}

#[test]
fn test_deserialize_streaming_arguments_delta() {
    // Function-call arguments stream incrementally as raw JSON fragments
    let delta_json = r#"{"type": "arguments_delta", "arguments": "{\"city\": \"Par"}"#;
    let delta: StepDelta = serde_json::from_str(delta_json).expect("Deserialization failed");

    match &delta {
        StepDelta::ArgumentsDelta { arguments } => {
            assert_eq!(arguments, "{\"city\": \"Par");
        }
        other => panic!("Expected ArgumentsDelta delta, got {:?}", other),
    }

    assert_eq!(delta.as_arguments_delta(), Some("{\"city\": \"Par"));
    assert_eq!(delta.as_text(), None);
    assert!(!delta.is_unknown());
}

#[test]
fn test_deserialize_streaming_unknown_delta() {
    // Unknown delta types are preserved (Evergreen principle)
    let delta_json = r#"{"type": "future_delta", "payload": 42}"#;
    let delta: StepDelta = serde_json::from_str(delta_json).expect("Deserialization failed");

    assert!(delta.is_unknown());
    assert_eq!(delta.unknown_delta_type(), Some("future_delta"));
    let data = delta.unknown_data().expect("Should preserve data");
    assert_eq!(data["payload"], 42);
}

#[test]
fn test_deserialize_step_delta_event() {
    let event_json = r#"{
        "event_type": "step.delta",
        "index": 0,
        "delta": {"type": "text", "text": "Hello"},
        "event_id": "evt_1"
    }"#;

    let event: InteractionStreamEvent =
        serde_json::from_str(event_json).expect("Deserialization failed");

    assert_eq!(event.event_type, "step.delta");
    assert_eq!(event.index, Some(0));
    assert!(event.delta.is_some());
    assert!(event.interaction.is_none());
    assert_eq!(event.event_id.as_deref(), Some("evt_1"));

    let delta = event.delta.unwrap();
    assert_eq!(delta.as_text(), Some("Hello"));
}

#[test]
fn test_deserialize_step_start_event() {
    let event_json = r#"{
        "event_type": "step.start",
        "index": 0,
        "step": {"type": "model_output", "content": []}
    }"#;

    let event: InteractionStreamEvent =
        serde_json::from_str(event_json).expect("Deserialization failed");

    assert_eq!(event.event_type, "step.start");
    assert_eq!(event.index, Some(0));
    let step = event.step.expect("Should have step");
    assert!(matches!(step, Step::ModelOutput { .. }));
}

#[test]
fn test_deserialize_step_stop_event() {
    let event_json = r#"{
        "event_type": "step.stop",
        "index": 0,
        "usage": {"total_tokens": 42},
        "step_usage": {"total_tokens": 10}
    }"#;

    let event: InteractionStreamEvent =
        serde_json::from_str(event_json).expect("Deserialization failed");

    assert_eq!(event.event_type, "step.stop");
    assert_eq!(event.index, Some(0));
    assert_eq!(event.usage.as_ref().and_then(|u| u.total_tokens), Some(42));
    assert_eq!(
        event.step_usage.as_ref().and_then(|u| u.total_tokens),
        Some(10)
    );
}

#[test]
fn test_deserialize_interaction_completed_event() {
    let event_json = r#"{
        "event_type": "interaction.completed",
        "interaction": {
            "id": "interaction_456",
            "model": "gemini-3-flash-preview",
            "steps": [
                {"type": "user_input", "content": [{"type": "text", "text": "Count to 3"}]},
                {"type": "model_output", "content": [{"type": "text", "text": "1, 2, 3"}]}
            ],
            "status": "completed"
        }
    }"#;

    let event: InteractionStreamEvent =
        serde_json::from_str(event_json).expect("Deserialization failed");

    assert_eq!(event.event_type, "interaction.completed");
    assert!(event.interaction.is_some());
    assert!(event.delta.is_none());

    let interaction = event.interaction.unwrap();
    assert_eq!(interaction.id.as_deref(), Some("interaction_456"));
    assert_eq!(interaction.as_text(), Some("1, 2, 3"));
}
