use super::*;

use serde_json::json;

/// The exact `tool_call` payload observed live 2026-08-16
/// (`gemini-3.7-flash`, `mcp.deepwiki.com`): three keys, nothing else.
#[test]
fn tool_call_step_deserializes_the_observed_mcp_shape() {
    let wire = json!({
        "type": "tool_call",
        "id": "call_abc123",
        "signature": "opaque"
    });
    let step: Step = serde_json::from_value(wire.clone()).unwrap();

    match &step {
        Step::ToolCall { id, signature } => {
            assert_eq!(id, "call_abc123");
            assert_eq!(signature.as_deref(), Some("opaque"));
        }
        other => panic!("expected Step::ToolCall, got {other:?}"),
    }
    assert!(
        !step.is_unknown(),
        "tool_call must not fall through to Unknown — that is what made a \
             successful MCP call report zero tool calls"
    );
    assert_eq!(
        step.signature(),
        Some("opaque"),
        "signature() must report it — the API sends one on every observed \
             tool_call step, and a caller collecting signatures for stateless \
             replay would silently drop it"
    );
    assert_eq!(serde_json::to_value(&step).unwrap(), wire);
}

/// `signature` is optional; the API has been seen to send it, but the
/// spec does not require it.
#[test]
fn tool_call_step_allows_a_missing_signature() {
    let wire = json!({"type": "tool_call", "id": "call_1"});
    let step: Step = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        &step,
        Step::ToolCall {
            signature: None,
            ..
        }
    ));
    assert_eq!(serde_json::to_value(&step).unwrap(), wire);
}

// =========================================================================
// Step wire fixtures (shapes derived from google-genai 2.10 generated
// bindings for API revision 2026-05-20)
// =========================================================================

#[test]
fn test_step_user_input_roundtrip() {
    let json_str = r#"{"type":"user_input","content":[{"type":"text","text":"Hello"}]}"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    assert!(matches!(&step, Step::UserInput { content } if content.len() == 1));
    assert_eq!(step.as_text(), Some("Hello"));

    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["type"], "user_input");
    assert_eq!(out["content"][0]["text"], "Hello");
}

#[test]
fn test_step_model_output_with_error() {
    let json_str = r#"{
            "type": "model_output",
            "content": [{"type": "text", "text": "Partial"}],
            "error": {"code": 8, "message": "quota exhausted"}
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::ModelOutput { content, error } => {
            assert_eq!(content.len(), 1);
            let error = error.as_ref().unwrap();
            assert_eq!(error.code, Some(8));
            assert_eq!(error.message.as_deref(), Some("quota exhausted"));
        }
        other => panic!("Expected ModelOutput, got {other:?}"),
    }
}

#[test]
fn test_step_thought_roundtrip() {
    let json_str = r#"{
            "type": "thought",
            "signature": "sig-abc",
            "summary": [{"type": "text", "text": "Thinking about it"}]
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::Thought { signature, summary } => {
            assert_eq!(signature.as_deref(), Some("sig-abc"));
            assert_eq!(summary.len(), 1);
        }
        other => panic!("Expected Thought, got {other:?}"),
    }
    assert_eq!(step.signature(), Some("sig-abc"));

    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["type"], "thought");
    assert_eq!(out["signature"], "sig-abc");
}

#[test]
fn test_step_function_call_roundtrip() {
    let json_str = r#"{
            "type": "function_call",
            "id": "call_1",
            "name": "get_weather",
            "arguments": {"city": "Tokyo"}
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::FunctionCall {
            id,
            name,
            arguments,
            signature,
        } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "get_weather");
            assert_eq!(arguments["city"], "Tokyo");
            assert_eq!(*signature, None);
        }
        other => panic!("Expected FunctionCall, got {other:?}"),
    }

    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["type"], "function_call");
    assert_eq!(out["id"], "call_1");
    assert_eq!(out["arguments"]["city"], "Tokyo");
    // No signature was present, so none is emitted.
    assert!(out.get("signature").is_none());
}

#[test]
fn test_step_function_call_roundtrip_preserves_signature() {
    // Verified live 2026-07: the API returns `signature` on function_call
    // steps and rejects stateless replay without it.
    let json_str = r#"{
            "type": "function_call",
            "id": "call_2",
            "name": "get_weather",
            "arguments": {"city": "Paris"},
            "signature": "sig-fc-123"
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::FunctionCall { signature, .. } => {
            assert_eq!(signature.as_deref(), Some("sig-fc-123"));
        }
        other => panic!("Expected FunctionCall, got {other:?}"),
    }
    assert_eq!(step.signature(), Some("sig-fc-123"));

    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["type"], "function_call");
    assert_eq!(out["signature"], "sig-fc-123");
}

#[test]
fn test_step_function_result_roundtrip_preserves_signature() {
    let json_str = r#"{
            "type": "function_result",
            "call_id": "call_2",
            "name": "get_weather",
            "result": "sunny",
            "signature": "sig-fr-456"
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::FunctionResult { signature, .. } => {
            assert_eq!(signature.as_deref(), Some("sig-fr-456"));
        }
        other => panic!("Expected FunctionResult, got {other:?}"),
    }
    assert_eq!(step.signature(), Some("sig-fr-456"));

    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["type"], "function_result");
    assert_eq!(out["signature"], "sig-fr-456");

    // Constructors leave the signature unset.
    let constructed = Step::function_result("get_weather", "call_2", "sunny");
    let out = serde_json::to_value(&constructed).unwrap();
    assert!(out.get("signature").is_none());
}

#[test]
fn test_step_function_result_payload_union() {
    // String result
    let s: Step =
        serde_json::from_str(r#"{"type":"function_result","call_id":"c1","result":"22 degrees"}"#)
            .unwrap();
    match &s {
        Step::FunctionResult { result, .. } => {
            assert_eq!(result.as_text(), Some("22 degrees"));
        }
        other => panic!("Expected FunctionResult, got {other:?}"),
    }

    // Object result
    let s: Step = serde_json::from_str(
        r#"{"type":"function_result","call_id":"c2","result":{"temp":22},"is_error":false}"#,
    )
    .unwrap();
    match &s {
        Step::FunctionResult {
            result, is_error, ..
        } => {
            assert_eq!(result.as_json().unwrap()["temp"], 22);
            assert_eq!(*is_error, Some(false));
        }
        other => panic!("Expected FunctionResult, got {other:?}"),
    }

    // Content-block list result
    let s: Step = serde_json::from_str(
        r#"{"type":"function_result","call_id":"c3","result":[{"type":"text","text":"hi"}]}"#,
    )
    .unwrap();
    match &s {
        Step::FunctionResult { result, .. } => {
            let contents = result.as_contents().unwrap();
            assert_eq!(contents.len(), 1);
            assert_eq!(contents[0].as_text(), Some("hi"));
        }
        other => panic!("Expected FunctionResult, got {other:?}"),
    }
}

#[test]
fn test_step_code_execution_nested_arguments_roundtrip() {
    let json_str = r#"{
            "type": "code_execution_call",
            "id": "exec_1",
            "arguments": {"language": "python", "code": "print(42)"},
            "signature": "sig-1"
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::CodeExecutionCall {
            id,
            language,
            code,
            signature,
        } => {
            assert_eq!(id, "exec_1");
            assert_eq!(*language, CodeExecutionLanguage::Python);
            assert_eq!(code, "print(42)");
            assert_eq!(signature.as_deref(), Some("sig-1"));
        }
        other => panic!("Expected CodeExecutionCall, got {other:?}"),
    }

    // Serialization nests language/code back under arguments.
    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["arguments"]["language"], "python");
    assert_eq!(out["arguments"]["code"], "print(42)");
    assert_eq!(out["signature"], "sig-1");
}

#[test]
fn test_step_google_search_call_with_search_type() {
    let json_str = r#"{
            "type": "google_search_call",
            "id": "s1",
            "arguments": {"queries": ["rust serde"]},
            "search_type": "web_search",
            "signature": "sig"
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::GoogleSearchCall {
            queries,
            search_type,
            ..
        } => {
            assert_eq!(queries, &["rust serde".to_string()]);
            assert!(matches!(
                search_type,
                Some(crate::tools::SearchType::WebSearch)
            ));
        }
        other => panic!("Expected GoogleSearchCall, got {other:?}"),
    }
}

#[test]
fn test_step_mcp_server_tool_call_roundtrip() {
    let json_str = r#"{
            "type": "mcp_server_tool_call",
            "id": "m1",
            "name": "read_file",
            "server_name": "fs",
            "arguments": {"path": "/tmp/x"}
        }"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    match &step {
        Step::McpServerToolCall {
            name, server_name, ..
        } => {
            assert_eq!(name, "read_file");
            assert_eq!(server_name, "fs");
        }
        other => panic!("Expected McpServerToolCall, got {other:?}"),
    }
    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["server_name"], "fs");
}

#[test]
#[cfg(not(feature = "strict-unknown"))]
fn test_step_unknown_preserves_data_and_roundtrips() {
    let json_str = r#"{"type":"quantum_step","novel_field":42}"#;
    let step: Step = serde_json::from_str(json_str).unwrap();
    assert!(step.is_unknown());
    assert_eq!(step.unknown_step_type(), Some("quantum_step"));
    assert_eq!(step.unknown_data().unwrap()["novel_field"], 42);

    let out = serde_json::to_value(&step).unwrap();
    assert_eq!(out["type"], "quantum_step");
    assert_eq!(out["novel_field"], 42);
}

#[test]
fn test_step_type_accessor() {
    assert_eq!(Step::user_text("x").step_type(), "user_input");
    assert_eq!(
        Step::function_call("id", "f", json!({})).step_type(),
        "function_call"
    );
    assert_eq!(
        Step::Unknown {
            step_type: "future".into(),
            data: serde_json::Value::Null
        }
        .step_type(),
        "future"
    );
}

// =========================================================================
// FunctionResultPayload
// =========================================================================

#[test]
fn test_function_result_payload_from_value() {
    assert!(matches!(
        FunctionResultPayload::from_value(json!("text")),
        FunctionResultPayload::Text(_)
    ));
    assert!(matches!(
        FunctionResultPayload::from_value(json!({"a": 1})),
        FunctionResultPayload::Json(_)
    ));
    assert!(matches!(
        FunctionResultPayload::from_value(json!([{"type": "text", "text": "hi"}])),
        FunctionResultPayload::Contents(_)
    ));
    // Non-content array stays raw JSON
    assert!(matches!(
        FunctionResultPayload::from_value(json!([1, 2, 3])),
        FunctionResultPayload::Json(_)
    ));
}

#[test]
fn test_function_result_payload_roundtrip() {
    for payload in [
        FunctionResultPayload::Text("hello".into()),
        FunctionResultPayload::Json(json!({"k": [1, 2]})),
        FunctionResultPayload::Contents(vec![Content::text("block")]),
    ] {
        let serialized = serde_json::to_string(&payload).unwrap();
        let back: FunctionResultPayload = serde_json::from_str(&serialized).unwrap();
        assert_eq!(payload, back);
    }
}

// =========================================================================
// processing_* / retrieval_* steps (google-genai 2.21+ / 2.24+ bindings)
// =========================================================================

/// Live shape, `gemini-3.8-flash`, video with `processing: "agentic"`
/// (2026-09-24). Signatures shortened.
#[test]
fn processing_steps_roundtrip_the_live_shape() {
    let call = json!({"type": "processing_call", "id": "call_140238", "signature": "EqnS"});
    let result =
        json!({"type": "processing_result", "call_id": "call_140238", "signature": "ErrR"});

    let step: Step = serde_json::from_value(call.clone()).unwrap();
    assert!(matches!(&step, Step::ProcessingCall { id, .. } if id == "call_140238"));
    assert_eq!(step.signature(), Some("EqnS"));
    assert_eq!(step.step_type(), "processing_call");
    assert_eq!(serde_json::to_value(&step).unwrap(), call);

    let step: Step = serde_json::from_value(result.clone()).unwrap();
    assert!(matches!(&step, Step::ProcessingResult { call_id, .. } if call_id == "call_140238"));
    assert_eq!(step.signature(), Some("ErrR"));
    assert_eq!(serde_json::to_value(&step).unwrap(), result);
}

#[test]
fn retrieval_steps_roundtrip_the_binding_shape() {
    let call = json!({
        "type": "retrieval_call",
        "id": "call_1",
        "arguments": {"queries": ["rust serde"]},
        "retrieval_type": "vertex_ai_search",
        "signature": "sig"
    });
    let step: Step = serde_json::from_value(call.clone()).unwrap();
    match &step {
        Step::RetrievalCall {
            queries,
            retrieval_type,
            ..
        } => {
            assert_eq!(queries, &["rust serde"]);
            assert_eq!(
                retrieval_type,
                &Some(crate::tools::RetrievalType::VertexAiSearch)
            );
        }
        other => panic!("expected RetrievalCall, got {other:?}"),
    }
    assert_eq!(serde_json::to_value(&step).unwrap(), call);

    let result = json!({"type": "retrieval_result", "call_id": "call_1", "is_error": false});
    let step: Step = serde_json::from_value(result.clone()).unwrap();
    assert!(matches!(
        step,
        Step::RetrievalResult {
            is_error: Some(false),
            ..
        }
    ));
    assert_eq!(serde_json::to_value(&step).unwrap(), result);
}
