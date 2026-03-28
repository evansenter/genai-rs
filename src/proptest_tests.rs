//! Property-based tests for serialization roundtrips using proptest.
//!
//! These tests verify that `deserialize(serialize(x)) == x` for all key types,
//! catching edge cases that hand-written tests might miss.

use chrono::{DateTime, TimeZone, Utc};
use proptest::prelude::*;

use super::content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, Place, Resolution, UrlContextResultItem,
};
use super::request::{
    AgentConfig, DeepResearchConfig, DynamicConfig, GenerationConfig, ImageAspectRatio,
    ImageConfig, ImageSize, Role, SpeechConfig, ThinkingLevel, ThinkingSummaries, Turn,
    TurnContent,
};
use super::response::{
    GroundingChunk, GroundingMetadata, InteractionResponse, InteractionStatus, ModalityTokens,
    OwnedFunctionCallInfo, UrlContextMetadata, UrlMetadataEntry, UrlRetrievalStatus, UsageMetadata,
    WebSource,
};
use super::tools::{FunctionCallingMode, FunctionParameters, SearchType, Tool};
use super::wire_streaming::StreamChunk;

// =============================================================================
// Strategy Generators for Arbitrary Types
// =============================================================================

/// Strategy for generating "clean" floating point numbers that roundtrip reliably.
/// Uses integer-based construction to avoid precision issues.
fn arb_clean_float() -> impl Strategy<Value = serde_json::Value> {
    // Generate floats from integer components to ensure clean roundtrip
    // e.g., 123 / 100 = 1.23, -456 / 1000 = -0.456
    (
        any::<i32>(),
        prop_oneof![Just(1i64), Just(10), Just(100), Just(1000)],
    )
        .prop_filter_map("must be representable", |(n, divisor)| {
            let f = (n as f64) / (divisor as f64);
            serde_json::Number::from_f64(f).map(serde_json::Value::Number)
        })
}

/// Strategy for generating arbitrary serde_json::Value for function args/results.
/// Limited in depth to avoid overly complex nested structures.
fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
    // Simple JSON values for function args/results
    prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::Value::Number(n.into())),
        // Float numbers using clean construction for reliable roundtrip
        arb_clean_float(),
        ".*".prop_map(serde_json::Value::String),
        // Simple arrays
        prop::collection::vec(
            prop_oneof![
                Just(serde_json::Value::Null),
                any::<bool>().prop_map(serde_json::Value::Bool),
                any::<i64>().prop_map(|n| serde_json::Value::Number(n.into())),
                ".*".prop_map(serde_json::Value::String),
            ],
            0..5
        )
        .prop_map(serde_json::Value::Array),
        // Simple objects
        prop::collection::hash_map("[a-zA-Z_][a-zA-Z0-9_]*", ".*", 0..5).prop_map(|m| {
            serde_json::Value::Object(
                m.into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            )
        }),
    ]
}

/// Strategy for generating valid identifiers (function names, IDs, etc.)
fn arb_identifier() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,30}"
}

/// Strategy for generating text strings up to 500 characters (may be empty).
fn arb_text() -> impl Strategy<Value = String> {
    ".{0,500}"
}

/// Strategy for generating DateTime<Utc> values.
/// Uses second precision to ensure reliable roundtrip (avoiding nanosecond precision issues).
fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
    // Generate timestamps between 2020-01-01 and 2030-01-01 (reasonable range)
    (0i64..315_360_000).prop_map(|offset_secs| {
        // Base: 2020-01-01 00:00:00 UTC (timestamp 1577836800)
        Utc.timestamp_opt(1_577_836_800 + offset_secs, 0)
            .single()
            .expect("valid timestamp")
    })
}

/// Strategy for generating arbitrary Resolution values
fn arb_resolution() -> impl Strategy<Value = Resolution> {
    prop_oneof![
        Just(Resolution::Low),
        Just(Resolution::Medium),
        Just(Resolution::High),
        Just(Resolution::UltraHigh),
        // Unknown variant with arbitrary string value
        "[a-z_]{1,20}".prop_map(|s| Resolution::Unknown {
            resolution_type: s.clone(),
            data: serde_json::Value::String(s),
        }),
    ]
}

// =============================================================================
// Annotation Strategies
// =============================================================================

/// Strategy for generating Annotation objects for text content.
fn arb_annotation() -> impl Strategy<Value = Annotation> {
    (0usize..1000, 0usize..1000, proptest::option::of(".{0,100}")).prop_map(
        |(start, len, source)| Annotation {
            start_index: start,
            end_index: start.saturating_add(len),
            source,
        },
    )
}

// =============================================================================
// GoogleSearchResultItem Strategies
// =============================================================================

/// Strategy for generating GoogleSearchResultItem objects.
fn arb_google_search_result_item() -> impl Strategy<Value = GoogleSearchResultItem> {
    (arb_text(), arb_text(), proptest::option::of(arb_text())).prop_map(
        |(title, url, rendered_content)| GoogleSearchResultItem {
            title,
            url,
            rendered_content,
        },
    )
}

/// Strategy for generating Place objects.
/// Uses only simple fields to avoid f64 NaN comparison issues.
fn arb_place() -> impl Strategy<Value = Place> {
    (
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
    )
        .prop_map(|(name, formatted_address, place_id)| Place {
            name,
            formatted_address,
            place_id,
            lat: None,
            lng: None,
            types: None,
            rating: None,
            user_ratings_total: None,
            website: None,
            phone_number: None,
            extra: serde_json::Map::new(),
        })
}

/// Strategy for generating GoogleMapsResultItem objects.
fn arb_google_maps_result_item() -> impl Strategy<Value = GoogleMapsResultItem> {
    (
        proptest::option::of(proptest::collection::vec(arb_place(), 0..3)),
        proptest::option::of(arb_text()),
    )
        .prop_map(|(places, widget_context_token)| GoogleMapsResultItem {
            places,
            widget_context_token,
        })
}

/// Strategy for generating FileSearchResultItem objects.
fn arb_file_search_result_item() -> impl Strategy<Value = FileSearchResultItem> {
    (arb_text(), arb_text(), arb_text()).prop_map(|(title, text, store)| FileSearchResultItem {
        title,
        text,
        store,
    })
}

/// Strategy for generating UrlContextResultItem objects.
fn arb_url_context_result_item() -> impl Strategy<Value = UrlContextResultItem> {
    (
        arb_text(),
        prop_oneof![Just("success"), Just("error"), Just("unsafe")],
    )
        .prop_map(|(url, status)| UrlContextResultItem::new(url, status))
}

// =============================================================================
// InteractionStatus Strategies
// =============================================================================

/// Strategy for known InteractionStatus variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_interaction_status() -> impl Strategy<Value = InteractionStatus> {
    prop_oneof![
        Just(InteractionStatus::Completed),
        Just(InteractionStatus::InProgress),
        Just(InteractionStatus::RequiresAction),
        Just(InteractionStatus::Failed),
        Just(InteractionStatus::Cancelled),
        Just(InteractionStatus::Incomplete),
    ]
}

/// Strategy for all InteractionStatus variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_interaction_status() -> impl Strategy<Value = InteractionStatus> {
    prop_oneof![
        Just(InteractionStatus::Completed),
        Just(InteractionStatus::InProgress),
        Just(InteractionStatus::RequiresAction),
        Just(InteractionStatus::Failed),
        Just(InteractionStatus::Cancelled),
        Just(InteractionStatus::Incomplete),
        // Unknown variant with preserved data
        arb_identifier().prop_map(|status_type| InteractionStatus::Unknown {
            status_type: status_type.clone(),
            data: serde_json::Value::String(status_type),
        }),
    ]
}

// =============================================================================
// FunctionCallingMode Strategies
// =============================================================================

/// Strategy for known FunctionCallingMode variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_function_calling_mode() -> impl Strategy<Value = FunctionCallingMode> {
    prop_oneof![
        Just(FunctionCallingMode::Auto),
        Just(FunctionCallingMode::Any),
        Just(FunctionCallingMode::None),
        Just(FunctionCallingMode::Validated),
    ]
}

/// Strategy for all FunctionCallingMode variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_function_calling_mode() -> impl Strategy<Value = FunctionCallingMode> {
    prop_oneof![
        Just(FunctionCallingMode::Auto),
        Just(FunctionCallingMode::Any),
        Just(FunctionCallingMode::None),
        Just(FunctionCallingMode::Validated),
        // Unknown variant with preserved data
        arb_identifier().prop_map(|mode_type| FunctionCallingMode::Unknown {
            mode_type: mode_type.clone(),
            data: serde_json::Value::String(mode_type),
        }),
    ]
}

// =============================================================================
// ThinkingLevel Strategies
// =============================================================================

/// Strategy for known ThinkingLevel variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_thinking_level() -> impl Strategy<Value = ThinkingLevel> {
    prop_oneof![
        Just(ThinkingLevel::Minimal),
        Just(ThinkingLevel::Low),
        Just(ThinkingLevel::Medium),
        Just(ThinkingLevel::High),
    ]
}

/// Strategy for all ThinkingLevel variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_thinking_level() -> impl Strategy<Value = ThinkingLevel> {
    prop_oneof![
        Just(ThinkingLevel::Minimal),
        Just(ThinkingLevel::Low),
        Just(ThinkingLevel::Medium),
        Just(ThinkingLevel::High),
        // Unknown variant with preserved data
        arb_identifier().prop_map(|level_type| ThinkingLevel::Unknown {
            level_type: level_type.clone(),
            data: serde_json::Value::String(level_type),
        }),
    ]
}

// =============================================================================
// ImageAspectRatio Strategies
// =============================================================================

/// Strategy for known ImageAspectRatio variants only.
#[cfg(feature = "strict-unknown")]
fn arb_image_aspect_ratio() -> impl Strategy<Value = ImageAspectRatio> {
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
    ]
}

/// Strategy for all ImageAspectRatio variants including Unknown.
#[cfg(not(feature = "strict-unknown"))]
fn arb_image_aspect_ratio() -> impl Strategy<Value = ImageAspectRatio> {
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
        "[a-z0-9:]{1,10}".prop_map(|ratio_type| ImageAspectRatio::Unknown {
            ratio_type: ratio_type.clone(),
            data: serde_json::Value::String(ratio_type),
        }),
    ]
}

// =============================================================================
// ImageSize Strategies
// =============================================================================

/// Strategy for known ImageSize variants only.
#[cfg(feature = "strict-unknown")]
fn arb_image_size() -> impl Strategy<Value = ImageSize> {
    prop_oneof![
        Just(ImageSize::Sd512),
        Just(ImageSize::Hd1k),
        Just(ImageSize::Hd2k),
        Just(ImageSize::Uhd4k),
    ]
}

/// Strategy for all ImageSize variants including Unknown.
#[cfg(not(feature = "strict-unknown"))]
fn arb_image_size() -> impl Strategy<Value = ImageSize> {
    prop_oneof![
        Just(ImageSize::Sd512),
        Just(ImageSize::Hd1k),
        Just(ImageSize::Hd2k),
        Just(ImageSize::Uhd4k),
        // Unknown variant with preserved data
        "[a-zA-Z0-9]{1,10}".prop_map(|size_type| ImageSize::Unknown {
            size_type: size_type.clone(),
            data: serde_json::Value::String(size_type),
        }),
    ]
}

// =============================================================================
// ImageConfig Strategies
// =============================================================================

/// Strategy for generating arbitrary ImageConfig values.
fn arb_image_config() -> impl Strategy<Value = ImageConfig> {
    (
        proptest::option::of(arb_image_aspect_ratio()),
        proptest::option::of(arb_image_size()),
    )
        .prop_map(|(aspect_ratio, image_size)| ImageConfig {
            aspect_ratio,
            image_size,
        })
}

// =============================================================================
// Role Strategies
// =============================================================================

/// Strategy for known Role variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_role() -> impl Strategy<Value = Role> {
    prop_oneof![Just(Role::User), Just(Role::Model),]
}

/// Strategy for all Role variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_role() -> impl Strategy<Value = Role> {
    prop_oneof![
        Just(Role::User),
        Just(Role::Model),
        // Unknown variant with preserved data (role_type and data fields per Evergreen pattern)
        arb_identifier().prop_map(|role_type| Role::Unknown {
            data: serde_json::Value::String(role_type.clone()),
            role_type,
        }),
    ]
}

// =============================================================================
// TurnContent Strategies
// =============================================================================

/// Strategy for TurnContent.
/// Generates either text or parts content.
fn arb_turn_content() -> impl Strategy<Value = TurnContent> {
    prop_oneof![
        // Text content
        arb_text().prop_map(TurnContent::Text),
        // Parts content with interaction content
        prop::collection::vec(arb_interaction_content(), 0..3).prop_map(TurnContent::Parts),
    ]
}

// =============================================================================
// Turn Strategies
// =============================================================================

/// Strategy for Turn.
/// Generates a turn with a role and content.
fn arb_turn() -> impl Strategy<Value = Turn> {
    (arb_role(), arb_turn_content()).prop_map(|(role, content)| Turn::new(role, content))
}

// =============================================================================
// ThinkingSummaries Strategies
// =============================================================================

/// Strategy for known ThinkingSummaries variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_thinking_summaries() -> impl Strategy<Value = ThinkingSummaries> {
    prop_oneof![Just(ThinkingSummaries::Auto), Just(ThinkingSummaries::None),]
}

/// Strategy for all ThinkingSummaries variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_thinking_summaries() -> impl Strategy<Value = ThinkingSummaries> {
    prop_oneof![
        Just(ThinkingSummaries::Auto),
        Just(ThinkingSummaries::None),
        // Unknown variant with preserved data
        arb_identifier().prop_map(|summaries_type| ThinkingSummaries::Unknown {
            summaries_type: summaries_type.clone(),
            data: serde_json::Value::String(summaries_type),
        }),
    ]
}

// =============================================================================
// GenerationConfig Strategies
// =============================================================================

fn arb_speech_config() -> impl Strategy<Value = SpeechConfig> {
    (
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
    )
        .prop_map(|(voice, language, speaker)| SpeechConfig {
            voice,
            language,
            speaker,
        })
}

fn arb_generation_config() -> impl Strategy<Value = GenerationConfig> {
    (
        proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32)),
        proptest::option::of(1..10000i32),
        proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32)),
        proptest::option::of(1..100i32),
        proptest::option::of(arb_thinking_level()),
        proptest::option::of(1..1000i64),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(arb_thinking_summaries()),
        proptest::option::of(arb_function_calling_mode()),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
        proptest::option::of(arb_speech_config()),
    )
        .prop_map(
            |(
                temperature,
                max_output_tokens,
                top_p,
                top_k,
                thinking_level,
                seed,
                stop_sequences,
                thinking_summaries,
                tool_choice,
                allowed_tools,
                speech_config,
            )| {
                GenerationConfig {
                    temperature,
                    max_output_tokens,
                    top_p,
                    top_k,
                    thinking_level,
                    seed,
                    stop_sequences,
                    thinking_summaries,
                    tool_choice,
                    allowed_tools,
                    image_config: None,
                    speech_config,
                }
            },
        )
}

// =============================================================================
// AgentConfig Strategies
// =============================================================================

/// Strategy for AgentConfig using typed config structs.
/// Since AgentConfig is now a thin wrapper around serde_json::Value,
/// we generate configs via the typed structs (DeepResearchConfig, DynamicConfig)
/// and the raw from_value() method for arbitrary configs.
fn arb_agent_config() -> impl Strategy<Value = AgentConfig> {
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

// =============================================================================
// CodeExecutionLanguage Strategies
// =============================================================================

/// Strategy for known CodeExecutionLanguage variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_code_execution_language() -> impl Strategy<Value = CodeExecutionLanguage> {
    Just(CodeExecutionLanguage::Python)
}

/// Strategy for all CodeExecutionLanguage variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_code_execution_language() -> impl Strategy<Value = CodeExecutionLanguage> {
    prop_oneof![
        Just(CodeExecutionLanguage::Python),
        // Unknown variant with preserved data
        arb_identifier().prop_map(|language_type| CodeExecutionLanguage::Unknown {
            language_type: language_type.clone(),
            data: serde_json::Value::String(language_type),
        }),
    ]
}

// =============================================================================
// ModalityTokens Strategy
// =============================================================================

/// Strategy for generating a single ModalityTokens value.
fn arb_modality_tokens() -> impl Strategy<Value = ModalityTokens> {
    // Use realistic modality names that the API might return
    (
        prop_oneof![
            Just("TEXT".to_string()),
            Just("IMAGE".to_string()),
            Just("AUDIO".to_string()),
            Just("VIDEO".to_string()),
            arb_identifier(), // For forward compatibility with unknown modalities
        ],
        any::<u32>(),
    )
        .prop_map(|(modality, tokens)| ModalityTokens { modality, tokens })
}

/// Strategy for generating an optional Vec of ModalityTokens.
fn arb_modality_tokens_vec() -> impl Strategy<Value = Option<Vec<ModalityTokens>>> {
    proptest::option::of(prop::collection::vec(arb_modality_tokens(), 0..4))
}

// =============================================================================
// UsageMetadata Strategy
// =============================================================================

fn arb_usage_metadata() -> impl Strategy<Value = UsageMetadata> {
    (
        proptest::option::of(any::<u32>()),
        proptest::option::of(any::<u32>()),
        proptest::option::of(any::<u32>()),
        proptest::option::of(any::<u32>()),
        proptest::option::of(any::<u32>()),
        proptest::option::of(any::<u32>()),
        proptest::option::of(any::<u32>()),
        arb_modality_tokens_vec(),
        arb_modality_tokens_vec(),
        arb_modality_tokens_vec(),
        arb_modality_tokens_vec(),
    )
        .prop_map(
            |(
                total_input_tokens,
                total_output_tokens,
                total_tokens,
                total_cached_tokens,
                total_reasoning_tokens,
                total_thought_tokens,
                total_tool_use_tokens,
                input_tokens_by_modality,
                output_tokens_by_modality,
                cached_tokens_by_modality,
                tool_use_tokens_by_modality,
            )| {
                UsageMetadata {
                    total_input_tokens,
                    total_output_tokens,
                    total_tokens,
                    total_cached_tokens,
                    total_reasoning_tokens,
                    total_thought_tokens,
                    total_tool_use_tokens,
                    input_tokens_by_modality,
                    output_tokens_by_modality,
                    cached_tokens_by_modality,
                    tool_use_tokens_by_modality,
                }
            },
        )
}

// =============================================================================
// OwnedFunctionCallInfo Strategy
// =============================================================================

fn arb_owned_function_call_info() -> impl Strategy<Value = OwnedFunctionCallInfo> {
    (
        proptest::option::of(arb_identifier()),
        arb_identifier(),
        arb_json_value(),
    )
        .prop_map(|(id, name, args)| OwnedFunctionCallInfo { id, name, args })
}

// =============================================================================
// Content Strategy
// =============================================================================

/// Helper to create the known Content variants (used by both strict and non-strict modes).
fn arb_known_interaction_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        // Text content (with optional annotations)
        (
            proptest::option::of(arb_text()),
            proptest::option::of(proptest::collection::vec(arb_annotation(), 0..3))
        )
            .prop_map(|(text, annotations)| Content::Text { text, annotations }),
        // Thought content (contains signature, not text)
        proptest::option::of(arb_text()).prop_map(|signature| Content::Thought { signature }),
        // ThoughtSignature content
        arb_text().prop_map(|signature| Content::ThoughtSignature { signature }),
        // Image content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_resolution())
        )
            .prop_map(|(data, uri, mime_type, resolution)| Content::Image {
                data,
                uri,
                mime_type,
                resolution
            }),
        // Audio content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text())
        )
            .prop_map(|(data, uri, mime_type)| Content::Audio {
                data,
                uri,
                mime_type
            }),
        // Video content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_resolution())
        )
            .prop_map(|(data, uri, mime_type, resolution)| Content::Video {
                data,
                uri,
                mime_type,
                resolution
            }),
        // Document content
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text())
        )
            .prop_map(|(data, uri, mime_type)| Content::Document {
                data,
                uri,
                mime_type
            }),
        // FunctionCall content
        (
            proptest::option::of(arb_identifier()),
            arb_identifier(),
            arb_json_value(),
        )
            .prop_map(|(id, name, args)| { Content::FunctionCall { id, name, args } }),
        // FunctionResult content
        (
            proptest::option::of(arb_identifier()),
            arb_identifier(),
            arb_json_value(),
            proptest::option::of(proptest::bool::ANY),
        )
            .prop_map(|(name, call_id, result, is_error)| {
                Content::FunctionResult {
                    name,
                    call_id,
                    result,
                    is_error,
                }
            }),
        // CodeExecutionCall content
        (
            proptest::option::of(arb_identifier()),
            arb_code_execution_language(),
            arb_text(),
        )
            .prop_map(|(id, language, code)| Content::CodeExecutionCall {
                id,
                language,
                code
            }),
        // CodeExecutionResult content
        (
            proptest::option::of(arb_identifier()),
            any::<bool>(),
            arb_text(),
        )
            .prop_map(|(call_id, is_error, result)| {
                Content::CodeExecutionResult {
                    call_id,
                    is_error,
                    result,
                }
            }),
        // GoogleSearchCall content
        (arb_text(), proptest::collection::vec(arb_text(), 0..3))
            .prop_map(|(id, queries)| Content::GoogleSearchCall { id, queries }),
        // GoogleMapsCall content
        (
            arb_text(),
            proptest::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text())
        )
            .prop_map(|(id, queries, signature)| Content::GoogleMapsCall {
                id,
                queries,
                signature
            }),
        // GoogleSearchResult content
        (
            arb_text(),
            proptest::collection::vec(arb_google_search_result_item(), 0..3)
        )
            .prop_map(|(call_id, result)| Content::GoogleSearchResult { call_id, result }),
        // UrlContextCall content
        (arb_text(), proptest::collection::vec(arb_text(), 1..3))
            .prop_map(|(id, urls)| Content::UrlContextCall { id, urls }),
        // UrlContextResult content
        (
            arb_text(),
            proptest::collection::vec(arb_url_context_result_item(), 0..3)
        )
            .prop_map(|(call_id, result)| Content::UrlContextResult { call_id, result }),
        // FileSearchResult content
        (
            arb_text(),
            proptest::collection::vec(arb_file_search_result_item(), 0..3)
        )
            .prop_map(|(call_id, result)| Content::FileSearchResult { call_id, result }),
        // GoogleMapsResult content
        (
            arb_text(),
            proptest::collection::vec(arb_google_maps_result_item(), 0..3),
            proptest::option::of(arb_text())
        )
            .prop_map(|(call_id, result, signature)| Content::GoogleMapsResult {
                call_id,
                result,
                signature
            }),
    ]
}

/// Strategy for known Content variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_interaction_content() -> impl Strategy<Value = Content> {
    arb_known_interaction_content()
}

/// Strategy for all Content variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_interaction_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        arb_known_interaction_content(),
        // Unknown content (for forward compatibility testing)
        (arb_identifier(), arb_json_value())
            .prop_map(|(content_type, data)| { Content::Unknown { content_type, data } }),
    ]
}

// =============================================================================
// Metadata Strategies
// =============================================================================

fn arb_web_source() -> impl Strategy<Value = WebSource> {
    (arb_text(), arb_text(), arb_text()).prop_map(|(uri, title, domain)| WebSource {
        uri,
        title,
        domain,
    })
}

fn arb_grounding_chunk() -> impl Strategy<Value = GroundingChunk> {
    arb_web_source().prop_map(|web| GroundingChunk { web })
}

fn arb_grounding_metadata() -> impl Strategy<Value = GroundingMetadata> {
    (
        prop::collection::vec(arb_text(), 0..5),
        prop::collection::vec(arb_grounding_chunk(), 0..5),
    )
        .prop_map(|(web_search_queries, grounding_chunks)| GroundingMetadata {
            web_search_queries,
            grounding_chunks,
        })
}

/// Strategy for known UrlRetrievalStatus variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_url_retrieval_status() -> impl Strategy<Value = UrlRetrievalStatus> {
    prop_oneof![
        Just(UrlRetrievalStatus::Unspecified),
        Just(UrlRetrievalStatus::Success),
        Just(UrlRetrievalStatus::Unsafe),
        Just(UrlRetrievalStatus::Error),
    ]
}

/// Strategy for all UrlRetrievalStatus variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_url_retrieval_status() -> impl Strategy<Value = UrlRetrievalStatus> {
    prop_oneof![
        Just(UrlRetrievalStatus::Unspecified),
        Just(UrlRetrievalStatus::Success),
        Just(UrlRetrievalStatus::Unsafe),
        Just(UrlRetrievalStatus::Error),
        // Unknown variant with preserved data
        arb_identifier().prop_map(|status_type| UrlRetrievalStatus::Unknown {
            status_type: status_type.clone(),
            data: serde_json::Value::String(status_type),
        }),
    ]
}

fn arb_url_metadata_entry() -> impl Strategy<Value = UrlMetadataEntry> {
    (arb_text(), arb_url_retrieval_status()).prop_map(|(retrieved_url, url_retrieval_status)| {
        UrlMetadataEntry {
            retrieved_url,
            url_retrieval_status,
        }
    })
}

fn arb_url_context_metadata() -> impl Strategy<Value = UrlContextMetadata> {
    prop::collection::vec(arb_url_metadata_entry(), 0..5)
        .prop_map(|url_metadata| UrlContextMetadata { url_metadata })
}

// =============================================================================
// Tool Strategy
// =============================================================================

fn arb_function_parameters() -> impl Strategy<Value = FunctionParameters> {
    (
        Just("object".to_string()),
        arb_json_value(),
        prop::collection::vec(arb_identifier(), 0..3),
    )
        .prop_map(|(type_, properties, required)| {
            FunctionParameters::new(type_, properties, required)
        })
}

/// Helper to create the known Tool variants (used by both strict and non-strict modes).
fn arb_known_tool() -> impl Strategy<Value = Tool> {
    prop_oneof![
        // Function tool
        (arb_identifier(), arb_text(), arb_function_parameters()).prop_map(
            |(name, description, parameters)| Tool::Function {
                name,
                description,
                parameters
            }
        ),
        // Built-in tools
        proptest::option::of(proptest::collection::vec(
            prop_oneof![Just(SearchType::WebSearch), Just(SearchType::ImageSearch),],
            1..3,
        ))
        .prop_map(|search_types| Tool::GoogleSearch { search_types }),
        proptest::option::of(any::<bool>())
            .prop_map(|enable_widget| Tool::GoogleMaps { enable_widget }),
        Just(Tool::CodeExecution),
        Just(Tool::UrlContext),
        // FileSearch tool
        (
            proptest::collection::vec(arb_identifier(), 1..4),
            proptest::option::of(any::<i32>()),
            proptest::option::of(arb_text())
        )
            .prop_map(|(store_names, top_k, metadata_filter)| {
                Tool::FileSearch {
                    store_names,
                    top_k,
                    metadata_filter,
                }
            }),
        // MCP Server (use BTreeMap for deterministic JSON key ordering in roundtrip tests)
        (
            arb_identifier(),
            arb_text(),
            proptest::option::of(proptest::collection::vec(arb_identifier(), 1..4)),
            proptest::option::of(proptest::collection::btree_map(
                arb_identifier(),
                arb_text(),
                1..3
            )),
        )
            .prop_map(|(name, url, allowed_tools, headers)| Tool::McpServer {
                name,
                url,
                allowed_tools,
                headers: headers.map(|m| m.into_iter().collect()),
            }),
    ]
}

/// Strategy for known Tool variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_tool() -> impl Strategy<Value = Tool> {
    arb_known_tool()
}

/// Strategy for all Tool variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_tool() -> impl Strategy<Value = Tool> {
    prop_oneof![
        arb_known_tool(),
        // Unknown tool
        (arb_identifier(), arb_json_value())
            .prop_map(|(tool_type, data)| Tool::Unknown { tool_type, data }),
    ]
}

// =============================================================================
// InteractionResponse Strategy
// =============================================================================

fn arb_interaction_response() -> impl Strategy<Value = InteractionResponse> {
    // Split into two tuples to avoid proptest's 12-element limit
    let part1 = (
        proptest::option::of(arb_identifier()),                 // id
        proptest::option::of(arb_identifier()),                 // model
        proptest::option::of(arb_identifier()),                 // agent
        prop::collection::vec(arb_interaction_content(), 0..3), // input
        prop::collection::vec(arb_interaction_content(), 0..5), // outputs
        arb_interaction_status(),                               // status
        proptest::option::of(arb_usage_metadata()),             // usage
    );
    let part2 = (
        proptest::option::of(prop::collection::vec(arb_tool(), 0..3)), // tools
        proptest::option::of(arb_grounding_metadata()),                // grounding_metadata
        proptest::option::of(arb_url_context_metadata()),              // url_context_metadata
        proptest::option::of(arb_identifier()),                        // previous_interaction_id
        proptest::option::of(arb_datetime()),                          // created
        proptest::option::of(arb_datetime()),                          // updated
    );

    (part1, part2).prop_map(
        |(
            (id, model, agent, input, outputs, status, usage),
            (
                tools,
                grounding_metadata,
                url_context_metadata,
                previous_interaction_id,
                created,
                updated,
            ),
        )| {
            InteractionResponse {
                id,
                model,
                agent,
                input,
                outputs,
                status,
                usage,
                tools,
                grounding_metadata,
                url_context_metadata,
                previous_interaction_id,
                created,
                updated,
            }
        },
    )
}

// =============================================================================
// StreamChunk Strategy
// =============================================================================

/// Helper to create the known StreamChunk variants (used by both strict and non-strict modes).
fn arb_known_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    prop_oneof![
        // Start variant
        arb_interaction_response().prop_map(|interaction| StreamChunk::Start { interaction }),
        // StatusUpdate variant
        (arb_identifier(), arb_interaction_status()).prop_map(|(interaction_id, status)| {
            StreamChunk::StatusUpdate {
                interaction_id,
                status,
            }
        }),
        // ContentStart variant
        (any::<usize>(), proptest::option::of(arb_identifier())).prop_map(
            |(index, content_type)| StreamChunk::ContentStart {
                index,
                content_type,
            }
        ),
        // Delta variant
        arb_interaction_content().prop_map(StreamChunk::Delta),
        // ContentStop variant
        any::<usize>().prop_map(|index| StreamChunk::ContentStop { index }),
        // Complete variant
        arb_interaction_response().prop_map(StreamChunk::Complete),
        // Error variant
        (arb_text(), proptest::option::of(arb_identifier()))
            .prop_map(|(message, code)| { StreamChunk::Error { message, code } }),
    ]
}

/// Strategy for known StreamChunk variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    arb_known_stream_chunk()
}

/// Strategy for all StreamChunk variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    prop_oneof![
        arb_known_stream_chunk(),
        // Unknown variant for forward compatibility
        (arb_identifier(), arb_json_value())
            .prop_map(|(chunk_type, data)| StreamChunk::Unknown { chunk_type, data }),
    ]
}

// =============================================================================
// Property Tests
// =============================================================================

proptest! {
    /// Test that ModalityTokens roundtrips correctly through JSON.
    #[test]
    fn modality_tokens_roundtrip(tokens in arb_modality_tokens()) {
        let json = serde_json::to_string(&tokens).expect("Serialization should succeed");
        let restored: ModalityTokens = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(tokens, restored);
    }

    /// Test that UsageMetadata roundtrips correctly through JSON.
    #[test]
    fn usage_metadata_roundtrip(usage in arb_usage_metadata()) {
        let json = serde_json::to_string(&usage).expect("Serialization should succeed");
        let restored: UsageMetadata = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(usage, restored);
    }

    /// Test that OwnedFunctionCallInfo roundtrips correctly through JSON.
    #[test]
    fn owned_function_call_info_roundtrip(info in arb_owned_function_call_info()) {
        let json = serde_json::to_string(&info).expect("Serialization should succeed");
        let restored: OwnedFunctionCallInfo = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(info, restored);
    }

    /// Test that InteractionStatus roundtrips correctly through JSON.
    #[test]
    fn interaction_status_roundtrip(status in arb_interaction_status()) {
        let json = serde_json::to_string(&status).expect("Serialization should succeed");
        let restored: InteractionStatus = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(status, restored);
    }

    /// Test that CodeExecutionLanguage roundtrips correctly through JSON.
    #[test]
    fn code_execution_language_roundtrip(lang in arb_code_execution_language()) {
        let json = serde_json::to_string(&lang).expect("Serialization should succeed");
        let restored: CodeExecutionLanguage = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(lang, restored);
    }

    /// Test that FunctionCallingMode roundtrips correctly through JSON.
    #[test]
    fn function_calling_mode_roundtrip(mode in arb_function_calling_mode()) {
        let json = serde_json::to_string(&mode).expect("Serialization should succeed");
        let restored: FunctionCallingMode = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(mode, restored);
    }

    /// Test that ThinkingLevel roundtrips correctly through JSON.
    #[test]
    fn thinking_level_roundtrip(level in arb_thinking_level()) {
        let json = serde_json::to_string(&level).expect("Serialization should succeed");
        let restored: ThinkingLevel = serde_json::from_str(&json).expect("Deserialization should succeed");

        // ThinkingLevel doesn't derive PartialEq, so we verify roundtrip by comparing JSON strings
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
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

    /// Test that Role roundtrips correctly through JSON.
    #[test]
    fn role_roundtrip(role in arb_role()) {
        let json = serde_json::to_string(&role).expect("Serialization should succeed");
        let restored: Role = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(role, restored);
    }

    /// Test that TurnContent roundtrips correctly through JSON.
    ///
    /// Note: Uses JSON comparison since TurnContent can contain Content::Unknown
    /// which doesn't preserve exact data structure through roundtrip.
    #[test]
    fn turn_content_roundtrip(content in arb_turn_content()) {
        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: TurnContent = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Use JSON comparison since Content::Unknown doesn't roundtrip exactly
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that Turn roundtrips correctly through JSON.
    ///
    /// Note: Uses JSON comparison since Turn can contain Content::Unknown
    /// which doesn't preserve exact data structure through roundtrip.
    #[test]
    fn turn_roundtrip(turn in arb_turn()) {
        let json = serde_json::to_string(&turn).expect("Serialization should succeed");
        let restored: Turn = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Use JSON comparison since Content::Unknown doesn't roundtrip exactly
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that ThinkingSummaries roundtrips correctly through JSON.
    #[test]
    fn thinking_summaries_roundtrip(summaries in arb_thinking_summaries()) {
        let json = serde_json::to_string(&summaries).expect("Serialization should succeed");
        let restored: ThinkingSummaries = serde_json::from_str(&json).expect("Deserialization should succeed");

        // ThinkingSummaries doesn't derive PartialEq, so we verify roundtrip by comparing JSON strings
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that AgentConfig roundtrips correctly through JSON.
    #[test]
    fn agent_config_roundtrip(config in arb_agent_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: AgentConfig = serde_json::from_str(&json).expect("Deserialization should succeed");

        // AgentConfig derives PartialEq, so we can compare directly
        prop_assert_eq!(config, restored);
    }

    /// Test that UrlRetrievalStatus roundtrips correctly through JSON.
    #[test]
    fn url_retrieval_status_roundtrip(status in arb_url_retrieval_status()) {
        let json = serde_json::to_string(&status).expect("Serialization should succeed");
        let restored: UrlRetrievalStatus = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(status, restored);
    }

    /// Test that WebSource roundtrips correctly through JSON.
    #[test]
    fn web_source_roundtrip(source in arb_web_source()) {
        let json = serde_json::to_string(&source).expect("Serialization should succeed");
        let restored: WebSource = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(source, restored);
    }

    /// Test that GroundingMetadata roundtrips correctly through JSON.
    #[test]
    fn grounding_metadata_roundtrip(metadata in arb_grounding_metadata()) {
        let json = serde_json::to_string(&metadata).expect("Serialization should succeed");
        let restored: GroundingMetadata = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(metadata, restored);
    }

    /// Test that UrlContextMetadata roundtrips correctly through JSON.
    #[test]
    fn url_context_metadata_roundtrip(metadata in arb_url_context_metadata()) {
        let json = serde_json::to_string(&metadata).expect("Serialization should succeed");
        let restored: UrlContextMetadata = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(metadata, restored);
    }

    /// Test that Tool roundtrips correctly through JSON.
    #[test]
    fn tool_roundtrip(tool in arb_tool()) {
        let json = serde_json::to_string(&tool).expect("Serialization should succeed");
        let restored: Tool = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Tool doesn't derive PartialEq, so we verify roundtrip by comparing JSON values
        // (not strings, since HashMap key ordering is non-deterministic)
        let original_value: serde_json::Value = serde_json::from_str(&json).expect("Parse original JSON");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        let restored_value: serde_json::Value = serde_json::from_str(&restored_json).expect("Parse restored JSON");
        prop_assert_eq!(original_value, restored_value);
    }

    /// Test that Content roundtrips correctly through JSON.
    ///
    /// Note: This uses structural comparison since Content doesn't derive PartialEq.
    #[test]
    fn interaction_content_roundtrip(content in arb_interaction_content()) {
        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Compare by re-serializing since Content doesn't derive PartialEq
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that InteractionResponse roundtrips correctly through JSON.
    ///
    /// This is the most comprehensive test, covering the full response structure.
    #[test]
    fn interaction_response_roundtrip(response in arb_interaction_response()) {
        let json = serde_json::to_string(&response).expect("Serialization should succeed");
        let restored: InteractionResponse = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Compare key fields since InteractionResponse doesn't derive PartialEq for all fields
        prop_assert_eq!(&response.id, &restored.id);
        prop_assert_eq!(&response.model, &restored.model);
        prop_assert_eq!(&response.agent, &restored.agent);
        prop_assert_eq!(&response.status, &restored.status);
        prop_assert_eq!(&response.usage, &restored.usage);
        prop_assert_eq!(&response.previous_interaction_id, &restored.previous_interaction_id);
        prop_assert_eq!(response.input.len(), restored.input.len());
        prop_assert_eq!(response.outputs.len(), restored.outputs.len());

        // Verify grounding_metadata if present
        prop_assert_eq!(&response.grounding_metadata, &restored.grounding_metadata);

        // Verify url_context_metadata if present
        prop_assert_eq!(&response.url_context_metadata, &restored.url_context_metadata);

        // Verify timestamps
        prop_assert_eq!(&response.created, &restored.created);
        prop_assert_eq!(&response.updated, &restored.updated);

        // Verify the full JSON roundtrip is stable (compare as Value for HashMap key order independence)
        let original_value: serde_json::Value = serde_json::from_str(&json).expect("Parse original JSON");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        let restored_value: serde_json::Value = serde_json::from_str(&restored_json).expect("Parse restored JSON");
        prop_assert_eq!(original_value, restored_value);
    }

    /// Test that StreamChunk roundtrips correctly through JSON.
    #[test]
    fn stream_chunk_roundtrip(chunk in arb_stream_chunk()) {
        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let restored: StreamChunk = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Compare as Value for HashMap key order independence
        let original_value: serde_json::Value = serde_json::from_str(&json).expect("Parse original JSON");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        let restored_value: serde_json::Value = serde_json::from_str(&restored_json).expect("Parse restored JSON");
        prop_assert_eq!(original_value, restored_value);
    }
}

// =============================================================================
// GenerationConfig Tests
// =============================================================================

proptest! {
    /// Test that GenerationConfig roundtrips correctly through JSON.
    /// Uses Value comparison since GenerationConfig doesn't derive PartialEq (contains floats).
    #[test]
    fn generation_config_roundtrip(config in arb_generation_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let value1: serde_json::Value = serde_json::from_str(&json).expect("Should parse as Value");
        let restored: GenerationConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        let json2 = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        let value2: serde_json::Value = serde_json::from_str(&json2).expect("Should parse as Value");
        prop_assert_eq!(value1, value2);
    }
}

// =============================================================================
// Additional Edge Case Tests
// =============================================================================

proptest! {
    /// Test empty strings are handled correctly.
    #[test]
    fn empty_text_content_roundtrip(_unused in Just(())) {
        let content = Content::Text { text: Some(String::new()), annotations: None };
        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test None text content is handled correctly.
    #[test]
    fn none_text_content_roundtrip(_unused in Just(())) {
        let content = Content::Text { text: None, annotations: None };
        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test special characters in strings are handled correctly.
    #[test]
    fn special_chars_in_text(text in ".*[\n\r\t\"\\\\].*") {
        let content = Content::Text { text: Some(text), annotations: None };
        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test Unicode in strings is handled correctly.
    #[test]
    fn unicode_in_text(text in ".*[\\u{1F600}-\\u{1F64F}].*") {
        let content = Content::Text { text: Some(text), annotations: None };
        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test large token counts don't overflow or cause issues.
    #[test]
    fn large_token_counts(
        input in any::<u32>(),
        output in any::<u32>(),
        total in any::<u32>(),
    ) {
        let usage = UsageMetadata {
            total_input_tokens: Some(input),
            total_output_tokens: Some(output),
            total_tokens: Some(total),
            ..Default::default()
        };
        let json = serde_json::to_string(&usage).expect("Serialization should succeed");
        let restored: UsageMetadata = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(usage, restored);
    }

    /// Test deeply nested JSON in function call arguments (3-4 levels).
    #[test]
    fn deeply_nested_json_in_function_call(_unused in Just(())) {
        let nested_args = serde_json::json!({
            "level1": {
                "level2": {
                    "level3": {
                        "level4": [1, 2, 3, "four", true, null]
                    },
                    "another_level3": {
                        "data": "value",
                        "numbers": [1.5, 2.5, 3.5]
                    }
                },
                "array_at_level2": [
                    {"nested_in_array": "works"},
                    [1, 2, [3, 4, 5]]
                ]
            }
        });

        let content = Content::FunctionCall {
            id: Some("call_123".to_string()),
            name: "deep_function".to_string(),
            args: nested_args,
        };

        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test deeply nested JSON in function result.
    #[test]
    fn deeply_nested_json_in_function_result(_unused in Just(())) {
        let nested_result = serde_json::json!({
            "success": true,
            "data": {
                "items": [
                    {
                        "id": 1,
                        "metadata": {
                            "created": "2024-01-01",
                            "tags": ["tag1", "tag2", {"complex": "tag"}]
                        }
                    },
                    {
                        "id": 2,
                        "metadata": {
                            "created": "2024-01-02",
                            "nested_array": [[1, 2], [3, 4], [[5, 6], [7, 8]]]
                        }
                    }
                ]
            }
        });

        let content = Content::FunctionResult {
            name: Some("deep_function".to_string()),
            call_id: "call_123".to_string(),
            result: nested_result,
            is_error: None,
        };

        let json = serde_json::to_string(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_str(&json).expect("Deserialization should succeed");
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }
}
