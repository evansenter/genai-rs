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
    Annotation, CodeExecutionLanguage, Content, GoogleMapsResultItem, GoogleSearchResultItem,
    Place, Resolution, ReviewSnippet, UrlContextResultItem, VideoProcessing,
};
use super::environments::EnvironmentStatus;
use super::environments::{
    AllowlistEntry, EnvironmentSource, EnvironmentSpec, NetworkConfig, RemoteEnvironment,
    SourceType,
};
use super::request::{
    AgentConfig, DeepResearchConfig, DynamicConfig, GenerationConfig, ImageAspectRatio,
    ImageConfig, ImageSize, InteractionInput, ServiceTier, SpeechConfig, ThinkingLevel,
    ThinkingSummaries, TranscriptionConfig, TurnContent, VideoConfig, VideoTask, Visualization,
};
use super::response::{
    GroundingToolCount, InteractionResponse, InteractionStatus, ModalityTokens,
    OwnedFunctionCallInfo, UsageMetadata,
};
use super::response_format::{ResponseDelivery, ResponseFormat, ResponseFormatSpec};
use super::safety::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};
use super::steps::{CodeExecutionOutput, FunctionResultPayload, Step, StepDelta, StepError};
use super::streaming::{AutoFunctionResult, AutoFunctionStreamChunk, FunctionExecutionResult};
use super::tools::{
    AllowedTools, ExaAiSearchConfig, FunctionCallingMode, FunctionParameters, HybridSearchConfig,
    ParallelAiSearchConfig, RagFilter, RagRanking, RagResource, RagRetrievalConfig, RagStoreConfig,
    RetrievalType, SearchType, Tool, ToolChoice, VertexAiSearchConfig,
};
use super::triggers::{TriggerExecutionStatus, TriggerStatus};
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

// =============================================================================
// String Enum Strategies
// =============================================================================
//
// Their Unknown arms go through `unknown_or`: under `genai_strict_unknown` an
// unknown value fails to deserialize, so the arm yields a known value instead.

/// The Unknown arm of a string-enum strategy (see above).
fn unknown_or<T: Clone + std::fmt::Debug + 'static>(
    known: T,
    unknown: impl Fn(String) -> T + 'static,
) -> BoxedStrategy<T> {
    #[cfg(not(genai_strict_unknown))]
    {
        let _ = known;
        arb_unknown_type().prop_map(unknown).boxed()
    }
    #[cfg(genai_strict_unknown)]
    {
        let _ = unknown;
        Just(known).boxed()
    }
}

// One file per feature, beside this one (`src/proptest_<feature>.rs`); the
// strategies are `pub(super)` so each file can build on the others'.
#[path = "proptest_content.rs"]
mod content;
#[path = "proptest_request.rs"]
mod request;
#[path = "proptest_resources.rs"]
mod resources;
#[path = "proptest_response.rs"]
mod response;
#[path = "proptest_steps.rs"]
mod steps;
#[path = "proptest_tools.rs"]
mod tools;
#[path = "proptest_unknown_fields.rs"]
mod unknown_fields;

// The request side of the coverage oracle samples these
// (`src/coverage_oracle/request/`).
pub(crate) use request::{
    arb_generation_config, arb_response_format, arb_response_format_spec, arb_safety_setting,
    arb_speech_config, arb_transcription_config,
};
pub(crate) use resources::{arb_environment_spec, arb_webhook_config};
pub(crate) use tools::arb_tool_choice;
