//! Property-based tests for serialization roundtrips using proptest.
//!
//! These tests verify that `deserialize(serialize(x)) == x` for all key types,
//! catching edge cases that hand-written tests might miss.
//!
//! Types whose `Unknown` variants normalize their `data` payload through a
//! roundtrip (e.g. [`Step`], [`Content`]) are compared as `serde_json::Value`
//! (serialize -> deserialize -> re-serialize must be stable); types with
//! well-behaved `PartialEq` are compared directly.

use chrono::{DateTime, TimeZone, Utc};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use super::content::{
    Annotation, CodeExecutionLanguage, Content, FileSearchResultItem, GoogleMapsResultItem,
    GoogleSearchResultItem, Place, Resolution, ReviewSnippet, UrlContextResultItem,
};
use super::environment::{
    AllowlistEntry, EnvironmentSource, EnvironmentSpec, NetworkConfig, RemoteEnvironment,
    SourceType,
};
use super::request::{
    AgentConfig, DeepResearchConfig, DynamicConfig, GenerationConfig, ImageAspectRatio,
    ImageConfig, ImageSize, InteractionInput, Role, ServiceTier, SpeechConfig, ThinkingLevel,
    ThinkingSummaries, TurnContent, VideoConfig, VideoTask, Visualization,
};
use super::response::{
    GroundingToolCount, InteractionResponse, InteractionStatus, ModalityTokens,
    OwnedFunctionCallInfo, UsageMetadata,
};
use super::response_format::{ResponseDelivery, ResponseFormat, ResponseFormatSpec};
use super::steps::{FunctionResultPayload, Step, StepDelta, StepError};
use super::tools::{
    AllowedTools, ExaAiSearchConfig, FunctionCallingMode, FunctionParameters, HybridSearchConfig,
    ParallelAiSearchConfig, RagFilter, RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig,
    RetrievalType, SearchType, Tool, ToolChoice, VertexAiSearchConfig,
};
use super::webhooks::{RevocationBehavior, WebhookConfig, WebhookEvent, WebhookState};
use super::wire_streaming::StreamChunk;

// =============================================================================
// Shared Helpers
// =============================================================================

/// Asserts that `serialize -> deserialize -> re-serialize` is stable as JSON.
///
/// Used for types that don't derive `PartialEq` or whose `Unknown` variants
/// normalize their internal `data` payload through a roundtrip.
fn assert_value_roundtrip<T>(value: &T) -> Result<(), TestCaseError>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let json = serde_json::to_value(value).expect("Serialization should succeed");
    let restored: T = serde_json::from_value(json.clone()).expect("Deserialization should succeed");
    let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
    prop_assert_eq!(json, restored_json);
    Ok(())
}

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

/// Strategy for generating a clean `f64` (for latitude/longitude and similar).
fn arb_clean_f64() -> impl Strategy<Value = f64> {
    (-180_000i32..180_000).prop_map(|n| (n as f64) / 1000.0)
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
        // Simple arrays (scalars only, so arrays never look like content lists)
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

/// Strategy for a non-string, non-content-shaped JSON value.
///
/// Used for [`FunctionResultPayload::Json`]: a bare string would reclassify to
/// `Text` on deserialization, and an array of `{"type": ...}` objects would
/// reclassify to `Contents`, so both are excluded here (the arrays produced by
/// [`arb_json_value`] contain scalars only, which never look like content).
fn arb_non_string_json_value() -> impl Strategy<Value = serde_json::Value> {
    arb_json_value().prop_filter("Json payload must not be a bare string", |v| !v.is_string())
}

/// Strategy for generating valid identifiers (function names, IDs, etc.)
fn arb_identifier() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,30}"
}

/// Strategy for generating text strings up to 500 characters (may be empty).
fn arb_text() -> impl Strategy<Value = String> {
    ".{0,500}"
}

/// Strategy for generating unknown wire type tags.
///
/// Uses a `zz_` prefix so generated tags can never collide with any known
/// snake_case type tag (`text`, `user_input`, `function_call`, ...).
fn arb_unknown_type() -> impl Strategy<Value = String> {
    "zz_[a-z0-9_]{1,12}"
}

/// Strategy for generating a small JSON object used as Unknown-variant payload.
fn arb_unknown_object() -> impl Strategy<Value = serde_json::Value> {
    prop::collection::hash_map("[a-z_][a-z0-9_]{0,10}", ".{0,20}", 0..3).prop_map(|m| {
        serde_json::Value::Object(
            m.into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect(),
        )
    })
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
        arb_unknown_type().prop_map(|s| Resolution::Unknown {
            resolution_type: s.clone(),
            data: serde_json::Value::String(s),
        }),
    ]
}

// =============================================================================
// Annotation Strategies (discriminated union: url/file/place citation)
// =============================================================================

fn arb_review_snippet() -> impl Strategy<Value = ReviewSnippet> {
    (
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_identifier()),
    )
        .prop_map(|(title, url, review_id)| ReviewSnippet {
            title,
            url,
            review_id,
        })
}

/// Strategy for generating Annotation values (all citation types + Unknown).
fn arb_annotation() -> impl Strategy<Value = Annotation> {
    fn span() -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        (0usize..1000, 0usize..1000)
    }
    prop_oneof![
        // UrlCitation
        (
            span(),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text())
        )
            .prop_map(|((start, len), url, title)| Annotation::UrlCitation {
                url,
                title,
                start_index: start,
                end_index: start.saturating_add(len),
            }),
        // FileCitation
        (
            span(),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_unknown_object()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(arb_identifier()),
        )
            .prop_map(
                |(
                    (start, len),
                    document_uri,
                    file_name,
                    source,
                    custom_metadata,
                    page_number,
                    media_id,
                )| {
                    Annotation::FileCitation {
                        document_uri,
                        file_name,
                        source,
                        custom_metadata,
                        page_number,
                        media_id,
                        start_index: start,
                        end_index: start.saturating_add(len),
                    }
                }
            ),
        // PlaceCitation
        (
            span(),
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            prop::collection::vec(arb_review_snippet(), 0..2),
        )
            .prop_map(|((start, len), place_id, name, url, review_snippets)| {
                Annotation::PlaceCitation {
                    place_id,
                    name,
                    url,
                    review_snippets,
                    start_index: start,
                    end_index: start.saturating_add(len),
                }
            }),
        // Unknown annotation type (forward compatibility)
        (arb_unknown_type(), arb_unknown_object()).prop_map(|(annotation_type, data)| {
            Annotation::Unknown {
                annotation_type,
                data,
            }
        }),
    ]
}

// =============================================================================
// Tool Result Item Strategies
// =============================================================================

/// Strategy for generating GoogleSearchResultItem objects.
fn arb_google_search_result_item() -> impl Strategy<Value = GoogleSearchResultItem> {
    (
        arb_text(),
        arb_text(),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
    )
        .prop_map(|(title, url, rendered_content, search_suggestions)| {
            GoogleSearchResultItem {
                title,
                url,
                rendered_content,
                search_suggestions,
            }
        })
}

/// Strategy for generating Place objects.
/// Uses only simple fields to avoid f64 NaN comparison issues.
fn arb_place() -> impl Strategy<Value = Place> {
    (
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_text()),
        proptest::option::of(prop::collection::vec(arb_review_snippet(), 0..2)),
    )
        .prop_map(
            |(name, formatted_address, place_id, url, review_snippets)| Place {
                name,
                formatted_address,
                place_id,
                url,
                lat: None,
                lng: None,
                types: None,
                rating: None,
                user_ratings_total: None,
                website: None,
                phone_number: None,
                review_snippets,
                extra: serde_json::Map::new(),
            },
        )
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
        prop_oneof![
            Just("success"),
            Just("error"),
            Just("paywall"),
            Just("unsafe")
        ],
    )
        .prop_map(|(url, status)| UrlContextResultItem::new(url, status))
}

// =============================================================================
// String Enum Strategies
// =============================================================================
//
// Note: the string enums below (InteractionStatus, FunctionCallingMode, ...)
// deserialize unknown values into their Unknown variant even when the
// `strict-unknown` feature is enabled — strict mode only affects Content,
// Step, and FileState — so their Unknown branches are not feature-gated.

/// Strategy for InteractionStatus (all variants including Unknown).
fn arb_interaction_status() -> impl Strategy<Value = InteractionStatus> {
    prop_oneof![
        Just(InteractionStatus::Completed),
        Just(InteractionStatus::InProgress),
        Just(InteractionStatus::RequiresAction),
        Just(InteractionStatus::Failed),
        Just(InteractionStatus::Cancelled),
        Just(InteractionStatus::Incomplete),
        Just(InteractionStatus::BudgetExceeded),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|status_type| InteractionStatus::Unknown {
            status_type: status_type.clone(),
            data: serde_json::Value::String(status_type),
        }),
    ]
}

/// Strategy for FunctionCallingMode (wire format is lowercase).
fn arb_function_calling_mode() -> impl Strategy<Value = FunctionCallingMode> {
    prop_oneof![
        Just(FunctionCallingMode::Auto),
        Just(FunctionCallingMode::Any),
        Just(FunctionCallingMode::None),
        Just(FunctionCallingMode::Validated),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|mode_type| FunctionCallingMode::Unknown {
            mode_type: mode_type.clone(),
            data: serde_json::Value::String(mode_type),
        }),
    ]
}

/// Strategy for ThinkingLevel.
fn arb_thinking_level() -> impl Strategy<Value = ThinkingLevel> {
    prop_oneof![
        Just(ThinkingLevel::Minimal),
        Just(ThinkingLevel::Low),
        Just(ThinkingLevel::Medium),
        Just(ThinkingLevel::High),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|level_type| ThinkingLevel::Unknown {
            level_type: level_type.clone(),
            data: serde_json::Value::String(level_type),
        }),
    ]
}

/// Strategy for ThinkingSummaries.
fn arb_thinking_summaries() -> impl Strategy<Value = ThinkingSummaries> {
    prop_oneof![
        Just(ThinkingSummaries::Auto),
        Just(ThinkingSummaries::None),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|summaries_type| ThinkingSummaries::Unknown {
            summaries_type: summaries_type.clone(),
            data: serde_json::Value::String(summaries_type),
        }),
    ]
}

/// Strategy for ServiceTier (wire format is lowercase).
fn arb_service_tier() -> impl Strategy<Value = ServiceTier> {
    prop_oneof![
        Just(ServiceTier::Flex),
        Just(ServiceTier::Standard),
        Just(ServiceTier::Priority),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|tier_type| ServiceTier::Unknown {
            tier_type: tier_type.clone(),
            data: serde_json::Value::String(tier_type),
        }),
    ]
}

/// Strategy for SearchType (includes EnterpriseWebSearch).
fn arb_search_type() -> impl Strategy<Value = SearchType> {
    prop_oneof![
        Just(SearchType::WebSearch),
        Just(SearchType::ImageSearch),
        Just(SearchType::EnterpriseWebSearch),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|search_type| SearchType::Unknown {
            search_type: search_type.clone(),
            data: serde_json::Value::String(search_type),
        }),
    ]
}

/// Strategy for CodeExecutionLanguage (wire format is lowercase "python").
fn arb_code_execution_language() -> impl Strategy<Value = CodeExecutionLanguage> {
    prop_oneof![
        Just(CodeExecutionLanguage::Python),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|language_type| CodeExecutionLanguage::Unknown {
            language_type: language_type.clone(),
            data: serde_json::Value::String(language_type),
        }),
    ]
}

/// Strategy for Role.
fn arb_role() -> impl Strategy<Value = Role> {
    prop_oneof![
        Just(Role::User),
        Just(Role::Model),
        // Unknown variant with preserved data (role_type and data fields per Evergreen pattern)
        arb_unknown_type().prop_map(|role_type| Role::Unknown {
            data: serde_json::Value::String(role_type.clone()),
            role_type,
        }),
    ]
}

// =============================================================================
// ImageAspectRatio / ImageSize / ImageConfig Strategies
// =============================================================================

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
        arb_unknown_type().prop_map(|ratio_type| ImageAspectRatio::Unknown {
            ratio_type: ratio_type.clone(),
            data: serde_json::Value::String(ratio_type),
        }),
    ]
}

fn arb_image_size() -> impl Strategy<Value = ImageSize> {
    prop_oneof![
        Just(ImageSize::Sd512),
        Just(ImageSize::Hd1k),
        Just(ImageSize::Hd2k),
        Just(ImageSize::Uhd4k),
        // Unknown variant with preserved data
        arb_unknown_type().prop_map(|size_type| ImageSize::Unknown {
            size_type: size_type.clone(),
            data: serde_json::Value::String(size_type),
        }),
    ]
}

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
// Content Strategy (slimmed media union: text/image/audio/video/document)
// =============================================================================

/// Helper to create the known Content variants (used by both strict and non-strict modes).
fn arb_known_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        // Text content (with optional annotations)
        (
            proptest::option::of(arb_text()),
            proptest::option::of(proptest::collection::vec(arb_annotation(), 0..3))
        )
            .prop_map(|(text, annotations)| Content::Text { text, annotations }),
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
        // Audio content (with sample_rate/channels)
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(1u32..8),
        )
            .prop_map(
                |(data, uri, mime_type, sample_rate, channels)| Content::Audio {
                    data,
                    uri,
                    mime_type,
                    sample_rate,
                    channels,
                }
            ),
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
    ]
}

/// Strategy for known Content variants only.
/// Used when strict-unknown is enabled (Unknown variants fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_content() -> impl Strategy<Value = Content> {
    arb_known_content()
}

/// Strategy for all Content variants including Unknown.
/// Used in normal mode (Unknown variants are gracefully handled).
#[cfg(not(feature = "strict-unknown"))]
fn arb_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        arb_known_content(),
        // Unknown content (for forward compatibility testing)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(content_type, data)| Content::Unknown { content_type, data }),
    ]
}

// =============================================================================
// FunctionResultPayload Strategy
// =============================================================================

/// Strategy for FunctionResultPayload.
///
/// Deserialization classifies raw JSON: bare strings become `Text`, arrays of
/// content-shaped objects become `Contents`, everything else `Json`. To keep
/// the roundtrip property exact:
/// - `Json` never carries a bare string (would flip to `Text`)
/// - `Contents` is always non-empty (an empty array flips to `Json([])`)
fn arb_function_result_payload() -> impl Strategy<Value = FunctionResultPayload> {
    prop_oneof![
        arb_text().prop_map(FunctionResultPayload::Text),
        arb_non_string_json_value().prop_map(FunctionResultPayload::Json),
        prop::collection::vec(arb_content(), 1..3).prop_map(FunctionResultPayload::Contents),
    ]
}

// =============================================================================
// Step Strategies (all 17 variants + Unknown)
// =============================================================================

fn arb_step_error() -> impl Strategy<Value = StepError> {
    (
        proptest::option::of(any::<i64>()),
        proptest::option::of(arb_text()),
        proptest::option::of(prop::collection::vec(arb_json_value(), 0..2)),
    )
        .prop_map(|(code, message, details)| StepError {
            code,
            message,
            details,
        })
}

/// Helper to create the known Step variants (used by both strict and non-strict modes).
fn arb_known_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        // user_input
        prop::collection::vec(arb_content(), 0..3).prop_map(|content| Step::UserInput { content }),
        // model_output
        (
            prop::collection::vec(arb_content(), 0..3),
            proptest::option::of(arb_step_error()),
        )
            .prop_map(|(content, error)| Step::ModelOutput { content, error }),
        // thought
        (
            proptest::option::of(arb_text()),
            prop::collection::vec(arb_content(), 0..2),
        )
            .prop_map(|(signature, summary)| Step::Thought { signature, summary }),
        // function_call
        (arb_identifier(), arb_identifier(), arb_json_value()).prop_map(|(id, name, arguments)| {
            Step::FunctionCall {
                id,
                name,
                arguments,
            }
        }),
        // function_result
        (
            arb_identifier(),
            proptest::option::of(arb_identifier()),
            arb_function_result_payload(),
            proptest::option::of(proptest::bool::ANY),
        )
            .prop_map(|(call_id, name, result, is_error)| Step::FunctionResult {
                call_id,
                name,
                result,
                is_error,
            }),
        // code_execution_call (wire: nested arguments {language, code})
        (
            arb_identifier(),
            arb_code_execution_language(),
            arb_text(),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(id, language, code, signature)| Step::CodeExecutionCall {
                id,
                language,
                code,
                signature,
            }),
        // code_execution_result
        (
            arb_identifier(),
            arb_text(),
            proptest::bool::ANY,
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(call_id, result, is_error, signature)| Step::CodeExecutionResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                }
            ),
        // url_context_call (wire: nested arguments {urls})
        (
            arb_identifier(),
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(id, urls, signature)| Step::UrlContextCall {
                id,
                urls,
                signature
            }),
        // url_context_result
        (
            arb_identifier(),
            prop::collection::vec(arb_url_context_result_item(), 0..3),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(call_id, result, is_error, signature)| Step::UrlContextResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                }
            ),
        // google_search_call (wire: nested arguments {queries})
        (
            arb_identifier(),
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_search_type()),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(id, queries, search_type, signature)| Step::GoogleSearchCall {
                    id,
                    queries,
                    search_type,
                    signature,
                }
            ),
        // google_search_result
        (
            arb_identifier(),
            prop::collection::vec(arb_google_search_result_item(), 0..3),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(call_id, result, is_error, signature)| Step::GoogleSearchResult {
                    call_id,
                    result,
                    is_error,
                    signature,
                }
            ),
        // mcp_server_tool_call
        (
            arb_identifier(),
            arb_identifier(),
            arb_identifier(),
            arb_json_value(),
        )
            .prop_map(
                |(id, name, server_name, arguments)| Step::McpServerToolCall {
                    id,
                    name,
                    server_name,
                    arguments,
                }
            ),
        // mcp_server_tool_result
        (
            arb_identifier(),
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_identifier()),
            arb_function_result_payload(),
        )
            .prop_map(
                |(call_id, name, server_name, result)| Step::McpServerToolResult {
                    call_id,
                    name,
                    server_name,
                    result,
                }
            ),
        // file_search_call
        (arb_identifier(), proptest::option::of(arb_text()))
            .prop_map(|(id, signature)| { Step::FileSearchCall { id, signature } }),
        // file_search_result (empty result skips the field; default is empty)
        (
            arb_identifier(),
            prop::collection::vec(arb_file_search_result_item(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(call_id, result, signature)| Step::FileSearchResult {
                call_id,
                result,
                signature,
            }),
        // google_maps_call (wire: nested arguments {queries})
        (
            arb_identifier(),
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(id, queries, signature)| Step::GoogleMapsCall {
                id,
                queries,
                signature
            }),
        // google_maps_result
        (
            arb_identifier(),
            prop::collection::vec(arb_google_maps_result_item(), 0..2),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(call_id, result, signature)| Step::GoogleMapsResult {
                call_id,
                result,
                signature,
            }),
    ]
}

/// Strategy for known Step variants only.
/// Used when strict-unknown is enabled (Unknown steps fail to deserialize in strict mode).
#[cfg(feature = "strict-unknown")]
fn arb_step() -> impl Strategy<Value = Step> {
    arb_known_step()
}

/// Strategy for all Step variants including Unknown.
#[cfg(not(feature = "strict-unknown"))]
fn arb_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        arb_known_step(),
        // Unknown step (for forward compatibility testing)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(step_type, data)| Step::Unknown { step_type, data }),
    ]
}

// =============================================================================
// StepDelta Strategy (streaming step.delta payloads)
// =============================================================================

fn arb_step_delta() -> impl Strategy<Value = StepDelta> {
    prop_oneof![
        // text
        arb_text().prop_map(|text| StepDelta::Text { text }),
        // image
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_resolution()),
        )
            .prop_map(|(data, uri, mime_type, resolution)| StepDelta::Image {
                data,
                uri,
                mime_type,
                resolution,
            }),
        // audio
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(any::<u32>()),
            proptest::option::of(1u32..8),
        )
            .prop_map(|(data, uri, mime_type, rate, sample_rate, channels)| {
                StepDelta::Audio {
                    data,
                    uri,
                    mime_type,
                    rate,
                    sample_rate,
                    channels,
                }
            }),
        // video
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_resolution()),
        )
            .prop_map(|(data, uri, mime_type, resolution)| StepDelta::Video {
                data,
                uri,
                mime_type,
                resolution,
            }),
        // document
        (
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(data, uri, mime_type)| StepDelta::Document {
                data,
                uri,
                mime_type
            }),
        // thought_summary
        proptest::option::of(arb_content())
            .prop_map(|content| StepDelta::ThoughtSummary { content }),
        // thought_signature
        proptest::option::of(arb_text())
            .prop_map(|signature| StepDelta::ThoughtSignature { signature }),
        // text_annotation_delta
        prop::collection::vec(arb_annotation(), 0..3)
            .prop_map(|annotations| StepDelta::TextAnnotation { annotations }),
        // arguments_delta (streaming function-call args)
        arb_text().prop_map(|arguments| StepDelta::ArgumentsDelta { arguments }),
        // function_result
        (
            arb_identifier(),
            proptest::option::of(arb_identifier()),
            arb_function_result_payload(),
            proptest::option::of(proptest::bool::ANY),
        )
            .prop_map(
                |(call_id, name, result, is_error)| StepDelta::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                }
            ),
        // code_execution_call
        //
        // `language` must be `Some`: the wire nests it in `arguments` where a
        // JSON null would deserialize into `Some(CodeExecutionLanguage::Unknown)`
        // rather than back to `None`.
        (
            arb_code_execution_language(),
            proptest::option::of(arb_text()),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(language, code, signature)| StepDelta::CodeExecutionCall {
                language: Some(language),
                code,
                signature,
            }),
        // code_execution_result
        (
            arb_text(),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(result, is_error, signature)| StepDelta::CodeExecutionResult {
                    result,
                    is_error,
                    signature,
                }
            ),
        // url_context_call
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(urls, signature)| StepDelta::UrlContextCall { urls, signature }),
        // url_context_result
        (
            prop::collection::vec(arb_url_context_result_item(), 0..3),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(result, is_error, signature)| StepDelta::UrlContextResult {
                    result,
                    is_error,
                    signature,
                }
            ),
        // google_search_call
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(queries, signature)| StepDelta::GoogleSearchCall { queries, signature }),
        // google_search_result
        (
            prop::collection::vec(arb_google_search_result_item(), 0..3),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(result, is_error, signature)| StepDelta::GoogleSearchResult {
                    result,
                    is_error,
                    signature,
                }
            ),
        // mcp_server_tool_call
        (arb_identifier(), arb_identifier(), arb_json_value()).prop_map(
            |(name, server_name, arguments)| StepDelta::McpServerToolCall {
                name,
                server_name,
                arguments,
            }
        ),
        // mcp_server_tool_result
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_identifier()),
            arb_function_result_payload(),
        )
            .prop_map(
                |(name, server_name, result)| StepDelta::McpServerToolResult {
                    name,
                    server_name,
                    result,
                }
            ),
        // file_search_call
        proptest::option::of(arb_text())
            .prop_map(|signature| StepDelta::FileSearchCall { signature }),
        // file_search_result
        (
            prop::collection::vec(arb_file_search_result_item(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(result, signature)| StepDelta::FileSearchResult { result, signature }),
        // google_maps_call
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(queries, signature)| StepDelta::GoogleMapsCall { queries, signature }),
        // google_maps_result
        (
            prop::collection::vec(arb_google_maps_result_item(), 0..2),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(result, signature)| StepDelta::GoogleMapsResult { result, signature }),
        // Unknown delta (not gated: StepDelta preserves unknowns even in strict mode)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(delta_type, data)| StepDelta::Unknown { delta_type, data }),
    ]
}

// =============================================================================
// InteractionInput Strategy
// =============================================================================

/// Strategy for InteractionInput.
///
/// Note: `Content` is generated non-empty because an empty JSON array
/// deserializes as `Steps(vec![])` (the canonical revision 2026-05-20 form).
fn arb_interaction_input() -> impl Strategy<Value = InteractionInput> {
    prop_oneof![
        arb_text().prop_map(InteractionInput::Text),
        prop::collection::vec(arb_content(), 1..3).prop_map(InteractionInput::Content),
        prop::collection::vec(arb_step(), 0..3).prop_map(InteractionInput::Steps),
    ]
}

// =============================================================================
// TurnContent Strategy (still used by ConversationBuilder)
// =============================================================================

fn arb_turn_content() -> impl Strategy<Value = TurnContent> {
    prop_oneof![
        // Text content
        arb_text().prop_map(TurnContent::Text),
        // Parts content
        prop::collection::vec(arb_content(), 0..3).prop_map(TurnContent::Parts),
    ]
}

// =============================================================================
// ToolChoice / AllowedTools Strategies
// =============================================================================

fn arb_allowed_tools() -> impl Strategy<Value = AllowedTools> {
    (
        proptest::option::of(arb_function_calling_mode()),
        prop::collection::vec(arb_identifier(), 0..3),
    )
        .prop_map(|(mode, tools)| AllowedTools { mode, tools })
}

fn arb_tool_choice() -> impl Strategy<Value = ToolChoice> {
    prop_oneof![
        arb_function_calling_mode().prop_map(ToolChoice::Mode),
        arb_allowed_tools().prop_map(ToolChoice::AllowedTools),
        // Unknown shape: an object without the "allowed_tools" key
        arb_unknown_type().prop_map(|key| {
            let data = serde_json::json!({ key.clone(): "future_config" });
            ToolChoice::Unknown {
                choice_type: key,
                data,
            }
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
        proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32)),
        proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32)),
        proptest::option::of(proptest::collection::vec(arb_speech_config(), 1..3)),
        proptest::option::of(arb_image_config()),
        proptest::option::of(arb_video_config()),
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
            (
                tool_choice,
                presence_penalty,
                frequency_penalty,
                speech_config,
                image_config,
                video_config,
            ),
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
                presence_penalty,
                frequency_penalty,
                speech_config,
                image_config,
                video_config,
            }
        },
    )
}

fn arb_video_task() -> impl Strategy<Value = VideoTask> {
    prop_oneof![
        Just(VideoTask::TextToVideo),
        Just(VideoTask::ImageToVideo),
        Just(VideoTask::ReferenceToVideo),
        Just(VideoTask::Edit),
        arb_unknown_type().prop_map(|task_type| VideoTask::Unknown {
            data: serde_json::Value::String(task_type.clone()),
            task_type,
        }),
    ]
}

fn arb_video_config() -> impl Strategy<Value = VideoConfig> {
    proptest::option::of(arb_video_task()).prop_map(|task| VideoConfig { task })
}

// =============================================================================
// AgentConfig Strategies
// =============================================================================

/// Strategy for AgentConfig using typed config structs.
/// Since AgentConfig is a thin wrapper around serde_json::Value,
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
// UsageMetadata Strategies
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

/// Strategy for generating GroundingToolCount values.
fn arb_grounding_tool_count() -> impl Strategy<Value = GroundingToolCount> {
    (
        proptest::option::of(prop_oneof![
            Just("google_search".to_string()),
            Just("google_maps".to_string()),
            Just("retrieval".to_string()),
            arb_identifier(),
        ]),
        proptest::option::of(any::<u32>()),
    )
        .prop_map(|(tool_type, count)| GroundingToolCount { tool_type, count })
}

fn arb_usage_metadata() -> impl Strategy<Value = UsageMetadata> {
    (
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
        proptest::option::of(prop::collection::vec(arb_grounding_tool_count(), 0..3)),
    )
        .prop_map(
            |(
                total_input_tokens,
                total_output_tokens,
                total_tokens,
                total_cached_tokens,
                total_thought_tokens,
                total_tool_use_tokens,
                input_tokens_by_modality,
                output_tokens_by_modality,
                cached_tokens_by_modality,
                tool_use_tokens_by_modality,
                grounding_tool_count,
            )| {
                UsageMetadata {
                    total_input_tokens,
                    total_output_tokens,
                    total_tokens,
                    total_cached_tokens,
                    total_thought_tokens,
                    total_tool_use_tokens,
                    input_tokens_by_modality,
                    output_tokens_by_modality,
                    cached_tokens_by_modality,
                    tool_use_tokens_by_modality,
                    grounding_tool_count,
                }
            },
        )
}

// =============================================================================
// OwnedFunctionCallInfo Strategy
// =============================================================================

fn arb_owned_function_call_info() -> impl Strategy<Value = OwnedFunctionCallInfo> {
    (arb_identifier(), arb_identifier(), arb_json_value())
        .prop_map(|(id, name, args)| OwnedFunctionCallInfo { id, name, args })
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

fn arb_tool() -> impl Strategy<Value = Tool> {
    prop_oneof![
        // Function tool
        (arb_identifier(), arb_text(), arb_function_parameters()).prop_map(
            |(name, description, parameters)| Tool::Function {
                name,
                description,
                parameters
            }
        ),
        // Google Search (non-empty search_types when present)
        proptest::option::of(proptest::collection::vec(arb_search_type(), 1..3))
            .prop_map(|search_types| Tool::GoogleSearch { search_types }),
        // Google Maps (with optional location bias)
        (
            proptest::option::of(any::<bool>()),
            proptest::option::of(arb_clean_f64()),
            proptest::option::of(arb_clean_f64()),
        )
            .prop_map(|(enable_widget, latitude, longitude)| Tool::GoogleMaps {
                enable_widget,
                latitude,
                longitude,
            }),
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
        // ComputerUse tool
        (
            prop_oneof![
                Just("browser".to_string()),
                Just("mobile".to_string()),
                Just("desktop".to_string()),
                arb_identifier(),
            ],
            proptest::collection::vec(arb_identifier(), 0..3),
            proptest::option::of(any::<bool>()),
            proptest::collection::vec(arb_identifier(), 0..2),
        )
            .prop_map(
                |(environment, excluded, detect, disabled)| Tool::ComputerUse {
                    environment,
                    excluded_predefined_functions: excluded,
                    enable_prompt_injection_detection: detect,
                    disabled_safety_policies: disabled,
                }
            ),
        // MCP Server (use BTreeMap for deterministic JSON key ordering in roundtrip tests;
        // allowed_tools is non-empty when present since empty is skipped on serialize)
        (
            arb_identifier(),
            arb_text(),
            proptest::option::of(proptest::collection::vec(arb_allowed_tools(), 1..3)),
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
        // Retrieval tool (types non-empty when present since empty is skipped)
        (
            proptest::option::of(proptest::collection::vec(arb_retrieval_type(), 1..3)),
            proptest::option::of(arb_vertex_ai_search_config()),
            proptest::option::of(arb_exa_ai_search_config()),
            proptest::option::of(arb_parallel_ai_search_config()),
            proptest::option::of(arb_rag_store_config()),
        )
            .prop_map(
                |(
                    retrieval_types,
                    vertex_ai_search_config,
                    exa_ai_search_config,
                    parallel_ai_search_config,
                    rag_store_config,
                )| Tool::Retrieval {
                    retrieval_types,
                    vertex_ai_search_config,
                    exa_ai_search_config,
                    parallel_ai_search_config,
                    rag_store_config: rag_store_config.map(Box::new),
                },
            ),
        // Unknown tool (Tool preserves unknowns even in strict mode)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(tool_type, data)| Tool::Unknown { tool_type, data }),
    ]
}

// =============================================================================
// Retrieval Tool Strategies
// =============================================================================

fn arb_retrieval_type() -> impl Strategy<Value = RetrievalType> {
    prop_oneof![
        Just(RetrievalType::VertexAiSearch),
        Just(RetrievalType::RagStore),
        Just(RetrievalType::ExaAiSearch),
        Just(RetrievalType::ParallelAiSearch),
        arb_unknown_type().prop_map(|retrieval_type| RetrievalType::Unknown {
            data: serde_json::Value::String(retrieval_type.clone()),
            retrieval_type,
        }),
    ]
}

fn arb_vertex_ai_search_config() -> impl Strategy<Value = VertexAiSearchConfig> {
    (
        proptest::option::of(arb_identifier()),
        proptest::option::of(proptest::collection::vec(arb_identifier(), 0..3)),
    )
        .prop_map(|(engine, datastores)| VertexAiSearchConfig { engine, datastores })
}

fn arb_exa_ai_search_config() -> impl Strategy<Value = ExaAiSearchConfig> {
    (arb_identifier(), proptest::option::of(arb_unknown_object())).prop_map(
        |(api_key, custom_config)| ExaAiSearchConfig {
            api_key,
            custom_config,
        },
    )
}

fn arb_parallel_ai_search_config() -> impl Strategy<Value = ParallelAiSearchConfig> {
    (
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_unknown_object()),
    )
        .prop_map(|(api_key, custom_config)| ParallelAiSearchConfig {
            api_key,
            custom_config,
        })
}

fn arb_rag_store_config() -> impl Strategy<Value = RagStoreConfig> {
    (
        proptest::option::of(proptest::collection::vec(
            (
                proptest::option::of(arb_identifier()),
                proptest::option::of(proptest::collection::vec(arb_identifier(), 0..2)),
            )
                .prop_map(|(rag_corpus, rag_file_ids)| RagResource {
                    rag_corpus,
                    rag_file_ids,
                }),
            0..3,
        )),
        proptest::option::of(1..100i32),
        proptest::option::of(arb_clean_f64()),
        proptest::option::of(arb_rag_retrieval_config()),
    )
        .prop_map(
            |(rag_resources, similarity_top_k, vector_distance_threshold, rag_retrieval_config)| {
                RagStoreConfig {
                    rag_resources,
                    similarity_top_k,
                    vector_distance_threshold,
                    rag_retrieval_config,
                }
            },
        )
}

fn arb_rag_retrieval_config() -> impl Strategy<Value = RagRetrievalConfig> {
    (
        proptest::option::of(1..100i32),
        proptest::option::of(
            proptest::option::of(arb_clean_float().prop_map(|v| v.as_f64().unwrap() as f32))
                .prop_map(|alpha| HybridSearchConfig { alpha }),
        ),
        proptest::option::of(
            (
                proptest::option::of(arb_clean_f64()),
                proptest::option::of(arb_clean_f64()),
                proptest::option::of(arb_text()),
            )
                .prop_map(
                    |(vector_distance_threshold, vector_similarity_threshold, metadata_filter)| {
                        RagFilter {
                            vector_distance_threshold,
                            vector_similarity_threshold,
                            metadata_filter,
                        }
                    },
                ),
        ),
        proptest::option::of(
            proptest::option::of(arb_identifier()).prop_map(|model_name| RagRanking {
                ranking_config: "rank_service".to_string(),
                model_name,
            }),
        ),
    )
        .prop_map(
            |(top_k, hybrid_search, filter, ranking)| RagRetrievalConfig {
                top_k,
                hybrid_search,
                filter,
                ranking,
            },
        )
}

// =============================================================================
// Webhook / Environment / ResponseFormat Strategies
// =============================================================================

fn arb_webhook_event() -> impl Strategy<Value = WebhookEvent> {
    prop_oneof![
        Just(WebhookEvent::BatchSucceeded),
        Just(WebhookEvent::BatchExpired),
        Just(WebhookEvent::BatchFailed),
        Just(WebhookEvent::InteractionRequiresAction),
        Just(WebhookEvent::InteractionCompleted),
        Just(WebhookEvent::InteractionFailed),
        Just(WebhookEvent::VideoGenerated),
        arb_unknown_type().prop_map(|event_type| WebhookEvent::Unknown {
            data: serde_json::Value::String(event_type.clone()),
            event_type,
        }),
    ]
}

fn arb_webhook_state() -> impl Strategy<Value = WebhookState> {
    prop_oneof![
        Just(WebhookState::Enabled),
        Just(WebhookState::Disabled),
        Just(WebhookState::DisabledDueToFailedDeliveries),
        arb_unknown_type().prop_map(|state_type| WebhookState::Unknown {
            data: serde_json::Value::String(state_type.clone()),
            state_type,
        }),
    ]
}

fn arb_revocation_behavior() -> impl Strategy<Value = RevocationBehavior> {
    prop_oneof![
        Just(RevocationBehavior::RevokePreviousSecretsAfterH24),
        Just(RevocationBehavior::RevokePreviousSecretsImmediately),
        arb_unknown_type().prop_map(|behavior_type| RevocationBehavior::Unknown {
            data: serde_json::Value::String(behavior_type.clone()),
            behavior_type,
        }),
    ]
}

fn arb_webhook_config() -> impl Strategy<Value = WebhookConfig> {
    (
        proptest::option::of(proptest::collection::vec(arb_text(), 0..3)),
        proptest::option::of(arb_unknown_object()),
    )
        .prop_map(|(uris, user_metadata)| WebhookConfig {
            uris,
            user_metadata,
        })
}

fn arb_source_type() -> impl Strategy<Value = SourceType> {
    prop_oneof![
        Just(SourceType::Gcs),
        Just(SourceType::Inline),
        Just(SourceType::Repository),
        Just(SourceType::SkillRegistry),
        arb_unknown_type().prop_map(|source_type| SourceType::Unknown {
            data: serde_json::Value::String(source_type.clone()),
            source_type,
        }),
    ]
}

fn arb_environment_source() -> impl Strategy<Value = EnvironmentSource> {
    (
        proptest::option::of(arb_source_type()),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_identifier()),
        proptest::option::of(arb_text()),
        proptest::option::of(arb_identifier()),
    )
        .prop_map(
            |(source_type, source, target, content, encoding)| EnvironmentSource {
                source_type,
                source,
                target,
                content,
                encoding,
            },
        )
}

fn arb_network_config() -> impl Strategy<Value = NetworkConfig> {
    prop_oneof![
        Just(NetworkConfig::Disabled),
        proptest::collection::vec(
            (
                arb_identifier(),
                proptest::option::of(proptest::collection::vec(
                    proptest::collection::btree_map(arb_identifier(), arb_text(), 1..3)
                        .prop_map(|m| m.into_iter().collect::<std::collections::HashMap<_, _>>()),
                    1..2,
                )),
            )
                .prop_map(|(domain, transform)| AllowlistEntry { domain, transform }),
            0..3,
        )
        .prop_map(NetworkConfig::Allowlist),
    ]
}

fn arb_environment_spec() -> impl Strategy<Value = EnvironmentSpec> {
    prop_oneof![
        arb_identifier().prop_map(EnvironmentSpec::Id),
        (
            proptest::collection::vec(arb_environment_source(), 0..3),
            proptest::option::of(arb_network_config()),
        )
            .prop_map(|(sources, network)| {
                EnvironmentSpec::Remote(RemoteEnvironment { sources, network })
            }),
    ]
}

fn arb_response_delivery() -> impl Strategy<Value = ResponseDelivery> {
    prop_oneof![
        Just(ResponseDelivery::Inline),
        Just(ResponseDelivery::Uri),
        arb_unknown_type().prop_map(|delivery_type| ResponseDelivery::Unknown {
            data: serde_json::Value::String(delivery_type.clone()),
            delivery_type,
        }),
    ]
}

fn arb_response_format() -> impl Strategy<Value = ResponseFormat> {
    prop_oneof![
        (
            proptest::option::of(arb_identifier()),
            proptest::option::of(arb_unknown_object()),
        )
            .prop_map(|(mime_type, schema)| ResponseFormat::Text { mime_type, schema }),
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
                }
            }),
    ]
}

fn arb_response_format_spec() -> impl Strategy<Value = ResponseFormatSpec> {
    prop_oneof![
        arb_response_format().prop_map(ResponseFormatSpec::Single),
        proptest::collection::vec(arb_response_format(), 0..3).prop_map(ResponseFormatSpec::List),
    ]
}

fn arb_visualization() -> impl Strategy<Value = Visualization> {
    prop_oneof![
        Just(Visualization::Off),
        Just(Visualization::Auto),
        arb_unknown_type().prop_map(|visualization_type| Visualization::Unknown {
            data: serde_json::Value::String(visualization_type.clone()),
            visualization_type,
        }),
    ]
}

// =============================================================================
// InteractionResponse Strategy
// =============================================================================

fn arb_interaction_response() -> impl Strategy<Value = InteractionResponse> {
    // Split into two tuples to avoid proptest's 12-element limit
    let part1 = (
        proptest::option::of(arb_identifier()),        // id
        proptest::option::of(arb_identifier()),        // model
        proptest::option::of(arb_identifier()),        // agent
        proptest::option::of(arb_interaction_input()), // input
        prop::collection::vec(arb_step(), 0..4),       // steps
        arb_interaction_status(),                      // status
        proptest::option::of(arb_usage_metadata()),    // usage
    );
    let part2 = (
        proptest::option::of(prop::collection::vec(arb_tool(), 0..3)), // tools
        proptest::option::of(arb_identifier()),                        // previous_interaction_id
        proptest::option::of(arb_identifier()),                        // environment_id
        proptest::option::of(arb_text()),                              // output_text
        proptest::option::of(arb_datetime()),                          // created
        proptest::option::of(arb_datetime()),                          // updated
    );

    (part1, part2).prop_map(
        |(
            (id, model, agent, input, steps, status, usage),
            (tools, previous_interaction_id, environment_id, output_text, created, updated),
        )| {
            InteractionResponse {
                id,
                model,
                agent,
                input,
                steps,
                status,
                usage,
                tools,
                previous_interaction_id,
                environment_id,
                output_text,
                created,
                updated,
            }
        },
    )
}

// =============================================================================
// StreamChunk Strategy
// =============================================================================

fn arb_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    prop_oneof![
        // Created variant
        arb_interaction_response().prop_map(|interaction| StreamChunk::Created { interaction }),
        // StatusUpdate variant
        (arb_identifier(), arb_interaction_status()).prop_map(|(interaction_id, status)| {
            StreamChunk::StatusUpdate {
                interaction_id,
                status,
            }
        }),
        // StepStart variant
        (any::<usize>(), arb_step())
            .prop_map(|(index, step)| StreamChunk::StepStart { index, step }),
        // StepDelta variant
        (any::<usize>(), arb_step_delta())
            .prop_map(|(index, delta)| StreamChunk::StepDelta { index, delta }),
        // StepStop variant
        (
            any::<usize>(),
            proptest::option::of(arb_usage_metadata()),
            proptest::option::of(arb_usage_metadata()),
        )
            .prop_map(|(index, usage, step_usage)| StreamChunk::StepStop {
                index,
                usage,
                step_usage,
            }),
        // Completed variant
        arb_interaction_response().prop_map(StreamChunk::Completed),
        // Error variant
        (arb_text(), proptest::option::of(arb_identifier()))
            .prop_map(|(message, code)| { StreamChunk::Error { message, code } }),
        // Unknown variant for forward compatibility
        (arb_unknown_type(), arb_json_value())
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

    /// Test that GroundingToolCount roundtrips correctly through JSON.
    #[test]
    fn grounding_tool_count_roundtrip(count in arb_grounding_tool_count()) {
        let json = serde_json::to_string(&count).expect("Serialization should succeed");
        let restored: GroundingToolCount = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(count, restored);
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

    /// Test that CodeExecutionLanguage roundtrips correctly through JSON
    /// (wire format is lowercase "python").
    #[test]
    fn code_execution_language_roundtrip(lang in arb_code_execution_language()) {
        let json = serde_json::to_string(&lang).expect("Serialization should succeed");
        let restored: CodeExecutionLanguage = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(lang, restored);
    }

    /// Test that the known CodeExecutionLanguage wire format is lowercase.
    #[test]
    fn code_execution_language_wire_is_lowercase(_unused in Just(())) {
        let json = serde_json::to_string(&CodeExecutionLanguage::Python).unwrap();
        prop_assert_eq!(json, "\"python\"");
    }

    /// Test that FunctionCallingMode roundtrips correctly through JSON
    /// (wire format is lowercase).
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

    /// Test that SearchType roundtrips correctly through JSON.
    #[test]
    fn search_type_roundtrip(search_type in arb_search_type()) {
        let json = serde_json::to_string(&search_type).expect("Serialization should succeed");
        let restored: SearchType = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(search_type, restored);
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

    /// Test that Annotation roundtrips stably through JSON.
    ///
    /// Uses JSON comparison since the Unknown variant normalizes its data
    /// payload (the "type" tag is merged into the preserved JSON).
    #[test]
    fn annotation_roundtrip(annotation in arb_annotation()) {
        assert_value_roundtrip(&annotation)?;
    }

    /// Test that TurnContent roundtrips stably through JSON.
    #[test]
    fn turn_content_roundtrip(content in arb_turn_content()) {
        assert_value_roundtrip(&content)?;
    }

    /// Test that AllowedTools roundtrips correctly through JSON.
    #[test]
    fn allowed_tools_roundtrip(allowed in arb_allowed_tools()) {
        let json = serde_json::to_string(&allowed).expect("Serialization should succeed");
        let restored: AllowedTools = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(allowed, restored);
    }

    /// Test that ToolChoice roundtrips stably through JSON.
    ///
    /// Uses JSON comparison since the Unknown variant regenerates its
    /// choice_type descriptor on deserialization.
    #[test]
    fn tool_choice_roundtrip(choice in arb_tool_choice()) {
        assert_value_roundtrip(&choice)?;
    }

    /// Test that AgentConfig roundtrips correctly through JSON.
    #[test]
    fn agent_config_roundtrip(config in arb_agent_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: AgentConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(config, restored);
    }

    /// Test that GenerationConfig roundtrips stably through JSON.
    /// Uses Value comparison since GenerationConfig doesn't derive PartialEq (contains floats).
    #[test]
    fn generation_config_roundtrip(config in arb_generation_config()) {
        assert_value_roundtrip(&config)?;
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

    /// Test that WebhookEvent roundtrips correctly through JSON.
    #[test]
    fn webhook_event_roundtrip(event in arb_webhook_event()) {
        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        let restored: WebhookEvent = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(event, restored);
    }

    /// Test that WebhookState roundtrips correctly through JSON.
    #[test]
    fn webhook_state_roundtrip(state in arb_webhook_state()) {
        let json = serde_json::to_string(&state).expect("Serialization should succeed");
        let restored: WebhookState = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(state, restored);
    }

    /// Test that RevocationBehavior roundtrips correctly through JSON.
    #[test]
    fn revocation_behavior_roundtrip(behavior in arb_revocation_behavior()) {
        let json = serde_json::to_string(&behavior).expect("Serialization should succeed");
        let restored: RevocationBehavior = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(behavior, restored);
    }

    /// Test that WebhookConfig roundtrips correctly through JSON.
    #[test]
    fn webhook_config_roundtrip(config in arb_webhook_config()) {
        let json = serde_json::to_string(&config).expect("Serialization should succeed");
        let restored: WebhookConfig = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(config, restored);
    }

    /// Test that SourceType roundtrips correctly through JSON.
    #[test]
    fn source_type_roundtrip(source_type in arb_source_type()) {
        let json = serde_json::to_string(&source_type).expect("Serialization should succeed");
        let restored: SourceType = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(source_type, restored);
    }

    /// Test that EnvironmentSpec (string ID or remote environment)
    /// roundtrips correctly through JSON.
    #[test]
    fn environment_spec_roundtrip(spec in arb_environment_spec()) {
        let json = serde_json::to_string(&spec).expect("Serialization should succeed");
        let restored: EnvironmentSpec = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(spec, restored);
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

    /// Test that RetrievalType roundtrips correctly through JSON.
    #[test]
    fn retrieval_type_roundtrip(retrieval_type in arb_retrieval_type()) {
        let json = serde_json::to_string(&retrieval_type).expect("Serialization should succeed");
        let restored: RetrievalType = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(retrieval_type, restored);
    }

    /// Test that FunctionResultPayload roundtrips through JSON, preserving
    /// both the JSON representation and the variant classification.
    #[test]
    fn function_result_payload_roundtrip(payload in arb_function_result_payload()) {
        let json = serde_json::to_value(&payload).expect("Serialization should succeed");
        let restored: FunctionResultPayload =
            serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        // Variant classification must be stable
        prop_assert_eq!(
            std::mem::discriminant(&payload),
            std::mem::discriminant(&restored)
        );

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that Content roundtrips stably through JSON.
    #[test]
    fn content_roundtrip(content in arb_content()) {
        assert_value_roundtrip(&content)?;
    }

    /// Test that StepError roundtrips correctly through JSON.
    #[test]
    fn step_error_roundtrip(error in arb_step_error()) {
        let json = serde_json::to_string(&error).expect("Serialization should succeed");
        let restored: StepError = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(error, restored);
    }

    /// Test that Step roundtrips stably through JSON (all step types,
    /// including nested-arguments wire formats for code_execution_call,
    /// url_context_call, google_search_call and google_maps_call).
    #[test]
    fn step_roundtrip(step in arb_step()) {
        assert_value_roundtrip(&step)?;
    }

    /// Test that StepDelta roundtrips stably through JSON.
    #[test]
    fn step_delta_roundtrip(delta in arb_step_delta()) {
        assert_value_roundtrip(&delta)?;
    }

    /// Test that InteractionInput roundtrips stably through JSON.
    #[test]
    fn interaction_input_roundtrip(input in arb_interaction_input()) {
        assert_value_roundtrip(&input)?;

        // Text input must stay Text (arrays/objects never look like strings)
        if matches!(input, InteractionInput::Text(_)) {
            let json = serde_json::to_value(&input).unwrap();
            let restored: InteractionInput = serde_json::from_value(json).unwrap();
            prop_assert!(matches!(restored, InteractionInput::Text(_)));
        }
    }

    /// Test that Tool roundtrips stably through JSON.
    #[test]
    fn tool_roundtrip(tool in arb_tool()) {
        assert_value_roundtrip(&tool)?;
    }

    /// Test that InteractionResponse roundtrips correctly through JSON.
    ///
    /// This is the most comprehensive test, covering the full response structure.
    #[test]
    fn interaction_response_roundtrip(response in arb_interaction_response()) {
        let json = serde_json::to_value(&response).expect("Serialization should succeed");
        let restored: InteractionResponse =
            serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        // Compare key fields (InteractionResponse doesn't derive PartialEq)
        prop_assert_eq!(&response.id, &restored.id);
        prop_assert_eq!(&response.model, &restored.model);
        prop_assert_eq!(&response.agent, &restored.agent);
        prop_assert_eq!(&response.status, &restored.status);
        prop_assert_eq!(&response.usage, &restored.usage);
        prop_assert_eq!(&response.previous_interaction_id, &restored.previous_interaction_id);
        prop_assert_eq!(&response.environment_id, &restored.environment_id);
        prop_assert_eq!(&response.output_text, &restored.output_text);
        prop_assert_eq!(response.steps.len(), restored.steps.len());

        // Verify timestamps
        prop_assert_eq!(&response.created, &restored.created);
        prop_assert_eq!(&response.updated, &restored.updated);

        // Verify the full JSON roundtrip is stable (compare as Value for HashMap key order independence)
        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that StreamChunk roundtrips stably through JSON.
    #[test]
    fn stream_chunk_roundtrip(chunk in arb_stream_chunk()) {
        assert_value_roundtrip(&chunk)?;
    }
}

// =============================================================================
// Unknown Variant Preservation Tests (Evergreen)
// =============================================================================

#[cfg(not(feature = "strict-unknown"))]
proptest! {
    /// Test that unknown Step types are preserved through a roundtrip.
    #[test]
    fn step_unknown_preservation(step_type in arb_unknown_type(), data in arb_unknown_object()) {
        let step = Step::Unknown {
            step_type: step_type.clone(),
            data,
        };

        let json = serde_json::to_value(&step).expect("Serialization should succeed");
        let restored: Step = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_step_type(), Some(step_type.as_str()));
        prop_assert_eq!(restored.step_type(), step_type.as_str());

        // Full data is preserved (roundtrip stable)
        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that unknown Content types are preserved through a roundtrip.
    #[test]
    fn content_unknown_preservation(content_type in arb_unknown_type(), data in arb_unknown_object()) {
        let content = Content::Unknown {
            content_type: content_type.clone(),
            data,
        };

        let json = serde_json::to_value(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_content_type(), Some(content_type.as_str()));

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }
}

proptest! {
    /// Test that unknown StepDelta types are preserved through a roundtrip
    /// (StepDelta preserves unknowns even in strict mode).
    #[test]
    fn step_delta_unknown_preservation(delta_type in arb_unknown_type(), data in arb_unknown_object()) {
        let delta = StepDelta::Unknown {
            delta_type: delta_type.clone(),
            data,
        };

        let json = serde_json::to_value(&delta).expect("Serialization should succeed");
        let restored: StepDelta = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_delta_type(), Some(delta_type.as_str()));

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that unknown Annotation types are preserved through a roundtrip.
    #[test]
    fn annotation_unknown_preservation(annotation_type in arb_unknown_type(), data in arb_unknown_object()) {
        let annotation = Annotation::Unknown {
            annotation_type: annotation_type.clone(),
            data,
        };

        let json = serde_json::to_value(&annotation).expect("Serialization should succeed");
        let restored: Annotation = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_annotation_type(), Some(annotation_type.as_str()));

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
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
        assert_value_roundtrip(&content)?;
    }

    /// Test None text content is handled correctly.
    #[test]
    fn none_text_content_roundtrip(_unused in Just(())) {
        let content = Content::Text { text: None, annotations: None };
        assert_value_roundtrip(&content)?;
    }

    /// Test special characters in strings are handled correctly.
    #[test]
    fn special_chars_in_text(text in ".*[\n\r\t\"\\\\].*") {
        let content = Content::Text { text: Some(text), annotations: None };
        assert_value_roundtrip(&content)?;
    }

    /// Test Unicode in strings is handled correctly.
    #[test]
    fn unicode_in_text(text in ".*[\\u{1F600}-\\u{1F64F}].*") {
        let content = Content::Text { text: Some(text), annotations: None };
        assert_value_roundtrip(&content)?;
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

    /// Test an empty content array deserializes as Steps (documented gotcha:
    /// `InteractionInput::Content(vec![])` serializes to `[]`, which
    /// canonically deserializes as `Steps(vec![])`).
    #[test]
    fn empty_input_array_deserializes_as_steps(_unused in Just(())) {
        let input = InteractionInput::Content(vec![]);
        let json = serde_json::to_value(&input).expect("Serialization should succeed");
        prop_assert_eq!(&json, &serde_json::json!([]));

        let restored: InteractionInput =
            serde_json::from_value(json).expect("Deserialization should succeed");
        prop_assert_eq!(restored, InteractionInput::Steps(vec![]));
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

        let step = Step::FunctionCall {
            id: "call_123".to_string(),
            name: "deep_function".to_string(),
            arguments: nested_args,
        };

        assert_value_roundtrip(&step)?;
    }

    /// Test deeply nested JSON in a function result payload.
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

        let step = Step::FunctionResult {
            call_id: "call_123".to_string(),
            name: Some("deep_function".to_string()),
            result: FunctionResultPayload::Json(nested_result),
            is_error: None,
        };

        assert_value_roundtrip(&step)?;
    }
}
