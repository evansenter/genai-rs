//! Unit tests for `Content`: serialization, constructors, media resolution,
//! MIME routing, and Unknown variant handling (API revision 2026-05-20).
//!
//! Tool calls, tool results, and thoughts are no longer `Content` variants;
//! they are typed `Step` variants, and their wire-format tests live in
//! `src/steps/step_serde_tests.rs`. Annotations are tested in
//! `annotation_tests.rs`, video processing in `video_processing_tests.rs`.
//!
//! Note: the launch-era computer-use content tests were removed entirely —
//! computer use has no Step equivalent (it flows through `function_call`
//! steps).

use crate::*;

// --- Basic Content Serialization/Deserialization ---

#[test]
fn test_serialize_interaction_content() {
    let content = Content::Text {
        text: Some("Hello".to_string()),
        annotations: None,
        extra: Default::default(),
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
        extra: Default::default(),
    };
    assert_eq!(content.as_text(), None);

    let content_none = Content::Text {
        text: None,
        annotations: None,
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        extra: Default::default(),
    };
    let json = serde_json::to_string(&audio).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "audio");
    assert!(value.get("sample_rate").is_none());
    assert!(value.get("channels").is_none());
}

// --- Unknown Variant Tests ---
// Note: Tests that rely on graceful unknown handling are disabled when strict mode is enabled,
// since strict mode causes deserialization errors for unknown types instead of capturing them.

#[cfg(not(genai_strict_unknown))]
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
        _ => panic!("Expected Unknown variant, got {content:?}"),
    }

    assert!(content.is_unknown());
    assert_eq!(content.unknown_content_type(), Some("future_api_feature"));
    assert!(content.unknown_data().is_some());
}

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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
// CodeExecutionLanguage Tests
// =============================================================================

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_code_execution_language_unknown_display() {
    let unknown = CodeExecutionLanguage::Unknown {
        language_type: "RUST".to_string(),
        data: serde_json::Value::String("RUST".to_string()),
    };
    assert_eq!(format!("{unknown}"), "RUST");
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

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_code_execution_language_uppercase_is_unknown() {
    // The pre-revision spelling is not the wire format any more.
    let deserialized: CodeExecutionLanguage =
        serde_json::from_str(r#""PYTHON""#).expect("Should deserialize");
    assert_eq!(deserialized.unknown_language_type(), Some("PYTHON"));
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
        extra: Default::default(),
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
        extra: Default::default(),
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
        processing: None,
        name: None,
        extra: Default::default(),
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
            ..
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
            processing,
            ..
        } => {
            assert_eq!(data, None);
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::UltraHigh));
            assert_eq!(processing, None);
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
        extra: Default::default(),
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
            ..
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
        processing: None,
        name: None,
        extra: Default::default(),
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: Content = serde_json::from_str(&json).unwrap();

    match restored {
        Content::Video {
            data,
            uri,
            mime_type,
            resolution,
            processing,
            ..
        } => {
            assert_eq!(data, None);
            assert_eq!(uri, Some("gs://bucket/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert_eq!(resolution, Some(Resolution::High));
            assert_eq!(processing, None);
        }
        _ => panic!("Expected Video variant"),
    }
}

// --- Resolution Unknown Tests ---

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_resolution_unknown_deserialization() {
    // Test that unrecognized resolution strings deserialize to Unknown
    let json = r#""super_high""#;
    let resolution: Resolution = serde_json::from_str(json).unwrap();

    assert!(resolution.is_unknown());
    assert_eq!(resolution.unknown_resolution_type(), Some("super_high"));
}

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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

// =============================================================================
// Content Constructor Tests
// =============================================================================

#[test]
fn test_new_text_creates_correct_variant() {
    let content = Content::text("Hello world");
    match &content {
        Content::Text {
            text, annotations, ..
        } => {
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
            ..
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
        Content::image_data("base64encodeddata", "image/png").with_resolution(Resolution::High);
    match content {
        Content::Image {
            data,
            uri,
            mime_type,
            resolution,
            ..
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
            ..
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
    let content = Content::image_uri("https://example.com/image.png", "image/png")
        .with_resolution(Resolution::Low);
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
            ..
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
            ..
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
            processing,
            ..
        } => {
            assert_eq!(data, Some("base64videodata".to_string()));
            assert!(uri.is_none());
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert!(resolution.is_none());
            assert!(processing.is_none());
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_data_with_resolution_creates_correct_variant() {
    let content =
        Content::video_data("base64videodata", "video/mp4").with_resolution(Resolution::Low);
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
            processing,
            ..
        } => {
            assert!(data.is_none());
            assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
            assert_eq!(mime_type, Some("video/mp4".to_string()));
            assert!(resolution.is_none());
            assert!(processing.is_none());
        }
        _ => panic!("Expected Video variant"),
    }
}

#[test]
fn test_new_video_uri_with_resolution_creates_correct_variant() {
    let content = Content::video_uri("https://example.com/video.mp4", "video/mp4")
        .with_resolution(Resolution::Medium);
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
            ..
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
            ..
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

    let image = Content::image_data("data", "image/png").with_resolution(Resolution::High);
    let json = serde_json::to_string(&image).expect("Should serialize");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["type"], "image");
    assert_eq!(value["data"], "data");
    assert_eq!(value["resolution"], "high");
}

// =============================================================================
// GoogleSearchResultItem Constructor Tests
// =============================================================================

#[test]
fn test_google_search_result_item_new() {
    let item = GoogleSearchResultItem::new("<div/>");
    assert_eq!(item.search_suggestions.as_deref(), Some("<div/>"));
    assert!(item.extra.is_empty());
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
            ..
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

#[test]
fn test_google_search_result_item_suggestions_only_roundtrip() {
    // Live wire shape (verified 2026-07 and 2026-11-05): result items carry
    // only `search_suggestions` (an HTML rendering payload).
    let wire = serde_json::json!({"search_suggestions": "<style>.container{}</style>"});

    let item: GoogleSearchResultItem =
        serde_json::from_value(wire.clone()).expect("Deserialization failed");
    assert_eq!(
        item.search_suggestions.as_deref(),
        Some("<style>.container{}</style>")
    );

    let roundtripped = serde_json::to_value(&item).expect("Serialization failed");
    assert_eq!(roundtripped, wire);
}

#[test]
fn test_google_search_result_item_keeps_a_removed_field() {
    // `title`, `url` and `rendered_content` were modeled from an older
    // spec and never served; one that arrives is kept in `extra`.
    let wire = serde_json::json!({"title": "wikipedia.org", "url": "https://example.com/r"});
    let item: GoogleSearchResultItem = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(item.extra["title"], "wikipedia.org");
    assert_eq!(serde_json::to_value(&item).unwrap(), wire);
}
