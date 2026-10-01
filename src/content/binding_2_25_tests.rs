use super::*;
use serde_json::json;

/// The per-content form accepted live by `gemini-3.8-flash-tts`
/// (2026-09-24): no indices, one annotation per text block.
#[test]
fn speaker_text_serializes_the_accepted_multi_speaker_shape() {
    assert_eq!(
        serde_json::to_value(Content::speaker_text("Alice", "Hello Bob!")).unwrap(),
        json!({
            "type": "text",
            "text": "Hello Bob!",
            "annotations": [{"type": "speech_metadata", "speaker": "Alice"}]
        })
    );
}

#[test]
fn speech_metadata_roundtrips_with_and_without_indices() {
    for wire in [
        json!({"type": "speech_metadata", "speaker": "Bob", "style": "whisper",
               "start_index": 11, "end_index": 37}),
        json!({"type": "speech_metadata", "style": "excited"}),
    ] {
        let annotation: Annotation = serde_json::from_value(wire.clone()).unwrap();
        assert!(matches!(annotation, Annotation::SpeechMetadata { .. }));
        assert_eq!(serde_json::to_value(&annotation).unwrap(), wire);
    }
    let spanned: Annotation = serde_json::from_value(
        json!({"type": "speech_metadata", "start_index": 0, "end_index": 5}),
    )
    .unwrap();
    assert_eq!(spanned.extract_span("Hello there"), Some("Hello"));
    assert_eq!(spanned.source(), None);
}

#[test]
fn word_info_roundtrips_the_binding_shape() {
    let wire = json!({
        "type": "word_info",
        "text": "Hello",
        "speaker": "1",
        "start_offset": "0.1s",
        "end_offset": "0.4s",
        "start_index": 0,
        "end_index": 5
    });
    let annotation: Annotation = serde_json::from_value(wire.clone()).unwrap();
    match &annotation {
        Annotation::WordInfo {
            text, start_offset, ..
        } => {
            assert_eq!(text.as_deref(), Some("Hello"));
            assert_eq!(start_offset.as_deref(), Some("0.1s"));
        }
        other => panic!("expected WordInfo, got {other:?}"),
    }
    assert_eq!(annotation.end_index(), Some(5));
    assert_eq!(serde_json::to_value(&annotation).unwrap(), wire);
}

#[test]
fn video_name_roundtrips() {
    let wire =
        json!({"type": "video", "uri": "files/abc", "mime_type": "video/mp4", "name": "clip.mp4"});
    let content: Content = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(&content, Content::Video { name: Some(n), .. } if n == "clip.mp4"));
    assert_eq!(serde_json::to_value(&content).unwrap(), wire);
    // Builders preserve it.
    let content = content.with_resolution(Resolution::Low);
    assert!(matches!(&content, Content::Video { name: Some(_), .. }));
    let named = Content::video_uri("files/x", "video/mp4").with_video_name("n");
    assert!(matches!(&named, Content::Video { name: Some(n), .. } if n == "n"));
    let text = Content::text("hi").with_video_name("n");
    assert_eq!(text.as_text(), Some("hi"));
}
