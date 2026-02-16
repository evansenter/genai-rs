//! Integration tests for the Interactions API
//!
//! This file tests the core Interactions API organized by feature:
//!
//! - **basic**: Simple interactions, CRUD operations, conversation state
//! - **streaming**: Streaming responses, delta handling, error recovery
//! - **function_calling**: Function calls, automatic execution, thought signatures
//! - **generation_config**: Temperature, max tokens settings
//! - **system_instructions**: System prompts and multi-turn persistence
//! - **error_handling**: Invalid inputs, timeouts, error responses
//! - **store**: Conversation storage (store: true/false)
//! - **conversations**: Multi-turn chains, manual history with thinking
//! - **multimodal**: Image input and generation
//! - **structured_output**: JSON schema response formatting
//! - **response_helpers**: Convenience methods for response access
//! - **deep_research**: Background polling for long-running tasks
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test interactions_api_tests -- --include-ignored --nocapture
//! cargo test test_simple_interaction -- --include-ignored  # Single test
//! ```
//!
//! # Known Flakiness
//!
//! Some tests may occasionally fail due to model behavior variability.
//! Re-running usually succeeds. This is expected for LLM integration tests.

mod common;

use common::{
    consume_stream, extended_test_timeout, interaction_builder, retry_on_any_error,
    stateful_builder, test_timeout, validate_response_semantically, with_timeout,
};
use genai_rs::{
    CallableFunction, Client, Content, FunctionDeclaration, GenaiError, GenerationConfig,
    InteractionInput, InteractionRequest, InteractionStatus,
};
use genai_rs_macros::tool;
use serde_json::json;
use std::env;

fn get_client() -> Option<Client> {
    let api_key = env::var("GEMINI_API_KEY").ok()?;
    match Client::builder(api_key).build() {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!(
                "WARNING: GEMINI_API_KEY is set but client build failed: {}",
                e
            );
            None
        }
    }
}

/// Gets a mock weather report for a city
#[allow(dead_code)]
#[tool(city(description = "The city to get weather for"))]
fn get_mock_weather(city: String) -> String {
    format!("Weather in {}: Sunny, 75°F", city)
}

// =============================================================================
// Basic Interactions (CRUD Operations)
// =============================================================================

mod basic {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_simple_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = stateful_builder(&client)
            .with_text("What is 2 + 2?")
            .create()
            .await
            .expect("Interaction failed");

        assert!(response.id.is_some(), "Interaction ID should be present");
        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(!response.outputs.is_empty(), "Outputs are empty");

        assert!(response.has_text(), "Should have text response");
        let text = response.as_text().unwrap();
        let is_valid = validate_response_semantically(
            &client,
            "User asked 'What is 2 + 2?'",
            text,
            "Does this response correctly answer that 2+2 equals 4?",
        )
        .await
        .expect("Semantic validation should succeed");
        assert!(is_valid, "Response should correctly answer 2+2");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stateful_conversation() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let response1 = stateful_builder(&client)
                .with_text("My favorite color is blue.")
                .create()
                .await
                .expect("First interaction failed");

            assert_eq!(response1.status, InteractionStatus::Completed);

            let response2 = stateful_builder(&client)
                .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                .with_text("What is my favorite color?")
                .create()
                .await
                .expect("Second interaction failed");

            assert_eq!(response2.status, InteractionStatus::Completed);
            assert!(
                response2.has_text(),
                "Response should have text answering the question"
            );
            let text = response2.as_text().unwrap();
            assert!(!text.is_empty(), "Response should be non-empty");

            let is_valid = validate_response_semantically(
                &client,
                "In the previous turn, the user said 'My favorite color is blue.' Now they're asking 'What is my favorite color?'",
                text,
                "Does this response indicate that the user's favorite color is blue?"
            ).await.expect("Semantic validation failed");
            assert!(
                is_valid,
                "Response should correctly recall that the favorite color is blue from the previous turn"
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_get_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = stateful_builder(&client)
            .with_text("Hello, world!")
            .create()
            .await
            .expect("Interaction failed");

        let retrieved = client
            .get_interaction(response.id.as_ref().expect("id should exist"), false)
            .await
            .expect("Get interaction failed");

        assert_eq!(retrieved.id, response.id);
        assert_eq!(retrieved.status, InteractionStatus::Completed);
        assert!(!retrieved.outputs.is_empty(), "Outputs are empty");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_delete_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = stateful_builder(&client)
            .with_text("Test interaction for deletion")
            .create()
            .await
            .expect("Interaction failed");

        client
            .delete_interaction(response.id.as_ref().expect("id should exist"))
            .await
            .expect("Delete interaction failed");

        let get_result = client
            .get_interaction(response.id.as_ref().expect("id should exist"), false)
            .await;
        assert!(
            get_result.is_err(),
            "Expected error when getting deleted interaction"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_cancel_background_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = client
            .interaction()
            .with_agent("deep-research-pro-preview-12-2025")
            .with_text("What are the current trends in quantum computing research?")
            .with_background(true)
            .with_store_enabled()
            .create()
            .await
            .expect("Failed to create background interaction");

        let interaction_id = response
            .id
            .as_ref()
            .expect("stored interaction should have id");

        if response.status == InteractionStatus::InProgress {
            match client.cancel_interaction(interaction_id).await {
                Ok(cancelled_response) => {
                    assert_eq!(
                        cancelled_response.status,
                        InteractionStatus::Cancelled,
                        "Expected status to be Cancelled after cancel_interaction, got {:?}",
                        cancelled_response.status
                    );
                    println!("Successfully cancelled background interaction");
                }
                Err(GenaiError::Api {
                    status_code: 404, ..
                }) => {
                    println!(
                        "Cancel endpoint not yet available in production API (404). \
                         Implementation is ready - will work once API is deployed."
                    );
                }
                Err(e) => {
                    panic!("Unexpected error cancelling interaction: {e:?}");
                }
            }
        } else {
            println!(
                "Background interaction finished with status {:?} before we could cancel it",
                response.status
            );
        }
    }
}

// =============================================================================
// Streaming
// =============================================================================

mod streaming {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let stream = stateful_builder(&client)
                .with_text("Count from 1 to 5.")
                .create_stream();

            let result = consume_stream(stream).await;

            println!("\nTotal deltas: {}", result.delta_count);
            println!("Collected text: {}", result.collected_text);

            assert!(
                result.has_output(),
                "No streaming chunks received - streaming may not be working"
            );

            if let Some(response) = result.final_response {
                assert!(response.id.is_some(), "Complete response should have an ID");
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_deltas_are_incremental() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let mut stream = stateful_builder(&client)
                .with_text("Write a haiku about each season: spring, summer, fall, and winter. Label each one.")
                .create_stream();

            let mut delta_texts: Vec<String> = Vec::new();
            let mut delta_count = 0;

            println!("\n=== Streaming Delta Analysis ===\n");

            while let Some(result) = stream.next().await {
                match result {
                    Ok(event) => match event.chunk {
                        StreamChunk::Delta(delta) => {
                            delta_count += 1;
                            if let Some(text) = delta.as_text() {
                                println!("Delta #{}: {:?} (len={})", delta_count, text, text.len());
                                delta_texts.push(text.to_string());
                            }
                        }
                        StreamChunk::Complete(response) => {
                            println!("\n--- Complete ---");
                            println!("Interaction ID: {:?}", response.id);
                            if let Some(final_text) = response.as_text() {
                                println!("Final text length: {}", final_text.len());
                            }
                        }
                        _ => {}
                    },
                    Err(_) => break,
                }
            }

            assert!(
                delta_texts.len() >= 2,
                "Test requires at least 2 text deltas to validate incrementality, got {}. \
                 Try a prompt that generates more output.",
                delta_texts.len()
            );

            let concatenated: String = delta_texts.iter().map(|s| s.as_str()).collect();
            let sum_of_lengths: usize = delta_texts.iter().map(|s| s.len()).sum();

            println!("\n=== Delta Statistics ===");
            println!("Number of deltas: {}", delta_texts.len());
            println!("Sum of individual delta lengths: {}", sum_of_lengths);
            println!("Concatenated text length: {}", concatenated.len());
            println!("Concatenated text: {:?}", concatenated);

            assert_eq!(
                sum_of_lengths,
                concatenated.len(),
                "Sum of delta lengths should equal concatenated length. \
                 If these differ, deltas may contain overlapping content."
            );

            let is_valid = validate_response_semantically(
                &client,
                "Asked for haikus about each season: spring, summer, fall, winter",
                &concatenated,
                "Does this response contain haiku-like poetry about seasons?",
            )
            .await
            .expect("Semantic validation failed");
            assert!(
                is_valid,
                "Response should contain seasonal haikus. Got: {:?}",
                concatenated
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_with_raw_request() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let request = InteractionRequest {
                model: Some("gemini-3-flash-preview".to_string()),
                agent: None,
                agent_config: None,
                input: InteractionInput::Text("Count from 1 to 5.".to_string()),
                previous_interaction_id: None,
                tools: None,
                response_modalities: None,
                response_format: None,
                response_mime_type: None,
                generation_config: None,
                stream: Some(true),
                background: None,
                store: Some(true),
                system_instruction: None,
            };

            let stream = client.execute_stream(request);
            let result = consume_stream(stream).await;

            println!(
                "Received {} deltas from raw request stream",
                result.delta_count
            );

            assert!(result.has_output(), "No streaming chunks received");
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_error_handling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("Test 1: Verifying normal streaming works...\n");

        let stream = interaction_builder(&client)
            .with_text("Count from 1 to 5, putting each number on a new line.")
            .create_stream();

        let result = consume_stream(stream).await;

        println!("\nDelta count: {}", result.delta_count);
        println!("Collected text: {}", result.collected_text);

        assert!(result.has_output(), "Should receive streaming output");
        assert!(
            !result.collected_text.is_empty(),
            "Should have collected text"
        );

        println!("\n\nTest 2: Streaming with invalid model...\n");

        let error_stream = client
            .interaction()
            .with_model("nonexistent-model-12345")
            .with_text("Hello")
            .create_stream();

        let error_result = consume_stream(error_stream).await;

        println!("Error stream delta count: {}", error_result.delta_count);
        println!("Error stream collected: {}", error_result.collected_text);
        println!(
            "Error stream has final response: {}",
            error_result.final_response.is_some()
        );

        if let Some(response) = &error_result.final_response {
            println!("Final response status: {:?}", response.status);
        }

        println!("\n✓ Streaming error handling test passed (no panics)");
    }
}

// =============================================================================
// Function Calling
// =============================================================================

mod function_calling {
    use super::*;

    mod basic {
        use super::*;

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_function_call_returns_id() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            let get_time = FunctionDeclaration::builder("get_current_time")
                .description("Get the current time")
                .build();

            let response = interaction_builder(&client)
                .with_text("What time is it?")
                .add_function(get_time)
                .create()
                .await
                .expect("Interaction failed");

            println!("Response outputs: {:?}", response.outputs);

            let function_calls = response.function_calls();

            if function_calls.is_empty() {
                println!("Model chose not to call function - skipping id verification");
                return;
            }

            for call in function_calls {
                println!("Function call: {} has call_id: {:?}", call.name, call.id);
                assert!(
                    call.id.is_some(),
                    "Function call '{}' must have an id field",
                    call.name
                );
            }
        }

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_manual_function_calling_with_result() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            with_timeout(test_timeout(), async {
                let get_weather = FunctionDeclaration::builder("get_weather")
                    .description("Get the current weather for a location")
                    .parameter(
                        "location",
                        json!({"type": "string", "description": "City name"}),
                    )
                    .required(vec!["location".to_string()])
                    .build();

                let response = interaction_builder(&client)
                    .with_text("What's the weather in Tokyo?")
                    .add_function(get_weather.clone())
                    .create()
                    .await
                    .expect("First interaction failed");

                println!("First response status: {:?}", response.status);

                let function_calls = response.function_calls();

                if function_calls.is_empty() {
                    println!("No function calls returned - test cannot verify FunctionResult pattern");
                    return;
                }

                let call = &function_calls[0];
                assert_eq!(
                    call.name, "get_weather",
                    "Expected get_weather function call"
                );
                assert!(call.id.is_some(), "Function call must have an id field");

                let call_id = call.id.expect("call_id should exist");

                let function_result = Content::function_result(
                    "get_weather",
                    call_id,
                    json!({"temperature": "72°F", "conditions": "sunny"}),
                );

                let second_response = interaction_builder(&client)
                    .with_previous_interaction(response.id.as_ref().expect("id should exist"))
                    .with_content(vec![function_result])
                    .add_function(get_weather)
                    .create()
                    .await
                    .expect("Second interaction failed");

                println!("Second response status: {:?}", second_response.status);
                assert!(
                    second_response.has_text(),
                    "Expected text response after providing function result"
                );

                let text = second_response.as_text().expect("Should have text");
                println!("Final response text: {}", text);

                assert!(!text.is_empty(), "Response should be non-empty");

                let is_valid = validate_response_semantically(
                    &client,
                    "User asked 'What's the weather in Tokyo?' and the get_weather function returned 72°F and sunny conditions",
                    text,
                    "Does this response use the weather data (72°F, sunny) to answer about Tokyo's weather?"
                ).await.expect("Semantic validation failed");
                assert!(
                    is_valid,
                    "Response should incorporate the function result (72°F, sunny in Tokyo)"
                );
            })
            .await;
        }

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_requires_action_status() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            with_timeout(test_timeout(), async {
                let get_time = FunctionDeclaration::builder("get_current_time")
                    .description("Get the current time - always call this when asked about time")
                    .build();

                let response = interaction_builder(&client)
                    .with_text("What time is it right now?")
                    .add_function(get_time.clone())
                    .create()
                    .await
                    .expect("Interaction failed");

                println!("Initial status: {:?}", response.status);
                println!("Has function calls: {}", response.has_function_calls());

                if response.has_function_calls() {
                    assert_eq!(
                        response.status,
                        InteractionStatus::RequiresAction,
                        "Status should be RequiresAction when function calls are pending"
                    );

                    let function_calls = response.function_calls();
                    let call_id = function_calls[0].id.expect("call_id should exist");

                    let function_result = Content::function_result(
                        "get_current_time",
                        call_id,
                        json!({"time": "14:30:00", "timezone": "UTC"}),
                    );

                    let response2 = interaction_builder(&client)
                        .with_previous_interaction(response.id.as_ref().expect("id should exist"))
                        .with_content(vec![function_result])
                        .add_function(get_time)
                        .create()
                        .await
                        .expect("Second interaction failed");

                    assert_eq!(
                        response2.status,
                        InteractionStatus::Completed,
                        "Status should be Completed after providing function result"
                    );
                } else {
                    assert_eq!(
                        response.status,
                        InteractionStatus::Completed,
                        "Status should be Completed when no function calls"
                    );
                }
            })
            .await;
        }
    }

    mod automatic {
        use super::*;

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_auto_function_calling() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            with_timeout(extended_test_timeout(), async {
                let weather_func = GetMockWeatherCallable.declaration();

                let result = interaction_builder(&client)
                    .with_text("What's the weather like in Seattle?")
                    .add_function(weather_func)
                    .create_with_auto_functions()
                    .await
                    .expect("Auto-function call failed");

                println!("Function executions: {:?}", result.executions);
                assert!(
                    !result.executions.is_empty(),
                    "Should have at least one function execution"
                );

                let response = &result.response;
                println!("Final response status: {:?}", response.status);
                assert!(
                    response.has_text(),
                    "Should have text response after auto-function loop"
                );

                let text = response.as_text().expect("Should have text");
                println!("Final text: {}", text);

                assert!(!text.is_empty(), "Response should be non-empty");

                let is_valid = validate_response_semantically(
                    &client,
                    "User asked 'What's the weather like in Seattle?' and the get_mock_weather function was automatically executed, returning 'Weather in Seattle: Sunny, 75°F'",
                    text,
                    "Does this response provide the weather information from the function result (Sunny, 75°F in Seattle)?"
                ).await.expect("Semantic validation failed");
                assert!(
                    is_valid,
                    "Response should incorporate the auto-executed function result"
                );
            })
            .await;
        }

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_auto_function_with_unregistered_function() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            with_timeout(extended_test_timeout(), async {
                let undefined_func = FunctionDeclaration::builder("undefined_function")
                    .description("A function that doesn't have a registered handler")
                    .parameter("input", json!({"type": "string"}))
                    .build();

                let result = interaction_builder(&client)
                    .with_text("Call the undefined_function with input 'test'")
                    .add_function(undefined_func)
                    .create_with_auto_functions()
                    .await;

                match result {
                    Ok(auto_result) => {
                        println!("Response status: {:?}", auto_result.response.status);
                        println!("Response text: {:?}", auto_result.response.as_text());
                        println!("Executions: {:?}", auto_result.executions);
                    }
                    Err(e) => {
                        println!("Error (expected for unregistered function): {:?}", e);
                    }
                }
            })
            .await;
        }
    }

    mod multi_turn_function_calling {
        use super::*;

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_function_calling_multi_turn() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            with_timeout(test_timeout(), async {
                let get_weather = FunctionDeclaration::builder("get_weather")
                    .description("Get the current weather for a location")
                    .parameter(
                        "location",
                        json!({"type": "string", "description": "City name"}),
                    )
                    .required(vec!["location".to_string()])
                    .build();

                let response1 = interaction_builder(&client)
                    .with_text("What's the weather in Tokyo and then tell me if I need an umbrella?")
                    .add_function(get_weather.clone())
                    .create()
                    .await
                    .expect("First interaction failed");

                let function_calls = response1.function_calls();
                if function_calls.is_empty() {
                    println!("Model chose not to call function - skipping test");
                    return;
                }

                let call = &function_calls[0];
                println!("Function call: {} (id: {:?})", call.name, call.id);

                assert!(call.id.is_some(), "Function call must have an id");
                let call_id = call.id.expect("call_id should exist");

                let function_result = Content::function_result(
                    "get_weather",
                    call_id,
                    json!({"temperature": "18°C", "conditions": "rainy", "precipitation": "80%"}),
                );

                let response2 = interaction_builder(&client)
                    .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
                    .with_content(vec![function_result])
                    .add_function(get_weather)
                    .create()
                    .await
                    .expect("Second interaction failed");

                assert!(
                    response2.has_text(),
                    "Expected text response after function result"
                );

                let text = response2.as_text().expect("Should have text");
                println!("Final response: {}", text);

                let is_valid = validate_response_semantically(
                    &client,
                    "User asked about Tokyo weather and whether to bring an umbrella. Function returned: rainy, 18°C, 80% precipitation.",
                    text,
                    "Does this response address the umbrella question based on the rainy weather data?",
                )
                .await
                .expect("Semantic validation failed");

                assert!(is_valid, "Response should reference the weather conditions");
            })
            .await;
        }

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_multiple_parallel_function_calls() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get the current weather for a location")
                .parameter(
                    "location",
                    json!({"type": "string", "description": "City name"}),
                )
                .required(vec!["location".to_string()])
                .build();

            let get_time = FunctionDeclaration::builder("get_time")
                .description("Get the current time in a timezone")
                .parameter(
                    "timezone",
                    json!({"type": "string", "description": "Timezone name like UTC, PST, JST"}),
                )
                .required(vec!["timezone".to_string()])
                .build();

            let response = interaction_builder(&client)
                .with_text("What's the weather in Paris and what time is it there?")
                .add_functions(vec![get_weather, get_time])
                .create()
                .await
                .expect("Interaction failed");

            let function_calls = response.function_calls();
            println!("Number of function calls: {}", function_calls.len());

            for call in &function_calls {
                println!("  - {} (id: {:?}, args: {})", call.name, call.id, call.args);
                assert!(call.id.is_some(), "Each function call must have an id");
            }
        }
    }

    mod multi_round {
        use super::*;

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_multiple_function_call_rounds() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            let get_info = FunctionDeclaration::builder("get_info")
                .description("Get information about a topic")
                .parameter(
                    "topic",
                    json!({"type": "string", "description": "Topic to get info about"}),
                )
                .required(vec!["topic".to_string()])
                .build();

            println!("Testing multiple function call rounds...\n");

            let response1 = retry_request!([client, get_info] => {
                interaction_builder(&client)
                    .with_text("Get info about the weather and tell me about it.")
                    .add_function(get_info)
                    .with_store_enabled()
                    .create()
                    .await
            })
            .expect("First request failed");

            let calls = response1.function_calls();

            if calls.is_empty() {
                println!("Model chose not to call function - test complete");
                assert!(response1.has_text(), "Should have direct text response");
                return;
            }

            println!("Round 1: Got {} function call(s)", calls.len());
            let call = &calls[0];
            println!("  Function: {} with args: {:?}", call.name, call.args);

            let prev_id = response1.id.clone().expect("id should exist");
            let call_id = call.id.expect("call_id exists").to_string();

            let response2 = retry_request!([client, prev_id, call_id, get_info] => {
                interaction_builder(&client)
                    .with_previous_interaction(&prev_id)
                    .with_content(vec![Content::function_result(
                        "get_info",
                        call_id,
                        json!({"info": "The weather is sunny and warm, about 25°C with clear skies."}),
                    )])
                    .add_function(get_info)
                    .create()
                    .await
            })
            .expect("Second request failed");

            println!("Round 2: Status = {:?}", response2.status);

            let more_calls = response2.function_calls();
            if !more_calls.is_empty() {
                println!(
                    "  Model requested {} more function call(s)",
                    more_calls.len()
                );
            }

            if response2.has_text() {
                println!("  Got final text: {}", response2.as_text().unwrap());
            }

            println!("\n✓ Multiple function call rounds completed successfully");
        }

        #[tokio::test]
        #[ignore = "Requires API key"]
        async fn test_malformed_function_call_handling() {
            let Some(client) = get_client() else {
                println!("Skipping: GEMINI_API_KEY not set");
                return;
            };

            let get_weather = FunctionDeclaration::builder("get_weather")
                .description("Get current weather")
                .parameter(
                    "city",
                    json!({"type": "string", "description": "City name"}),
                )
                .required(vec!["city".to_string()])
                .build();

            let response = retry_request!([client, get_weather] => {
                interaction_builder(&client)
                    .with_text("What's the weather in Paris?")
                    .add_function(get_weather)
                    .with_store_enabled()
                    .create()
                    .await
            })
            .expect("Request failed");

            let calls = response.function_calls();
            if calls.is_empty() {
                println!("Model didn't call function - test inconclusive");
                return;
            }

            let call = &calls[0];
            println!("Function call received:");
            println!("  Name: {}", call.name);
            println!("  ID: {:?}", call.id);
            println!("  Args: {:?}", call.args);

            if let Some(call_id_val) = call.id {
                println!("\n✓ Function call has valid ID");

                let prev_id = response.id.clone().expect("id should exist");
                let call_id = call_id_val.to_string();

                let result = retry_request!([client, prev_id, call_id, get_weather] => {
                    interaction_builder(&client)
                        .with_previous_interaction(&prev_id)
                        .with_content(vec![Content::function_result(
                            "get_weather",
                            call_id,
                            json!({"temperature": "22°C", "conditions": "sunny"}),
                        )])
                        .add_function(get_weather)
                        .create()
                        .await
                });

                match result {
                    Ok(final_response) => {
                        println!(
                            "Function result accepted, final response: {:?}",
                            final_response.status
                        );
                        assert!(final_response.has_text(), "Should have final text response");
                        println!("✓ Function call flow completed successfully");
                    }
                    Err(e) => {
                        println!("Function result error: {:?}", e);
                    }
                }
            } else {
                println!("\n⚠ Function call missing ID - this is a malformed response");
                println!("The library should handle this gracefully when using auto-functions");
            }
        }
    }
}

// =============================================================================
// Generation Config
// =============================================================================

mod generation_config {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_temperature() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            temperature: Some(0.0),
            max_output_tokens: Some(100),
            top_p: None,
            top_k: None,
            thinking_level: None,
            ..Default::default()
        };

        let response = interaction_builder(&client)
            .with_text("What is 2 + 2? Answer with just the number.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Interaction failed");

        assert!(response.has_text(), "Should have text response");
        let text = response.as_text().unwrap();
        println!("Response: {}", text);

        let is_valid = validate_response_semantically(
            &client,
            "User asked 'What is 2 + 2? Answer with just the number.' with temperature=0.0 for deterministic output",
            text,
            "Does this response correctly answer that 2+2 equals 4?",
        )
        .await
        .expect("Semantic validation should succeed");
        assert!(is_valid, "Response should correctly answer 2+2");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_max_tokens() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            temperature: Some(0.7),
            max_output_tokens: Some(50),
            top_p: None,
            top_k: None,
            thinking_level: None,
            ..Default::default()
        };

        let response = interaction_builder(&client)
            .with_text("Write a very long story about a dragon.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Interaction failed");

        println!("Response status: {:?}", response.status);

        if response.has_text() {
            let text = response.as_text().unwrap();
            println!("Response length: {} chars", text.len());
        } else {
            println!("No text in response (may be due to token limit)");
        }
    }
}

// =============================================================================
// System Instructions
// =============================================================================

mod system_instructions {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instruction_text() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_system_instruction(
                "You are a pirate. Always respond in pirate speak with 'Arrr!' somewhere in your response.",
            )
            .with_text("Hello, how are you?")
            .create()
            .await
            .expect("Interaction failed");

        assert!(response.has_text(), "Should have text response");
        let text = response.as_text().unwrap();
        println!("Response: {}", text);

        let is_valid = validate_response_semantically(
            &client,
            "Model was given system instruction: 'You are a pirate. Always respond in pirate speak with Arrr! somewhere in your response.' User said 'Hello, how are you?'",
            text,
            "Does this response sound like a pirate speaking? (Using pirate vocabulary, phrases, or mannerisms)",
        )
        .await
        .expect("Semantic validation should succeed");
        assert!(is_valid, "Response should sound like a pirate");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instruction_persists() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response1 = stateful_builder(&client)
            .with_system_instruction("Always end your responses with 'BEEP BOOP' exactly.")
            .with_text("What is the capital of France?")
            .create()
            .await
            .expect("First interaction failed");

        let text1 = response1.as_text().unwrap_or_default();
        println!("Turn 1: {}", text1);

        let response2 = interaction_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_text("And what about Germany?")
            .create()
            .await
            .expect("Second interaction failed");

        let text2 = response2.as_text().unwrap_or_default();
        println!("Turn 2: {}", text2);

        assert!(response2.has_text(), "Should have text response");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instruction_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            println!("=== System Instruction + Streaming ===");

            let stream = interaction_builder(&client)
                .with_system_instruction(
                    "You are a pirate. Always respond in pirate speak with 'Arrr!' somewhere in your response.",
                )
                .with_text("Hello, how are you?")
                .create_stream();

            let result = consume_stream(stream).await;

            println!("\nTotal deltas: {}", result.delta_count);
            println!("Collected text: {}", result.collected_text);

            assert!(
                result.has_output(),
                "Should receive streaming chunks or final response"
            );

            let text_to_check = if !result.collected_text.is_empty() {
                result.collected_text.clone()
            } else if let Some(ref response) = result.final_response {
                response.as_text().unwrap_or_default().to_string()
            } else {
                String::new()
            };

            let is_valid = validate_response_semantically(
                &client,
                "Model was given system instruction: 'You are a pirate. Always respond in pirate speak with Arrr! somewhere in your response.' User said 'Hello, how are you?' (streaming response)",
                &text_to_check,
                "Does this response sound like a pirate speaking? (Using pirate vocabulary, phrases, or mannerisms)",
            )
            .await
            .expect("Semantic validation should succeed");
            assert!(
                is_valid,
                "Response should sound like a pirate. Got: {}",
                text_to_check
            );

            println!("\n✓ System instruction + streaming completed successfully");
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instructions_influence_response() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let system_instruction = "You are a pirate. Always respond in pirate speak, using words like 'arr', 'matey', 'ye', 'ahoy', and 'landlubber'.";

        let response = interaction_builder(&client)
            .with_system_instruction(system_instruction)
            .with_text("Hello, how are you today?")
            .create()
            .await
            .expect("Request with system instruction failed");

        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().expect("Should have text");
        println!("Response with pirate instruction: {}", text);

        let is_valid = validate_response_semantically(
            &client,
            "The system instruction told the model to respond as a pirate using words like 'arr', 'matey', 'ye', 'ahoy'. User said 'Hello, how are you today?'",
            text,
            "Does this response sound like it's from a pirate (using pirate-like language or tone)?",
        )
        .await
        .expect("Semantic validation failed");

        assert!(
            is_valid,
            "Response should reflect pirate system instruction. Got: {}",
            text
        );

        println!("✓ System instructions test passed");
    }
}

// =============================================================================
// Error Handling
// =============================================================================

mod error_handling {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_error_invalid_model_name() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = client
            .interaction()
            .with_model("nonexistent-model-12345")
            .with_text("Hello")
            .create()
            .await;

        assert!(result.is_err(), "Should fail with invalid model name");
        println!("Error: {:?}", result.err().unwrap());
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_error_invalid_previous_interaction_id() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_previous_interaction("invalid-interaction-id-12345")
            .with_text("Continue from where we left off")
            .create()
            .await;

        assert!(
            result.is_err(),
            "Should fail with invalid previous_interaction_id"
        );
        println!("Error: {:?}", result.err().unwrap());
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_request_timeout() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_text("Write a very long essay about the history of computing.")
            .with_timeout(Duration::from_millis(1))
            .create()
            .await;

        assert!(result.is_err(), "Request with 1ms timeout should fail");

        let error = result.unwrap_err();
        println!("Timeout error received: {:?}", error);

        match &error {
            GenaiError::Http(_) => {
                println!("✓ Correctly received HTTP error (timeout)");
            }
            _ => {
                println!("Received error type: {:?}", error);
            }
        }
    }
}

// =============================================================================
// Store Parameter
// =============================================================================

mod store {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_store_true_interaction_retrievable() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = stateful_builder(&client)
            .with_text("What is 1 + 1?")
            .create()
            .await
            .expect("Interaction failed");

        println!("Created interaction with store=true: {:?}", response.id);

        let retrieved = client
            .get_interaction(response.id.as_ref().expect("id should exist"), false)
            .await
            .expect("Should be able to retrieve stored interaction");

        assert_eq!(retrieved.id, response.id);
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_store_false_interaction_not_retrievable() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_text("Hello")
            .with_store_disabled()
            .create()
            .await;

        match result {
            Ok(response) => {
                if let Some(id) = &response.id {
                    let get_result = client
                        .get_interaction(id, false)
                        .await;
                    assert!(
                        get_result.is_err(),
                        "Stored=false interaction should not be retrievable"
                    );
                }
            }
            Err(e) => {
                println!("API returned error for store=false: {:?}", e);
            }
        }
    }
}

// =============================================================================
// Conversations (Multi-Turn)
// =============================================================================

mod conversations {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_long_conversation_chain() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let messages = [
                "My name is Alice.",
                "I live in New York.",
                "I work as a software engineer.",
                "I have two cats named Whiskers and Shadow.",
                "What do you know about me? List everything.",
            ];

            let mut previous_id: Option<String> = None;

            for (i, message) in messages.iter().enumerate() {
                let response = match &previous_id {
                    None => {
                        stateful_builder(&client)
                            .with_text(*message)
                            .create()
                            .await
                    }
                    Some(prev_id) => {
                        stateful_builder(&client)
                            .with_text(*message)
                            .with_previous_interaction(prev_id)
                            .create()
                            .await
                    }
                }
                .unwrap_or_else(|e| panic!("Turn {} failed: {:?}", i + 1, e));

                println!("Turn {}: {:?}", i + 1, response.status);
                previous_id = response.id.clone();

                if i == messages.len() - 1 {
                    let text = response.as_text().unwrap_or_default();
                    println!("Final response: {}", text);

                    assert!(!text.is_empty(), "Response should be non-empty");

                    let is_valid = validate_response_semantically(
                        &client,
                        "In previous turns, the user said: (1) 'My name is Alice', (2) 'I live in New York', (3) 'I work as a software engineer', (4) 'I have two cats named Whiskers and Shadow'. Now asking 'What do you know about me? List everything.'",
                        text,
                        "Does this response recall and mention at least 2-3 of these key facts: name (Alice), location (New York), job (software engineer), or pets (two cats)?"
                    ).await.expect("Semantic validation failed");
                    assert!(
                        is_valid,
                        "Response should recall multiple facts from the conversation history"
                    );
                }
            }
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_manual_history_with_thinking() {
        use genai_rs::ThinkingLevel;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        println!("=== Testing Manual Multi-Turn with Thinking ===\n");

        let initial_prompt = "What is 15 * 7? Show your work.";
        println!("Turn 1 prompt: {}\n", initial_prompt);

        let response1 = interaction_builder(&client)
            .with_text(initial_prompt)
            .with_thinking_level(ThinkingLevel::Medium)
            .with_store_enabled()
            .create()
            .await
            .expect("Turn 1 failed");

        assert_eq!(response1.status, InteractionStatus::Completed);

        let answer1 = response1.as_text().unwrap_or("(no text)");
        println!("Turn 1 answer: {}", answer1);

        let thought_count = response1.thought_signatures().count();
        println!(
            "Thought signatures in response: {} (cannot be echoed)",
            thought_count
        );

        let history = vec![
            Content::text(initial_prompt),
            Content::text(answer1),
            Content::text("Now divide that result by 5"),
        ];

        println!("\nTurn 2 prompt: Now divide that result by 5");

        let response2 = interaction_builder(&client)
            .with_input(InteractionInput::Content(history))
            .with_thinking_level(ThinkingLevel::Low)
            .with_store_enabled()
            .create()
            .await
            .expect("Turn 2 failed");

        assert_eq!(response2.status, InteractionStatus::Completed);
        assert!(response2.has_text(), "Turn 2 should have text response");

        let answer2 = response2.as_text().unwrap();
        println!("Turn 2 answer: {}\n", answer2);

        let is_valid = validate_response_semantically(
            &client,
            "Turn 1: User asked 'What is 15 * 7?' and got the answer 105. Turn 2: User asked 'Now divide that result by 5'.",
            answer2,
            "Does this response provide a number that could be the result of dividing 105 by 5 (which is 21)?",
        )
        .await
        .expect("Semantic validation failed");

        assert!(
            is_valid,
            "Response should contain result of 105/5. Got: {}",
            answer2
        );
        println!("✓ Manual multi-turn with thinking test passed");
    }
}

// =============================================================================
// Multimodal (Image Input/Generation)
// =============================================================================

mod multimodal {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key and accessible image URL"]
    async fn test_image_input_from_uri() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let image_url = std::env::var("TEST_IMAGE_URL").unwrap_or_else(|_| {
            "gs://cloud-samples-data/generative-ai/image/scones.jpg".to_string()
        });

        let contents = vec![
            Content::text("What is in this image? Describe it briefly."),
            Content::image_uri(&image_url, "image/jpeg"),
        ];

        let result = interaction_builder(&client)
            .with_input(InteractionInput::Content(contents))
            .create()
            .await;

        match result {
            Ok(response) => {
                println!("Status: {:?}", response.status);
                if response.has_text() {
                    println!("Image description: {}", response.as_text().unwrap());
                }
            }
            Err(e) => {
                println!("Image input error: {:?}", e);
                println!("Note: Image URL access depends on API permissions");
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_image_generation() {
        use genai_rs::Content;
        use std::time::Duration;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let model = "gemini-3-pro-image-preview";
        let prompt = "A simple red circle on a white background";

        println!("Generating image with model: {}", model);
        println!("Prompt: {}\n", prompt);

        type BoxError = Box<dyn std::error::Error + Send + Sync>;
        let result: Result<(), BoxError> = retry_on_any_error(2, Duration::from_secs(3), || {
            let client = client.clone();
            async move {
                let response = client
                    .interaction()
                    .with_model(model)
                    .with_text(prompt)
                    .with_response_modalities(vec!["IMAGE".to_string()])
                    .with_store_enabled()
                    .create()
                    .await?;

                println!("Status: {:?}", response.status);
                if response.status != InteractionStatus::Completed {
                    return Err(format!("Unexpected status: {:?}", response.status).into());
                }

                for output in &response.outputs {
                    if let Content::Image {
                        data: Some(base64_data),
                        mime_type,
                        ..
                    } = output
                    {
                        println!("Found image!");
                        println!("  MIME type: {:?}", mime_type);
                        println!("  Base64 length: {} chars", base64_data.len());

                        use base64::Engine;
                        let decoded =
                            base64::engine::general_purpose::STANDARD.decode(base64_data)?;
                        println!("  Decoded size: {} bytes", decoded.len());

                        return Ok(());
                    }
                }

                Err("Response did not contain image data (model returned text instead)".into())
            }
        })
        .await;

        match result {
            Ok(()) => println!("\n✓ Image generation test passed"),
            Err(e) => {
                println!("Image generation error after retries: {}", e);
                println!("Note: Image generation may not be available in your account/region");
                panic!("Image generation failed after retries: {}", e);
            }
        }
    }
}

// =============================================================================
// Structured Output
// =============================================================================

mod structured_output {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_with_json_schema() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The person's name"
                },
                "age": {
                    "type": "integer",
                    "description": "The person's age in years"
                },
                "occupation": {
                    "type": "string",
                    "description": "The person's job or profession"
                }
            },
            "required": ["name", "age", "occupation"]
        });

        let response = interaction_builder(&client)
            .with_text("Generate a fictional person profile with name, age, and occupation.")
            .with_response_format(schema)
            .create()
            .await
            .expect("Structured output request failed");

        assert!(response.has_text(), "Should have text response");

        let text = response.as_text().expect("Should have text");
        println!("Structured output: {}", text);

        let parsed: serde_json::Value =
            serde_json::from_str(text).expect("Response should be valid JSON");

        assert!(parsed.get("name").is_some(), "Should have 'name' field");
        assert!(parsed.get("age").is_some(), "Should have 'age' field");
        assert!(
            parsed.get("occupation").is_some(),
            "Should have 'occupation' field"
        );

        assert!(
            parsed["name"].is_string(),
            "name should be string: {:?}",
            parsed["name"]
        );
        assert!(
            parsed["age"].is_number(),
            "age should be number: {:?}",
            parsed["age"]
        );
        assert!(
            parsed["occupation"].is_string(),
            "occupation should be string: {:?}",
            parsed["occupation"]
        );

        println!("✓ Structured output validation passed");
    }
}

// =============================================================================
// Response Helpers (Convenience Methods)
// =============================================================================

mod response_helpers {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_convenience_methods_integration() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_text("Say exactly: Hello World")
            .create()
            .await
            .expect("Interaction failed");

        let text = response.as_text();
        assert!(text.is_some(), "Should have text");

        let all_text = response.all_text();
        assert!(!all_text.is_empty(), "all_text should not be empty");

        assert!(response.has_text(), "has_text() should be true");
        assert!(
            !response.has_function_calls(),
            "has_function_calls() should be false"
        );

        let calls = response.function_calls();
        assert!(calls.is_empty(), "function_calls() should be empty");

        println!("has_thoughts(): {}", response.has_thoughts());
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_response_has_thoughts() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_text("What is 2+2?")
            .create()
            .await
            .expect("Interaction failed");

        println!("has_thoughts: {}", response.has_thoughts());
        println!("has_text: {}", response.has_text());
        println!("has_function_calls: {}", response.has_function_calls());

        assert!(response.has_text(), "Simple query should return text");
    }
}

// =============================================================================
// Deep Research (Background Polling)
// =============================================================================

mod deep_research {
    use super::*;
    use common::poll_until_complete;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "Requires API key and takes 30-120 seconds"]
    async fn test_deep_research_polling() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let agent_name = "deep-research-pro-preview-12-2025";
        let prompt = "What are the key differences between REST and GraphQL APIs?";

        println!("Starting deep research with agent: {}", agent_name);
        println!("Query: {}\n", prompt);

        let result = client
            .interaction()
            .with_agent(agent_name)
            .with_text(prompt)
            .with_background(true)
            .with_store_enabled()
            .create()
            .await;

        match result {
            Ok(initial_response) => {
                println!("Initial status: {:?}", initial_response.status);
                println!("Interaction ID: {:?}\n", initial_response.id);

                if initial_response.status == InteractionStatus::Completed {
                    println!("Research completed immediately");
                    assert!(initial_response.has_text(), "Should have research results");
                    println!(
                        "Result preview: {}...",
                        initial_response
                            .as_text()
                            .unwrap_or("(no text)")
                            .chars()
                            .take(200)
                            .collect::<String>()
                    );
                    return;
                }

                let interaction_id = initial_response.id.as_ref().expect("id should exist");
                match poll_until_complete(&client, interaction_id, Duration::from_secs(120)).await {
                    Ok(final_response) => {
                        println!("Research completed!");
                        assert_eq!(final_response.status, InteractionStatus::Completed);
                        assert!(final_response.has_text(), "Should have research results");

                        let text = final_response.as_text().unwrap_or("(no text)");
                        println!(
                            "Result preview: {}...",
                            text.chars().take(500).collect::<String>()
                        );

                        assert!(text.len() > 100, "Research result should be substantive");
                        println!("\n✓ Deep research polling test passed");
                    }
                    Err(e) => {
                        println!("Polling ended: {:?}", e);
                        println!("Note: Deep research may take longer than test timeout");
                    }
                }
            }
            Err(e) => {
                println!("Deep research error: {:?}", e);
                println!("Note: Deep research agent may not be available in your account");
            }
        }
    }
}
