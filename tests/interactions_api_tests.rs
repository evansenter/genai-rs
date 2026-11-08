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

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a failure by panicking"
)]

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
        // google-genai 2.34.0; on every response live (2026-11-01).
        let usage = response.usage.as_ref().expect("usage");
        assert!(usage.initial_input_tokens.is_some(), "{usage:?}");

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
    async fn test_interactions_get() {
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
            .interactions()
            .get(response.id.as_ref().expect("id should exist"))
            .await
            .expect("Get interaction failed");

        assert_eq!(retrieved.id, response.id);
        assert_eq!(retrieved.status, InteractionStatus::Completed);
        assert!(!retrieved.steps.is_empty(), "Steps are empty");
        assert!(
            retrieved.input.is_none(),
            "a plain get leaves the input out: {:?}",
            retrieved.input
        );

        // `include_input=true` echoes the input as a `user_input` step (live
        // 2026-11-04).
        let with_input = client
            .interactions()
            .get_with_input(response.id.as_ref().expect("id should exist"))
            .await
            .expect("Get interaction with input failed");
        match &with_input.input {
            Some(InteractionInput::Steps(steps)) => assert!(
                matches!(steps.first(), Some(Step::UserInput { .. })),
                "expected the input as a user_input step, got {steps:?}"
            ),
            other => panic!("expected the input echoed as steps, got {other:?}"),
        }
    }

    /// A stored interaction read back serves the request's generation
    /// config, response format, modalities and safety settings, and each is
    /// modeled on `InteractionResponse` (live 2026-11-05; they used to land
    /// in `extra`). With `include_input=true` the lists repeat their entries
    /// (live 2026-11-04 and 2026-11-05), which this checks only loosely, so a
    /// server fix does not fail it.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_stored_read_serves_the_request_config() {
        use genai_rs::{
            HarmCategory, ResponseFormat, ResponseFormatSpec, SafetySetting, SafetyThreshold,
        };
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };
        let setting = SafetySetting::new(HarmCategory::HateSpeech, SafetyThreshold::BlockOnlyHigh);
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"answer": {"type": "integer"}}
        });

        let response = retry_request!([client, setting, schema] => {
            stateful_builder(&client)
                .with_text("Reply with a JSON object whose answer is 4.")
                .with_stop_sequences(vec!["HALT".to_string()])
                .with_response_format(ResponseFormat::json_schema(schema))
                .with_response_modalities(vec!["text".to_string()])
                .add_safety_setting(setting)
                .create()
                .await
        })
        .expect("Interaction failed");
        let id = response.id.clone().expect("id should exist");
        let stored = client.interactions().get(&id).await;
        let with_input = client.interactions().get_with_input(&id).await;
        let _ = client.interactions().delete(&id).await;
        let stored = stored.expect("Get interaction failed");
        let with_input = with_input.expect("Get interaction with input failed");

        for key in [
            "generation_config",
            "response_format",
            "response_modalities",
            "safety_settings",
        ] {
            assert!(
                !stored.extra.contains_key(key),
                "{key} landed in extra: {:?}",
                stored.extra
            );
        }
        let config = stored
            .generation_config
            .as_ref()
            .expect("generation_config");
        assert_eq!(config.stop_sequences, Some(vec!["HALT".to_string()]));
        assert!(config.extra.is_empty(), "{:?}", config.extra);
        match &stored.response_format {
            Some(ResponseFormatSpec::Single(ResponseFormat::Text { mime_type, .. })) => {
                assert_eq!(mime_type.as_deref(), Some("application/json"));
            }
            other => panic!("expected the JSON text format, got {other:?}"),
        }
        assert_eq!(stored.response_modalities, Some(vec!["text".to_string()]));
        assert_eq!(stored.safety_settings, Some(vec![setting]));

        let repeated = with_input
            .generation_config
            .and_then(|c| c.stop_sequences)
            .expect("stop_sequences on the include_input read");
        assert!(
            !repeated.is_empty() && repeated.iter().all(|s| s == "HALT"),
            "{repeated:?}"
        );
        println!("include_input stop_sequences: {repeated:?}");
    }

    /// The stable `/v1` endpoint serves interactions as `/v1beta` does
    /// (create, get, delete; live 2026-11-05), and answers 404 for the
    /// resources it lacks, such as webhooks (#489).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_v1_endpoint_serves_interactions() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };
        let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY");
        let v1 = genai_rs::Client::builder(api_key)
            .with_api_version("v1")
            .build()
            .expect("v1 client");

        let response = retry_request!([v1] => {
            v1.interaction()
                .with_model(genai_rs::DEFAULT_MODEL)
                .with_text("Say hi in two words.")
                .create()
                .await
        })
        .expect("create on /v1");
        let id = response.id.clone().expect("a stored interaction has an id");
        let fetched = v1.interactions().get(&id).await;
        // Created on /v1, readable on /v1beta.
        let fetched_beta = client.interactions().get(&id).await;
        let deleted = v1.interactions().delete(&id).await;
        assert_eq!(response.status, InteractionStatus::Completed);
        assert_eq!(
            fetched.expect("get on /v1").id.as_deref(),
            Some(id.as_str())
        );
        assert_eq!(
            fetched_beta.expect("get on /v1beta").id.as_deref(),
            Some(id.as_str())
        );
        deleted.expect("delete on /v1");

        let webhooks = v1.webhooks().list().send().await;
        assert!(
            matches!(
                webhooks,
                Err(GenaiError::Api {
                    status_code: 404,
                    ..
                })
            ),
            "/v1 has no webhooks, got {webhooks:?}"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_interactions_delete() {
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
            .interactions()
            .delete(&id)
            .await
            .expect("Delete interaction failed");

        let get_result = client.interactions().get(&id).await;
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
            .interactions()
            .cancel(&id)
            .await
            .expect("interactions().cancel failed");
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

    /// About 80 live assertions trust the semantic validator, so it needs a
    /// control: a validator that drifted lenient would pass every one of
    /// them without a marker.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_semantic_validator_rejects_a_wrong_answer() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let judge = |answer: &'static str| {
            let client = client.clone();
            async move {
                crate::retry_request!([client] => {
                    common::validate_response_semantically(
                        &client,
                        "Asked: What is the capital of France?",
                        answer,
                        "Does this response correctly identify Paris as the capital of France?",
                    )
                    .await
                })
                .expect("validator request failed")
            }
        };
        assert!(
            !judge("The capital of France is Berlin.").await,
            "the validator accepted a wrong answer"
        );
        assert!(
            judge("The capital of France is Paris.").await,
            "the validator rejected a right answer"
        );
    }
}

// =============================================================================
// Streaming
// =============================================================================

mod streaming {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::{StreamChunk, ThinkingLevel, ThinkingSummaries};

    /// A `store: false` stream's status updates carry no interaction ID
    /// (live 2026-11-01). They used to be dropped, with a warn, for that.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_unstored_stream_surfaces_status_updates() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(test_timeout(), async {
            let mut stream = interaction_builder(&client)
                .with_text("Say OK.")
                .with_store_disabled()
                .create_stream();
            let mut updates = Vec::new();
            let mut completed = false;
            while let Some(event) = stream.next().await {
                match event.expect("stream error").chunk {
                    StreamChunk::StatusUpdate {
                        interaction_id,
                        status,
                        ..
                    } => updates.push((interaction_id, status)),
                    StreamChunk::Completed { .. } => completed = true,
                    StreamChunk::Error { message, .. } => panic!("stream error event: {message}"),
                    _ => {}
                }
            }
            assert!(completed, "no completed event");
            assert!(!updates.is_empty(), "no status update reached the caller");
            assert!(
                updates.iter().all(|(id, _)| id.is_none()),
                "an unstored stream named an interaction: {updates:?}"
            );
        })
        .await;
    }

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
            let mut id = None;
            while let Some(result) = stream.next().await {
                match result.expect("stream error").chunk {
                    StreamChunk::StepDelta { delta, .. } => {
                        if let Some(text) = delta.as_text() {
                            delta_texts.push(text.to_string());
                        }
                    }
                    StreamChunk::Completed { interaction: response, .. } => id = response.id,
                    StreamChunk::Error { message, .. } => panic!("stream error event: {message}"),
                    _ => {}
                }
            }

            assert!(
                delta_texts.len() >= 2,
                "need at least 2 text deltas to test incrementality, got {}",
                delta_texts.len()
            );

            // Deltas are fragments, not cumulative snapshots: their
            // concatenation is the stored text, with nothing repeated. The
            // stored interaction is the oracle: the Completed response's text
            // can be rebuilt by the crate from these same deltas, which
            // would make the comparison hold by construction.
            let concatenated: String = delta_texts.concat();
            let id = id.expect("stream ended without a completed interaction id");
            let stored = client.interactions().get(&id).await.expect("get stored interaction");
            let stored_text = stored.as_text().expect("stored interaction has no text");
            assert_eq!(concatenated, stored_text, "deltas should concatenate to the stored text");

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
                input: Some(InteractionInput::Text("Count from 1 to 5.".to_string())),
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

    /// Decoding continuation (google-genai 2.34.0), live on the continuation
    /// model only: a response cut off by `max_output_tokens` is
    /// `incomplete` with a token, and resending the same input with the
    /// token and a larger (cumulative) budget continues it.
    ///
    /// Checked on status, token and steps, not on text: thinking can spend
    /// a budget before any text is written, so either response may have
    /// none, and a text check flaked on exactly that (#504).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_decoding_continuation_resumes_a_cut_off_answer() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let ask = |max_output_tokens| {
            client
                .interaction()
                .with_model(genai_rs::CONTINUATION_MODEL)
                .with_text("Write a sixteen-line poem about the sea.")
                .with_store_disabled()
                .with_generation_config(GenerationConfig {
                    max_output_tokens: Some(max_output_tokens),
                    ..Default::default()
                })
        };
        with_timeout(extended_test_timeout(), async {
            let first = ask(30).create().await.expect("first request");
            assert_eq!(first.status, InteractionStatus::Incomplete, "{first:?}");
            let token = first
                .continuation_token
                .clone()
                .expect("a cut-off answer carries a continuation token");

            // A budget the poem fits in with room for thinking, so the
            // continuation usually finishes.
            let rest = ask(2048)
                .with_continuation_token(&token)
                .create()
                .await
                .expect("the continuation was rejected");
            println!(
                "first: {:?}\nrest: {:?} ({:?}, {} steps)",
                first.all_text(),
                rest.all_text(),
                rest.status,
                rest.steps.len()
            );
            // The decode ran on: it finished with the answer's output, or
            // the cumulative budget cut it off again, with a token of its own
            // to go on from.
            match &rest.status {
                InteractionStatus::Completed => assert!(
                    rest.steps
                        .iter()
                        .any(|step| matches!(step, Step::ModelOutput { .. })),
                    "a finished continuation has a model_output step: {rest:?}"
                ),
                InteractionStatus::Incomplete => assert!(
                    rest.continuation_token.is_some(),
                    "a continuation cut off again carries a token: {rest:?}"
                ),
                other => panic!("the continuation ended {other:?}: {rest:?}"),
            }
        })
        .await;
    }

    /// A stored decode continues from its id and token alone, with no input
    /// (live 2026-11-05): `continue_decode` sends exactly that, with no
    /// `max_output_tokens`, and the decode runs on from where it stopped.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_continue_decode_without_input() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        with_timeout(extended_test_timeout(), async {
            let first = client
                .interaction()
                .with_model(genai_rs::CONTINUATION_MODEL)
                .with_text("Write three sentences about the moon.")
                .with_generation_config(GenerationConfig {
                    max_output_tokens: Some(20),
                    ..Default::default()
                })
                .create()
                .await
                .expect("first request");
            let first_id = first.id.clone().expect("a stored interaction has an id");
            assert_eq!(first.status, InteractionStatus::Incomplete);
            assert!(first.continuation_token.is_some(), "{first:?}");

            let request = client
                .interaction()
                .continue_decode(&first)
                .build()
                .expect("continuation request");
            assert!(request.input.is_none());
            let rest = client.execute(request).await;
            let _ = client.interactions().delete(&first_id).await;
            let rest = rest.expect("the continuation without input was rejected");
            if let Some(id) = rest.id.as_deref() {
                let _ = client.interactions().delete(id).await;
            }
            let first_text = first.all_text();
            let rest_text = rest.all_text();
            println!("first: {first_text:?}\nrest: {rest_text:?}");
            assert!(
                !rest_text.trim().is_empty(),
                "the continuation produced no text"
            );
            // A cut-off that spent its budget on thinking has no text to
            // compare with; every string starts with "".
            if !first_text.trim().is_empty() {
                assert!(
                    !rest_text.starts_with(first_text.trim()),
                    "the continuation restarted the answer instead of continuing it"
                );
            }
        })
        .await;
    }

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
// Unmodeled request fields (`add_extra`)
// =============================================================================

mod extra_fields {
    use super::*;

    /// An `extra` key reaches the server as a top-level field: the API
    /// rejects a name it doesn't know (live 2026-11-04).
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_extra_field_reaches_the_wire() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = interaction_builder(&client)
            .with_text("Reply with the single word ok.")
            .with_store_disabled()
            .add_extra("genai_rs_unmodeled_probe", 1)
            .create()
            .await;

        match result {
            Err(GenaiError::Api {
                status_code: 400,
                message,
                ..
            }) => assert!(
                message.contains("genai_rs_unmodeled_probe"),
                "the 400 should name the field: {message}"
            ),
            other => panic!("expected a 400 for an unknown field, got: {other:?}"),
        }
    }

    /// A key that collides with a modeled field overrides it: the body
    /// carries both, the `extra` one last, and the API keeps the last.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_extra_field_overrides_a_modeled_one() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = client
            .interaction()
            .with_model("nonexistent-model-12345")
            .add_extra("model", genai_rs::DEFAULT_MODEL)
            .with_text("Reply with the single word ok.")
            .with_store_disabled()
            .create()
            .await
            .expect("the extra `model` should replace the unknown one");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.as_text().is_some(), "{response:?}");
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
            .interactions()
            .get(response.id.as_ref().expect("id should exist"))
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
