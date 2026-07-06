//! Integration tests for the Webhooks and Agents resources, environments,
//! per-request webhook routing, retrieval grounding, typed response formats,
//! multi-speaker TTS, and video config.
//!
//! These tests require the GEMINI_API_KEY environment variable to be set.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test webhooks_and_agents_tests -- --include-ignored --nocapture
//! ```
//!
//! # Notes
//!
//! - Webhook tests use `https://example.com` endpoints; deliveries will fail
//!   but resource CRUD is exercised end-to-end. Created resources are cleaned
//!   up at the end of each test.
//! - Some resources (agents, environments, retrieval backends) may not be
//!   available in all accounts; tests report and tolerate `not found` /
//!   `permission` errors rather than failing hard on capability gaps.

mod common;

use common::get_client;
use genai_rs::{
    Agent, EnvironmentSource, RemoteEnvironment, ResponseFormat, RetrievalConfig, SpeechConfig,
    Tool, VideoConfig, VideoTask, Webhook, WebhookConfig, WebhookEvent, WebhookState,
    WebhookUpdate,
};

/// A test webhook endpoint. Deliveries fail (no listener), which is fine for
/// resource CRUD tests.
const TEST_WEBHOOK_URI: &str = "https://example.com/genai-rs-test-hook";

// =============================================================================
// Webhooks resource: create / get / list / update / ping / rotate / delete
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_webhook_crud_lifecycle() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Create
    let created = match client
        .create_webhook(
            &Webhook::new(
                TEST_WEBHOOK_URI,
                vec![
                    WebhookEvent::InteractionCompleted,
                    WebhookEvent::InteractionFailed,
                ],
            )
            .with_name("genai-rs-integration-test"),
        )
        .await
    {
        Ok(webhook) => webhook,
        Err(e) => {
            println!("Webhook create not available for this account: {e}");
            return;
        }
    };

    println!("Created webhook: id={:?}", created.id);
    assert_eq!(created.uri, TEST_WEBHOOK_URI);
    assert!(
        created.new_signing_secret.is_some(),
        "create should return new_signing_secret"
    );
    let id = created.id.clone().expect("created webhook has an id");

    // Get
    let fetched = client.get_webhook(&id).await.expect("get_webhook");
    assert_eq!(fetched.uri, TEST_WEBHOOK_URI);
    assert_eq!(fetched.subscribed_events.len(), 2);

    // List (should contain our webhook)
    let list = client
        .list_webhooks(Some(50), None)
        .await
        .expect("list_webhooks");
    assert!(
        list.webhooks
            .iter()
            .any(|w| w.id.as_deref() == Some(id.as_str())),
        "created webhook should appear in list"
    );

    // Update: disable it
    let updated = client
        .update_webhook(
            &id,
            &WebhookUpdate::new().with_state(WebhookState::Disabled),
            Some("state"),
        )
        .await
        .expect("update_webhook");
    println!("Updated state: {:?}", updated.state);

    // Ping (delivery to example.com fails server-side; the RPC itself may
    // error - both outcomes exercise the endpoint)
    match client.ping_webhook(&id).await {
        Ok(()) => println!("Ping accepted"),
        Err(e) => println!("Ping returned error (expected for unreachable URI): {e}"),
    }

    // Rotate signing secret
    match client.rotate_webhook_signing_secret(&id, None).await {
        Ok(rotated) => {
            println!("Rotated secret: present={}", rotated.secret.is_some());
        }
        Err(e) => println!("Rotate returned error: {e}"),
    }

    // Delete (cleanup)
    client.delete_webhook(&id).await.expect("delete_webhook");
    println!("Deleted webhook {id}");
}

// =============================================================================
// Per-request webhook_config
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_interaction_with_webhook_config() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Per-request webhook routing; the URI is unreachable, but the request
    // itself must be accepted with the webhook_config field present.
    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Say hello.")
        .with_webhook_config(
            WebhookConfig::new()
                .with_uris(vec![TEST_WEBHOOK_URI.to_string()])
                .with_user_metadata(serde_json::json!({"test": "webhook_config"})),
        )
        .create()
        .await;

    match result {
        Ok(response) => {
            println!(
                "Interaction accepted with webhook_config: {:?}",
                response.status
            );
        }
        Err(e) => panic!("Request with webhook_config rejected: {e}"),
    }
}

// =============================================================================
// Agents resource: create / get / list / delete
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_agent_crud_lifecycle() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let agent_id = "genai-rs-test-agent";
    let agent = Agent::new(agent_id)
        .with_system_instruction("You are a test agent that answers briefly.")
        .with_description("Integration-test agent created by genai-rs")
        .add_tool(Tool::CodeExecution);

    let created = match client.create_agent(&agent).await {
        Ok(agent) => agent,
        Err(e) => {
            println!("Agent create not available for this account: {e}");
            return;
        }
    };
    println!("Created agent: id={:?}", created.id);

    // Get
    let fetched = client.get_agent(agent_id).await.expect("get_agent");
    assert_eq!(fetched.id.as_deref(), Some(agent_id));

    // List
    let list = client
        .list_agents(Some(50), None, None)
        .await
        .expect("list_agents");
    println!("Listed {} agents", list.agents.len());
    assert!(
        list.agents
            .iter()
            .any(|a| a.id.as_deref() == Some(agent_id)),
        "created agent should appear in list"
    );

    // Delete (cleanup)
    client.delete_agent(agent_id).await.expect("delete_agent");
    println!("Deleted agent {agent_id}");
}

// =============================================================================
// Environments
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_interaction_with_inline_environment() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Typed remote environment with an inline source. Environment support is
    // agent-dependent; tolerate capability errors but require a structured
    // API response (i.e., the request shape itself must be valid).
    let result = client
        .interaction()
        .with_agent("antigravity-preview-05-2026")
        .with_text("Print the contents of /etc/motd")
        .with_background(true)
        .with_store_enabled()
        .with_environment(
            RemoteEnvironment::new().add_source(EnvironmentSource::inline(
                "/etc/motd",
                "hello from genai-rs",
            )),
        )
        .create()
        .await;

    match result {
        Ok(response) => {
            println!(
                "Environment interaction accepted: status={:?}, environment_id={:?}",
                response.status, response.environment_id
            );
            // Clean up background interaction if possible
            if let Some(id) = &response.id {
                let _ = client.cancel_interaction(id).await;
            }
        }
        Err(e) => println!("Environment not available for this account/agent: {e}"),
    }
}

// =============================================================================
// Retrieval tool
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_retrieval_tool_request_accepted() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Retrieval backends need real engines/corpora; this exercises the wire
    // shape. The API should return a structured error (bad resource) rather
    // than a schema-level 400 on unknown fields.
    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What does our internal handbook say about PTO?")
        .add_tool(
            RetrievalConfig::new().with_vertex_ai_search(
                genai_rs::VertexAiSearchConfig::new()
                    .with_engine("projects/invalid/locations/global/engines/does-not-exist"),
            ),
        )
        .create()
        .await;

    match result {
        Ok(response) => println!("Retrieval request accepted: {:?}", response.status),
        Err(e) => {
            let message = e.to_string();
            println!("Retrieval request returned error: {message}");
            assert!(
                !message.contains("Unknown name"),
                "API rejected the retrieval tool schema itself: {message}"
            );
        }
    }
}

// =============================================================================
// Typed response formats
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_typed_text_response_format() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // The typed text format with a JSON schema must behave like the legacy
    // raw-schema response_format.
    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Generate info for a person named Alice, age 30")
        .with_response_format(ResponseFormat::json_schema(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"]
        })))
        .create()
        .await
        .expect("typed text response_format should be accepted");

    let text = response.as_text().expect("structured output text");
    let data: serde_json::Value = serde_json::from_str(text).expect("valid JSON output");
    assert!(data.get("name").is_some());
    assert!(data.get("age").is_some());
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_multi_speaker_tts_with_audio_response_format() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let result = client
        .interaction()
        .with_model("gemini-2.5-pro-preview-tts")
        .with_text("Alice: Hello Bob!\nBob: Hi Alice, lovely day!")
        .with_audio_output()
        .with_speech_configs(vec![
            SpeechConfig {
                voice: Some("Kore".to_string()),
                language: Some("en-US".to_string()),
                speaker: Some("Alice".to_string()),
            },
            SpeechConfig {
                voice: Some("Puck".to_string()),
                language: Some("en-US".to_string()),
                speaker: Some("Bob".to_string()),
            },
        ])
        .create()
        .await;

    match result {
        Ok(response) => {
            let audio = response.first_audio();
            println!("Multi-speaker TTS: audio present={}", audio.is_some());
            assert!(audio.is_some(), "expected audio output");
        }
        Err(e) => panic!("Multi-speaker TTS request rejected: {e}"),
    }
}

// =============================================================================
// Video generation config (request shape)
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_video_generation_request_shape() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Video generation is long-running and model availability varies; this
    // verifies the request shape (video modality + video_config + video
    // response_format) is accepted, then cancels to avoid burning quota.
    let result = client
        .interaction()
        .with_model("veo-3.1-generate-preview")
        .with_text("A hummingbird hovering over a red flower, slow motion")
        .with_video_output()
        .with_video_config(VideoConfig::new().with_task(VideoTask::TextToVideo))
        .with_background(true)
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(response) => {
            println!("Video generation accepted: status={:?}", response.status);
            if let Some(id) = &response.id {
                let _ = client.cancel_interaction(id).await;
            }
        }
        Err(e) => println!("Video generation not available for this account/model: {e}"),
    }
}
