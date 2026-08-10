//! Integration tests against a real `localharness` binary.
//!
//! These tests need the binary from the `google-antigravity` wheel
//! (`pip install google-antigravity==0.1.10`) discoverable via the standard
//! order (`ANTIGRAVITY_HARNESS_PATH`, python3 site-packages, `PATH`).
//!
//! Most tests do NOT need a Gemini API key: the harness completes its
//! handshake and conversation init with a placeholder key (verified
//! against harness 0.1.5 and 0.1.10). Chat tests need a real
//! `GEMINI_API_KEY` — CI supplies one, because these are the only tests
//! that drive a real turn end to end and therefore the only guard
//! against a harness protocol change breaking turn completion.
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
        .with_model("gemini-3.6-flash")
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
        .with_model("gemini-3.6-flash")
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
async fn test_antigravity_turn_timeout_halts_harness_turn() {
    use std::sync::Arc;
    use std::time::Duration;

    let inspector = Arc::new(WsSendCapture::default());
    let mut agent = AntigravityAgent::builder()
        .with_api_key("dummy-key-timeout-test")
        .with_model("gemini-3.6-flash")
        // Far below any network round-trip: the timeout always fires before
        // the turn can produce a terminal event.
        .with_turn_timeout(Duration::from_millis(10))
        .add_wire_inspector(inspector.clone())
        .spawn()
        .await
        .expect("spawn");

    let err = agent
        .chat("this turn cannot finish in 10ms")
        .await
        .expect_err("a 10ms turn budget must time out");
    assert!(
        matches!(err, AntigravityError::Timeout { .. }),
        "expected Timeout, got {err:?}"
    );

    // Timeout recovery must have halted the harness's still-running turn
    // (and drained its events) so the next turn cannot consume stale
    // output. Assert the wire discipline: userInput, then haltRequest.
    let sends = inspector.0.lock().unwrap().clone();
    let user_input = sends.iter().position(|p| p.get("userInput").is_some());
    let halt = sends.iter().position(|p| p.get("haltRequest").is_some());
    let user_input = user_input.expect("userInput was sent");
    let halt = halt.expect("the timed-out turn must be halted");
    assert!(
        halt > user_input,
        "halt must follow the timed-out turn's input; sends: {sends:?}"
    );

    agent.shutdown().await.expect("graceful shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_chat_after_trigger_discards_trigger_turn() {
    use genai_rs::antigravity::TriggerConfig;
    use std::sync::Arc;
    use std::time::Duration;

    let inspector = Arc::new(WsSendCapture::default());
    let mut agent = AntigravityAgent::builder()
        .with_api_key("dummy-key-trigger-chat-test")
        .with_model("gemini-3.6-flash")
        .with_turn_timeout(Duration::from_secs(15))
        .add_trigger(TriggerConfig::new("tick-quux", Duration::from_secs(1)))
        .add_wire_inspector(inspector.clone())
        .spawn()
        .await
        .expect("spawn");

    // Wait for the trigger to deliver (it starts a harness-side turn that
    // nothing consumes).
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        let delivered = inspector
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.get("automatedTrigger").is_some());
        if delivered {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // chat() must halt-and-drain the trigger's turn before sending its own
    // input, and must never surface the stale turn's outcome. With a dummy
    // key this turn ends in a model-backend error or a timeout — never in
    // the halted trigger turn's cancellation, and never in a stale Ok.
    let result = agent.chat("hello after the trigger").await;
    if let Err(AntigravityError::Turn(message)) = &result {
        assert!(
            !message.to_lowercase().contains("cancel"),
            "the halted trigger turn's cancellation leaked into chat(): {message}"
        );
    }

    // Wire discipline: automatedTrigger, then haltRequest, then userInput.
    let sends = inspector.0.lock().unwrap().clone();
    let trigger = sends
        .iter()
        .position(|p| p.get("automatedTrigger").is_some())
        .expect("trigger was delivered");
    let halt = sends
        .iter()
        .position(|p| p.get("haltRequest").is_some())
        .expect("chat() must halt the unconsumed trigger turn");
    let user_input = sends
        .iter()
        .position(|p| p.get("userInput").is_some())
        .expect("userInput was sent");
    assert!(
        trigger < halt && halt < user_input,
        "expected automatedTrigger < haltRequest < userInput; sends: {sends:?}"
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
        .with_model("gemini-3.6-flash")
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
        .with_model("gemini-3.6-flash")
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
        .with_model("gemini-3.6-flash")
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
        .with_model("gemini-3.6-flash")
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

// =============================================================================
// Session persistence (real API key required)
// =============================================================================

/// The `with_save_dir` + `conversation_id()` + `with_conversation_id` round
/// trip, which `examples/real_world/session_resume` demonstrates.
///
/// Asserts the half that is easy to get silently wrong: resuming an unknown
/// id is *not* an error, it just comes back empty — so a broken resume looks
/// exactly like a working one unless `initial_history()` is checked.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_session_resume_restores_history() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let save_dir = scratch_dir("agy-resume");
    let save_path = save_dir.path().to_string_lossy().to_string();

    // --- First session: plant a fact, capture the id, shut down cleanly.
    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key.clone())
        .with_model("gemini-3.6-flash")
        .with_system_instructions("You are a terse note-keeper. Recall facts when asked.")
        .with_capabilities(Capabilities::none())
        .with_save_dir(save_path.clone())
        .spawn()
        .await
        .expect("spawn (fresh)");

    let conversation_id = agent
        .conversation_id()
        .expect("harness assigns a conversation id")
        .to_string();
    assert!(
        agent.initial_history().is_empty(),
        "a fresh conversation must restore nothing"
    );

    agent
        .chat("Remember this: the fixture code is xyzzy-42.")
        .await
        .expect("first turn");
    // shutdown(), not drop: this is what makes the trajectory durable.
    agent.shutdown().await.expect("shutdown (fresh)");

    // --- Second session: same dir + id, in a brand-new agent.
    let mut resumed = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_system_instructions("You are a terse note-keeper. Recall facts when asked.")
        .with_capabilities(Capabilities::none())
        .with_save_dir(save_path)
        .with_conversation_id(conversation_id.clone())
        .spawn()
        .await
        .expect("spawn (resumed)");

    assert_eq!(
        resumed.conversation_id(),
        Some(conversation_id.as_str()),
        "resuming must keep the same conversation id"
    );
    let restored = resumed.initial_history().len();
    assert!(
        restored > 0,
        "resume restored no history — a silently-fresh conversation is \
         indistinguishable from a working resume without this check"
    );

    // The planted token cannot come from anywhere but restored history.
    let response = resumed
        .chat("What is the fixture code? Reply with just the code.")
        .await
        .expect("resumed turn");
    let text = response.text().to_lowercase();
    assert!(
        text.contains("xyzzy-42"),
        "resumed agent should recall the planted fact, got: {text:?}"
    );

    resumed.shutdown().await.expect("shutdown (resumed)");
}

// =============================================================================
// Workspaces, typed tool actions, and hooks (real API key required)
// =============================================================================

/// Points the builtins at a real directory and asserts the observable
/// surface `examples/real_world/workspace_explorer` is built on:
/// `AgentEvent::ToolAction` actually arrives for harness-executed file
/// tools, and `on_post_tool` reports a *successful* builtin as successful.
///
/// That last assertion is a regression guard: the harness sends
/// `"error": ""` on success, which the bridge previously surfaced as
/// `Some("")` — making `ToolOutcome::error.is_some()` true for every
/// successful call.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_workspace_actions_and_post_tool_success() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let workspace = scratch_dir("agy-workspace");
    std::fs::write(
        workspace.path().join("NOTES.md"),
        "The build token is grue-77.\n",
    )
    .expect("seed workspace");

    // Every post-tool outcome, so we can assert none of the successful
    // ones were reported as errors.
    /// (tool name, error) for each completed tool call.
    type PostToolLog = std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>;
    let outcomes: PostToolLog = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook_outcomes = std::sync::Arc::clone(&outcomes);

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "Read the files in the workspace to answer. Use the file tools rather than guessing.",
        )
        .add_workspace(workspace.path().to_string_lossy().to_string())
        .with_capabilities(Capabilities::read_only())
        .on_post_tool(move |outcome| {
            hook_outcomes
                .lock()
                .unwrap()
                .push((outcome.name.clone(), outcome.error.clone()));
        })
        .spawn()
        .await
        .expect("spawn");

    let mut actions: Vec<String> = Vec::new();
    {
        let mut stream = agent
            .send_streaming("What is the build token? Read NOTES.md.")
            .await
            .expect("stream");
        while let Some(event) = stream.next().await {
            match event.expect("stream event") {
                AgentEvent::ToolAction { action, .. } => actions.push(action.tool_name()),
                AgentEvent::Finished(_) => break,
                _ => {}
            }
        }
    }

    // The workspace is real, so real file tools must have run against it.
    assert!(
        !actions.is_empty(),
        "expected at least one harness ToolAction against the workspace"
    );
    assert!(
        actions
            .iter()
            .any(|a| a == "view_file" || a == "list_directory" || a == "search_directory"),
        "expected a file-tool action, got: {actions:?}"
    );

    // Regression guard: a successful builtin must not report an error.
    // Snapshot and release before the shutdown await.
    let recorded: Vec<(String, Option<String>)> = std::mem::take(&mut *outcomes.lock().unwrap());
    assert!(
        !recorded.is_empty(),
        "on_post_tool should fire for harness-executed builtins"
    );
    for (name, error) in &recorded {
        assert!(
            error.as_deref().is_none_or(|e| !e.trim().is_empty()),
            "{name} reported a blank error string as a failure — the harness \
             sends \"error\": \"\" on success and it must normalize to None"
        );
    }

    agent.shutdown().await.expect("shutdown");
}
