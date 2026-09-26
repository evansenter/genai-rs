use super::*;

use serde_json::json;

// =========================================================================
// StepDelta wire fixtures
// =========================================================================

#[test]
fn test_step_delta_text() {
    let delta: StepDelta = serde_json::from_str(r#"{"type":"text","text":"Hel"}"#).unwrap();
    assert_eq!(delta.as_text(), Some("Hel"));
    let out = serde_json::to_value(&delta).unwrap();
    assert_eq!(out, json!({"type": "text", "text": "Hel"}));
}

#[test]
fn test_step_delta_arguments_delta() {
    let delta: StepDelta =
        serde_json::from_str(r#"{"type":"arguments_delta","arguments":"{\"city\": \"To"}"#)
            .unwrap();
    assert_eq!(delta.as_arguments_delta(), Some("{\"city\": \"To"));
}

#[test]
fn test_step_delta_audio_with_rate_and_channels() {
    let delta: StepDelta = serde_json::from_str(
            r#"{"type":"audio","data":"QUJD","mime_type":"audio/l16","rate":24000,"sample_rate":24000,"channels":1}"#,
        )
        .unwrap();
    match &delta {
        StepDelta::Audio {
            sample_rate,
            channels,
            rate,
            ..
        } => {
            assert_eq!(*sample_rate, Some(24000));
            assert_eq!(*rate, Some(24000));
            assert_eq!(*channels, Some(1));
        }
        other => panic!("Expected Audio, got {other:?}"),
    }
}

#[test]
fn test_step_delta_thought_summary_and_signature() {
    let summary: StepDelta = serde_json::from_str(
        r#"{"type":"thought_summary","content":{"type":"text","text":"Analyzing"}}"#,
    )
    .unwrap();
    assert!(matches!(summary, StepDelta::ThoughtSummary { .. }));

    let sig: StepDelta =
        serde_json::from_str(r#"{"type":"thought_signature","signature":"abc123"}"#).unwrap();
    assert!(matches!(
        sig,
        StepDelta::ThoughtSignature { signature: Some(s) } if s == "abc123"
    ));
}

#[test]
fn test_step_delta_text_annotation() {
    let delta: StepDelta = serde_json::from_str(
            r#"{"type":"text_annotation_delta","annotations":[
                {"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":5}
            ]}"#,
        )
        .unwrap();
    match &delta {
        StepDelta::TextAnnotation { annotations } => {
            assert_eq!(annotations.len(), 1);
            assert!(matches!(annotations[0], Annotation::UrlCitation { .. }));
        }
        other => panic!("Expected TextAnnotation, got {other:?}"),
    }
}

#[test]
fn test_step_delta_unknown_roundtrip() {
    let delta: StepDelta = serde_json::from_str(r#"{"type":"hologram","frames":3}"#).unwrap();
    assert!(delta.is_unknown());
    assert_eq!(delta.unknown_delta_type(), Some("hologram"));
    assert_eq!(delta.unknown_data().unwrap()["frames"], 3);
    let out = serde_json::to_value(&delta).unwrap();
    assert_eq!(out["type"], "hologram");
    assert_eq!(out["frames"], 3);
}

// =========================================================================
// processing_* / retrieval_* steps (google-genai 2.21+ / 2.24+ bindings)
// =========================================================================

#[test]
fn processing_and_retrieval_deltas_deserialize() {
    for (wire, expect) in [
        (
            json!({"type": "processing_call", "signature": "a"}),
            "processing_call",
        ),
        (
            json!({"type": "processing_result", "signature": "b"}),
            "processing_result",
        ),
        (
            json!({"type": "retrieval_call", "arguments": {"queries": ["q"]}}),
            "retrieval_call",
        ),
        (
            json!({"type": "retrieval_result", "is_error": true}),
            "retrieval_result",
        ),
    ] {
        let delta: StepDelta = serde_json::from_value(wire.clone()).unwrap();
        assert!(!delta.is_unknown(), "{expect} fell through to Unknown");
        assert_eq!(serde_json::to_value(&delta).unwrap(), wire);
    }
}
