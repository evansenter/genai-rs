//! Property tests: `Step`, `StepDelta` and function result payloads.

use super::content::{
    arb_annotation, arb_code_execution_language, arb_content, arb_google_maps_result_item,
    arb_google_search_result_item, arb_resolution, arb_url_context_result_item,
};
use super::tools::{arb_retrieval_type, arb_search_type};
use super::*;

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
pub(super) fn arb_function_result_payload() -> impl Strategy<Value = FunctionResultPayload> {
    prop_oneof![
        arb_text().prop_map(FunctionResultPayload::Text),
        arb_non_string_json_value().prop_map(FunctionResultPayload::Json),
        prop::collection::vec(arb_content(), 1..3).prop_map(FunctionResultPayload::Contents),
    ]
}

// =============================================================================
// Step Strategies (every known variant + Unknown)
// =============================================================================

pub(super) fn arb_step_error() -> impl Strategy<Value = StepError> {
    (
        proptest::option::of(any::<i64>()),
        proptest::option::of(arb_text()),
        proptest::option::of(prop::collection::vec(arb_json_value(), 0..2)),
    )
        .prop_map(|(code, message, details)| StepError {
            code,
            message,
            details,
            extra: Default::default(),
        })
}

/// Helper to create the known Step variants (used by both strict and non-strict modes).
pub(super) fn arb_known_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        // user_input
        prop::collection::vec(arb_content(), 0..3).prop_map(|content| Step::UserInput {
            content,
            extra: Default::default()
        }),
        // model_output
        (
            prop::collection::vec(arb_content(), 0..3),
            proptest::option::of(arb_step_error()),
        )
            .prop_map(|(content, error)| Step::ModelOutput {
                content,
                error,
                extra: Default::default()
            }),
        // thought
        (
            proptest::option::of(arb_text()),
            prop::collection::vec(arb_content(), 0..2),
        )
            .prop_map(|(signature, summary)| Step::Thought {
                signature,
                summary,
                extra: Default::default()
            }),
        // function_call
        (
            arb_identifier(),
            arb_identifier(),
            arb_json_value(),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(id, name, arguments, signature)| Step::FunctionCall {
                id,
                name,
                arguments,
                signature,
                extra: Default::default(),
            }),
        // function_result
        (
            arb_identifier(),
            proptest::option::of(arb_identifier()),
            arb_function_result_payload(),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(call_id, name, result, is_error, signature)| {
                Step::FunctionResult {
                    call_id,
                    name,
                    result,
                    is_error,
                    signature,
                    extra: Default::default(),
                }
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
                arguments_extra: Default::default(),
                extra: Default::default(),
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
                    extra: Default::default(),
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
                signature,
                arguments_extra: Default::default(),
                extra: Default::default()
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
                    extra: Default::default(),
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
                    arguments_extra: Default::default(),
                    extra: Default::default(),
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
                    extra: Default::default(),
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
                    extra: Default::default(),
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
                    extra: Default::default(),
                }
            ),
        // file_search_call
        (arb_identifier(), proptest::option::of(arb_text())).prop_map(|(id, signature)| {
            Step::FileSearchCall {
                id,
                signature,
                extra: Default::default(),
            }
        }),
        // tool_call — the generic server-side call the API emits for MCP.
        // `prop_oneof!` is not exhaustiveness-checked, so a new variant is
        // silently absent from the roundtrip proptest unless added here.
        (arb_identifier(), proptest::option::of(arb_text())).prop_map(|(id, signature)| {
            Step::ToolCall {
                id,
                signature,
                extra: Default::default(),
            }
        }),
        // file_search_result (no result: none in the bindings or served)
        (arb_identifier(), proptest::option::of(arb_text())).prop_map(|(call_id, signature)| {
            Step::FileSearchResult {
                call_id,
                signature,
                extra: Default::default(),
            }
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
                signature,
                arguments_extra: Default::default(),
                extra: Default::default()
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
                extra: Default::default(),
            }),
        // processing_call / processing_result
        (arb_identifier(), proptest::option::of(arb_text())).prop_map(|(id, signature)| {
            Step::ProcessingCall {
                id,
                signature,
                extra: Default::default(),
            }
        }),
        (arb_identifier(), proptest::option::of(arb_text())).prop_map(|(call_id, signature)| {
            Step::ProcessingResult {
                call_id,
                signature,
                extra: Default::default(),
            }
        }),
        // retrieval_call (wire: nested arguments {queries}) / retrieval_result
        (
            arb_identifier(),
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_retrieval_type()),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(id, queries, retrieval_type, signature)| Step::RetrievalCall {
                    id,
                    queries,
                    retrieval_type,
                    signature,
                    arguments_extra: Default::default(),
                    extra: Default::default(),
                }
            ),
        (
            arb_identifier(),
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(call_id, is_error, signature)| Step::RetrievalResult {
                call_id,
                is_error,
                signature,
                extra: Default::default(),
            }),
    ]
}

/// Strategy for known Step variants only.
/// Used when strict mode is enabled (Unknown steps fail to deserialize in strict mode).
#[cfg(genai_strict_unknown)]
pub(super) fn arb_step() -> impl Strategy<Value = Step> {
    arb_known_step()
}

/// Strategy for all Step variants including Unknown.
#[cfg(not(genai_strict_unknown))]
pub(super) fn arb_step() -> impl Strategy<Value = Step> {
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

pub(super) fn arb_step_delta() -> impl Strategy<Value = StepDelta> {
    prop_oneof![
        // text
        arb_text().prop_map(|text| StepDelta::Text {
            text,
            extra: Default::default()
        }),
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
                extra: Default::default(),
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
                    extra: Default::default(),
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
                extra: Default::default(),
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
                mime_type,
                extra: Default::default()
            }),
        // thought_summary
        proptest::option::of(arb_content()).prop_map(|content| StepDelta::ThoughtSummary {
            content: content.map(Box::new),
            extra: Default::default()
        }),
        // thought_signature
        proptest::option::of(arb_text()).prop_map(|signature| StepDelta::ThoughtSignature {
            signature,
            extra: Default::default()
        }),
        // text_annotation_delta
        prop::collection::vec(arb_annotation(), 0..3).prop_map(|annotations| {
            StepDelta::TextAnnotation {
                annotations,
                extra: Default::default(),
            }
        }),
        // arguments_delta (streaming function-call args)
        arb_text().prop_map(|arguments| StepDelta::ArgumentsDelta {
            arguments,
            extra: Default::default()
        }),
        // function_result
        (
            arb_identifier(),
            proptest::option::of(arb_identifier()),
            arb_function_result_payload(),
            proptest::option::of(proptest::bool::ANY),
        )
            .prop_map(
                |(call_id, name, result, is_error)| StepDelta::FunctionResult {
                    call_id: Some(call_id),
                    name,
                    result,
                    is_error,
                    extra: Default::default(),
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
                arguments_extra: Default::default(),
                extra: Default::default(),
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
                    extra: Default::default(),
                }
            ),
        // url_context_call
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(urls, signature)| StepDelta::UrlContextCall {
                urls,
                signature,
                arguments_extra: Default::default(),
                extra: Default::default()
            }),
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
                    extra: Default::default(),
                }
            ),
        // google_search_call
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(queries, signature)| StepDelta::GoogleSearchCall {
                queries,
                signature,
                arguments_extra: Default::default(),
                extra: Default::default()
            }),
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
                    extra: Default::default(),
                }
            ),
        // mcp_server_tool_call
        (arb_identifier(), arb_identifier(), arb_json_value()).prop_map(
            |(name, server_name, arguments)| StepDelta::McpServerToolCall {
                name,
                server_name,
                arguments,
                extra: Default::default(),
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
                    extra: Default::default(),
                }
            ),
        // file_search_call
        proptest::option::of(arb_text()).prop_map(|signature| StepDelta::FileSearchCall {
            signature,
            extra: Default::default()
        }),
        // file_search_result (opaque result objects)
        (
            prop::collection::vec(
                arb_text().prop_map(|text| serde_json::json!({ "text": text })),
                0..3
            ),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(result, signature)| StepDelta::FileSearchResult {
                result,
                signature,
                extra: Default::default()
            }),
        // google_maps_call
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(queries, signature)| StepDelta::GoogleMapsCall {
                queries,
                signature,
                arguments_extra: Default::default(),
                extra: Default::default()
            }),
        // google_maps_result
        (
            prop::collection::vec(arb_google_maps_result_item(), 0..2),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(result, signature)| StepDelta::GoogleMapsResult {
                result,
                signature,
                extra: Default::default()
            }),
        // processing_call / processing_result
        proptest::option::of(arb_text()).prop_map(|signature| StepDelta::ProcessingCall {
            signature,
            extra: Default::default()
        }),
        proptest::option::of(arb_text()).prop_map(|signature| StepDelta::ProcessingResult {
            signature,
            extra: Default::default()
        }),
        // retrieval_call / retrieval_result
        (
            prop::collection::vec(arb_text(), 0..3),
            proptest::option::of(arb_retrieval_type()),
            proptest::option::of(arb_text()),
        )
            .prop_map(
                |(queries, retrieval_type, signature)| StepDelta::RetrievalCall {
                    queries,
                    retrieval_type,
                    signature,
                    arguments_extra: Default::default(),
                    extra: Default::default()
                }
            ),
        (
            proptest::option::of(proptest::bool::ANY),
            proptest::option::of(arb_text()),
        )
            .prop_map(|(is_error, signature)| StepDelta::RetrievalResult {
                is_error,
                signature,
                extra: Default::default()
            }),
        // Unknown delta (not gated: StepDelta preserves unknowns even in strict mode)
        (arb_unknown_type(), arb_unknown_object())
            .prop_map(|(delta_type, data)| StepDelta::Unknown { delta_type, data }),
    ]
}

proptest! {
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
            signature: None, extra: Default::default(),
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
            signature: None, extra: Default::default(),
        };

        assert_value_roundtrip(&step)?;
    }
}

#[cfg(not(genai_strict_unknown))]
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
}

// =============================================================================
// Recorded regressions
// =============================================================================
//
// A seed in `proptest-regressions/proptest_steps.txt` replays whatever value
// the strategy now draws from it, which drifts as the strategies change (the
// `CodeExecutionCall` step seed now draws a `UserInput`). The value each
// seed shrank to when it was recorded is pinned here, so the regression
// stays covered whatever the strategy generates.

/// Seed `f68b3f22` (`function_result_payload_roundtrip`): a content payload
/// holding a bare video whose only field is an unknown resolution.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_video_with_unknown_resolution_payload_roundtrips() {
    let payload = FunctionResultPayload::Contents(vec![Content::Video {
        data: None,
        uri: None,
        mime_type: None,
        resolution: Some(Resolution::Unknown {
            resolution_type: "zz_0".to_string(),
            data: serde_json::Value::String("zz_0".to_string()),
        }),
        processing: None,
        name: None,
        extra: Default::default(),
    }]);
    let json = serde_json::to_value(&payload).expect("Serialization should succeed");
    let restored: FunctionResultPayload =
        serde_json::from_value(json.clone()).expect("Deserialization should succeed");
    assert_eq!(
        std::mem::discriminant(&payload),
        std::mem::discriminant(&restored)
    );
    let restored_json = serde_json::to_value(&restored).expect("Re-serialization should succeed");
    assert_eq!(json, restored_json);
}

/// Seed `7b13b8ed` (`step_delta_roundtrip`): a code execution delta with an
/// unknown language and no code.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_code_execution_delta_with_unknown_language_roundtrips() {
    let delta = StepDelta::CodeExecutionCall {
        language: Some(CodeExecutionLanguage::Unknown {
            language_type: "zz_a".to_string(),
            data: serde_json::Value::String("zz_a".to_string()),
        }),
        code: None,
        signature: None,
        arguments_extra: Default::default(),
        extra: Default::default(),
    };
    assert_value_roundtrip(&delta).unwrap();
}

/// Seed `77c35343` (`step_roundtrip`): a code execution call with an unknown
/// language and empty code.
#[cfg(not(genai_strict_unknown))]
#[test]
fn recorded_seed_code_execution_step_with_unknown_language_roundtrips() {
    let step = Step::CodeExecutionCall {
        id: "a".to_string(),
        language: CodeExecutionLanguage::Unknown {
            language_type: "zz_a".to_string(),
            data: serde_json::Value::String("zz_a".to_string()),
        },
        code: String::new(),
        signature: None,
        arguments_extra: Default::default(),
        extra: Default::default(),
    };
    assert_value_roundtrip(&step).unwrap();
}
