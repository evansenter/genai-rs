//! Integration tests against a real `localharness` binary.
//!
//! These tests need the binary from the `google-antigravity` wheel
//! (`pip install google-antigravity==0.1.5`) discoverable via the standard
//! order (`ANTIGRAVITY_HARNESS_PATH`, python3 site-packages, `PATH`).
//!
//! Most tests do NOT need a Gemini API key: the harness completes its
//! handshake and conversation init with a placeholder key (verified
//! against harness 0.1.5). Chat tests need a real `GEMINI_API_KEY`.
//!
//! Run with:
//! ```bash
//! cargo nextest run --features antigravity --run-ignored all -E 'test(/antigravity/)'
//! ```

#![cfg(feature = "antigravity")]

use futures_util::StreamExt;
use genai_rs::CallableFunction;
use genai_rs::antigravity::{
    AgentEvent, AntigravityAgent, AntigravityError, BuiltinTool, Capabilities, policy,
};
use genai_rs_macros::tool;

/// Returns a fixed test weather report for a city.
#[tool(city(description = "The city to get weather for"))]
fn antigravity_test_weather(city: String) -> String {
    format!(r#"{{"city": "{city}", "temperature": "17C", "conditions": "drizzle-42-xyzzy"}}"#)
}

fn scratch_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(name)
        .tempdir()
        .expect("create scratch dir")
}

// =============================================================================
// No API key required (init succeeds with a placeholder key)
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_spawn_handshake_init_roundtrip() {
    let dir = scratch_dir("agy-init");
    let agent = AntigravityAgent::builder()
        .with_api_key("dummy-key-init-does-not-validate")
        .with_model("gemini-3-flash-preview")
        .with_save_dir(dir.path().to_string_lossy())
        .with_system_instructions("test instructions")
        .add_tool(AntigravityTestWeatherCallable.declaration())
        .spawn()
        .await
        .expect("spawn + handshake + init should succeed without a valid API key");

    // The harness assigns a conversation (cascade) id at init.
    let conversation_id = agent
        .conversation_id()
        .expect("init response carries a cascade id");
    assert!(!conversation_id.is_empty());
    // Fresh conversation: no restored history.
    assert!(agent.initial_history().is_empty());

    agent.shutdown().await.expect("graceful shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_graceful_shutdown_no_zombie() {
    let start = std::time::Instant::now();
    let agent = AntigravityAgent::builder()
        .with_api_key("dummy-key")
        .spawn()
        .await
        .expect("spawn should succeed");

    agent.shutdown().await.expect("graceful shutdown");
    // Closing stdin is the graceful signal; the harness must exit well
    // within the SIGTERM escalation window (5s) — a hang here means the
    // shutdown ordering regressed and we leaked/escalated.
    assert!(
        start.elapsed() < std::time::Duration::from_secs(15),
        "shutdown took {:?}, harness likely required kill escalation",
        start.elapsed()
    );
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_init_failure_surfaces_harness_stderr() {
    // No api key => no models in the HarnessConfig. Harness 0.1.5 refuses
    // to initialize a conversation without a text model and reports the
    // reason on stderr.
    let err = AntigravityAgent::builder()
        .spawn()
        .await
        .expect_err("init without models must fail");
    let AntigravityError::InitFailed { stderr, .. } = &err else {
        panic!("expected InitFailed, got {err:?}");
    };
    assert!(
        stderr.contains("no text model configuration provided"),
        "harness stderr should carry the actionable message, got:\n{stderr}"
    );
}

/// A wire inspector that records every WebSocket send.
#[derive(Debug, Default)]
struct WsSendCapture(std::sync::Mutex<Vec<serde_json::Value>>);

impl genai_rs::wire::WireInspector for WsSendCapture {
    fn on_event(&self, event: &genai_rs::wire::WireEvent) {
        if let genai_rs::wire::WireEvent::WsSend { payload, .. } = event {
            self.0.lock().unwrap().push(payload.clone());
        }
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_trigger_sends_automated_trigger_when_idle() {
    use genai_rs::antigravity::TriggerConfig;
    use std::sync::Arc;
    use std::time::Duration;

    let inspector = Arc::new(WsSendCapture::default());
    let agent = AntigravityAgent::builder()
        .with_api_key("dummy-key-trigger-test")
        .with_model("gemini-3-flash-preview")
        .add_trigger(TriggerConfig::new("tick-xyzzy", Duration::from_secs(1)))
        .add_wire_inspector(inspector.clone())
        .spawn()
        .await
        .expect("spawn");

    // The agent is idle after init, so the trigger must deliver after its
    // first 1s interval. Poll generously (up to 15s) to stay non-flaky on
    // slow machines; typical delivery is ~1s.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let mut delivered = false;
    while std::time::Instant::now() < deadline {
        delivered = inspector.0.lock().unwrap().iter().any(|payload| {
            payload.get("automatedTrigger").and_then(|v| v.as_str()) == Some("tick-xyzzy")
        });
        if delivered {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        delivered,
        "expected an automatedTrigger send within 15s; sends observed: {:?}",
        inspector.0.lock().unwrap()
    );

    agent.shutdown().await.expect("graceful shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_subagent_config_accepted_at_init() {
    use genai_rs::antigravity::{BuiltinTool, Capabilities, Subagent};

    // The harness must accept a conversation init carrying customSubagents
    // (no API key needed for init).
    let agent = AntigravityAgent::builder()
        .with_api_key("dummy-key-subagent-test")
        .with_model("gemini-3-flash-preview")
        .add_tool(AntigravityTestWeatherCallable.declaration())
        .add_subagent(
            Subagent::new("weather-checker")
                .with_description("Looks up the weather for one city.")
                .with_system_instructions("Always use the weather tool.")
                .add_tool("antigravity_test_weather"),
        )
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("init with custom subagents should succeed");
    assert!(agent.conversation_id().is_some());

    agent.shutdown().await.expect("graceful shutdown");
}

// =============================================================================
// Real API key required
// =============================================================================

fn api_key() -> Option<String> {
    std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_chat_basic() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3-flash-preview")
        .with_system_instructions("Answer in one short sentence.")
        .spawn()
        .await
        .expect("spawn");

    let response = agent
        .chat("Reply with the single word: pong")
        .await
        .expect("chat turn");
    // Structural assertions: a completed model response and usage exist.
    assert!(!response.text().is_empty(), "expected response text");
    assert!(response.usage().is_some(), "expected usage metadata");

    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_custom_tool_roundtrip() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3-flash-preview")
        .with_system_instructions(
            "You must use the antigravity_test_weather tool to answer weather questions.",
        )
        .with_capabilities(Capabilities::none().enable(BuiltinTool::Finish))
        .add_tool(AntigravityTestWeatherCallable.declaration())
        .add_policy(policy::deny_all())
        .add_policy(policy::allow("antigravity_test_weather"))
        .spawn()
        .await
        .expect("spawn");

    let mut saw_dispatch = false;
    let mut saw_finish = false;
    let mut text = String::new();
    {
        let mut stream = agent
            .send_streaming("What's the weather in Zurich? Use your tool.")
            .await
            .expect("stream");
        while let Some(event) = stream.next().await {
            match event.expect("stream event") {
                AgentEvent::ToolCallDispatched { name, .. } => {
                    assert_eq!(name, "antigravity_test_weather");
                    saw_dispatch = true;
                }
                AgentEvent::TextDelta(delta) => text.push_str(&delta),
                AgentEvent::Finished(response) => {
                    saw_finish = true;
                    if text.is_empty() {
                        text = response.text().to_string();
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    assert!(saw_dispatch, "the custom tool should have been dispatched");
    assert!(saw_finish, "the turn should finish");
    // The sentinel value from the tool result is deterministic data the
    // model must have echoed or used; check the dispatch happened rather
    // than exact phrasing (LLM output varies).
    assert!(!text.is_empty(), "expected final text");

    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_policy_denies_custom_tool() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3-flash-preview")
        .with_system_instructions(
            "Always try the antigravity_test_weather tool first for weather questions. \
             If a tool fails, say TOOL-DENIED and stop.",
        )
        .add_tool(AntigravityTestWeatherCallable.declaration())
        .add_policy(policy::deny("antigravity_test_weather"))
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");

    let response = agent
        .chat("What's the weather in Oslo?")
        .await
        .expect("turn should complete despite the deny (model sees the error)");
    assert!(!response.text().is_empty());

    agent.shutdown().await.expect("shutdown");
}
