//! Streaming in multi-turn conversations, with and without function calling.
//!
//! ```bash
//! cargo nextest run --test streaming_multiturn_tests --run-ignored all
//! ```

mod common;

use common::{assert_response_semantic, consume_stream, get_client, stateful_builder};
use genai_rs::{FunctionCallingMode, FunctionDeclaration, InteractionStatus, Step};
use serde_json::json;

// =============================================================================
// Multi-turn: Streaming
// =============================================================================

/// Test that streaming works correctly in a multi-turn conversation.
/// Turn 1 establishes context, Turn 2 uses streaming to verify recall.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_streaming_multi_turn_basic() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Turn 1: Establish a fact (non-streaming)
    let response1 = retry_request!([client] => {
        stateful_builder(&client)
            .with_text(
                "My favorite programming language is Python. Please acknowledge this.",
            )
            .create()
            .await
    })
    .expect("Turn 1 failed");

    assert_eq!(response1.status, InteractionStatus::Completed);

    // Turn 2: Stream a question that requires context from Turn 1
    let stream = stateful_builder(&client)
        .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
        .with_text("What is my favorite programming language? Answer in one word.")
        .create_stream();

    let result = consume_stream(stream).await;
    assert_eq!(
        result.final_response.as_ref().map(|r| &r.status),
        Some(&InteractionStatus::Completed)
    );

    assert_response_semantic(
        &client,
        "Turn 1 established 'My favorite programming language is Python'. Turn 2 asked 'What is my favorite programming language?'",
        &result.collected_text,
        "Does this response identify Python as the favorite programming language?",
    )
    .await;
}

/// Test streaming in a multi-turn conversation with function calling.
/// Turn 1: Trigger function call
/// Turn 2: Provide function result
/// Turn 3: Stream a follow-up question
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_streaming_multi_turn_function_calling() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let get_weather = FunctionDeclaration::builder("get_weather")
        .description("Get the current weather for a city")
        .parameter(
            "city",
            json!({"type": "string", "description": "The city name"}),
        )
        .required(vec!["city".to_string()])
        .build();

    // Turn 1: Trigger function call
    let response1 = retry_request!([client, get_weather] => {
        stateful_builder(&client)
            .with_text("What's the weather in Paris?")
            .add_function(get_weather)
            .with_function_calling_mode(FunctionCallingMode::Any)
            .create()
            .await
    })
    .expect("Turn 1 failed");

    let calls = response1.function_calls();
    let call = calls
        .first()
        .expect("FunctionCallingMode::Any should force a call");

    // Turn 2: Provide function result
    let function_result = Step::function_result(
        "get_weather",
        call.id.to_string(),
        json!({"temperature": "18°C", "conditions": "rainy", "humidity": "85%"}),
    );

    let prev_id = response1.id.clone().expect("id should exist");
    let response2 = retry_request!([client, prev_id, function_result, get_weather] => {
        stateful_builder(&client)
            .with_previous_interaction(&prev_id)
            .with_history(vec![function_result])
            .add_function(get_weather)
            .create()
            .await
    })
    .expect("Turn 2 failed");
    assert!(response2.has_text(), "Turn 2 should answer from the result");

    // Turn 3: Stream a follow-up question about the weather context
    let stream = stateful_builder(&client)
        .with_previous_interaction(response2.id.as_ref().expect("id should exist"))
        .with_text("Should I bring an umbrella? Answer briefly.")
        .add_function(get_weather)
        .create_stream();

    let result = consume_stream(stream).await;
    assert_eq!(
        result.final_response.as_ref().map(|r| &r.status),
        Some(&InteractionStatus::Completed)
    );

    assert_response_semantic(
        &client,
        "Turn 1 established weather in Paris: rainy, 18°C, high humidity. User asked 'Should I bring an umbrella?' in Turn 3.",
        &result.collected_text,
        "Does this response address whether to bring an umbrella based on the rainy weather?",
    )
    .await;
}
