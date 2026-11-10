//! Property tests: `InteractionResponse`, its usage and status, the stream chunks, and the
//! auto-function results.

use super::request::{
    arb_generation_config, arb_interaction_input, arb_response_format_spec, arb_service_tier,
};
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
        .prop_map(|(modality, tokens)| ModalityTokens::new(modality, tokens))
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
        .prop_map(|(tool_type, count)| GroundingToolCount {
            tool_type,
            count,
            extra: serde_json::Map::new(),
        })
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
        // The request config a stored interaction serves back, boxed: its
        // strategy trees are deep enough to overflow a test thread's stack
        // inline in the stream chunk strategies.
        proptest::option::of(arb_generation_config()).boxed(), // generation_config
        proptest::option::of(arb_response_format_spec()).boxed(), // response_format
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
            (id, model, agent, input, steps, status, usage, generation_config, response_format),
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
                generation_config,
                response_format,
                ..Default::default()
            }
        },
    )
}

// =============================================================================
// StreamChunk Strategy
// =============================================================================

/// A known chunk's `extra`: a few keys it does not read, any JSON each.
fn arb_chunk_extra() -> impl Strategy<Value = serde_json::Map<String, serde_json::Value>> {
    proptest::collection::btree_map("[a-z_]{1,12}", arb_json_value(), 0..3)
        .prop_map(|entries| entries.into_iter().collect())
}

pub(super) fn arb_stream_chunk() -> impl Strategy<Value = StreamChunk> {
    prop_oneof![
        // Created variant
        (arb_interaction_response(), arb_chunk_extra()).prop_map(|(interaction, extra)| {
            StreamChunk::Created {
                interaction: Box::new(interaction),
                extra,
            }
        }),
        // StatusUpdate variant
        (
            proptest::option::of(arb_identifier()),
            arb_interaction_status(),
            proptest::option::of(arb_usage_metadata()),
            proptest::option::of(arb_identifier()),
            arb_chunk_extra(),
        )
            .prop_map(
                |(interaction_id, status, usage, continuation_token, extra)| {
                    StreamChunk::StatusUpdate {
                        interaction_id,
                        status,
                        usage: usage.map(Box::new),
                        continuation_token,
                        extra,
                    }
                }
            ),
        // StepStart variant
        (any::<usize>(), arb_step(), arb_chunk_extra())
            .prop_map(|(index, step, extra)| { StreamChunk::StepStart { index, step, extra } }),
        // StepDelta variant
        (
            any::<usize>(),
            arb_step_delta(),
            proptest::option::of(arb_usage_metadata()),
            arb_chunk_extra(),
        )
            .prop_map(
                |(index, delta, total_usage, extra)| StreamChunk::StepDelta {
                    index,
                    delta,
                    total_usage: total_usage.map(Box::new),
                    extra,
                }
            ),
        // StepStop variant
        (
            any::<usize>(),
            proptest::option::of(arb_usage_metadata()),
            proptest::option::of(arb_usage_metadata()),
            arb_chunk_extra(),
        )
            .prop_map(|(index, usage, step_usage, extra)| StreamChunk::StepStop {
                index,
                usage: usage.map(Box::new),
                step_usage: step_usage.map(Box::new),
                extra,
            }),
        // Completed variant
        (arb_interaction_response(), arb_chunk_extra()).prop_map(|(r, extra)| {
            StreamChunk::Completed {
                interaction: Box::new(r),
                extra,
            }
        }),
        // Error variant
        (
            arb_text(),
            proptest::option::of(arb_identifier()),
            arb_chunk_extra(),
        )
            .prop_map(|(message, code, extra)| StreamChunk::Error {
                message,
                code,
                extra,
            }),
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
        (
            arb_step_delta(),
            prop::collection::btree_map("[a-z_]{1,8}", arb_json_value(), 0..3)
        )
            .prop_map(|(delta, extra)| AutoFunctionStreamChunk::Delta {
                delta,
                extra: extra.into_iter().collect(),
            }),
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
        // A passed-on SSE event can be named like a known chunk.
        (
            prop_oneof![
                arb_unknown_type(),
                prop::sample::select(vec![
                    "delta",
                    "executing_functions",
                    "function_results",
                    "complete",
                    "max_loops_reached",
                ])
                .prop_map(String::from),
            ],
            arb_json_value()
        )
            .prop_map(|(chunk_type, data)| {
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

/// `interaction_response_roundtrip`'s check, shared with the recorded seed
/// below.
fn assert_interaction_response_roundtrip(
    response: &InteractionResponse,
) -> Result<(), TestCaseError> {
    let json = serde_json::to_value(response).expect("Serialization should succeed");
    let restored: InteractionResponse =
        serde_json::from_value(json.clone()).expect("Deserialization should succeed");

    // Compare key fields (InteractionResponse doesn't derive PartialEq)
    prop_assert_eq!(&response.id, &restored.id);
    prop_assert_eq!(&response.model, &restored.model);
    prop_assert_eq!(&response.agent, &restored.agent);
    prop_assert_eq!(&response.status, &restored.status);
    prop_assert_eq!(&response.usage, &restored.usage);
    prop_assert_eq!(
        &response.previous_interaction_id,
        &restored.previous_interaction_id
    );
    prop_assert_eq!(&response.environment_id, &restored.environment_id);
    prop_assert_eq!(&response.output_text, &restored.output_text);
    prop_assert_eq!(response.steps.len(), restored.steps.len());

    // Verify timestamps
    prop_assert_eq!(&response.created, &restored.created);
    prop_assert_eq!(&response.updated, &restored.updated);

    // Verify the full JSON roundtrip is stable (compare as Value for HashMap key order independence)
    let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
    prop_assert_eq!(json, restored_json);
    Ok(())
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
        assert_interaction_response_roundtrip(&response)?;
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

// =============================================================================
// Recorded regressions
// =============================================================================
//
// A seed in `proptest-regressions/proptest_response.txt` replays whatever
// value the strategy now draws from it, which drifts as the strategies
// change. The value each seed shrank to when it was recorded is pinned here,
// so the regression stays covered whatever the strategy generates.
//
// Two seeds (`9fdf85c7`, `6fecc626`) were recorded on 2026-03-28 against the
// response model before API revision 2026-05-20, so their values are
// rebuilt in today's: `outputs` became `steps` (an unknown output block is
// a model output step's content, a `ThoughtSignature` a thought step's
// signature), content-shaped tool calls and results are steps, and the
// fields the revision removed (`grounding_metadata`,
// `url_context_metadata`, `total_reasoning_tokens`) have nowhere to go.
// Identifiers and short strings are verbatim; the strings past 70
// characters were proptest's random Unicode, hundreds to thousands of
// characters each, for which `RANDOM_UNICODE`, the first 120 characters of
// one of them, stands in.

/// The first 120 characters of the code execution result in seed
/// `9fdf85c7`.
#[cfg(not(genai_strict_unknown))]
const RANDOM_UNICODE: &str = "Ⱥ*�\r%%\u{106acf}ѨPѨÚ\u{202e}\u{12d9d}=\u{60fdb}\u{48be1}\
    \u{44101}\"\t{\u{9572b}/\u{f19d2}\"]𰭬\u{feff}*𣨴¥\u{3b643}\u{5f57f}<\u{edc9c}.%m𧫐/%\r\0={\
    𧃉.🕴\u{4}🕴/\u{b9754}\0{\u{625cf}\\D\u{10d4a4}\0\u{1b}\u{8f}\0\u{6}Ѩ&\u{1dd8}\u{48f8d}\
    \u{bd886}`\u{feff}`\"%귊\u{202e}²Ⱥ5a=\u{df123}\u{49c41}\u{6}u%?x:�:\u{ca65c}\0*`�'\u{89}\
    \u{11cc1}>\"/D'2k{\u{1a6c}🕴~\tpB\u{1aba3}:_🕴B漣\u{b}*C";

#[cfg(not(genai_strict_unknown))]
fn random(n: usize) -> String {
    format!("{RANDOM_UNICODE}{n}")
}

#[cfg(not(genai_strict_unknown))]
fn timestamp(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

/// Seed `9fdf85c7` (`interaction_response_roundtrip`): a cancelled agent
/// interaction with a code execution result as input, an unknown output
/// block holding `false`, an MCP server with random headers beside an
/// unknown tool holding an integer past 2^53, and an update time.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_cancelled_agent_response_roundtrips() {
    let response = InteractionResponse {
        agent: Some("A_bD".to_string()),
        input: Some(InteractionInput::Steps(vec![Step::CodeExecutionResult {
            call_id: "EH".to_string(),
            result: random(1).into(),
            is_error: false,
            signature: None,
            extra: Default::default(),
        }])),
        steps: vec![Step::model_output(vec![Content::Unknown {
            content_type: "ngzc834C2QzK93m_".to_string(),
            data: serde_json::Value::Bool(false),
        }])],
        status: InteractionStatus::Cancelled,
        tools: Some(vec![
            Tool::McpServer {
                name: "HgR_P3_M".to_string(),
                url: random(2),
                allowed_tools: None,
                headers: Some(std::collections::HashMap::from([
                    ("gfb__Wh".to_string(), "$Ⱥ\u{1b}?.�.\u{3}".to_string()),
                    ("KvCs1WI__t_K_F_lEh3".to_string(), random(3)),
                ])),
                extra: Default::default(),
            },
            Tool::Unknown {
                tool_type: "B_hjPaV_Y_jtXOUrTP".to_string(),
                data: serde_json::json!(1_289_167_811_881_697_062_u64),
            },
        ]),
        updated: Some(timestamp("2025-07-13T23:19:14Z")),
        ..Default::default()
    };
    assert_interaction_response_roundtrip(&response).unwrap();
}

/// Seed `6fecc626` (`stream_chunk_roundtrip`): the completed chunk of an
/// interaction awaiting action, with unknown content holding a float, an
/// object and null, a URL context call, a thought signature, usage counts
/// past `i32::MAX`, an MCP server with an allowlist, and an unknown tool
/// holding a negative float.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_requires_action_completed_chunk_roundtrips() {
    let response = InteractionResponse {
        agent: Some("__Q__u__mUI9_vC8LX0j_ql".to_string()),
        input: Some(InteractionInput::Content(vec![
            Content::Unknown {
                content_type: "_Q39_1NSR".to_string(),
                data: serde_json::json!(108_240_013.0),
            },
            Content::Unknown {
                content_type: "zytp_DSJSuaBtr4DaCb48oZc_b2Yw0W".to_string(),
                data: serde_json::json!({
                    "JvVP_8QALTp_uvsG6CdWB73A9y7E22F": "£/Ⱥ\rR\t@Ѩ1\t\u{470b8}/í\t\t\u{46f8b}<\u{e6b1f}\u{feff}Ѩ\u{d9c1b}.j%",
                    "_bdTit8_ySJXns_0732jF_0_": ".$\u{b}\u{60487}0\t�U\u{6385b}",
                    "b4_ekv6u8J_TkbC": "s",
                }),
            },
        ])),
        steps: vec![
            Step::model_output(vec![Content::Unknown {
                content_type: "G_1T_T_5XyxkZ__f1G5fZWn86".to_string(),
                data: serde_json::json!(-1_240_389_815.0),
            }]),
            Step::UrlContextCall {
                id: random(1),
                urls: vec![random(2), random(3)],
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::model_output(vec![Content::Unknown {
                content_type: "n_VYe7so_C2_0O_e_3".to_string(),
                data: serde_json::Value::Null,
            }]),
            Step::thought(random(4)),
        ],
        status: InteractionStatus::RequiresAction,
        usage: Some(UsageMetadata {
            total_output_tokens: Some(2_306_086_130),
            total_tokens: Some(265_066_474),
            total_cached_tokens: Some(479_620_274),
            total_thought_tokens: Some(1_088_037_244),
            total_tool_use_tokens: Some(3_577_001_627),
            ..Default::default()
        }),
        tools: Some(vec![
            Tool::McpServer {
                name: "UKQM7pSu1U__ea3W_yA_jd_F__".to_string(),
                url: random(5),
                allowed_tools: Some(vec![AllowedTools::new(vec![
                    "bWXVWKG_GUs_2QMEP4_iRhWZRr2V0".to_string(),
                ])]),
                headers: Some(std::collections::HashMap::from([
                    ("e_".to_string(), random(6)),
                    ("CX_I_8Jb__c_4___V3NBiPxSC_6L".to_string(), random(7)),
                ])),
                extra: Default::default(),
            },
            Tool::Unknown {
                tool_type: "AME2P5V_4Q_JzdFfGqI_iI329G4cz_F".to_string(),
                data: serde_json::json!(-60_500.337),
            },
        ]),
        previous_interaction_id: Some("_xnL_r_81Zj5_cK_gY2".to_string()),
        updated: Some(timestamp("2022-08-29T09:00:39Z")),
        ..Default::default()
    };
    assert_value_roundtrip(&StreamChunk::Completed {
        interaction: Box::new(response),
        extra: Default::default(),
    })
    .unwrap();
}

/// Seed `a34c4f88` (`stream_chunk_roundtrip` or
/// `auto_function_stream_chunk_roundtrip`, which both take a `chunk` with a
/// `Complete` arm when it was recorded): an otherwise empty response whose
/// status is unknown, in either stream's completed chunk.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_completed_chunk_with_unknown_status_roundtrips() {
    let response = InteractionResponse {
        status: InteractionStatus::Unknown {
            status_type: "zz_a".to_string(),
            data: serde_json::Value::String("zz_a".to_string()),
        },
        ..Default::default()
    };
    assert_value_roundtrip(&StreamChunk::Completed {
        interaction: Box::new(response.clone()),
        extra: Default::default(),
    })
    .unwrap();
    assert_value_roundtrip(&AutoFunctionStreamChunk::Complete(Box::new(response))).unwrap();
}
