//! Canaries for API drift: each exercises one response surface and fails if
//! the API returns a step, delta or status type the crate does not model.
//! `Unknown` keeps the library working when that happens; these make sure
//! someone finds out.
//!
//! Skipped under `strict-unknown`, where an unknown type is a deserialization
//! error rather than an `Unknown` to report.
//!
//! ```bash
//! cargo nextest run --test api_canary_tests --run-ignored all
//! ```

// Skip all tests in this module when strict-unknown is enabled
#![cfg(not(feature = "strict-unknown"))]

mod common;

use common::get_client;
use futures_util::StreamExt;
use genai_rs::InteractionInput;

const CANARY_MODEL: &str = genai_rs::DEFAULT_MODEL;

/// Panics with details if the response carries an unknown status or step.
fn assert_no_unknown_steps(response: &genai_rs::InteractionResponse, context: &str) {
    assert!(
        !response.status.is_unknown(),
        "API returned an unknown status in {context}: {:?}",
        response.status
    );
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

    let response = retry_request!([client] => {
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_text("Say 'hello' and nothing else.")
            .create()
            .await
    })
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
        .with_description("Get the current time")
        .build();

    let response = retry_request!([client, get_time] => {
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_text("What time is it?")
            .add_functions(vec![get_time.clone()])
            .with_function_calling_mode(genai_rs::FunctionCallingMode::Any)
            .create()
            .await
    })
    .expect("API call should succeed");

    assert_no_unknown_steps(&response, "function calling interaction");

    // The response to a function result is a different surface; check it too.
    {
        let calls = response.function_calls();
        let call = calls.first().expect("Any mode should force a call");

        use genai_rs::Step;

        // Build conversation history by replaying the model's actual output
        // steps — rebuilding the function_call by hand would drop its
        // signature, which the API requires on stateless replay.
        let mut steps = vec![Step::user_text("What time is it?")];
        steps.extend(response.output_steps());
        steps.push(Step::function_result(
            call.name,
            call.id,
            json!({"time": "12:00 PM"}),
        ));
        let history = InteractionInput::Steps(steps);

        let followup = retry_request!([client, history] => {
            client
                .interaction()
                .with_model(CANARY_MODEL)
                .with_input(history.clone())
                .create()
                .await
        })
        .expect("Follow-up API call should succeed");

        assert_no_unknown_steps(&followup, "function calling follow-up");
    }
}

/// Code execution returns its own call and result step types.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_code_execution_interaction() {
    let client = get_client().expect("GEMINI_API_KEY must be set");

    let response = retry_request!([client] => {
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_text("Use code execution to calculate 123 * 456")
            .with_code_execution()
            .create()
            .await
    })
    .expect("API call should succeed");

    assert!(
        !response.code_execution_results().is_empty(),
        "canary needs a code execution result to check"
    );
    assert_no_unknown_steps(&response, "code execution interaction");
}

/// Google Search returns its own call and result step types.
#[tokio::test]
#[ignore = "Requires API key"]
async fn canary_google_search_interaction() {
    let client = get_client().expect("GEMINI_API_KEY must be set");

    let response = retry_request!([client] => {
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_text("What is the current population of Tokyo according to recent data?")
            .with_google_search()
            .create()
            .await
    })
    .expect("API call should succeed");

    assert!(
        !response.google_search_calls().is_empty(),
        "canary needs a search step to check"
    );
    assert_no_unknown_steps(&response, "google search interaction");
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

    let response = retry_request!([client, input] => {
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_input(input.clone())
            .create()
            .await
    })
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

    let response = retry_request!([client, config] => {
        client
            .interaction()
            .with_model(CANARY_MODEL)
            .with_text("What is 15 * 23?")
            .with_generation_config(config.clone())
            .create()
            .await
    })
    .expect("API call should succeed");

    assert_no_unknown_steps(&response, "thinking model interaction");
}
