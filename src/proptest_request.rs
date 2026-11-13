//! Property tests: the request side: input, generation and agent config, image and video
//! settings, safety settings and response formats.

use super::content::arb_content;
use super::steps::arb_step;
use super::tools::arb_tool_choice;
use super::*;

/// Strategy for ThinkingLevel.
pub(super) fn arb_thinking_level() -> impl Strategy<Value = ThinkingLevel> {
    prop_oneof![
        Just(ThinkingLevel::Minimal),
        Just(ThinkingLevel::Low),
        Just(ThinkingLevel::Medium),
        Just(ThinkingLevel::High),
        // Unknown variant with preserved data
        unknown_or(ThinkingLevel::Minimal, |level_type, data| {
            ThinkingLevel::Unknown { level_type, data }
        }),
    ]
}

/// Strategy for ThinkingSummaries.
pub(super) fn arb_thinking_summaries() -> impl Strategy<Value = ThinkingSummaries> {
    prop_oneof![
        Just(ThinkingSummaries::Auto),
        Just(ThinkingSummaries::None),
        // Unknown variant with preserved data
        unknown_or(ThinkingSummaries::Auto, |summaries_type, data| {
            ThinkingSummaries::Unknown {
                summaries_type,
                data,
            }
        }),
    ]
}

/// Strategy for ServiceTier (wire format is lowercase).
pub(super) fn arb_service_tier() -> impl Strategy<Value = ServiceTier> {
    prop_oneof![
        Just(ServiceTier::Flex),
        Just(ServiceTier::Standard),
        Just(ServiceTier::Priority),
        Just(ServiceTier::Deferred),
        // Unknown variant with preserved data
        unknown_or(ServiceTier::Flex, |tier_type, data| ServiceTier::Unknown {
            tier_type,
            data,
        }),
    ]
}

// =============================================================================
// ImageAspectRatio / ImageSize / ImageConfig Strategies
// =============================================================================

pub(super) fn arb_image_aspect_ratio() -> impl Strategy<Value = ImageAspectRatio> {
    prop_oneof![
        Just(ImageAspectRatio::Square),
        Just(ImageAspectRatio::Portrait2x3),
        Just(ImageAspectRatio::Landscape3x2),
        Just(ImageAspectRatio::Portrait3x4),
        Just(ImageAspectRatio::Landscape4x3),
        Just(ImageAspectRatio::Portrait4x5),
        Just(ImageAspectRatio::Landscape5x4),
        Just(ImageAspectRatio::Portrait9x16),
        Just(ImageAspectRatio::Widescreen16x9),
        Just(ImageAspectRatio::Ultrawide21x9),
        Just(ImageAspectRatio::Tall1x8),
        Just(ImageAspectRatio::Wide8x1),
        Just(ImageAspectRatio::Tall1x4),
        Just(ImageAspectRatio::Wide4x1),
        // Unknown variant with preserved data
        unknown_or(ImageAspectRatio::Square, |ratio_type, data| {
            ImageAspectRatio::Unknown { ratio_type, data }
        }),
    ]
}

pub(super) fn arb_image_size() -> impl Strategy<Value = ImageSize> {
    prop_oneof![
        Just(ImageSize::Sd512),
        Just(ImageSize::Hd1k),
        Just(ImageSize::Hd2k),
        Just(ImageSize::Uhd4k),
        // Unknown variant with preserved data
        unknown_or(ImageSize::Sd512, |size_type, data| ImageSize::Unknown {
            size_type,
            data,
        }),
    ]
}

pub(super) fn arb_image_config() -> impl Strategy<Value = ImageConfig> {
    (
        proptest::option::of(arb_image_aspect_ratio()),
        proptest::option::of(arb_image_size()),
    )
        .prop_map(|(aspect_ratio, image_size)| ImageConfig {
            aspect_ratio,
            image_size,
            extra: serde_json::Map::new(),
        })
}

// =============================================================================
// InteractionInput Strategy
// =============================================================================

/// Strategy for InteractionInput.
///
/// Note: `Content` is generated non-empty because an empty JSON array
/// deserializes as `Steps(vec![])` (the canonical revision 2026-05-20 form).
pub(super) fn arb_interaction_input() -> impl Strategy<Value = InteractionInput> {
    prop_oneof![
        arb_text().prop_map(InteractionInput::Text),
        prop::collection::vec(arb_content(), 1..3).prop_map(InteractionInput::Content),
        prop::collection::vec(arb_step(), 0..3).prop_map(InteractionInput::Steps),
    ]
}

// =============================================================================
// TurnContent Strategy (still used by ConversationBuilder)
// =============================================================================

pub(super) fn arb_turn_content() -> impl Strategy<Value = TurnContent> {
    prop_oneof![
        // Text content
        arb_text().prop_map(TurnContent::Text),
        // Parts content
        prop::collection::vec(arb_content(), 0..3).prop_map(TurnContent::Parts),
    ]
}

// =============================================================================
// GenerationConfig Strategies
// =============================================================================

pub(crate) fn arb_speech_config() -> impl Strategy<Value = SpeechConfig> {
    (
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
    )
        .prop_map(|(voice, language, speaker)| SpeechConfig {
            voice,
            language,
            speaker,
            extra: serde_json::Map::new(),
        })
}

/// Strategy for `TranscriptionMode`: both known modes, and an unknown one in
/// the string and the object form (kept in strict mode too).
pub(super) fn arb_transcription_mode() -> impl Strategy<Value = TranscriptionMode> {
    prop_oneof![
        // Literals, not the constructors: the coverage oracle samples this,
        // and a constructor could leave a new field unset (its `fills` scan).
        proptest::option::of(arb_text()).prop_map(|source_text| TranscriptionMode::Smart {
            source_text,
            extra: serde_json::Map::new(),
        }),
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        )
            .prop_map(|(diarization_mode, timestamp_granularities)| {
                TranscriptionMode::Verbatim {
                    diarization_mode,
                    timestamp_granularities,
                    extra: serde_json::Map::new(),
                }
            }),
        arb_unknown_type().prop_map(|mode_type| TranscriptionMode::Unknown {
            data: serde_json::Value::String(mode_type.clone()),
            mode_type,
        }),
        (arb_unknown_type(), arb_unknown_object()).prop_map(|(mode_type, mut data)| {
            data["type"] = serde_json::Value::String(mode_type.clone());
            TranscriptionMode::Unknown { mode_type, data }
        }),
    ]
}

pub(crate) fn arb_transcription_config() -> impl Strategy<Value = TranscriptionConfig> {
    (
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(arb_identifier()),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(arb_transcription_mode()),
    )
        .prop_map(
            |(
                adaptation_phrases,
                custom_vocabulary,
                diarization_mode,
                language_codes,
                timestamp_granularities,
                mode,
            )| TranscriptionConfig {
                adaptation_phrases,
                custom_vocabulary,
                diarization_mode,
                language_codes,
                timestamp_granularities,
                mode,
                extra: serde_json::Map::new(),
            },
        )
}

pub(crate) fn arb_generation_config() -> impl Strategy<Value = GenerationConfig> {
    // Split into two tuples to stay under proptest's 12-element limit
    let part1 = (
        proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32)),
        proptest::option::of(1..10000i32),
        proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32)),
        proptest::option::of(arb_thinking_level()),
        proptest::option::of(1..1000i64),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(arb_thinking_summaries()),
    );
    let part2 = (
        proptest::option::of(arb_tool_choice()),
        proptest::option::of(proptest::collection::vec(arb_speech_config(), 1..3)),
        proptest::option::of(arb_image_config()),
        proptest::option::of(arb_video_config()),
        proptest::option::of(arb_transcription_config()),
    );
    (part1, part2).prop_map(
        |(
            (
                temperature,
                max_output_tokens,
                top_p,
                thinking_level,
                seed,
                stop_sequences,
                thinking_summaries,
            ),
            (tool_choice, speech_config, image_config, video_config, transcription_config),
        )| {
            GenerationConfig {
                temperature,
                max_output_tokens,
                top_p,
                thinking_level,
                seed,
                stop_sequences,
                thinking_summaries,
                tool_choice,
                speech_config,
                // The `{"speakers": [...]}` form's keys are not the crate's,
                // and the request oracle samples this strategy; they get a
                // property of their own.
                speech_config_extra: serde_json::Map::new(),
                image_config,
                video_config,
                transcription_config,
                extra: serde_json::Map::new(),
            }
        },
    )
}

pub(super) fn arb_video_task() -> impl Strategy<Value = VideoTask> {
    prop_oneof![
        Just(VideoTask::TextToVideo),
        Just(VideoTask::ImageToVideo),
        Just(VideoTask::ReferenceToVideo),
        Just(VideoTask::Edit),
        Just(VideoTask::Extend),
        unknown_or(VideoTask::TextToVideo, |task_type, data| {
            VideoTask::Unknown { data, task_type }
        }),
    ]
}

pub(super) fn arb_video_config() -> impl Strategy<Value = VideoConfig> {
    proptest::option::of(arb_video_task()).prop_map(|task| VideoConfig {
        task,
        extra: serde_json::Map::new(),
    })
}

// =============================================================================
// AgentConfig Strategies
// =============================================================================

/// Strategy for AgentConfig using typed config structs.
/// Since AgentConfig is a thin wrapper around serde_json::Value,
/// we generate configs via the typed structs (DeepResearchConfig, DynamicConfig)
/// and the raw from_value() method for arbitrary configs.
pub(super) fn arb_agent_config() -> impl Strategy<Value = AgentConfig> {
    prop_oneof![
        // DeepResearch config with optional thinking summaries
        proptest::option::of(arb_thinking_summaries()).prop_map(|thinking_summaries| {
            let mut config = DeepResearchConfig::new();
            if let Some(ts) = thinking_summaries {
                config = config.with_thinking_summaries(ts);
            }
            config.into()
        }),
        // Dynamic config
        Just(DynamicConfig::new().into()),
        // Arbitrary config via from_value (for future agent types)
        arb_identifier().prop_map(|config_type| {
            AgentConfig::from_value(serde_json::json!({
                "type": config_type,
                "customField": 42
            }))
        }),
    ]
}

pub(super) fn arb_harm_category() -> impl Strategy<Value = HarmCategory> {
    prop_oneof![
        Just(HarmCategory::HateSpeech),
        Just(HarmCategory::DangerousContent),
        Just(HarmCategory::Harassment),
        Just(HarmCategory::SexuallyExplicit),
        Just(HarmCategory::CivicIntegrity),
        Just(HarmCategory::ImageHate),
        Just(HarmCategory::ImageDangerousContent),
        Just(HarmCategory::ImageHarassment),
        Just(HarmCategory::ImageSexuallyExplicit),
        Just(HarmCategory::Jailbreak),
        unknown_or(HarmCategory::HateSpeech, |category_type, data| {
            HarmCategory::Unknown {
                data,
                category_type,
            }
        }),
    ]
}

pub(super) fn arb_safety_threshold() -> impl Strategy<Value = SafetyThreshold> {
    prop_oneof![
        Just(SafetyThreshold::BlockLowAndAbove),
        Just(SafetyThreshold::BlockMediumAndAbove),
        Just(SafetyThreshold::BlockOnlyHigh),
        Just(SafetyThreshold::BlockNone),
        Just(SafetyThreshold::Off),
        unknown_or(SafetyThreshold::BlockLowAndAbove, |threshold_type, data| {
            SafetyThreshold::Unknown {
                data,
                threshold_type,
            }
        }),
    ]
}

pub(super) fn arb_safety_method() -> impl Strategy<Value = SafetyMethod> {
    prop_oneof![
        Just(SafetyMethod::Severity),
        Just(SafetyMethod::Probability),
        unknown_or(SafetyMethod::Severity, |method_type, data| {
            SafetyMethod::Unknown { data, method_type }
        }),
    ]
}

pub(crate) fn arb_safety_setting() -> impl Strategy<Value = SafetySetting> {
    (
        arb_harm_category(),
        arb_safety_threshold(),
        proptest::option::of(arb_safety_method()),
    )
        .prop_map(|(category, threshold, method)| SafetySetting {
            category,
            threshold,
            method,
            extra: serde_json::Map::new(),
        })
}

pub(super) fn arb_response_delivery() -> impl Strategy<Value = ResponseDelivery> {
    prop_oneof![
        Just(ResponseDelivery::Inline),
        Just(ResponseDelivery::Uri),
        unknown_or(ResponseDelivery::Inline, |delivery_type, data| {
            ResponseDelivery::Unknown {
                data,
                delivery_type,
            }
        }),
    ]
}

pub(crate) fn arb_response_format() -> impl Strategy<Value = ResponseFormat> {
    prop_oneof![
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_unknown_object()),
        )
            .prop_map(|(mime_type, schema)| ResponseFormat::Text {
                mime_type,
                schema,
                extra: Default::default()
            }),
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_response_delivery()),
            proptest::option::of(8000..48000i32),
            proptest::option::of(32000..320_000i32),
        )
            .prop_map(|(mime_type, delivery, sample_rate, bit_rate)| {
                ResponseFormat::Audio {
                    mime_type,
                    delivery,
                    sample_rate,
                    bit_rate,
                    extra: Default::default(),
                }
            }),
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_response_delivery()),
            proptest::option::of(arb_image_aspect_ratio()),
            proptest::option::of(arb_image_size()),
        )
            .prop_map(|(mime_type, delivery, aspect_ratio, image_size)| {
                ResponseFormat::Image {
                    mime_type,
                    delivery,
                    aspect_ratio,
                    image_size,
                    extra: Default::default(),
                }
            }),
        (
            proptest::option::of(arb_response_delivery()),
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_image_aspect_ratio()),
            proptest::option::of(arb_identifier()),
        )
            .prop_map(|(delivery, gcs_uri, aspect_ratio, duration)| {
                ResponseFormat::Video {
                    delivery,
                    gcs_uri,
                    aspect_ratio,
                    duration,
                    resolution: None,
                    extra: Default::default(),
                }
            }),
    ]
}

pub(crate) fn arb_response_format_spec() -> impl Strategy<Value = ResponseFormatSpec> {
    prop_oneof![
        arb_response_format().prop_map(ResponseFormatSpec::Single),
        proptest::collection::vec(arb_response_format(), 0..3).prop_map(ResponseFormatSpec::List),
    ]
}

pub(super) fn arb_visualization() -> impl Strategy<Value = Visualization> {
    prop_oneof![
        Just(Visualization::Off),
        Just(Visualization::Auto),
        unknown_or(Visualization::Off, |visualization_type, data| {
            Visualization::Unknown {
                data,
                visualization_type,
            }
        }),
    ]
}

proptest! {
    /// Test that ThinkingLevel roundtrips correctly through JSON.
    #[test]
    fn thinking_level_roundtrip(level in arb_thinking_level()) {
        let json = serde_json::to_string(&level).expect("Serialization should succeed");
        let restored: ThinkingLevel = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(level, restored);
    }

    /// Test that ThinkingSummaries roundtrips correctly through JSON.
    #[test]
    fn thinking_summaries_roundtrip(summaries in arb_thinking_summaries()) {
        let json = serde_json::to_string(&summaries).expect("Serialization should succeed");
        let restored: ThinkingSummaries = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(summaries, restored);
    }

    /// Test that ServiceTier roundtrips correctly through JSON.
    #[test]
    fn service_tier_roundtrip(tier in arb_service_tier()) {
        let json = serde_json::to_string(&tier).expect("Serialization should succeed");
        let restored: ServiceTier = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(tier, restored);
    }

    /// Test that ImageAspectRatio roundtrips correctly through JSON.
    #[test]
    fn image_aspect_ratio_roundtrip(ratio in arb_image_aspect_ratio()) {
        let json = serde_json::to_string(&ratio).expect("Serialization should succeed");
        let restored: ImageAspectRatio = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(ratio, restored);
    }

    /// Test that ImageSize roundtrips correctly through JSON.
    #[test]
    fn image_size_roundtrip(size in arb_image_size()) {
        let json = serde_json::to_string(&size).expect("Serialization should succeed");
        let restored: ImageSize = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(size, restored);
    }

    /// Test that ImageConfig roundtrips correctly through JSON.
    #[test]
    fn image_config_roundtrip(config in arb_image_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: ImageConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(config, restored);
    }

    /// Test that TurnContent roundtrips through JSON: stably, and to the
    /// value it was written from.
    #[test]
    fn turn_content_roundtrip(content in arb_turn_content()) {
        assert_value_roundtrip(&content)?;
        assert_first_write_keeps(&content)?;
    }

    /// Test that AgentConfig roundtrips correctly through JSON.
    #[test]
    fn agent_config_roundtrip(config in arb_agent_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: AgentConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(config, restored);
    }

    /// Test that GenerationConfig roundtrips through JSON: stably, and to
    /// the value it was written from (its non-string unknowns included).
    #[test]
    fn generation_config_roundtrip(config in arb_generation_config()) {
        assert_value_roundtrip(&config)?;
        let json = serde_json::to_value(&config).expect("Serialization should succeed");
        let restored: GenerationConfig =
            serde_json::from_value(json.clone()).expect("Deserialization should succeed");
        // An unknown `tool_choice` reads back typed by its shape, which the
        // strategy does not mirror; its data must match.
        let mut expected = config;
        if let (Some(ToolChoice::Unknown { data: sent, .. }), Some(ToolChoice::Unknown { data: read, .. })) =
            (&expected.tool_choice, &restored.tool_choice)
        {
            prop_assert_eq!(sent, read);
            expected.tool_choice.clone_from(&restored.tool_choice);
        }
        prop_assert_eq!(expected, restored, "written as {}", json);
    }

    /// A `speech_config` read in its `{"speakers": [...]}` form with keys
    /// beside the list is written back in that form, with them (D-001).
    #[test]
    fn generation_config_speakers_form_roundtrip(
        mut config in arb_generation_config(),
        speakers in proptest::collection::vec(arb_speech_config(), 0..3),
        beside in arb_unknown_object(),
    ) {
        config.speech_config = Some(speakers);
        config.speech_config_extra = beside
            .as_object()
            .expect("an object")
            .iter()
            .filter(|(key, _)| key.as_str() != "speakers")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let json = serde_json::to_value(&config).expect("Serialization should succeed");
        let restored: GenerationConfig =
            serde_json::from_value(json.clone()).expect("Deserialization should succeed");
        prop_assert_eq!(&restored.speech_config_extra, &config.speech_config_extra);
        prop_assert_eq!(&restored.speech_config, &config.speech_config);
        prop_assert_eq!(serde_json::to_value(&restored).expect("re-serializes"), json);
    }

    /// Test that VideoTask roundtrips correctly through JSON.
    #[test]
    fn video_task_roundtrip(task in arb_video_task()) {
        let json = serde_json::to_string(&task).expect("Serialization should succeed");
        let restored: VideoTask = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(task, restored);
    }

    /// Test that VideoConfig roundtrips correctly through JSON.
    #[test]
    fn video_config_roundtrip(config in arb_video_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: VideoConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(config, restored);
    }

    /// Test that Visualization roundtrips correctly through JSON.
    #[test]
    fn visualization_roundtrip(visualization in arb_visualization()) {
        let json = serde_json::to_string(&visualization).expect("Serialization should succeed");
        let restored: Visualization = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(visualization, restored);
    }

    /// Test that SafetySetting (and its three hand-serialized enums)
    /// roundtrips correctly through JSON, including Unknown arms.
    #[test]
    fn safety_setting_roundtrip(setting in arb_safety_setting()) {
        let json = serde_json::to_string(&setting).expect("Serialization should succeed");
        let restored: SafetySetting = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(setting, restored);
    }

    /// Test that ResponseDelivery roundtrips correctly through JSON.
    #[test]
    fn response_delivery_roundtrip(delivery in arb_response_delivery()) {
        let json = serde_json::to_string(&delivery).expect("Serialization should succeed");
        let restored: ResponseDelivery = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(delivery, restored);
    }

    /// Test that ResponseFormat roundtrips correctly through JSON.
    #[test]
    fn response_format_roundtrip(format in arb_response_format()) {
        let json = serde_json::to_string(&format).expect("Serialization should succeed");
        let restored: ResponseFormat = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(format, restored);
    }

    /// Test that ResponseFormatSpec (single vs list) roundtrips correctly.
    #[test]
    fn response_format_spec_roundtrip(spec in arb_response_format_spec()) {
        let json = serde_json::to_string(&spec).expect("Serialization should succeed");
        let restored: ResponseFormatSpec = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(spec, restored);
    }

    /// Test that InteractionInput roundtrips through JSON: stably, and to
    /// the value it was written from where it reads back as the same arm.
    #[test]
    fn interaction_input_roundtrip(input in arb_interaction_input()) {
        assert_value_roundtrip(&input)?;
        assert_input_first_write_keeps(&input)?;

        // Text input must stay Text (arrays/objects never look like strings)
        if matches!(input, InteractionInput::Text(_)) {
            let json = serde_json::to_value(&input).unwrap();
            let restored: InteractionInput = serde_json::from_value(json).unwrap();
            prop_assert!(matches!(restored, InteractionInput::Text(_)));
        }
    }

    /// Test an empty content array deserializes as Steps (documented gotcha:
    /// `InteractionInput::Content(vec![])` serializes to `[]`, which
    /// canonically deserializes as `Steps(vec![])`).
    ///
    /// This is the *type's own* serialization, which stays faithful to the
    /// variant — the `user_input` wrap is scoped to
    /// `InteractionRequest::input` (#427), so it does not apply here.
    #[test]
    fn empty_input_array_deserializes_as_steps(_unused in Just(())) {
        let input = InteractionInput::Content(vec![]);
        let json = serde_json::to_value(&input).expect("Serialization should succeed");
        prop_assert_eq!(&json, &serde_json::json!([]));

        let restored: InteractionInput =
            serde_json::from_value(json).expect("Deserialization should succeed");
        prop_assert_eq!(restored, InteractionInput::Steps(vec![]));
    }

    /// Test that a request wraps `Content` input, whatever the content is.
    ///
    /// The wrap is unconditional, so the wire shape never depends on how much
    /// content the caller supplied — and it is wire-identical to a hand-built
    /// `Step::user_input`, which is what lets it deserialize back cleanly.
    #[test]
    fn request_wraps_content_input(contents in prop::collection::vec(arb_content(), 0..3)) {
        let request = crate::InteractionRequest {
            model: Some("test-model".to_string()),
            input: Some(InteractionInput::Content(contents.clone())),
            ..Default::default()
        };
        let json = serde_json::to_value(&request).expect("Serialization should succeed");
        let input_json = &json["input"];

        prop_assert!(input_json.is_array());
        prop_assert_eq!(input_json.as_array().map(Vec::len), Some(1));
        prop_assert_eq!(&input_json[0]["type"], &serde_json::json!("user_input"));
        prop_assert_eq!(
            input_json[0]["content"].as_array().map(Vec::len),
            Some(contents.len())
        );

        let by_hand = crate::InteractionRequest {
            model: Some("test-model".to_string()),
            input: Some(InteractionInput::Steps(vec![Step::user_input(contents)])),
            ..Default::default()
        };
        let by_hand_json = serde_json::to_value(&by_hand).expect("Serialization should succeed");
        prop_assert_eq!(input_json, &by_hand_json["input"]);
    }
}

// =============================================================================
// Recorded regressions
// =============================================================================
//
// A seed in `proptest-regressions/proptest_request.txt` replays whatever
// value the strategy now draws from it, which drifts as the strategies
// change. The value each seed shrank to when it was recorded is pinned here,
// so the regression stays covered whatever the strategy generates.

/// Seed `49fdbe18` (`image_aspect_ratio_roundtrip`): an unknown ratio whose
/// data is its own type string.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_unknown_image_aspect_ratio_roundtrips() {
    let ratio = ImageAspectRatio::Unknown {
        ratio_type: "zz__".to_string(),
        data: serde_json::Value::String("zz__".to_string()),
    };
    let json = serde_json::to_string(&ratio).expect("Serialization should succeed");
    let restored: ImageAspectRatio =
        serde_json::from_str(&json).expect("Deserialization should succeed");
    assert_eq!(ratio, restored);
}
