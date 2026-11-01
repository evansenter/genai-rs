//! Webhooks, agents, environments and triggers resources; per-request webhook
//! routing; typed response formats; multi-speaker TTS; and the request
//! parameters the Gemini API gates to Vertex.
//!
//! Gated capabilities are pinned by their specific rejection, so a schema
//! regression or an unrelated failure cannot read as "not available".
//!
//! ```bash
//! cargo nextest run --test webhooks_and_agents_tests --run-ignored all
//! ```

mod common;

use common::{TINY_WAV_BASE64, get_client};
use futures_util::{StreamExt, TryStreamExt};
use genai_rs::{
    Agent, Content, DeepResearchConfig, InteractionInput, ResponseFormat, RetrievalConfig,
    SpeechConfig, Tool, Visualization, Webhook, WebhookConfig, WebhookEvent, WebhookState,
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

    // Not retried: a retry after a lost response would leak a webhook with no
    // id to delete it by.
    let created = client
        .webhooks()
        .create(
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
        .expect("webhooks.create");
    let id = created.id.clone().expect("created webhook has an id");

    // Run the checks under catch_unwind so a failed assertion still deletes
    // the webhook.
    let checks = async {
        assert_eq!(created.uri, TEST_WEBHOOK_URI);
        assert!(
            created.new_signing_secret.is_some(),
            "create should return new_signing_secret"
        );
        assert_eq!(created.name.as_deref(), Some("genai-rs-integration-test"));
        assert_eq!(created.state, Some(WebhookState::Enabled));

        // Get echoes what create sent; the full secret is create-only.
        let fetched = client.webhooks().get(&id).await.expect("webhooks.get");
        assert_eq!(fetched.uri, TEST_WEBHOOK_URI);
        assert_eq!(fetched.subscribed_events, created.subscribed_events);
        assert_eq!(fetched.name, created.name);
        assert!(fetched.new_signing_secret.is_none());

        let list = client
            .webhooks()
            .list()
            .with_page_size(50)
            .send()
            .await
            .expect("webhooks.list");
        assert!(
            list.webhooks
                .iter()
                .any(|w| w.id.as_deref() == Some(id.as_str())),
            "created webhook should appear in list"
        );

        // The PATCH applies exactly the fields in the body; update_mask is
        // optional and observed to be ignored (verified live 2026-07).
        let updated = client
            .webhooks()
            .update(
                &id,
                &WebhookUpdate::new().with_state(WebhookState::Disabled),
            )
            .await
            .expect("webhooks.update");
        assert_eq!(updated.state, Some(WebhookState::Disabled));
        assert_eq!(
            updated.uri, TEST_WEBHOOK_URI,
            "unset fields must not change"
        );

        // Accepted even though the URI is unreachable: delivery fails later.
        client
            .webhooks()
            .ping(&id)
            .await
            .expect("webhooks.ping should be accepted with an empty JSON body");

        let rotated = client
            .webhooks()
            .rotate_signing_secret(&id, None)
            .await
            .expect("webhooks.rotate_signing_secret");
        let rotated_secret = rotated.secret.expect("rotate returns the new secret");
        assert_ne!(
            Some(rotated_secret.as_str()),
            created.new_signing_secret.as_deref(),
            "rotated secret must differ from the create-time secret"
        );

        // The old secret stays listed with a 24h expiry.
        let after_rotate = client.webhooks().get(&id).await.expect("get after rotate");
        assert!(
            after_rotate.signing_secrets.map_or(0, |s| s.len()) >= 2,
            "expected old + new signing secrets after rotation"
        );
    };
    let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(checks)).await;

    let deleted = client.webhooks().delete(&id).await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    deleted.expect("webhooks.delete");
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_webhook_list_streams_follow_every_page() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Two webhooks at one per page force at least one page-token round
    // trip. Live tokens are raw bytes with control characters (observed
    // 2026-09-26), so this also checks they survive being resent.
    let mut ids = Vec::new();
    for n in 1..=2 {
        // Not retried: a retry after a lost response would leak a webhook
        // with no id to delete it by.
        let created = client
            .webhooks()
            .create(
                &Webhook::new(TEST_WEBHOOK_URI, vec![WebhookEvent::InteractionCompleted])
                    .with_name(format!("genai-rs-paging-test-{n}")),
            )
            .await;
        match created.map(|w| w.id) {
            Ok(Some(id)) => ids.push(id),
            outcome => {
                for id in &ids {
                    let _ = client.webhooks().delete(id).await;
                }
                panic!("webhooks.create: {outcome:?}");
            }
        }
    }

    let checks = async {
        // Bounded, in case other webhooks exist on this key.
        let pages: Vec<_> = client
            .webhooks()
            .list()
            .with_page_size(1)
            .pages()
            .take(50)
            .try_collect()
            .await
            .expect("webhooks.list pages");
        println!("Listed {} page(s)", pages.len());
        assert!(pages.len() >= 2, "expected several pages, got {pages:?}");
        assert!(
            pages.iter().all(|p| p.webhooks.len() <= 1),
            "page_size=1 must hold on every page"
        );
        let paged: Vec<&str> = pages
            .iter()
            .flat_map(|p| &p.webhooks)
            .filter_map(|w| w.id.as_deref())
            .collect();
        for id in &ids {
            assert!(paged.contains(&id.as_str()), "{id} missing from {paged:?}");
        }

        let items: Vec<_> = client
            .webhooks()
            .list()
            .with_page_size(1)
            .items()
            .take(50)
            .try_collect()
            .await
            .expect("webhooks.list items");
        // Not compared to `paged`: the lifecycle test may add or remove its
        // webhook between the two listings.
        let streamed: Vec<&str> = items.iter().filter_map(|w| w.id.as_deref()).collect();
        for id in &ids {
            assert!(
                streamed.contains(&id.as_str()),
                "{id} missing from {streamed:?}"
            );
        }
    };
    let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(checks)).await;

    let mut deleted = Vec::new();
    for id in &ids {
        deleted.push(client.webhooks().delete(id).await);
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    for result in deleted {
        result.expect("webhooks.delete");
    }
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
        .with_model(genai_rs::DEFAULT_MODEL)
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

    // Unique per run: an existing id is a 409 (verified 2026-10-30), so a
    // fixed id would fail after a run that died before cleanup, or alongside
    // a concurrent CI run on the same key.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .subsec_nanos();
    let agent_id = format!("genai-rs-test-agent-{nanos}");
    let instruction = "You are a test agent that answers briefly.";
    let description = "Integration-test agent created by genai-rs";
    // Verified live 2026-10-30: `base_agent` is required and only the
    // Antigravity agent is accepted. Through 2026-10-08 every create on a
    // standard key was refused with a generic 400; it now succeeds.
    let agent = Agent::new(&agent_id, genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
        .with_system_instruction(instruction)
        .with_description(description)
        .add_tool(Tool::CodeExecution);

    // Not retried: a retry after a lost response would 409 on the id.
    let created = client.agents().create(&agent).await.expect("agents.create");
    println!("Created agent: id={:?}", created.id);

    let checks = async {
        // Create echoes the agent.
        assert_eq!(created.id.as_deref(), Some(agent_id.as_str()));
        assert_eq!(
            created.base_agent.as_deref(),
            Some(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
        );

        // Get: the tools subset must round-trip intact.
        let fetched = client.agents().get(&agent_id).await.expect("agents.get");
        assert_eq!(fetched.id.as_deref(), Some(agent_id.as_str()));
        assert_eq!(
            fetched.base_agent.as_deref(),
            Some(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
        );
        assert!(
            matches!(fetched.tools.as_deref(), Some([Tool::CodeExecution])),
            "agent tools must round-trip: {:?}",
            fetched.tools
        );
        assert_eq!(fetched.system_instruction.as_deref(), Some(instruction));
        assert_eq!(fetched.description.as_deref(), Some(description));

        // Every page: agents leaked by runs that died before cleanup can
        // push this one past the first.
        let listed: Vec<Agent> = client
            .agents()
            .list()
            .with_page_size(50)
            .items()
            .try_collect()
            .await
            .expect("agents.list");
        println!("Listed {} agents", listed.len());
        assert!(
            listed
                .iter()
                .any(|a| a.id.as_deref() == Some(agent_id.as_str())),
            "created agent should appear in list"
        );
    };
    let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(checks)).await;

    let deleted = client.agents().delete(&agent_id).await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    deleted.expect("agents.delete");

    // Deleted means gone.
    match client.agents().get(&agent_id).await {
        Err(genai_rs::GenaiError::Api {
            status_code: 404, ..
        }) => {}
        other => panic!("expected 404 after delete, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_agent_list_streams_end_on_the_last_page() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // The test key usually holds no agents (`{"agents": []}`, verified
    // 2026-10-30), so this pins the end-of-list rule on a real response.
    // Following several pages is covered offline in http_mock_resources.rs.
    let pages: Vec<_> = client
        .agents()
        .list()
        .with_page_size(2)
        .pages()
        .try_collect()
        .await
        .expect("agents.list pages");
    println!(
        "Listed {} page(s), {} agent(s)",
        pages.len(),
        pages.iter().map(|p| p.agents.len()).sum::<usize>()
    );
    let last = pages.last().expect("at least one page");
    assert!(
        last.next_page_token.as_deref().is_none_or(str::is_empty),
        "the stream must end on a page without a token: {:?}",
        last.next_page_token
    );

    let agents: Vec<_> = client
        .agents()
        .list()
        .with_page_size(2)
        .items()
        .try_collect()
        .await
        .expect("agents.list items");
    println!("Streamed {} agent(s)", agents.len());
}

// =============================================================================
// Retrieval tool
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_retrieval_tool_vertex_gated() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Vertex-only (verified live 2026-07): a 400 carrying the stable
    // "Gemini Enterprise" fragment of the gate message. Acceptance means the
    // tool launched and this should be upgraded; any other error is a
    // regression.
    let result = retry_request!([client] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("What does our internal handbook say about PTO?")
            .add_tool(
                RetrievalConfig::new().with_vertex_ai_search(
                    genai_rs::VertexAiSearchConfig::new()
                        .with_engine("projects/invalid/locations/global/engines/does-not-exist"),
                ),
            )
            .create()
            .await
    });

    match result {
        Ok(response) => panic!(
            "retrieval was accepted (status={:?}); it has launched on the Gemini API — \
             upgrade this probe to assert acceptance",
            response.status
        ),
        Err(genai_rs::GenaiError::Api {
            status_code: 400,
            message,
            ..
        }) if message.contains("Gemini Enterprise") => {
            println!("retrieval Vertex-gated as expected: {message}");
        }
        Err(e) => panic!("expected the Vertex-gate 400 for retrieval, got: {e}"),
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
        .with_model(genai_rs::DEFAULT_MODEL)
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

    // On gemini-3.8-flash-tts each turn names its speaker with a
    // `speech_metadata` annotation; the older `Alice: ...` transcript form is
    // rejected ("must specify a speaker for each text turn", 2026-09-24).
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_TTS_MODEL)
        .with_input(InteractionInput::Content(vec![
            Content::speaker_text("Alice", "Hello Bob!"),
            Content::speaker_text("Bob", "Hi Alice, lovely day!"),
        ]))
        .with_audio_output()
        .with_speech_configs(vec![
            SpeechConfig::for_speaker("Alice", "Kore", "en-US"),
            SpeechConfig::for_speaker("Bob", "Puck", "en-US"),
        ])
        .with_response_format(ResponseFormat::Audio {
            mime_type: None,
            delivery: None,
            sample_rate: Some(24_000),
            bit_rate: None,
        })
        .with_store_disabled()
        .create()
        .await
        .expect("multi-speaker TTS request failed");

    // One combined stream covering both speakers.
    let audio = response.first_audio().expect("expected audio output");
    let bytes = audio.bytes().expect("audio data must be decodable");
    assert!(bytes.starts_with(b"RIFF"), "expected a WAV container");
    assert_eq!(audio.extension(), "wav");
}

// =============================================================================
// TranscriptionConfig + Vertex-gated request params (request acceptance)
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_transcription_config_accepted() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    use genai_rs::TranscriptionConfig;

    // Verified live (2026-08-08): `generation_config.transcription_config`
    // is accepted by the Gemini API (200) — so assert the strong form.
    // A server-side rename, field rejection, *or* a Vertex-gating of the
    // parameter all fail this outright instead of shipping silently;
    // transients are absorbed by the retry.
    let response = crate::retry_request!([client] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_content(vec![
                genai_rs::Content::text(
                    "Transcribe this short clip. If it is silent, say 'Silent audio.'",
                ),
                genai_rs::Content::audio_data(TINY_WAV_BASE64, "audio/wav"),
            ])
            .with_transcription_config(
                TranscriptionConfig::new()
                    .with_language_codes(["en-US"])
                    .with_diarization_mode("speaker")
                    .with_timestamp_granularities(["word"]),
            )
            .create()
            .await
    })
    .expect("transcription_config should be accepted (verified live 2026-08-08)");
    println!(
        "transcription_config accepted: status={:?}",
        response.status
    );
}

/// `safety_settings` is accepted on the Gemini API (verified live 2026-10-30;
/// the parameter was Vertex-only through 2026-10-08). The item's category key
/// is `type`. The create response drops `method`, so the stored record is
/// read back with a GET.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_safety_settings_accepted_and_stored() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    use genai_rs::{HarmCategory, SafetyMethod, SafetySetting, SafetyThreshold};

    let setting = SafetySetting::new(HarmCategory::Harassment, SafetyThreshold::BlockOnlyHigh)
        .with_method(SafetyMethod::Severity);
    let response = crate::retry_request!([client, setting] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("Say OK.")
            .add_safety_setting(setting)
            .create()
            .await
    })
    .expect("safety_settings should be accepted (verified live 2026-10-30)");

    let id = response.id.expect("stored interaction id");
    // Delete before any assertion, so a failure doesn't leak it.
    let stored = client.interactions().get(&id).await;
    let _ = client.interactions().delete(&id).await;
    assert_eq!(response.status, genai_rs::InteractionStatus::Completed);
    let stored = stored.expect("get_interaction");
    // Parsing the echo, rather than comparing raw JSON, also exercises the
    // `type` key on the deserialize side.
    let echoed: Vec<SafetySetting> = serde_json::from_value(
        stored
            .extra
            .get("safety_settings")
            .cloned()
            .expect("safety_settings were not stored"),
    )
    .expect("stored safety_settings parse as SafetySetting");
    assert_eq!(echoed, vec![setting]);
}

/// `civic_integrity`, the `image_*` categories and `block_none` are
/// Vertex-only on the Gemini API (verified live 2026-10-30). The 400 must
/// carry the gate's stable "Gemini Enterprise" fragment, so a schema
/// rejection (such as an unknown item key), a mis-scoped key or a transport
/// failure cannot pass.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_safety_settings_vertex_only_value_gated() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    use genai_rs::{HarmCategory, SafetySetting, SafetyThreshold};

    let result = crate::retry_request!([client] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("Say OK.")
            .add_safety_setting(SafetySetting::new(
                HarmCategory::CivicIntegrity,
                SafetyThreshold::BlockOnlyHigh,
            ))
            .create()
            .await
    });

    match result {
        Ok(response) => {
            if let Some(id) = response.id.as_deref() {
                let _ = client.interactions().delete(id).await;
            }
            panic!(
                "civic_integrity was accepted (status={:?}) — update the Vertex-only \
                 notes in src/safety.rs and docs/ENUM_WIRE_FORMATS.md",
                response.status
            )
        }
        Err(genai_rs::GenaiError::Api {
            status_code: 400,
            message,
            ..
        }) if message.contains("Gemini Enterprise") => {
            println!("civic_integrity Vertex-gated as expected: {message}");
        }
        Err(e) => panic!("expected the documented Vertex-gate 400, got: {e}"),
    }
}

/// `labels` was Vertex-only until at least 2026-08-08; as of 2026-09-24 the
/// Gemini API accepts and stores them. The create response stopped echoing
/// them by 2026-10-01, so the stored record is read back with a GET.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_labels_accepted_and_stored() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let response = crate::retry_request!([client] => {
        client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text("Say OK.")
            .add_label("team", "genai-rs-ci")
            .create()
            .await
    })
    .expect("labels should be accepted (verified live 2026-09-24)");

    assert_eq!(response.status, genai_rs::InteractionStatus::Completed);
    let id = response.id.expect("stored interaction id");
    let stored = client
        .interactions()
        .get(&id)
        .await
        .expect("get_interaction");
    let _ = client.interactions().delete(&id).await;
    let labels = stored.labels.expect("labels were not stored");
    assert_eq!(labels.get("team").map(String::as_str), Some("genai-rs-ci"));
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_config_accepted() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    use genai_rs::{AntigravityConfig, EnvironmentSource, RemoteEnvironment};

    // Verified live (2026-10-01): `agent_config: {"type": "antigravity"}`
    // plus `max_total_tokens` and `model` is accepted on
    // `antigravity-preview-09-2026` (which requires an environment).
    // `DEFAULT_MODEL` (also the agent's documented default) is sent, so a
    // `model` the agent stops offering (404) fails the test. Retry transients,
    // assert the strong form, and cancel the background interaction. The retry is
    // deliberate despite the non-idempotent create: a retry after a lost
    // response can orphan an agent run plus its environment with no ID to
    // clean up, but max_total_tokens caps the orphan's cost — unlike the
    // trigger probe, whose orphan would fire on a schedule forever, so it
    // declines the retry. (The orphaned environment does not expire on its
    // own: environments from 2026-07 were still active on 2026-11-01.)
    let response = crate::retry_request!([client] => {
        client
            .interaction()
            .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
            .with_text("Print the contents of /etc/motd")
            .with_background(true)
            .with_store_enabled()
            .with_environment(
                RemoteEnvironment::new()
                    .add_source(EnvironmentSource::inline("/etc/motd", "config probe")),
            )
            .with_agent_config(
                AntigravityConfig::new()
                    .with_model(genai_rs::DEFAULT_MODEL)
                    .with_max_total_tokens(200_000),
            )
            .create()
            .await
    })
    .expect("AntigravityConfig should be accepted (verified live 2026-10-01)");
    println!("AntigravityConfig accepted: status={:?}", response.status);
    if let Some(id) = &response.id {
        // Print both arms: a failed cancel leaves a background agent
        // running against the account's budget — the larger of this
        // test's two possible leaks, so it must not be silent.
        match client.interactions().cancel(id).await {
            Ok(_) => println!("Cancelled interaction {id}"),
            Err(e) => println!("interactions().cancel({id}) failed (tolerated): {e}"),
        }
    }
    if let Some(env_id) = &response.environment_id {
        let bare_id = env_id.strip_prefix("environments/").unwrap_or(env_id);
        // Print both arms like the inline-environment probe: the
        // response-side environment_id form is unobserved, and a 404 here
        // is exactly the signal that the prefix assumption is wrong.
        match client.environments().delete(bare_id).await {
            Ok(()) => println!("Deleted environment {bare_id}"),
            Err(e) => println!("environments.delete({bare_id}) failed (tolerated): {e}"),
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
    // "auto", validated server-side), `collaborative_planning` and
    // `thinking_summaries` are accepted on the Gemini API.
    // `enable_bigquery_tool` is rejected as
    // Vertex-only ("not available on the Gemini API but it is available on
    // the Gemini Enterprise Agent Platform") and is deliberately not sent
    // here. Deep-research runs are long, so this only checks request
    // acceptance and then cancels the background interaction.
    // Retry-wrapped like the antigravity probe next door: a retried create
    // can at worst orphan a bounded background interaction (cancel-on-success
    // covers the common case), unlike the trigger probe's scheduled resource.
    let result = crate::retry_request!([client] => {
        client
            .interaction()
            .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
            .with_text("One-paragraph overview of Rust async runtimes")
            .with_background(true)
            .with_store_enabled()
            .with_agent_config(
                DeepResearchConfig::new()
                    .with_thinking_summaries(genai_rs::ThinkingSummaries::Auto)
                    .with_visualization(Visualization::Auto)
                    .with_collaborative_planning(true),
            )
            .create()
            .await
    });

    match result {
        Ok(response) => {
            println!(
                "Deep Research config accepted: status={:?}",
                response.status
            );
            if let Some(id) = &response.id {
                let _ = client.interactions().cancel(id).await;
            }
        }
        Err(e) => panic!("Deep Research config knobs rejected: {e}"),
    }
}

// =============================================================================
// Environments resource: create / get / list / delete
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_environment_crud_lifecycle() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    use genai_rs::{CreateEnvironmentRequest, EnvironmentSource};

    // Create: one inline file source.
    let request = CreateEnvironmentRequest::new().add_source(EnvironmentSource::inline(
        "/etc/motd",
        "hello from genai-rs environments CRUD",
    ));
    // Create is a non-idempotent POST, so a retry after a lost response can
    // orphan a first container — tolerated here (unlike the trigger test's
    // un-retried create) because an idle container costs nothing, while an
    // un-retried create would flake the whole lifecycle. Environments do not
    // expire on their own (2026-07 ones were still active on 2026-11-01).
    let created = crate::retry_request!([client, request] => {
        client.environments().create(&request).await
    })
    .expect("environments.create");
    println!(
        "Created environment: id={:?} status={:?}",
        created.id, created.status
    );
    let created_id = created.id.clone().unwrap_or_else(|| {
        // No ID means no handle to delete by — the container is leaked.
        // Name it loudly like the trigger and example
        // siblings instead of a bare expect.
        panic!(
            "environments.create returned no ID (protocol violation) — the container is \
             leaked; hunt it via environments.list"
        )
    });

    // Run the read assertions in a closure so the delete below also runs
    // on the failure path — a tripped assertion must not leak the
    // environment server-side (environments don't expire on their own, so
    // repeated CI failures accumulate containers). Reads retry transients like
    // the neighbouring CRUD tests.
    let checks = async {
        // Get: counts arrive as protobuf-JSON strings and must parse.
        let fetched = crate::retry_request!([client, created_id] => {
            client.environments().get(&created_id).await
        })
        .expect("environments.get");
        assert_eq!(fetched.id.as_deref(), Some(created_id.as_str()));
        assert!(
            fetched.file_count.is_some(),
            "file_count should deserialize from the string wire form: {fetched:?}"
        );

        // List: the newest environment comes first (live 2026-09-26), so the
        // first page is enough.
        let listed = crate::retry_request!([client] => {
            client.environments().list().with_page_size(50).send().await
        })
        .expect("environments.list");
        assert!(
            listed
                .environments
                .iter()
                .any(|e| e.id.as_deref() == Some(created_id.as_str())),
            "created environment missing from list"
        );

        // Paging: at one per page the stream still reaches it. Live pages
        // can be empty mid-list while a token remains (2026-09-26), which
        // the stream must follow rather than stop on. Bounded: the key can
        // hold many environments, and other tests create them concurrently.
        let pages: Vec<_> = crate::retry_request!([client] => {
            client
                .environments()
                .list()
                .with_page_size(1)
                .pages()
                .take(25)
                .try_collect::<Vec<_>>()
                .await
        })
        .expect("environments.list pages");
        println!(
            "Listed {} page(s) at page_size=1; page sizes {:?}",
            pages.len(),
            pages
                .iter()
                .map(|p| p.environments.len())
                .collect::<Vec<_>>()
        );
        assert!(pages.len() >= 2, "expected several pages, got {pages:?}");
        assert!(
            pages.iter().all(|p| p.environments.len() <= 1),
            "page_size=1 must hold on every page"
        );
        assert!(
            pages
                .iter()
                .flat_map(|p| &p.environments)
                .any(|e| e.id.as_deref() == Some(created_id.as_str())),
            "created environment missing from the first {} pages",
            pages.len()
        );
    };
    let outcome = std::panic::AssertUnwindSafe(checks);
    let outcome = futures_util::FutureExt::catch_unwind(outcome).await;

    // Delete runs regardless of assertion outcome (retrying transients so
    // a blip doesn't leak the container), but a tripped read assertion is
    // the diagnosis this test exists to produce — re-raise it before
    // judging the delete, so a double failure reports the real one.
    let deleted = crate::retry_request!([client, created_id] => {
        client.environments().delete(&created_id).await
    });
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    deleted.expect("environments.delete");
    // Confirm gone with the same rigor as the trigger probe below: pin the
    // positive form — a deleted environment gets a 404 (verified live
    // 2026-08-09). A broad 4xx would also admit outcomes that say nothing
    // about the delete (an exhausted-retry 429, a mis-scoped-key 403).
    // Retry transients first so a 503 becomes a real answer rather than a
    // panic.
    let gone = crate::retry_request!([client, created_id] => {
        client.environments().get(&created_id).await
    });
    match gone {
        Err(genai_rs::GenaiError::Api {
            status_code: 404, ..
        }) => {}
        Ok(env) => panic!("environment should be gone after delete, got: {env:?}"),
        Err(e) => panic!("expected a 404 for the deleted environment, got: {e}"),
    }
}

// =============================================================================
// Triggers resource: the full lifecycle, against a custom agent
// =============================================================================

/// Create, get, list, update, run, list executions and delete, verified live
/// 2026-11-01. A trigger needs a custom agent, which a standard key could not
/// create until 2026-10-30, so this was a gated-create probe before then.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_trigger_lifecycle() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    use genai_rs::{
        InteractionRequest, TriggerCreateParams, TriggerExecutionStatus, TriggerStatus,
        TriggerUpdate,
    };

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .subsec_nanos();
    let agent_id = format!("genai-rs-test-trigger-agent-{nanos}");
    let display_name = format!("genai-rs trigger test {nanos}");
    // Not retried: a retry after a lost response would 409 on the id.
    client
        .agents()
        .create(&Agent::new(&agent_id, genai_rs::DEFAULT_ANTIGRAVITY_AGENT))
        .await
        .expect("agents.create");

    let interaction = || InteractionRequest {
        agent: Some(agent_id.clone()),
        input: InteractionInput::Text("Say OK.".to_string()),
        ..Default::default()
    };
    // Once a year, so the schedule never fires during a run.
    let params = TriggerCreateParams::new("0 5 1 1 *", "UTC", interaction())
        .with_display_name(&display_name)
        .with_max_consecutive_failures(3)
        .with_execution_timeout_seconds(300);
    // Deliberately NOT retry-wrapped: create is a non-idempotent POST, and
    // a retry after a lost response could leave a second, scheduled trigger
    // behind with no ID to clean up.
    let created = match client.triggers().create(&params).await {
        Ok(created) => created,
        Err(e) => {
            let _ = client.agents().delete(&agent_id).await;
            panic!("triggers.create: {e}");
        }
    };
    let Some(trigger_id) = created.id.clone() else {
        let _ = client.agents().delete(&agent_id).await;
        panic!(
            "trigger created without an id: a trigger named {display_name:?} is \
             leaked; hunt it down via triggers().list()"
        );
    };
    println!("Created trigger {trigger_id} on agent {agent_id}");

    // The firing's interaction, for cleanup even if a check panics.
    let fired = std::sync::Mutex::new(None::<String>);
    let checks = async {
        // Create echoes the definition, with the server's defaults filled in.
        assert_eq!(created.schedule.as_deref(), Some("0 5 1 1 *"));
        assert_eq!(created.time_zone.as_deref(), Some("UTC"));
        assert_eq!(created.display_name.as_deref(), Some(display_name.as_str()));
        assert_eq!(created.status, Some(TriggerStatus::Active));
        assert_eq!(created.max_consecutive_failures, Some(3));
        assert_eq!(created.execution_timeout_seconds, Some(300));
        assert_eq!(created.consecutive_failure_count, Some(0));
        assert!(created.create_time.is_some() && created.next_run_time.is_some());
        let echoed = created.interaction.as_ref().expect("interaction echo");
        assert_eq!(echoed.agent.as_deref(), Some(agent_id.as_str()));
        // The echo carries `store: true`, which create rejects; Trigger's
        // docs tell callers to clear it before reuse.
        assert_eq!(echoed.store, Some(true));
        assert!(created.extra.is_empty(), "unmodeled: {:?}", created.extra);

        let fetched = client.triggers().get(&trigger_id).await.expect("get");
        assert_eq!(fetched.id.as_deref(), Some(trigger_id.as_str()));
        assert_eq!(fetched.display_name, created.display_name);

        // Every page: triggers leaked by runs that died before cleanup can
        // push this one past the first.
        let listed: Vec<genai_rs::Trigger> = client
            .triggers()
            .list()
            .with_page_size(50)
            .items()
            .try_collect()
            .await
            .expect("list");
        assert!(
            listed
                .iter()
                .any(|t| t.id.as_deref() == Some(trigger_id.as_str())),
            "created trigger should appear in list"
        );

        // PATCH applies only the fields in the body: pausing keeps the name,
        // and renaming keeps the pause.
        let paused = client
            .triggers()
            .update(
                &trigger_id,
                &TriggerUpdate::new().with_status(TriggerStatus::Paused),
            )
            .await
            .expect("pause");
        assert_eq!(paused.status, Some(TriggerStatus::Paused));
        assert!(paused.last_pause_time.is_some());
        assert_eq!(paused.display_name, created.display_name);
        let renamed_to = format!("{display_name} (renamed)");
        let renamed = client
            .triggers()
            .update(
                &trigger_id,
                &TriggerUpdate::new().with_display_name(&renamed_to),
            )
            .await
            .expect("rename");
        assert_eq!(renamed.display_name.as_deref(), Some(renamed_to.as_str()));
        assert_eq!(renamed.status, Some(TriggerStatus::Paused));
        // Run now, while paused: a manual run doesn't need an active
        // schedule. The API answers `{}` (live 2026-11-01), so the firing is
        // found through the executions listing.
        let run = client.triggers().run(&trigger_id).await.expect("run");
        println!("run returned: {run:?}");
        let mut execution = None;
        for _ in 0..36 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let executions: Vec<genai_rs::TriggerExecution> = client
                .triggers()
                .list_executions(&trigger_id)
                .items()
                .try_collect()
                .await
                .expect("list_executions");
            if let Some(e) = executions.into_iter().next() {
                if let Some(id) = &e.interaction_id {
                    *fired.lock().unwrap() = Some(id.clone());
                }
                if e.status != Some(TriggerExecutionStatus::InProgress) {
                    execution = Some(e);
                    break;
                }
            }
        }
        let execution = execution.expect("the run did not finish within 180s");
        println!("Execution: {execution:?}");
        assert_eq!(execution.trigger_id.as_deref(), Some(trigger_id.as_str()));
        assert!(execution.id.is_some() && execution.start_time.is_some());
        assert!(
            execution.status.as_ref().is_some_and(|s| !s.is_unknown()),
            "unrecognized execution status: {:?}",
            execution.status
        );
        if execution.status == Some(TriggerExecutionStatus::Completed) {
            assert!(execution.interaction_id.is_some());
        }
        assert!(
            execution.extra.is_empty(),
            "unmodeled: {:?}",
            execution.extra
        );

        let resumed = client
            .triggers()
            .update(
                &trigger_id,
                &TriggerUpdate::new().with_status(TriggerStatus::Active),
            )
            .await
            .expect("resume");
        assert_eq!(resumed.status, Some(TriggerStatus::Active));
        assert!(resumed.last_resume_time.is_some());

        // `error` is output-only.
        match client
            .triggers()
            .update(
                &trigger_id,
                &TriggerUpdate::new().with_status(TriggerStatus::Error),
            )
            .await
        {
            Err(genai_rs::GenaiError::Api {
                status_code: 400,
                message,
                ..
            }) if message.contains("Invalid status transition") => {}
            other => panic!("expected the status-transition 400, got {other:?}"),
        }

        // `store` in the nested request is rejected, which is why
        // TriggerCreateParams warns about it.
        let with_store = TriggerCreateParams::new(
            "0 5 1 1 *",
            "UTC",
            InteractionRequest {
                store: Some(true),
                ..interaction()
            },
        );
        match client.triggers().create(&with_store).await {
            Err(genai_rs::GenaiError::Api {
                status_code: 400,
                message,
                ..
            }) if message.contains("'store' is not allowed") => {}
            Ok(t) => {
                if let Some(id) = t.id.as_deref() {
                    let _ = client.triggers().delete(id).await;
                }
                panic!(
                    "a trigger with `store` set was accepted; update the TriggerCreateParams warning"
                );
            }
            Err(e) => panic!("expected the `store` 400, got {e}"),
        }
    };
    let outcome = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(checks)).await;

    // Best effort: the run stores an interaction and creates an environment.
    let fired = fired.into_inner().unwrap_or_else(|p| p.into_inner());
    if let Some(id) = fired {
        let _ = client.interactions().delete(&id).await;
    }
    if let Ok(trigger) = client.triggers().get(&trigger_id).await
        && let Some(env) = trigger.environment_id
    {
        let _ = client.environments().delete(&env).await;
    }
    let trigger_deleted = client.triggers().delete(&trigger_id).await;
    let agent_deleted = client.agents().delete(&agent_id).await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    trigger_deleted.expect("triggers.delete");
    agent_deleted.expect("agents.delete");

    match client.triggers().get(&trigger_id).await {
        Err(genai_rs::GenaiError::Api {
            status_code: 404, ..
        }) => {}
        other => panic!("expected 404 after delete, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_trigger_list_streams_end_on_the_last_page() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // The test key usually holds no triggers (`{}`, verified 2026-09-26),
    // so this pins the end-of-list rule on a real response; following
    // several pages is covered offline in http_mock_resources.rs.
    let pages: Vec<_> = client
        .triggers()
        .list()
        .with_page_size(2)
        .pages()
        .take(50)
        .try_collect()
        .await
        .expect("triggers.list pages");
    println!(
        "Listed {} page(s), {} trigger(s)",
        pages.len(),
        pages.iter().map(|p| p.triggers.len()).sum::<usize>()
    );
    let last = pages.last().expect("at least one page");
    assert!(
        last.next_page_token.as_deref().is_none_or(str::is_empty),
        "the stream must end on a page without a token: {:?}",
        last.next_page_token
    );

    let triggers: Vec<_> = client
        .triggers()
        .list()
        .with_page_size(2)
        .items()
        .take(100)
        .try_collect()
        .await
        .expect("triggers.list items");
    println!("Streamed {} trigger(s)", triggers.len());

    // The executions sub-collection answers `{}` for a trigger that does
    // not exist (verified 2026-09-26), not a 404, so this reaches the
    // executions path live without a trigger to create.
    let executions: Vec<_> = client
        .triggers()
        .list_executions("genai-rs-no-such-trigger")
        .with_page_size(2)
        .items()
        .take(100)
        .try_collect()
        .await
        .expect("triggers.list_executions items");
    assert!(
        executions.is_empty(),
        "a missing trigger has no executions: {executions:?}"
    );
}
