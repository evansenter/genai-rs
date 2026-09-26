use super::*;

// =========================================================================
// ServiceTier Tests
// =========================================================================

#[test]
fn test_service_tier_roundtrip() {
    for (tier, wire) in [
        (ServiceTier::Flex, "\"flex\""),
        (ServiceTier::Standard, "\"standard\""),
        (ServiceTier::Priority, "\"priority\""),
    ] {
        assert_eq!(serde_json::to_string(&tier).unwrap(), wire);
        let parsed: ServiceTier = serde_json::from_str(wire).unwrap();
        assert_eq!(parsed, tier);
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_service_tier_unknown_roundtrip() {
    let unknown: ServiceTier = serde_json::from_str("\"turbo\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_tier_type(), Some("turbo"));
    assert!(unknown.unknown_data().is_some());
    assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"turbo\"");
}

// =========================================================================
// InteractionInput Tests
// =========================================================================

#[test]
fn test_interaction_input_text_roundtrip() {
    let input = InteractionInput::Text("Hello".into());
    let json = serde_json::to_string(&input).unwrap();
    assert_eq!(json, "\"Hello\"");
    let back: InteractionInput = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, InteractionInput::Text(t) if t == "Hello"));
}

#[test]
fn test_interaction_input_content_array_roundtrip() {
    let json =
        r#"[{"type":"text","text":"hi"},{"type":"image","uri":"files/x","mime_type":"image/png"}]"#;
    let input: InteractionInput = serde_json::from_str(json).unwrap();
    match &input {
        InteractionInput::Content(c) => assert_eq!(c.len(), 2),
        other => panic!("Expected Content, got {other:?}"),
    }
}

#[test]
fn test_interaction_input_steps_array_roundtrip() {
    let json = r#"[
            {"type":"user_input","content":[{"type":"text","text":"hi"}]},
            {"type":"model_output","content":[{"type":"text","text":"hello"}]},
            {"type":"function_result","call_id":"c1","result":"done"}
        ]"#;
    let input: InteractionInput = serde_json::from_str(json).unwrap();
    match &input {
        InteractionInput::Steps(s) => assert_eq!(s.len(), 3),
        other => panic!("Expected Steps, got {other:?}"),
    }
}

#[test]
fn test_interaction_input_single_content_object() {
    let json = r#"{"type":"text","text":"hi"}"#;
    let input: InteractionInput = serde_json::from_str(json).unwrap();
    match &input {
        InteractionInput::Content(c) => assert_eq!(c.len(), 1),
        other => panic!("Expected Content, got {other:?}"),
    }
}

/// Serialize just the `input` field the way a request would.
fn request_input_json(input: InteractionInput) -> serde_json::Value {
    let request = InteractionRequest {
        model: Some("test-model".into()),
        input,
        ..Default::default()
    };
    serde_json::to_value(&request).unwrap()["input"].clone()
}

/// `Content` input goes out wrapped in a `user_input` step, not as a bare
/// content array — the shape the API accepts video `processing` in (#427).
#[test]
fn test_request_content_input_serializes_as_a_user_input_step() {
    let json = request_input_json(InteractionInput::Content(vec![
        Content::text("Describe briefly."),
        Content::from_uri_and_mime("files/clip", "video/mp4"),
    ]));

    assert!(json.is_array(), "input must still be an array: {json}");
    assert_eq!(json.as_array().unwrap().len(), 1, "one wrapping step");
    assert_eq!(json[0]["type"], "user_input");
    assert_eq!(
        json[0]["content"].as_array().map(Vec::len),
        Some(2),
        "both blocks are carried through unchanged: {json}"
    );
    assert_eq!(json[0]["content"][0]["type"], "text");
    assert_eq!(json[0]["content"][1]["type"], "video");
}

/// The wrap is byte-identical to building the step by hand, which is what
/// lets the round-trip land on `Steps` rather than losing information.
#[test]
fn test_request_content_input_matches_a_hand_built_user_input_step() {
    let content = vec![Content::text("hi")];
    let wrapped = request_input_json(InteractionInput::Content(content.clone()));
    let by_hand = request_input_json(InteractionInput::Steps(vec![Step::user_input(content)]));
    assert_eq!(wrapped, by_hand);
}

/// Unconditional: an empty content vector produces the same shape rather
/// than falling back to a bare `[]`, so the wire form never depends on
/// how much content the caller happened to supply.
#[test]
fn test_empty_request_content_input_is_wrapped_too() {
    assert_eq!(
        request_input_json(InteractionInput::Content(vec![])),
        serde_json::json!([{"type": "user_input", "content": []}])
    );
}

/// The other two variants are untouched by the wrap.
#[test]
fn test_wrap_does_not_touch_text_or_steps_input() {
    assert_eq!(
        request_input_json(InteractionInput::Text("hi".into())),
        serde_json::json!("hi")
    );
    let json = request_input_json(InteractionInput::Steps(vec![Step::user_input(vec![
        Content::text("hi"),
    ])]));
    assert_eq!(json.as_array().map(Vec::len), Some(1));
    assert_eq!(json[0]["type"], "user_input");
}

/// The documented round-trip, asserted end to end rather than implied by
/// chaining the serialize test with the steps-array parse test: a request
/// built from `Content` comes back as `Steps` holding one `UserInput`,
/// which is what the rustdoc and the CHANGELOG both claim.
#[test]
fn test_request_content_input_round_trips_as_a_user_input_step() {
    let request = InteractionRequest {
        model: Some("test-model".into()),
        input: InteractionInput::Content(vec![Content::text("hi")]),
        ..Default::default()
    };
    let json = serde_json::to_string(&request).unwrap();
    let back: InteractionRequest = serde_json::from_str(&json).unwrap();

    match &back.input {
        InteractionInput::Steps(steps) => {
            assert_eq!(steps.len(), 1, "one wrapping step, got {steps:?}");
            assert_eq!(
                steps[0],
                Step::user_input(vec![Content::text("hi")]),
                "the step must carry the original content unchanged"
            );
        }
        other => panic!("expected Steps after the round trip, got {other:?}"),
    }
}

/// The wrap is request-only. `InteractionInput`'s own `Serialize` stays
/// faithful to the variant, so a `Content` array echoed back on
/// `InteractionResponse::input` re-serializes in the shape the server
/// sent rather than being rewritten into a step.
#[test]
fn test_bare_input_serialization_is_unwrapped() {
    let input = InteractionInput::Content(vec![Content::text("hi")]);
    assert_eq!(
        serde_json::to_value(&input).unwrap(),
        serde_json::json!([{"type": "text", "text": "hi"}]),
        "the type's own Serialize must not wrap"
    );
}
