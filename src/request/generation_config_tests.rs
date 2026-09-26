use super::*;

// =========================================================================
// SpeechConfig Tests
// =========================================================================

#[test]
fn test_speech_config_with_voice() {
    let config = SpeechConfig::with_voice("Kore");
    assert_eq!(config.voice, Some("Kore".to_string()));
    assert_eq!(config.language, None);
    assert_eq!(config.speaker, None);
}

#[test]
fn test_speech_config_with_voice_and_language() {
    let config = SpeechConfig::with_voice_and_language("Puck", "en-GB");
    assert_eq!(config.voice, Some("Puck".to_string()));
    assert_eq!(config.language, Some("en-GB".to_string()));
    assert_eq!(config.speaker, None);
}

#[test]
fn test_transcription_mode_wire_forms() {
    let config = TranscriptionConfig::new().with_mode(TranscriptionMode::Verbatim {
        diarization_mode: Some("speaker".into()),
        timestamp_granularities: Some(vec!["word".into()]),
    });
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({"mode": {
            "type": "verbatim",
            "diarization_mode": "speaker",
            "timestamp_granularities": ["word"]
        }})
    );
    for (wire, expected) in [
        (serde_json::json!("smart"), TranscriptionMode::Smart),
        (
            serde_json::json!({"type": "smart"}),
            TranscriptionMode::Smart,
        ),
        (
            serde_json::json!("verbatim"),
            TranscriptionMode::Verbatim {
                diarization_mode: None,
                timestamp_granularities: None,
            },
        ),
    ] {
        assert_eq!(
            serde_json::from_value::<TranscriptionMode>(wire).unwrap(),
            expected
        );
    }
    let unknown: TranscriptionMode =
        serde_json::from_value(serde_json::json!({"type": "future", "x": 1})).unwrap();
    assert_eq!(unknown.unknown_mode_type(), Some("future"));
    assert_eq!(
        serde_json::to_value(&unknown).unwrap(),
        serde_json::json!({"type": "future", "x": 1})
    );
}

#[test]
fn test_speech_config_for_speaker() {
    let config = SpeechConfig::for_speaker("Bob", "Puck", "en-US");
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({"voice": "Puck", "language": "en-US", "speaker": "Bob"})
    );
}

#[test]
fn test_speech_config_serialization() {
    let config = SpeechConfig {
        voice: Some("Fenrir".to_string()),
        language: Some("en-US".to_string()),
        speaker: None,
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify flat format is produced (voice, language at top level)
    assert_eq!(value["voice"], "Fenrir");
    assert_eq!(value["language"], "en-US");
    assert!(value.get("speaker").is_none()); // None fields should be skipped

    // Verify nested format is NOT produced
    // Google docs suggest voiceConfig.prebuiltVoiceConfig.voiceName but that returns 400.
    // See docs/ENUM_WIRE_FORMATS.md ("SpeechConfig (generation_config)").
    assert!(
        value.get("voiceConfig").is_none(),
        "Should use flat format, not nested voiceConfig"
    );
    assert!(
        value.get("prebuiltVoiceConfig").is_none(),
        "Should use flat format, not nested prebuiltVoiceConfig"
    );
}

#[test]
fn test_speech_config_roundtrip() {
    let config = SpeechConfig {
        voice: Some("Aoede".to_string()),
        language: Some("es-ES".to_string()),
        speaker: Some("narrator".to_string()),
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let parsed: SpeechConfig = serde_json::from_str(&json).expect("Deserialization failed");

    assert_eq!(config.voice, parsed.voice);
    assert_eq!(config.language, parsed.language);
    assert_eq!(config.speaker, parsed.speaker);
}

#[test]
fn test_speech_config_default() {
    let config = SpeechConfig::default();
    assert_eq!(config.voice, None);
    assert_eq!(config.language, None);
    assert_eq!(config.speaker, None);
}

// =========================================================================
// ImageAspectRatio Tests
// =========================================================================

#[test]
fn test_image_aspect_ratio_serialization() {
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Square).unwrap(),
        "\"1:1\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Portrait2x3).unwrap(),
        "\"2:3\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Landscape3x2).unwrap(),
        "\"3:2\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Portrait3x4).unwrap(),
        "\"3:4\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Landscape4x3).unwrap(),
        "\"4:3\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Portrait4x5).unwrap(),
        "\"4:5\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Landscape5x4).unwrap(),
        "\"5:4\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Portrait9x16).unwrap(),
        "\"9:16\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Widescreen16x9).unwrap(),
        "\"16:9\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Ultrawide21x9).unwrap(),
        "\"21:9\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Tall1x8).unwrap(),
        "\"1:8\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Wide8x1).unwrap(),
        "\"8:1\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Tall1x4).unwrap(),
        "\"1:4\""
    );
    assert_eq!(
        serde_json::to_string(&ImageAspectRatio::Wide4x1).unwrap(),
        "\"4:1\""
    );
}

#[test]
fn test_image_aspect_ratio_deserialization_roundtrip() {
    let ratios = vec![
        ("\"1:1\"", ImageAspectRatio::Square),
        ("\"2:3\"", ImageAspectRatio::Portrait2x3),
        ("\"3:2\"", ImageAspectRatio::Landscape3x2),
        ("\"3:4\"", ImageAspectRatio::Portrait3x4),
        ("\"4:3\"", ImageAspectRatio::Landscape4x3),
        ("\"4:5\"", ImageAspectRatio::Portrait4x5),
        ("\"5:4\"", ImageAspectRatio::Landscape5x4),
        ("\"9:16\"", ImageAspectRatio::Portrait9x16),
        ("\"16:9\"", ImageAspectRatio::Widescreen16x9),
        ("\"21:9\"", ImageAspectRatio::Ultrawide21x9),
        ("\"1:8\"", ImageAspectRatio::Tall1x8),
        ("\"8:1\"", ImageAspectRatio::Wide8x1),
        ("\"1:4\"", ImageAspectRatio::Tall1x4),
        ("\"4:1\"", ImageAspectRatio::Wide4x1),
    ];

    for (json, expected) in ratios {
        let parsed: ImageAspectRatio = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, expected);

        // Roundtrip
        let serialized = serde_json::to_string(&parsed).unwrap();
        assert_eq!(serialized, json);
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_image_aspect_ratio_unknown_roundtrip() {
    let unknown: ImageAspectRatio = serde_json::from_str("\"7:3\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_ratio_type(), Some("7:3"));
    assert!(unknown.unknown_data().is_some());

    // Roundtrip preserves the unknown value
    let json = serde_json::to_string(&unknown).unwrap();
    assert_eq!(json, "\"7:3\"");
}

#[test]
fn test_image_aspect_ratio_known_not_unknown() {
    assert!(!ImageAspectRatio::Square.is_unknown());
    assert_eq!(ImageAspectRatio::Widescreen16x9.unknown_ratio_type(), None);
    assert_eq!(ImageAspectRatio::Portrait2x3.unknown_data(), None);
}

// =========================================================================
// ImageSize Tests
// =========================================================================

#[test]
fn test_image_size_serialization() {
    assert_eq!(serde_json::to_string(&ImageSize::Sd512).unwrap(), "\"512\"");
    assert_eq!(serde_json::to_string(&ImageSize::Hd1k).unwrap(), "\"1K\"");
    assert_eq!(serde_json::to_string(&ImageSize::Hd2k).unwrap(), "\"2K\"");
    assert_eq!(serde_json::to_string(&ImageSize::Uhd4k).unwrap(), "\"4K\"");
}

#[test]
fn test_image_size_deserialization_roundtrip() {
    let sizes = vec![
        ("\"512\"", ImageSize::Sd512),
        ("\"1K\"", ImageSize::Hd1k),
        ("\"2K\"", ImageSize::Hd2k),
        ("\"4K\"", ImageSize::Uhd4k),
    ];

    for (json, expected) in sizes {
        let parsed: ImageSize = serde_json::from_str(json).unwrap();
        assert_eq!(parsed, expected);

        let serialized = serde_json::to_string(&parsed).unwrap();
        assert_eq!(serialized, json);
    }
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_image_size_unknown_roundtrip() {
    let unknown: ImageSize = serde_json::from_str("\"8K\"").unwrap();
    assert!(unknown.is_unknown());
    assert_eq!(unknown.unknown_size_type(), Some("8K"));
    assert!(unknown.unknown_data().is_some());

    let json = serde_json::to_string(&unknown).unwrap();
    assert_eq!(json, "\"8K\"");
}

#[test]
fn test_image_size_known_not_unknown() {
    assert!(!ImageSize::Sd512.is_unknown());
    assert_eq!(ImageSize::Hd1k.unknown_size_type(), None);
    assert_eq!(ImageSize::Uhd4k.unknown_data(), None);
}

// =========================================================================
// ImageConfig Tests
// =========================================================================

#[test]
fn test_image_config_serialization_roundtrip() {
    let config = ImageConfig {
        aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
        image_size: Some(ImageSize::Hd2k),
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let parsed: ImageConfig = serde_json::from_str(&json).expect("Deserialization failed");

    assert_eq!(config, parsed);
}

#[test]
fn test_image_config_default() {
    let config = ImageConfig::default();
    assert_eq!(config.aspect_ratio, None);
    assert_eq!(config.image_size, None);
}

#[test]
fn test_image_config_partial_fields() {
    let config = ImageConfig {
        aspect_ratio: Some(ImageAspectRatio::Square),
        image_size: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["aspect_ratio"], "1:1");
    assert!(value.get("image_size").is_none());
}

#[test]
fn test_image_config_skip_serializing_none() {
    let config = ImageConfig::default();
    let json = serde_json::to_string(&config).unwrap();
    assert_eq!(json, "{}");
}

#[test]
fn test_generation_config_with_image_config() {
    let config = GenerationConfig {
        image_config: Some(ImageConfig {
            aspect_ratio: Some(ImageAspectRatio::Portrait9x16),
            image_size: Some(ImageSize::Uhd4k),
        }),
        ..Default::default()
    };

    let json = serde_json::to_string(&config).expect("Serialization failed");
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["image_config"]["aspect_ratio"], "9:16");
    assert_eq!(value["image_config"]["image_size"], "4K");
}

// =========================================================================
// GenerationConfig tool_choice / penalty Tests
// =========================================================================

#[test]
fn test_generation_config_tool_choice_mode_serializes_lowercase() {
    let config = GenerationConfig {
        tool_choice: Some(ToolChoice::Mode(crate::tools::FunctionCallingMode::Any)),
        ..Default::default()
    };

    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["tool_choice"], "any");
}

#[test]
fn test_generation_config_tool_choice_allowed_tools_object() {
    let config = GenerationConfig {
        tool_choice: Some(ToolChoice::allowed_tools(
            Some(crate::tools::FunctionCallingMode::Any),
            vec!["get_weather".to_string(), "get_time".to_string()],
        )),
        ..Default::default()
    };

    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["tool_choice"]["allowed_tools"]["mode"], "any");
    assert_eq!(
        value["tool_choice"]["allowed_tools"]["tools"][0],
        "get_weather"
    );
    assert!(
        value.get("allowed_tools").is_none(),
        "top-level allowed_tools was removed from generation_config"
    );
}

#[test]
fn test_generation_config_penalties_serialize() {
    let config = GenerationConfig {
        presence_penalty: Some(0.5),
        frequency_penalty: Some(-0.5),
        ..Default::default()
    };
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["presence_penalty"], 0.5);
    assert_eq!(value["frequency_penalty"], -0.5);
}

#[test]
fn test_generation_config_has_no_top_k() {
    // top_k was dropped from the 2026-05-20 spec; ensure it never serializes.
    let config = GenerationConfig {
        temperature: Some(0.3),
        ..Default::default()
    };
    let value = serde_json::to_value(&config).unwrap();
    assert!(value.get("top_k").is_none());
}
