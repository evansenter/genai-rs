//! Property-based tests for serialization roundtrips using proptest.
//!
//! These tests verify that `deserialize(serialize(x)) == x` for key streaming types,
//! catching edge cases that hand-written tests might miss.
//!
//! Note: Since `AutoFunctionResult` is `#[non_exhaustive]`, we can't construct it directly
//! from integration tests. Instead, we deserialize from JSON and verify roundtrip stability.
//!
//! ## Unknown Variant Tests
//!
//! Tests for Unknown variant preservation are gated with `#[cfg(not(feature = "strict-unknown"))]`
//! because the `strict-unknown` feature causes deserialization errors instead of creating
//! Unknown variants. The Unknown variant tests cover all 11 types with Unknown support:
//!
//! 1. Resolution, Content, StreamChunk, AutoFunctionStreamChunk
//! 2. Tool, FunctionCallingMode, FileState
//! 3. Role, ThinkingLevel, ThinkingSummaries, InteractionStatus

use chrono::{DateTime, TimeZone, Utc};
use genai_rs::{
    Annotation, AutoFunctionResult, AutoFunctionStreamChunk, Content, FunctionExecutionResult,
    InteractionResponse, InteractionStatus, ModalityTokens, PendingFunctionCall, UsageMetadata,
};
use proptest::prelude::*;

// Additional imports for Unknown variant tests (only used when strict-unknown is disabled)
#[cfg(not(feature = "strict-unknown"))]
use genai_rs::{
    FileState, FunctionCallingMode, Resolution, Role, StreamChunk, ThinkingLevel,
    ThinkingSummaries, Tool,
};
use std::time::Duration;

// =============================================================================
// Strategy Generators
// =============================================================================

/// Strategy for generating arbitrary serde_json::Value for function args/results.
fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::Value::Number(n.into())),
        ".*".prop_map(serde_json::Value::String),
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

/// Strategy for generating valid identifiers
fn arb_identifier() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,30}"
}

/// Strategy for generating text
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

// =============================================================================
// FunctionExecutionResult Strategies
// =============================================================================

fn arb_function_execution_result() -> impl Strategy<Value = FunctionExecutionResult> {
    (
        arb_identifier(),
        arb_identifier(),
        arb_json_value(),
        arb_json_value(),
        any::<u64>().prop_map(Duration::from_millis),
    )
        .prop_map(|(name, call_id, args, result, duration)| {
            FunctionExecutionResult::new(name, call_id, args, result, duration)
        })
}

fn arb_pending_function_call() -> impl Strategy<Value = PendingFunctionCall> {
    (arb_identifier(), arb_identifier(), arb_json_value())
        .prop_map(|(name, call_id, args)| PendingFunctionCall::new(&name, &call_id, args))
}

// =============================================================================
// InteractionStatus Strategy (simplified for use in responses)
// =============================================================================

fn arb_interaction_status() -> impl Strategy<Value = InteractionStatus> {
    prop_oneof![
        Just(InteractionStatus::Completed),
        Just(InteractionStatus::InProgress),
        Just(InteractionStatus::RequiresAction),
        Just(InteractionStatus::Failed),
        Just(InteractionStatus::Cancelled),
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
// Annotation Strategy
// =============================================================================

fn arb_annotation() -> impl Strategy<Value = Annotation> {
    (0usize..1000, 0usize..1000, proptest::option::of(".{0,100}"))
        .prop_map(|(start, len, source)| Annotation::new(start, start.saturating_add(len), source))
}

// =============================================================================
// Content Strategy (subset for streaming tests)
// =============================================================================

fn arb_interaction_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        // Text content with optional annotations
        (
            proptest::option::of(arb_text()),
            proptest::option::of(proptest::collection::vec(arb_annotation(), 0..3))
        )
            .prop_map(|(text, annotations)| Content::Text { text, annotations }),
        // Thought content (signature is a cryptographic value, not readable text)
        proptest::option::of(arb_text()).prop_map(|signature| Content::Thought { signature }),
        // FunctionCall content
        (
            proptest::option::of(arb_identifier()),
            arb_identifier(),
            arb_json_value(),
        )
            .prop_map(|(id, name, args)| {
                Content::FunctionCall {
                    id,
                    name,
                    args,
                    partial_args: None,
                    will_continue: None,
                }
            }),
    ]
}

// =============================================================================
// InteractionResponse Strategy (simplified for streaming tests)
// =============================================================================

fn arb_interaction_response() -> impl Strategy<Value = InteractionResponse> {
    (
        proptest::option::of(arb_identifier()),                 // id
        proptest::option::of(arb_identifier()),                 // model
        proptest::option::of(arb_identifier()),                 // agent
        prop::collection::vec(arb_interaction_content(), 0..3), // input
        prop::collection::vec(arb_interaction_content(), 0..5), // outputs
        arb_interaction_status(),                               // status
        proptest::option::of(arb_usage_metadata()),             // usage
        proptest::option::of(arb_identifier()),                 // previous_interaction_id
        proptest::option::of(arb_datetime()),                   // created
        proptest::option::of(arb_datetime()),                   // updated
    )
        .prop_map(
            |(
                id,
                model,
                agent,
                input,
                outputs,
                status,
                usage,
                previous_interaction_id,
                created,
                updated,
            )| {
                InteractionResponse {
                    id,
                    model,
                    agent,
                    input,
                    outputs,
                    status,
                    usage,
                    tools: None,
                    grounding_metadata: None,
                    url_context_metadata: None,
                    previous_interaction_id,
                    created,
                    updated,
                }
            },
        )
}

// =============================================================================
// AutoFunctionStreamChunk Strategy
// =============================================================================

fn arb_auto_function_stream_chunk() -> impl Strategy<Value = AutoFunctionStreamChunk> {
    prop_oneof![
        // Delta variant
        arb_interaction_content().prop_map(AutoFunctionStreamChunk::Delta),
        // ExecutingFunctions variant
        (
            arb_interaction_response(),
            prop::collection::vec(arb_pending_function_call(), 0..5)
        )
            .prop_map(|(response, pending_calls)| {
                AutoFunctionStreamChunk::ExecutingFunctions {
                    response,
                    pending_calls,
                }
            }),
        // FunctionResults variant
        prop::collection::vec(arb_function_execution_result(), 0..5)
            .prop_map(AutoFunctionStreamChunk::FunctionResults),
        // Complete variant
        arb_interaction_response().prop_map(AutoFunctionStreamChunk::Complete),
        // MaxLoopsReached variant
        arb_interaction_response().prop_map(AutoFunctionStreamChunk::MaxLoopsReached),
        // Unknown variant for forward compatibility
        (arb_identifier(), arb_json_value()).prop_map(|(chunk_type, data)| {
            AutoFunctionStreamChunk::Unknown { chunk_type, data }
        }),
    ]
}

// =============================================================================
// AutoFunctionResult Strategy
// =============================================================================

/// Strategy for generating AutoFunctionResult via JSON deserialization.
///
/// Since AutoFunctionResult is #[non_exhaustive], we can't construct it directly
/// from integration tests. Instead, we serialize components to JSON and deserialize.
fn arb_auto_function_result() -> impl Strategy<Value = AutoFunctionResult> {
    (
        arb_interaction_response(),
        prop::collection::vec(arb_function_execution_result(), 0..10),
        any::<bool>(),
    )
        .prop_map(|(response, executions, reached_max_loops)| {
            // Construct via JSON to work around #[non_exhaustive]
            let json = serde_json::json!({
                "response": serde_json::to_value(&response).unwrap(),
                "executions": serde_json::to_value(&executions).unwrap(),
                "reached_max_loops": reached_max_loops,
            });
            serde_json::from_value::<AutoFunctionResult>(json)
                .expect("AutoFunctionResult JSON construction should succeed")
        })
}

// =============================================================================
// Property Tests
// =============================================================================

proptest! {
    /// Test that FunctionExecutionResult roundtrips correctly through JSON.
    #[test]
    fn function_execution_result_roundtrip(result in arb_function_execution_result()) {
        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: FunctionExecutionResult = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(result, restored);
    }

    /// Test that AutoFunctionStreamChunk roundtrips correctly through JSON.
    ///
    /// Note: Since AutoFunctionStreamChunk contains types that don't derive PartialEq,
    /// we verify by re-serializing and comparing JSON strings.
    #[test]
    fn auto_function_stream_chunk_roundtrip(chunk in arb_auto_function_stream_chunk()) {
        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let restored: AutoFunctionStreamChunk = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Compare by re-serializing
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test that AutoFunctionResult roundtrips correctly through JSON.
    ///
    /// Note: We compare by re-serializing since nested types don't all derive PartialEq.
    #[test]
    fn auto_function_result_roundtrip(result in arb_auto_function_result()) {
        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: AutoFunctionResult = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Compare key fields
        prop_assert_eq!(&result.response.id, &restored.response.id);
        prop_assert_eq!(&result.response.model, &restored.response.model);
        prop_assert_eq!(&result.response.status, &restored.response.status);
        prop_assert_eq!(result.executions.len(), restored.executions.len());
        prop_assert_eq!(result.reached_max_loops, restored.reached_max_loops);

        // Verify executions
        for (orig, rest) in result.executions.iter().zip(restored.executions.iter()) {
            prop_assert_eq!(orig, rest);
        }

        // Compare by re-serializing for full verification
        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test Delta variant with various content types.
    #[test]
    fn delta_chunk_roundtrip(content in arb_interaction_content()) {
        let chunk = AutoFunctionStreamChunk::Delta(content);
        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let restored: AutoFunctionStreamChunk = serde_json::from_str(&json).expect("Deserialization should succeed");

        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test FunctionResults variant with multiple results.
    #[test]
    fn function_results_chunk_roundtrip(
        results in prop::collection::vec(arb_function_execution_result(), 0..10)
    ) {
        let chunk = AutoFunctionStreamChunk::FunctionResults(results);
        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let restored: AutoFunctionStreamChunk = serde_json::from_str(&json).expect("Deserialization should succeed");

        let restored_json = serde_json::to_string(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test various duration values in FunctionExecutionResult.
    #[test]
    fn function_execution_result_duration_roundtrip(millis in any::<u64>()) {
        let result = FunctionExecutionResult::new(
            "test_function",
            "call-123",
            serde_json::json!({}),
            serde_json::json!({"status": "ok"}),
            Duration::from_millis(millis),
        );

        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: FunctionExecutionResult = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Duration is serialized as milliseconds, so verify the roundtrip is correct
        prop_assert_eq!(result.duration.as_millis(), restored.duration.as_millis());
    }

    /// Test AutoFunctionResult with reached_max_loops = true.
    #[test]
    fn auto_function_result_max_loops_roundtrip(response in arb_interaction_response()) {
        // Construct via JSON to work around #[non_exhaustive]
        let json = serde_json::json!({
            "response": serde_json::to_value(&response).unwrap(),
            "executions": [],
            "reached_max_loops": true,
        });
        let result: AutoFunctionResult = serde_json::from_value(json).expect("Construction should succeed");

        let json_str = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: AutoFunctionResult = serde_json::from_str(&json_str).expect("Deserialization should succeed");

        prop_assert!(restored.reached_max_loops);
    }

    /// Test AutoFunctionResult with empty executions.
    #[test]
    fn auto_function_result_empty_executions_roundtrip(response in arb_interaction_response()) {
        // Construct via JSON to work around #[non_exhaustive]
        let json = serde_json::json!({
            "response": serde_json::to_value(&response).unwrap(),
            "executions": [],
            "reached_max_loops": false,
        });
        let result: AutoFunctionResult = serde_json::from_value(json).expect("Construction should succeed");

        let json_str = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: AutoFunctionResult = serde_json::from_str(&json_str).expect("Deserialization should succeed");

        prop_assert!(restored.executions.is_empty());
        prop_assert!(!restored.reached_max_loops);
    }

    /// Test Unknown variant preservation (forward compatibility).
    #[test]
    fn unknown_chunk_preservation(chunk_type in arb_identifier(), data in arb_json_value()) {
        let chunk = AutoFunctionStreamChunk::Unknown {
            chunk_type: chunk_type.clone(),
            data: data.clone(),
        };

        let json = serde_json::to_string(&chunk).expect("Serialization should succeed");
        let restored: AutoFunctionStreamChunk = serde_json::from_str(&json).expect("Deserialization should succeed");

        // Verify the Unknown variant is preserved
        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_chunk_type(), Some(chunk_type.as_str()));
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

proptest! {
    /// Test that valid identifier patterns in function names roundtrip correctly.
    #[test]
    fn valid_identifier_function_name_roundtrip(name in "[a-zA-Z_][a-zA-Z0-9_]*") {
        let result = FunctionExecutionResult::new(
            name,
            "call-1",
            serde_json::json!({}),
            serde_json::json!(null),
            Duration::from_millis(1),
        );

        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: FunctionExecutionResult = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(result, restored);
    }

    /// Test that very large JSON values in results are handled correctly.
    #[test]
    fn large_result_value(
        count in 0usize..100,
    ) {
        // Create a moderately large nested object
        let mut obj = serde_json::Map::new();
        for i in 0..count {
            obj.insert(format!("key_{}", i), serde_json::Value::String(format!("value_{}", i)));
        }
        let result_value = serde_json::Value::Object(obj);

        let result = FunctionExecutionResult::new(
            "big_result_function",
            "call-big",
            serde_json::json!({}),
            result_value,
            Duration::from_millis(500),
        );

        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: FunctionExecutionResult = serde_json::from_str(&json).expect("Deserialization should succeed");
        prop_assert_eq!(result, restored);
    }
}

// =============================================================================
// Unknown Variant Strategy Generators
// =============================================================================
//
// These strategies generate Unknown variants for all 11 types with Unknown support.
// Tests using these are gated with #[cfg(not(feature = "strict-unknown"))] because
// strict mode causes deserialization errors instead of creating Unknown variants.

/// Strategy for generating unknown type strings (simulating future API additions).
#[cfg(not(feature = "strict-unknown"))]
fn arb_unknown_type_string() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("future_type".to_string()),
        Just("experimental_feature".to_string()),
        Just("v2_content".to_string()),
        "[a-z_]{3,20}".prop_map(|s| format!("future_{}", s)),
    ]
}

/// Strategy for generating Resolution values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_resolution_with_unknown() -> impl Strategy<Value = Resolution> {
    prop_oneof![
        Just(Resolution::Low),
        Just(Resolution::Medium),
        Just(Resolution::High),
        Just(Resolution::UltraHigh),
        // Unknown variant via JSON deserialization
        arb_unknown_type_string().prop_map(|type_str| {
            serde_json::from_value::<Resolution>(serde_json::json!(type_str))
                .expect("Unknown resolution should deserialize")
        }),
    ]
}

/// Strategy for generating Role values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_role_with_unknown() -> impl Strategy<Value = Role> {
    prop_oneof![
        Just(Role::User),
        Just(Role::Model),
        // Unknown variant via JSON deserialization
        arb_unknown_type_string().prop_map(|type_str| {
            serde_json::from_value::<Role>(serde_json::json!(type_str))
                .expect("Unknown role should deserialize")
        }),
    ]
}

/// Strategy for generating ThinkingLevel values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_thinking_level_with_unknown() -> impl Strategy<Value = ThinkingLevel> {
    prop_oneof![
        Just(ThinkingLevel::Minimal),
        Just(ThinkingLevel::Low),
        Just(ThinkingLevel::Medium),
        Just(ThinkingLevel::High),
        // Unknown variant via JSON deserialization
        arb_unknown_type_string().prop_map(|type_str| {
            serde_json::from_value::<ThinkingLevel>(serde_json::json!(type_str))
                .expect("Unknown thinking level should deserialize")
        }),
    ]
}

/// Strategy for generating ThinkingSummaries values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_thinking_summaries_with_unknown() -> impl Strategy<Value = ThinkingSummaries> {
    prop_oneof![
        Just(ThinkingSummaries::Auto),
        Just(ThinkingSummaries::None),
        // Unknown variant via JSON deserialization (uses SCREAMING_CASE)
        arb_unknown_type_string().prop_map(|type_str| {
            let screaming = type_str.to_uppercase();
            serde_json::from_value::<ThinkingSummaries>(serde_json::json!(screaming))
                .expect("Unknown thinking summaries should deserialize")
        }),
    ]
}

/// Strategy for generating FunctionCallingMode values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_function_calling_mode_with_unknown() -> impl Strategy<Value = FunctionCallingMode> {
    prop_oneof![
        Just(FunctionCallingMode::Auto),
        Just(FunctionCallingMode::Any),
        Just(FunctionCallingMode::None),
        Just(FunctionCallingMode::Validated),
        // Unknown variant via JSON deserialization (uses SCREAMING_CASE)
        arb_unknown_type_string().prop_map(|type_str| {
            let screaming = type_str.to_uppercase();
            serde_json::from_value::<FunctionCallingMode>(serde_json::json!(screaming))
                .expect("Unknown function calling mode should deserialize")
        }),
    ]
}

/// Strategy for generating InteractionStatus values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_interaction_status_with_unknown() -> impl Strategy<Value = InteractionStatus> {
    prop_oneof![
        Just(InteractionStatus::Completed),
        Just(InteractionStatus::InProgress),
        Just(InteractionStatus::RequiresAction),
        Just(InteractionStatus::Failed),
        Just(InteractionStatus::Cancelled),
        // Unknown variant via JSON deserialization
        arb_unknown_type_string().prop_map(|type_str| {
            serde_json::from_value::<InteractionStatus>(serde_json::json!(type_str))
                .expect("Unknown interaction status should deserialize")
        }),
    ]
}

/// Strategy for generating FileState values including Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_file_state_with_unknown() -> impl Strategy<Value = FileState> {
    prop_oneof![
        Just(FileState::Processing),
        Just(FileState::Active),
        Just(FileState::Failed),
        // Unknown variant via JSON deserialization (uses SCREAMING_CASE)
        arb_unknown_type_string().prop_map(|type_str| {
            let screaming = format!("STATE_{}", type_str.to_uppercase());
            serde_json::from_value::<FileState>(serde_json::json!(screaming))
                .expect("Unknown file state should deserialize")
        }),
    ]
}

/// Strategy for generating Content Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_interaction_content_unknown() -> impl Strategy<Value = Content> {
    (arb_unknown_type_string(), arb_json_value()).prop_map(|(type_str, extra_data)| {
        let json = serde_json::json!({
            "type": type_str,
            "extra_field": extra_data,
        });
        serde_json::from_value::<Content>(json)
            .expect("Unknown interaction content should deserialize")
    })
}

/// Strategy for generating StreamChunk Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_stream_chunk_unknown() -> impl Strategy<Value = StreamChunk> {
    (arb_unknown_type_string(), arb_json_value()).prop_map(|(type_str, data)| {
        let json = serde_json::json!({
            "chunk_type": type_str,
            "data": data,
        });
        serde_json::from_value::<StreamChunk>(json)
            .expect("Unknown stream chunk should deserialize")
    })
}

/// Strategy for generating Tool Unknown variant.
#[cfg(not(feature = "strict-unknown"))]
fn arb_tool_unknown() -> impl Strategy<Value = Tool> {
    (arb_unknown_type_string(), arb_json_value()).prop_map(|(type_str, config)| {
        let json = serde_json::json!({
            "type": type_str,
            "config": config,
        });
        serde_json::from_value::<Tool>(json).expect("Unknown tool should deserialize")
    })
}

// =============================================================================
// Unknown Variant Roundtrip Tests
// =============================================================================
//
// These tests verify that Unknown variants roundtrip correctly through
// JSON serialization/deserialization. They are skipped when strict-unknown
// feature is enabled because strict mode errors instead of creating Unknown.

#[cfg(not(feature = "strict-unknown"))]
proptest! {
    /// Test Resolution Unknown variant roundtrip.
    #[test]
    fn resolution_unknown_roundtrip(resolution in arb_resolution_with_unknown()) {
        let json = serde_json::to_value(&resolution).expect("Serialization should succeed");
        let restored: Resolution = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        // Verify Unknown preservation
        if resolution.is_unknown() {
            prop_assert!(restored.is_unknown(), "Unknown variant should be preserved");
            prop_assert_eq!(
                resolution.unknown_resolution_type(),
                restored.unknown_resolution_type(),
                "Unknown type string should match"
            );
        }

        // Verify roundtrip by re-serializing
        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test Role Unknown variant roundtrip.
    #[test]
    fn role_unknown_roundtrip(role in arb_role_with_unknown()) {
        let json = serde_json::to_value(&role).expect("Serialization should succeed");
        let restored: Role = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        if role.is_unknown() {
            prop_assert!(restored.is_unknown());
            prop_assert_eq!(role.unknown_role_type(), restored.unknown_role_type());
        }

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test ThinkingLevel Unknown variant roundtrip.
    #[test]
    fn thinking_level_unknown_roundtrip(level in arb_thinking_level_with_unknown()) {
        let json = serde_json::to_value(&level).expect("Serialization should succeed");
        let restored: ThinkingLevel = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        if level.is_unknown() {
            prop_assert!(restored.is_unknown());
            prop_assert_eq!(level.unknown_level_type(), restored.unknown_level_type());
        }

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test ThinkingSummaries Unknown variant roundtrip.
    #[test]
    fn thinking_summaries_unknown_roundtrip(summaries in arb_thinking_summaries_with_unknown()) {
        let json = serde_json::to_value(&summaries).expect("Serialization should succeed");
        let restored: ThinkingSummaries = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        if summaries.is_unknown() {
            prop_assert!(restored.is_unknown());
            prop_assert_eq!(summaries.unknown_summaries_type(), restored.unknown_summaries_type());
        }

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test FunctionCallingMode Unknown variant roundtrip.
    #[test]
    fn function_calling_mode_unknown_roundtrip(mode in arb_function_calling_mode_with_unknown()) {
        let json = serde_json::to_value(&mode).expect("Serialization should succeed");
        let restored: FunctionCallingMode = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        if mode.is_unknown() {
            prop_assert!(restored.is_unknown());
            prop_assert_eq!(mode.unknown_mode_type(), restored.unknown_mode_type());
        }

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test InteractionStatus Unknown variant roundtrip.
    #[test]
    fn interaction_status_unknown_roundtrip(status in arb_interaction_status_with_unknown()) {
        let json = serde_json::to_value(&status).expect("Serialization should succeed");
        let restored: InteractionStatus = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        if status.is_unknown() {
            prop_assert!(restored.is_unknown());
            prop_assert_eq!(status.unknown_status_type(), restored.unknown_status_type());
        }

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test FileState Unknown variant roundtrip.
    #[test]
    fn file_state_unknown_roundtrip(state in arb_file_state_with_unknown()) {
        let json = serde_json::to_value(&state).expect("Serialization should succeed");
        let restored: FileState = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        if state.is_unknown() {
            prop_assert!(restored.is_unknown());
            prop_assert_eq!(state.unknown_state_type(), restored.unknown_state_type());
        }

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test Content Unknown variant roundtrip.
    #[test]
    fn interaction_content_unknown_roundtrip(content in arb_interaction_content_unknown()) {
        let json = serde_json::to_value(&content).expect("Serialization should succeed");
        let restored: Content = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(content.is_unknown(), "Generated content should be Unknown");
        prop_assert!(restored.is_unknown(), "Restored content should be Unknown");
        prop_assert_eq!(content.unknown_content_type(), restored.unknown_content_type());

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test StreamChunk Unknown variant roundtrip.
    #[test]
    fn stream_chunk_unknown_roundtrip(chunk in arb_stream_chunk_unknown()) {
        let json = serde_json::to_value(&chunk).expect("Serialization should succeed");
        let restored: StreamChunk = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(chunk.is_unknown(), "Generated chunk should be Unknown");
        prop_assert!(restored.is_unknown(), "Restored chunk should be Unknown");
        prop_assert_eq!(chunk.unknown_chunk_type(), restored.unknown_chunk_type());

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test Tool Unknown variant roundtrip.
    #[test]
    fn tool_unknown_roundtrip(tool in arb_tool_unknown()) {
        let json = serde_json::to_value(&tool).expect("Serialization should succeed");
        let restored: Tool = serde_json::from_value(json.clone()).expect("Deserialization should succeed");

        prop_assert!(tool.is_unknown(), "Generated tool should be Unknown");
        prop_assert!(restored.is_unknown(), "Restored tool should be Unknown");
        prop_assert_eq!(tool.unknown_tool_type(), restored.unknown_tool_type());

        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// Test AutoFunctionStreamChunk Unknown variant data preservation.
    ///
    /// This test verifies that Unknown variants preserve all original data through
    /// serialization roundtrips, not just the type string.
    #[test]
    fn auto_function_stream_chunk_unknown_data_preservation(
        chunk_type in arb_unknown_type_string(),
        data in arb_json_value()
    ) {
        let chunk = AutoFunctionStreamChunk::Unknown {
            chunk_type: chunk_type.clone(),
            data: data.clone(),
        };

        let json = serde_json::to_value(&chunk).expect("Serialization should succeed");
        let restored: AutoFunctionStreamChunk = serde_json::from_value(json.clone())
            .expect("Deserialization should succeed");

        prop_assert!(restored.is_unknown());
        prop_assert_eq!(restored.unknown_chunk_type(), Some(chunk_type.as_str()));

        // Verify full data preservation via re-serialization
        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }
}
