//! API Canary Tests
//!
//! These tests act as early-warning "canaries" to detect when the Gemini API
//! starts returning content types that the library doesn't recognize.
//!
//! When these tests fail, it indicates:
//! 1. Google has added new content types to the API
//! 2. The library should be updated to add proper support
//!
//! The Unknown variant ensures the library doesn't break, but we want to know
//! when new types appear so we can add first-class support.
//!
//! # Test Execution Time
//!
//! These tests make 6 API calls and typically complete in 12-60 seconds total.
//! Consider using `--test-threads=1` to avoid rate limiting.
//!
//! # Feature Flags
//!
//! These tests are SKIPPED when `strict-unknown` feature is enabled, since they
//! rely on graceful degradation behavior (capturing unknown types in Unknown variants).
//! In strict mode, unknown types cause deserialization errors instead.

// Skip all tests in this module when strict-unknown is enabled
#![cfg(not(feature = "strict-unknown"))]

mod common;

use common::get_client;
use futures_util::StreamExt;
use genai_rs::InteractionInput;

/// Model used for all canary tests - update if model availability changes
const CANARY_MODEL: &str = "gemini-3-flash-preview";

/// Helper to check a response for unknown step types and panic with details if found
fn assert_no_unknown_steps(response: &genai_rs::InteractionResponse, context: &str) {
    if response.has_unknown() {
        let summary = response.step_summary();
        panic!(
            "API returned unknown step types in {context}!\n\
             Unknown types: {:?}\n\
             Full summary: {summary}\n\n\
             Action required: Add support for these step types in \
             src/",
            summary.unknown_types
        );
    }
}

/// Canary test for basic text interaction
///
/// Tests the simplest API call pattern to detect any new content types
/// in basic text responses.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_basic_text_interaction() {
    let client = get_client().expect("GEMINI_API_KEY must be set");

    let response = client
        .interaction()
        .with_model(CANARY_MODEL)
        .with_text("Say 'hello' and nothing else.")
        .create()
        .await
        .expect("API call should succeed");

    assert_no_unknown_steps(&response, "basic text interaction");
}

/// Canary test for streaming interaction
///
/// Tests streaming responses to detect any new delta content types.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_streaming_interaction() {
    let client = get_client().expect("GEMINI_API_KEY must be set");

    let mut stream = client
        .interaction()
        .with_model(CANARY_MODEL)
        .with_text("Count from 1 to 3.")
        .create_stream();

    let mut unknown_types_found = Vec::new();
    let mut chunk_count = 0;

    while let Some(result) = stream.next().await {
        chunk_count += 1;
        let event = result.expect("Stream event should be valid");
        match event.chunk {
            genai_rs::StreamChunk::StepDelta { delta, .. } => {
                if let genai_rs::StepDelta::Unknown { delta_type, .. } = &delta
                    && !unknown_types_found.contains(delta_type)
                {
                    unknown_types_found.push(delta_type.clone());
                }
            }
            genai_rs::StreamChunk::Completed(response) => {
                assert_no_unknown_steps(&response, "streaming completed response");
            }
            _ => {} // Handle unknown variants
        }
    }

    assert!(chunk_count > 0, "Streaming should yield at least one chunk");

    if !unknown_types_found.is_empty() {
        panic!(
            "API returned unknown delta types in streaming deltas!\n\
             Unknown types: {:?}\n\n\
             Action required: Add support for these delta types in \
             src/",
            unknown_types_found
        );
    }
}

/// Canary test for function calling interaction
///
/// Tests function calling responses to detect any new content types
/// in function call/result handling.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_function_calling_interaction() {
    use genai_rs::FunctionDeclaration;
    use serde_json::json;

    let client = get_client().expect("GEMINI_API_KEY must be set");

    let get_time = FunctionDeclaration::builder("get_current_time")
        .description("Get the current time")
        .build();

    let response = client
        .interaction()
        .with_model(CANARY_MODEL)
        .with_text("What time is it?")
        .add_functions(vec![get_time])
        .create()
        .await
        .expect("API call should succeed");

    assert_no_unknown_steps(&response, "function calling interaction");

    // Also check the follow-up response after providing function results.
    // This tests for unknown types in the model's response to function results,
    // which may differ from the initial function call response.
    if !response.function_calls().is_empty() {
        let call = &response.function_calls()[0];

        use genai_rs::Step;

        // Build conversation history with the function call and result
        let history = InteractionInput::Steps(vec![
            Step::user_text("What time is it?"),
            Step::function_call(call.id, call.name, call.args.clone()),
            Step::function_result(call.name, call.id, json!({"time": "12:00 PM"})),
        ]);

        let followup = client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_input(history)
            .create()
            .await
            .expect("Follow-up API call should succeed");

        assert_no_unknown_steps(&followup, "function calling follow-up");
    }
}

/// Canary test for code execution tool
///
/// Tests the built-in code execution tool to detect any new content types.
/// Uses timeout protection since code execution sandbox can be slow/unavailable.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_code_execution_interaction() {
    use genai_rs::Tool;
    use std::time::Duration;

    let client = get_client().expect("GEMINI_API_KEY must be set");

    let result = tokio::time::timeout(
        Duration::from_secs(60),
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_text("Use code execution to calculate 2 + 2")
            .set_tools(vec![Tool::CodeExecution])
            .create(),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            assert_no_unknown_steps(&response, "code execution interaction");
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

/// Canary test for multimodal interaction
///
/// Tests image input to detect any new content types in multimodal responses.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_multimodal_interaction() {
    use genai_rs::Content;

    let client = get_client().expect("GEMINI_API_KEY must be set");

    // Use a tiny 1x1 red PNG
    let input = InteractionInput::Content(vec![
        Content::text("What color is this image?"),
        Content::image_data(common::TINY_RED_PNG_BASE64, "image/png"),
    ]);

    let response = client
        .interaction()
        .with_model(CANARY_MODEL)
        .with_input(input)
        .create()
        .await
        .expect("API call should succeed");

    assert_no_unknown_steps(&response, "multimodal interaction");
}

/// Canary test for thinking/reasoning models
///
/// Tests models with extended thinking to detect any new thought-related content types.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_thinking_model_interaction() {
    use genai_rs::{GenerationConfig, ThinkingLevel};

    let client = get_client().expect("GEMINI_API_KEY must be set");

    // Use generation config with thinking level enabled
    let config = GenerationConfig {
        thinking_level: Some(ThinkingLevel::Medium),
        ..Default::default()
    };

    let response = client
        .interaction()
        .with_model(CANARY_MODEL)
        .with_text("What is 15 * 23?")
        .with_generation_config(config)
        .create()
        .await
        .expect("API call should succeed");

    assert_no_unknown_steps(&response, "thinking model interaction");
}
