//! Unit tests for the built-in tool accessors on `InteractionResponse`:
//! code execution, Google Search, URL context, Google Maps and file search.

use super::*;
use crate::*;

// --- Built-in Tool Helper Tests ---

#[test]
fn test_interaction_response_code_execution_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::model_text("Here's the code:"),
            Step::CodeExecutionCall {
                id: "call_123".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print(42)".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::CodeExecutionResult {
                call_id: "call_123".to_string(),
                is_error: false,
                result: "42\n".to_string(),
                signature: None,
                extra: Default::default(),
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
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleSearchResult {
                call_id: "call123".to_string(),
                result: vec![GoogleSearchResultItem::new("<div>Test</div>")],
                is_error: None,
                signature: None,
                extra: Default::default(),
            },
            Step::model_text("Based on search results..."),
        ],
        ..Default::default()
    };

    assert!(response.has_google_search_results());

    let search_results = response.google_search_results();
    assert_eq!(search_results.len(), 1);
    assert_eq!(
        search_results[0].search_suggestions.as_deref(),
        Some("<div>Test</div>")
    );
}

#[test]
fn test_interaction_response_url_context_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::UrlContextResult {
            call_id: "ctx_123".to_string(),
            result: vec![UrlContextResultItem::new("https://example.com", "success")],
            is_error: None,
            signature: None,
            extra: Default::default(),
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
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleMapsResult {
                call_id: "maps_123".to_string(),
                signature: None,
                result: vec![GoogleMapsResultItem {
                    places: Some(vec![Place {
                        name: Some("Eiffel Tower".to_string()),
                        place_id: Some("ChIJLU7jZClu5kcR".to_string()),
                        url: Some("https://maps.google.com/maps?cid=1".to_string()),
                        ..Default::default()
                    }]),
                    widget_context_token: Some("token123".to_string()),
                    extra: Default::default(),
                }],
                extra: Default::default(),
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
        places[0].url.as_deref(),
        Some("https://maps.google.com/maps?cid=1")
    );
}

#[test]
fn test_interaction_response_google_maps_helpers_empty() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No maps here.")],
        ..Default::default()
    };

    assert!(!response.has_google_maps_results());
    assert!(response.google_maps_results().is_empty());
}

// --- Google Search Helpers ---

#[test]
fn test_interaction_response_google_search_call_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleSearchCall {
                id: "search1".to_string(),
                queries: vec!["Rust programming language".to_string()],
                search_type: None,
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::GoogleSearchCall {
                id: "search2".to_string(),
                queries: vec!["async await Rust".to_string()],
                search_type: None,
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::model_text("Search results..."),
        ],
        ..Default::default()
    };

    assert!(response.has_google_search_calls());

    let queries = response.google_search_calls();
    assert_eq!(queries.len(), 2);
    assert_eq!(queries[0], "Rust programming language");
    assert_eq!(queries[1], "async await Rust");
}

#[test]
fn test_interaction_response_no_google_search_calls() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No search")],
        ..Default::default()
    };

    assert!(!response.has_google_search_calls());
    assert!(response.google_search_calls().is_empty());
}

// --- URL Context Helpers ---

#[test]
fn test_interaction_response_url_context_call_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::UrlContextCall {
                id: "ctx_1".to_string(),
                urls: vec!["https://docs.rs".to_string()],
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::UrlContextCall {
                id: "ctx_2".to_string(),
                urls: vec![
                    "https://rust-lang.org".to_string(),
                    "https://crates.io".to_string(),
                ],
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    assert!(response.has_url_context_calls());

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
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![],
        ..Default::default()
    };

    assert!(!response.has_url_context_calls());
    assert!(response.url_context_call_urls().is_empty());
}

// --- Code Execution Helpers ---

#[test]
fn test_interaction_response_code_execution_call_singular() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionCall {
                id: "call_first".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('first')".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::CodeExecutionCall {
                id: "call_second".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('second')".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    let calls = response.code_execution_calls();
    let call = calls.first().expect("first call");
    assert_eq!(call.id, "call_first");
    assert_eq!(call.language, CodeExecutionLanguage::Python);
    assert_eq!(call.code, "print('first')");
}

#[test]
fn test_interaction_response_no_code_execution_call() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No code")],
        ..Default::default()
    };

    assert!(response.code_execution_calls().is_empty());
}

#[test]
fn test_interaction_response_code_execution_calls_plural() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionCall {
                id: "call_1".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('first')".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::CodeExecutionCall {
                id: "call_2".to_string(),
                language: CodeExecutionLanguage::Python,
                code: "print('second')".to_string(),
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
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
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::CodeExecutionResult {
                call_id: "call_1".to_string(),
                is_error: false,
                result: "first output".to_string(),
                signature: None,
                extra: Default::default(),
            },
            Step::CodeExecutionResult {
                call_id: "call_2".to_string(),
                is_error: true,
                result: "error message".to_string(),
                signature: None,
                extra: Default::default(),
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
        model: Some("test-model".to_string()),
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
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![
            Step::GoogleSearchResult {
                call_id: "search1".to_string(),
                result: vec![GoogleSearchResultItem::new("<div>Rust Lang</div>")],
                is_error: None,
                signature: None,
                extra: Default::default(),
            },
            Step::GoogleSearchResult {
                call_id: "search2".to_string(),
                result: vec![GoogleSearchResultItem::new("<div>Cargo</div>")],
                is_error: None,
                signature: None,
                extra: Default::default(),
            },
        ],
        ..Default::default()
    };

    assert!(response.has_google_search_results());

    let results = response.google_search_results();
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].search_suggestions.as_deref(),
        Some("<div>Rust Lang</div>")
    );
    assert_eq!(
        results[1].search_suggestions.as_deref(),
        Some("<div>Cargo</div>")
    );
}

#[test]
fn test_interaction_response_no_google_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
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
        model: Some("test-model".to_string()),
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
                extra: Default::default(),
            },
            Step::UrlContextResult {
                call_id: "ctx_2".to_string(),
                result: vec![UrlContextResultItem::new("https://blocked.com", "error")],
                is_error: None,
                signature: None,
                extra: Default::default(),
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
        model: Some("test-model".to_string()),
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
        model: Some("test-model".to_string()),
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
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::CodeExecutionResult {
                call_id: "code-exec-001".to_string(),
                is_error: false,
                result: "4".to_string(),
                signature: None,
                extra: Default::default(),
            },
            // Google search
            Step::GoogleSearchCall {
                id: "gsearch-001".to_string(),
                queries: vec!["weather in Tokyo".to_string()],
                search_type: None,
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::GoogleSearchResult {
                call_id: "gsearch-001".to_string(),
                result: vec![GoogleSearchResultItem::new("<div>Tokyo Weather</div>")],
                is_error: None,
                signature: None,
                extra: Default::default(),
            },
            // URL context
            Step::UrlContextCall {
                id: "ctx_123".to_string(),
                urls: vec!["https://example.com".to_string()],
                signature: None,
                arguments_extra: Default::default(),
                extra: Default::default(),
            },
            Step::UrlContextResult {
                call_id: "ctx_123".to_string(),
                result: vec![UrlContextResultItem::new("https://example.com", "success")],
                is_error: None,
                signature: None,
                extra: Default::default(),
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
    assert!(json_str.contains("test-model"), "Should contain model");
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
        json_str.contains("Tokyo Weather"),
        "Should contain the search suggestions"
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
    assert_eq!(deserialized.model, Some("test-model".to_string()));
    assert_eq!(deserialized.status, InteractionStatus::Completed);
    assert_eq!(
        deserialized.previous_interaction_id,
        Some("previous-interaction-abc".to_string())
    );

    // Verify steps have correct count
    assert_eq!(deserialized.steps.len(), 10);

    // Verify thought signature survived
    let signatures: Vec<_> = deserialized.thought_signatures();
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
    assert_eq!(
        search_results[0].search_suggestions.as_deref(),
        Some("<div>Tokyo Weather</div>")
    );

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

// --- File Search Result Helper Tests ---

#[test]
fn test_interaction_response_has_file_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::FileSearchResult {
            call_id: "call_123".to_string(),
            signature: None,
            extra: Default::default(),
        }],
        ..Default::default()
    };

    assert!(response.has_file_search_results());
}

#[test]
fn test_interaction_response_no_file_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("test-model".to_string()),
        status: InteractionStatus::Completed,
        steps: vec![Step::model_text("No file search here")],
        ..Default::default()
    };

    assert!(!response.has_file_search_results());
}
