//! Property tests: `InteractionResponse`, its usage and status, the stream chunks, and the
//! auto-function results.

use super::request::{arb_interaction_input, arb_service_tier};
use super::resources::arb_webhook_config;
use super::steps::{arb_step, arb_step_delta};
use super::tools::arb_tool;
use super::*;

/// Strategy for InteractionStatus (all variants including Unknown).
pub(super) fn arb_interaction_status() -> impl Strategy<Value = InteractionStatus> {
    prop_oneof![
        Just(InteractionStatus::Completed),
        Just(InteractionStatus::InProgress),
        Just(InteractionStatus::RequiresAction),
        Just(InteractionStatus::Failed),
        Just(InteractionStatus::Cancelled),
        Just(InteractionStatus::Incomplete),
        Just(InteractionStatus::BudgetExceeded),
        Just(InteractionStatus::RequiresContinuation),
        Just(InteractionStatus::Queued),
        // Unknown variant with preserved data
        unknown_or(InteractionStatus::Completed, |status_type| {
            InteractionStatus::Unknown {
                status_type: status_type.clone(),
                data: serde_json::Value::String(status_type),
            }
        }),
    ]
}

// =============================================================================
// UsageMetadata Strategies
// =============================================================================

/// Strategy for generating a single ModalityTokens value.
pub(super) fn arb_modality_tokens() -> impl Strategy<Value = ModalityTokens> {
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
pub(super) fn arb_modality_tokens_vec() -> impl Strategy<Value = Option<Vec<ModalityTokens>>> {
    proptest::option::of(prop::collection::vec(arb_modality_tokens(), 0..4))
}

/// Strategy for generating GroundingToolCount values.
pub(super) fn arb_grounding_tool_count() -> impl Strategy<Value = GroundingToolCount> {
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

pub(super) fn arb_usage_metadata() -> impl Strategy<Value = UsageMetadata> {
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
                    ..Default::default()
                }
            },
        )
}

// =============================================================================
// OwnedFunctionCallInfo Strategy
// =============================================================================

pub(super) fn arb_owned_function_call_info() -> impl Strategy<Value = OwnedFunctionCallInfo> {
    (arb_identifier(), arb_identifier(), arb_json_value())
        .prop_map(|(id, name, args)| OwnedFunctionCallInfo { id, name, args })
}

// =============================================================================
// InteractionResponse Strategy
// =============================================================================

pub(super) fn arb_interaction_response() -> impl Strategy<Value = InteractionResponse> {
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
        proptest::option::of(Just("interaction".to_string())),         // object
        proptest::option::of(arb_service_tier()),                      // service_tier
        proptest::option::of(arb_webhook_config()),                    // webhook_config
        proptest::option::of(arb_text()),                              // output_text
        proptest::option::of(arb_datetime()),                          // created
        proptest::option::of(arb_datetime()),                          // updated
    );

    (part1, part2).prop_map(
        |(
            (id, model, agent, input, steps, status, usage),
            (
                tools,
                previous_interaction_id,
                environment_id,
                object,
                service_tier,
                webhook_config,
                output_text,
                created,
                updated,
            ),
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
                object,
                service_tier,
                webhook_config,
                output_text,
                created,
                updated,
                ..Default::default()
            }
        },
    )
}

// =============================================================================
// StreamChunk Strategy
// =============================================================================

pub(super) fn arb_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    prop_oneof![
        // Created variant
        arb_interaction_response().prop_map(|interaction| StreamChunk::Created {
            interaction: Box::new(interaction)
        }),
        // StatusUpdate variant
        (
            proptest::option::of(arb_identifier()),
            arb_interaction_status(),
            proptest::option::of(arb_usage_metadata()),
            proptest::option::of(arb_identifier()),
        )
            .prop_map(|(interaction_id, status, usage, continuation_token)| {
                StreamChunk::StatusUpdate {
                    interaction_id,
                    status,
                    usage: usage.map(Box::new),
                    continuation_token,
                }
            }),
        // StepStart variant
        (any::<usize>(), arb_step())
            .prop_map(|(index, step)| StreamChunk::StepStart { index, step }),
        // StepDelta variant
        (
            any::<usize>(),
            arb_step_delta(),
            proptest::option::of(arb_usage_metadata())
        )
            .prop_map(|(index, delta, total_usage)| StreamChunk::StepDelta {
                index,
                delta,
                total_usage: total_usage.map(Box::new)
            }),
        // StepStop variant
        (
            any::<usize>(),
            proptest::option::of(arb_usage_metadata()),
            proptest::option::of(arb_usage_metadata()),
        )
            .prop_map(|(index, usage, step_usage)| StreamChunk::StepStop {
                index,
                usage: usage.map(Box::new),
                step_usage: step_usage.map(Box::new),
            }),
        // Completed variant
        arb_interaction_response().prop_map(|r| StreamChunk::Completed(Box::new(r))),
        // Error variant
        (arb_text(), proptest::option::of(arb_identifier()))
            .prop_map(|(message, code)| { StreamChunk::Error { message, code } }),
        // Unknown variant for forward compatibility
        (arb_unknown_type(), arb_json_value())
            .prop_map(|(chunk_type, data)| StreamChunk::Unknown { chunk_type, data }),
    ]
}

// =============================================================================
// Auto-Function Types
// =============================================================================
//
// Integration tests used to cover these with their own copies of the
// strategies above; they live here so there is one set to keep current.

pub(super) fn arb_function_execution_result() -> impl Strategy<Value = FunctionExecutionResult> {
    (
        arb_identifier(),
        arb_identifier(),
        arb_json_value(),
        arb_json_value(),
        any::<u64>(),
    )
        .prop_map(|(name, call_id, args, result, millis)| {
            FunctionExecutionResult::new(
                name,
                call_id,
                args,
                result,
                std::time::Duration::from_millis(millis),
            )
        })
}

pub(super) fn arb_auto_function_stream_chunk() -> impl Strategy<Value = AutoFunctionStreamChunk> {
    prop_oneof![
        arb_step_delta().prop_map(AutoFunctionStreamChunk::Delta),
        (
            arb_interaction_response(),
            prop::collection::vec(arb_owned_function_call_info(), 0..5)
        )
            .prop_map(|(response, pending_calls)| {
                AutoFunctionStreamChunk::ExecutingFunctions {
                    response: Box::new(response),
                    pending_calls,
                }
            }),
        prop::collection::vec(arb_function_execution_result(), 0..5)
            .prop_map(AutoFunctionStreamChunk::FunctionResults),
        arb_interaction_response().prop_map(|r| AutoFunctionStreamChunk::Complete(Box::new(r))),
        arb_interaction_response()
            .prop_map(|r| AutoFunctionStreamChunk::MaxLoopsReached(Box::new(r))),
        (arb_unknown_type(), arb_json_value()).prop_map(|(chunk_type, data)| {
            AutoFunctionStreamChunk::Unknown { chunk_type, data }
        }),
    ]
}

pub(super) fn arb_auto_function_result() -> impl Strategy<Value = AutoFunctionResult> {
    (
        arb_interaction_response(),
        prop::collection::vec(arb_function_execution_result(), 0..10),
        any::<bool>(),
    )
        .prop_map(
            |(response, executions, reached_max_loops)| AutoFunctionResult {
                response,
                executions,
                reached_max_loops,
            },
        )
}

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

    #[test]
    fn function_execution_result_roundtrip(result in arb_function_execution_result()) {
        let json = serde_json::to_string(&result).expect("Serialization should succeed");
        let restored: FunctionExecutionResult =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        // Duration travels as milliseconds, which the strategy generates.
        prop_assert_eq!(result, restored);
    }

    #[test]
    fn auto_function_stream_chunk_roundtrip(chunk in arb_auto_function_stream_chunk()) {
        assert_value_roundtrip(&chunk)?;
    }

    #[test]
    fn auto_function_result_roundtrip(result in arb_auto_function_result()) {
        let json = serde_json::to_value(&result).expect("Serialization should succeed");
        let restored: AutoFunctionResult =
            serde_json::from_value(json.clone()).expect("Deserialization should succeed");
        prop_assert_eq!(result.reached_max_loops, restored.reached_max_loops);
        prop_assert_eq!(&result.executions, &restored.executions);
        let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
        prop_assert_eq!(json, restored_json);
    }

    /// `reached_max_loops` is `#[serde(default)]`: payloads written before the
    /// field existed deserialize as `false`.
    #[test]
    fn auto_function_result_defaults_reached_max_loops(response in arb_interaction_response()) {
        let json = serde_json::json!({
            "response": serde_json::to_value(&response).unwrap(),
            "executions": [],
        });
        let restored: AutoFunctionResult =
            serde_json::from_value(json).expect("Deserialization should succeed");
        prop_assert!(!restored.reached_max_loops);
    }
}
