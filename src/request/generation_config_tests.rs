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
    let config = TranscriptionConfig::new().with_mode(TranscriptionMode::verbatim(
        Some("speaker".into()),
        Some(vec!["word".into()]),
    ));
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({"mode": {
            "type": "verbatim",
            "diarization_mode": "speaker",
            "timestamp_granularities": ["word"]
        }})
    );
    for (wire, expected) in [
        (serde_json::json!("smart"), TranscriptionMode::smart(None)),
        (
            serde_json::json!({"type": "smart"}),
            TranscriptionMode::smart(None),
        ),
        (
            serde_json::json!({"type": "smart", "source_text": "Hello there."}),
            TranscriptionMode::smart(Some("Hello there.".into())),
        ),
        (
            serde_json::json!("verbatim"),
            TranscriptionMode::verbatim(None, None),
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

/// Exact serialized strings: the tag comes first, `None` fields are
/// omitted, `Some(vec![])` is still written, and fields keep their order.
#[test]
fn test_transcription_mode_serialized_strings() {
    let verbatim = |diarization_mode: Option<&str>, granularities: Option<Vec<&str>>| {
        TranscriptionMode::verbatim(
            diarization_mode.map(String::from),
            granularities.map(|g| g.into_iter().map(String::from).collect()),
        )
    };
    for (mode, expected) in [
        (TranscriptionMode::smart(None), r#"{"type":"smart"}"#),
        (
            TranscriptionMode::smart(Some("Hi.".into())),
            r#"{"type":"smart","source_text":"Hi."}"#,
        ),
        (verbatim(None, None), r#"{"type":"verbatim"}"#),
        (
            verbatim(Some("speaker"), None),
            r#"{"type":"verbatim","diarization_mode":"speaker"}"#,
        ),
        (
            verbatim(None, Some(vec![])),
            r#"{"type":"verbatim","timestamp_granularities":[]}"#,
        ),
        (
            verbatim(Some("speaker"), Some(vec!["word"])),
            r#"{"type":"verbatim","diarization_mode":"speaker","timestamp_granularities":["word"]}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&mode).unwrap(), expected, "{mode:?}");
    }
}

/// An unknown mode re-serializes as its `data`, verbatim: `mode_type` is
/// not merged in, so data without a `"type"` stays without one, a stale
/// `"type"` is kept, and non-object or null data is written as-is.
#[test]
fn test_transcription_mode_unknown_serializes_data_verbatim() {
    let unknown = |data: serde_json::Value| TranscriptionMode::Unknown {
        mode_type: "future".into(),
        data,
    };
    for (mode, expected) in [
        (unknown(serde_json::json!({"x": 1})), r#"{"x":1}"#),
        (
            unknown(serde_json::json!({"type": "stale", "x": 1})),
            r#"{"type":"stale","x":1}"#,
        ),
        (unknown(serde_json::json!("future")), r#""future""#),
        (unknown(serde_json::json!([1, 2])), "[1,2]"),
        (unknown(serde_json::Value::Null), "null"),
    ] {
        assert_eq!(serde_json::to_string(&mode).unwrap(), expected, "{mode:?}");
    }

    // A bare-string unknown read off the wire goes back out as that string.
    let read: TranscriptionMode = serde_json::from_str(r#""future""#).unwrap();
    assert_eq!(read.unknown_mode_type(), Some("future"));
    assert_eq!(serde_json::to_string(&read).unwrap(), r#""future""#);
}

/// A known mode keeps the fields it does not model and writes them back
/// after its own (D-001): a stored interaction's `mode` used to lose them.
#[test]
fn test_transcription_mode_keeps_unmodeled_fields() {
    for wire in [
        serde_json::json!({"type": "smart", "source_text": "Hi.", "future_knob": {"a": 1}}),
        serde_json::json!({"type": "verbatim", "diarization_mode": "speaker", "future_knob": 2}),
    ] {
        let mode: TranscriptionMode = serde_json::from_value(wire.clone()).unwrap();
        assert!(!mode.is_unknown(), "{mode:?}");
        assert_eq!(mode.extra().unwrap()["future_knob"], wire["future_knob"]);
        assert_eq!(serde_json::to_value(&mode).unwrap(), wire);
    }

    // The bare-string form has nothing to keep.
    let smart: TranscriptionMode = serde_json::from_value(serde_json::json!("smart")).unwrap();
    assert_eq!(smart.extra().map(serde_json::Map::len), Some(0));

    // A hand-set entry is written after the modeled fields, unless it names
    // one of them or `type`.
    let mut mode = TranscriptionMode::smart(Some("Hi.".into()));
    let extra = mode.extra_mut().unwrap();
    extra.insert("source_text".into(), serde_json::json!("shadow"));
    extra.insert("type".into(), serde_json::json!("verbatim"));
    extra.insert("future_knob".into(), serde_json::json!(true));
    assert_eq!(
        serde_json::to_string(&mode).unwrap(),
        r#"{"type":"smart","source_text":"Hi.","future_knob":true}"#
    );

    let unknown = TranscriptionMode::Unknown {
        mode_type: "future".into(),
        data: serde_json::json!("future"),
    };
    assert!(unknown.extra().is_none());
}

/// A known mode with a mistyped field is kept whole as `Unknown`, where it
/// used to be read leniently, dropping the bad value.
#[test]
fn test_transcription_mode_with_a_malformed_field_is_kept_whole() {
    for wire in [
        serde_json::json!({"type": "smart", "source_text": 7}),
        serde_json::json!({"type": "verbatim", "timestamp_granularities": "word"}),
    ] {
        let mode: TranscriptionMode = serde_json::from_value(wire.clone()).unwrap();
        assert!(mode.is_unknown(), "{mode:?}");
        assert_eq!(serde_json::to_value(&mode).unwrap(), wire);
    }
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
        ..Default::default()
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
        ..Default::default()
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

#[cfg(not(genai_strict_unknown))]
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

#[cfg(not(genai_strict_unknown))]
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
        ..Default::default()
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
        ..Default::default()
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
            ..Default::default()
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

/// The Gemini API rejects both penalties ("Unknown parameter
/// 'presence_penalty' at 'generation_config'", live 2026-11-01), and neither
/// is in the google-genai 2.34.0 bindings, so they are not modeled (D-005).
#[test]
fn test_generation_config_has_no_penalties() {
    let value = serde_json::to_value(GenerationConfig::default()).unwrap();
    assert!(value.get("presence_penalty").is_none());
    assert!(value.get("frequency_penalty").is_none());
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

// =========================================================================
// Wire form: the speakers form's keys, and f32 range (D-001)
// =========================================================================

/// The `{"speakers": [...]}` form of `speech_config` keeps the keys beside
/// the list and is written back in that form; without them, it is written
/// as the list.
#[test]
fn speakers_form_keeps_the_keys_beside_the_list() {
    for speech_config in [
        serde_json::json!({"speakers": [{"voice": "Kore"}], "multi_speaker_mode": "x"}),
        serde_json::json!({"speakers": [{"voice": "Kore"}], "voice": "Puck", "future": 1}),
        serde_json::json!({"speakers": [], "future": [1, 2]}),
    ] {
        let wire = serde_json::json!({"speech_config": speech_config, "seed": 3});
        let config: GenerationConfig = serde_json::from_value(wire.clone()).unwrap();
        assert!(config.extra.is_empty(), "{:?}", config.extra);
        let mut beside = speech_config.as_object().unwrap().clone();
        let speakers = beside.remove("speakers").unwrap();
        assert_eq!(config.speech_config_extra, beside);
        assert_eq!(
            serde_json::to_value(&config.speech_config).unwrap(),
            speakers
        );
        let written = serde_json::to_value(&config).unwrap();
        assert_eq!(written, wire);
        let again: GenerationConfig = serde_json::from_value(written).unwrap();
        assert_eq!(again, config);
    }
    // Through a stored interaction read back.
    let wire = serde_json::json!({
        "id": "i1",
        "status": "completed",
        "generation_config": {
            "speech_config": {"speakers": [{"voice": "Kore"}], "multi_speaker_mode": "x"}
        }
    });
    let response: crate::InteractionResponse = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&response).unwrap()["generation_config"],
        wire["generation_config"]
    );
}

#[test]
fn speech_config_is_sent_as_the_list_without_keys_beside_it() {
    let config: GenerationConfig = serde_json::from_value(serde_json::json!({
        "speech_config": {"speakers": [{"voice": "Kore"}]}
    }))
    .unwrap();
    assert!(config.speech_config_extra.is_empty());
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({"speech_config": [{"voice": "Kore"}]})
    );
    // Keys set without a `speech_config` are not written, and a `speakers`
    // key among them never is.
    let config = GenerationConfig {
        speech_config_extra: serde_json::Map::from_iter([
            ("future".to_string(), serde_json::json!(1)),
            ("speakers".to_string(), serde_json::json!("not the list")),
        ]),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({})
    );
    let config = GenerationConfig {
        speech_config: Some(vec![SpeechConfig::with_voice("Kore")]),
        ..config
    };
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        serde_json::json!({"speech_config": {"speakers": [{"voice": "Kore"}], "future": 1}})
    );
}

/// Every field keeps its key and position on the way out, as the derived
/// serializer wrote it: modeled fields in declaration order, then `extra`.
#[test]
fn generation_config_writes_every_field_under_its_key() {
    let wire = serde_json::json!({
        "temperature": 0.5,
        "max_output_tokens": 100,
        "top_p": 0.25,
        "thinking_level": "high",
        "seed": 7,
        "stop_sequences": ["STOP"],
        "thinking_summaries": "auto",
        "tool_choice": "any",
        "speech_config": [{"voice": "Kore"}],
        "image_config": {"aspect_ratio": "1:1"},
        "video_config": {"task": "text_to_video"},
        "transcription_config": {"language_codes": ["en-US"]},
        "future_field": {"a": 1}
    });
    let config: GenerationConfig = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(config.extra.len(), 1);
    assert_eq!(serde_json::to_value(&config).unwrap(), wire);
    assert_eq!(
        serde_json::to_string(&GenerationConfig {
            seed: Some(1),
            temperature: Some(0.5),
            extra: serde_json::Map::from_iter([("a".to_string(), serde_json::json!(1))]),
            ..Default::default()
        })
        .unwrap(),
        r#"{"temperature":0.5,"seed":1,"a":1}"#
    );
}

/// An `f32` field whose number is beyond `f32`'s range would read as an
/// infinity, be written as `null` and read back as `None`; it is dropped on
/// the first read instead, so the config is stable from then on. The same
/// for the retrieval tool's hybrid-search `alpha`.
#[test]
fn f32_fields_beyond_range_are_dropped_on_read() {
    for wire in [
        serde_json::json!({"temperature": 1e39, "seed": 1}),
        serde_json::json!({"top_p": -1e39, "seed": 1}),
    ] {
        let config: GenerationConfig = serde_json::from_value(wire).unwrap();
        assert_eq!(config.temperature, None);
        assert_eq!(config.top_p, None);
        let written = serde_json::to_value(&config).unwrap();
        assert_eq!(written, serde_json::json!({"seed": 1}));
    }
    // In range, the value reads rounded to an f32, the largest included.
    let config: GenerationConfig = serde_json::from_value(serde_json::json!({
        "temperature": 3.4028235e38, "top_p": 0.123456789
    }))
    .unwrap();
    assert_eq!(config.temperature, Some(f32::MAX));
    assert_eq!(config.top_p, Some(0.123_456_79));

    let tool = serde_json::json!({
        "type": "retrieval",
        "retrieval_types": ["rag_store"],
        "rag_store_config": {"rag_retrieval_config": {"hybrid_search": {"alpha": 1e39}}}
    });
    let tool: crate::Tool = serde_json::from_value(tool).unwrap();
    let written = serde_json::to_value(&tool).unwrap();
    assert_eq!(
        written["rag_store_config"]["rag_retrieval_config"]["hybrid_search"],
        serde_json::json!({})
    );
    let again: crate::Tool = serde_json::from_value(written.clone()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), written);
}
