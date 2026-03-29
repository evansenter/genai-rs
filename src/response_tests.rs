//! Unit tests for InteractionResponse, UsageMetadata, ContentSummary, InteractionStatus, and helpers.

use super::*;

// --- Response Deserialization ---

#[test]
fn test_deserialize_interaction_response_completed() {
    let response_json = r#"{
        "id": "interaction_123",
        "model": "gemini-3-flash-preview",
        "input": [{"type": "text", "text": "Hello"}],
        "outputs": [{"type": "text", "text": "Hi there!"}],
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
    assert_eq!(response.input.len(), 1);
    assert_eq!(response.outputs.len(), 1);
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
    assert_eq!(usage.total_reasoning_tokens, None);
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

// --- Response Helper Tests ---

#[test]
fn test_interaction_response_text() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Hello".to_string()),
                annotations: None,
            },
            Content::Text {
                text: Some("World".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert_eq!(response.as_text(), Some("Hello"));
    assert_eq!(response.all_text(), "HelloWorld");
    assert!(response.has_text());
    assert!(!response.has_function_calls());
}

#[test]
fn test_interaction_response_thoughts() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Thought {
                signature: Some("sig_1".to_string()),
            },
            Content::Thought {
                signature: Some("sig_2".to_string()),
            },
            Content::Text {
                text: Some("The answer is 42.".to_string()),
                annotations: None,
            },
            // Thought with None signature should be filtered out
            Content::Thought { signature: None },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_thoughts());

    let signatures: Vec<_> = response.thought_signatures().collect();
    assert_eq!(signatures.len(), 2);
    assert_eq!(signatures[0], "sig_1");
    assert_eq!(signatures[1], "sig_2");

    // Verify text() still works (only returns Text content)
    assert_eq!(response.as_text(), Some("The answer is 42."));
}

#[test]
fn test_interaction_response_no_thoughts() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Just text, no thoughts.".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_thoughts());
    let signatures: Vec<_> = response.thought_signatures().collect();
    assert!(signatures.is_empty());
}

#[test]
fn test_interaction_response_function_calls() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::FunctionCall {
                id: Some("call_001".to_string()),
                name: "get_weather".to_string(),
                args: serde_json::json!({"location": "Paris"}),
            },
            Content::FunctionCall {
                id: Some("call_002".to_string()),
                name: "get_time".to_string(),
                args: serde_json::json!({"timezone": "UTC"}),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let calls = response.function_calls();
    assert_eq!(calls.len(), 2);
    // FunctionCallInfo struct fields
    assert_eq!(calls[0].id, Some("call_001"));
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].args["location"], "Paris");
    assert_eq!(calls[1].id, Some("call_002"));
    assert_eq!(calls[1].name, "get_time");
    assert!(response.has_function_calls());
    assert!(!response.has_text());
}

#[test]
fn test_function_call_missing_id() {
    // Test that function calls with missing id are correctly captured as None.
    // This scenario should not normally occur (API contract requires call_id),
    // but if it does, the auto-function loop will return an error.
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::FunctionCall {
            id: None, // Missing call_id - should be captured correctly
            name: "get_weather".to_string(),
            args: serde_json::json!({"location": "Tokyo"}),
        }],
        status: InteractionStatus::RequiresAction,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);
    // Verify that missing id is correctly captured as None
    assert_eq!(calls[0].id, None);
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].args["location"], "Tokyo");

    // The auto-function loop in request_builder.rs will return an error
    // when it encounters a function call with None id, since call_id is
    // required to send function results back to the API.
}

#[test]
fn test_interaction_response_mixed_content() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Let me check".to_string()),
                annotations: None,
            },
            Content::FunctionCall {
                id: Some("call_mixed".to_string()),
                name: "check_status".to_string(),
                args: serde_json::json!({}),
            },
            Content::Text {
                text: Some("Done!".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert_eq!(response.as_text(), Some("Let me check"));
    assert_eq!(response.all_text(), "Let me checkDone!");
    assert_eq!(response.function_calls().len(), 1);
    assert!(response.has_text());
    assert!(response.has_function_calls());
}

#[test]
fn test_interaction_response_empty_outputs() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Here's the result:".to_string()),
                annotations: None,
            },
            Content::Unknown {
                content_type: "code_execution_result".to_string(),
                data: serde_json::json!({
                    "type": "code_execution_result",
                    "outcome": "success",
                    "output": "42"
                }),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_unknown());
    assert!(response.has_text());

    let unknowns = response.unknown_content();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].0, "code_execution_result");
    assert_eq!(unknowns[0].1["outcome"], "success");
}

#[test]
fn test_interaction_response_no_unknown() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Normal response".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_unknown());
    assert!(response.unknown_content().is_empty());
}

// --- ContentSummary Tests ---

#[test]
fn test_content_summary() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Text 1".to_string()),
                annotations: None,
            },
            Content::Text {
                text: Some("Text 2".to_string()),
                annotations: None,
            },
            Content::Thought {
                signature: Some("sig_thinking".to_string()),
            },
            Content::FunctionCall {
                id: Some("call_1".to_string()),
                name: "test_fn".to_string(),
                args: serde_json::json!({}),
            },
            Content::Unknown {
                content_type: "type_a".to_string(),
                data: serde_json::json!({"type": "type_a"}),
            },
            Content::Unknown {
                content_type: "type_b".to_string(),
                data: serde_json::json!({"type": "type_b"}),
            },
            Content::Unknown {
                content_type: "type_a".to_string(), // Duplicate type
                data: serde_json::json!({"type": "type_a", "extra": true}),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let summary = response.content_summary();

    assert_eq!(summary.text_count, 2);
    assert_eq!(summary.thought_count, 1);
    assert_eq!(summary.function_call_count, 1);
    assert_eq!(summary.unknown_count, 3);
    // Unknown types should be deduplicated and sorted
    assert_eq!(summary.unknown_types.len(), 2);
    assert_eq!(summary.unknown_types, vec!["type_a", "type_b"]);
}

#[test]
fn test_content_summary_empty() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let summary = response.content_summary();

    assert_eq!(summary.text_count, 0);
    assert_eq!(summary.unknown_count, 0);
    assert!(summary.unknown_types.is_empty());
}

#[test]
fn test_content_summary_display() {
    // Test Display for ContentSummary with various counts
    let summary = ContentSummary {
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
fn test_content_summary_display_empty() {
    let summary = ContentSummary::default();
    assert_eq!(format!("{}", summary), "empty");
}

#[test]
fn test_content_summary_display_with_unknown() {
    let summary = ContentSummary {
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
fn test_content_summary_with_built_in_tools() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::CodeExecutionCall {
                id: Some("call_1".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print(1)".to_string(),
            },
            Content::CodeExecutionCall {
                id: Some("call_2".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print(2)".to_string(),
            },
            Content::CodeExecutionResult {
                call_id: Some("call_1".to_string()),
                is_error: false,
                result: "1\n2\n".to_string(),
            },
            Content::GoogleSearchCall {
                id: "search1".to_string(),
                queries: vec!["test".to_string()],
            },
            Content::GoogleSearchResult {
                call_id: "search1".to_string(),
                result: vec![],
            },
            Content::UrlContextCall {
                id: "ctx_123".to_string(),
                urls: vec!["https://example.com".to_string()],
            },
            Content::UrlContextResult {
                call_id: "ctx_123".to_string(),
                result: vec![],
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let summary = response.content_summary();

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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Here's the code:".to_string()),
                annotations: None,
            },
            Content::CodeExecutionCall {
                id: Some("call_123".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print(42)".to_string(),
            },
            Content::CodeExecutionResult {
                call_id: Some("call_123".to_string()),
                is_error: false,
                result: "42\n".to_string(),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_code_execution_calls());
    assert!(response.has_code_execution_results());
    assert!(!response.has_unknown());

    // Test code_execution_calls helper
    let code_blocks = response.code_execution_calls();
    assert_eq!(code_blocks.len(), 1);
    assert_eq!(code_blocks[0].id, Some("call_123"));
    assert_eq!(code_blocks[0].language, CodeExecutionLanguage::Python);
    assert_eq!(code_blocks[0].code, "print(42)");

    // Test code_execution_results helper
    let results = response.code_execution_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].call_id, Some("call_123"));
    assert!(!results[0].is_error);
    assert_eq!(results[0].result, "42\n");

    // Test successful_code_output helper
    assert_eq!(response.successful_code_output(), Some("42\n"));
}

#[test]
fn test_interaction_response_google_search_helpers() {
    use super::content::GoogleSearchResultItem;

    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::GoogleSearchResult {
                call_id: "call123".to_string(),
                result: vec![GoogleSearchResultItem::new("Test", "https://example.com")],
            },
            Content::Text {
                text: Some("Based on search results...".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![Content::UrlContextResult {
            call_id: "ctx_123".to_string(),
            result: vec![UrlContextResultItem::new("https://example.com", "success")],
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
    use super::content::{GoogleMapsResultItem, Place};

    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::GoogleMapsResult {
                call_id: "maps_123".to_string(),
                signature: None,
                result: vec![GoogleMapsResultItem {
                    places: Some(vec![Place {
                        name: Some("Eiffel Tower".to_string()),
                        formatted_address: Some("Paris, France".to_string()),
                        place_id: Some("ChIJLU7jZClu5kcR".to_string()),
                        lat: Some(48.8584),
                        lng: Some(2.2945),
                        types: None,
                        rating: None,
                        user_ratings_total: None,
                        website: None,
                        phone_number: None,
                        extra: serde_json::Map::new(),
                    }]),
                    widget_context_token: Some("token123".to_string()),
                }],
            },
            Content::Text {
                text: Some("Here are the maps results.".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("No maps here.".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_google_maps_results());
    assert!(response.google_maps_results().is_empty());
}

// --- URL Context Metadata Tests ---

#[test]
fn test_deserialize_url_context_metadata() {
    // Test full deserialization with all statuses
    let json = r#"{
        "urlMetadata": [
            {
                "retrievedUrl": "https://example.com",
                "urlRetrievalStatus": "URL_RETRIEVAL_STATUS_SUCCESS"
            },
            {
                "retrievedUrl": "https://blocked.com",
                "urlRetrievalStatus": "URL_RETRIEVAL_STATUS_UNSAFE"
            },
            {
                "retrievedUrl": "https://failed.com",
                "urlRetrievalStatus": "URL_RETRIEVAL_STATUS_ERROR"
            }
        ]
    }"#;

    let metadata: UrlContextMetadata = serde_json::from_str(json).expect("Failed to deserialize");

    assert_eq!(metadata.url_metadata.len(), 3);

    assert_eq!(
        metadata.url_metadata[0].retrieved_url,
        "https://example.com"
    );
    assert_eq!(
        metadata.url_metadata[0].url_retrieval_status,
        UrlRetrievalStatus::Success
    );

    assert_eq!(
        metadata.url_metadata[1].retrieved_url,
        "https://blocked.com"
    );
    assert_eq!(
        metadata.url_metadata[1].url_retrieval_status,
        UrlRetrievalStatus::Unsafe
    );

    assert_eq!(metadata.url_metadata[2].retrieved_url, "https://failed.com");
    assert_eq!(
        metadata.url_metadata[2].url_retrieval_status,
        UrlRetrievalStatus::Error
    );
}

#[test]
fn test_deserialize_url_context_metadata_empty() {
    // Test empty url_metadata array
    let json = r#"{"urlMetadata": []}"#;
    let metadata: UrlContextMetadata = serde_json::from_str(json).expect("Failed to deserialize");
    assert!(metadata.url_metadata.is_empty());
}

#[test]
fn test_deserialize_url_context_metadata_missing_field() {
    // Test missing urlMetadata field (should default to empty vec)
    let json = r#"{}"#;
    let metadata: UrlContextMetadata = serde_json::from_str(json).expect("Failed to deserialize");
    assert!(metadata.url_metadata.is_empty());
}

#[test]
fn test_url_retrieval_status_serialization_roundtrip() {
    // Test all enum variants roundtrip correctly
    let statuses = vec![
        UrlRetrievalStatus::Unspecified,
        UrlRetrievalStatus::Success,
        UrlRetrievalStatus::Unsafe,
        UrlRetrievalStatus::Error,
    ];

    for status in statuses {
        let serialized = serde_json::to_string(&status).expect("Failed to serialize");
        let deserialized: UrlRetrievalStatus =
            serde_json::from_str(&serialized).expect("Failed to deserialize");
        assert_eq!(status, deserialized);
    }
}

#[test]
fn test_url_retrieval_status_known_variants_wire_format() {
    // Verify exact wire format for all known variants
    let variants = [
        (
            UrlRetrievalStatus::Unspecified,
            "URL_RETRIEVAL_STATUS_UNSPECIFIED",
        ),
        (UrlRetrievalStatus::Success, "URL_RETRIEVAL_STATUS_SUCCESS"),
        (UrlRetrievalStatus::Unsafe, "URL_RETRIEVAL_STATUS_UNSAFE"),
        (UrlRetrievalStatus::Error, "URL_RETRIEVAL_STATUS_ERROR"),
    ];

    for (status, expected_wire) in variants {
        let serialized = serde_json::to_string(&status).expect("Should serialize");
        assert_eq!(serialized, format!(r#""{}""#, expected_wire));

        let deserialized: UrlRetrievalStatus =
            serde_json::from_str(&serialized).expect("Should deserialize");
        assert_eq!(deserialized, status);
        assert!(!deserialized.is_unknown());
    }
}

#[test]
fn test_url_retrieval_status_helper_methods() {
    assert!(UrlRetrievalStatus::Success.is_success());
    assert!(!UrlRetrievalStatus::Success.is_error());

    assert!(!UrlRetrievalStatus::Error.is_success());
    assert!(UrlRetrievalStatus::Error.is_error());

    assert!(!UrlRetrievalStatus::Unsafe.is_success());
    assert!(UrlRetrievalStatus::Unsafe.is_error());

    assert!(!UrlRetrievalStatus::Unspecified.is_success());
    assert!(!UrlRetrievalStatus::Unspecified.is_error());
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_url_retrieval_status_unknown_deserialization() {
    // Simulate a new status the library doesn't know about
    let unknown_json = r#""URL_RETRIEVAL_STATUS_PENDING""#;
    let status: UrlRetrievalStatus =
        serde_json::from_str(unknown_json).expect("Should deserialize as Unknown");

    assert!(status.is_unknown());
    assert!(!status.is_success());
    assert!(!status.is_error()); // Unknown status is neither success nor error

    // Verify helper methods
    assert_eq!(
        status.unknown_status_type(),
        Some("URL_RETRIEVAL_STATUS_PENDING")
    );
    assert!(status.unknown_data().is_some());

    // Verify roundtrip serialization preserves the value
    let reserialized = serde_json::to_string(&status).expect("Should serialize");
    assert_eq!(reserialized, r#""URL_RETRIEVAL_STATUS_PENDING""#);
}

#[cfg(not(feature = "strict-unknown"))]
#[test]
fn test_url_retrieval_status_unknown_display() {
    let unknown = UrlRetrievalStatus::Unknown {
        status_type: "URL_RETRIEVAL_STATUS_FUTURE".to_string(),
        data: serde_json::Value::String("URL_RETRIEVAL_STATUS_FUTURE".to_string()),
    };
    assert_eq!(format!("{}", unknown), "URL_RETRIEVAL_STATUS_FUTURE");
}

// --- Function Result Helpers ---

#[test]
fn test_interaction_response_function_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::FunctionResult {
                name: Some("get_weather".to_string()),
                call_id: "call_001".to_string(),
                result: serde_json::json!({"temp": 72, "unit": "F"}),
                is_error: None,
            },
            Content::FunctionResult {
                name: Some("get_time".to_string()),
                call_id: "call_002".to_string(),
                result: serde_json::json!({"time": "14:30", "zone": "UTC"}),
                is_error: None,
            },
            Content::Text {
                text: Some("Here are the results".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_function_results());
    assert!(response.has_text());

    let results = response.function_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, Some("get_weather"));
    assert_eq!(results[0].call_id, "call_001");
    assert_eq!(results[0].result["temp"], 72);
    assert_eq!(results[1].name, Some("get_time"));
    assert_eq!(results[1].call_id, "call_002");
}

#[test]
fn test_interaction_response_no_function_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Just text".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::GoogleSearchCall {
                id: "search1".to_string(),
                queries: vec!["Rust programming language".to_string()],
            },
            Content::GoogleSearchCall {
                id: "search2".to_string(),
                queries: vec!["async await Rust".to_string()],
            },
            Content::Text {
                text: Some("Search results...".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("No search".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::UrlContextCall {
                id: "ctx_1".to_string(),
                urls: vec!["https://docs.rs".to_string()],
            },
            Content::UrlContextCall {
                id: "ctx_2".to_string(),
                urls: vec![
                    "https://rust-lang.org".to_string(),
                    "https://crates.io".to_string(),
                ],
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::CodeExecutionCall {
                id: Some("call_first".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print('first')".to_string(),
            },
            Content::CodeExecutionCall {
                id: Some("call_second".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print('second')".to_string(),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    // Test code_execution_call() - returns first one
    let call = response.code_execution_call();
    assert!(call.is_some());
    let call = call.unwrap();
    assert_eq!(call.id, Some("call_first"));
    assert_eq!(call.language, CodeExecutionLanguage::Python);
    assert_eq!(call.code, "print('first')");
}

#[test]
fn test_interaction_response_no_code_execution_call() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("No code".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.code_execution_call().is_none());
}

// --- Metadata Helpers ---

#[test]
fn test_interaction_response_google_search_metadata_helpers() {
    use super::GroundingMetadata;

    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Response grounded with search".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: Some(GroundingMetadata {
            web_search_queries: vec!["Rust language".to_string()],
            grounding_chunks: vec![],
        }),
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_google_search_metadata());

    let metadata = response.google_search_metadata();
    assert!(metadata.is_some());
    let metadata = metadata.unwrap();
    assert_eq!(metadata.web_search_queries, vec!["Rust language"]);
}

#[test]
fn test_interaction_response_no_google_search_metadata() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_google_search_metadata());
    assert!(response.google_search_metadata().is_none());
}

#[test]
fn test_interaction_response_url_context_metadata_helpers() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: Some(UrlContextMetadata {
            url_metadata: vec![UrlMetadataEntry {
                retrieved_url: "https://example.com".to_string(),
                url_retrieval_status: UrlRetrievalStatus::Success,
            }],
        }),
        created: None,
        updated: None,
    };

    assert!(response.has_url_context_metadata());

    let metadata = response.url_context_metadata();
    assert!(metadata.is_some());
    let metadata = metadata.unwrap();
    assert_eq!(metadata.url_metadata.len(), 1);
    assert_eq!(
        metadata.url_metadata[0].retrieved_url,
        "https://example.com"
    );
    assert_eq!(
        metadata.url_metadata[0].url_retrieval_status,
        UrlRetrievalStatus::Success
    );
}

#[test]
fn test_interaction_response_no_url_context_metadata() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_url_context_metadata());
    assert!(response.url_context_metadata().is_none());
}

#[test]
fn test_interaction_response_code_execution_calls_plural() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::CodeExecutionCall {
                id: Some("call_1".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print('first')".to_string(),
            },
            Content::CodeExecutionCall {
                id: Some("call_2".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print('second')".to_string(),
            },
            Content::Text {
                text: Some("Results".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_code_execution_calls());

    let calls = response.code_execution_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, Some("call_1"));
    assert_eq!(calls[0].language, CodeExecutionLanguage::Python);
    assert_eq!(calls[0].code, "print('first')");
    assert_eq!(calls[1].id, Some("call_2"));
    assert_eq!(calls[1].language, CodeExecutionLanguage::Python);
    assert_eq!(calls[1].code, "print('second')");
}

#[test]
fn test_interaction_response_code_execution_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::CodeExecutionResult {
                call_id: Some("call_1".to_string()),
                is_error: false,
                result: "first output".to_string(),
            },
            Content::CodeExecutionResult {
                call_id: Some("call_2".to_string()),
                is_error: true,
                result: "error message".to_string(),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_code_execution_results());

    let results = response.code_execution_results();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].call_id, Some("call_1"));
    assert!(!results[0].is_error);
    assert_eq!(results[0].result, "first output");
    assert_eq!(results[1].call_id, Some("call_2"));
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
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("No code".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::GoogleSearchResult {
                call_id: "search1".to_string(),
                result: vec![GoogleSearchResultItem {
                    title: "Rust Lang".to_string(),
                    url: "https://rust-lang.org".to_string(),
                    rendered_content: None,
                }],
            },
            Content::GoogleSearchResult {
                call_id: "search2".to_string(),
                result: vec![GoogleSearchResultItem {
                    title: "Cargo".to_string(),
                    url: "https://doc.rust-lang.org/cargo/".to_string(),
                    rendered_content: None,
                }],
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_google_search_results());
    assert!(response.google_search_results().is_empty());
}

#[test]
fn test_interaction_response_url_context_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::UrlContextResult {
                call_id: "ctx_1".to_string(),
                result: vec![
                    UrlContextResultItem::new("https://docs.rs", "success"),
                    UrlContextResultItem::new("https://crates.io", "success"),
                ],
            },
            Content::UrlContextResult {
                call_id: "ctx_2".to_string(),
                result: vec![UrlContextResultItem::new("https://blocked.com", "error")],
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_url_context_results());
    assert!(response.url_context_results().is_empty());
}

/// Comprehensive roundtrip test for InteractionResponse with all content types.
///
/// This test verifies that complex responses with multiple content types,
/// function calls, thoughts, and metadata can be serialized and deserialized
/// without data loss. This is critical for save/resume semantics.
#[test]
fn test_interaction_response_complex_roundtrip() {
    // Build a response with many different content types
    let response = InteractionResponse {
        id: Some("complex-interaction-xyz".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![
            Content::Text {
                text: Some("Analyze this and call the weather function".to_string()),
                annotations: None,
            },
            Content::Image {
                mime_type: Some("image/png".to_string()),
                data: Some("base64encodeddata".to_string()),
                uri: None,
                resolution: None,
            },
        ],
        outputs: vec![
            // Thought with signature (thinking models)
            Content::Thought {
                signature: Some("sig_thinking".to_string()),
            },
            Content::ThoughtSignature {
                signature: "thought-sig-abc123".to_string(),
            },
            // Function call
            Content::FunctionCall {
                id: Some("call-func-001".to_string()),
                name: "get_weather".to_string(),
                args: serde_json::json!({"city": "Tokyo", "units": "celsius"}),
            },
            // Function result
            Content::FunctionResult {
                call_id: "call-func-001".to_string(),
                name: Some("get_weather".to_string()),
                result: serde_json::json!({"temp": 22, "conditions": "sunny"}),
                is_error: None,
            },
            // Code execution
            Content::CodeExecutionCall {
                id: Some("code-exec-001".to_string()),
                language: CodeExecutionLanguage::Python,
                code: "print(2 + 2)".to_string(),
            },
            Content::CodeExecutionResult {
                call_id: Some("code-exec-001".to_string()),
                is_error: false,
                result: "4".to_string(),
            },
            // Google search
            Content::GoogleSearchCall {
                id: "gsearch-001".to_string(),
                queries: vec!["weather in Tokyo".to_string()],
            },
            Content::GoogleSearchResult {
                call_id: "gsearch-001".to_string(),
                result: vec![GoogleSearchResultItem {
                    title: "Tokyo Weather".to_string(),
                    url: "https://weather.example.com/tokyo".to_string(),
                    rendered_content: None,
                }],
            },
            // URL context
            Content::UrlContextCall {
                id: "ctx_123".to_string(),
                urls: vec!["https://example.com".to_string()],
            },
            Content::UrlContextResult {
                call_id: "ctx_123".to_string(),
                result: vec![UrlContextResultItem::new("https://example.com", "success")],
            },
            // Final text response
            Content::Text {
                text: Some("The weather in Tokyo is 22°C and sunny.".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: Some(UsageMetadata {
            total_input_tokens: Some(150),
            total_output_tokens: Some(200),
            total_tokens: Some(350),
            total_cached_tokens: Some(50),
            total_reasoning_tokens: Some(30),
            total_tool_use_tokens: Some(20),
            ..Default::default()
        }),
        tools: Some(vec![
            crate::Tool::GoogleSearch { search_types: None },
            crate::Tool::CodeExecution,
        ]),
        previous_interaction_id: Some("previous-interaction-abc".to_string()),
        grounding_metadata: Some(GroundingMetadata {
            grounding_chunks: vec![GroundingChunk {
                web: WebSource {
                    title: "Weather Report".to_string(),
                    uri: "https://weather.example.com".to_string(),
                    domain: "weather.example.com".to_string(),
                },
            }],
            web_search_queries: vec!["tokyo weather".to_string()],
        }),
        url_context_metadata: Some(UrlContextMetadata {
            url_metadata: vec![UrlMetadataEntry {
                retrieved_url: "https://example.com".to_string(),
                url_retrieval_status: UrlRetrievalStatus::Success,
            }],
        }),
        created: None,
        updated: None,
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
        "Should contain grounding URI"
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

    // Verify input
    assert_eq!(deserialized.input.len(), 2);

    // Verify outputs have correct count
    assert_eq!(deserialized.outputs.len(), 11);

    // Verify function calls are accessible
    let function_calls = deserialized.function_calls();
    assert_eq!(function_calls.len(), 1);
    assert_eq!(function_calls[0].name, "get_weather");
    assert_eq!(function_calls[0].id, Some("call-func-001"));
    assert_eq!(function_calls[0].args["city"], "Tokyo");

    // Verify code execution results
    let code_results = deserialized.code_execution_results();
    assert_eq!(code_results.len(), 1);
    assert!(!code_results[0].is_error);
    assert_eq!(code_results[0].result, "4");

    // Verify URL context results
    let url_results = deserialized.url_context_results();
    assert_eq!(url_results.len(), 1);
    assert_eq!(url_results[0].call_id, "ctx_123");
    assert_eq!(url_results[0].items.len(), 1);
    assert!(url_results[0].items[0].is_success());

    // Verify usage metadata
    let usage = deserialized.usage.expect("Should have usage");
    assert_eq!(usage.total_input_tokens, Some(150));
    assert_eq!(usage.total_output_tokens, Some(200));
    assert_eq!(usage.total_tokens, Some(350));
    assert_eq!(usage.total_cached_tokens, Some(50));
    assert_eq!(usage.total_reasoning_tokens, Some(30));
    assert_eq!(usage.total_tool_use_tokens, Some(20));

    // Verify grounding metadata
    let grounding = deserialized
        .grounding_metadata
        .expect("Should have grounding");
    assert_eq!(grounding.grounding_chunks.len(), 1);
    assert_eq!(grounding.grounding_chunks[0].web.title, "Weather Report");
    assert_eq!(
        grounding.grounding_chunks[0].web.uri,
        "https://weather.example.com"
    );
    assert_eq!(
        grounding.web_search_queries,
        vec!["tokyo weather".to_string()]
    );

    // Verify URL context metadata
    let url_meta = deserialized
        .url_context_metadata
        .expect("Should have URL metadata");
    assert_eq!(url_meta.url_metadata.len(), 1);
    assert_eq!(
        url_meta.url_metadata[0].retrieved_url,
        "https://example.com"
    );
    assert_eq!(
        url_meta.url_metadata[0].url_retrieval_status,
        UrlRetrievalStatus::Success
    );

    // Verify tools
    let tools = deserialized.tools.expect("Should have tools");
    assert_eq!(tools.len(), 2);
    assert!(matches!(tools[0], crate::Tool::GoogleSearch { .. }));
    assert!(matches!(tools[1], crate::Tool::CodeExecution));
}

// --- InteractionStatus Unknown Variant Tests ---

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
        "input": [{"type": "text", "text": "Hello"}],
        "outputs": [{"type": "text", "text": "Hi there!"}],
        "status": "completed"
    }"#;

    let response: InteractionResponse =
        serde_json::from_str(json).expect("Deserialization should succeed without id");

    assert!(response.id.is_none(), "ID should be None when not present");
    assert_eq!(response.model, Some("gemini-3-flash-preview".to_string()));
    assert_eq!(response.status, InteractionStatus::Completed);
    assert!(!response.outputs.is_empty());
}

#[test]
fn test_interaction_response_deserialize_with_id() {
    // When store=true (or by default), the API response includes an id field.
    // This test verifies that we can still deserialize such responses correctly.
    let json = r#"{
        "id": "interaction-abc123",
        "model": "gemini-3-flash-preview",
        "input": [{"type": "text", "text": "Hello"}],
        "outputs": [{"type": "text", "text": "Hi there!"}],
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
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Hello".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        grounding_metadata: None,
        url_context_metadata: None,
        previous_interaction_id: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Test response".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        grounding_metadata: None,
        url_context_metadata: None,
        previous_interaction_id: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![Content::FunctionCall {
            id: Some("call_123".to_string()),
            name: "get_weather".to_string(),
            args: serde_json::json!({"city": "Tokyo", "units": "celsius"}),
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let calls = response.function_calls();
    assert_eq!(calls.len(), 1);

    // Convert to owned
    let owned = calls[0].to_owned();

    // Verify all fields are correctly converted
    assert_eq!(owned.id, Some("call_123".to_string()));
    assert_eq!(owned.name, "get_weather");
    assert_eq!(owned.args["city"], "Tokyo");
    assert_eq!(owned.args["units"], "celsius");
}

#[test]
fn test_function_call_info_to_owned_none_fields() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::FunctionCall {
            id: None,
            name: "simple_function".to_string(),
            args: serde_json::json!({}),
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let calls = response.function_calls();
    let owned = calls[0].to_owned();

    assert_eq!(owned.id, None);
    assert_eq!(owned.name, "simple_function");
    assert_eq!(owned.args, serde_json::json!({}));
}

#[test]
fn test_owned_function_call_info_outlives_response() {
    // Demonstrate the main use case: owned call can outlive the response
    let owned_calls: Vec<OwnedFunctionCallInfo> = {
        let response = InteractionResponse {
            id: Some("test_id".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            agent: None,
            input: vec![],
            outputs: vec![
                Content::FunctionCall {
                    id: Some("call_1".to_string()),
                    name: "func_a".to_string(),
                    args: serde_json::json!({"x": 1}),
                },
                Content::FunctionCall {
                    id: Some("call_2".to_string()),
                    name: "func_b".to_string(),
                    args: serde_json::json!({"y": 2}),
                },
            ],
            status: InteractionStatus::RequiresAction,
            usage: None,
            tools: None,
            previous_interaction_id: None,
            grounding_metadata: None,
            url_context_metadata: None,
            created: None,
            updated: None,
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
    assert_eq!(owned_calls[0].name, "func_a");
    assert_eq!(owned_calls[0].args["x"], 1);
    assert_eq!(owned_calls[1].name, "func_b");
    assert_eq!(owned_calls[1].args["y"], 2);
}

#[test]
fn test_owned_function_call_info_serialization_roundtrip() {
    let owned = OwnedFunctionCallInfo {
        id: Some("call_xyz".to_string()),
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
        id: Some("call_id".to_string()),
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
        id: Some("same_id".to_string()),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    let owned2 = OwnedFunctionCallInfo {
        id: Some("same_id".to_string()),
        name: "same_name".to_string(),
        args: serde_json::json!({"same": true}),
    };

    let different = OwnedFunctionCallInfo {
        id: Some("different_id".to_string()),
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
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("According to the source, climate change is accelerating.".to_string()),
            annotations: Some(vec![Annotation {
                start_index: 19,
                end_index: 25,
                source: Some("https://climate.gov".to_string()),
            }]),
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_annotations());
}

#[test]
fn test_interaction_response_no_annotations() {
    // Response without annotations
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Plain text without citations.".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_annotations());
}

#[test]
fn test_interaction_response_empty_annotations_not_counted() {
    // Response with empty annotations array (should not count as having annotations)
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Text with empty annotations.".to_string()),
            annotations: Some(vec![]), // Empty array
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_annotations());
}

#[test]
fn test_interaction_response_all_annotations() {
    // Response with multiple text outputs, each with annotations
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("First claim from source A.".to_string()),
                annotations: Some(vec![Annotation {
                    start_index: 0,
                    end_index: 11,
                    source: Some("https://source-a.com".to_string()),
                }]),
            },
            Content::Thought {
                signature: Some("sig_thinking".to_string()),
            },
            Content::Text {
                text: Some("Second and third claims.".to_string()),
                annotations: Some(vec![
                    Annotation {
                        start_index: 0,
                        end_index: 6,
                        source: Some("https://source-b.com".to_string()),
                    },
                    Annotation {
                        start_index: 11,
                        end_index: 16,
                        source: Some("https://source-c.com".to_string()),
                    },
                ]),
            },
            Content::Text {
                text: Some("Text without annotations.".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let annotations: Vec<_> = response.all_annotations().collect();

    // Should collect all 3 annotations from the two Text outputs with annotations
    assert_eq!(annotations.len(), 3);
    assert_eq!(
        annotations[0].source.as_deref(),
        Some("https://source-a.com")
    );
    assert_eq!(
        annotations[1].source.as_deref(),
        Some("https://source-b.com")
    );
    assert_eq!(
        annotations[2].source.as_deref(),
        Some("https://source-c.com")
    );
}

#[test]
fn test_interaction_response_all_annotations_empty() {
    // Response with no annotations at all
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("No annotations here.".to_string()),
                annotations: None,
            },
            Content::FunctionCall {
                id: Some("call_1".to_string()),
                name: "test".to_string(),
                args: serde_json::json!({}),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Image {
                data: Some("base64".to_string()),
                uri: None,
                mime_type: Some("image/png".to_string()),
                resolution: None,
            },
            Content::CodeExecutionResult {
                call_id: Some("call_1".to_string()),
                is_error: false,
                result: "result".to_string(),
            },
            Content::Text {
                text: Some("Only text has annotations.".to_string()),
                annotations: Some(vec![Annotation {
                    start_index: 0,
                    end_index: 4,
                    source: Some("https://example.com".to_string()),
                }]),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let annotations: Vec<_> = response.all_annotations().collect();

    // Should only find the one annotation from the Text content
    assert_eq!(annotations.len(), 1);
    assert_eq!(
        annotations[0].source.as_deref(),
        Some("https://example.com")
    );
}

// --- File Search Result Helper Tests ---

use super::content::FileSearchResultItem;

#[test]
fn test_interaction_response_has_file_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::FileSearchResult {
            call_id: "call_123".to_string(),
            result: vec![FileSearchResultItem {
                title: "Technical Doc".to_string(),
                text: "Relevant content...".to_string(),
                store: "stores/my-store".to_string(),
            }],
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_file_search_results());
}

#[test]
fn test_interaction_response_no_file_search_results() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("No file search here".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_file_search_results());
}

#[test]
fn test_interaction_response_file_search_results_extraction() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::FileSearchResult {
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
            },
            Content::Text {
                text: Some("Summary of search results".to_string()),
                annotations: None,
            },
            Content::FileSearchResult {
                call_id: "call_2".to_string(),
                result: vec![FileSearchResultItem {
                    title: "Doc 3".to_string(),
                    text: "Content from doc 3".to_string(),
                    store: "stores/store-b".to_string(),
                }],
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_file_search_results());

    let results = response.file_search_results();
    // Should collect all items from both FileSearchResult outputs
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
        agent: None,
        input: vec![],
        outputs: vec![Content::FileSearchResult {
            call_id: "call_empty".to_string(),
            result: vec![], // Empty results array
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    // has_file_search_results returns true if the content type exists
    assert!(response.has_file_search_results());

    // But file_search_results returns empty vec since result array is empty
    let results = response.file_search_results();
    assert!(results.is_empty());
}

// =============================================================================
// ImageInfo and images() Tests
// =============================================================================

#[test]
fn test_image_info_bytes_decodes_valid_base64() {
    use base64::Engine as _;
    // Valid PNG header in base64
    let png_bytes = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let base64_data = base64::engine::general_purpose::STANDARD.encode(png_bytes);

    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some(base64_data),
            uri: None,
            mime_type: Some("image/png".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images.len(), 1);

    let decoded = images[0].bytes().expect("Should decode valid base64");
    assert_eq!(decoded, png_bytes);
}

#[test]
fn test_image_info_bytes_invalid_base64() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("not valid base64!!!".to_string()),
            uri: None,
            mime_type: Some("image/png".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images.len(), 1);

    let result = images[0].bytes();
    assert!(result.is_err());
}

#[test]
fn test_image_info_mime_type() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Image {
                data: Some("YWJj".to_string()), // "abc" in base64
                uri: None,
                mime_type: Some("image/jpeg".to_string()),
                resolution: None,
            },
            Content::Image {
                data: Some("ZGVm".to_string()), // "def" in base64
                uri: None,
                mime_type: None,
                resolution: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images.len(), 2);

    assert_eq!(images[0].mime_type(), Some("image/jpeg"));
    assert_eq!(images[1].mime_type(), None);
}

#[test]
fn test_image_info_extension_jpeg() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: Some("image/jpeg".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "jpg");
}

#[test]
fn test_image_info_extension_jpg_alternate() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: Some("image/jpg".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "jpg");
}

#[test]
fn test_image_info_extension_png() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: Some("image/png".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "png");
}

#[test]
fn test_image_info_extension_webp() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: Some("image/webp".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "webp");
}

#[test]
fn test_image_info_extension_gif() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: Some("image/gif".to_string()),
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "gif");
}

#[test]
fn test_image_info_extension_unknown_defaults_to_png() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: Some("image/tiff".to_string()), // Unknown type
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "png"); // Defaults to png for unknown types
}

#[test]
fn test_image_info_extension_none_mime_type_defaults_to_png() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Image {
            data: Some("YWJj".to_string()),
            uri: None,
            mime_type: None,
            resolution: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert_eq!(images[0].extension(), "png"); // Defaults to png when no MIME type
}

#[test]
fn test_images_iterator_returns_only_images_with_data() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Text content".to_string()),
                annotations: None,
            },
            Content::Image {
                data: Some("YWJj".to_string()), // Has data - should be included
                uri: None,
                mime_type: Some("image/png".to_string()),
                resolution: None,
            },
            Content::Image {
                data: None, // No data - URI-based image, should be excluded
                uri: Some("https://example.com/image.png".to_string()),
                mime_type: Some("image/png".to_string()),
                resolution: None,
            },
            Content::Image {
                data: Some("ZGVm".to_string()), // Has data - should be included
                uri: None,
                mime_type: Some("image/jpeg".to_string()),
                resolution: None,
            },
            Content::FunctionCall {
                id: None,
                name: "test".to_string(),
                args: serde_json::json!({}),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Just text".to_string()),
                annotations: None,
            },
            Content::Thought {
                signature: Some("sig_thought".to_string()),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let images: Vec<_> = response.images().collect();
    assert!(images.is_empty());
}

#[test]
fn test_images_iterator_empty_outputs() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Text content".to_string()),
                annotations: None,
            },
            Content::Audio {
                data: Some("YWJj".to_string()), // Has data - should be included
                uri: None,
                mime_type: Some("audio/L16;codec=pcm;rate=24000".to_string()),
            },
            Content::Audio {
                data: None, // No data - URI-based audio, should be excluded
                uri: Some("https://example.com/audio.mp3".to_string()),
                mime_type: Some("audio/mp3".to_string()),
            },
            Content::Audio {
                data: Some("ZGVm".to_string()), // Has data - should be included
                uri: None,
                mime_type: Some("audio/wav".to_string()),
            },
            Content::FunctionCall {
                id: None,
                name: "test".to_string(),
                args: serde_json::json!({}),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
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
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Just text".to_string()),
                annotations: None,
            },
            Content::Thought {
                signature: Some("sig_thought".to_string()),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let audios: Vec<_> = response.audios().collect();
    assert!(audios.is_empty());
}

#[test]
fn test_audios_iterator_empty_outputs() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-2.5-pro-preview-tts".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let audios: Vec<_> = response.audios().collect();
    assert!(audios.is_empty());
}

#[test]
fn test_first_audio() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-2.5-pro-preview-tts".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("Hello".to_string()),
                annotations: None,
            },
            Content::Audio {
                data: Some("Zmlyc3Q=".to_string()),
                uri: None,
                mime_type: Some("audio/L16;codec=pcm;rate=24000".to_string()),
            },
            Content::Audio {
                data: Some("c2Vjb25k".to_string()),
                uri: None,
                mime_type: Some("audio/wav".to_string()),
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let first = response.first_audio();
    assert!(first.is_some());
    let audio = first.unwrap();
    assert_eq!(audio.mime_type(), Some("audio/L16;codec=pcm;rate=24000"));
    assert_eq!(audio.extension(), "pcm");
}

#[test]
fn test_first_audio_none_when_empty() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("No audio here".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.first_audio().is_none());
}

#[test]
fn test_has_audio_true() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-2.5-pro-preview-tts".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Audio {
            data: Some("YXVkaW8=".to_string()),
            uri: None,
            mime_type: Some("audio/L16;codec=pcm;rate=24000".to_string()),
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(response.has_audio());
}

#[test]
fn test_has_audio_false_when_no_audio() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Text only".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_audio());
}

#[test]
fn test_has_audio_false_when_uri_only() {
    // URI-based audio (no data) should not count as "having audio"
    // since the audios() iterator only includes data-based audio
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-2.5-pro-preview-tts".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Audio {
            data: None,
            uri: Some("https://example.com/audio.mp3".to_string()),
            mime_type: Some("audio/mp3".to_string()),
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    assert!(!response.has_audio());
}

// =============================================================================
// as_model_turn() Tests
// =============================================================================

#[test]
fn test_as_model_turn_creates_model_turn() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![Content::Text {
            text: Some("Response text".to_string()),
            annotations: None,
        }],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let turn = response.as_model_turn();
    assert!(turn.is_model());
    assert!(!turn.is_user());
}

#[test]
fn test_as_model_turn_includes_all_outputs() {
    let response = InteractionResponse {
        id: Some("test_id".to_string()),
        model: Some("gemini-3-flash-preview".to_string()),
        agent: None,
        input: vec![],
        outputs: vec![
            Content::Text {
                text: Some("First part".to_string()),
                annotations: None,
            },
            Content::Text {
                text: Some("Second part".to_string()),
                annotations: None,
            },
        ],
        status: InteractionStatus::Completed,
        usage: None,
        tools: None,
        previous_interaction_id: None,
        grounding_metadata: None,
        url_context_metadata: None,
        created: None,
        updated: None,
    };

    let turn = response.as_model_turn();
    // The turn should contain all outputs
    if let Some(parts) = turn.content().as_parts() {
        assert_eq!(parts.len(), 2);
    } else {
        panic!("Expected Parts variant");
    }
}
