//! Unit tests for `InteractionResponse` deserialization, its text, function,
//! annotation and output-step accessors, and `InteractionStatus` (API
//! revision 2026-05-20). The usage, step-summary, built-in tool and view
//! accessors are tested beside their modules (`usage_tests.rs`,
//! `step_summary_tests.rs`, `tool_steps_tests.rs`, `views_tests.rs`).
//!
//! The response model carries a `steps: Vec<Step>` array instead of the
//! launch-era `outputs: Vec<Content>`; the helper tests below exercise the
//! same behaviors against the step model.
//!
//! Removed coverage (types deleted from the API surface):
//! - `GroundingMetadata` / `GroundingChunk` / `WebSource` and
//!   `UrlContextMetadata` / `UrlMetadataEntry` / `UrlRetrievalStatus` tests —
//!   grounding data now arrives as typed steps and text annotations.
//! - `reasoning_tokens()` / `UsageMetadata.total_reasoning_tokens` tests —
//!   the field was removed; `total_thought_tokens` remains.

use super::*;

// --- Response Deserialization ---

#[test]
fn test_deserialize_interaction_response_completed() {
    let response_json = r#"{
        "id": "interaction_123",
        "model": "test-model",
        "steps": [
            {"type": "user_input", "content": [{"type": "text", "text": "Hello"}]},
            {"type": "model_output", "content": [{"type": "text", "text": "Hi there!"}]}
        ],
        "status": "completed",
        "usage": {
            "total_input_tokens": 5,
            "total_output_tokens": 10,
            "total_tokens": 15
        }
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(response_json).expect("Deserialization failed");

    assert_eq!(response.id.as_deref(), Some("interaction_123"));
    assert_eq!(response.model.as_deref(), Some("test-model"));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert_eq!(response.steps.len(), 2);
    assert_eq!(response.as_text(), Some("Hi there!"));
    assert!(response.usage.is_some());
    let usage = response.usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(5));
    assert_eq!(usage.total_output_tokens, Some(10));
    assert_eq!(usage.total_tokens, Some(15));
}

// --- Response Helper Tests ---

/// The counting arm is the user-visible bug #433 reports: a successful MCP
/// interaction reporting zero tool calls.
///
/// Worth pinning explicitly because it is not self-enforcing. `Display`
/// indexes a fixed-size array, so *adding a field* without extending that
/// array is a compile error — but adding a `Step` variant and forgetting to
/// count it is not, and yields a permanent 0. That is exactly the failure
/// this change fixes, one variant over.
#[test]
fn tool_call_steps_are_counted_and_not_attributed_to_mcp() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![
            Step::ToolCall {
                id: "call_1".to_string(),
                signature: Some("sig".to_string()),
                extra: Default::default(),
            },
            Step::ToolCall {
                id: "call_2".to_string(),
                signature: None,
                extra: Default::default(),
            },
            Step::model_text("the answer"),
        ],
        ..Default::default()
    };

    let summary = response.step_summary();
    assert_eq!(
        summary.tool_call_count, 2,
        "generic tool_call steps must be counted — landing in Unknown is what \
         made a successful MCP call report none"
    );
    assert_eq!(
        summary.mcp_server_tool_call_count, 0,
        "and must not be attributed to the MCP-specific counter, which is the \
         field whose 0 reads as 'MCP did not run'"
    );
    assert_eq!(summary.unknown_count, 0);

    // The step-level accessors over the same response. Ordering is the part
    // the doc comment promises and nothing else checks.
    assert!(response.has_tool_calls());
    let calls = response.tool_calls();
    assert_eq!(calls.len(), 2, "the model_text step must not be included");
    let ids: Vec<&str> = calls.iter().map(|c| c.id).collect();
    assert_eq!(ids, ["call_1", "call_2"], "returned in step order");
    assert_eq!(calls[0].signature, Some("sig"));
    assert_eq!(calls[1].signature, None);
}

#[test]
fn test_interaction_response_text() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![
            Content::text("Hello"),
            Content::text("World"),
        ])],
        ..Default::default()
    };

    assert_eq!(response.as_text(), Some("Hello"));
    assert_eq!(response.all_text(), "HelloWorld");
    assert!(response.has_text());
    assert!(!response.has_function_calls());
}

#[test]
fn test_interaction_response_text_output_text_fallback() {
    // as_text()/all_text() fall back to the API-provided output_text field
    // when no text-bearing steps are present.
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        output_text: Some("Fallback text".to_string()),
        ..Default::default()
    };

    assert_eq!(response.as_text(), Some("Fallback text"));
    assert_eq!(response.all_text(), "Fallback text");
    assert!(response.has_text());
}

#[test]
fn test_interaction_response_thoughts() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::thought("sig_1"),
            Step::thought("sig_2"),
            Step::model_text("The answer is 42."),
            // Thought with None signature should be filtered out
            Step::Thought {
                signature: None,
                summary: vec![],
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    assert!(response.has_thoughts());

    let signatures: Vec<_> = response.thought_signatures();
    assert_eq!(signatures.len(), 2);
    assert_eq!(signatures[0], "sig_1");
    assert_eq!(signatures[1], "sig_2");

    // Verify as_text() still works (only returns model output text)
    assert_eq!(response.as_text(), Some("The answer is 42."));
}

#[test]
fn test_interaction_response_no_thoughts() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text, no thoughts.")],
        ..Default::default()
    };

    assert!(!response.has_thoughts());
    let signatures: Vec<_> = response.thought_signatures();
    assert!(signatures.is_empty());
}

#[test]
fn test_interaction_response_thought_summaries() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![
            Step::Thought {
                signature: Some("sig".to_string()),
                summary: vec![Content::text("Summarized reasoning")],
                extra: Default::default(),
            },
            Step::model_text("Answer"),
        ],
        ..Default::default()
    };

    let summaries: Vec<_> = response.thought_summaries();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].as_text(), Some("Summarized reasoning"));
}

#[test]
fn test_interaction_response_function_calls() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::function_call(
                "call_001",
                "get_weather",
                serde_json::json!({"location": "Paris"}),
            ),
            // Signature-bearing call (the API returns `signature` on
            // function_call steps; verified live 2026-07) still surfaces
            // through the accessor.
            Step::FunctionCall {
                id: "call_002".to_string(),
                name: "get_time".to_string(),
                arguments: serde_json::json!({"timezone": "UTC"}),
                signature: Some("sig-abc".to_string()),
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    let calls = response.function_calls();
    assert_eq!(calls.len(), 2);
    // FunctionCallInfo struct fields (id is now a required &str)
    assert_eq!(calls[0].id, "call_001");
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].args["location"], "Paris");
    assert_eq!(calls[1].id, "call_002");
    assert_eq!(calls[1].name, "get_time");
    assert!(response.has_function_calls());
    assert!(!response.has_text());
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_function_call_step_without_id_becomes_unknown() {
    // The 2026-05-20 spec requires an id on function_call steps. A payload
    // missing it can no longer be represented as Step::FunctionCall (the old
    // Content model used Option<String>), so it degrades gracefully to
    // Step::Unknown with the data preserved.
    let response_json = r#"{
        "id": "test_id",
        "model": "test-model",
        "steps": [
            {"type": "function_call", "name": "get_weather", "arguments": {"location": "Tokyo"}}
        ],
        "status": "requires_action"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(response_json).expect("Should deserialize");

    assert!(response.function_calls().is_empty());
    assert!(response.has_unknown());
    let unknowns = response.unknown_steps();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].0, "function_call");
    assert_eq!(unknowns[0].1["name"], "get_weather");
}

#[test]
fn test_interaction_response_mixed_content() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_text("Let me check"),
            Step::function_call("call_mixed", "check_status", serde_json::json!({})),
            Step::model_text("Done!"),
        ],
        ..Default::default()
    };

    assert_eq!(response.as_text(), Some("Let me check"));
    assert_eq!(response.all_text(), "Let me checkDone!");
    assert_eq!(response.function_calls().len(), 1);
    assert!(response.has_text());
    assert!(response.has_function_calls());
}

#[test]
fn test_interaction_response_empty_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    assert_eq!(response.as_text(), None);
    assert_eq!(response.all_text(), "");
    assert_eq!(response.function_calls().len(), 0);
    assert!(!response.has_text());
    assert!(!response.has_function_calls());
}

// --- Unknown Response Helper Tests ---

#[test]
fn test_interaction_response_has_unknown() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_text("Here's the result:"),
            Step::Unknown {
                step_type: "future_step_type".to_string(),
                data: serde_json::json!({
                    "type": "future_step_type",
                    "outcome": "success",
                    "output": "42"
                }),
            },
        ],
        ..Default::default()
    };

    assert!(response.has_unknown());
    assert!(response.has_text());

    let unknowns = response.unknown_steps();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].0, "future_step_type");
    assert_eq!(unknowns[0].1["outcome"], "success");
}

#[test]
fn test_interaction_response_no_unknown() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Normal response")],
        ..Default::default()
    };

    assert!(!response.has_unknown());
    assert!(response.unknown_steps().is_empty());
}

// --- Function Result Helpers ---

#[test]
fn test_interaction_response_function_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::function_result(
                "get_weather",
                "call_001",
                serde_json::json!({"temp": 72, "unit": "F"}),
            ),
            Step::function_result(
                "get_time",
                "call_002",
                serde_json::json!({"time": "14:30", "zone": "UTC"}),
            ),
            Step::model_text("Here are the results"),
        ],
        ..Default::default()
    };

    assert!(response.has_function_results());
    assert!(response.has_text());

    let results = response.function_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, Some("get_weather"));
    assert_eq!(results[0].call_id, "call_001");
    // result is now a FunctionResultPayload
    assert_eq!(results[0].result.as_json().unwrap()["temp"], 72);
    assert_eq!(results[1].name, Some("get_time"));
    assert_eq!(results[1].call_id, "call_002");
}

#[test]
fn test_interaction_response_function_results_text_payload() {
    // Function results may carry a plain string payload
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![Step::function_result("echo", "call_003", "plain output")],
        ..Default::default()
    };

    let results = response.function_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].result.as_text(), Some("plain output"));
    assert!(results[0].is_error.is_none());
}

#[test]
fn test_interaction_response_no_function_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text")],
        ..Default::default()
    };

    assert!(!response.has_function_results());
    assert!(response.function_results().is_empty());
}

// --- InteractionStatus Tests ---

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_interaction_status_unknown_deserialize() {
    // Simulate a new API status that this library doesn't know about
    let json = r#""future_pending_state""#;
    let status: InteractionStatus = serde_json::from_str(json).expect("Should deserialize");

    assert!(status.is_unknown());
    assert_eq!(status.unknown_status_type(), Some("future_pending_state"));
    assert!(status.unknown_data().is_some());
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_interaction_status_unknown_roundtrip() {
    // Deserialize unknown status
    let json = r#""new_background_processing""#;
    let status: InteractionStatus = serde_json::from_str(json).expect("Should deserialize");

    assert!(status.is_unknown());

    // Serialize back
    let reserialized = serde_json::to_string(&status).expect("Should serialize");
    assert_eq!(reserialized, r#""new_background_processing""#);

    // Deserialize again to verify roundtrip
    let status2: InteractionStatus =
        serde_json::from_str(&reserialized).expect("Should deserialize again");
    assert!(status2.is_unknown());
    assert_eq!(
        status2.unknown_status_type(),
        Some("new_background_processing")
    );
}

#[test]
fn test_interaction_status_known_types_not_unknown() {
    // Verify known types don't trigger Unknown
    let completed: InteractionStatus =
        serde_json::from_str(r#""completed""#).expect("Should deserialize");
    assert!(!completed.is_unknown());
    assert_eq!(completed.unknown_status_type(), None);
    assert_eq!(completed.unknown_data(), None);

    let in_progress: InteractionStatus =
        serde_json::from_str(r#""in_progress""#).expect("Should deserialize");
    assert!(!in_progress.is_unknown());

    let failed: InteractionStatus =
        serde_json::from_str(r#""failed""#).expect("Should deserialize");
    assert!(!failed.is_unknown());

    let requires_action: InteractionStatus =
        serde_json::from_str(r#""requires_action""#).expect("Should deserialize");
    assert!(!requires_action.is_unknown());
}

#[test]
fn test_interaction_status_budget_exceeded() {
    // BudgetExceeded is new in revision 2026-05-20; wire format "budget_exceeded"
    let status: InteractionStatus =
        serde_json::from_str(r#""budget_exceeded""#).expect("Should deserialize");
    assert_eq!(status, InteractionStatus::BudgetExceeded);
    assert!(!status.is_unknown());

    let reserialized = serde_json::to_string(&status).expect("Should serialize");
    assert_eq!(reserialized, r#""budget_exceeded""#);
}

#[test]
fn test_interaction_status_default_is_in_progress() {
    assert_eq!(InteractionStatus::default(), InteractionStatus::InProgress);
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_interaction_status_non_string_handled() {
    // Edge case: API returns non-string (shouldn't happen but code handles it)
    let json = r#"42"#;
    let status: InteractionStatus = serde_json::from_str(json).expect("Should deserialize");

    assert!(status.is_unknown());
    // The status_type should indicate it was non-string
    let status_type = status
        .unknown_status_type()
        .expect("Should have status_type");
    assert!(status_type.contains("non-string"));

    // The data should preserve the original value
    let data = status.unknown_data().expect("Should have data");
    assert_eq!(*data, serde_json::json!(42));
}

// --- Optional ID Tests (Issue #210) ---

#[test]
fn test_interaction_response_deserialize_without_id() {
    // When store=false, the API response does not include an id field.
    // This test verifies that we can deserialize such responses correctly.
    let json = r#"{
        "model": "test-model",
        "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi there!"}]}],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(json).expect("Deserialization should succeed without id");

    assert!(response.id.is_none(), "ID should be None when not present");
    assert_eq!(response.model, Some("test-model".to_string()));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(!response.steps.is_empty());
}

#[test]
fn test_interaction_response_deserialize_with_id() {
    // When store=true (or by default), the API response includes an id field.
    // This test verifies that we can still deserialize such responses correctly.
    let json = r#"{
        "id": "interaction-abc123",
        "model": "test-model",
        "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi there!"}]}],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(json).expect("Deserialization should succeed with id");

    assert_eq!(
        response.id.as_deref(),
        Some("interaction-abc123"),
        "ID should be present when included"
    );
    assert_eq!(response.model, Some("test-model".to_string()));
    assert_eq!(response.status, InteractionStatus::Completed);
}

#[test]
fn test_interaction_response_serialize_without_id() {
    // When id is None, it should not be serialized into the JSON output.
    // This uses skip_serializing_if to avoid "id": null in the output.
    let response = InteractionResponse {
        id: None,
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Hello")],
        ..Default::default()
    };

    let json = serde_json::to_string(&response).expect("Serialization should succeed");

    assert!(
        !json.contains(r#""id""#),
        "JSON should not contain id field when None: {json}"
    );
}

#[test]
fn test_interaction_response_roundtrip_without_id() {
    // Verify roundtrip serialization works correctly when id is None.
    let original = InteractionResponse {
        id: None,
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Test response")],
        ..Default::default()
    };

    let json = serde_json::to_string(&original).expect("Serialization should succeed");
    let restored: InteractionResponse =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(restored.id, original.id);
    assert_eq!(restored.model, original.model);
    assert_eq!(restored.status, original.status);
}

// --- Annotation Helper Tests ---

#[test]
fn test_interaction_response_has_annotations() {
    // Response with annotations
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![Content::Text {
            text: Some("According to the source, climate change is accelerating.".to_string()),
            annotations: Some(vec![Annotation::url_citation(
                "https://climate.gov",
                None,
                19,
                25,
            )]),
            extra: Default::default(),
        }])],
        ..Default::default()
    };

    assert!(response.has_annotations());
}

#[test]
fn test_interaction_response_no_annotations() {
    // Response without annotations
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Plain text without citations.")],
        ..Default::default()
    };

    assert!(!response.has_annotations());
}

#[test]
fn test_interaction_response_empty_annotations_not_counted() {
    // Response with empty annotations array (should not count as having annotations)
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![Content::Text {
            text: Some("Text with empty annotations.".to_string()),
            annotations: Some(vec![]), // Empty array
            extra: Default::default(),
        }])],
        ..Default::default()
    };

    assert!(!response.has_annotations());
}

#[test]
fn test_interaction_response_all_annotations() {
    // Response with multiple text blocks, each with annotations
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![Content::Text {
                text: Some("First claim from source A.".to_string()),
                annotations: Some(vec![Annotation::url_citation(
                    "https://source-a.com",
                    None,
                    0,
                    11,
                )]),
                extra: Default::default(),
            }]),
            Step::thought("sig_thinking"),
            Step::model_output(vec![
                Content::Text {
                    text: Some("Second and third claims.".to_string()),
                    annotations: Some(vec![
                        Annotation::url_citation("https://source-b.com", None, 0, 6),
                        Annotation::url_citation("https://source-c.com", None, 11, 16),
                    ]),
                    extra: Default::default(),
                },
                Content::Text {
                    text: Some("Text without annotations.".to_string()),
                    annotations: None,
                    extra: Default::default(),
                },
            ]),
        ],
        ..Default::default()
    };

    let annotations: Vec<_> = response.all_annotations();

    // Should collect all 3 annotations from the two Text blocks with annotations
    assert_eq!(annotations.len(), 3);
    assert_eq!(annotations[0].source(), Some("https://source-a.com"));
    assert_eq!(annotations[1].source(), Some("https://source-b.com"));
    assert_eq!(annotations[2].source(), Some("https://source-c.com"));
}

#[test]
fn test_interaction_response_all_annotations_empty() {
    // Response with no annotations at all
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_text("No annotations here."),
            Step::function_call("call_1", "test", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let count = response.all_annotations().len();
    assert_eq!(count, 0);
}

#[test]
fn test_interaction_response_all_annotations_skips_non_text() {
    // Verify all_annotations only looks at Text content, not other types
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![
                Content::image_data("base64", "image/png"),
                Content::Text {
                    text: Some("Only text has annotations.".to_string()),
                    annotations: Some(vec![Annotation::url_citation(
                        "https://example.com",
                        None,
                        0,
                        4,
                    )]),
                    extra: Default::default(),
                },
            ]),
            Step::CodeExecutionResult {
                call_id: "call_1".to_string(),
                is_error: false,
                result: "result".into(),
                signature: None,
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    let annotations: Vec<_> = response.all_annotations();

    // Should only find the one annotation from the Text content
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].source(), Some("https://example.com"));
}

// =============================================================================
// output_steps() Tests (replaces the launch-era as_model_turn())
// =============================================================================

#[test]
fn test_output_steps_returns_owned_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Response text")],
        ..Default::default()
    };

    let steps = response.output_steps();
    assert_eq!(steps.len(), 1);
    assert!(matches!(steps[0], Step::ModelOutput { .. }));
    assert_eq!(steps[0].as_text(), Some("Response text"));

    // The returned steps are owned clones; the response is untouched
    assert_eq!(response.steps.len(), 1);
}

#[test]
fn test_output_steps_extends_history() {
    // The documented multi-turn pattern: history.extend(response.output_steps())
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::thought("sig"),
            Step::model_output(vec![
                Content::text("First part"),
                Content::text("Second part"),
            ]),
        ],
        ..Default::default()
    };

    let mut history = vec![Step::user_text("What is 2+2?")];
    history.extend(response.output_steps());
    history.push(Step::user_text("Now multiply that by 3"));

    assert_eq!(history.len(), 4);
    assert!(matches!(history[0], Step::UserInput { .. }));
    assert!(matches!(history[1], Step::Thought { .. }));
    assert!(matches!(history[2], Step::ModelOutput { .. }));
    assert_eq!(history[2].content().unwrap().len(), 2);
    assert!(matches!(history[3], Step::UserInput { .. }));
}

#[test]
fn test_output_contents_iterates_model_output_only() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![
            Step::user_input(vec![Content::text("user text")]),
            Step::model_output(vec![Content::text("model text")]),
            Step::function_call("call_1", "fn", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let contents: Vec<_> = response.output_contents();
    // Only content from model_output steps is included
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].as_text(), Some("model text"));
}

#[test]
fn test_deserialize_response_object_and_service_tier() {
    // The live API (Api-Revision 2026-05-20, verified 2026-07) returns
    // `object: "interaction"` and echoes the effective `service_tier` on
    // every response. Both must roundtrip losslessly.
    let wire = serde_json::json!({
        "id": "interaction_123",
        "object": "interaction",
        "model": "test-model",
        "service_tier": "standard",
        "status": "completed",
        "steps": [
            {"type": "model_output", "content": [{"type": "text", "text": "hi"}]}
        ]
    });

    let response: InteractionResponse =
        serde_json::from_value(wire.clone()).expect("Deserialization failed");
    assert_eq!(response.object.as_deref(), Some("interaction"));
    assert_eq!(response.service_tier, Some(crate::ServiceTier::Standard));

    // Lossless roundtrip: re-serialization matches the wire JSON exactly.
    let roundtripped = serde_json::to_value(&response).expect("Serialization failed");
    assert_eq!(roundtripped, wire);
}

#[test]
fn test_response_object_and_service_tier_absent_are_skipped() {
    // Responses without these fields (e.g. local fixtures) must not emit them.
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        ..Default::default()
    };
    assert_eq!(response.object, None);
    assert_eq!(response.service_tier, None);
    let json = serde_json::to_value(&response).unwrap();
    assert!(json.get("object").is_none());
    assert!(json.get("service_tier").is_none());
}

#[test]
fn test_deserialize_response_webhook_config_echo() {
    // The live API (Api-Revision 2026-05-20, verified 2026-07) echoes the
    // request's webhook_config verbatim on the create response for
    // background interactions. It must roundtrip losslessly.
    let wire = serde_json::json!({
        "id": "interaction_123",
        "object": "interaction",
        "model": "test-model",
        "status": "in_progress",
        "steps": [],
        "webhook_config": {
            "uris": ["https://example.com/hook"],
            "user_metadata": {"job": "nightly"}
        }
    });

    let response: InteractionResponse =
        serde_json::from_value(wire.clone()).expect("Deserialization failed");
    let config = response
        .webhook_config
        .as_ref()
        .expect("webhook_config echo");
    assert_eq!(
        config.uris.as_deref(),
        Some(&["https://example.com/hook".to_string()][..])
    );
    assert_eq!(
        config.user_metadata,
        Some(serde_json::json!({"job": "nightly"}))
    );

    // Lossless roundtrip: re-serialization matches the wire JSON exactly.
    let roundtripped = serde_json::to_value(&response).expect("Serialization failed");
    assert_eq!(roundtripped, wire);

    // Absent field is skipped on serialize.
    let bare = InteractionResponse {
        status: InteractionStatus::Completed,
        ..Default::default()
    };
    assert!(
        serde_json::to_value(&bare)
            .unwrap()
            .get("webhook_config")
            .is_none()
    );
}

// --- InteractionResponse Accessor and Wire-Format Tests ---

fn minimal_response(usage: Option<UsageMetadata>) -> InteractionResponse {
    InteractionResponse {
        status: InteractionStatus::Completed,
        usage,
        ..Default::default()
    }
}

#[test]
fn test_token_helpers_with_usage() {
    let response = minimal_response(Some(UsageMetadata {
        total_input_tokens: Some(100),
        total_output_tokens: Some(50),
        total_tokens: Some(150),
        total_cached_tokens: Some(25),
        total_thought_tokens: Some(10),
        total_tool_use_tokens: Some(5),
        ..Default::default()
    }));

    assert_eq!(response.input_tokens(), Some(100));
    assert_eq!(response.output_tokens(), Some(50));
    assert_eq!(response.total_tokens(), Some(150));
    assert_eq!(response.cached_tokens(), Some(25));
    assert_eq!(response.thought_tokens(), Some(10));
    assert_eq!(response.tool_use_tokens(), Some(5));
}

#[test]
fn test_token_helpers_without_usage() {
    let response = minimal_response(None);

    assert_eq!(response.input_tokens(), None);
    assert_eq!(response.output_tokens(), None);
    assert_eq!(response.total_tokens(), None);
    assert_eq!(response.cached_tokens(), None);
    assert_eq!(response.thought_tokens(), None);
    assert_eq!(response.tool_use_tokens(), None);
}

// =========================================================================
// Steps-based response deserialization (wire fixtures)
// =========================================================================

#[test]
fn test_response_deserializes_steps_wire_fixture() {
    // Representative revision 2026-05-20 response shape. ID forms:
    // `id`/`previous_interaction_id` are live-observed bare opaque
    // strings (`v1_...`-style, no `interactions/` prefix);
    // `environment_id`'s response-side form has not been observed
    // non-None at accept-time — bare is assumed here to match the
    // live-verified environments *resource* capture, and the
    // inline-environment integration test defensively strips a
    // possible prefix until the field is observed for real.
    let json = r#"{
            "id": "v1_abc123",
            "model": "test-model",
            "status": "completed",
            "steps": [
                {"type": "thought", "signature": "sig-1"},
                {"type": "model_output", "content": [
                    {"type": "text", "text": "The answer is 4."}
                ]}
            ],
            "usage": {
                "total_input_tokens": 10,
                "total_output_tokens": 8,
                "total_tokens": 18,
                "grounding_tool_count": [{"type": "google_search", "count": 2}]
            },
            "previous_interaction_id": "v1_prev",
            "environment_id": "38aac1ae7f30fe9bd67afe42382ea041",
            "created": "2026-05-21T10:00:00Z"
        }"#;

    let response: InteractionResponse = serde_json::from_str(json).unwrap();
    assert_eq!(response.id.as_deref(), Some("v1_abc123"));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert_eq!(response.steps.len(), 2);
    assert_eq!(response.as_text(), Some("The answer is 4."));
    assert_eq!(response.thought_signatures(), vec!["sig-1"]);
    assert_eq!(
        response.environment_id.as_deref(),
        Some("38aac1ae7f30fe9bd67afe42382ea041")
    );
    let usage = response.usage.as_ref().unwrap();
    assert_eq!(usage.grounding_count_for_tool("google_search"), Some(2));
    assert!(response.created.is_some());
}

#[test]
fn test_response_serializes_snake_case() {
    let response = InteractionResponse {
        previous_interaction_id: Some("v1_prev".into()),
        status: InteractionStatus::Completed,
        ..Default::default()
    };
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["previous_interaction_id"], "v1_prev");
    assert!(json.get("previousInteractionId").is_none());
}

#[test]
fn test_budget_exceeded_status_roundtrip() {
    let status: InteractionStatus = serde_json::from_str("\"budget_exceeded\"").unwrap();
    assert_eq!(status, InteractionStatus::BudgetExceeded);
    assert_eq!(
        serde_json::to_string(&status).unwrap(),
        "\"budget_exceeded\""
    );
}

#[test]
fn test_function_calls_over_steps() {
    let response = InteractionResponse {
        status: InteractionStatus::RequiresAction,
        steps: vec![
            Step::thought("sig"),
            Step::function_call(
                "call_1",
                "get_weather",
                serde_json::json!({"city": "Tokyo"}),
            ),
        ],
        ..Default::default()
    };
    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].args["city"], "Tokyo");
    assert!(response.has_function_calls());
    assert!(response.has_thoughts());
}

#[test]
fn test_as_text_falls_back_to_output_text() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        output_text: Some("fallback".into()),
        ..Default::default()
    };
    assert_eq!(response.as_text(), Some("fallback"));
    assert_eq!(response.all_text(), "fallback");
    assert!(response.has_text());
}

#[test]
fn test_unknown_steps_helper() {
    let response = InteractionResponse {
        status: InteractionStatus::Completed,
        steps: vec![Step::Unknown {
            step_type: "quantum".into(),
            data: serde_json::json!({"type": "quantum", "x": 1}),
        }],
        ..Default::default()
    };
    assert!(response.has_unknown());
    let unknown = response.unknown_steps();
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].0, "quantum");
}

// =========================================================================
// InteractionStatus Tests
// =========================================================================

#[test]
fn test_interaction_status_incomplete_roundtrip() {
    let json = r#""incomplete""#;
    let status: InteractionStatus = serde_json::from_str(json).unwrap();
    assert_eq!(status, InteractionStatus::Incomplete);

    let serialized = serde_json::to_string(&status).unwrap();
    assert_eq!(serialized, r#""incomplete""#);
}

#[cfg(not(genai_strict_unknown))]
#[test]
fn test_interaction_status_unknown_preserved() {
    let status: InteractionStatus = serde_json::from_str("\"hibernating\"").unwrap();
    assert!(status.is_unknown());
    assert_eq!(status.unknown_status_type(), Some("hibernating"));
    assert!(status.unknown_data().is_some());
    assert_eq!(serde_json::to_string(&status).unwrap(), "\"hibernating\"");
}

#[test]
fn interaction_echoes_labels_system_instruction_and_extras() {
    let wire = serde_json::json!({
        "id": "int_1",
        "status": "completed",
        "steps": [],
        "system_instruction": "Be brief.",
        "labels": {"team": "audit"},
        "environment": {"type": "remote", "env": [{"A": {"value": "1"}}]}
    });
    let response: InteractionResponse = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(response.system_instruction.as_deref(), Some("Be brief."));
    assert_eq!(response.labels.as_ref().unwrap()["team"], "audit");
    assert!(
        matches!(
            response.environment,
            Some(crate::EnvironmentSpec::Remote(_))
        ),
        "{:?}",
        response.environment
    );
    assert!(response.extra.is_empty(), "{:?}", response.extra);
    // The echo spells `env` as a list of single-key maps; it goes back out
    // as the map a request sends (`RemoteEnvironment::env`).
    let mut sent = wire.clone();
    sent["environment"]["env"] = serde_json::json!({"A": {"value": "1"}});
    assert_eq!(serde_json::to_value(&response).unwrap(), sent);
}

#[test]
fn test_pending_function_calls_skip_answered_calls() {
    let response = InteractionResponse {
        status: InteractionStatus::RequiresAction,
        steps: vec![
            Step::function_call(
                "mcp_1",
                "deepwiki:read_wiki_structure",
                serde_json::json!({}),
            ),
            Step::function_result("deepwiki:read_wiki_structure", "mcp_1", "{}"),
            Step::function_call(
                "call_1",
                "get_weather",
                serde_json::json!({"city": "Tokyo"}),
            ),
        ],
        ..Default::default()
    };
    assert_eq!(response.function_calls().len(), 2);
    let pending = response.pending_function_calls();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "call_1");
    assert_eq!(pending[0].name, "get_weather");
}

/// A continuable response, captured live (2026-11-01, the continuation
/// model at `max_output_tokens: 30`): status `incomplete` with a top-level
/// token. Plus a cancelled interaction's `errors`.
#[test]
fn continuation_token_and_errors_are_typed() {
    let wire = serde_json::json!({
        "id": "v1_abc",
        "status": "incomplete",
        "steps": [],
        "continuation_token": "CiQxYzQ5ZGIwZi0=",
        "errors": [{"code": "CANCELLED", "message": "There was a problem processing your request."}],
        "usage": {"total_input_tokens": 43, "initial_input_tokens": 43}
    });
    let response: InteractionResponse = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(response.status, InteractionStatus::Incomplete);
    assert_eq!(
        response.continuation_token.as_deref(),
        Some("CiQxYzQ5ZGIwZi0=")
    );
    let errors = response.errors.as_deref().unwrap();
    assert_eq!(errors[0].code.as_deref(), Some("CANCELLED"));
    assert_eq!(
        response.usage.as_ref().unwrap().initial_input_tokens,
        Some(43)
    );
    assert!(response.extra.is_empty(), "{:?}", response.extra);
    assert!(response.usage.as_ref().unwrap().extra.is_empty());
    assert_eq!(serde_json::to_value(&response).unwrap(), wire);

    let status: InteractionStatus =
        serde_json::from_value(serde_json::json!("requires_continuation")).unwrap();
    assert_eq!(status, InteractionStatus::RequiresContinuation);
}

#[test]
fn stored_read_request_config_is_modeled() {
    // A stored TTS interaction read back (live 2026-11-05): the speakers
    // object form, and the modalities.
    let tts: InteractionResponse = serde_json::from_value(serde_json::json!({
        "id": "i1",
        "status": "completed",
        "response_modalities": ["audio"],
        "generation_config": {"speech_config": {"speakers": [{"voice": "Kore"}]}}
    }))
    .unwrap();
    assert!(tts.extra.is_empty(), "{:?}", tts.extra);
    assert_eq!(tts.response_modalities, Some(vec!["audio".to_string()]));
    let speakers = tts
        .generation_config
        .as_ref()
        .and_then(|c| c.speech_config.as_ref())
        .unwrap();
    assert_eq!(speakers[0].voice.as_deref(), Some("Kore"));

    // A stored model interaction with a JSON response format and safety
    // settings (live 2026-11-05).
    let json: InteractionResponse = serde_json::from_value(serde_json::json!({
        "id": "i2",
        "status": "completed",
        "generation_config": {"stop_sequences": ["STOP"], "max_output_tokens": 2000, "thinking_level": "low", "future_knob": 1},
        "response_format": {"type": "text", "mime_type": "application/json", "schema": {"type": "object"}},
        "safety_settings": [{"type": "hate_speech", "threshold": "block_only_high"}]
    }))
    .unwrap();
    assert!(json.extra.is_empty(), "{:?}", json.extra);
    let config = json.generation_config.as_ref().unwrap();
    assert_eq!(config.max_output_tokens, Some(2000));
    assert_eq!(config.extra["future_knob"], 1);
    assert!(json.response_format.is_some());
    assert_eq!(json.safety_settings.as_ref().map(Vec::len), Some(1));

    // A stored agent interaction (live 2026-11-05).
    let agent: InteractionResponse = serde_json::from_value(serde_json::json!({
        "id": "i3",
        "status": "completed",
        "agent": crate::DEFAULT_ANTIGRAVITY_AGENT,
        "agent_config": {"type": "antigravity"},
        "environment": "835e59966a2fa3c2d4541ce46ebc26d6",
        "environment_id": "835e59966a2fa3c2d4541ce46ebc26d6"
    }))
    .unwrap();
    assert!(agent.extra.is_empty(), "{:?}", agent.extra);
    assert_eq!(
        agent.agent_config.as_ref().and_then(|c| c.config_type()),
        Some("antigravity")
    );
    assert_eq!(
        agent.environment,
        Some(crate::EnvironmentSpec::Id(
            "835e59966a2fa3c2d4541ce46ebc26d6".to_string()
        ))
    );
    let back = serde_json::to_value(&agent).unwrap();
    assert_eq!(back["agent_config"]["type"], "antigravity");
    assert_eq!(back["environment"], "835e59966a2fa3c2d4541ce46ebc26d6");
}

/// The request config a stored interaction serves back keeps a field the
/// crate does not model, at every level, and writes it back (D-001): the
/// generation config's own `extra` already did, but its speech, image,
/// video and transcription configs, a `tool_choice` restriction, a safety
/// setting and the webhook config dropped theirs before #504's follow-ups.
#[test]
fn stored_request_config_keeps_unmodeled_nested_fields() {
    let served = serde_json::json!({
        "id": "i4",
        "status": "completed",
        "generation_config": {
            "speech_config": [{"voice": "Kore", "future_voice_knob": 1}],
            "image_config": {"aspect_ratio": "1:1", "future_image_knob": "x"},
            "video_config": {"task": "text_to_video", "future_video_knob": true},
            "transcription_config": {"language_codes": ["en-US"], "future_transcription_knob": [1]},
            "tool_choice": {"allowed_tools": {"mode": "any", "tools": ["f"], "future_choice_knob": 2}}
        },
        "safety_settings": [{"type": "hate_speech", "threshold": "block_only_high", "future_safety_knob": 3}],
        "webhook_config": {"uris": ["https://example.com/hook"], "future_webhook_knob": {"a": 1}}
    });
    let response: InteractionResponse = serde_json::from_value(served.clone()).unwrap();

    let config = response.generation_config.as_ref().unwrap();
    assert!(config.extra.is_empty(), "{:?}", config.extra);
    let speech = &config.speech_config.as_ref().unwrap()[0];
    assert_eq!(speech.extra["future_voice_knob"], 1);
    assert_eq!(
        config.image_config.as_ref().unwrap().extra["future_image_knob"],
        "x"
    );
    assert_eq!(
        config.video_config.as_ref().unwrap().extra["future_video_knob"],
        true
    );
    assert_eq!(
        config.transcription_config.as_ref().unwrap().extra["future_transcription_knob"],
        serde_json::json!([1])
    );
    let Some(crate::ToolChoice::AllowedTools {
        allowed_tools: allowed,
        ..
    }) = &config.tool_choice
    else {
        panic!(
            "expected an allowed_tools restriction: {:?}",
            config.tool_choice
        );
    };
    assert_eq!(allowed.extra["future_choice_knob"], 2);
    assert_eq!(
        response.safety_settings.as_ref().unwrap()[0].extra["future_safety_knob"],
        3
    );
    assert_eq!(
        response.webhook_config.as_ref().unwrap().extra["future_webhook_knob"],
        serde_json::json!({"a": 1})
    );

    let back = serde_json::to_value(&response).unwrap();
    for pointer in [
        "/generation_config/speech_config/0/future_voice_knob",
        "/generation_config/image_config/future_image_knob",
        "/generation_config/video_config/future_video_knob",
        "/generation_config/transcription_config/future_transcription_knob",
        "/generation_config/tool_choice/allowed_tools/future_choice_knob",
        "/safety_settings/0/future_safety_knob",
        "/webhook_config/future_webhook_knob",
    ] {
        assert_eq!(
            back.pointer(pointer),
            served.pointer(pointer),
            "{pointer} did not round-trip"
        );
    }
}
