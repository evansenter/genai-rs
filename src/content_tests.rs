//! Unit tests for Content types, Step types, serialization, and Unknown
//! variant handling (API revision 2026-05-20).
//!
//! Tool calls, tool results, and thoughts are no longer `Content` variants;
//! they are typed `Step` variants (see `src/steps.rs`). The tool-content
//! tests that used to live here have been migrated to the equivalent Step
//! wire-format tests below.
//!
//! Note: the launch-era computer-use content tests were removed entirely —
//! computer use has no Step equivalent (it flows through `function_call`
//! steps).

use super::*;

// --- Basic Content Serialization/Deserialization ---

#[test]
fn test_serialize_interaction_content() {
    let content = Content::Text {
        text: Some("Hello".to_string()),
        annotations: None,
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Hello");
}

#[test]
fn test_content_empty_text_returns_none() {
    let content = Content::Text {
        text: Some(String::new()),
        annotations: None,
    };
    assert_eq!(content.as_text(), None);

    let content_none = Content::Text {
        text: None,
        annotations: None,
    };
    assert_eq!(content_none.as_text(), None);
}

#[test]
fn test_content_type_check_helpers() {
    assert!(Content::text("hi").is_text());
    assert!(Content::image_data("data", "image/png").is_image());
    assert!(Content::audio_data("data", "audio/wav").is_audio());
    assert!(Content::video_data("data", "video/mp4").is_video());
    assert!(Content::document_data("data", "application/pdf").is_document());

    let text = Content::text("hi");
    assert!(!text.is_image());
    assert!(!text.is_audio());
    assert!(!text.is_video());
    assert!(!text.is_document());
    assert!(!text.is_unknown());
}

#[test]
fn test_known_types_still_work() {
    // Ensure adding Unknown doesn't break known types
    let text_json = r#"{"type": "text", "text": "Hello"}"#;
    let content: Content = serde_json::from_str(text_json).unwrap();
    assert!(matches!(content, Content::Text { .. }));
    assert!(!content.is_unknown());

    let image_json = r#"{"type": "image", "data": "base64", "mime_type": "image/png"}"#;
    let content: Content = serde_json::from_str(image_json).unwrap();
    assert!(matches!(content, Content::Image { .. }));
    assert!(!content.is_unknown());

    let audio_json = r#"{"type": "audio", "data": "base64", "mime_type": "audio/wav"}"#;
    let content: Content = serde_json::from_str(audio_json).unwrap();
    assert!(matches!(content, Content::Audio { .. }));
    assert!(!content.is_unknown());

    let document_json =
        r#"{"type": "document", "uri": "files/abc", "mime_type": "application/pdf"}"#;
    let content: Content = serde_json::from_str(document_json).unwrap();
    assert!(matches!(content, Content::Document { .. }));
    assert!(!content.is_unknown());
}

#[test]
fn test_audio_content_sample_rate_and_channels() {
    // Audio gained sample_rate/channels in revision 2026-05-20
    let audio = Content::Audio {
        data: Some("base64audio".to_string()),
        uri: None,
        mime_type: Some("audio/L16;codec=pcm;rate=24000".to_string()),
        sample_rate: Some(24000),
        channels: Some(1),
    };

    let json = serde_json::to_string(&audio).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "audio");
    assert_eq!(value["sample_rate"], 24000);
    assert_eq!(value["channels"], 1);

    let restored: Content = serde_json::from_str(&json).expect("Deserialization failed");
    match restored {
        Content::Audio {
            sample_rate,
            channels,
            ..
        } => {
            assert_eq!(sample_rate, Some(24000));
            assert_eq!(channels, Some(1));
        }
        _ => panic!("Expected Audio variant"),
    }

    // Backward compatibility: audio without the new fields still deserializes
    let json = r#"{"type": "audio", "data": "base64", "mime_type": "audio/wav"}"#;
    let content: Content = serde_json::from_str(json).unwrap();
    match content {
        Content::Audio {
            sample_rate,
            channels,
            ..
        } => {
            assert_eq!(sample_rate, None);
            assert_eq!(channels, None);
        }
        _ => panic!("Expected Audio variant"),
    }
}

#[test]
fn test_serialize_known_variant_with_none_fields() {
    // Test that known variants with None fields serialize correctly (omit None fields)
    let text = Content::Text {
        text: None,
        annotations: None,
    };
    let json = serde_json::to_string(&text).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "text");
    assert!(value.get("text").is_none());
    assert!(value.get("annotations").is_none());

    let image = Content::Image {
        data: Some("base64data".to_string()),
        uri: None,
        mime_type: None,
        resolution: None,
    };
    let json = serde_json::to_string(&image).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "base64data");
    assert!(value.get("uri").is_none());
    assert!(value.get("mime_type").is_none());
    assert!(value.get("resolution").is_none());

    let audio = Content::Audio {
        data: Some("base64audio".to_string()),
        uri: None,
        mime_type: None,
        sample_rate: None,
        channels: None,
    };
    let json = serde_json::to_string(&audio).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "audio");
    assert!(value.get("sample_rate").is_none());
    assert!(value.get("channels").is_none());
}

// --- Unknown Variant Tests ---
// Note: Tests that rely on graceful unknown handling are disabled when strict-unknown is enabled,
// since strict mode causes deserialization errors for unknown types instead of capturing them.

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_interaction_content() {
    // Simulate a new API content type that this library doesn't know about
    let unknown_json = r#"{"type": "future_api_feature", "data_field": "some_value", "count": 42}"#;

    let content: Content =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    match &content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "future_api_feature");
            assert_eq!(data["data_field"], "some_value");
            assert_eq!(data["count"], 42);
        }
        _ => panic!("Expected Unknown variant, got {:?}", content),
    }

    assert!(content.is_unknown());
    assert_eq!(content.unknown_content_type(), Some("future_api_feature"));
    assert!(content.unknown_data().is_some());
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_streaming_content() {
    // Simulate a new streaming content type that this library doesn't know about
    let unknown_json = r#"{"type": "new_feature_delta", "data": "some_value"}"#;

    let content: Content =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    assert!(content.is_unknown());
    assert_eq!(content.unknown_content_type(), Some("new_feature_delta"));

    match &content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "new_feature_delta");
            assert_eq!(data["data"], "some_value");
        }
        _ => panic!("Expected Unknown variant"),
    }
}

#[test]
fn test_serialize_unknown_content_roundtrip() {
    // Create an Unknown content (simulating what we'd receive from API).
    // Note: code_execution_result is a Step type in revision 2026-05-20, so
    // as *content* it is genuinely unknown.
    let unknown = Content::Unknown {
        content_type: "code_execution_result".to_string(),
        data: serde_json::json!({
            "outcome": "success",
            "output": "42"
        }),
    };

    // Serialize it
    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify the structure: type field + flattened data
    assert_eq!(value["type"], "code_execution_result");
    assert_eq!(value["outcome"], "success");
    assert_eq!(value["output"], "42");
}

#[test]
fn test_serialize_unknown_with_non_object_data() {
    // Test that Unknown with non-object data (array, string, number) is preserved
    let unknown_array = Content::Unknown {
        content_type: "weird_type".to_string(),
        data: serde_json::json!([1, 2, 3]),
    };
    let json = serde_json::to_string(&unknown_array).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "weird_type");
    assert_eq!(value["data"], serde_json::json!([1, 2, 3]));

    let unknown_string = Content::Unknown {
        content_type: "string_type".to_string(),
        data: serde_json::json!("just a string"),
    };
    let json = serde_json::to_string(&unknown_string).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "string_type");
    assert_eq!(value["data"], "just a string");

    let unknown_null = Content::Unknown {
        content_type: "null_type".to_string(),
        data: serde_json::Value::Null,
    };
    let json = serde_json::to_string(&unknown_null).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "null_type");
    // Null data should be omitted
    assert!(value.get("data").is_none());
}

#[test]
fn test_serialize_unknown_with_duplicate_type_field() {
    // When data contains a "type" field, it should be ignored in serialization
    // (the content_type takes precedence)
    let unknown = Content::Unknown {
        content_type: "correct_type".to_string(),
        data: serde_json::json!({
            "type": "should_be_ignored",
            "field1": "value1",
            "field2": 42
        }),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // The type should be from content_type, not from data
    assert_eq!(value["type"], "correct_type");
    // Other fields should be preserved
    assert_eq!(value["field1"], "value1");
    assert_eq!(value["field2"], 42);
    // There should be exactly one "type" field, not two
    let obj = value.as_object().unwrap();
    let type_count = obj.keys().filter(|k| *k == "type").count();
    assert_eq!(type_count, 1);
}

#[test]
fn test_serialize_unknown_with_empty_content_type() {
    // Empty content_type is allowed but not recommended
    let unknown = Content::Unknown {
        content_type: String::new(),
        data: serde_json::json!({"field": "value"}),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "");
    assert_eq!(value["field"], "value");
}

#[test]
fn test_serialize_unknown_with_special_characters() {
    // Type names with special characters should be preserved
    let unknown = Content::Unknown {
        content_type: "special/type:with.chars-and_underscores".to_string(),
        data: serde_json::json!({"key": "value"}),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "special/type:with.chars-and_underscores");
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_unknown_manual_construction_roundtrip() {
    // Test that manually constructed Unknown variants can round-trip through JSON
    let original = Content::Unknown {
        content_type: "manual_test".to_string(),
        data: serde_json::json!({
            "nested": {"deeply": {"nested": "value"}},
            "array": [1, 2, 3],
            "number": 42,
            "boolean": true,
            "null_field": null
        }),
    };

    // Serialize
    let json = serde_json::to_string(&original).expect("Serialization should work");

    // Deserialize back
    let deserialized: Content = serde_json::from_str(&json).expect("Deserialization should work");

    // Verify it's still Unknown with same type
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_content_type(), Some("manual_test"));

    // Verify the data was preserved (check a few fields)
    if let Content::Unknown { data, .. } = deserialized {
        assert_eq!(data["nested"]["deeply"]["nested"], "value");
        assert_eq!(data["array"], serde_json::json!([1, 2, 3]));
        assert_eq!(data["number"], 42);
        assert_eq!(data["boolean"], true);
        // null_field should be present with null value (not stripped during serialization)
        assert_eq!(data.get("null_field"), Some(&serde_json::Value::Null));
    } else {
        panic!("Expected Unknown variant");
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_with_missing_type() {
    // Edge case: JSON object without a type field
    let malformed_json = r#"{"foo": "bar", "baz": 42}"#;
    let content: Content = serde_json::from_str(malformed_json).unwrap();
    match content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "<missing type>");
            assert_eq!(data["foo"], "bar");
            assert_eq!(data["baz"], 42);
        }
        _ => panic!("Expected Unknown variant"),
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_with_null_type() {
    // Edge case: JSON object with null type field
    let null_type_json = r#"{"type": null, "content": "test"}"#;
    let content: Content = serde_json::from_str(null_type_json).unwrap();
    match content {
        Content::Unknown { content_type, data } => {
            assert_eq!(content_type, "<missing type>");
            assert_eq!(data["content"], "test");
        }
        _ => panic!("Expected Unknown variant"),
    }
}

// =============================================================================
// Step Tests (migrated from removed Content tool variants)
// =============================================================================
// FunctionCall, Thought, CodeExecution*, GoogleSearch*, UrlContext*, and
// FileSearchResult are Step variants under revision 2026-05-20. These tests
// preserve the wire-format coverage the removed Content variants had.

// --- Function Call / Thought Steps ---

#[test]
fn test_deserialize_function_call_step() {
    let step_json = r#"{"type": "function_call", "id": "call-1", "name": "get_weather", "arguments": {"location": "Paris"}}"#;

    let step: Step = serde_json::from_str(step_json).expect("Deserialization failed");

    match step {
        Step::FunctionCall {
            id,
            name,
            arguments,
            signature: _,
        } => {
            assert_eq!(id, "call-1");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["location"], "Paris");
        }
        _ => panic!("Expected FunctionCall variant"),
    }
}

#[test]
fn test_deserialize_function_call_step_missing_arguments() {
    // Arguments default to null when not provided
    let step_json = r#"{"type": "function_call", "id": "call-abc123", "name": "get_weather"}"#;

    let step: Step = serde_json::from_str(step_json).expect("Deserialization failed");

    match step {
        Step::FunctionCall {
            id,
            name,
            arguments,
            signature: _,
        } => {
            assert_eq!(id, "call-abc123");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments, serde_json::Value::Null);
        }
        _ => panic!("Expected FunctionCall variant, got {:?}", step),
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_function_call_step_missing_required_fields_becomes_unknown() {
    // id and name are required on function_call steps; a payload without them
    // becomes Unknown per the Evergreen philosophy (data preserved, no error).
    let step_json = r#"{"type": "function_call", "id": "call-abc123"}"#;

    let step: Step = serde_json::from_str(step_json).expect("Should deserialize");

    assert!(step.is_unknown());
    assert_eq!(step.unknown_step_type(), Some("function_call"));
    let data = step.unknown_data().expect("Should preserve data");
    assert_eq!(data["id"], "call-abc123");
}

#[test]
fn test_serialize_function_call_step() {
    let step = Step::function_call("call_1", "test_fn", serde_json::json!({"arg": "value"}));

    let json = serde_json::to_string(&step).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // id/name/arguments live at the top level of the step
    assert_eq!(value["type"], "function_call");
    assert_eq!(value["id"], "call_1");
    assert_eq!(value["name"], "test_fn");
    assert_eq!(value["arguments"]["arg"], "value");
}

#[test]
fn test_step_thought_signature_accessor() {
    // Thought step with signature returns Some
    let step = Step::thought("EosFCogFAXLI2...");
    assert_eq!(step.signature(), Some("EosFCogFAXLI2..."));

    // Thought step without signature returns None
    let none = Step::Thought {
        signature: None,
        summary: vec![],
    };
    assert_eq!(none.signature(), None);

    // Non-signature-bearing steps return None
    let text_step = Step::model_text("hello");
    assert_eq!(text_step.signature(), None);
}

#[test]
fn test_step_constructors() {
    // user_text / model_text wrap text content
    let user = Step::user_text("Hi");
    assert!(matches!(user, Step::UserInput { .. }));
    assert_eq!(user.as_text(), Some("Hi"));

    let model = Step::model_text("Hello");
    assert!(matches!(model, Step::ModelOutput { .. }));
    assert_eq!(model.as_text(), Some("Hello"));

    // user_input / model_output take content blocks
    let user = Step::user_input(vec![Content::text("a"), Content::text("b")]);
    assert_eq!(user.content().unwrap().len(), 2);

    let model = Step::model_output(vec![Content::text("c")]);
    assert_eq!(model.content().unwrap().len(), 1);

    // function_call
    let call = Step::function_call("call_9", "get_weather", serde_json::json!({"city": "SF"}));
    match &call {
        Step::FunctionCall {
            id,
            name,
            arguments,
            signature: _,
        } => {
            assert_eq!(id, "call_9");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["city"], "SF");
        }
        _ => panic!("Expected FunctionCall variant"),
    }

    // function_result (success)
    let result = Step::function_result(
        "get_weather",
        "call_9",
        serde_json::json!({"temperature": "72F"}),
    );
    match &result {
        Step::FunctionResult {
            call_id,
            name,
            result,
            is_error,
            signature: _,
        } => {
            assert_eq!(call_id, "call_9");
            assert_eq!(name.as_deref(), Some("get_weather"));
            assert_eq!(result.as_json().unwrap()["temperature"], "72F");
            assert!(is_error.is_none());
        }
        _ => panic!("Expected FunctionResult variant"),
    }

    // function_result_error sets is_error
    let error = Step::function_result_error(
        "api_call",
        "call_10",
        serde_json::json!({"error": "timeout", "code": 504}),
    );
    match &error {
        Step::FunctionResult {
            call_id,
            name,
            result,
            is_error,
            signature: _,
        } => {
            assert_eq!(call_id, "call_10");
            assert_eq!(name.as_deref(), Some("api_call"));
            assert_eq!(result.as_json().unwrap()["code"], 504);
            assert_eq!(*is_error, Some(true));
        }
        _ => panic!("Expected FunctionResult variant"),
    }
}

#[test]
fn test_step_type_names() {
    assert_eq!(Step::user_text("x").step_type(), "user_input");
    assert_eq!(Step::model_text("x").step_type(), "model_output");
    assert_eq!(Step::thought("sig").step_type(), "thought");
    assert_eq!(
        Step::function_call("id", "fn", serde_json::Value::Null).step_type(),
        "function_call"
    );
    assert_eq!(
        Step::function_result("fn", "id", "ok").step_type(),
        "function_result"
    );
    assert_eq!(
        Step::Unknown {
            step_type: "future_step".to_string(),
            data: serde_json::Value::Null,
        }
        .step_type(),
        "future_step"
    );
}

#[test]
fn test_model_output_step_with_error() {
    let json = r#"{
        "type": "model_output",
        "content": [],
        "error": {"code": 400, "message": "Bad things happened"}
    }"#;

    let step: Step = serde_json::from_str(json).expect("Should deserialize");
    match step {
        Step::ModelOutput { content, error } => {
            assert!(content.is_empty());
            let error = error.expect("Should have error");
            assert_eq!(error.code, Some(400));
            assert_eq!(error.message.as_deref(), Some("Bad things happened"));
            assert!(error.details.is_none());
        }
        _ => panic!("Expected ModelOutput variant"),
    }
}

// --- Step Unknown Variant (Evergreen pattern, mirrors Content::Unknown) ---

#[test]
fn test_serialize_unknown_step() {
    let unknown = Step::Unknown {
        step_type: "future_step_type".to_string(),
        data: serde_json::json!({
            "type": "future_step_type",
            "payload": {"nested": true},
            "count": 7
        }),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "future_step_type");
    assert_eq!(value["payload"]["nested"], true);
    assert_eq!(value["count"], 7);
    // Exactly one "type" key
    let obj = value.as_object().unwrap();
    assert_eq!(obj.keys().filter(|k| *k == "type").count(), 1);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_unknown_step_roundtrip() {
    let unknown_json = r#"{"type": "future_step_type", "payload": "some_value", "count": 42}"#;

    let step: Step = serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    assert!(step.is_unknown());
    assert_eq!(step.unknown_step_type(), Some("future_step_type"));
    let data = step.unknown_data().expect("Should have data");
    assert_eq!(data["payload"], "some_value");
    assert_eq!(data["count"], 42);

    // Roundtrip: serialize back and re-deserialize
    let reserialized = serde_json::to_string(&step).expect("Should serialize");
    let restored: Step = serde_json::from_str(&reserialized).expect("Should deserialize again");
    assert!(restored.is_unknown());
    assert_eq!(restored.unknown_step_type(), Some("future_step_type"));
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_step_with_missing_type() {
    let malformed_json = r#"{"foo": "bar"}"#;
    let step: Step = serde_json::from_str(malformed_json).unwrap();
    match step {
        Step::Unknown { step_type, data } => {
            assert_eq!(step_type, "<missing type>");
            assert_eq!(data["foo"], "bar");
        }
        _ => panic!("Expected Unknown variant"),
    }
}

// --- FunctionResultPayload ---

#[test]
fn test_function_result_payload_conversions() {
    // From &str / String -> Text
    let payload: FunctionResultPayload = "plain result".into();
    assert_eq!(payload.as_text(), Some("plain result"));
    assert!(payload.as_json().is_none());
    assert!(payload.as_contents().is_none());
    assert_eq!(payload.to_value(), serde_json::json!("plain result"));

    let payload: FunctionResultPayload = String::from("owned").into();
    assert_eq!(payload.as_text(), Some("owned"));

    // From Value: strings become Text, objects become Json
    let payload: FunctionResultPayload = serde_json::json!("string value").into();
    assert_eq!(payload.as_text(), Some("string value"));

    let payload: FunctionResultPayload = serde_json::json!({"temp": 72}).into();
    assert_eq!(payload.as_json().unwrap()["temp"], 72);
    assert_eq!(payload.to_value(), serde_json::json!({"temp": 72}));

    // From Vec<Content> -> Contents
    let payload: FunctionResultPayload = vec![Content::text("block")].into();
    let contents = payload.as_contents().expect("Should be Contents");
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].as_text(), Some("block"));
}

#[test]
fn test_function_result_payload_from_value_classification() {
    // Arrays of typed objects are classified as content lists
    let payload = FunctionResultPayload::from_value(serde_json::json!([
        {"type": "text", "text": "hello"}
    ]));
    assert!(payload.as_contents().is_some());

    // Mixed / untyped arrays stay as raw JSON
    let payload = FunctionResultPayload::from_value(serde_json::json!([1, 2, 3]));
    assert!(payload.as_json().is_some());

    let payload = FunctionResultPayload::from_value(serde_json::json!([{"no_type": true}]));
    assert!(payload.as_json().is_some());
}

#[test]
fn test_function_result_payload_serde_roundtrip() {
    // Text payload serializes as a bare string
    let payload = FunctionResultPayload::Text("output".to_string());
    let json = serde_json::to_string(&payload).unwrap();
    assert_eq!(json, r#""output""#);
    let restored: FunctionResultPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.as_text(), Some("output"));

    // Json payload roundtrips
    let payload = FunctionResultPayload::Json(serde_json::json!({"a": 1}));
    let json = serde_json::to_string(&payload).unwrap();
    let restored: FunctionResultPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.as_json().unwrap()["a"], 1);

    // Contents payload roundtrips
    let payload = FunctionResultPayload::Contents(vec![Content::text("hi")]);
    let json = serde_json::to_string(&payload).unwrap();
    let restored: FunctionResultPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.as_contents().unwrap()[0].as_text(), Some("hi"));
}

// --- Code Execution Steps ---

#[test]
fn test_deserialize_code_execution_call_step() {
    // Wire format nests language/code inside `arguments`; language is lowercase.
    let json = r#"{"type": "code_execution_call", "id": "call_123", "arguments": {"code": "print(42)", "language": "python"}}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::CodeExecutionCall {
            id,
            language,
            code,
            signature,
        } => {
            assert_eq!(id, "call_123");
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print(42)");
            assert!(signature.is_none());
        }
        _ => panic!("Expected CodeExecutionCall variant, got {:?}", step),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_deserialize_code_execution_call_step_legacy_uppercase_language() {
    // Legacy uppercase "PYTHON" is still accepted on deserialize
    let json = r#"{"type": "code_execution_call", "id": "call_123", "arguments": {"code": "print(42)", "language": "PYTHON"}}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::CodeExecutionCall { language, code, .. } => {
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print(42)");
        }
        _ => panic!("Expected CodeExecutionCall variant, got {:?}", step),
    }
}

#[test]
fn test_deserialize_code_execution_call_step_missing_arguments_defaults() {
    // Under revision 2026-05-20 a code_execution_call without arguments is
    // handled leniently: language defaults to Python and code to empty.
    // (This replaces the launch-era malformed-becomes-Unknown behavior.)
    let json = r#"{"type": "code_execution_call", "id": "call_no_args"}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::CodeExecutionCall {
            id, language, code, ..
        } => {
            assert_eq!(id, "call_no_args");
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert!(code.is_empty());
        }
        _ => panic!("Expected CodeExecutionCall variant, got {:?}", step),
    }
}

#[test]
fn test_deserialize_code_execution_result_step() {
    let json = r#"{"type": "code_execution_result", "call_id": "call_123", "is_error": false, "result": "42\n"}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::CodeExecutionResult {
            call_id,
            is_error,
            result,
            signature,
        } => {
            assert_eq!(call_id, "call_123");
            assert!(!is_error);
            assert_eq!(result, "42\n");
            assert!(signature.is_none());
        }
        _ => panic!("Expected CodeExecutionResult variant, got {:?}", step),
    }
}

#[test]
fn test_deserialize_code_execution_result_step_error() {
    let json = r#"{"type": "code_execution_result", "call_id": "call_456", "is_error": true, "result": "NameError: x not defined"}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::CodeExecutionResult {
            call_id,
            is_error,
            result,
            ..
        } => {
            assert_eq!(call_id, "call_456");
            assert!(is_error);
            assert!(result.contains("NameError"));
        }
        _ => panic!("Expected CodeExecutionResult variant, got {:?}", step),
    }
}

#[test]
fn test_serialize_code_execution_call_step() {
    let step = Step::CodeExecutionCall {
        id: "call_123".to_string(),
        language: CodeExecutionLanguage::Python,
        code: "print(42)".to_string(),
        signature: None,
    };

    let json = serde_json::to_string(&step).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "code_execution_call");
    assert_eq!(value["id"], "call_123");
    // Wire format nests language and code inside arguments; language is lowercase
    assert_eq!(value["arguments"]["language"], "python");
    assert_eq!(value["arguments"]["code"], "print(42)");
}

#[test]
fn test_serialize_code_execution_result_step() {
    let step = Step::CodeExecutionResult {
        call_id: "call_123".to_string(),
        is_error: false,
        result: "42".to_string(),
        signature: None,
    };

    let json = serde_json::to_string(&step).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "code_execution_result");
    assert_eq!(value["call_id"], "call_123");
    assert_eq!(value["is_error"], false);
    assert_eq!(value["result"], "42");
}

#[test]
fn test_serialize_code_execution_result_step_error() {
    let step = Step::CodeExecutionResult {
        call_id: "call_456".to_string(),
        is_error: true,
        result: "NameError: x not defined".to_string(),
        signature: None,
    };

    let json = serde_json::to_string(&step).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "code_execution_result");
    assert_eq!(value["call_id"], "call_456");
    assert_eq!(value["is_error"], true);
    assert!(value["result"].as_str().unwrap().contains("NameError"));
}

#[test]
fn test_step_signature_preserved_on_tool_steps() {
    // Tool call/result steps carry an opaque signature that must roundtrip
    let step = Step::CodeExecutionCall {
        id: "call_sig".to_string(),
        language: CodeExecutionLanguage::Python,
        code: "print(1)".to_string(),
        signature: Some("sig-abc".to_string()),
    };
    assert_eq!(step.signature(), Some("sig-abc"));

    let json = serde_json::to_string(&step).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["signature"], "sig-abc");

    let restored: Step = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.signature(), Some("sig-abc"));
}

#[test]
fn test_roundtrip_built_in_tool_steps() {
    // CodeExecutionCall roundtrip
    let original = Step::CodeExecutionCall {
        id: "call_123".to_string(),
        language: CodeExecutionLanguage::Python,
        code: "print('hello')".to_string(),
        signature: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::CodeExecutionCall { .. }));

    // CodeExecutionResult roundtrip
    let original = Step::CodeExecutionResult {
        call_id: "call_123".to_string(),
        is_error: false,
        result: "hello\n".to_string(),
        signature: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::CodeExecutionResult { .. }));

    // GoogleSearchCall roundtrip
    let original = Step::GoogleSearchCall {
        id: "call123".to_string(),
        queries: vec!["test query".to_string()],
        search_type: None,
        signature: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::GoogleSearchCall { queries, .. } => assert_eq!(queries, vec!["test query"]),
        other => panic!("Expected GoogleSearchCall variant, got {:?}", other),
    }

    // GoogleSearchResult roundtrip
    let original = Step::GoogleSearchResult {
        call_id: "call123".to_string(),
        result: vec![],
        is_error: None,
        signature: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::GoogleSearchResult { .. }));

    // UrlContextCall roundtrip
    let original = Step::UrlContextCall {
        id: "ctx_123".to_string(),
        urls: vec!["https://example.com".to_string()],
        signature: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::UrlContextCall { urls, .. } => assert_eq!(urls, vec!["https://example.com"]),
        other => panic!("Expected UrlContextCall variant, got {:?}", other),
    }

    // UrlContextResult roundtrip
    let original = Step::UrlContextResult {
        call_id: "ctx_123".to_string(),
        result: vec![UrlContextResultItem::new("https://example.com", "success")],
        is_error: None,
        signature: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::UrlContextResult { .. }));
}

#[test]
fn test_step_edge_cases_empty_values() {
    // Empty code in CodeExecutionCall
    let step = Step::CodeExecutionCall {
        id: "call_empty".to_string(),
        language: CodeExecutionLanguage::Python,
        code: String::new(),
        signature: None,
    };
    let json = serde_json::to_string(&step).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::CodeExecutionCall {
            id, language, code, ..
        } => {
            assert_eq!(id, "call_empty");
            assert_eq!(language, CodeExecutionLanguage::Python);
            assert!(code.is_empty());
        }
        _ => panic!("Expected CodeExecutionCall"),
    }

    // Empty results in GoogleSearchResult
    let step = Step::GoogleSearchResult {
        call_id: "call_empty".to_string(),
        result: vec![],
        is_error: None,
        signature: None,
    };
    let json = serde_json::to_string(&step).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::GoogleSearchResult { .. }));

    // UrlContextResult with unsafe status item
    let step = Step::UrlContextResult {
        call_id: "ctx_unsafe".to_string(),
        result: vec![UrlContextResultItem::new(
            "https://blocked.example.com",
            "unsafe",
        )],
        is_error: None,
        signature: None,
    };
    let json = serde_json::to_string(&step).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::UrlContextResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "ctx_unsafe");
            assert_eq!(result.len(), 1);
            assert!(result[0].is_unsafe());
        }
        _ => panic!("Expected UrlContextResult"),
    }

    // Empty result string in CodeExecutionResult
    let step = Step::CodeExecutionResult {
        call_id: "call_no_output".to_string(),
        is_error: false,
        result: String::new(),
        signature: None,
    };
    let json = serde_json::to_string(&step).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::CodeExecutionResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "call_no_output");
            assert!(result.is_empty());
        }
        _ => panic!("Expected CodeExecutionResult"),
    }
}

// =============================================================================
// CodeExecutionLanguage Tests
// =============================================================================

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_code_execution_language_unknown_deserialization() {
    // Simulate a new language the library doesn't know about
    let unknown_json = r#""JAVASCRIPT""#;
    let language: CodeExecutionLanguage =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    assert!(language.is_unknown());

    // Verify helper methods
    assert_eq!(language.unknown_language_type(), Some("JAVASCRIPT"));
    assert!(language.unknown_data().is_some());

    // Verify roundtrip serialization preserves the value
    let reserialized = serde_json::to_string(&language).expect("Should serialize");
    assert_eq!(reserialized, r#""JAVASCRIPT""#);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_code_execution_language_unknown_display() {
    let unknown = CodeExecutionLanguage::Unknown {
        language_type: "RUST".to_string(),
        data: serde_json::Value::String("RUST".to_string()),
    };
    assert_eq!(format!("{}", unknown), "RUST");
}

#[test]
fn test_code_execution_language_known_variants_serde() {
    // Revision 2026-05-20 wire format is lowercase "python"
    let language = CodeExecutionLanguage::Python;
    let serialized = serde_json::to_string(&language).expect("Should serialize");
    assert_eq!(serialized, r#""python""#);

    let deserialized: CodeExecutionLanguage =
        serde_json::from_str(&serialized).expect("Should deserialize");
    assert_eq!(deserialized, language);
    assert!(!deserialized.is_unknown());

    // Display uses lowercase
    assert_eq!(format!("{}", CodeExecutionLanguage::Python), "python");
}

#[test]
fn test_code_execution_language_accepts_legacy_uppercase() {
    // Legacy uppercase "PYTHON" is still accepted on deserialize
    let deserialized: CodeExecutionLanguage =
        serde_json::from_str(r#""PYTHON""#).expect("Should deserialize legacy format");
    assert_eq!(deserialized, CodeExecutionLanguage::Python);
    assert!(!deserialized.is_unknown());
}

// --- Google Search / URL Context Steps ---

#[test]
fn test_deserialize_google_search_call_step() {
    // Wire format: arguments.queries is an array
    let json = r#"{"type": "google_search_call", "id": "call123", "arguments": {"queries": ["Rust programming", "latest version"]}}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::GoogleSearchCall { id, queries, .. } => {
            assert_eq!(id, "call123");
            assert_eq!(queries.len(), 2);
            assert_eq!(queries[0], "Rust programming");
            assert_eq!(queries[1], "latest version");
        }
        _ => panic!("Expected GoogleSearchCall variant, got {:?}", step),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_deserialize_google_search_result_step() {
    // Wire format: result is an array of objects with title/url
    let json = r#"{"type": "google_search_result", "call_id": "call123", "result": [{"title": "Rust", "url": "https://rust-lang.org", "rendered_content": "Some content"}]}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::GoogleSearchResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "call123");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].title, "Rust");
            assert_eq!(result[0].url, "https://rust-lang.org");
            assert_eq!(result[0].rendered_content.as_deref(), Some("Some content"));
        }
        _ => panic!("Expected GoogleSearchResult variant, got {:?}", step),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_deserialize_url_context_call_step() {
    // Wire format: {"type": "url_context_call", "id": "...", "arguments": {"urls": [...]}}
    let json = r#"{"type": "url_context_call", "id": "ctx_123", "arguments": {"urls": ["https://example.com", "https://example.org"]}}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::UrlContextCall { id, urls, .. } => {
            assert_eq!(id, "ctx_123");
            assert_eq!(urls.len(), 2);
            assert_eq!(urls[0], "https://example.com");
            assert_eq!(urls[1], "https://example.org");
        }
        _ => panic!("Expected UrlContextCall variant, got {:?}", step),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_deserialize_url_context_result_step() {
    // Wire format: {"type": "url_context_result", "call_id": "...", "result": [{"url": "...", "status": "..."}]}
    let json = r#"{"type": "url_context_result", "call_id": "ctx_123", "result": [{"url": "https://example.com", "status": "success"}, {"url": "https://example.org", "status": "error"}]}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::UrlContextResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "ctx_123");
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].url, "https://example.com");
            assert_eq!(result[0].status, "success");
            assert!(result[0].is_success());
            assert_eq!(result[1].url, "https://example.org");
            assert_eq!(result[1].status, "error");
            assert!(result[1].is_error());
        }
        _ => panic!("Expected UrlContextResult variant, got {:?}", step),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_url_context_result_step_with_empty_result_array() {
    // Test UrlContextResult with empty result array
    let step = Step::UrlContextResult {
        call_id: "ctx_empty".to_string(),
        result: vec![],
        is_error: None,
        signature: None,
    };

    // Serialize and verify structure
    let json = serde_json::to_string(&step).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "url_context_result");
    assert_eq!(value["call_id"], "ctx_empty");
    assert!(value["result"].as_array().unwrap().is_empty());

    // Deserialize with empty result array
    let json_empty_result =
        r#"{"type": "url_context_result", "call_id": "ctx_empty", "result": []}"#;
    let deserialized: Step = serde_json::from_str(json_empty_result).expect("Should deserialize");

    match &deserialized {
        Step::UrlContextResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "ctx_empty");
            assert!(result.is_empty());
        }
        _ => panic!("Expected UrlContextResult variant"),
    }
}

#[test]
fn test_url_context_result_item_status_helpers() {
    let success_item = UrlContextResultItem::new("https://example.com", "success");
    assert!(success_item.is_success());
    assert!(!success_item.is_error());
    assert!(!success_item.is_unsafe());
    assert!(!success_item.is_paywall());

    let error_item = UrlContextResultItem::new("https://example.org", "error");
    assert!(!error_item.is_success());
    assert!(error_item.is_error());
    assert!(!error_item.is_unsafe());

    let unsafe_item = UrlContextResultItem::new("https://malware.example", "unsafe");
    assert!(!unsafe_item.is_success());
    assert!(!unsafe_item.is_error());
    assert!(unsafe_item.is_unsafe());

    let paywall_item = UrlContextResultItem::new("https://news.example", "paywall");
    assert!(!paywall_item.is_success());
    assert!(paywall_item.is_paywall());
}

// --- Response-Level Step Deserialization ---

#[test]
fn test_deserialize_response_with_built_in_tool_steps() {
    // Test deserializing a full response whose steps include built-in tools
    let response_json = r#"{
        "id": "interaction_789",
        "model": "gemini-3-flash-preview",
        "steps": [
            {"type": "model_output", "content": [{"type": "text", "text": "Here's the result:"}]},
            {"type": "code_execution_call", "id": "call_abc", "arguments": {"code": "print(42)", "language": "python"}},
            {"type": "code_execution_result", "call_id": "call_abc", "is_error": false, "result": "42"}
        ],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(response_json).expect("Should deserialize with built-in tool steps");

    assert_eq!(response.id.as_deref(), Some("interaction_789"));
    assert_eq!(response.steps.len(), 3);
    assert!(response.has_text());
    assert!(response.has_code_execution_calls());
    assert!(response.has_code_execution_results());
    assert!(!response.has_unknown()); // These are all known step types

    let summary = response.step_summary();
    assert_eq!(summary.model_output_count, 1);
    assert_eq!(summary.text_count, 1);
    assert_eq!(summary.code_execution_call_count, 1);
    assert_eq!(summary.code_execution_result_count, 1);
    assert_eq!(summary.unknown_count, 0);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_deserialize_response_with_unknown_steps() {
    // Test deserializing a full response that contains truly unknown steps
    let response_json = r#"{
        "id": "interaction_789",
        "model": "gemini-3-flash-preview",
        "steps": [
            {"type": "model_output", "content": [{"type": "text", "text": "Result:"}]},
            {"type": "future_tool_result", "data": "some_data"},
            {"type": "another_unknown_type", "value": 123}
        ],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(response_json).expect("Should deserialize with unknown step types");

    assert_eq!(response.id.as_deref(), Some("interaction_789"));
    assert_eq!(response.steps.len(), 3);
    assert!(response.has_text());
    assert!(response.has_unknown());

    let summary = response.step_summary();
    assert_eq!(summary.text_count, 1);
    assert_eq!(summary.unknown_count, 2);
    assert!(
        summary
            .unknown_types
            .contains(&"future_tool_result".to_string())
    );
    assert!(
        summary
            .unknown_types
            .contains(&"another_unknown_type".to_string())
    );
}

// --- Annotation Tests ---
// Annotation is a discriminated union under revision 2026-05-20:
// url_citation / file_citation / place_citation / Unknown.

#[test]
fn test_annotation_url_citation_constructor_and_accessors() {
    let annotation = Annotation::url_citation("https://example.com", None, 0, 10);

    assert_eq!(annotation.start_index(), Some(0));
    assert_eq!(annotation.end_index(), Some(10));
    assert_eq!(annotation.source(), Some("https://example.com"));
    assert!(!annotation.is_unknown());

    let with_title = Annotation::url_citation(
        "https://example.org",
        Some("Example Title".to_string()),
        5,
        15,
    );
    match &with_title {
        Annotation::UrlCitation { url, title, .. } => {
            assert_eq!(url.as_deref(), Some("https://example.org"));
            assert_eq!(title.as_deref(), Some("Example Title"));
        }
        _ => panic!("Expected UrlCitation variant"),
    }
}

#[test]
fn test_annotation_extract_span() {
    let text = "Hello, world!";
    let annotation = Annotation::url_citation("https://example.com", None, 0, 5);
    assert_eq!(annotation.extract_span(text), Some("Hello"));

    let annotation_mid = Annotation::url_citation("https://example.com", None, 7, 12);
    assert_eq!(annotation_mid.extract_span(text), Some("world"));

    // Out of bounds
    let out_of_bounds = Annotation::url_citation("https://example.com", None, 100, 200);
    assert_eq!(out_of_bounds.extract_span(text), None);
}

#[test]
fn test_annotation_extract_span_utf8() {
    // Test with UTF-8 text - annotations use byte indices
    let text = "Héllo, 世界!"; // "Héllo" = 6 bytes (H=1, é=2, l=1, l=1, o=1), ", " = 2 bytes, "世界" = 6 bytes
    let annotation = Annotation::url_citation("https://example.com", None, 0, 6);
    assert_eq!(annotation.extract_span(text), Some("Héllo"));

    // Extract Chinese characters
    let world_annotation = Annotation::url_citation("https://example.com", None, 8, 14);
    assert_eq!(world_annotation.extract_span(text), Some("世界"));
}

#[test]
fn test_annotation_extract_span_inverted_indices() {
    // Edge case: start_index > end_index (malformed annotation)
    // The implementation should gracefully return None
    let inverted = Annotation::url_citation("https://example.com", None, 10, 5);

    let text = "Hello, world!";
    assert_eq!(
        inverted.extract_span(text),
        None,
        "Inverted indices should return None"
    );
}

#[test]
fn test_annotation_extract_span_zero_length() {
    // Edge case: start_index == end_index (zero-length span)
    let zero_len = Annotation::url_citation("https://example.com", None, 5, 5);

    let text = "Hello, world!";
    assert_eq!(
        zero_len.extract_span(text),
        Some(""),
        "Zero-length span should return empty string"
    );
}

#[test]
fn test_annotation_extract_span_mid_utf8_boundary() {
    // Edge case: indices that land in the middle of a multi-byte character
    // "世" is a 3-byte UTF-8 character (E4 B8 96)
    let text = "Hello, 世界!"; // "世" starts at byte 7, "界" starts at byte 10

    // Try to slice starting in the middle of "世" (byte 8)
    let mid_start = Annotation::url_citation("https://example.com", None, 8, 13);
    assert_eq!(
        mid_start.extract_span(text),
        None,
        "Slicing from middle of UTF-8 character should return None"
    );

    // Try to slice ending in the middle of "界" (byte 11)
    let mid_end = Annotation::url_citation("https://example.com", None, 7, 11);
    assert_eq!(
        mid_end.extract_span(text),
        None,
        "Slicing to middle of UTF-8 character should return None"
    );

    // Valid slice of "世界" (bytes 7-13)
    let valid_cjk = Annotation::url_citation("https://example.com", None, 7, 13);
    assert_eq!(
        valid_cjk.extract_span(text),
        Some("世界"),
        "Valid CJK character slice should work"
    );
}

#[test]
fn test_annotation_url_citation_wire_format() {
    // Wire format: {"type":"url_citation","url":...,"title":...,"start_index":N,"end_index":N}
    let annotation =
        Annotation::url_citation("https://example.com", Some("Example".to_string()), 3, 9);

    let json = serde_json::to_string(&annotation).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "url_citation");
    assert_eq!(value["url"], "https://example.com");
    assert_eq!(value["title"], "Example");
    assert_eq!(value["start_index"], 3);
    assert_eq!(value["end_index"], 9);
}

#[test]
fn test_serialize_text_with_annotations() {
    let annotations = vec![
        Annotation::url_citation("https://example.com", None, 0, 5),
        Annotation::UrlCitation {
            url: None,
            title: None,
            start_index: 10,
            end_index: 20,
        },
    ];

    let content = Content::Text {
        text: Some("Hello, world! This is grounded text.".to_string()),
        annotations: Some(annotations),
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Hello, world! This is grounded text.");
    assert!(value["annotations"].is_array());
    assert_eq!(value["annotations"].as_array().unwrap().len(), 2);
    assert_eq!(value["annotations"][0]["type"], "url_citation");
    assert_eq!(value["annotations"][0]["start_index"], 0);
    assert_eq!(value["annotations"][0]["end_index"], 5);
    assert_eq!(value["annotations"][0]["url"], "https://example.com");
    assert_eq!(value["annotations"][1]["start_index"], 10);
    assert_eq!(value["annotations"][1]["end_index"], 20);
    // None url is omitted from the wire format
    assert!(value["annotations"][1].get("url").is_none());
}

#[test]
fn test_serialize_text_with_empty_annotations_omitted() {
    // Empty annotations array should not be serialized
    let content = Content::Text {
        text: Some("Plain text".to_string()),
        annotations: Some(vec![]),
    };

    let json = serde_json::to_string(&content).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "text");
    assert_eq!(value["text"], "Plain text");
    // Empty annotations should not be serialized
    assert!(value.get("annotations").is_none());
}

#[test]
fn test_deserialize_text_with_annotations() {
    let json = r#"{
        "type": "text",
        "text": "This is grounded text.",
        "annotations": [
            {"type": "url_citation", "url": "https://example.com", "title": "Example", "start_index": 0, "end_index": 4},
            {"type": "url_citation", "start_index": 8, "end_index": 16}
        ]
    }"#;

    let content: Content = serde_json::from_str(json).expect("Deserialization failed");

    match content {
        Content::Text { text, annotations } => {
            assert_eq!(text, Some("This is grounded text.".to_string()));
            let annots = annotations.expect("Should have annotations");
            assert_eq!(annots.len(), 2);
            assert_eq!(annots[0].start_index(), Some(0));
            assert_eq!(annots[0].end_index(), Some(4));
            assert_eq!(annots[0].source(), Some("https://example.com"));
            assert_eq!(annots[1].start_index(), Some(8));
            assert_eq!(annots[1].end_index(), Some(16));
            assert_eq!(annots[1].source(), None);
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_deserialize_text_without_annotations() {
    let json = r#"{"type": "text", "text": "Plain text"}"#;

    let content: Content = serde_json::from_str(json).expect("Deserialization failed");

    match content {
        Content::Text { text, annotations } => {
            assert_eq!(text, Some("Plain text".to_string()));
            assert!(annotations.is_none());
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_text_with_annotations_roundtrip() {
    let annotations = vec![Annotation::url_citation(
        "https://source.example.com",
        None,
        5,
        15,
    )];

    let original = Content::Text {
        text: Some("Some grounded content here.".to_string()),
        annotations: Some(annotations),
    };

    let json = serde_json::to_string(&original).expect("Serialization failed");
    let deserialized: Content = serde_json::from_str(&json).expect("Deserialization failed");

    // Compare by re-serializing and checking the JSON
    let roundtrip_json = serde_json::to_string(&deserialized).expect("Serialization failed");
    assert_eq!(json, roundtrip_json);

    // Also verify content matches
    match deserialized {
        Content::Text { text, annotations } => {
            assert_eq!(text, Some("Some grounded content here.".to_string()));
            let annots = annotations.expect("Should have annotations");
            assert_eq!(annots.len(), 1);
            assert_eq!(annots[0].start_index(), Some(5));
            assert_eq!(annots[0].end_index(), Some(15));
            assert_eq!(annots[0].source(), Some("https://source.example.com"));
        }
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn test_annotations_helper_method() {
    let content_with_annotations = Content::Text {
        text: Some("Hello".to_string()),
        annotations: Some(vec![Annotation::url_citation(
            "https://example.com",
            None,
            0,
            5,
        )]),
    };

    assert!(content_with_annotations.annotations().is_some());
    assert_eq!(content_with_annotations.annotations().unwrap().len(), 1);

    let content_without_annotations = Content::Text {
        text: Some("Hello".to_string()),
        annotations: None,
    };

    assert!(content_without_annotations.annotations().is_none());

    // Non-text content returns None
    let image = Content::image_data("base64", "image/png");
    assert!(image.annotations().is_none());
}

#[test]
fn test_annotation_file_citation_deserialize() {
    let json = r#"{
        "type": "file_citation",
        "document_uri": "files/abc123",
        "file_name": "report.pdf",
        "source": "stores/my-store",
        "page_number": 3,
        "start_index": 0,
        "end_index": 12
    }"#;

    let annotation: Annotation = serde_json::from_str(json).expect("Should deserialize");

    match &annotation {
        Annotation::FileCitation {
            document_uri,
            file_name,
            source,
            page_number,
            start_index,
            end_index,
            ..
        } => {
            assert_eq!(document_uri.as_deref(), Some("files/abc123"));
            assert_eq!(file_name.as_deref(), Some("report.pdf"));
            assert_eq!(source.as_deref(), Some("stores/my-store"));
            assert_eq!(*page_number, Some(3));
            assert_eq!(*start_index, 0);
            assert_eq!(*end_index, 12);
        }
        _ => panic!("Expected FileCitation variant, got {:?}", annotation),
    }

    // source() prefers document_uri for file citations
    assert_eq!(annotation.source(), Some("files/abc123"));
    assert!(!annotation.is_unknown());
}

#[test]
fn test_annotation_place_citation_deserialize() {
    let json = r#"{
        "type": "place_citation",
        "place_id": "ChIJLU7jZClu5kcR",
        "name": "Eiffel Tower",
        "url": "https://maps.example.com/eiffel",
        "review_snippets": [{"title": "Great!", "url": "https://r.example.com", "review_id": "r1"}],
        "start_index": 4,
        "end_index": 16
    }"#;

    let annotation: Annotation = serde_json::from_str(json).expect("Should deserialize");

    match &annotation {
        Annotation::PlaceCitation {
            place_id,
            name,
            url,
            review_snippets,
            start_index,
            end_index,
        } => {
            assert_eq!(place_id.as_deref(), Some("ChIJLU7jZClu5kcR"));
            assert_eq!(name.as_deref(), Some("Eiffel Tower"));
            assert_eq!(url.as_deref(), Some("https://maps.example.com/eiffel"));
            assert_eq!(review_snippets.len(), 1);
            assert_eq!(review_snippets[0].title.as_deref(), Some("Great!"));
            assert_eq!(review_snippets[0].review_id.as_deref(), Some("r1"));
            assert_eq!(*start_index, 4);
            assert_eq!(*end_index, 16);
        }
        _ => panic!("Expected PlaceCitation variant, got {:?}", annotation),
    }

    // source() prefers the place URL
    assert_eq!(annotation.source(), Some("https://maps.example.com/eiffel"));
}

#[test]
fn test_annotation_unknown_variant() {
    // Unrecognized annotation types are preserved in Unknown
    let json =
        r#"{"type": "future_citation", "custom_field": "value", "start_index": 2, "end_index": 6}"#;
    let annotation: Annotation = serde_json::from_str(json).expect("Should deserialize");

    assert!(annotation.is_unknown());
    assert_eq!(
        annotation.unknown_annotation_type(),
        Some("future_citation")
    );
    let data = annotation.unknown_data().expect("Should have data");
    assert_eq!(data["custom_field"], "value");

    // Indices are still readable from the preserved data
    assert_eq!(annotation.start_index(), Some(2));
    assert_eq!(annotation.end_index(), Some(6));
    assert_eq!(annotation.extract_span("Hello, world!"), Some("llo,"));
    assert_eq!(annotation.source(), None);

    // Roundtrip preserves the original fields
    let reserialized = serde_json::to_string(&annotation).expect("Should serialize");
    let value: serde_json::Value = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(value["type"], "future_citation");
    assert_eq!(value["custom_field"], "value");
    assert_eq!(value["start_index"], 2);
    assert_eq!(value["end_index"], 6);
}

// --- Resolution Tests ---

#[test]
fn test_resolution_enum_serialization() {
    // Test that Resolution serializes to snake_case
    assert_eq!(serde_json::to_string(&Resolution::Low).unwrap(), "\"low\"");
    assert_eq!(
        serde_json::to_string(&Resolution::Medium).unwrap(),
        "\"medium\""
    );
    assert_eq!(
        serde_json::to_string(&Resolution::High).unwrap(),
        "\"high\""
    );
    assert_eq!(
        serde_json::to_string(&Resolution::UltraHigh).unwrap(),
        "\"ultra_high\""
    );
}

#[test]
fn test_resolution_enum_deserialization() {
    assert_eq!(
        serde_json::from_str::<Resolution>("\"low\"").unwrap(),
        Resolution::Low
    );
    assert_eq!(
        serde_json::from_str::<Resolution>("\"medium\"").unwrap(),
        Resolution::Medium
    );
    assert_eq!(
        serde_json::from_str::<Resolution>("\"high\"").unwrap(),
        Resolution::High
    );
    assert_eq!(
        serde_json::from_str::<Resolution>("\"ultra_high\"").unwrap(),
        Resolution::UltraHigh
    );
}

#[test]
fn test_resolution_default_is_medium() {
    assert_eq!(Resolution::default(), Resolution::Medium);
}

#[test]
fn test_image_with_resolution_serialization() {
    let image = Content::Image {
        data: Some("base64data".to_string()),
        uri: None,
        mime_type: Some("image/png".to_string()),
        resolution: Some(Resolution::High),
    };

    let json = serde_json::to_string(&image).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "base64data");
    assert_eq!(value["mime_type"], "image/png");
    assert_eq!(value["resolution"], "high");
}

#[test]
fn test_image_with_ultra_high_resolution_serialization() {
    let image = Content::Image {
        data: None,
        uri: Some("https://example.com/image.png".to_string()),
        mime_type: Some("image/png".to_string()),
        resolution: Some(Resolution::UltraHigh),
    };

    let json = serde_json::to_string(&image).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "image");
    assert_eq!(value["uri"], "https://example.com/image.png");
    assert_eq!(value["resolution"], "ultra_high");
}

#[test]
fn test_video_with_resolution_serialization() {
    let video = Content::Video {
        data: Some("videobytes".to_string()),
        uri: None,
        mime_type: Some("video/mp4".to_string()),
        resolution: Some(Resolution::Low),
    };

    let json = serde_json::to_string(&video).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "video");
    assert_eq!(value["data"], "videobytes");
    assert_eq!(value["mime_type"], "video/mp4");
    assert_eq!(value["resolution"], "low");
}

#[test]
fn test_image_with_resolution_deserialization() {
    let json = r#"{"type": "image", "data": "base64data", "mime_type": "image/png", "resolution": "high"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64data".to_string()));
            assert_eq!(uri, None);
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_video_with_resolution_deserialization() {
    let json = r#"{"type": "video", "uri": "https://example.com/video.mp4", "mime_type": "video/mp4", "resolution": "ultra_high"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, None);
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::UltraHigh));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_image_without_resolution_deserialization() {
    // Verify backward compatibility: images without resolution field should work
    let json = r#"{"type": "image", "data": "base64data", "mime_type": "image/png"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Image { resolution, .. } => {
            assert_eq!(resolution, None);
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_image_with_resolution_roundtrip() {
    let original = Content::Image {
        data: Some("testdata".to_string()),
        uri: None,
        mime_type: Some("image/jpeg".to_string()),
        resolution: Some(Resolution::Medium),
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("testdata".to_string()));
            assert_eq!(uri, None);
            assert_eq!(mime_type, Some("image/jpeg".to_string()));
            assert_eq!(resolution, Some(Resolution::Medium));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_video_with_resolution_roundtrip() {
    let original = Content::Video {
        data: None,
        uri: Some("gs://bucket/video.mp4".to_string()),
        mime_type: Some("video/mp4".to_string()),
        resolution: Some(Resolution::High),
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, None);
            assert_eq!(uri, Some("gs://bucket/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Video variant"),
    }
}

// --- Resolution Unknown Tests ---

#[test]
fn test_resolution_unknown_deserialization() {
    // Test that unrecognized resolution strings deserialize to Unknown
    let json = r#""super_high""#;
    let resolution: Resolution = serde_json::from_str(json).unwrap();

    assert!(resolution.is_unknown());
    assert_eq!(resolution.unknown_resolution_type(), Some("super_high"));
}

#[test]
fn test_resolution_unknown_roundtrip() {
    // Test that Unknown variant roundtrips correctly
    let unknown = Resolution::Unknown {
        resolution_type: "extreme".to_string(),
        data: serde_json::Value::String("extreme".to_string()),
    };

    let json = serde_json::to_string(&unknown).expect("Serialization failed");
    assert_eq!(json, "\"extreme\"");

    let deserialized: Resolution = serde_json::from_str(&json).unwrap();
    assert!(deserialized.is_unknown());
    assert_eq!(deserialized.unknown_resolution_type(), Some("extreme"));
}

#[test]
fn test_resolution_unknown_helper_methods() {
    let known = Resolution::High;
    assert!(!known.is_unknown());
    assert_eq!(known.unknown_resolution_type(), None);
    assert!(known.unknown_data().is_none());

    let unknown = Resolution::Unknown {
        resolution_type: "future_res".to_string(),
        data: serde_json::json!({"level": "future_res", "extra": true}),
    };
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_resolution_type(), Some("future_res"));
    let data = unknown.unknown_data().unwrap();
    assert_eq!(data.get("extra").unwrap(), true);
}

#[test]
fn test_resolution_unknown_in_image_content() {
    // Test that unknown resolution works within Image content
    let json =
        r#"{"type": "image", "data": "base64", "mime_type": "image/png", "resolution": "auto"}"#;
    let content: Content = serde_json::from_str(json).unwrap();

    match content {
        Content::Image { resolution, .. } => {
            let res = resolution.expect("resolution should be present");
            assert!(res.is_unknown());
            assert_eq!(res.unknown_resolution_type(), Some("auto"));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_resolution_unknown_object_form() {
    // Test that object-form resolution values are handled (future API compatibility)
    // Non-string values get "<non-string: ...>" as the resolution_type
    let json = r#"{"level": "ultra_ultra_high", "tokens": 5000}"#;
    let resolution: Resolution = serde_json::from_str(json).expect("Should deserialize");

    assert!(resolution.is_unknown());
    // Object form gets formatted as "<non-string: ...>" in resolution_type
    assert!(
        resolution
            .unknown_resolution_type()
            .unwrap()
            .starts_with("<non-string:")
    );

    // Verify the full object is preserved in data
    let data = resolution.unknown_data().unwrap();
    assert_eq!(data.get("level").unwrap(), "ultra_ultra_high");
    assert_eq!(data.get("tokens").unwrap(), 5000);
}

// --- File Search Steps ---

#[test]
fn test_deserialize_file_search_result_step() {
    // Test the actual API format
    let json = r#"{"type": "file_search_result", "call_id": "call123", "result": [{"title": "Document.pdf", "text": "Relevant content", "file_search_store": "store-1"}]}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::FileSearchResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "call123");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].title, "Document.pdf");
            assert_eq!(result[0].text, "Relevant content");
            assert_eq!(result[0].store, "store-1");
        }
        _ => panic!("Expected FileSearchResult variant, got {:?}", step),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_deserialize_file_search_result_step_multiple_items() {
    let json = r#"{
        "type": "file_search_result",
        "call_id": "call456",
        "result": [
            {"title": "First.pdf", "text": "First content", "file_search_store": "store-a"},
            {"title": "Second.pdf", "text": "Second content", "file_search_store": "store-b"}
        ]
    }"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::FileSearchResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "call456");
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].title, "First.pdf");
            assert_eq!(result[1].title, "Second.pdf");
        }
        _ => panic!("Expected FileSearchResult variant"),
    }
}

#[test]
fn test_serialize_file_search_result_step() {
    let step = Step::FileSearchResult {
        call_id: "call789".to_string(),
        result: vec![FileSearchResultItem {
            title: "Results.pdf".to_string(),
            text: "Found text".to_string(),
            store: "my-store".to_string(),
        }],
        signature: None,
    };

    let json = serde_json::to_string(&step).expect("Serialization should work");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["type"], "file_search_result");
    assert_eq!(value["call_id"], "call789");
    assert!(value["result"].is_array());
    assert_eq!(value["result"][0]["title"], "Results.pdf");
    assert_eq!(value["result"][0]["text"], "Found text");
    assert_eq!(value["result"][0]["file_search_store"], "my-store");
}

#[test]
fn test_file_search_result_step_roundtrip() {
    let original = Step::FileSearchResult {
        call_id: "roundtrip_test".to_string(),
        result: vec![
            FileSearchResultItem {
                title: "Doc1.pdf".to_string(),
                text: "Content one".to_string(),
                store: "store-1".to_string(),
            },
            FileSearchResultItem {
                title: "Doc2.pdf".to_string(),
                text: "Content two".to_string(),
                store: "store-2".to_string(),
            },
        ],
        signature: None,
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();

    match restored {
        Step::FileSearchResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "roundtrip_test");
            assert_eq!(result.len(), 2);
            assert_eq!(result[0].title, "Doc1.pdf");
            assert_eq!(result[0].text, "Content one");
            assert_eq!(result[1].title, "Doc2.pdf");
        }
        _ => panic!("Expected FileSearchResult variant"),
    }
}

#[test]
fn test_file_search_result_step_empty_results() {
    // Empty results are omitted from the wire but roundtrip to an empty vec
    let step = Step::FileSearchResult {
        call_id: "no_results".to_string(),
        result: vec![],
        signature: None,
    };

    let json = serde_json::to_string(&step).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    // The 2026-05-20 spec doesn't document a result field, so empty results
    // are not serialized.
    assert!(value.get("result").is_none());

    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::FileSearchResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "no_results");
            assert!(result.is_empty());
        }
        _ => panic!("Expected FileSearchResult variant"),
    }
}

#[test]
fn test_file_search_result_item_default() {
    let item = FileSearchResultItem::default();
    assert!(item.title.is_empty());
    assert!(item.text.is_empty());
    assert!(item.store.is_empty());
}

// =============================================================================
// Content Constructor Tests
// =============================================================================

#[test]
fn test_new_text_creates_correct_variant() {
    let content = Content::text("Hello world");
    match &content {
        Content::Text { text, annotations } => {
            assert_eq!(*text, Some("Hello world".to_string()));
            assert!(annotations.is_none());
        }
        _ => panic!("Expected Text variant"),
    }
    assert!(content.is_text());
    assert_eq!(content.as_text(), Some("Hello world"));
}

#[test]
fn test_new_text_with_empty_string() {
    let content = Content::text("");
    match &content {
        Content::Text { text, .. } => {
            assert_eq!(*text, Some(String::new()));
        }
        _ => panic!("Expected Text variant"),
    }
    // Empty string returns None from as_text() accessor
    assert_eq!(content.as_text(), None);
}

#[test]
fn test_new_image_data_creates_correct_variant() {
    let content = Content::image_data("base64encodeddata", "image/png");
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64encodeddata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_image_data_with_resolution_creates_correct_variant() {
    let content =
        Content::image_data_with_resolution("base64encodeddata", "image/png", Resolution::High);
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64encodeddata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_image_uri_creates_correct_variant() {
    let content = Content::image_uri("https://example.com/image.png", "image/png");
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/image.png".to_string()));
            assert_eq!(mime_type, Some("image/png".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_image_uri_with_resolution_creates_correct_variant() {
    let content = Content::image_uri_with_resolution(
        "https://example.com/image.png",
        "image/png",
        Resolution::Low,
    );
    match content {
        Content::Image {
            data,
            uri,
            resolution,
            ..
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/image.png".to_string()));
            assert_eq!(resolution, Some(Resolution::Low));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_new_audio_data_creates_correct_variant() {
    let content = Content::audio_data("base64audiodata", "audio/mp3");
    match content {
        Content::Audio {
            data,
            uri,
            mime_type,
            sample_rate,
            channels,
        } => {
            assert_eq!(data, Some("base64audiodata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("audio/mp3".to_string()));
            // Constructors no longer take a sample rate; both default to None
            assert!(sample_rate.is_none());
            assert!(channels.is_none());
        }
        _ => panic!("Expected Audio variant"),
    }
}

#[test]
fn test_new_audio_uri_creates_correct_variant() {
    let content = Content::audio_uri("https://example.com/audio.mp3", "audio/mp3");
    match content {
        Content::Audio {
            data,
            uri,
            mime_type,
            sample_rate,
            channels,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/audio.mp3".to_string()));
            assert_eq!(mime_type, Some("audio/mp3".to_string()));
            assert!(sample_rate.is_none());
            assert!(channels.is_none());
        }
        _ => panic!("Expected Audio variant"),
    }
}

#[test]
fn test_new_video_data_creates_correct_variant() {
    let content = Content::video_data("base64videodata", "video/mp4");
    match content {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64videodata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_data_with_resolution_creates_correct_variant() {
    let content =
        Content::video_data_with_resolution("base64videodata", "video/mp4", Resolution::Low);
    match content {
        Content::Video {
            data, resolution, ..
        } => {
            assert_eq!(data, Some("base64videodata".to_string()));
            assert_eq!(resolution, Some(Resolution::Low));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_uri_creates_correct_variant() {
    let content = Content::video_uri("https://example.com/video.mp4", "video/mp4");
    match content {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert!(resolution.is_none());
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_uri_with_resolution_creates_correct_variant() {
    let content = Content::video_uri_with_resolution(
        "https://example.com/video.mp4",
        "video/mp4",
        Resolution::Medium,
    );
    match content {
        Content::Video {
            uri, resolution, ..
        } => {
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::Medium));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_document_data_creates_correct_variant() {
    let content = Content::document_data("base64pdfdata", "application/pdf");
    match content {
        Content::Document {
            data,
            uri,
            mime_type,
        } => {
            assert_eq!(data, Some("base64pdfdata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("application/pdf".to_string()));
        }
        _ => panic!("Expected Document variant"),
    }
}

#[test]
fn test_new_document_uri_creates_correct_variant() {
    let content = Content::document_uri("https://example.com/doc.pdf", "application/pdf");
    match content {
        Content::Document {
            data,
            uri,
            mime_type,
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/doc.pdf".to_string()));
            assert_eq!(mime_type, Some("application/pdf".to_string()));
        }
        _ => panic!("Expected Document variant"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_image() {
    let content = Content::from_uri_and_mime("files/abc123", "image/png");
    match content {
        Content::Image { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("image/png".to_string()));
        }
        _ => panic!("Expected Image variant for image/* MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_audio() {
    let content = Content::from_uri_and_mime("files/abc123", "audio/wav");
    match content {
        Content::Audio { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("audio/wav".to_string()));
        }
        _ => panic!("Expected Audio variant for audio/* MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_video() {
    let content = Content::from_uri_and_mime("files/abc123", "video/webm");
    match content {
        Content::Video { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("video/webm".to_string()));
        }
        _ => panic!("Expected Video variant for video/* MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_document_for_pdf() {
    let content = Content::from_uri_and_mime("files/abc123", "application/pdf");
    match content {
        Content::Document { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("application/pdf".to_string()));
        }
        _ => panic!("Expected Document variant for application/pdf MIME type"),
    }
}

#[test]
fn test_from_uri_and_mime_infers_document_for_text() {
    let content = Content::from_uri_and_mime("files/abc123", "text/plain");
    match content {
        Content::Document { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/abc123".to_string()));
            assert_eq!(mime_type, Some("text/plain".to_string()));
        }
        _ => panic!("Expected Document variant for text/* MIME type"),
    }
}

#[test]
fn test_constructors_accept_string_types() {
    // Test that constructors work with various string types via Into<String>
    let text1 = Content::text(String::from("owned string"));
    assert_eq!(text1.as_text(), Some("owned string"));

    let text2 = Content::text("&str literal");
    assert_eq!(text2.as_text(), Some("&str literal"));

    // Cow, Box<str>, etc. would also work via Into<String>
}

#[test]
fn test_constructor_serialization_roundtrip() {
    // Test that content created via constructors serializes correctly
    let content = Content::text("Test message");
    let json = serde_json::to_string(&content).expect("Should serialize");
    let deserialized: Content = serde_json::from_str(&json).expect("Should deserialize");
    assert_eq!(deserialized.as_text(), Some("Test message"));

    let image = Content::image_data_with_resolution("data", "image/png", Resolution::High);
    let json = serde_json::to_string(&image).expect("Should serialize");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "data");
    assert_eq!(value["resolution"], "high");
}

// =============================================================================
// GoogleSearchResultItem / FileSearchResultItem Constructor Tests
// =============================================================================

#[test]
fn test_google_search_result_item_new() {
    let item = GoogleSearchResultItem::new("Rust Lang", "https://rust-lang.org");
    assert_eq!(item.title, "Rust Lang");
    assert_eq!(item.url, "https://rust-lang.org");
    assert!(!item.has_rendered_content());
    assert!(item.rendered_content.is_none());
    assert!(item.search_suggestions.is_none());
}

#[test]
fn test_file_search_result_item_new() {
    let item = FileSearchResultItem::new("Document Title", "Extracted text content", "my-store");
    assert_eq!(item.title, "Document Title");
    assert_eq!(item.text, "Extracted text content");
    assert_eq!(item.store, "my-store");
    assert!(item.has_text());
}

#[test]
fn test_file_search_result_item_has_text_empty() {
    let item = FileSearchResultItem::new("Title", "", "store");
    assert!(!item.has_text());
}

// =============================================================================
// Builder Method Tests (with_resolution)
// =============================================================================

#[test]
fn test_with_resolution_on_image() {
    let content = Content::image_uri("files/abc123", "image/png").with_resolution(Resolution::High);
    match content {
        Content::Image { resolution, .. } => {
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_with_resolution_on_video() {
    let content = Content::video_uri("files/def456", "video/mp4").with_resolution(Resolution::Low);
    match content {
        Content::Video { resolution, .. } => {
            assert_eq!(resolution, Some(Resolution::Low));
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_with_resolution_preserves_other_fields() {
    let content =
        Content::image_data("base64data", "image/jpeg").with_resolution(Resolution::UltraHigh);
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
        } => {
            assert_eq!(data, Some("base64data".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("image/jpeg".to_string()));
            assert_eq!(resolution, Some(Resolution::UltraHigh));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_with_resolution_on_non_media_returns_unchanged() {
    // with_resolution on non-media content should return unchanged
    let original = Content::text("Hello");
    let after = original.clone().with_resolution(Resolution::High);
    assert_eq!(original, after);
}

#[test]
fn test_with_resolution_on_audio_returns_unchanged() {
    // Audio content doesn't support resolution (unlike Image/Video)
    let original = Content::audio_uri("files/abc123", "audio/mp3");
    let after = original.clone().with_resolution(Resolution::High);
    assert_eq!(original, after);
}

#[test]
fn test_with_resolution_overwrites_existing() {
    let content = Content::image_uri("files/abc123", "image/png")
        .with_resolution(Resolution::Low)
        .with_resolution(Resolution::High);
    match content {
        Content::Image { resolution, .. } => {
            assert_eq!(resolution, Some(Resolution::High));
        }
        _ => panic!("Expected Image variant"),
    }
}

#[test]
fn test_with_resolution_unknown_variant() {
    // Unknown resolution variants can be used in builder and roundtrip
    let unknown_res = Resolution::Unknown {
        resolution_type: "ULTRA_MEGA_HD".to_string(),
        data: serde_json::json!("ULTRA_MEGA_HD"),
    };
    let content = Content::image_uri("files/abc123", "image/png").with_resolution(unknown_res);

    match content {
        Content::Image { resolution, .. } => {
            let res = resolution.expect("Should have resolution");
            assert!(res.is_unknown());
            assert_eq!(res.unknown_resolution_type(), Some("ULTRA_MEGA_HD"));
        }
        _ => panic!("Expected Image variant"),
    }
}

// =============================================================================
// from_uri_and_mime MIME Type Routing Tests
// =============================================================================

#[test]
fn test_from_uri_and_mime_image_types() {
    for mime in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Image { .. }),
            "Expected Image for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_audio_types() {
    for mime in ["audio/mp3", "audio/wav", "audio/ogg", "audio/mpeg"] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Audio { .. }),
            "Expected Audio for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_video_types() {
    for mime in ["video/mp4", "video/webm", "video/quicktime"] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Video { .. }),
            "Expected Video for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_document_fallback() {
    // PDFs, text files, and unknown types all become Document
    for mime in [
        "application/pdf",
        "text/plain",
        "text/csv",
        "application/json",
        "application/octet-stream",
    ] {
        let content = Content::from_uri_and_mime("files/abc123", mime);
        assert!(
            matches!(content, Content::Document { .. }),
            "Expected Document for {mime}"
        );
    }
}

#[test]
fn test_from_uri_and_mime_preserves_values() {
    let content = Content::from_uri_and_mime("files/test123", "image/png");
    match content {
        Content::Image { uri, mime_type, .. } => {
            assert_eq!(uri, Some("files/test123".to_string()));
            assert_eq!(mime_type, Some("image/png".to_string()));
        }
        _ => panic!("Expected Image variant"),
    }
}
