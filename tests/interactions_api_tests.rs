//! Core Interactions API tests: CRUD, background mode, streaming, generation
//! config, system instructions, error mapping, `store`, and response helpers.
//!
//! Function calling lives in `function_calling_tests.rs`, built-in tools in
//! `tools_and_config_tests.rs`, and conversation mechanics in
//! `multiturn_tests.rs`.
//!
//! ```bash
//! cargo nextest run --test interactions_api_tests --run-ignored all
//! ```

mod common;

use common::{
    assert_response_semantic, consume_stream, extended_test_timeout, get_client,
    interaction_builder, poll_until_done, stateful_builder, test_timeout, with_timeout,
};
use genai_rs::{
    GenaiError, GenerationConfig, InteractionInput, InteractionRequest, InteractionStatus, Step,
};

// =============================================================================
// Basic Interactions (CRUD Operations)
// =============================================================================

mod basic {
    use super::*;
    use std::time::Duration;

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
        assert!(!response.steps.is_empty(), "Steps are empty");

        let text = response.as_text().expect("Should have text response");
        assert_response_semantic(
            &client,
            "User asked 'What is 2 + 2?'",
            text,
            "Does this response correctly answer that 2+2 equals 4?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stateful_conversation() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // Two retried calls can exceed the 60s default on a slow API day.
        with_timeout(extended_test_timeout(), async {
            let response1 = retry_request!([client] => {
                stateful_builder(&client)
                    .with_text("My favorite color is blue.")
                    .create()
                    .await
            })
            .expect("First interaction failed");
            assert_eq!(response1.status, InteractionStatus::Completed);

            let prev_id = response1.id.clone().expect("id should exist");
            let response2 = retry_request!([client, prev_id] => {
                stateful_builder(&client)
                    .with_previous_interaction(&prev_id)
                    .with_text("What is my favorite color?")
                    .create()
                    .await
            })
            .expect("Second interaction failed");

            assert_eq!(response2.status, InteractionStatus::Completed);
            let text = response2.as_text().expect("Should have text");
            assert_response_semantic(
                &client,
                "In the previous turn, the user said 'My favorite color is blue.' Now they're asking 'What is my favorite color?'",
                text,
                "Does this response indicate that the user's favorite color is blue?",
            )
            .await;
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

        let response = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Hello, world!")
                .create()
                .await
        })
        .expect("Interaction failed");

        let retrieved = client
            .get_interaction(response.id.as_ref().expect("id should exist"))
            .await
            .expect("Get interaction failed");

        assert_eq!(retrieved.id, response.id);
        assert_eq!(retrieved.status, InteractionStatus::Completed);
        assert!(!retrieved.steps.is_empty(), "Steps are empty");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_delete_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Test interaction for deletion")
                .create()
                .await
        })
        .expect("Interaction failed");
        let id = response.id.expect("id should exist");

        client
            .delete_interaction(&id)
            .await
            .expect("Delete interaction failed");

        let get_result = client.get_interaction(&id).await;
        assert!(
            matches!(
                get_result,
                Err(GenaiError::Api {
                    status_code: 404,
                    ..
                })
            ),
            "a deleted interaction should 404, got: {get_result:?}"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_cancel_background_interaction() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        // A deep-research run takes minutes, so it is reliably still running
        // when the cancel lands. A retried create at worst orphans a bounded
        // background interaction.
        let response = retry_request!([client] => {
            client
                .interaction()
                .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
                .with_text("What are the current trends in quantum computing research?")
                .with_background(true)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Failed to create background interaction");

        assert_eq!(response.status, InteractionStatus::InProgress);
        let id = response.id.expect("stored interaction should have id");

        let cancelled = client
            .cancel_interaction(&id)
            .await
            .expect("cancel_interaction failed");
        assert_eq!(cancelled.status, InteractionStatus::Cancelled);
    }

    /// Background mode on a plain model: accepted as `in_progress`, then
    /// retrievable by polling until it completes.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_background_interaction_polls_to_completion() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("What is the capital of France? Answer in one word.")
                .with_background(true)
                .create()
                .await
        })
        .expect("background create failed");
        let id = response
            .id
            .expect("background interaction should have an id");

        let done = poll_until_done(&client, &id, Duration::from_secs(90)).await;
        assert_eq!(done.status, InteractionStatus::Completed);
        let text = done
            .as_text()
            .expect("completed interaction should have text");
        assert_response_semantic(
            &client,
            "Asked for the capital of France in one word",
            text,
            "Does this response identify Paris?",
        )
        .await;
    }
}

// =============================================================================
// Streaming
// =============================================================================

mod streaming {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::{StreamChunk, ThinkingLevel, ThinkingSummaries};

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

            assert!(result.delta_count > 0, "no deltas received");
            assert!(!result.collected_text.is_empty(), "no text streamed");
            let response = result.final_response.expect("no Completed event");
            assert!(response.id.is_some(), "Complete response should have an ID");
            assert_eq!(response.status, InteractionStatus::Completed);
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
            let mut final_text = None;
            while let Some(result) = stream.next().await {
                match result.expect("stream error").chunk {
                    StreamChunk::StepDelta { delta, .. } => {
                        if let Some(text) = delta.as_text() {
                            delta_texts.push(text.to_string());
                        }
                    }
                    StreamChunk::Completed(response) => {
                        final_text = response.as_text().map(str::to_string);
                    }
                    _ => {}
                }
            }

            assert!(
                delta_texts.len() >= 2,
                "need at least 2 text deltas to test incrementality, got {}",
                delta_texts.len()
            );

            // Deltas are fragments, not cumulative snapshots: their
            // concatenation is the final text, with nothing repeated.
            let concatenated: String = delta_texts.concat();
            if let Some(final_text) = final_text {
                assert_eq!(concatenated, final_text, "deltas should concatenate to the final text");
            }

            assert_response_semantic(
                &client,
                "Asked for haikus about each season: spring, summer, fall, winter",
                &concatenated,
                "Does this response contain haiku-like poetry about seasons?",
            )
            .await;
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
                model: Some(genai_rs::DEFAULT_MODEL.to_string()),
                input: InteractionInput::Text("Count from 1 to 5.".to_string()),
                stream: Some(true),
                store: Some(true),
                ..Default::default()
            };

            let result = consume_stream(client.execute_stream(request)).await;

            assert!(result.delta_count > 0, "no deltas received");
            assert!(result.final_response.is_some(), "no Completed event");
        })
        .await;
    }

    /// A stream request for a model that does not exist fails with the API's
    /// 404 as the stream's first item, rather than an empty stream.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_invalid_model_yields_error() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = client
            .interaction()
            .with_model("nonexistent-model-12345")
            .with_text("Hello")
            .create_stream();

        let first = stream.next().await.expect("stream ended without an item");
        assert!(
            matches!(
                first,
                Err(GenaiError::Api {
                    status_code: 404,
                    ..
                })
            ),
            "expected a 404 for an unknown model, got: {first:?}"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_long_response() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let stream = stateful_builder(&client)
                .with_text("Write a detailed 500-word essay about the history of the Internet.")
                .create_stream();

            let result = consume_stream(stream).await;

            assert!(
                result.delta_count > 5,
                "a long response should arrive in many deltas, got {}",
                result.delta_count
            );
            assert!(
                result.collected_text.len() > 500,
                "response should be substantial, got {} chars",
                result.collected_text.len()
            );
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_streaming_with_thinking_summaries() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let stream = stateful_builder(&client)
                .with_text("Explain briefly why the sky is blue.")
                .with_thinking_level(ThinkingLevel::Medium)
                .with_thinking_summaries(ThinkingSummaries::Auto)
                .create_stream();

            let result = consume_stream(stream).await;

            assert!(result.saw_thought, "thinking with summaries should stream a thought");
            assert!(!result.collected_text.is_empty(), "no text streamed");
            assert_response_semantic(
                &client,
                "Asked 'Why is the sky blue?' with thinking mode enabled",
                &result.collected_text,
                "Does this response explain the scientific reason for the sky appearing blue (light, scattering, wavelengths)?",
            )
            .await;
        })
        .await;
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
            // Headroom: this is about temperature, and the model spends ~100
            // thinking tokens even on this prompt (verified live 2026-08-10).
            max_output_tokens: Some(2000),
            ..Default::default()
        };

        let response = interaction_builder(&client)
            .with_text("What is 2 + 2? Answer with just the number.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Interaction failed");

        let text = response.as_text().expect("Should have text response");
        assert_response_semantic(
            &client,
            "User asked 'What is 2 + 2? Answer with just the number.' with temperature=0.0",
            text,
            "Does this response correctly answer that 2+2 equals 4?",
        )
        .await;
    }

    /// The cap is enforced: a long request stops as `incomplete`, and
    /// thinking draws from the same budget (verified live 2026-09-24).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_max_tokens() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        const CAP: u32 = 50;
        let config = GenerationConfig {
            max_output_tokens: Some(CAP as i32),
            ..Default::default()
        };

        let response = retry_request!([client, config] => {
            interaction_builder(&client)
                .with_text("Write a very long story about a dragon.")
                .with_generation_config(config)
                .create()
                .await
        })
        .expect("Interaction failed");

        assert_eq!(response.status, InteractionStatus::Incomplete);
        let usage = response.usage.expect("usage should be reported");
        let spent =
            usage.total_output_tokens.unwrap_or(0) + usage.total_thought_tokens.unwrap_or(0);
        assert!(spent <= CAP, "spent {spent} tokens against a cap of {CAP}");
    }
}

// =============================================================================
// System Instructions
// =============================================================================

mod system_instructions {
    use super::*;

    const PIRATE: &str =
        "You are a pirate. Always respond in pirate speak with 'Arrr!' somewhere in your response.";

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instruction_text() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_system_instruction(PIRATE)
            .with_text("Hello, how are you?")
            .create()
            .await
            .expect("Interaction failed");

        let text = response.as_text().expect("Should have text response");
        assert_response_semantic(
            &client,
            "Model was given system instruction: 'You are a pirate. Always respond in pirate speak.' User said 'Hello, how are you?'",
            text,
            "Does this response sound like a pirate speaking? (Using pirate vocabulary, phrases, or mannerisms)",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_system_instruction_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let stream = interaction_builder(&client)
                .with_system_instruction(PIRATE)
                .with_text("Hello, how are you?")
                .create_stream();

            let result = consume_stream(stream).await;
            assert!(!result.collected_text.is_empty(), "no text streamed");

            assert_response_semantic(
                &client,
                "Model was given system instruction: 'You are a pirate. Always respond in pirate speak.' User said 'Hello, how are you?' (streaming response)",
                &result.collected_text,
                "Does this response sound like a pirate speaking? (Using pirate vocabulary, phrases, or mannerisms)",
            )
            .await;
        })
        .await;
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

        match result {
            Err(GenaiError::Api {
                status_code: 404,
                message,
                ..
            }) => assert!(
                message.contains("nonexistent-model-12345"),
                "the 404 should name the model: {message}"
            ),
            other => panic!("expected a 404 for an unknown model, got: {other:?}"),
        }
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

        // The API answers a malformed id with a generic 400 (verified live
        // 2026-09-24), not a 404.
        assert!(
            matches!(
                result,
                Err(GenaiError::Api {
                    status_code: 400,
                    ..
                })
            ),
            "expected a 400 for a malformed previous_interaction_id, got: {result:?}"
        );
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

        assert!(
            matches!(result, Err(GenaiError::Timeout(_))),
            "expected GenaiError::Timeout, got: {result:?}"
        );
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

        let retrieved = client
            .get_interaction(response.id.as_ref().expect("id should exist"))
            .await
            .expect("Should be able to retrieve stored interaction");

        assert_eq!(retrieved.id, response.id);
    }

    /// An unstored interaction is never given an id, so there is nothing to
    /// retrieve (verified live 2026-09-24).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_store_false_interaction_has_no_id() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_text("Hello")
            .with_store_disabled()
            .create()
            .await
            .expect("store=false request failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(
            response.id.is_none(),
            "an unstored interaction should have no id, got {:?}",
            response.id
        );
    }
}

// =============================================================================
// Conversations (Manual History)
// =============================================================================

mod conversations {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_manual_history_with_thinking() {
        use genai_rs::ThinkingLevel;

        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let initial_prompt = "What is 15 * 7? Show your work.";
        let response1 = retry_request!([client] => {
            interaction_builder(&client)
                .with_text(initial_prompt)
                .with_thinking_level(ThinkingLevel::Medium)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 1 failed");
        assert_eq!(response1.status, InteractionStatus::Completed);
        let answer1 = response1.as_text().expect("Turn 1 should have text");

        // Thought signatures cannot be echoed as model text, so manual
        // history carries only the visible answer.
        let history = vec![
            Step::user_text(initial_prompt),
            Step::model_text(answer1),
            Step::user_text("Now divide that result by 5"),
        ];

        let response2 = retry_request!([client, history] => {
            interaction_builder(&client)
                .with_history(history.clone())
                .with_thinking_level(ThinkingLevel::Low)
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Turn 2 failed");

        assert_eq!(response2.status, InteractionStatus::Completed);
        let answer2 = response2.as_text().expect("Turn 2 should have text");
        assert_response_semantic(
            &client,
            "Turn 1: User asked 'What is 15 * 7?' and got the answer 105. Turn 2: User asked 'Now divide that result by 5'.",
            answer2,
            "Does this response give 21 (105 divided by 5)?",
        )
        .await;
    }
}

// =============================================================================
// Response Helpers
// =============================================================================

mod response_helpers {
    use super::*;
    use genai_rs::{ThinkingLevel, ThinkingSummaries};

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

        assert!(response.has_text(), "has_text() should be true");
        assert!(response.as_text().is_some(), "Should have text");
        assert!(
            !response.all_text().is_empty(),
            "all_text should not be empty"
        );
        assert!(
            !response.has_function_calls(),
            "has_function_calls() should be false"
        );
        assert!(
            response.function_calls().is_empty(),
            "function_calls() should be empty"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_response_has_thoughts() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_text("What is 17 * 23?")
            .with_thinking_level(ThinkingLevel::Medium)
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .create()
            .await
            .expect("Interaction failed");

        assert!(
            response.has_thoughts(),
            "thinking with summaries should return a thought step"
        );
        assert!(response.has_text(), "should also return the answer");
    }
}
