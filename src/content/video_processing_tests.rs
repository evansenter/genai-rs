//! `VideoProcessing`, the `processing` field on video content.

use crate::*;

// --- VideoProcessing (wire: `processing` on video content) ---
//
// Wire forms verified live 2026-08-16 against gemini-3.7-flash
// (Api-Revision 2026-05-20). See docs/ENUM_WIRE_FORMATS.md.

#[test]
fn test_video_processing_bare_modes_serialize() {
    assert_eq!(
        serde_json::to_string(&VideoProcessing::Static).unwrap(),
        "\"static\""
    );
    assert_eq!(
        serde_json::to_string(&VideoProcessing::Agentic).unwrap(),
        "\"agentic\""
    );
}

#[test]
fn test_video_processing_bare_modes_deserialize() {
    assert_eq!(
        serde_json::from_str::<VideoProcessing>("\"static\"").unwrap(),
        VideoProcessing::Static
    );
    assert_eq!(
        serde_json::from_str::<VideoProcessing>("\"agentic\"").unwrap(),
        VideoProcessing::Agentic
    );
}

#[test]
fn test_video_processing_segment_serializes_to_object() {
    // The exact payload accepted live (16,198 video input tokens as of
    // 2026-08-18; 455 when first measured 2026-08-16 — see `VideoProcessing`).
    let processing = VideoProcessing::segment()
        .with_start_offset("5s")
        .with_end_offset("10s")
        .with_fps(1.0)
        .build();

    let json = serde_json::to_value(&processing).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "type": "static",
            "start_offset": "5s",
            "end_offset": "10s",
            "fps": 1.0
        })
    );
}

#[test]
fn test_video_processing_segment_omits_unset_fields() {
    let processing = VideoProcessing::segment().with_fps(2.5).build();
    let json = serde_json::to_value(&processing).unwrap();

    assert_eq!(json, serde_json::json!({"type": "static", "fps": 2.5}));
    assert!(json.get("start_offset").is_none());
    assert!(json.get("end_offset").is_none());
}

#[test]
fn test_video_processing_segment_roundtrip() {
    let wire = serde_json::json!({
        "type": "static",
        "start_offset": "10.5s",
        "end_offset": "30s",
        "fps": 1.0
    });

    let processing: VideoProcessing = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        processing,
        VideoProcessing::StaticSegment {
            start_offset: Some("10.5s".to_string()),
            end_offset: Some("30s".to_string()),
            fps: Some(1.0),
            extra: Default::default(),
        }
    );

    assert_eq!(serde_json::to_value(&processing).unwrap(), wire);
}

#[test]
fn test_video_processing_bare_and_object_static_stay_distinct() {
    // A bare "static" must not normalize into an empty object, and an object
    // must not collapse into a bare string — each round-trips to its own form.
    let bare: VideoProcessing = serde_json::from_str("\"static\"").unwrap();
    let object: VideoProcessing =
        serde_json::from_value(serde_json::json!({"type": "static"})).unwrap();

    assert_eq!(bare, VideoProcessing::Static);
    assert_eq!(
        object,
        VideoProcessing::StaticSegment {
            start_offset: None,
            end_offset: None,
            fps: None,
            extra: Default::default(),
        }
    );

    assert_eq!(
        serde_json::to_value(&bare).unwrap(),
        serde_json::json!("static")
    );
    assert_eq!(
        serde_json::to_value(&object).unwrap(),
        serde_json::json!({"type": "static"})
    );
}

#[test]
fn test_video_processing_unknown_string_preserved() {
    let processing: VideoProcessing = serde_json::from_str("\"quantum\"").unwrap();

    assert!(processing.is_unknown());
    assert_eq!(processing.unknown_processing_type(), Some("quantum"));
    assert_eq!(
        processing.unknown_data(),
        Some(&serde_json::json!("quantum"))
    );
    // Roundtrips back to the original wire value.
    assert_eq!(
        serde_json::to_value(&processing).unwrap(),
        serde_json::json!("quantum")
    );
}

#[test]
fn test_video_processing_unknown_object_preserved() {
    let wire = serde_json::json!({"type": "adaptive", "budget": 42});
    let processing: VideoProcessing = serde_json::from_value(wire.clone()).unwrap();

    assert!(processing.is_unknown());
    assert_eq!(processing.unknown_processing_type(), Some("adaptive"));
    // Full JSON preserved for roundtrip, including the unmodeled `budget`.
    assert_eq!(serde_json::to_value(&processing).unwrap(), wire);
}

#[test]
fn test_video_processing_known_variants_have_no_unknown_data() {
    assert!(!VideoProcessing::Static.is_unknown());
    assert!(VideoProcessing::Agentic.unknown_processing_type().is_none());
    assert!(
        VideoProcessing::segment()
            .with_fps(1.0)
            .build()
            .unknown_data()
            .is_none()
    );
}

#[test]
fn test_video_content_with_processing_serializes() {
    let video =
        Content::video_uri("files/abc123", "video/mp4").with_processing(VideoProcessing::Agentic);

    let json = serde_json::to_value(&video).unwrap();
    assert_eq!(json["type"], "video");
    assert_eq!(json["uri"], "files/abc123");
    assert_eq!(json["processing"], "agentic");
}

#[test]
fn test_video_content_processing_absent_when_unset() {
    let video = Content::video_uri("files/abc123", "video/mp4");
    let json = serde_json::to_value(&video).unwrap();

    assert!(
        json.get("processing").is_none(),
        "processing must be omitted when unset, not sent as null"
    );
}

#[test]
fn test_video_content_processing_roundtrip() {
    let wire = serde_json::json!({
        "type": "video",
        "uri": "https://example.com/v.mp4",
        "mime_type": "video/mp4",
        "processing": {"type": "static", "start_offset": "5s", "fps": 1.0}
    });

    let content: Content = serde_json::from_value(wire.clone()).unwrap();
    match &content {
        Content::Video { processing, .. } => assert_eq!(
            processing.as_ref().unwrap(),
            &VideoProcessing::StaticSegment {
                start_offset: Some("5s".to_string()),
                end_offset: None,
                fps: Some(1.0),
                extra: Default::default(),
            }
        ),
        other => panic!("expected video content, got {:?}", other),
    }

    assert_eq!(serde_json::to_value(&content).unwrap(), wire);
}

#[test]
fn test_video_content_with_processing_preserves_resolution() {
    // with_processing must not clobber a previously set resolution.
    let video = Content::video_uri("files/x", "video/mp4")
        .with_resolution(Resolution::High)
        .with_processing(VideoProcessing::Static);

    match &video {
        Content::Video {
            resolution,
            processing,
            ..
        } => {
            assert_eq!(resolution.as_ref(), Some(&Resolution::High));
            assert_eq!(processing.as_ref(), Some(&VideoProcessing::Static));
        }
        other => panic!("expected video content, got {:?}", other),
    }
}

#[test]
fn test_video_content_with_resolution_preserves_processing() {
    // The reverse: with_resolution must not drop processing.
    let video = Content::video_uri("files/x", "video/mp4")
        .with_processing(VideoProcessing::Agentic)
        .with_resolution(Resolution::Low);

    match &video {
        Content::Video {
            resolution,
            processing,
            ..
        } => {
            assert_eq!(resolution.as_ref(), Some(&Resolution::Low));
            assert_eq!(processing.as_ref(), Some(&VideoProcessing::Agentic));
        }
        other => panic!("expected video content, got {:?}", other),
    }
}

#[test]
fn test_with_processing_on_non_video_is_a_no_op() {
    let text = Content::text("hello");
    let unchanged = text.clone().with_processing(VideoProcessing::Static);
    assert_eq!(text, unchanged);
}
