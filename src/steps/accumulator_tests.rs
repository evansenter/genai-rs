use super::*;

use serde_json::json;

// =========================================================================
// StepAccumulator
// =========================================================================

#[test]
fn test_accumulator_text_stream() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &StepDelta::Text {
            text: "Hello ".into(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Text {
            text: "world".into(),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].as_text(), Some("Hello world"));
}

#[test]
fn test_accumulator_image_deltas_accumulate_into_one_block() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &StepDelta::Image {
            data: Some("AAAA".into()),
            uri: None,
            mime_type: Some("image/png".into()),
            resolution: None,
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Image {
            data: Some("BBBB".into()),
            uri: None,
            mime_type: Some("image/png".into()),
            resolution: None,
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    match &steps[0] {
        Step::ModelOutput { content, .. } => {
            assert_eq!(content.len(), 1);
            match &content[0] {
                Content::Image { data, .. } => assert_eq!(data.as_deref(), Some("AAAABBBB")),
                other => panic!("Expected Image, got {other:?}"),
            }
        }
        other => panic!("Expected ModelOutput, got {other:?}"),
    }
}

#[test]
fn test_accumulator_video_and_document_deltas_accumulate() {
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &StepDelta::Video {
            data: Some("VV".into()),
            uri: None,
            mime_type: Some("video/mp4".into()),
            resolution: None,
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Video {
            data: Some("WW".into()),
            uri: None,
            mime_type: Some("video/mp4".into()),
            resolution: None,
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Document {
            data: Some("DD".into()),
            uri: None,
            mime_type: Some("application/pdf".into()),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Document {
            data: Some("EE".into()),
            uri: None,
            mime_type: Some("application/pdf".into()),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    match &steps[0] {
        Step::ModelOutput { content, .. } => {
            assert_eq!(content.len(), 2);
            match &content[0] {
                Content::Video { data, .. } => assert_eq!(data.as_deref(), Some("VVWW")),
                other => panic!("Expected Video, got {other:?}"),
            }
            match &content[1] {
                Content::Document { data, .. } => assert_eq!(data.as_deref(), Some("DDEE")),
                other => panic!("Expected Document, got {other:?}"),
            }
        }
        other => panic!("Expected ModelOutput, got {other:?}"),
    }
}

#[test]
fn test_accumulator_image_uri_delta_pushes_new_block() {
    // Deltas without inline data (e.g. URI references) must not merge
    // into the previous block.
    let mut acc = StepAccumulator::new();
    acc.start(0, Step::model_output(vec![]));
    acc.apply_delta(
        0,
        &StepDelta::Image {
            data: Some("AAAA".into()),
            uri: None,
            mime_type: Some("image/png".into()),
            resolution: None,
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::Image {
            data: None,
            uri: Some("https://example.com/img.png".into()),
            mime_type: Some("image/png".into()),
            resolution: None,
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    match &steps[0] {
        Step::ModelOutput { content, .. } => assert_eq!(content.len(), 2),
        other => panic!("Expected ModelOutput, got {other:?}"),
    }
}

#[test]
fn test_accumulator_function_call_arguments_delta() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        Step::FunctionCall {
            id: "c1".into(),
            name: "get_weather".into(),
            arguments: serde_json::Value::Null,
            signature: None,
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ArgumentsDelta {
            arguments: "{\"city\": ".into(),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ArgumentsDelta {
            arguments: "\"Tokyo\"}".into(),
        },
    );
    acc.stop(0);
    let steps = acc.finish();
    match &steps[0] {
        Step::FunctionCall { arguments, .. } => assert_eq!(arguments["city"], "Tokyo"),
        other => panic!("Expected FunctionCall, got {other:?}"),
    }
}

#[test]
fn test_accumulator_thought_summary_and_signature() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        Step::Thought {
            signature: None,
            summary: vec![],
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ThoughtSummary {
            content: Some(Content::text("Consider ")),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ThoughtSummary {
            content: Some(Content::text("the problem")),
        },
    );
    acc.apply_delta(
        0,
        &StepDelta::ThoughtSignature {
            signature: Some("sig-xyz".into()),
        },
    );
    let steps = acc.finish();
    match &steps[0] {
        Step::Thought { signature, summary } => {
            assert_eq!(signature.as_deref(), Some("sig-xyz"));
            assert_eq!(summary[0].as_text(), Some("Consider the problem"));
        }
        other => panic!("Expected Thought, got {other:?}"),
    }
}

#[test]
fn test_accumulator_delta_without_start_creates_model_output() {
    let mut acc = StepAccumulator::new();
    acc.apply_delta(
        0,
        &StepDelta::Text {
            text: "orphan".into(),
        },
    );
    let steps = acc.finish();
    assert_eq!(steps[0].as_text(), Some("orphan"));
}

#[test]
fn test_accumulator_orders_steps_by_index() {
    let mut acc = StepAccumulator::new();
    acc.start(2, Step::model_text("second"));
    acc.start(1, Step::thought("sig"));
    let steps = acc.finish();
    assert!(matches!(steps[0], Step::Thought { .. }));
    assert!(matches!(steps[1], Step::ModelOutput { .. }));
}

// =========================================================================
// processing_* / retrieval_* steps (google-genai 2.21+ / 2.24+ bindings)
// =========================================================================

/// The streamed `processing_call` announces `signature: ""` on
/// `step.start` and sends the value in `step.delta` (live 2026-09-24).
/// Dropping it made stateless replay fail with
/// `400 Processing call step is missing signature`.
#[test]
fn accumulator_moves_streamed_processing_signature_onto_the_step() {
    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(json!({"type": "processing_call", "id": "c1", "signature": ""}))
            .unwrap(),
    );
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "processing_call", "signature": "SIG"})).unwrap(),
    );
    acc.stop(0);
    acc.start(
        1,
        serde_json::from_value(
            json!({"type": "processing_result", "call_id": "c1", "signature": ""}),
        )
        .unwrap(),
    );
    acc.apply_delta(
        1,
        &serde_json::from_value(json!({"type": "processing_result", "signature": "RES"})).unwrap(),
    );
    let steps = acc.finish();
    assert_eq!(steps[0].signature(), Some("SIG"));
    assert_eq!(steps[1].signature(), Some("RES"));
}

#[test]
fn merge_signature_replaces_empty_and_extends_fragments() {
    let mut sig = Some(String::new());
    merge_signature(&mut sig, Some("ab"));
    assert_eq!(sig.as_deref(), Some("ab"));
    merge_signature(&mut sig, Some("cd"));
    assert_eq!(sig.as_deref(), Some("abcd"));
    merge_signature(&mut sig, Some(""));
    merge_signature(&mut sig, None);
    assert_eq!(sig.as_deref(), Some("abcd"));
    let mut none = None;
    merge_signature(&mut none, Some("x"));
    assert_eq!(none.as_deref(), Some("x"));
}

/// A step type the crate does not model yet must not lose a streamed
/// signature either — the same failure, one API release later.
#[test]
fn accumulator_merges_unknown_delta_signature_into_unknown_step() {
    let mut acc = StepAccumulator::new();
    // Built directly: `strict-unknown` rejects unknown steps on deserialize.
    acc.start(
        0,
        Step::Unknown {
            step_type: "future_call".into(),
            data: json!({"type": "future_call", "id": "f1", "signature": ""}),
        },
    );
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "future_call", "signature": "FUT"})).unwrap(),
    );
    // A differently-typed unknown delta is not merged.
    acc.apply_delta(
        0,
        &serde_json::from_value(json!({"type": "other_delta", "signature": "NO"})).unwrap(),
    );
    let steps = acc.finish();
    assert_eq!(
        steps[0].unknown_data().unwrap(),
        &json!({"type": "future_call", "id": "f1", "signature": "FUT"})
    );
    assert_eq!(
        serde_json::to_value(&steps[0]).unwrap(),
        json!({"type": "future_call", "id": "f1", "signature": "FUT"})
    );
}

/// `FunctionResultDelta.call_id` is gone from the 2.25 bindings.
#[test]
fn function_result_delta_without_call_id_stays_typed() {
    let delta: StepDelta =
        serde_json::from_value(json!({"type": "function_result", "result": "ok"})).unwrap();
    assert!(matches!(
        delta,
        StepDelta::FunctionResult { call_id: None, .. }
    ));

    let mut acc = StepAccumulator::new();
    acc.start(
        0,
        serde_json::from_value(
            json!({"type": "function_result", "call_id": "c9", "signature": "s"}),
        )
        .unwrap(),
    );
    acc.apply_delta(0, &delta);
    match &acc.finish()[0] {
        Step::FunctionResult {
            call_id, signature, ..
        } => {
            assert_eq!(call_id, "c9");
            assert_eq!(signature.as_deref(), Some("s"));
        }
        other => panic!("expected FunctionResult, got {other:?}"),
    }
}
