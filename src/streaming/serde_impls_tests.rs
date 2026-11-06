//! Round trips of the auto-function stream chunk and event wire format.

use serde_json::json;

use super::super::*;
use crate::StepDelta;

#[test]
fn test_auto_function_stream_chunk_serialization_roundtrip() {
    // Test Delta variant roundtrip (payload is the StepDelta wire form)
    let delta = AutoFunctionStreamChunk::Delta(StepDelta::Text {
        text: "Hello, world!".to_string(),
        extra: Default::default(),
    });

    let json_str = serde_json::to_string(&delta).expect("Serialization should succeed");
    assert!(json_str.contains("chunk_type"), "Should contain tag field");
    assert!(json_str.contains("delta"), "Should contain variant name");
    assert!(json_str.contains("Hello, world!"), "Should contain text");

    // The delta payload uses the StepDelta wire form: {"type":"text","text":"..."}
    let value: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(value["data"]["type"], "text");
    assert_eq!(value["data"]["text"], "Hello, world!");

    let deserialized: AutoFunctionStreamChunk =
        serde_json::from_str(&json_str).expect("Deserialization should succeed");

    match deserialized {
        AutoFunctionStreamChunk::Delta(delta) => {
            assert_eq!(delta.as_text(), Some("Hello, world!"));
        }
        _ => panic!("Expected Delta variant"),
    }

    // Test FunctionResults variant roundtrip
    let results = AutoFunctionStreamChunk::FunctionResults(vec![
        FunctionExecutionResult::new(
            "get_weather",
            "call-1",
            json!({"city": "Tokyo"}),
            json!({"temp": 20}),
            Duration::from_millis(50),
        ),
        FunctionExecutionResult::new(
            "get_time",
            "call-2",
            json!({"timezone": "UTC"}),
            json!({"time": "14:30"}),
            Duration::from_millis(30),
        ),
    ]);

    let json_str = serde_json::to_string(&results).expect("Serialization should succeed");
    let deserialized: AutoFunctionStreamChunk =
        serde_json::from_str(&json_str).expect("Deserialization should succeed");

    match deserialized {
        AutoFunctionStreamChunk::FunctionResults(execs) => {
            assert_eq!(execs.len(), 2);
            assert_eq!(execs[0].name, "get_weather");
            assert_eq!(execs[1].name, "get_time");
        }
        _ => panic!("Expected FunctionResults variant"),
    }

    // Test Unknown variant handling (forward compatibility)
    let unknown_json = r#"{"chunk_type": "future_event_type", "data": {"key": "value"}}"#;
    let deserialized: AutoFunctionStreamChunk =
        serde_json::from_str(unknown_json).expect("Should deserialize unknown variant");

    // Verify it's an Unknown variant with data preserved
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_chunk_type(), Some("future_event_type"));
    let data = deserialized.unknown_data().expect("Should have data");
    assert_eq!(data["key"], "value");

    // Verify roundtrip serialization
    let reserialized = serde_json::to_string(&deserialized).expect("Should serialize");
    assert!(reserialized.contains("future_event_type"));
    assert!(reserialized.contains("value"));
}

#[test]
fn test_auto_function_stream_chunk_unknown_without_data() {
    // Test unknown chunk type without data field
    let unknown_json = r#"{"chunk_type": "no_data_chunk"}"#;
    let deserialized: AutoFunctionStreamChunk =
        serde_json::from_str(unknown_json).expect("Should deserialize unknown variant");

    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_chunk_type(), Some("no_data_chunk"));

    // Data should be null when not provided
    let data = deserialized.unknown_data().expect("Should have data field");
    assert!(data.is_null());
}

#[test]
fn test_max_loops_reached_chunk_roundtrip() {
    use crate::InteractionStatus;

    // Create a MaxLoopsReached chunk
    let response = crate::InteractionResponse {
        id: Some("interaction-max-loops".to_string()),
        model: Some("test-model".to_string()),
        steps: vec![crate::Step::function_call(
            "call-pending",
            "stuck_function",
            json!({}),
        )],
        status: InteractionStatus::Completed,
        ..Default::default()
    };

    let chunk = AutoFunctionStreamChunk::MaxLoopsReached(Box::new(response));

    // Serialize
    let json_str = serde_json::to_string(&chunk).expect("Serialization should succeed");
    assert!(
        json_str.contains("max_loops_reached"),
        "Should contain chunk_type"
    );
    assert!(
        json_str.contains("interaction-max-loops"),
        "Should contain response data"
    );

    // Deserialize
    let deserialized: AutoFunctionStreamChunk =
        serde_json::from_str(&json_str).expect("Deserialization should succeed");

    match deserialized {
        AutoFunctionStreamChunk::MaxLoopsReached(resp) => {
            assert_eq!(resp.id.as_deref(), Some("interaction-max-loops"));
            assert_eq!(resp.function_calls().len(), 1);
            assert_eq!(resp.function_calls()[0].name, "stuck_function");
        }
        other => panic!("Expected MaxLoopsReached, got {:?}", other),
    }
}

#[test]
fn test_auto_function_stream_event_with_event_id_roundtrip() {
    let event = AutoFunctionStreamEvent::new(
        AutoFunctionStreamChunk::Delta(StepDelta::Text {
            text: "Hello from auto-function".to_string(),
            extra: Default::default(),
        }),
        Some("evt_auto_abc123".to_string()),
    );

    // Test helper methods
    assert!(event.is_delta());
    assert!(!event.is_complete());
    assert!(!event.is_unknown());

    let json = serde_json::to_string(&event).expect("Serialization should succeed");
    assert!(json.contains("evt_auto_abc123"), "Should have event_id");
    assert!(
        json.contains("Hello from auto-function"),
        "Should have content"
    );

    let deserialized: AutoFunctionStreamEvent =
        serde_json::from_str(&json).expect("Deserialization should succeed");
    assert_eq!(deserialized.event_id.as_deref(), Some("evt_auto_abc123"));
    assert!(deserialized.is_delta());
}

#[test]
fn test_auto_function_stream_event_without_event_id() {
    // Client-generated events like FunctionResults don't have event_id
    let event = AutoFunctionStreamEvent::new(
        AutoFunctionStreamChunk::FunctionResults(vec![FunctionExecutionResult::new(
            "weather",
            "call-123",
            json!({"city": "Denver"}),
            json!({"temp": 72}),
            Duration::from_millis(50),
        )]),
        None,
    );

    assert!(!event.is_delta());
    assert!(!event.is_complete());
    assert!(event.event_id.is_none());

    let json = serde_json::to_string(&event).expect("Serialization should succeed");
    assert!(!json.contains("event_id"), "Should not have event_id field");
    assert!(json.contains("weather"), "Should have function name");

    let deserialized: AutoFunctionStreamEvent =
        serde_json::from_str(&json).expect("Deserialization should succeed");
    assert!(deserialized.event_id.is_none());
}

#[test]
fn test_auto_function_stream_event_with_empty_event_id() {
    // Edge case: empty string event_id should still serialize/deserialize
    let event = AutoFunctionStreamEvent::new(
        AutoFunctionStreamChunk::Delta(StepDelta::Text {
            text: "Test".to_string(),
            extra: Default::default(),
        }),
        Some(String::new()),
    );

    let json = serde_json::to_string(&event).expect("Serialization should succeed");
    assert!(
        json.contains(r#""event_id":"""#),
        "Should have empty event_id"
    );

    let deserialized: AutoFunctionStreamEvent =
        serde_json::from_str(&json).expect("Deserialization should succeed");
    assert_eq!(deserialized.event_id.as_deref(), Some(""));
}

#[test]
fn test_executing_functions_new_format_roundtrip() {
    use crate::InteractionStatus;

    let chunk = AutoFunctionStreamChunk::ExecutingFunctions {
        response: Box::new(crate::InteractionResponse {
            id: Some("interaction-new".to_string()),
            model: Some("test-model".to_string()),
            status: InteractionStatus::Completed,
            ..Default::default()
        }),
        pending_calls: vec![
            OwnedFunctionCallInfo {
                id: "call-1".to_string(),
                name: "func1".to_string(),
                args: json!({"a": 1}),
            },
            OwnedFunctionCallInfo {
                id: "call-2".to_string(),
                name: "func2".to_string(),
                args: json!({"b": 2}),
            },
        ],
    };

    let json_str = serde_json::to_string(&chunk).expect("Serialization should succeed");
    assert!(json_str.contains("pending_calls"));
    assert!(json_str.contains("func1"));
    assert!(json_str.contains("func2"));

    let deserialized: AutoFunctionStreamChunk =
        serde_json::from_str(&json_str).expect("Deserialization should succeed");

    match deserialized {
        AutoFunctionStreamChunk::ExecutingFunctions {
            response,
            pending_calls,
        } => {
            assert_eq!(response.id.as_deref(), Some("interaction-new"));
            assert_eq!(pending_calls.len(), 2);
            assert_eq!(pending_calls[0].name, "func1");
            assert_eq!(pending_calls[1].name, "func2");
        }
        _ => panic!("Expected ExecutingFunctions variant"),
    }
}
