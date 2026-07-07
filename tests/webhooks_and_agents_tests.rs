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
    Agent, DeepResearchConfig, EnvironmentSource, RemoteEnvironment, ResponseFormat,
    RetrievalConfig, SpeechConfig, Tool, VideoConfig, VideoTask, Visualization, Webhook,
    WebhookConfig, WebhookEvent, WebhookState, WebhookUpdate,
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
    assert_eq!(created.name.as_deref(), Some("genai-rs-integration-test"));
    assert_eq!(created.state, Some(WebhookState::Enabled));
    let id = created.id.clone().expect("created webhook has an id");

    // Get: must echo exactly what create sent (verified live 2026-07).
    let fetched = client.get_webhook(&id).await.expect("get_webhook");
    assert_eq!(fetched.uri, TEST_WEBHOOK_URI);
    assert_eq!(fetched.subscribed_events, created.subscribed_events);
    assert_eq!(fetched.name, created.name);
    assert!(
        fetched.new_signing_secret.is_none(),
        "the full secret is only returned on create"
    );

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

    // Update: disable it. Verified live (2026-07): update_mask is optional
    // and observed to be *ignored* by the API — the PATCH applies exactly
    // the fields present in the body. Passing None exercises the real
    // partial-update contract.
    let updated = client
        .update_webhook(
            &id,
            &WebhookUpdate::new().with_state(WebhookState::Disabled),
            None,
        )
        .await
        .expect("update_webhook");
    assert_eq!(updated.state, Some(WebhookState::Disabled));
    assert_eq!(
        updated.uri, TEST_WEBHOOK_URI,
        "unset fields must not change"
    );

    // Ping. Verified live (2026-07): the RPC accepts our empty `{}` body and
    // returns 200 + `{}` even though the destination URI is unreachable
    // (delivery failure is asynchronous, not surfaced by the RPC).
    client
        .ping_webhook(&id)
        .await
        .expect("ping_webhook should be accepted with an empty JSON body");

    // Rotate signing secret: must return a fresh full secret, distinct from
    // the one issued at create (verified live 2026-07).
    let rotated = client
        .rotate_webhook_signing_secret(&id, None)
        .await
        .expect("rotate_webhook_signing_secret");
    let rotated_secret = rotated.secret.expect("rotate returns the new secret");
    assert_ne!(
        Some(rotated_secret.as_str()),
        created.new_signing_secret.as_deref(),
        "rotated secret must differ from the create-time secret"
    );

    // After rotation the resource lists multiple signing secrets (the old
    // one gets a 24h expire_time by default).
    let after_rotate = client.get_webhook(&id).await.expect("get after rotate");
    assert!(
        after_rotate.signing_secrets.map_or(0, |s| s.len()) >= 2,
        "expected old + new signing secrets after rotation"
    );

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
    // Verified live (2026-07): the API rejects webhook_config unless
    // background=true is also set.
    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Say hello.")
        .with_background(true)
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
            // Verified live (2026-07): the create response echoes the
            // request's webhook_config verbatim.
            let echo = response
                .webhook_config
                .as_ref()
                .expect("create response should echo webhook_config");
            assert_eq!(
                echo.uris.as_deref(),
                Some(&[TEST_WEBHOOK_URI.to_string()][..]),
                "uris must be echoed verbatim"
            );
            assert_eq!(
                echo.user_metadata,
                Some(serde_json::json!({"test": "webhook_config"})),
                "user_metadata must be echoed verbatim"
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
    // Verified live (2026-07): the API validates the payload schema first
    // (snake_case fields; agent `tools` only accept `code_execution`,
    // `google_search`, `url_context`), but creation itself is rejected with
    // a generic 400 "Request contains an invalid argument." for every
    // schema-valid payload tried on a standard API key — the resource
    // appears gated/allowlisted. The tolerant early-return below covers
    // that case; the CRUD assertions run where creation is available.
    let agent = Agent::new(agent_id)
        .with_system_instruction("You are a test agent that answers briefly.")
        .with_description("Integration-test agent created by genai-rs")
        .add_tool(Tool::CodeExecution);

    let created = match client.create_agent(&agent).await {
        Ok(agent) => agent,
        Err(e) => {
            println!("Agent create not available for this account: {e}");
            let message = e.to_string();
            assert!(
                !message.contains("Unknown parameter"),
                "agent payload schema itself was rejected: {message}"
            );
            return;
        }
    };
    println!("Created agent: id={:?}", created.id);

    // Get: the tools subset must round-trip intact.
    let fetched = client.get_agent(agent_id).await.expect("get_agent");
    assert_eq!(fetched.id.as_deref(), Some(agent_id));
    assert!(
        matches!(fetched.tools.as_deref(), Some([Tool::CodeExecution])),
        "agent tools must round-trip: {:?}",
        fetched.tools
    );
    assert_eq!(
        fetched.system_instruction.as_deref(),
        Some("You are a test agent that answers briefly.")
    );

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
            // Verified live (2026-07): the create response carries the
            // provisioned environment's ID.
            assert!(
                response.environment_id.is_some(),
                "accepted environment request should return environment_id"
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

    // Verified live (2026-07): the Gemini API rejects `type: "retrieval"`
    // outright — "The value 'retrieval' is not supported for 'tools[0].type'
    // on the Gemini API, it is allowed on the Gemini Enterprise Agent
    // Platform." (i.e. the tool is Vertex-only). This test pins that the
    // rejection stays a *capability* error, not a schema-level
    // unknown-field error — if it starts succeeding, the tool has launched
    // on the Gemini API and this test should be upgraded.
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
        Ok(response) => println!(
            "Retrieval request accepted (tool now live on the Gemini API?): {:?}",
            response.status
        ),
        Err(e) => {
            let message = e.to_string();
            println!("Retrieval request returned error: {message}");
            assert!(
                !message.contains("Unknown parameter") && !message.contains("Unknown name"),
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

    // The output must actually conform to the schema we sent (deterministic
    // structural check, not LLM-content matching).
    let text = response.as_text().expect("structured output text");
    let data: serde_json::Value = serde_json::from_str(text).expect("valid JSON output");
    assert!(
        data["name"].is_string(),
        "schema requires string `name`: {data}"
    );
    assert!(
        data["age"].is_i64() || data["age"].is_u64(),
        "schema requires integer `age`: {data}"
    );
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
            // Verified live (2026-07): the list-form speech_config is
            // accepted and the API returns one combined `audio/l16` stream
            // covering both speakers (per-speaker audio is not split out).
            let audio = response.first_audio().expect("expected audio output");
            let bytes = audio.bytes().expect("audio data must be decodable");
            assert!(!bytes.is_empty(), "decoded audio must be non-empty");
            println!(
                "Multi-speaker TTS: mime_type={:?}, sample_rate={:?}, {} bytes",
                audio.mime_type(),
                audio.sample_rate(),
                bytes.len()
            );
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

    // Verified live (2026-07): Veo models are not served by the Interactions
    // API — `veo-3.1-generate-preview` returns 404 "Model ... not found"
    // (the `/v1beta/models` list exposes it with only the legacy
    // `predictLongRunning` method), and no Interactions-served model
    // supported the `video` response modality. The `video_config` schema
    // itself is validated server-side (its `task` enum lists
    // text_to_video/image_to_video/reference_to_video/edit/extend). If this
    // request starts succeeding, video generation has launched on the
    // Interactions API and this test should be upgraded.
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
        Err(e) => {
            let message = e.to_string();
            println!("Video generation not available for this account/model: {message}");
            assert!(
                !message.contains("Unknown parameter"),
                "video request schema itself was rejected: {message}"
            );
        }
    }
}

// =============================================================================
// Deep Research config knobs (request acceptance)
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_deep_research_config_knobs_accepted() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Verified live (2026-07): `agent_config.visualization` (enum "off" |
    // "auto", validated server-side) and `collaborative_planning` are
    // accepted on the Gemini API. `enable_bigquery_tool` is rejected as
    // Vertex-only ("not available on the Gemini API but it is available on
    // the Gemini Enterprise Agent Platform") and is deliberately not sent
    // here. Deep-research runs are long, so this only checks request
    // acceptance and then cancels the background interaction.
    let result = client
        .interaction()
        .with_agent("deep-research-preview-04-2026")
        .with_text("One-paragraph overview of Rust async runtimes")
        .with_background(true)
        .with_store_enabled()
        .with_agent_config(
            DeepResearchConfig::new()
                .with_visualization(Visualization::Auto)
                .with_collaborative_planning(true),
        )
        .create()
        .await;

    match result {
        Ok(response) => {
            println!(
                "Deep Research config accepted: status={:?}",
                response.status
            );
            if let Some(id) = &response.id {
                let _ = client.cancel_interaction(id).await;
            }
        }
        Err(e) => panic!("Deep Research config knobs rejected: {e}"),
    }
}
