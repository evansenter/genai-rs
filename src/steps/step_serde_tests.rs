//! The wire format of the tool-call, tool-result and thought steps, which
//! were `Content` variants before API revision 2026-05-20, alone and inside a
//! response.

use crate::*;

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
            ..
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
            ..
        } => {
            assert_eq!(id, "call-abc123");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments, serde_json::Value::Null);
        }
        _ => panic!("Expected FunctionCall variant, got {step:?}"),
    }
}

#[cfg(not(genai_strict_unknown))]
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
        extra: Default::default(),
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
            ..
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
            ..
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
            ..
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
        Step::ModelOutput { content, error, .. } => {
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

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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

    // Any other value is wrapped: the API rejects a top-level array
    for value in [
        serde_json::json!([1, 2, 3]),
        serde_json::json!([{"type": "text", "text": "not converted to Contents"}]),
        serde_json::json!(42),
        serde_json::json!(true),
        serde_json::Value::Null,
    ] {
        let payload: FunctionResultPayload = value.clone().into();
        assert_eq!(
            payload.to_value(),
            serde_json::json!({"result": value}),
            "{value}"
        );
    }

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
            ..
        } => {
            assert_eq!(id, "call_123");
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print(42)");
            assert!(signature.is_none());
        }
        _ => panic!("Expected CodeExecutionCall variant, got {step:?}"),
    }

    assert!(!step.is_unknown());
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
        _ => panic!("Expected CodeExecutionCall variant, got {step:?}"),
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
            ..
        } => {
            assert_eq!(call_id, "call_123");
            assert!(!is_error);
            assert_eq!(result.as_text(), Some("42\n"));
            assert!(signature.is_none());
        }
        _ => panic!("Expected CodeExecutionResult variant, got {step:?}"),
    }
}

/// The list arm of the result union (google-genai 3.0.0) reads as content
/// blocks and is written back as the list.
#[test]
fn code_execution_result_step_in_the_list_form_round_trips() {
    let wire = serde_json::json!({
        "type": "code_execution_result",
        "call_id": "call_7",
        "is_error": false,
        "result": [
            {"type": "text", "text": "plotted\n"},
            {"type": "image", "data": "iVBORw0KGgo=", "mime_type": "image/png"}
        ]
    });
    let step: Step = serde_json::from_value(wire.clone()).expect("Should deserialize");
    match &step {
        Step::CodeExecutionResult { result, extra, .. } => {
            assert_eq!(result.as_contents().map(<[_]>::len), Some(2));
            assert_eq!(result.text(), "plotted\n");
            assert!(extra.is_empty(), "{extra:?}");
        }
        _ => panic!("Expected CodeExecutionResult variant, got {step:?}"),
    }
    assert_eq!(serde_json::to_value(&step).unwrap(), wire);
}

/// A `result` in neither form cannot be modeled, so the step is kept whole
/// as `Unknown` (strict mode rejects it).
#[test]
fn code_execution_result_in_another_shape_is_kept_as_unknown() {
    let wire = serde_json::json!({
        "type": "code_execution_result", "call_id": "call_8", "result": {"stdout": "x"}
    });
    let read = serde_json::from_value::<Step>(wire.clone());
    #[cfg(genai_strict_unknown)]
    assert!(read.is_err());
    #[cfg(not(genai_strict_unknown))]
    {
        let step = read.expect("Should deserialize");
        assert!(step.is_unknown(), "{step:?}");
        assert_eq!(serde_json::to_value(&step).unwrap(), wire);
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
            assert!(result.text().contains("NameError"));
        }
        _ => panic!("Expected CodeExecutionResult variant, got {step:?}"),
    }
}

#[test]
fn test_serialize_code_execution_call_step() {
    let step = Step::CodeExecutionCall {
        id: "call_123".to_string(),
        language: CodeExecutionLanguage::Python,
        code: "print(42)".to_string(),
        signature: None,
        arguments_extra: Default::default(),
        extra: Default::default(),
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
        result: "42".into(),
        signature: None,
        extra: Default::default(),
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
        result: "NameError: x not defined".into(),
        signature: None,
        extra: Default::default(),
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
        arguments_extra: Default::default(),
        extra: Default::default(),
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
        arguments_extra: Default::default(),
        extra: Default::default(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::CodeExecutionCall { .. }));

    // CodeExecutionResult roundtrip
    let original = Step::CodeExecutionResult {
        call_id: "call_123".to_string(),
        is_error: false,
        result: "hello\n".into(),
        signature: None,
        extra: Default::default(),
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
        arguments_extra: Default::default(),
        extra: Default::default(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::GoogleSearchCall { queries, .. } => assert_eq!(queries, vec!["test query"]),
        other => panic!("Expected GoogleSearchCall variant, got {other:?}"),
    }

    // GoogleSearchResult roundtrip
    let original = Step::GoogleSearchResult {
        call_id: "call123".to_string(),
        result: vec![],
        is_error: None,
        signature: None,
        extra: Default::default(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    assert!(matches!(restored, Step::GoogleSearchResult { .. }));

    // UrlContextCall roundtrip
    let original = Step::UrlContextCall {
        id: "ctx_123".to_string(),
        urls: vec!["https://example.com".to_string()],
        signature: None,
        arguments_extra: Default::default(),
        extra: Default::default(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::UrlContextCall { urls, .. } => assert_eq!(urls, vec!["https://example.com"]),
        other => panic!("Expected UrlContextCall variant, got {other:?}"),
    }

    // UrlContextResult roundtrip
    let original = Step::UrlContextResult {
        call_id: "ctx_123".to_string(),
        result: vec![UrlContextResultItem::new("https://example.com", "success")],
        is_error: None,
        signature: None,
        extra: Default::default(),
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
        arguments_extra: Default::default(),
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        result: crate::CodeExecutionOutput::default(),
        signature: None,
        extra: Default::default(),
    };
    let json = serde_json::to_string(&step).unwrap();
    let restored: Step = serde_json::from_str(&json).unwrap();
    match restored {
        Step::CodeExecutionResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "call_no_output");
            assert_eq!(result.as_text(), Some(""));
        }
        _ => panic!("Expected CodeExecutionResult"),
    }
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
        _ => panic!("Expected GoogleSearchCall variant, got {step:?}"),
    }

    assert!(!step.is_unknown());
}

#[test]
fn test_deserialize_google_search_result_step() {
    // Wire format (live 2026-11-05): items carry only `search_suggestions`.
    let json = r#"{"type": "google_search_result", "call_id": "call123", "result": [{"search_suggestions": "<div/>"}]}"#;
    let step: Step = serde_json::from_str(json).expect("Should deserialize");

    match &step {
        Step::GoogleSearchResult {
            call_id, result, ..
        } => {
            assert_eq!(call_id, "call123");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].search_suggestions.as_deref(), Some("<div/>"));
            assert!(result[0].extra.is_empty());
        }
        _ => panic!("Expected GoogleSearchResult variant, got {step:?}"),
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
        _ => panic!("Expected UrlContextCall variant, got {step:?}"),
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
        _ => panic!("Expected UrlContextResult variant, got {step:?}"),
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
        extra: Default::default(),
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
        "model": "test-model",
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

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_deserialize_response_with_unknown_steps() {
    // Test deserializing a full response that contains truly unknown steps
    let response_json = r#"{
        "id": "interaction_789",
        "model": "test-model",
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

// --- File Search Steps ---

#[test]
fn test_file_search_result_step_wire_shape() {
    // What a stored interaction serves (live 2026-11-05): no `result`.
    let wire = serde_json::json!({
        "type": "file_search_result",
        "call_id": "call123",
        "signature": "c2ln"
    });
    let step: Step = serde_json::from_value(wire.clone()).expect("Should deserialize");
    match &step {
        Step::FileSearchResult {
            call_id,
            signature,
            extra,
        } => {
            assert_eq!(call_id, "call123");
            assert_eq!(signature.as_deref(), Some("c2ln"));
            assert!(extra.is_empty());
        }
        _ => panic!("Expected FileSearchResult variant, got {step:?}"),
    }
    assert_eq!(serde_json::to_value(&step).unwrap(), wire);
}

#[test]
fn test_file_search_result_step_keeps_a_result_it_is_sent() {
    // Neither the bindings nor the spec give the step a `result`; one that
    // arrives is kept in `extra`, unchanged.
    let wire = serde_json::json!({
        "type": "file_search_result",
        "call_id": "call456",
        "result": [{"title": "First.pdf", "text": "First content"}]
    });
    let step: Step = serde_json::from_value(wire.clone()).expect("Should deserialize");
    assert_eq!(step.extra().unwrap()["result"][0]["title"], "First.pdf");
    assert_eq!(serde_json::to_value(&step).unwrap(), wire);
}
