//! Unit tests for InteractionResponse, UsageMetadata, StepSummary,
//! InteractionStatus, and helpers (API revision 2026-05-20).
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
        "model": "gemini-3-flash-preview",
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
    assert_eq!(response.model.as_deref(), Some("gemini-3-flash-preview"));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert_eq!(response.steps.len(), 2);
    assert_eq!(response.as_text(), Some("Hi there!"));
    assert!(response.usage.is_some());
    let usage = response.usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(5));
    assert_eq!(usage.total_output_tokens, Some(10));
    assert_eq!(usage.total_tokens, Some(15));
}

// --- UsageMetadata Tests ---

#[test]
fn test_deserialize_usage_metadata_partial() {
    // Test that partial usage responses deserialize correctly with #[serde(default)]
    let partial_json = r#"{"total_tokens": 42}"#;
    let usage: UsageMetadata = serde_json::from_str(partial_json).expect("Deserialization failed");

    assert_eq!(usage.total_tokens, Some(42));
    assert_eq!(usage.total_input_tokens, None);
    assert_eq!(usage.total_output_tokens, None);
    assert_eq!(usage.total_cached_tokens, None);
    assert_eq!(usage.total_thought_tokens, None);
    assert_eq!(usage.total_tool_use_tokens, None);
}

#[test]
fn test_deserialize_usage_metadata_empty() {
    // Test that empty usage object deserializes to defaults
    let empty_json = r#"{}"#;
    let usage: UsageMetadata = serde_json::from_str(empty_json).expect("Deserialization failed");

    assert_eq!(usage.total_tokens, None);
    assert_eq!(usage.total_input_tokens, None);
    assert_eq!(usage.total_output_tokens, None);
}

#[test]
fn test_usage_metadata_has_data() {
    // Empty usage has no data
    let empty = UsageMetadata::default();
    assert!(!empty.has_data());

    // Usage with only total_tokens
    let with_total = UsageMetadata {
        total_tokens: Some(100),
        ..Default::default()
    };
    assert!(with_total.has_data());

    // Usage with only cached tokens
    let with_cached = UsageMetadata {
        total_cached_tokens: Some(50),
        ..Default::default()
    };
    assert!(with_cached.has_data());
}

#[test]
fn test_usage_metadata_grounding_tool_count() {
    // grounding_tool_count is new in revision 2026-05-20
    let json = r#"{
        "total_tokens": 100,
        "grounding_tool_count": [
            {"type": "google_search", "count": 2},
            {"type": "google_maps", "count": 1}
        ]
    }"#;
    let usage: UsageMetadata = serde_json::from_str(json).expect("Deserialization failed");

    assert!(usage.has_data());
    let counts = usage.grounding_tool_count.as_ref().expect("Should exist");
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].tool_type.as_deref(), Some("google_search"));
    assert_eq!(counts[0].count, Some(2));

    assert_eq!(usage.grounding_count_for_tool("google_search"), Some(2));
    assert_eq!(usage.grounding_count_for_tool("google_maps"), Some(1));
    assert_eq!(usage.grounding_count_for_tool("retrieval"), None);

    // The wire field is "type" (snake_case)
    let reserialized = serde_json::to_value(&usage).unwrap();
    assert_eq!(
        reserialized["grounding_tool_count"][0]["type"],
        "google_search"
    );
}

// --- Response Helper Tests ---

#[test]
fn test_interaction_response_text() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::thought("sig_1"),
            Step::thought("sig_2"),
            Step::model_text("The answer is 42."),
            // Thought with None signature should be filtered out
            Step::Thought {
                signature: None,
                summary: vec![],
            },
        ],
        ..Default::default()
    };

    assert!(response.has_thoughts());

    let signatures: Vec<_> = response.thought_signatures().collect();
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
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text, no thoughts.")],
        ..Default::default()
    };

    assert!(!response.has_thoughts());
    let signatures: Vec<_> = response.thought_signatures().collect();
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
            },
            Step::model_text("Answer"),
        ],
        ..Default::default()
    };

    let summaries: Vec<_> = response.thought_summaries().collect();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].as_text(), Some("Summarized reasoning"));
}

#[test]
fn test_interaction_response_function_calls() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
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

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_function_call_step_without_id_becomes_unknown() {
    // The 2026-05-20 spec requires an id on function_call steps. A payload
    // missing it can no longer be represented as Step::FunctionCall (the old
    // Content model used Option<String>), so it degrades gracefully to
    // Step::Unknown with the data preserved.
    let response_json = r#"{
        "id": "test_id",
        "model": "gemini-3-flash-preview",
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
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Normal response")],
        ..Default::default()
    };

    assert!(!response.has_unknown());
    assert!(response.unknown_steps().is_empty());
}

// --- StepSummary Tests ---

#[test]
fn test_step_summary() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![Content::text("Text 1"), Content::text("Text 2")]),
            Step::thought("sig_thinking"),
            Step::function_call("call_1", "test_fn", serde_json::json!({})),
            Step::Unknown {
                step_type: "type_a".to_string(),
                data: serde_json::json!({"type": "type_a"}),
            },
            Step::Unknown {
                step_type: "type_b".to_string(),
                data: serde_json::json!({"type": "type_b"}),
            },
            Step::Unknown {
                step_type: "type_a".to_string(), // Duplicate type
                data: serde_json::json!({"type": "type_a", "extra": true}),
            },
        ],
        ..Default::default()
    };

    let summary = response.step_summary();

    assert_eq!(summary.model_output_count, 1);
    assert_eq!(summary.text_count, 2);
    assert_eq!(summary.thought_count, 1);
    assert_eq!(summary.function_call_count, 1);
    assert_eq!(summary.unknown_count, 3);
    // Unknown types should be deduplicated and sorted
    assert_eq!(summary.unknown_types.len(), 2);
    assert_eq!(summary.unknown_types, vec!["type_a", "type_b"]);
}

#[test]
fn test_step_summary_empty() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    let summary = response.step_summary();

    assert_eq!(summary.text_count, 0);
    assert_eq!(summary.unknown_count, 0);
    assert!(summary.unknown_types.is_empty());
}

#[test]
fn test_step_summary_display() {
    // Test Display for StepSummary with various counts
    let summary = StepSummary {
        text_count: 2,
        thought_count: 1,
        code_execution_call_count: 1,
        code_execution_result_count: 1,
        ..Default::default()
    };
    let display = format!("{}", summary);
    assert!(display.contains("2 text"));
    assert!(display.contains("1 thought"));
    assert!(display.contains("1 code_execution_call"));
    assert!(display.contains("1 code_execution_result"));
    // Should not contain zero-count items
    assert!(!display.contains("image"));
    assert!(!display.contains("audio"));
}

#[test]
fn test_step_summary_display_empty() {
    let summary = StepSummary::default();
    assert_eq!(format!("{}", summary), "empty");
}

#[test]
fn test_step_summary_display_with_unknown() {
    let summary = StepSummary {
        unknown_count: 2,
        unknown_types: vec!["new_type_a".to_string(), "new_type_b".to_string()],
        ..Default::default()
    };
    let display = format!("{}", summary);
    assert!(display.contains("2 unknown"));
    assert!(display.contains("new_type_a"));
    assert!(display.contains("new_type_b"));
}

#[test]
fn test_step_summary_with_built_in_tools() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionCall {
                id: "call_1".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(1)".to_string(),
                signature: None,
            },
            Step::CodeExecutionCall {
                id: "call_2".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(2)".to_string(),
                signature: None,
            },
            Step::CodeExecutionResult {
                call_id: "call_1".to_string(),
                is_error: false,
                result: "1\n2\n".to_string(),
                signature: None,
            },
            Step::GoogleSearchCall {
                id: "search1".to_string(),
                queries: vec!["test".to_string()],
                search_type: None,
                signature: None,
            },
            Step::GoogleSearchResult {
                call_id: "search1".to_string(),
                result: vec![],
                is_error: None,
                signature: None,
            },
            Step::UrlContextCall {
                id: "ctx_123".to_string(),
                urls: vec!["https://example.com".to_string()],
                signature: None,
            },
            Step::UrlContextResult {
                call_id: "ctx_123".to_string(),
                result: vec![],
                is_error: None,
                signature: None,
            },
        ],
        ..Default::default()
    };

    let summary = response.step_summary();

    assert_eq!(summary.code_execution_call_count, 2);
    assert_eq!(summary.code_execution_result_count, 1);
    assert_eq!(summary.google_search_call_count, 1);
    assert_eq!(summary.google_search_result_count, 1);
    assert_eq!(summary.url_context_call_count, 1);
    assert_eq!(summary.url_context_result_count, 1);
    assert_eq!(summary.unknown_count, 0);
}

// --- Built-in Tool Helper Tests ---

#[test]
fn test_interaction_response_code_execution_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_text("Here's the code:"),
            Step::CodeExecutionCall {
                id: "call_123".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(42)".to_string(),
                signature: None,
            },
            Step::CodeExecutionResult {
                call_id: "call_123".to_string(),
                is_error: false,
                result: "42\n".to_string(),
                signature: None,
            },
        ],
        ..Default::default()
    };

    assert!(response.has_code_execution_calls());
    assert!(response.has_code_execution_results());
    assert!(!response.has_unknown());

    // Test code_execution_calls helper (id is now a required &str)
    let code_blocks = response.code_execution_calls();
    assert_eq!(code_blocks.len(), 1);
    assert_eq!(code_blocks[0].id, "call_123");
    assert_eq!(code_blocks[0].language, CodeExecutionLanguage::Python);
    assert_eq!(code_blocks[0].code, "print(42)");

    // Test code_execution_results helper
    let results = response.code_execution_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].call_id, "call_123");
    assert!(!results[0].is_error);
    assert_eq!(results[0].result, "42\n");

    // Test successful_code_output helper
    assert_eq!(response.successful_code_output(), Some("42\n"));
}

#[test]
fn test_interaction_response_google_search_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleSearchResult {
                call_id: "call123".to_string(),
                result: vec![GoogleSearchResultItem::new("Test", "https://example.com")],
                is_error: None,
                signature: None,
            },
            Step::model_text("Based on search results..."),
        ],
        ..Default::default()
    };

    assert!(response.has_google_search_results());

    let search_results = response.google_search_results();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].title, "Test");
    assert_eq!(search_results[0].url, "https://example.com");
}

#[test]
fn test_interaction_response_url_context_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::UrlContextResult {
            call_id: "ctx_123".to_string(),
            result: vec![UrlContextResultItem::new("https://example.com", "success")],
            is_error: None,
            signature: None,
        }],
        ..Default::default()
    };

    assert!(response.has_url_context_results());

    let url_results = response.url_context_results();
    assert_eq!(url_results.len(), 1);
    assert_eq!(url_results[0].call_id, "ctx_123");
    assert_eq!(url_results[0].items.len(), 1);
    assert_eq!(url_results[0].items[0].url, "https://example.com");
    assert!(url_results[0].items[0].is_success());
}

#[test]
fn test_interaction_response_google_maps_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleMapsResult {
                call_id: "maps_123".to_string(),
                signature: None,
                result: vec![GoogleMapsResultItem {
                    places: Some(vec![Place {
                        name: Some("Eiffel Tower".to_string()),
                        formatted_address: Some("Paris, France".to_string()),
                        place_id: Some("ChIJLU7jZClu5kcR".to_string()),
                        lat: Some(48.8584),
                        lng: Some(2.2945),
                        ..Default::default()
                    }]),
                    widget_context_token: Some("token123".to_string()),
                }],
            },
            Step::model_text("Here are the maps results."),
        ],
        ..Default::default()
    };

    assert!(response.has_google_maps_results());

    let maps_results = response.google_maps_results();
    assert_eq!(maps_results.len(), 1);
    assert_eq!(maps_results[0].call_id, "maps_123");
    assert_eq!(maps_results[0].items.len(), 1);

    let places = maps_results[0].items[0].places.as_ref().unwrap();
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].name.as_deref(), Some("Eiffel Tower"));
    assert_eq!(
        places[0].formatted_address.as_deref(),
        Some("Paris, France")
    );
}

#[test]
fn test_interaction_response_google_maps_helpers_empty() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No maps here.")],
        ..Default::default()
    };

    assert!(!response.has_google_maps_results());
    assert!(response.google_maps_results().is_empty());
}

// --- Function Result Helpers ---

#[test]
fn test_interaction_response_function_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text")],
        ..Default::default()
    };

    assert!(!response.has_function_results());
    assert!(response.function_results().is_empty());
}

// --- Google Search Helpers ---

#[test]
fn test_interaction_response_google_search_call_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleSearchCall {
                id: "search1".to_string(),
                queries: vec!["Rust programming language".to_string()],
                search_type: None,
                signature: None,
            },
            Step::GoogleSearchCall {
                id: "search2".to_string(),
                queries: vec!["async await Rust".to_string()],
                search_type: None,
                signature: None,
            },
            Step::model_text("Search results..."),
        ],
        ..Default::default()
    };

    assert!(response.has_google_search_calls());

    // Test google_search_call() - returns first one
    assert_eq!(
        response.google_search_call(),
        Some("Rust programming language")
    );

    // Test google_search_calls() - returns all
    let queries = response.google_search_calls();
    assert_eq!(queries.len(), 2);
    assert_eq!(queries[0], "Rust programming language");
    assert_eq!(queries[1], "async await Rust");
}

#[test]
fn test_interaction_response_no_google_search_calls() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No search")],
        ..Default::default()
    };

    assert!(!response.has_google_search_calls());
    assert!(response.google_search_call().is_none());
    assert!(response.google_search_calls().is_empty());
}

// --- URL Context Helpers ---

#[test]
fn test_interaction_response_url_context_call_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::UrlContextCall {
                id: "ctx_1".to_string(),
                urls: vec!["https://docs.rs".to_string()],
                signature: None,
            },
            Step::UrlContextCall {
                id: "ctx_2".to_string(),
                urls: vec![
                    "https://rust-lang.org".to_string(),
                    "https://crates.io".to_string(),
                ],
                signature: None,
            },
        ],
        ..Default::default()
    };

    assert!(response.has_url_context_calls());

    // Test url_context_call_id() - returns first call ID
    assert_eq!(response.url_context_call_id(), Some("ctx_1"));

    // Test url_context_call_urls() - returns all URLs flattened
    let urls = response.url_context_call_urls();
    assert_eq!(urls.len(), 3);
    assert_eq!(urls[0], "https://docs.rs");
    assert_eq!(urls[1], "https://rust-lang.org");
    assert_eq!(urls[2], "https://crates.io");
}

#[test]
fn test_interaction_response_no_url_context_calls() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    assert!(!response.has_url_context_calls());
    assert!(response.url_context_call_id().is_none());
    assert!(response.url_context_call_urls().is_empty());
}

// --- Code Execution Helpers ---

#[test]
fn test_interaction_response_code_execution_call_singular() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionCall {
                id: "call_first".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('first')".to_string(),
                signature: None,
            },
            Step::CodeExecutionCall {
                id: "call_second".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('second')".to_string(),
                signature: None,
            },
        ],
        ..Default::default()
    };

    // Test code_execution_call() - returns first one
    let call = response.code_execution_call();
    assert!(call.is_some());
    let call = call.unwrap();
    assert_eq!(call.id, "call_first");
    assert_eq!(call.language, CodeExecutionLanguage::Python);
    assert_eq!(call.code, "print('first')");
}

#[test]
fn test_interaction_response_no_code_execution_call() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No code")],
        ..Default::default()
    };

    assert!(response.code_execution_call().is_none());
}

#[test]
fn test_interaction_response_code_execution_calls_plural() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionCall {
                id: "call_1".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('first')".to_string(),
                signature: None,
            },
            Step::CodeExecutionCall {
                id: "call_2".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('second')".to_string(),
                signature: None,
            },
            Step::model_text("Results"),
        ],
        ..Default::default()
    };

    assert!(response.has_code_execution_calls());

    let calls = response.code_execution_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].language, CodeExecutionLanguage::Python);
    assert_eq!(calls[0].code, "print('first')");
    assert_eq!(calls[1].id, "call_2");
    assert_eq!(calls[1].language, CodeExecutionLanguage::Python);
    assert_eq!(calls[1].code, "print('second')");
}

#[test]
fn test_interaction_response_code_execution_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionResult {
                call_id: "call_1".to_string(),
                is_error: false,
                result: "first output".to_string(),
                signature: None,
            },
            Step::CodeExecutionResult {
                call_id: "call_2".to_string(),
                is_error: true,
                result: "error message".to_string(),
                signature: None,
            },
        ],
        ..Default::default()
    };

    assert!(response.has_code_execution_results());

    let results = response.code_execution_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].call_id, "call_1");
    assert!(!results[0].is_error);
    assert_eq!(results[0].result, "first output");
    assert_eq!(results[1].call_id, "call_2");
    assert!(results[1].is_error);
    assert_eq!(results[1].result, "error message");

    // Test successful_code_output - should return first successful output
    let success = response.successful_code_output();
    assert_eq!(success, Some("first output"));
}

#[test]
fn test_interaction_response_no_code_execution_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No code")],
        ..Default::default()
    };

    assert!(!response.has_code_execution_results());
    assert!(response.code_execution_results().is_empty());
    assert!(response.successful_code_output().is_none());
}

#[test]
fn test_interaction_response_google_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleSearchResult {
                call_id: "search1".to_string(),
                result: vec![GoogleSearchResultItem::new(
                    "Rust Lang",
                    "https://rust-lang.org",
                )],
                is_error: None,
                signature: None,
            },
            Step::GoogleSearchResult {
                call_id: "search2".to_string(),
                result: vec![GoogleSearchResultItem::new(
                    "Cargo",
                    "https://doc.rust-lang.org/cargo/",
                )],
                is_error: None,
                signature: None,
            },
        ],
        ..Default::default()
    };

    assert!(response.has_google_search_results());

    let results = response.google_search_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Rust Lang");
    assert_eq!(results[1].title, "Cargo");
}

#[test]
fn test_interaction_response_no_google_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    assert!(!response.has_google_search_results());
    assert!(response.google_search_results().is_empty());
}

#[test]
fn test_interaction_response_url_context_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::UrlContextResult {
                call_id: "ctx_1".to_string(),
                result: vec![
                    UrlContextResultItem::new("https://docs.rs", "success"),
                    UrlContextResultItem::new("https://crates.io", "success"),
                ],
                is_error: None,
                signature: None,
            },
            Step::UrlContextResult {
                call_id: "ctx_2".to_string(),
                result: vec![UrlContextResultItem::new("https://blocked.com", "error")],
                is_error: None,
                signature: None,
            },
        ],
        ..Default::default()
    };

    assert!(response.has_url_context_results());

    let results = response.url_context_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].call_id, "ctx_1");
    assert_eq!(results[0].items.len(), 2);
    assert!(results[0].items[0].is_success());
    assert!(results[0].items[1].is_success());
    assert_eq!(results[1].call_id, "ctx_2");
    assert_eq!(results[1].items.len(), 1);
    assert!(results[1].items[0].is_error());
}

#[test]
fn test_interaction_response_no_url_context_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    assert!(!response.has_url_context_results());
    assert!(response.url_context_results().is_empty());
}

/// Comprehensive roundtrip test for InteractionResponse with many step types.
///
/// This test verifies that complex responses with multiple step types,
/// function calls, thoughts, and tool activity can be serialized and
/// deserialized without data loss. This is critical for save/resume semantics.
#[test]
fn test_interaction_response_complex_roundtrip() {
    // Build a response with many different step types
    let response = InteractionResponse {
        id: Some("complex-interaction-xyz".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            // Thought with signature (thinking models)
            Step::thought("thought-sig-abc123"),
            // Function call
            Step::function_call(
                "call-func-001",
                "get_weather",
                serde_json::json!({"city": "Tokyo", "units": "celsius"}),
            ),
            // Function result
            Step::function_result(
                "get_weather",
                "call-func-001",
                serde_json::json!({"temp": 22, "conditions": "sunny"}),
            ),
            // Code execution
            Step::CodeExecutionCall {
                id: "code-exec-001".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(2 + 2)".to_string(),
                signature: None,
            },
            Step::CodeExecutionResult {
                call_id: "code-exec-001".to_string(),
                is_error: false,
                result: "4".to_string(),
                signature: None,
            },
            // Google search
            Step::GoogleSearchCall {
                id: "gsearch-001".to_string(),
                queries: vec!["weather in Tokyo".to_string()],
                search_type: None,
                signature: None,
            },
            Step::GoogleSearchResult {
                call_id: "gsearch-001".to_string(),
                result: vec![GoogleSearchResultItem::new(
                    "Tokyo Weather",
                    "https://weather.example.com/tokyo",
                )],
                is_error: None,
                signature: None,
            },
            // URL context
            Step::UrlContextCall {
                id: "ctx_123".to_string(),
                urls: vec!["https://example.com".to_string()],
                signature: None,
            },
            Step::UrlContextResult {
                call_id: "ctx_123".to_string(),
                result: vec![UrlContextResultItem::new("https://example.com", "success")],
                is_error: None,
                signature: None,
            },
            // Final model output (text + image)
            Step::model_output(vec![
                Content::text("The weather in Tokyo is 22°C and sunny."),
                Content::image_data("base64encodeddata", "image/png"),
            ]),
        ],
        usage: Some(UsageMetadata {
            total_input_tokens: Some(150),
            total_output_tokens: Some(200),
            total_tokens: Some(350),
            total_cached_tokens: Some(50),
            total_thought_tokens: Some(30),
            total_tool_use_tokens: Some(20),
            ..Default::default()
        }),
        tools: Some(vec![
            crate::Tool::GoogleSearch { search_types: None },
            crate::Tool::CodeExecution,
        ]),
        previous_interaction_id: Some("previous-interaction-abc".to_string()),
        ..Default::default()
    };

    // Serialize to JSON
    let json_str = serde_json::to_string(&response).expect("Serialization should succeed");

    // Verify key data is present in serialized JSON
    assert!(
        json_str.contains("complex-interaction-xyz"),
        "Should contain ID"
    );
    assert!(
        json_str.contains("gemini-3-flash-preview"),
        "Should contain model"
    );
    assert!(
        json_str.contains("get_weather"),
        "Should contain function name"
    );
    assert!(
        json_str.contains("call-func-001"),
        "Should contain function call ID"
    );
    assert!(json_str.contains("Tokyo"), "Should contain city");
    assert!(
        json_str.contains("thought-sig-abc123"),
        "Should contain thought signature"
    );
    assert!(json_str.contains("print(2 + 2)"), "Should contain code");
    assert!(
        json_str.contains("weather.example.com"),
        "Should contain search result URL"
    );
    assert!(
        json_str.contains("previous-interaction-abc"),
        "Should contain previous ID"
    );

    // Deserialize back
    let deserialized: InteractionResponse =
        serde_json::from_str(&json_str).expect("Deserialization should succeed");

    // Verify top-level fields
    assert_eq!(deserialized.id.as_deref(), Some("complex-interaction-xyz"));
    assert_eq!(
        deserialized.model,
        Some("gemini-3-flash-preview".to_string())
    );
    assert_eq!(deserialized.status, InteractionStatus::Completed);
    assert_eq!(
        deserialized.previous_interaction_id,
        Some("previous-interaction-abc".to_string())
    );

    // Verify steps have correct count
    assert_eq!(deserialized.steps.len(), 10);

    // Verify thought signature survived
    let signatures: Vec<_> = deserialized.thought_signatures().collect();
    assert_eq!(signatures, vec!["thought-sig-abc123"]);

    // Verify function calls are accessible
    let function_calls = deserialized.function_calls();
    assert_eq!(function_calls.len(), 1);
    assert_eq!(function_calls[0].name, "get_weather");
    assert_eq!(function_calls[0].id, "call-func-001");
    assert_eq!(function_calls[0].args["city"], "Tokyo");

    // Verify function results
    let function_results = deserialized.function_results();
    assert_eq!(function_results.len(), 1);
    assert_eq!(
        function_results[0].result.as_json().unwrap()["conditions"],
        "sunny"
    );

    // Verify code execution results
    let code_results = deserialized.code_execution_results();
    assert_eq!(code_results.len(), 1);
    assert!(!code_results[0].is_error);
    assert_eq!(code_results[0].result, "4");

    // Verify Google Search results
    let search_results = deserialized.google_search_results();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].title, "Tokyo Weather");

    // Verify URL context results
    let url_results = deserialized.url_context_results();
    assert_eq!(url_results.len(), 1);
    assert_eq!(url_results[0].call_id, "ctx_123");
    assert_eq!(url_results[0].items.len(), 1);
    assert!(url_results[0].items[0].is_success());

    // Verify model output text and image survived
    assert_eq!(
        deserialized.as_text(),
        Some("The weather in Tokyo is 22°C and sunny.")
    );
    assert!(deserialized.has_images());

    // Verify usage metadata
    let usage = deserialized.usage.expect("Should have usage");
    assert_eq!(usage.total_input_tokens, Some(150));
    assert_eq!(usage.total_output_tokens, Some(200));
    assert_eq!(usage.total_tokens, Some(350));
    assert_eq!(usage.total_cached_tokens, Some(50));
    assert_eq!(usage.total_thought_tokens, Some(30));
    assert_eq!(usage.total_tool_use_tokens, Some(20));

    // Verify tools
    let tools = deserialized.tools.expect("Should have tools");
    assert_eq!(tools.len(), 2);
    assert!(matches!(tools[0], crate::Tool::GoogleSearch { .. }));
    assert!(matches!(tools[1], crate::Tool::CodeExecution));
}

// --- InteractionStatus Tests ---

#[test]
fn test_interaction_status_unknown_deserialize() {
    // Simulate a new API status that this library doesn't know about
    let json = r#""future_pending_state""#;
    let status: InteractionStatus = serde_json::from_str(json).expect("Should deserialize");

    assert!(status.is_unknown());
    assert_eq!(status.unknown_status_type(), Some("future_pending_state"));
    assert!(status.unknown_data().is_some());
}

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
        "model": "gemini-3-flash-preview",
        "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi there!"}]}],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(json).expect("Deserialization should succeed without id");

    assert!(response.id.is_none(), "ID should be None when not present");
    assert_eq!(response.model, Some("gemini-3-flash-preview".to_string()));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(!response.steps.is_empty());
}

#[test]
fn test_interaction_response_deserialize_with_id() {
    // When store=true (or by default), the API response includes an id field.
    // This test verifies that we can still deserialize such responses correctly.
    let json = r#"{
        "id": "interaction-abc123",
        "model": "gemini-3-flash-preview",
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
    assert_eq!(response.model, Some("gemini-3-flash-preview".to_string()));
    assert_eq!(response.status, InteractionStatus::Completed);
}

#[test]
fn test_interaction_response_serialize_without_id() {
    // When id is None, it should not be serialized into the JSON output.
    // This uses skip_serializing_if to avoid "id": null in the output.
    let response = InteractionResponse {
        id: None,
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Hello")],
        ..Default::default()
    };

    let json = serde_json::to_string(&response).expect("Serialization should succeed");

    assert!(
        !json.contains(r#""id""#),
        "JSON should not contain id field when None: {}",
        json
    );
}

#[test]
fn test_interaction_response_roundtrip_without_id() {
    // Verify roundtrip serialization works correctly when id is None.
    let original = InteractionResponse {
        id: None,
        model: Some("gemini-3-flash-preview".to_string()),
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

// --- OwnedFunctionCallInfo Tests ---

#[test]
fn test_function_call_info_to_owned() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::function_call(
            "call_123",
            "get_weather",
            serde_json::json!({"city": "Tokyo", "units": "celsius"}),
        )],
        ..Default::default()
    };

    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);

    // Convert to owned
    let owned = calls[0].to_owned();

    // Verify all fields are correctly converted (id is an owned String now)
    assert_eq!(owned.id, "call_123");
    assert_eq!(owned.name, "get_weather");
    assert_eq!(owned.args["city"], "Tokyo");
    assert_eq!(owned.args["units"], "celsius");
}

#[test]
fn test_owned_function_call_info_outlives_response() {
    // Demonstrate the main use case: owned call can outlive the response
    let owned_calls: Vec<OwnedFunctionCallInfo> = {
        let response = InteractionResponse {
            id: Some("test_id".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            status: InteractionStatus::RequiresAction,
            steps: vec![
                Step::function_call("call_1", "func_a", serde_json::json!({"x": 1})),
                Step::function_call("call_2", "func_b", serde_json::json!({"y": 2})),
            ],
            ..Default::default()
        };

        // Convert to owned before response goes out of scope
        response
            .function_calls()
            .into_iter()
            .map(|call| call.to_owned())
            .collect()
    }; // response is dropped here

    // owned_calls is still valid and usable
    assert_eq!(owned_calls.len(), 2);
    assert_eq!(owned_calls[0].id, "call_1");
    assert_eq!(owned_calls[0].name, "func_a");
    assert_eq!(owned_calls[0].args["x"], 1);
    assert_eq!(owned_calls[1].name, "func_b");
    assert_eq!(owned_calls[1].args["y"], 2);
}

#[test]
fn test_owned_function_call_info_serialization_roundtrip() {
    let owned = OwnedFunctionCallInfo {
        id: "call_xyz".to_string(),
        name: "my_function".to_string(),
        args: serde_json::json!({"key": "value", "number": 42}),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&owned).expect("Serialization should succeed");

    // Verify JSON contains expected data
    assert!(json.contains("call_xyz"));
    assert!(json.contains("my_function"));

    // Deserialize back
    let restored: OwnedFunctionCallInfo =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(restored.id, owned.id);
    assert_eq!(restored.name, owned.name);
    assert_eq!(restored.args, owned.args);
}

#[test]
fn test_owned_function_call_info_clone() {
    let owned = OwnedFunctionCallInfo {
        id: "call_id".to_string(),
        name: "cloneable".to_string(),
        args: serde_json::json!({"data": [1, 2, 3]}),
    };

    let cloned = owned.clone();

    assert_eq!(cloned.id, owned.id);
    assert_eq!(cloned.name, owned.name);
    assert_eq!(cloned.args, owned.args);
}

#[test]
fn test_owned_function_call_info_equality() {
    let owned1 = OwnedFunctionCallInfo {
        id: "same_id".to_string(),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    let owned2 = OwnedFunctionCallInfo {
        id: "same_id".to_string(),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    let different = OwnedFunctionCallInfo {
        id: "different_id".to_string(),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    assert_eq!(owned1, owned2);
    assert_ne!(owned1, different);
}

// --- Annotation Helper Tests ---

#[test]
fn test_interaction_response_has_annotations() {
    // Response with annotations
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![Content::Text {
            text: Some("According to the source, climate change is accelerating.".to_string()),
            annotations: Some(vec![Annotation::url_citation(
                "https://climate.gov",
                None,
                19,
                25,
            )]),
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
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(vec![Content::Text {
            text: Some("Text with empty annotations.".to_string()),
            annotations: Some(vec![]), // Empty array
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
        model: Some("gemini-3-flash-preview".to_string()),
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
            }]),
            Step::thought("sig_thinking"),
            Step::model_output(vec![
                Content::Text {
                    text: Some("Second and third claims.".to_string()),
                    annotations: Some(vec![
                        Annotation::url_citation("https://source-b.com", None, 0, 6),
                        Annotation::url_citation("https://source-c.com", None, 11, 16),
                    ]),
                },
                Content::Text {
                    text: Some("Text without annotations.".to_string()),
                    annotations: None,
                },
            ]),
        ],
        ..Default::default()
    };

    let annotations: Vec<_> = response.all_annotations().collect();

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
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_text("No annotations here."),
            Step::function_call("call_1", "test", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let count = response.all_annotations().count();
    assert_eq!(count, 0);
}

#[test]
fn test_interaction_response_all_annotations_skips_non_text() {
    // Verify all_annotations only looks at Text content, not other types
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
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
                },
            ]),
            Step::CodeExecutionResult {
                call_id: "call_1".to_string(),
                is_error: false,
                result: "result".to_string(),
                signature: None,
            },
        ],
        ..Default::default()
    };

    let annotations: Vec<_> = response.all_annotations().collect();

    // Should only find the one annotation from the Text content
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].source(), Some("https://example.com"));
}

// --- File Search Result Helper Tests ---

#[test]
fn test_interaction_response_has_file_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::FileSearchResult {
            call_id: "call_123".to_string(),
            result: vec![FileSearchResultItem {
                title: "Technical Doc".to_string(),
                text: "Relevant content...".to_string(),
                store: "stores/my-store".to_string(),
            }],
            signature: None,
        }],
        ..Default::default()
    };

    assert!(response.has_file_search_results());
}

#[test]
fn test_interaction_response_no_file_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No file search here")],
        ..Default::default()
    };

    assert!(!response.has_file_search_results());
}

#[test]
fn test_interaction_response_file_search_results_extraction() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::FileSearchResult {
                call_id: "call_1".to_string(),
                result: vec![
                    FileSearchResultItem {
                        title: "Doc 1".to_string(),
                        text: "Content from doc 1".to_string(),
                        store: "stores/store-a".to_string(),
                    },
                    FileSearchResultItem {
                        title: "Doc 2".to_string(),
                        text: "Content from doc 2".to_string(),
                        store: "stores/store-a".to_string(),
                    },
                ],
                signature: None,
            },
            Step::model_text("Summary of search results"),
            Step::FileSearchResult {
                call_id: "call_2".to_string(),
                result: vec![FileSearchResultItem {
                    title: "Doc 3".to_string(),
                    text: "Content from doc 3".to_string(),
                    store: "stores/store-b".to_string(),
                }],
                signature: None,
            },
        ],
        ..Default::default()
    };

    assert!(response.has_file_search_results());

    let results = response.file_search_results();
    // Should collect all items from both FileSearchResult steps
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].title, "Doc 1");
    assert_eq!(results[1].title, "Doc 2");
    assert_eq!(results[2].title, "Doc 3");
    assert_eq!(results[0].store, "stores/store-a");
    assert_eq!(results[2].store, "stores/store-b");
}

#[test]
fn test_interaction_response_file_search_results_empty_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::FileSearchResult {
            call_id: "call_empty".to_string(),
            result: vec![], // Empty results array
            signature: None,
        }],
        ..Default::default()
    };

    // has_file_search_results returns true if the step type exists
    assert!(response.has_file_search_results());

    // But file_search_results returns empty vec since result array is empty
    let results = response.file_search_results();
    assert!(results.is_empty());
}

// =============================================================================
// ImageInfo and images() Tests
// =============================================================================

/// Builds a completed response with the given content in one model_output step.
fn model_output_response(content: Vec<Content>) -> InteractionResponse {
    InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_output(content)],
        ..Default::default()
    }
}

#[test]
fn test_image_info_bytes_decodes_valid_base64() {
    use base64::Engine as _;
    // Valid PNG header in base64
    let png_bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let base64_data = base64::engine::general_purpose::STANDARD.encode(png_bytes);

    let response = model_output_response(vec![Content::image_data(base64_data, "image/png")]);

    let images: Vec<_> = response.images().collect();
    assert_eq!(images.len(), 1);

    let decoded = images[0].bytes().expect("Should decode valid base64");
    assert_eq!(decoded, png_bytes);
}

#[test]
fn test_image_info_bytes_invalid_base64() {
    let response = model_output_response(vec![Content::image_data(
        "not valid base64!!!",
        "image/png",
    )]);

    let images: Vec<_> = response.images().collect();
    assert_eq!(images.len(), 1);

    let result = images[0].bytes();
    assert!(result.is_err());
}

#[test]
fn test_image_info_mime_type() {
    let response = model_output_response(vec![
        Content::image_data("YWJj", "image/jpeg"), // "abc" in base64
        Content::Image {
            data: Some("ZGVm".to_string()), // "def" in base64
            uri: None,
            mime_type: None,
            resolution: None,
        },
    ]);

    let images: Vec<_> = response.images().collect();
    assert_eq!(images.len(), 2);

    assert_eq!(images[0].mime_type(), Some("image/jpeg"));
    assert_eq!(images[1].mime_type(), None);
}

#[test]
fn test_image_info_extension_jpeg() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/jpeg")]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "jpg");
}

#[test]
fn test_image_info_extension_jpg_alternate() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/jpg")]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "jpg");
}

#[test]
fn test_image_info_extension_png() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/png")]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "png");
}

#[test]
fn test_image_info_extension_webp() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/webp")]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "webp");
}

#[test]
fn test_image_info_extension_gif() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/gif")]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "gif");
}

#[test]
fn test_image_info_extension_unknown_defaults_to_png() {
    let response = model_output_response(vec![Content::image_data("YWJj", "image/tiff")]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "png"); // Defaults to png for unknown types
}

#[test]
fn test_image_info_extension_none_mime_type_defaults_to_png() {
    let response = model_output_response(vec![Content::Image {
        data: Some("YWJj".to_string()),
        uri: None,
        mime_type: None,
        resolution: None,
    }]);
    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "png"); // Defaults to png when no MIME type
}

#[test]
fn test_images_iterator_returns_only_images_with_data() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![
                Content::text("Text content"),
                Content::image_data("YWJj", "image/png"), // Has data - included
                Content::image_uri("https://example.com/image.png", "image/png"), // URI-based - excluded
                Content::image_data("ZGVm", "image/jpeg"), // Has data - included
            ]),
            Step::function_call("call_x", "test", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let images: Vec<_> = response.images().collect();
    // Only 2 images with data should be included, not the URI-based one
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].mime_type(), Some("image/png"));
    assert_eq!(images[1].mime_type(), Some("image/jpeg"));
}

#[test]
fn test_images_iterator_empty_when_no_images() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text"), Step::thought("sig_thought")],
        ..Default::default()
    };

    let images: Vec<_> = response.images().collect();
    assert!(images.is_empty());
}

#[test]
fn test_images_iterator_empty_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    let images: Vec<_> = response.images().collect();
    assert!(images.is_empty());
}

// ============================================================================
// AudioInfo Tests
// ============================================================================

#[test]
fn test_audios_iterator_returns_only_audios_with_data() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-2.5-pro-preview-tts".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_output(vec![
                Content::text("Text content"),
                Content::audio_data("YWJj", "audio/L16;codec=pcm;rate=24000"), // Has data - included
                Content::audio_uri("https://example.com/audio.mp3", "audio/mp3"), // URI-based - excluded
                Content::audio_data("ZGVm", "audio/wav"), // Has data - included
            ]),
            Step::function_call("call_y", "test", serde_json::json!({})),
        ],
        ..Default::default()
    };

    let audios: Vec<_> = response.audios().collect();
    // Only 2 audios with data should be included, not the URI-based one
    assert_eq!(audios.len(), 2);
    assert_eq!(
        audios[0].mime_type(),
        Some("audio/L16;codec=pcm;rate=24000")
    );
    assert_eq!(audios[1].mime_type(), Some("audio/wav"));
}

#[test]
fn test_audios_iterator_empty_when_no_audios() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("Just text"), Step::thought("sig_thought")],
        ..Default::default()
    };

    let audios: Vec<_> = response.audios().collect();
    assert!(audios.is_empty());
}

#[test]
fn test_audios_iterator_empty_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-2.5-pro-preview-tts".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    let audios: Vec<_> = response.audios().collect();
    assert!(audios.is_empty());
}

#[test]
fn test_first_audio() {
    let response = model_output_response(vec![
        Content::text("Hello"),
        Content::audio_data("Zmlyc3Q=", "audio/L16;codec=pcm;rate=24000"),
        Content::audio_data("c2Vjb25k", "audio/wav"),
    ]);

    let first = response.first_audio();
    assert!(first.is_some());
    let audio = first.unwrap();
    assert_eq!(audio.mime_type(), Some("audio/L16;codec=pcm;rate=24000"));
    assert_eq!(audio.extension(), "pcm");
}

#[test]
fn test_audio_info_sample_rate_and_channels() {
    // The new Audio fields flow through to AudioInfo accessors
    let response = model_output_response(vec![Content::Audio {
        data: Some("YXVkaW8=".to_string()),
        uri: None,
        mime_type: Some("audio/wav".to_string()),
        sample_rate: Some(24000),
        channels: Some(1),
    }]);

    let audio = response.first_audio().expect("Should have audio");
    assert_eq!(audio.sample_rate(), Some(24000));
    assert_eq!(audio.channels(), Some(1));

    // Absent fields report None
    let response = model_output_response(vec![Content::audio_data("YXVkaW8=", "audio/wav")]);
    let audio = response.first_audio().expect("Should have audio");
    assert_eq!(audio.sample_rate(), None);
    assert_eq!(audio.channels(), None);
}

#[test]
fn test_first_audio_none_when_empty() {
    let response = model_output_response(vec![Content::text("No audio here")]);
    assert!(response.first_audio().is_none());
}

#[test]
fn test_has_audio_true() {
    let response = model_output_response(vec![Content::audio_data(
        "YXVkaW8=",
        "audio/L16;codec=pcm;rate=24000",
    )]);
    assert!(response.has_audio());
}

#[test]
fn test_has_audio_false_when_no_audio() {
    let response = model_output_response(vec![Content::text("Text only")]);
    assert!(!response.has_audio());
}

#[test]
fn test_has_audio_false_when_uri_only() {
    // URI-based audio (no data) should not count as "having audio"
    // since the audios() iterator only includes data-based audio
    let response = model_output_response(vec![Content::audio_uri(
        "https://example.com/audio.mp3",
        "audio/mp3",
    )]);
    assert!(!response.has_audio());
}

// =============================================================================
// output_steps() Tests (replaces the launch-era as_model_turn())
// =============================================================================

#[test]
fn test_output_steps_returns_owned_steps() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
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
        model: Some("gemini-3-flash-preview".to_string()),
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

    let contents: Vec<_> = response.output_contents().collect();
    // Only content from model_output steps is included
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].as_text(), Some("model text"));
}
