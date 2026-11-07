//! Unit tests for the builder's output settings: response modalities and
//! formats, image, speech, video and transcription config.

use super::super::tests::create_test_client;
use super::*;
use crate::{ImageAspectRatio, ImageSize};
use serde_json::json;

#[test]
fn test_interaction_builder_with_response_modalities() {
    let client = create_test_client();
    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Generate an image")
        .with_response_modalities(vec!["IMAGE".to_string()]);

    // Modalities are normalized to lowercase - the API is case-sensitive and
    // rejects uppercase values (verified live).
    assert_eq!(
        builder.response_modalities.as_ref().unwrap(),
        &vec!["image".to_string()]
    );
}

#[test]
fn test_with_image_config() {
    let client = create_test_client();
    let config = ImageConfig {
        aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
        image_size: Some(ImageSize::Hd2k),
        ..Default::default()
    };

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_IMAGE_MODEL)
        .with_text("Generate a landscape")
        .with_image_config(config);

    let gen_config = builder.generation_config.as_ref().unwrap();
    let image_config = gen_config.image_config.as_ref().unwrap();
    assert_eq!(
        image_config.aspect_ratio,
        Some(ImageAspectRatio::Widescreen16x9)
    );
    assert_eq!(image_config.image_size, Some(ImageSize::Hd2k));
}

#[test]
fn test_with_image_config_merges_with_existing_generation_config() {
    let client = create_test_client();
    let config = ImageConfig {
        aspect_ratio: Some(ImageAspectRatio::Square),
        image_size: None,
        ..Default::default()
    };

    let builder = client
        .interaction()
        .with_model(crate::DEFAULT_IMAGE_MODEL)
        .with_text("Generate an image")
        .with_thinking_level(crate::ThinkingLevel::Low)
        .with_image_config(config);

    let gen_config = builder.generation_config.as_ref().unwrap();
    // Both thinking_level and image_config should be present
    assert!(gen_config.thinking_level.is_some());
    assert!(gen_config.image_config.is_some());
    assert_eq!(
        gen_config.image_config.as_ref().unwrap().aspect_ratio,
        Some(ImageAspectRatio::Square)
    );
}

#[test]
fn test_builder_with_response_format_raw_schema_maps_to_text() {
    // Backward compatibility: raw serde_json::Value schemas keep working and
    // now serialize inside the typed text format.
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Generate data")
        .with_response_format(json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        }))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["response_format"]["type"], "text");
    assert_eq!(value["response_format"]["mime_type"], "application/json");
    assert_eq!(
        value["response_format"]["schema"]["properties"]["name"]["type"],
        "string"
    );
}

#[test]
fn test_builder_with_response_format_typed_variant() {
    use crate::{ResponseDelivery, ResponseFormat};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_TTS_MODEL)
        .with_text("Read aloud")
        .with_response_format(ResponseFormat::Audio {
            mime_type: Some("audio/mp3".to_string()),
            delivery: Some(ResponseDelivery::Inline),
            sample_rate: Some(24000),
            bit_rate: None,
            extra: Default::default(),
        })
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["response_format"]["type"], "audio");
    assert_eq!(value["response_format"]["mime_type"], "audio/mp3");
    assert_eq!(value["response_format"]["delivery"], "inline");
    assert_eq!(value["response_format"]["sample_rate"], 24000);
}

#[test]
fn test_builder_with_response_formats_list() {
    use crate::ResponseFormat;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_IMAGE_MODEL)
        .with_text("A diagram")
        .with_response_formats(vec![
            ResponseFormat::text_plain(),
            ResponseFormat::Image {
                mime_type: Some("image/jpeg".to_string()),
                delivery: None,
                aspect_ratio: None,
                image_size: None,
                extra: Default::default(),
            },
        ])
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let formats = value["response_format"].as_array().expect("list form");
    assert_eq!(formats.len(), 2);
    assert_eq!(formats[0]["type"], "text");
    assert_eq!(formats[1]["type"], "image");
}

#[test]
fn test_builder_with_speech_config_serializes_as_single_entry_list() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_TTS_MODEL)
        .with_text("Hello")
        .with_speech_config(SpeechConfig::with_voice_and_language("Kore", "en-US"))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let speech = value["generation_config"]["speech_config"]
        .as_array()
        .expect("speech_config must be a list on the wire");
    assert_eq!(speech.len(), 1);
    assert_eq!(speech[0]["voice"], "Kore");
    assert_eq!(speech[0]["language"], "en-US");
}

#[test]
fn test_builder_multi_speaker_speech_configs() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_TTS_MODEL)
        .with_text("Alice: hi\nBob: hey")
        .add_speech_config(SpeechConfig {
            voice: Some("Kore".to_string()),
            language: Some("en-US".to_string()),
            speaker: Some("Alice".to_string()),
            ..Default::default()
        })
        .add_speech_config(SpeechConfig {
            voice: Some("Puck".to_string()),
            language: Some("en-US".to_string()),
            speaker: Some("Bob".to_string()),
            ..Default::default()
        })
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let speech = value["generation_config"]["speech_config"]
        .as_array()
        .expect("speech_config must be a list on the wire");
    assert_eq!(speech.len(), 2);
    assert_eq!(speech[0]["speaker"], "Alice");
    assert_eq!(speech[1]["speaker"], "Bob");
}

#[test]
fn test_builder_with_speech_configs_replaces() {
    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_TTS_MODEL)
        .with_text("Hello")
        .add_speech_config(SpeechConfig::with_voice("Kore"))
        .with_speech_configs(vec![SpeechConfig::with_voice("Puck")])
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let speech = value["generation_config"]["speech_config"]
        .as_array()
        .unwrap();
    assert_eq!(speech.len(), 1, "with_speech_configs replaces");
    assert_eq!(speech[0]["voice"], "Puck");
}

#[test]
fn test_builder_with_video_config_and_output() {
    use crate::{VideoConfig, VideoTask};

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model("veo-3.1-generate-preview")
        .with_text("A hummingbird in slow motion")
        .with_video_output()
        .with_video_config(VideoConfig::new().with_task(VideoTask::TextToVideo))
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(value["response_modalities"][0], "video");
    assert_eq!(
        value["generation_config"]["video_config"]["task"],
        "text_to_video"
    );
}

#[test]
fn test_builder_with_transcription_config() {
    use crate::TranscriptionConfig;

    let client = create_test_client();
    let request = client
        .interaction()
        .with_model(crate::DEFAULT_MODEL)
        .with_text("Transcribe this")
        .with_generation_config(GenerationConfig {
            temperature: Some(0.5), // pre-existing generation_config must survive
            ..Default::default()
        })
        .with_transcription_config(
            TranscriptionConfig::new()
                .with_language_codes(["en-US"])
                .with_diarization_mode("speaker"),
        )
        .build()
        .unwrap();

    let value = serde_json::to_value(&request).unwrap();
    let gc = &value["generation_config"];
    assert_eq!(gc["temperature"], 0.5);
    assert_eq!(gc["transcription_config"]["language_codes"][0], "en-US");
    assert_eq!(gc["transcription_config"]["diarization_mode"], "speaker");
}

#[test]
fn test_transcription_config_builder_methods() {
    use crate::TranscriptionConfig;

    // The builder form covers every field the struct-literal form does,
    // with add_adaptation_phrase accumulating.
    let config = TranscriptionConfig::new()
        .with_language_codes(["en-US", "ja-JP"])
        .with_diarization_mode("speaker")
        .with_timestamp_granularities(["word"])
        .with_custom_vocabulary(["genai-rs"])
        .with_adaptation_phrases(["stale, replaced wholesale"])
        .with_adaptation_phrases(["Interactions API"])
        .add_adaptation_phrase("Evergreen");

    assert_eq!(
        config.language_codes.as_deref(),
        Some(["en-US".to_string(), "ja-JP".to_string()].as_slice())
    );
    assert_eq!(config.diarization_mode.as_deref(), Some("speaker"));
    assert_eq!(
        config.timestamp_granularities.as_deref(),
        Some(["word".to_string()].as_slice())
    );
    assert_eq!(
        config.custom_vocabulary.as_deref(),
        Some(["genai-rs".to_string()].as_slice())
    );
    assert_eq!(
        config.adaptation_phrases.as_deref(),
        Some(["Interactions API".to_string(), "Evergreen".to_string()].as_slice())
    );
}
