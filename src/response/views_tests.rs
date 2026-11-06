//! Unit tests for the view types (`ImageInfo`, `AudioInfo`,
//! `OwnedFunctionCallInfo`) and the response accessors that return them.

use super::*;
use crate::{Content, InteractionResponse, InteractionStatus, Step};

/// A completed response with one model text step.
fn text_response(text: &str) -> InteractionResponse {
    InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text(text)],
        ..Default::default()
    }
}

#[test]
fn test_image_info_extension() {
    let check = |mime: Option<&str>, expected: &str| {
        let info = ImageInfo {
            data: "",
            mime_type: mime,
        };
        assert_eq!(info.extension(), expected);
    };

    check(Some("image/jpeg"), "jpg");
    check(Some("image/jpg"), "jpg");
    check(Some("image/png"), "png");
    check(Some("image/webp"), "webp");
    check(Some("image/gif"), "gif");
    check(Some("image/unknown"), "png"); // default
    check(None, "png"); // default
}

// =========================================================================
// AudioInfo Tests
// =========================================================================

#[test]
fn test_audio_info_extension() {
    let check = |mime: Option<&str>, expected: &str| {
        let info = AudioInfo {
            data: "",
            mime_type: mime,
            sample_rate: None,
            channels: None,
        };
        assert_eq!(info.extension(), expected);
    };

    check(Some("audio/wav"), "wav");
    check(Some("audio/x-wav"), "wav");
    check(Some("audio/mp3"), "mp3");
    check(Some("audio/mpeg"), "mp3");
    check(Some("audio/ogg"), "ogg");
    check(Some("audio/flac"), "flac");
    check(Some("audio/aac"), "aac");
    check(Some("audio/webm"), "webm");
    // PCM/L16 format from TTS API
    check(Some("audio/L16;codec=pcm;rate=24000"), "pcm");
    check(Some("audio/l16"), "pcm");
    check(Some("audio/unknown"), "wav"); // default
    check(None, "wav"); // default
}

// --- OwnedFunctionCallInfo Tests ---

#[test]
fn test_function_call_info_to_owned() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::function_call(
            "call_123",
            "get_weather",
            serde_json::json!({"city": "Tokyo", "units": "celsius"}),
        )],
        ..Default::default()
    };

    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);

    // Convert to owned
    let owned = calls[0].to_owned();

    // Verify all fields are correctly converted (id is an owned String now)
    assert_eq!(owned.id, "call_123");
    assert_eq!(owned.name, "get_weather");
    assert_eq!(owned.args["city"], "Tokyo");
    assert_eq!(owned.args["units"], "celsius");
}

#[test]
fn test_owned_function_call_info_outlives_response() {
    // Demonstrate the main use case: owned call can outlive the response
    let owned_calls: Vec<OwnedFunctionCallInfo> = {
        let response = InteractionResponse {
            id: Some("test_id".to_string()),
            model: Some("test-model".to_string()),
            status: InteractionStatus::RequiresAction,
            steps: vec![
                Step::function_call("call_1", "func_a", serde_json::json!({"x": 1})),
                Step::function_call("call_2", "func_b", serde_json::json!({"y": 2})),
            ],
            ..Default::default()
        };

        // Convert to owned before response goes out of scope
        response
            .function_calls()
            .into_iter()
            .map(|call| call.to_owned())
            .collect()
    }; // response is dropped here

    // owned_calls is still valid and usable
    assert_eq!(owned_calls.len(), 2);
    assert_eq!(owned_calls[0].id, "call_1");
    assert_eq!(owned_calls[0].name, "func_a");
    assert_eq!(owned_calls[0].args["x"], 1);
    assert_eq!(owned_calls[1].name, "func_b");
    assert_eq!(owned_calls[1].args["y"], 2);
}

#[test]
fn test_owned_function_call_info_serialization_roundtrip() {
    let owned = OwnedFunctionCallInfo {
        id: "call_xyz".to_string(),
        name: "my_function".to_string(),
        args: serde_json::json!({"key": "value", "number": 42}),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&owned).expect("Serialization should succeed");

    // Verify JSON contains expected data
    assert!(json.contains("call_xyz"));
    assert!(json.contains("my_function"));

    // Deserialize back
    let restored: OwnedFunctionCallInfo =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(restored.id, owned.id);
    assert_eq!(restored.name, owned.name);
    assert_eq!(restored.args, owned.args);
}

#[test]
fn test_owned_function_call_info_clone() {
    let owned = OwnedFunctionCallInfo {
        id: "call_id".to_string(),
        name: "cloneable".to_string(),
        args: serde_json::json!({"data": [1, 2, 3]}),
    };

    let cloned = owned.clone();

    assert_eq!(cloned.id, owned.id);
    assert_eq!(cloned.name, owned.name);
    assert_eq!(cloned.args, owned.args);
}

#[test]
fn test_owned_function_call_info_equality() {
    let owned1 = OwnedFunctionCallInfo {
        id: "same_id".to_string(),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    let owned2 = OwnedFunctionCallInfo {
        id: "same_id".to_string(),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    let different = OwnedFunctionCallInfo {
        id: "different_id".to_string(),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    assert_eq!(owned1, owned2);
    assert_ne!(owned1, different);
}

// =============================================================================
// ImageInfo and images() Tests
// =============================================================================

/// Builds a completed response with the given content in one model_output step.
fn model_output_response(content: Vec<Content>) -> InteractionResponse {
    InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(content)],
        ..Default::default()
    }
}

#[test]
fn test_image_info_bytes_decodes_valid_base64() {
    use base64::Engine as _;
    // Valid PNG header in base64
    let png_bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let base64_data = base64::engine::general_purpose::STANDARD.encode(png_bytes);

    let response = model_output_response(vec![Content::image_data(base64_data, "image/png")]);

    let images: Vec<_> = response.images();
    assert_eq!(images.len(), 1);

    let decoded = images[0].bytes().expect("Should decode valid base64");
    assert_eq!(decoded, png_bytes);
}

#[test]
fn test_image_info_bytes_invalid_base64() {
    let response = model_output_response(vec![Content::image_data(
        "not valid base64!!!",
        "image/png",
    )]);

    let images: Vec<_> = response.images();
    assert_eq!(images.len(), 1);

    let result = images[0].bytes();
    assert!(result.is_err());
}

#[test]
fn test_image_info_mime_type() {
    let response = model_output_response(vec![
        Content::image_data("YWJj", "image/jpeg"), // "abc" in base64
        Content::Image {
            data: Some("ZGVm".to_string()), // "def" in base64
            uri: None,
            mime_type: None,
            resolution: None,
            extra: Default::default(),
        },
    ]);

    let images: Vec<_> = response.images();
    assert_eq!(images.len(), 2);

    assert_eq!(images[0].mime_type(), Some("image/jpeg"));
    assert_eq!(images[1].mime_type(), None);
}

#[test]
fn test_image_info_extension_jpeg() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/jpeg")]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "jpg");
}

#[test]
fn test_image_info_extension_jpg_alternate() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/jpg")]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "jpg");
}

#[test]
fn test_image_info_extension_png() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/png")]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "png");
}

#[test]
fn test_image_info_extension_webp() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/webp")]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "webp");
}

#[test]
fn test_image_info_extension_gif() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/gif")]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "gif");
}

#[test]
fn test_image_info_extension_unknown_defaults_to_png() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/tiff")]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "png"); // Defaults to png for unknown types
}

#[test]
fn test_image_info_extension_none_mime_type_defaults_to_png() {
    let response = model_output_response(vec![Content::Image {
        data: Some("YWJj".to_string()),
        uri: None,
        mime_type: None,
        resolution: None,
        extra: Default::default(),
    }]);
    let images: Vec<_> = response.images();
    assert_eq!(images[0].extension(), "png"); // Defaults to png when no MIME type
}

#[test]
fn test_images_iterator_returns_only_images_with_data() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![
                Content::text("Text content"),
                Content::image_data("YWJj", "image/png"), // Has data - included
                Content::image_uri("https://example.com/image.png", "image/png"), // URI-based - excluded
                Content::image_data("ZGVm", "image/jpeg"), // Has data - included
            ]),
            Step::function_call("call_x", "test", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let images: Vec<_> = response.images();
    // Only 2 images with data should be included, not the URI-based one
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].mime_type(), Some("image/png"));
    assert_eq!(images[1].mime_type(), Some("image/jpeg"));
}

#[test]
fn test_images_iterator_empty_when_no_images() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text"), Step::thought("sig_thought")],
        ..Default::default()
    };

    let images: Vec<_> = response.images();
    assert!(images.is_empty());
}

#[test]
fn test_images_iterator_empty_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    let images: Vec<_> = response.images();
    assert!(images.is_empty());
}

// ============================================================================
// AudioInfo Tests
// ============================================================================

#[test]
fn test_audios_iterator_returns_only_audios_with_data() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some(crate::DEFAULT_TTS_MODEL.to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![
                Content::text("Text content"),
                Content::audio_data("YWJj", "audio/L16;codec=pcm;rate=24000"), // Has data - included
                Content::audio_uri("https://example.com/audio.mp3", "audio/mp3"), // URI-based - excluded
                Content::audio_data("ZGVm", "audio/wav"), // Has data - included
            ]),
            Step::function_call("call_y", "test", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let audios: Vec<_> = response.audios();
    // Only 2 audios with data should be included, not the URI-based one
    assert_eq!(audios.len(), 2);
    assert_eq!(
        audios[0].mime_type(),
        Some("audio/L16;codec=pcm;rate=24000")
    );
    assert_eq!(audios[1].mime_type(), Some("audio/wav"));
}

#[test]
fn test_audios_iterator_empty_when_no_audios() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text"), Step::thought("sig_thought")],
        ..Default::default()
    };

    let audios: Vec<_> = response.audios();
    assert!(audios.is_empty());
}

#[test]
fn test_audios_iterator_empty_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some(crate::DEFAULT_TTS_MODEL.to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    let audios: Vec<_> = response.audios();
    assert!(audios.is_empty());
}

#[test]
fn test_first_audio() {
    let response = model_output_response(vec![
        Content::text("Hello"),
        Content::audio_data("Zmlyc3Q=", "audio/L16;codec=pcm;rate=24000"),
        Content::audio_data("c2Vjb25k", "audio/wav"),
    ]);

    let first = response.first_audio();
    assert!(first.is_some());
    let audio = first.unwrap();
    assert_eq!(audio.mime_type(), Some("audio/L16;codec=pcm;rate=24000"));
    assert_eq!(audio.extension(), "pcm");
}

#[test]
fn test_audio_info_sample_rate_and_channels() {
    // The new Audio fields flow through to AudioInfo accessors
    let response = model_output_response(vec![Content::Audio {
        data: Some("YXVkaW8=".to_string()),
        uri: None,
        mime_type: Some("audio/wav".to_string()),
        sample_rate: Some(24000),
        channels: Some(1),
        extra: Default::default(),
    }]);

    let audio = response.first_audio().expect("Should have audio");
    assert_eq!(audio.sample_rate(), Some(24000));
    assert_eq!(audio.channels(), Some(1));

    // Absent fields report None
    let response = model_output_response(vec![Content::audio_data("YXVkaW8=", "audio/wav")]);
    let audio = response.first_audio().expect("Should have audio");
    assert_eq!(audio.sample_rate(), None);
    assert_eq!(audio.channels(), None);
}

#[test]
fn test_first_audio_none_when_empty() {
    let response = model_output_response(vec![Content::text("No audio here")]);
    assert!(response.first_audio().is_none());
}

#[test]
fn test_has_audio_true() {
    let response = model_output_response(vec![Content::audio_data(
        "YXVkaW8=",
        "audio/L16;codec=pcm;rate=24000",
    )]);
    assert!(response.has_audio());
}

#[test]
fn test_has_audio_false_when_no_audio() {
    let response = model_output_response(vec![Content::text("Text only")]);
    assert!(!response.has_audio());
}

#[test]
fn test_has_audio_false_when_uri_only() {
    // URI-based audio (no data) should not count as "having audio"
    // since the audios() iterator only includes data-based audio
    let response = model_output_response(vec![Content::audio_uri(
        "https://example.com/audio.mp3",
        "audio/mp3",
    )]);
    assert!(!response.has_audio());
}

// =========================================================================
// Image Helper Tests
// =========================================================================

fn make_response_with_image(base64_data: &str, mime_type: Option<&str>) -> InteractionResponse {
    InteractionResponse {
        id: Some("test-id".to_string()),
        model: Some("test-model".to_string()),
        steps: vec![Step::model_output(vec![Content::Image {
            data: Some(base64_data.to_string()),
            mime_type: mime_type.map(String::from),
            uri: None,
            resolution: None,
            extra: Default::default(),
        }])],
        status: InteractionStatus::Completed,
        ..Default::default()
    }
}

#[test]
fn test_first_image_bytes_success() {
    // Base64 for "test"
    let response = make_response_with_image("dGVzdA==", Some("image/png"));

    let bytes = response.first_image_bytes().unwrap();
    assert_eq!(bytes.unwrap(), b"test");
}

#[test]
fn test_first_image_bytes_no_images() {
    let response = text_response("Hello");
    assert!(response.first_image_bytes().unwrap().is_none());
}

#[test]
fn test_first_image_bytes_invalid_base64() {
    let response = make_response_with_image("not-valid-base64!!!", Some("image/png"));
    let err = response.first_image_bytes().unwrap_err().to_string();
    assert!(err.contains("Invalid base64"));
}

#[test]
fn test_images_iterator() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![
            Content::image_data("dGVzdDE=", "image/png"),
            Content::text("text between"),
            Content::image_data("dGVzdDI=", "image/jpeg"),
        ])],
        ..Default::default()
    };

    let images: Vec<_> = response.images();
    assert_eq!(images.len(), 2);

    assert_eq!(images[0].bytes().unwrap(), b"test1");
    assert_eq!(images[0].mime_type(), Some("image/png"));
    assert_eq!(images[0].extension(), "png");

    assert_eq!(images[1].bytes().unwrap(), b"test2");
    assert_eq!(images[1].extension(), "jpg");
}

#[test]
fn test_has_images() {
    assert!(make_response_with_image("dGVzdA==", Some("image/png")).has_images());
    assert!(!text_response("no images").has_images());
}

#[test]
fn test_audio_channels_and_sample_rate_exposed() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![Content::Audio {
            data: Some("dGVzdA==".into()),
            uri: None,
            mime_type: Some("audio/l16".into()),
            sample_rate: Some(24000),
            channels: Some(1),
            extra: Default::default(),
        }])],
        ..Default::default()
    };
    let audio = response.first_audio().unwrap();
    assert_eq!(audio.sample_rate(), Some(24000));
    assert_eq!(audio.channels(), Some(1));
    assert!(response.has_audio());
}
