//! Round-trip fidelity of unknown fields on known variants (#491).
//!
//! D-001 says unknown API data survives a round trip. Unknown *types* always
//! did, through the `Unknown` variants. These tests cover unknown *fields*
//! on known types: the steps a caller replays as stateless history, the
//! content inside them, and the nested result items, annotations and
//! deltas. A field the API adds must come back out unchanged, as
//! `signature` would have, had it been preserved before it was modeled.

use genai_rs::{Content, Step};
use serde_json::{Value, json};

fn round_trip<T>(wire: &Value) -> Value
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let parsed: T = serde_json::from_value(wire.clone())
        .unwrap_or_else(|e| panic!("{wire} should deserialize: {e}"));
    serde_json::to_value(&parsed).expect("should serialize")
}

// =============================================================================
// The three probe-verified failures from #491
// =============================================================================

#[test]
fn function_call_keeps_an_unknown_top_level_field() {
    let wire = json!({
        "type": "function_call",
        "id": "call_1",
        "name": "get_weather",
        "arguments": {"city": "Paris"},
        "signature": "sig",
        "future_field": {"nested": [1, 2, 3]}
    });
    assert_eq!(round_trip::<Step>(&wire), wire);
}

#[test]
fn url_context_call_keeps_an_unknown_argument() {
    let wire = json!({
        "type": "url_context_call",
        "id": "call_2",
        "arguments": {"urls": ["https://example.com"], "max_pages": 3}
    });
    assert_eq!(round_trip::<Step>(&wire), wire);
}

#[test]
fn text_content_keeps_an_unknown_field() {
    let wire = json!({"type": "text", "text": "hi", "future_field": 1});
    assert_eq!(round_trip::<Content>(&wire), wire);
}

// =============================================================================
// Every known step, with unknown keys injected at every depth
// =============================================================================

/// One wire object per known step type, in the shapes the API sends (see
/// `docs/ENUM_WIRE_FORMATS.md`), with nested content, annotations, result
/// items, places and review snippets where the type carries them.
fn known_steps() -> Vec<Value> {
    vec![
        json!({"type": "user_input", "content": [
            {"type": "text", "text": "hi"},
            {"type": "video", "uri": "files/v", "mime_type": "video/mp4",
             "processing": {"type": "static", "fps": 1.0}}
        ]}),
        json!({"type": "model_output", "content": [
            {"type": "text", "text": "cited", "annotations": [
                {"type": "url_citation", "url": "https://a.example", "start_index": 0, "end_index": 5},
                {"type": "place_citation", "name": "Cafe", "start_index": 0, "end_index": 5,
                 "review_snippets": [{"title": "Good", "review_id": "r1"}]}
            ]},
            {"type": "image", "data": "AAAA", "mime_type": "image/png"},
            {"type": "audio", "data": "AAAA", "mime_type": "audio/wav", "sample_rate": 24000}
        ], "error": {"code": 3, "message": "partial"}}),
        json!({"type": "thought", "signature": "sig", "summary": [{"type": "text", "text": "hm"}]}),
        json!({"type": "function_call", "id": "c1", "name": "f", "arguments": {"a": 1}, "signature": "sig"}),
        json!({"type": "function_result", "call_id": "c1", "name": "f", "result": {"ok": true}, "is_error": false, "signature": "sig"}),
        json!({"type": "code_execution_call", "id": "c2", "arguments": {"language": "python", "code": "print(1)"}, "signature": "sig"}),
        json!({"type": "code_execution_result", "call_id": "c2", "result": "1\n", "is_error": false, "signature": "sig"}),
        json!({"type": "url_context_call", "id": "c3", "arguments": {"urls": ["https://a.example"]}, "signature": "sig"}),
        json!({"type": "url_context_result", "call_id": "c3", "result": [{"url": "https://a.example", "status": "success"}], "signature": "sig"}),
        json!({"type": "google_search_call", "id": "c4", "arguments": {"queries": ["rust"]}, "search_type": "web_search", "signature": "sig"}),
        json!({"type": "google_search_result", "call_id": "c4", "result": [{"title": "Rust", "url": "https://rust-lang.org"}, {"search_suggestions": "<div/>"}], "signature": "sig"}),
        json!({"type": "tool_call", "id": "c5", "signature": "sig"}),
        json!({"type": "mcp_server_tool_call", "id": "c6", "name": "search", "server_name": "docs", "arguments": {"q": "x"}}),
        json!({"type": "mcp_server_tool_result", "call_id": "c6", "name": "search", "server_name": "docs", "result": "found"}),
        json!({"type": "file_search_call", "id": "c7", "signature": "sig"}),
        json!({"type": "file_search_result", "call_id": "c7", "result": [{"title": "Doc", "text": "chunk", "file_search_store": "stores/s"}], "signature": "sig"}),
        json!({"type": "google_maps_call", "id": "c8", "arguments": {"queries": ["cafes"]}, "signature": "sig"}),
        json!({"type": "google_maps_result", "call_id": "c8", "result": [{"places": [
            {"name": "Cafe", "place_id": "p1", "review_snippets": [{"title": "Nice"}]}
        ], "widget_context_token": "w"}], "signature": "sig"}),
        json!({"type": "processing_call", "id": "c9", "signature": "sig"}),
        json!({"type": "processing_result", "call_id": "c9", "signature": "sig"}),
        json!({"type": "retrieval_call", "id": "c10", "arguments": {"queries": ["q"]}, "retrieval_type": "rag_store", "signature": "sig"}),
        json!({"type": "retrieval_result", "call_id": "c10", "is_error": false, "signature": "sig"}),
    ]
}

/// Adds `x_future` to every object in `value`, at every depth.
fn inject_everywhere(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.values_mut().for_each(inject_everywhere);
            map.insert("x_future".into(), json!({"n": [1, "two"]}));
        }
        Value::Array(items) => items.iter_mut().for_each(inject_everywhere),
        _ => {}
    }
}

#[test]
fn every_known_step_keeps_unknown_keys_at_every_depth() {
    let steps = known_steps();
    let types: std::collections::BTreeSet<&str> =
        steps.iter().map(|s| s["type"].as_str().unwrap()).collect();
    assert_eq!(types.len(), 22, "one fixture per known step type");

    for mut wire in steps {
        inject_everywhere(&mut wire);
        let step: Step = serde_json::from_value(wire.clone())
            .unwrap_or_else(|e| panic!("{wire} should deserialize: {e}"));
        assert!(!step.is_unknown(), "{wire} should stay a known step");
        assert_eq!(serde_json::to_value(&step).unwrap(), wire);

        let extra = step.extra().expect("a known step has extras");
        assert_eq!(extra.get("x_future"), Some(&json!({"n": [1, "two"]})));
        // The five reshaped calls keep unknown argument keys in
        // `arguments_extra`; the others keep `arguments` raw.
        if let Some(arguments_extra) = step.arguments_extra() {
            assert!(arguments_extra.contains_key("x_future"), "{wire}");
        }
    }
}

#[test]
fn every_known_step_round_trips_through_a_response() {
    let mut steps = Value::Array(known_steps());
    inject_everywhere(&mut steps);
    let wire = json!({"id": "i1", "status": "completed", "steps": steps});
    let response: genai_rs::InteractionResponse = serde_json::from_value(wire.clone()).unwrap();
    // Replaying the steps as stateless history sends them back as they came.
    assert_eq!(
        serde_json::to_value(&response.steps).unwrap(),
        wire["steps"]
    );
}

#[test]
fn extras_never_shadow_a_modeled_field_or_the_type() {
    let wire = json!({
        "type": "url_context_call",
        "id": "c1",
        "arguments": {"urls": ["https://a.example"]}
    });
    let mut step: Step = serde_json::from_value(wire.clone()).unwrap();
    let extra = step.extra_mut().unwrap();
    extra.insert("type".into(), json!("function_call"));
    extra.insert("id".into(), json!("forged"));
    extra.insert("signature".into(), json!("forged"));
    step.arguments_extra_mut()
        .unwrap()
        .insert("urls".into(), json!(["https://forged.example"]));
    assert_eq!(serde_json::to_value(&step).unwrap(), wire);
}

#[test]
fn malformed_known_steps_are_kept_whole_as_unknown() {
    // A non-object `arguments` cannot be modeled, so the step stays whole.
    let wire = json!({"type": "url_context_call", "id": "c1", "arguments": "https://a.example"});
    let parsed = serde_json::from_value::<Step>(wire.clone());
    if cfg!(feature = "strict-unknown") {
        // strict-unknown rejects what would be `Unknown`.
        assert!(parsed.is_err());
    } else {
        let step = parsed.unwrap();
        assert!(step.is_unknown());
        assert_eq!(serde_json::to_value(&step).unwrap(), wire);
    }
}
