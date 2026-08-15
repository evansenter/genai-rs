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

mod common;

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

/// `with_response_schema` must actually constrain the final output, and
/// the result must arrive on `structured_output()` rather than only as
/// text the caller has to re-parse.
///
/// The schema is deliberately not the obvious shape for the question (two
/// renamed fields plus a number), so a response that happens to look right
/// cannot be a coincidence of the model's default formatting.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_structured_output_follows_response_schema() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "capital_city": {"type": "string"},
            "country_name": {"type": "string"},
            "letters_in_capital": {"type": "integer"},
        },
        "required": ["capital_city", "country_name", "letters_in_capital"],
    });

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_capabilities(Capabilities::none())
        .with_response_schema(schema)
        .spawn()
        .await
        .expect("spawn");

    let response = agent
        .chat("What is the capital of France?")
        .await
        .expect("chat turn");

    let output = response
        .structured_output()
        .expect("with_response_schema should produce structured_output");

    // Structural, not semantic: the point is that the schema was honored,
    // not that the model knows French geography.
    let object = output
        .as_object()
        .unwrap_or_else(|| panic!("structured output should be a JSON object, got: {output}"));
    for key in ["capital_city", "country_name", "letters_in_capital"] {
        assert!(
            object.contains_key(key),
            "structured output is missing required key {key:?}: {output}"
        );
    }
    assert!(
        object["capital_city"].is_string(),
        "capital_city should be a string: {output}"
    );
    assert!(
        object["letters_in_capital"].is_i64() || object["letters_in_capital"].is_u64(),
        "letters_in_capital should be an integer: {output}"
    );

    agent.shutdown().await.expect("shutdown");
}

/// A `CancelHandle` taken before a turn must be able to halt that turn
/// from outside the `&mut self` borrow that `send_streaming` holds — which
/// is the entire reason the handle exists, and is not exercised anywhere
/// else.
///
/// This test also pins *how* a cancelled turn ends, which is not what the
/// crate documented before it was run: harness 0.1.10 answers a
/// `haltRequest` with `STATE_FULLY_IDLE`, not `STATE_CANCELLED`, so the
/// turn resolves as a normal `Finished` carrying whatever partial output
/// existed — it does **not** fail with `AntigravityError::Turn`. The
/// `Cancelled` arm in the turn loop still matters for harness-initiated
/// cancellation; it is simply not the client-halt path.
///
/// The prompt is long on purpose: it has to still be running when the
/// cancel lands, or the test proves nothing.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_cancel_handle_halts_an_in_flight_turn() {
    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    /// The turn must end promptly after the halt. Generous enough to
    /// absorb a slow round trip, far below both the turn timeout and how
    /// long the un-cancelled prompt would take.
    const MUST_END_WITHIN: std::time::Duration = std::time::Duration::from_secs(30);

    let mut agent = AntigravityAgent::builder()
        // Much longer than MUST_END_WITHIN: if the halt is ignored, this
        // test must fail on the elapsed-time assertion rather than being
        // rescued by a timeout that looks like a pass.
        .with_turn_timeout(std::time::Duration::from_secs(180))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_capabilities(Capabilities::none())
        .spawn()
        .await
        .expect("spawn");

    // Taken *before* the stream borrows the agent — the handle is the only
    // way to reach the session while a turn is in flight.
    //
    // The halt fires on accumulated output rather than on a timer: a fixed
    // delay races the model, and losing that race ("it answered before we
    // cancelled") is an inconclusive run reported as a defect. Keying off
    // real output means the turn is provably mid-generation when the halt
    // is sent.
    //
    // The threshold is deliberately not "the first delta" — the harness
    // emits a step before the upstream request is even in flight, and
    // halting there cancelled the POST rather than a running generation.
    // Any real answer to this prompt clears this bar many times over, so
    // failing to reach it means something is broken, not merely fast.
    /// Characters of streamed text/thinking to see before halting.
    const OUTPUT_BEFORE_CANCEL: usize = 200;

    let handle = agent.cancel_handle();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    // Three-valued on purpose: "never reached the threshold" and "the halt
    // request itself failed" want different diagnostics, and collapsing
    // them would point a future debugger at the model's speed when the
    // real fault was the halt.
    let canceller = tokio::spawn(async move {
        if started_rx.await.is_err() {
            // Stream ended before generating enough to halt.
            return None;
        }
        Some(handle.cancel().await)
    });

    let started = std::time::Instant::now();
    let (finished, produced, outcome) = {
        let mut stream = agent
            .send_streaming(
                "Write an extremely detailed 3000-word essay about the history of \
                 the bicycle. Include many sections and go slowly.",
            )
            .await
            .expect("stream");

        let mut started_tx = Some(started_tx);
        let mut finished = false;
        let mut produced = 0usize;
        let mut outcome = Ok(());
        while let Some(event) = stream.next().await {
            match event {
                Ok(AgentEvent::TextDelta(chunk) | AgentEvent::ThinkingDelta(chunk)) => {
                    produced += chunk.chars().count();
                    // Fire once, when generation is demonstrably underway.
                    if produced >= OUTPUT_BEFORE_CANCEL
                        && let Some(tx) = started_tx.take()
                    {
                        let _ = tx.send(());
                    }
                }
                Ok(AgentEvent::Finished(_)) => {
                    finished = true;
                    break;
                }
                Ok(_) => {}
                Err(err) => {
                    outcome = Err(err);
                    break;
                }
            }
        }
        (finished, produced, outcome)
    };
    let elapsed = started.elapsed();
    match canceller.await.expect("canceller task") {
        Some(Ok(())) => {}
        Some(Err(err)) => panic!(
            "the halt request itself failed after {produced} chars of output: \
             {err:?}"
        ),
        None => panic!(
            "the turn streamed only {produced} chars, below the \
             {OUTPUT_BEFORE_CANCEL} needed before halting, so no halt was sent \
             — this run proves nothing about cancellation"
        ),
    }

    // The assertion that actually proves cancellation: the turn stopped
    // long before it would have finished on its own, and long before the
    // turn timeout could have ended it.
    assert!(
        elapsed < MUST_END_WITHIN,
        "the turn ran {elapsed:?}, past the {MUST_END_WITHIN:?} bound — the \
         halt did not stop it"
    );

    match outcome {
        // The documented-and-verified shape on 0.1.10.
        Ok(()) => assert!(
            finished,
            "the stream ended without a Finished event and without an error"
        ),
        // Not what 0.1.10 does, but the turn loop still maps a harness
        // STATE_CANCELLED here; accept it rather than fail if the harness
        // changes its mind, and say so loudly.
        Err(AntigravityError::Turn(message)) => {
            println!(
                "note: this harness reported cancellation as a Turn error \
                 ({message:?}) — 0.1.10 ended the turn as Finished. Update the \
                 CancelHandle docs if this is the new behavior."
            );
        }
        Err(other) => panic!("unexpected error from a cancelled turn: {other:?}"),
    }

    agent.shutdown().await.expect("shutdown");
}

/// The `on_questions` hook end to end: the harness's `ask_question`
/// builtin must reach the hook, and the hook's answer must get back into
/// the turn well enough for the model to act on it.
///
/// Without a hook the crate replies "unanswered" to every question, which
/// never deadlocks but also never proves the reply path works. So the
/// closing assertion is semantic: the final answer has to reflect the
/// choice the hook selected. A harness that dropped every answer on the
/// floor and let the model proceed on its own would pass a
/// "was a question asked?" test unchanged.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_on_questions_answers_reach_the_model() {
    use genai_rs::antigravity::{AgentQuestion, QuestionAnswer, QuestionReply};

    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let key_for_validation = key.clone();

    /// The questions the hook saw, so the test can assert on them after
    /// the turn rather than from inside a sync closure.
    type QuestionLog = std::sync::Arc<std::sync::Mutex<Vec<AgentQuestion>>>;
    let asked: QuestionLog = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook_asked = std::sync::Arc::clone(&asked);

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "Before answering anything ambiguous, you MUST use the ask_question \
             tool to ask the user exactly one clarifying question with concrete \
             choices. Then answer using their choice.",
        )
        // AskQuestion is write-capable, so it needs a policy to clear the
        // spawn-time gate even though questions bypass the policy engine.
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::AskQuestion))
        .add_policy(policy::allow_all())
        .on_questions(move |questions| {
            hook_asked.lock().unwrap().extend_from_slice(questions);
            // Always the *last* choice, never the first: models tend to
            // list their own preferred option first, so picking it would
            // leave "the model ignored the answer and did what it wanted"
            // indistinguishable from "the answer arrived".
            QuestionReply::Answers(
                questions
                    .iter()
                    .map(|q| {
                        if q.choices.is_empty() {
                            QuestionAnswer::Freeform("the last option".to_string())
                        } else {
                            QuestionAnswer::Choices {
                                selected: vec![q.choices.len() - 1],
                                freeform: None,
                            }
                        }
                    })
                    .collect(),
            )
        })
        .spawn()
        .await
        .expect("spawn");

    let response = agent
        .chat(
            "I want a recommendation. Ask me one clarifying question first, \
             then give me a one-sentence recommendation.",
        )
        .await
        .expect("chat turn");
    let text = response.text().to_string();
    println!("final response: {text}");

    let questions = std::mem::take(&mut *asked.lock().unwrap());
    assert!(
        !questions.is_empty(),
        "on_questions never fired — the ask_question builtin did not reach \
         the hook (response was: {text})"
    );
    for question in &questions {
        println!(
            "asked: {:?} choices={:?} multi={}",
            question.question, question.choices, question.is_multi_select
        );
        assert!(
            !question.question.trim().is_empty(),
            "a question arrived with no text: {question:?}"
        );
    }

    // The turn must have continued past the question rather than stalling
    // on it — the reply path is what this test is really about.
    assert!(
        !text.trim().is_empty(),
        "the turn produced no text after the question was answered"
    );

    // ...and the answer must have *landed*. Semantic rather than a
    // substring match, per CLAUDE.md: the model is free to phrase the
    // recommendation however it likes, but it is not free to recommend
    // from a category the user did not pick.
    let selected: Vec<&str> = questions
        .iter()
        .filter_map(|q| q.choices.last().map(String::as_str))
        .collect();
    if selected.is_empty() {
        // No multiple-choice question to check against; the freeform arm
        // of the hook ran instead.
        println!("note: no choice-bearing question, skipping the choice check");
    } else {
        let client = genai_rs::Client::builder(key_for_validation)
            .build()
            .expect("validation client");
        let context = format!(
            "The assistant asked the user a clarifying question and the user \
             selected this option: {}",
            selected.join(" | ")
        );
        common::assert_response_semantic(
            &client,
            &context,
            &text,
            "Is this response consistent with the option the user selected, \
             rather than a different category?",
        )
        .await;
    }

    agent.shutdown().await.expect("shutdown");
}

/// A configured subagent must actually be *invoked*, not merely accepted
/// at init — `test_antigravity_subagent_config_accepted_at_init` covers
/// the config half and stops there.
///
/// This also re-checks a documented claim that was pinned against harness
/// 0.1.5: `ActionInvokeSubagent::name` says the harness sends an empty
/// message and the name is always `None`. That is exactly the kind of
/// version-pinned claim that goes stale silently, so the test reports what
/// 0.1.10 actually does rather than asserting the old answer.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_subagent_is_actually_invoked() {
    use genai_rs::antigravity::Subagent;

    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "You have a subagent named `haiku-writer`. For any request to write \
             a haiku you MUST delegate to it with the start_subagent tool rather \
             than writing one yourself.",
        )
        .add_subagent(
            Subagent::new("haiku-writer")
                .with_description("Writes a haiku on any topic.")
                .with_system_instructions("Reply with a haiku and nothing else."),
        )
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");

    // (tool name, reported subagent name) for every action in the turn,
    // including the subagent's own trajectory.
    let mut actions: Vec<(String, Option<String>)> = Vec::new();
    {
        let mut stream = agent
            .send_streaming("Write me a haiku about rust the programming language.")
            .await
            .expect("stream");
        while let Some(event) = stream.next().await {
            match event.expect("stream event") {
                AgentEvent::ToolAction { action, .. } => actions.push((
                    action.tool_name(),
                    action.subagent_name().map(ToString::to_string),
                )),
                AgentEvent::Finished(_) => break,
                _ => {}
            }
        }
    }

    let invocations: Vec<&(String, Option<String>)> = actions
        .iter()
        .filter(|(name, _)| name == "start_subagent")
        .collect();
    assert!(
        !invocations.is_empty(),
        "the model never delegated to the subagent — no start_subagent action \
         in the turn. Saw: {actions:?}"
    );

    // Report rather than assert: the crate documents `name` as always
    // `None` on 0.1.5, and the point of running this live is to notice if
    // that stopped being true.
    let named: Vec<&str> = invocations
        .iter()
        .filter_map(|(_, subagent)| subagent.as_deref())
        .collect();
    if named.is_empty() {
        println!(
            "invokeSubagent carried no name ({} invocation(s)) — matches the \
             documented 0.1.5 behavior, still true on 0.1.10",
            invocations.len()
        );
    } else {
        println!(
            "NOTE: invokeSubagent now reports names {named:?} — update the \
             ActionInvokeSubagent docs, which say the name is always None"
        );
    }

    agent.shutdown().await.expect("shutdown");
}

/// An MCP server configured via `add_mcp_server` must actually be reached:
/// the harness has to spawn it, discover its tools, and call one on the
/// model's behalf.
///
/// The fixture server returns a token that exists nowhere else, so a
/// response containing it can only have come from a real round trip —
/// which is the difference between testing the wire config and testing
/// that MCP works.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_antigravity_mcp_server_tool_is_called() {
    use genai_rs::antigravity::McpServer;

    let Some(key) = api_key() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Same token as tests/fixtures/mcp_echo_server.py.
    const WIDGET_CODE: &str = "wibble-3317-quux";

    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_echo_server.py");
    assert!(fixture.is_file(), "missing fixture: {}", fixture.display());

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(key)
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "Widget codes can only be obtained by calling the MCP tool named \
             exactly `lookup_widget_code` on the `widgets` server. Call that \
             tool by that exact name — do not invent a different tool name, \
             and do not guess the code. Report the code verbatim.",
        )
        .add_mcp_server(
            McpServer::stdio("python3", [fixture.to_string_lossy().to_string()])
                .with_name("widgets"),
        )
        // No builtins: MCP servers are configured independently of the
        // builtin capability set, so this leaves the MCP tool as the only
        // thing to call. Without it the model wanders into grep/find over
        // the whole repo when its first attempt misses (observed: a
        // repo-wide grep that timed out and ate the turn budget).
        .with_capabilities(Capabilities::none())
        // MCP tools run harness-side, so they go through the policy engine
        // like any other harness tool.
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn with an MCP server");

    let mut actions: Vec<String> = Vec::new();
    let mut text = String::new();
    {
        let mut stream = agent
            .send_streaming("What is the code for the widget named `flange`?")
            .await
            .expect("stream");
        while let Some(event) = stream.next().await {
            match event.expect("stream event") {
                AgentEvent::ToolAction { action, .. } => actions.push(action.tool_name()),
                AgentEvent::Finished(response) => {
                    text = response.text().to_string();
                    break;
                }
                _ => {}
            }
        }
    }
    println!("actions: {actions:?}");
    println!("response: {text}");

    // The tool-name spelling is the crate's own `mcp_<server>_<tool>`
    // convention, which is also what a policy target has to match — so
    // this pins the naming as well as the round trip.
    assert!(
        actions
            .iter()
            .any(|a| a == "mcp_widgets_lookup_widget_code"),
        "no MCP tool action for the configured server; saw {actions:?}"
    );

    // Deterministic value, not LLM phrasing: the token cannot be guessed,
    // so its presence proves the server's answer reached the model.
    assert!(
        text.contains(WIDGET_CODE),
        "the response does not carry the MCP server's token {WIDGET_CODE:?}: {text}"
    );

    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Protocol drift guard
// =============================================================================

/// Dumps `{proto_enum_path: [values]}` from the installed harness wheel.
///
/// The wheel ships the compiled `localharness_pb2`, which carries a full
/// `FileDescriptorProto` — the authoritative list of what the harness can
/// send. Its module path moved in 0.1.10, so both are tried.
fn harness_proto_enums() -> Option<serde_json::Value> {
    const SCRIPT: &str = r#"
import json, importlib, sys
print("interpreter: " + sys.executable, file=sys.stderr)
try:
    from google.protobuf import descriptor_pb2
except Exception as exc:
    print("google.protobuf is not importable: %r" % (exc,), file=sys.stderr)
    sys.exit(4)
mod = None
for path in ("google.antigravity.proto.localharness_pb2",
             "google.antigravity.connections.local.localharness_pb2"):
    try:
        mod = importlib.import_module(path); break
    except Exception:
        continue
if mod is None:
    sys.exit(3)
fdp = descriptor_pb2.FileDescriptorProto()
mod.DESCRIPTOR.CopyToProto(fdp)
out = {}
def walk(msgs, prefix=""):
    for mt in msgs:
        full = prefix + mt.name
        for e in mt.enum_type:
            out[full + "." + e.name] = sorted(v.name for v in e.value)
        # Message fields under a "fields:" prefix so one dump carries both
        # kinds without colliding with an enum path, in the camelCase
        # spelling the wire uses (which is what the crate matches on).
        #
        # CopyToProto does not populate json_name, so derive it the way
        # proto3 does — lowerCamelCase of the snake_case name — and fall
        # back to json_name only if it is actually set. Reading json_name
        # alone yields a list of empty strings, which looks exactly like
        # "the harness sends nothing" and reports drift for every field.
        def json_name(f):
            if f.json_name:
                return f.json_name
            head, *rest = f.name.split("_")
            return head + "".join(w.capitalize() for w in rest)
        out["fields:" + full] = sorted(json_name(f) for f in mt.field)
        walk(mt.nested_type, full + ".")
for e in fdp.enum_type:
    out[e.name] = sorted(v.name for v in e.value)
walk(fdp.message_type)
print(json.dumps(out))
"#;
    // Point the interpreter at the *same* install the tests exercise.
    // Two of the crate's four discovery modes put the wheel somewhere the
    // system `python3` will not import from on its own — an explicit
    // `ANTIGRAVITY_HARNESS_PATH`, and a `localharness` that a pipx / `uv
    // tool` install dropped on `PATH`. In both, comparing against whatever
    // `python3` happens to import would check a different wheel than the
    // one under test, or none at all. Deriving PYTHONPATH from the
    // *resolved* harness path (rather than from the env var alone) keeps
    // the guard aimed at the same install for every mode that has one.
    let mut command = std::process::Command::new("python3");
    if let Some(dir) = resolved_harness_site_packages() {
        // Prepend, never replace: an inherited PYTHONPATH may be the only
        // thing supplying `google.protobuf` when the wheel and protobuf
        // live in different trees. Ours goes first so it still wins.
        let mut entries = vec![dir];
        if let Some(inherited) = std::env::var_os("PYTHONPATH") {
            entries.extend(std::env::split_paths(&inherited));
        }
        if let Ok(joined) = std::env::join_paths(entries) {
            command.env("PYTHONPATH", joined);
        }
    }
    let output = command.args(["-c", SCRIPT]).output().ok()?;
    if output.status.success() {
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("descriptor dump should be JSON");
        assert!(
            parsed.as_object().is_some_and(|m| !m.is_empty()),
            "the harness descriptor yielded nothing at all — the guard would have \
             checked nothing"
        );
        return Some(parsed);
    }

    // Reaching here means the harness binary was discoverable (this test is
    // gated on that) but its descriptor was not. Failing beats skipping:
    // a guard that goes green when it checked nothing is the same vacuous
    // pass that let the 0.1.5 -> 0.1.10 rename reach a release. The module
    // path has already moved once, and the bump that moves it again is
    // exactly the bump likely to carry a rename with it.
    //
    // Which fix to apply depends on *why* the dump failed, so say so — the
    // script's exit code distinguishes "the module moved again" from "this
    // interpreter has no wheel". The stderr passthrough names the
    // interpreter in every case.
    let diagnosis = match output.status.code() {
        Some(3) => {
            "localharness_pb2 was not importable under either known module path. The wheel \
             most likely moved it again — add the new path to the list in harness_proto_enums."
        }
        Some(4) => {
            "google.protobuf was not importable, so the descriptor could not be read at all. \
             Install the harness wheel (which depends on protobuf) into the interpreter named \
             below."
        }
        _ => {
            "the descriptor dump failed before it could emit JSON. If this interpreter has no \
             harness wheel installed, install one; otherwise read the traceback below."
        }
    };
    panic!(
        "the protocol-drift guard checked nothing: {diagnosis}\n\
         Expected wheel: google-antigravity=={}\n\
         python3 stderr: {}",
        genai_rs::antigravity::SUPPORTED_HARNESS_VERSION,
        String::from_utf8_lossy(&output.stderr).trim()
    );
}

/// python3's site-packages directories, as the crate's discovery sees
/// them. Kept in step with `python_site_dirs` in
/// `src/antigravity/process.rs`; an empty result (no python3) simply means
/// that discovery step matches nothing, which is what the crate does too.
fn python_site_dirs() -> Vec<std::path::PathBuf> {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(
            "import site\n\
             paths = list(getattr(site, 'getsitepackages', lambda: [])())\n\
             usersite = getattr(site, 'getusersitepackages', lambda: None)()\n\
             if usersite: paths.append(usersite)\n\
             print('\\n'.join(paths))",
        )
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(std::path::PathBuf::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// The site-packages directory of the harness install the *crate* would
/// resolve, when that is somewhere `python3` would not look on its own.
///
/// Mirrors `discover_harness`'s order — `ANTIGRAVITY_HARNESS_PATH`, then
/// python3's own site-packages, then `localharness` on `PATH` — minus the
/// builder-path mode, which these tests do not use.
///
/// The site-packages step is included even though it never *produces* a
/// PYTHONPATH, because it is a precedence step: with the env var unset, a
/// wheel in python3's site-packages, and a second `localharness` on `PATH`
/// (pipx / `uv tool`), skipping it would prepend the `PATH` copy's
/// site-packages and win — comparing the crate's model against the wheel
/// the crate is *not* running.
///
/// `None` means "let python3 use its own site-packages", which is correct
/// both there and whenever the resolved binary does not sit inside a
/// recognizable wheel layout.
fn resolved_harness_site_packages() -> Option<std::path::PathBuf> {
    let from_path_var = || {
        let path_var = std::env::var_os("PATH")?;
        std::env::split_paths(&path_var)
            .map(|dir| dir.join("localharness"))
            .find(|candidate| candidate.is_file())
    };

    let harness = match std::env::var_os("ANTIGRAVITY_HARNESS_PATH")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
    {
        Some(explicit) => explicit,
        // python3 would find its own copy before anything on PATH, so
        // leaving PYTHONPATH alone is what keeps the two in sync.
        None if python_site_dirs()
            .iter()
            .any(|d| d.join("google/antigravity/bin/localharness").is_file()) =>
        {
            return None;
        }
        None => from_path_var()?,
    };

    // A `PATH` hit is usually a symlink into the venv that owns the wheel,
    // so resolve it before reading the layout — the link's own directory
    // says nothing about where the package lives.
    let harness = harness.canonicalize().unwrap_or(harness);

    // A wheel install lays the binary out as
    // .../<site-packages>/google/antigravity/bin/localharness, so the
    // fourth ancestor is site-packages. Anything else (a symlink farm, a
    // hand-built binary) fails the layout check and falls back to None
    // rather than pointing the interpreter somewhere useless.
    harness
        .ancestors()
        .nth(4)
        .map(std::path::Path::to_path_buf)
        .filter(|d| d.join("google/antigravity").is_dir())
}

/// Compares a harness descriptor dump against what this crate models,
/// returning one human-readable line per drifted enum.
///
/// Pure and synchronous so the detection logic itself is testable — a
/// drift guard that cannot be shown to detect drift is worse than none,
/// because it reads as coverage.
fn find_enum_drift(harness: &serde_json::Value, modeled: &[(&str, &[&str])]) -> Vec<String> {
    let mut drift = Vec::new();
    for (proto_path, known) in modeled {
        let Some(values) = harness.get(proto_path).and_then(|v| v.as_array()) else {
            drift.push(format!(
                "{proto_path}: not present in the harness descriptor — the message or \
                 enum was renamed or removed"
            ));
            continue;
        };
        let unmodeled: Vec<&str> = values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|v| !known.contains(v))
            .collect();
        if !unmodeled.is_empty() {
            drift.push(format!(
                "{proto_path}: harness can send {unmodeled:?}, which this crate does not \
                 model (they would deserialize to Unknown and stop matching); known = {known:?}"
            ));
        }
    }
    drift
}

/// Compares harness message *fields* against the wire names this crate
/// reads by hand, returning one line per field that has gone missing.
///
/// Enum-value drift and field drift are different failure modes, and the
/// 0.1.5 -> 0.1.10 upgrade shipped one of each: `STATE_IDLE` ->
/// `STATE_FULLY_IDLE` (a value) and `usageMetadata` -> `usageUpdate` (a
/// field). A guard covering only the first would have caught only half of
/// the break it was written for.
///
/// Deliberately one-directional, like the enum check: a field the harness
/// has that the crate ignores is not drift, but a field the crate reads
/// that the harness no longer has means a `get()` silently returning
/// `None` forever.
fn find_field_drift(harness: &serde_json::Value, modeled: &[(&str, &[&str])]) -> Vec<String> {
    let mut drift = Vec::new();
    for (message, required) in modeled {
        let key = format!("fields:{message}");
        let Some(fields) = harness.get(&key).and_then(|v| v.as_array()) else {
            drift.push(format!(
                "{message}: not present in the harness descriptor — the message was \
                 renamed or removed"
            ));
            continue;
        };
        let present: Vec<&str> = fields
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        let missing: Vec<&&str> = required.iter().filter(|f| !present.contains(f)).collect();
        if !missing.is_empty() {
            drift.push(format!(
                "{message}: this crate reads {missing:?}, which the harness no longer \
                 sends (the read silently yields None); harness has = {present:?}"
            ));
        }
    }
    drift
}

#[test]
fn field_drift_detector_catches_the_usage_rename() {
    // The other half of the 0.1.10 break, replayed: the crate reads
    // `usageMetadata`, the harness renamed it to `usageUpdate`, and token
    // accounting silently zeroed.
    let harness = serde_json::json!({
        "fields:OutputEvent": ["seqNum", "timestampMicros", "usageUpdate", "stepUpdate"],
    });
    let drift = find_field_drift(
        &harness,
        &[("OutputEvent", &["seqNum", "usageMetadata"] as &[&str])],
    );
    assert_eq!(drift.len(), 1, "expected one drift line, got {drift:?}");
    assert!(
        drift[0].contains("usageMetadata"),
        "the report must name the field that vanished: {}",
        drift[0]
    );

    // A field the harness added but the crate ignores is not drift.
    let quiet = find_field_drift(&harness, &[("OutputEvent", &["seqNum"] as &[&str])]);
    assert!(
        quiet.is_empty(),
        "extra harness fields are not drift: {quiet:?}"
    );

    // A renamed *message* is reported rather than passing vacuously.
    let gone = find_field_drift(&harness, &[("StepUpdate", &["mcpTool"] as &[&str])]);
    assert_eq!(gone.len(), 1);
    assert!(gone[0].contains("not present"));
}

#[test]
fn drift_detector_catches_the_0_1_10_rename() {
    // The exact shape of the bug this guard exists for: the harness
    // renamed the terminal trajectory state, and a crate that models only
    // the old spelling must be told loudly.
    let harness = serde_json::json!({
        "TrajectoryStateUpdate.State": [
            "STATE_CANCELLED", "STATE_FULLY_IDLE", "STATE_RUNNING",
            "STATE_UNSPECIFIED", "STATE_WAITING_FOR_TASKS"
        ]
    });
    let stale: &[&str] = &[
        "STATE_UNSPECIFIED",
        "STATE_RUNNING",
        "STATE_IDLE",
        "STATE_CANCELLED",
    ];
    let drift = find_enum_drift(&harness, &[("TrajectoryStateUpdate.State", stale)]);
    assert_eq!(drift.len(), 1, "expected one drifted enum, got {drift:?}");
    assert!(drift[0].contains("STATE_FULLY_IDLE"), "got: {}", drift[0]);
    assert!(
        drift[0].contains("STATE_WAITING_FOR_TASKS"),
        "got: {}",
        drift[0]
    );

    // And the current model is clean against the same descriptor.
    let current = genai_rs::antigravity::protocol::TrajectoryState::all_wire_values();
    assert!(find_enum_drift(&harness, &[("TrajectoryStateUpdate.State", current)]).is_empty());
}

#[test]
fn drift_detector_flags_a_missing_enum_and_ignores_retired_values() {
    // A message/enum that vanished from the descriptor is drift.
    let empty = serde_json::json!({});
    let drift = find_enum_drift(&empty, &[("StepUpdate.State", &["STATE_DONE"][..])]);
    assert_eq!(drift.len(), 1);
    assert!(drift[0].contains("not present"), "got: {}", drift[0]);

    // A value the crate keeps but the harness no longer sends is NOT
    // drift — that is exactly what the alias mechanism is for.
    let harness = serde_json::json!({ "StepUpdate.State": ["STATE_DONE"] });
    let with_alias: &[&str] = &["STATE_DONE", "STATE_RETIRED_SPELLING"];
    assert!(find_enum_drift(&harness, &[("StepUpdate.State", with_alias)]).is_empty());
}

/// Fails when the harness's enum values drift from what this crate models.
///
/// This is the guard that would have caught the 0.1.5 -> 0.1.10 break on
/// the day the wheel was bumped, instead of every agent turn silently
/// running to its timeout. `STATE_IDLE` became `STATE_FULLY_IDLE`; because
/// only the `Idle` variant ends a turn and the renamed value was absorbed
/// as `Unknown`, nothing errored and nothing failed — the bridge simply
/// stopped recognizing the end of a turn.
///
/// Deliberately one-directional: a value the crate knows but the harness
/// no longer sends is fine (that is what aliases are for). A value the
/// **harness can send** and the crate does not model is the dangerous
/// direction, because it lands in `Unknown` and stops matching.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_antigravity_protocol_enums_have_not_drifted() {
    use genai_rs::antigravity::protocol::{
        HookDecision, LifecycleHook, LineAction, ModelType, StepSource, StepState, StepTarget,
        TrajectoryState,
    };

    let Some(harness) = harness_proto_enums() else {
        // python3 itself is absent — the only remaining reason to skip.
        // A wheel that is present but unreadable panics inside the helper.
        println!("Skipping: python3 not available to read the harness descriptor");
        return;
    };

    // (proto enum path, what this crate recognizes)
    let modeled: Vec<(&str, &[&str])> = vec![
        ("StepUpdate.State", StepState::all_wire_values()),
        ("StepUpdate.Source", StepSource::all_wire_values()),
        ("StepUpdate.Target", StepTarget::all_wire_values()),
        (
            "TrajectoryStateUpdate.State",
            TrajectoryState::all_wire_values(),
        ),
        ("ModelType", ModelType::all_wire_values()),
        ("LifecycleHook", LifecycleHook::all_wire_values()),
        ("PreToolResult.Decision", HookDecision::all_wire_values()),
        (
            "ActionEditFile.DiffLine.LineAction",
            LineAction::all_wire_values(),
        ),
    ];

    let mut drift = find_enum_drift(&harness, &modeled);

    // Field names the crate reads by hand rather than through serde's
    // derive — every one of these is a `map.remove(..)` / `get(..)` on a
    // string literal, so a rename turns into a silent `None` rather than a
    // parse error. `usageMetadata` is listed because it is still read as
    // the pre-0.1.10 spelling; if the harness ever drops it entirely, the
    // alias handling is what needs revisiting.
    let fields: Vec<(&str, &[&str])> = vec![
        (
            "OutputEvent",
            &[
                "seqNum",
                "timestampMicros",
                "usageUpdate",
                "stepUpdate",
                "trajectoryStateUpdate",
                "toolCall",
                "initializeConversationResponse",
                "callHookRequest",
                "sessionEndResponse",
            ],
        ),
        // Every InputEvent arm this crate can send. `config` is
        // deliberately absent: the init message is not an InputEvent arm,
        // which the guard caught when this list first claimed otherwise.
        (
            "InputEvent",
            &[
                "userInput",
                "complexUserInput",
                "toolConfirmation",
                "toolResponse",
                "questionResponse",
                "haltRequest",
                "automatedTrigger",
                "callHookResponse",
                "sessionEndRequest",
            ],
        ),
    ];
    drift.extend(find_field_drift(&harness, &fields));

    assert!(
        drift.is_empty(),
        "harness protocol drift detected — update the wire enums/fields in \
         src/antigravity/protocol.rs (and docs/ENUM_WIRE_FORMATS.md):\n  {}",
        drift.join("\n  ")
    );
}
