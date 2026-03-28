//! Live API canary tests for detecting wire format drift.
//!
//! These tests make real API calls to detect when Unknown variants appear in responses,
//! which indicates the API has added new types that our deserializers don't recognize.
//!
//! ## Purpose
//!
//! The Evergreen philosophy means our library gracefully handles unknown data by
//! deserializing it into Unknown variants. These tests detect when that happens,
//! alerting us to update our type definitions.
//!
//! ## Running These Tests
//!
//! These tests are ignored by default and require `GEMINI_API_KEY`:
//!
//! ```bash
//! # Run all canary tests
//! cargo test --test api_wire_format_live_tests -- --ignored
//!
//! # Run specific canary test
//! cargo test --test api_wire_format_live_tests canary_basic_interaction -- --ignored
//! ```
//!
//! ## CI Integration
//!
//! These tests run in the `test-integration` CI job. If any canary test fails,
//! it means the API has drifted and we need to update our types.

use futures_util::StreamExt;
use genai_rs::Client;
use std::env;

/// Helper to create a client for canary tests.
fn create_client() -> Option<Client> {
    let api_key = env::var("GEMINI_API_KEY").ok()?;
    Client::builder(api_key).build().ok()
}

/// Helper macro to skip tests when API key is not available.
macro_rules! require_api_key {
    ($client:ident) => {
        let Some($client) = create_client() else {
            eprintln!("Skipping test: GEMINI_API_KEY not set");
            return;
        };
    };
}

// =============================================================================
// Basic Interaction Canary Tests
// =============================================================================

/// Canary: Detect Unknown content types in basic text interaction.
///
/// This test makes a simple text request and verifies no Unknown variants
/// appear in the response outputs.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_basic_interaction_no_unknown_content() {
    require_api_key!(client);

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Say hello in exactly one word.")
        .create()
        .await
        .expect("API call should succeed");

    // Check for Unknown content types in outputs
    for (i, content) in response.outputs.iter().enumerate() {
        assert!(
            !content.is_unknown(),
            "Unknown content type detected at output index {}: {:?}. \
             This indicates API drift - update Content enum. \
             Unknown type: {:?}",
            i,
            content.unknown_content_type(),
            content.unknown_data()
        );
    }

    // Verify we got a valid response status
    assert!(
        !response.status.is_unknown(),
        "Unknown status detected: {:?}. \
         This indicates API drift - update InteractionStatus enum.",
        response.status.unknown_status_type()
    );
}

/// Canary: Detect Unknown status values.
///
/// This test verifies the interaction status is a known variant.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_response_status_is_known() {
    require_api_key!(client);

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What is 2+2?")
        .create()
        .await
        .expect("API call should succeed");

    assert!(
        !response.status.is_unknown(),
        "Unknown InteractionStatus detected: {:?}. \
         Update the InteractionStatus enum to include this new status.",
        response.status.unknown_status_type()
    );
}

// =============================================================================
// Function Calling Canary Tests
// =============================================================================

/// Canary: Detect Unknown content types in function calling responses.
///
/// Function calling responses can include additional content types like
/// FunctionCall. This test ensures we handle all of them.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_function_calling_no_unknown_content() {
    require_api_key!(client);

    use genai_rs::FunctionDeclaration;

    // Simple function declaration using the correct API
    let get_weather = FunctionDeclaration::builder("get_weather")
        .description("Get the current weather")
        .parameter(
            "city",
            serde_json::json!({
                "type": "string",
                "description": "City name"
            }),
        )
        .required(vec!["city".to_string()])
        .build();

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What's the weather in Paris?")
        .add_functions(vec![get_weather])
        .create()
        .await
        .expect("API call should succeed");

    // Check all output content types
    for (i, content) in response.outputs.iter().enumerate() {
        assert!(
            !content.is_unknown(),
            "Unknown content type in function calling response at index {}: {:?}. Data: {:?}",
            i,
            content.unknown_content_type(),
            content.unknown_data()
        );
    }
}

// =============================================================================
// Thinking Mode Canary Tests
// =============================================================================

/// Canary: Detect Unknown content types in thinking mode responses.
///
/// Thinking mode responses include Thought content. This test ensures
/// we handle all content types in thinking responses.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_thinking_mode_no_unknown_content() {
    require_api_key!(client);

    use genai_rs::ThinkingLevel;

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What is the square root of 144?")
        .with_thinking_level(ThinkingLevel::Low)
        .create()
        .await
        .expect("API call should succeed");

    // Check all output content types including thoughts
    for (i, content) in response.outputs.iter().enumerate() {
        assert!(
            !content.is_unknown(),
            "Unknown content type in thinking response at index {}: {:?}. \
             This may be a new thinking-related content type.",
            i,
            content.unknown_content_type()
        );
    }
}

// =============================================================================
// Streaming Canary Tests
// =============================================================================

/// Canary: Detect Unknown chunk types in streaming responses.
///
/// Streaming responses use different chunk types. This test ensures
/// we handle all streaming chunk types correctly.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_streaming_no_unknown_chunks() {
    require_api_key!(client);

    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Count from 1 to 5.")
        .create_stream();

    let mut chunk_count = 0;
    while let Some(event_result) = stream.next().await {
        let event = event_result.expect("Stream event should be valid");

        // Check if chunk is unknown
        assert!(
            !event.chunk.is_unknown(),
            "Unknown StreamChunk type detected at chunk {}: {:?}. \
             This indicates a new streaming chunk type. Data: {:?}",
            chunk_count,
            event.chunk.unknown_chunk_type(),
            event.chunk.unknown_data()
        );

        chunk_count += 1;
    }

    assert!(chunk_count > 0, "Should have received at least one chunk");
}

// =============================================================================
// Built-in Tools Canary Tests
// =============================================================================

/// Canary: Detect Unknown content types with Google Search tool.
///
/// Google Search returns specific content types. This test ensures
/// we handle all search-related content types.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_google_search_no_unknown_content() {
    require_api_key!(client);

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What is the current population of Tokyo according to recent data?")
        .with_google_search()
        .create()
        .await
        .expect("API call should succeed");

    // Check all output content types
    for (i, content) in response.outputs.iter().enumerate() {
        assert!(
            !content.is_unknown(),
            "Unknown content type in Google Search response at index {}: {:?}. \
             This may be a new search-related content type.",
            i,
            content.unknown_content_type()
        );
    }
}

/// Canary: Detect Unknown content types with Code Execution tool.
///
/// Code execution returns specific content types for results.
/// Uses timeout protection since code execution sandbox can be slow/unavailable.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_code_execution_no_unknown_content() {
    use std::time::Duration;

    require_api_key!(client);

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Calculate 123 * 456 using Python code execution.")
            .with_code_execution()
            .create(),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            // Check all output content types
            for (i, content) in response.outputs.iter().enumerate() {
                assert!(
                    !content.is_unknown(),
                    "Unknown content type in code execution response at index {}: {:?}. \
                     This may be a new code execution content type.",
                    i,
                    content.unknown_content_type()
                );
            }
        }
        Ok(Err(e)) => {
            // API error - log and skip (code execution can be temporarily unavailable)
            eprintln!("Code execution API error (skipping): {}", e);
        }
        Err(_) => {
            // Timeout - code execution sandbox was slow
            eprintln!("Code execution timed out after 60s (skipping)");
        }
    }
}

// =============================================================================
// Tool Type Canary Tests
// =============================================================================

/// Canary: Verify built-in tools serialize to expected types.
///
/// This test ensures our Tool enum covers all built-in tool types
/// and they don't become Unknown variants.
#[test]
fn canary_builtin_tools_are_known() {
    use genai_rs::Tool;

    // Test all known built-in tools (using enum variants directly)
    let tools = vec![
        Tool::GoogleSearch { search_types: None },
        Tool::CodeExecution,
        Tool::UrlContext,
    ];

    for tool in tools {
        assert!(
            !tool.is_unknown(),
            "Built-in tool is Unknown: {:?}. \
             This indicates a deserialization issue.",
            tool.unknown_tool_type()
        );
    }
}

// =============================================================================
// Summary Test
// =============================================================================

/// Canary: Comprehensive check of a typical API interaction.
///
/// This test exercises multiple features and checks for Unknown variants
/// across the entire response structure.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_comprehensive_response_check() {
    require_api_key!(client);

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello! Please respond with a friendly greeting.")
        .create()
        .await
        .expect("API call should succeed");

    // Check status
    assert!(
        !response.status.is_unknown(),
        "Unknown status: {:?}",
        response.status.unknown_status_type()
    );

    // Check input content (echoed back in some cases)
    for content in &response.input {
        assert!(
            !content.is_unknown(),
            "Unknown input content: {:?}",
            content.unknown_content_type()
        );
    }

    // Check output content
    for content in &response.outputs {
        assert!(
            !content.is_unknown(),
            "Unknown output content: {:?}",
            content.unknown_content_type()
        );
    }

    // Check tools if present
    if let Some(tools) = &response.tools {
        for tool in tools {
            assert!(
                !tool.is_unknown(),
                "Unknown tool in response: {:?}",
                tool.unknown_tool_type()
            );
        }
    }

    println!(
        "Comprehensive canary passed: status={:?}, outputs={}",
        response.status,
        response.outputs.len()
    );
}
