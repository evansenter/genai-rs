//! Key-free, deterministic tests against a real `localharness` binary.
//!
//! The harness calls a scripted local model (`common::fake_gemini`) through
//! `AgentBuilder::with_gemini_base_url`, so each test decides exactly what
//! the "model" does — call `view_file` outside the workspace, call a custom
//! tool, fail with a 400 — and asserts on what the harness and this crate
//! did with it. Real builtins, hooks, policies, containment, triggers and
//! cancellation run; no API key is needed and no model output is guessed
//! at.
//!
//! These need only the binary, from the pinned wheel
//! (`pip install google-antigravity==0.1.23`), found the usual way
//! (`ANTIGRAVITY_HARNESS_PATH`, python3 site-packages, `PATH`). CI runs
//! them in the `Test Suite (antigravity)` job with `GEMINI_API_KEY`
//! unset, as part of the required check.
//!
//! ```bash
//! cargo nextest run --features antigravity --run-ignored all -E 'binary(antigravity_scripted)'
//! ```

#![cfg(feature = "antigravity")]

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use common::fake_gemini::{FakeGemini, ModelTurn};
use futures_util::StreamExt;
use genai_rs::antigravity::{
    AgentBuilder, AgentEvent, AntigravityAgent, AntigravityError, BuiltinTool, Capabilities,
    ChatResponse, PreToolDecision, SUPPORTED_HARNESS_VERSION, ToolDecision, ToolInvocation,
    ToolOutcome, TriggerConfig, policy,
};
use genai_rs::{CallableFunction, FunctionDeclaration, FunctionError, ToolService};
use genai_rs_macros::tool;
use serde_json::{Value, json};

/// The placeholder key the scripted model sees; never a real one.
const PLACEHOLDER_KEY: &str = "scripted-placeholder-key";

/// An agent wired to `fake`, with a turn budget far below the suite's
/// 10-minute hang guard.
fn scripted_agent(fake: &FakeGemini) -> AgentBuilder {
    AntigravityAgent::builder()
        .with_api_key(PLACEHOLDER_KEY)
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_gemini_base_url(fake.base_url())
        .with_turn_timeout(Duration::from_secs(60))
}

fn scratch_dir(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(name)
        .tempdir()
        .expect("create scratch dir")
}

/// A file holding `content`, inside `dir`; returns its absolute path.
fn write_file(dir: &tempfile::TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write fixture file");
    path.canonicalize()
        .expect("canonical path")
        .to_string_lossy()
        .into_owned()
}

/// A `view_file` call as the model makes it.
fn view_file(path: &str) -> ModelTurn {
    ModelTurn::call(
        "view_file",
        json!({"AbsolutePath": path, "toolSummary": "File read", "toolAction": "Reading file"}),
    )
}

/// Drives one streamed turn, collecting tool actions and the response.
async fn streamed_turn(
    agent: &mut AntigravityAgent,
    prompt: &str,
) -> (Vec<(String, ToolDecision)>, Vec<AgentEvent>, ChatResponse) {
    let mut actions = Vec::new();
    let mut events = Vec::new();
    let mut stream = agent.send_streaming(prompt).await.expect("stream starts");
    while let Some(event) = stream.next().await {
        match event.expect("stream event") {
            AgentEvent::ToolAction {
                action, decision, ..
            } => actions.push((action.tool_name(), decision)),
            AgentEvent::Finished(response) => return (actions, events, *response),
            other => events.push(other),
        }
    }
    panic!("the stream ended without a Finished event");
}

/// The text of every function response in model request `n`, joined.
fn function_response_text(fake: &FakeGemini, n: usize) -> String {
    fake.function_responses(n)
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

// =============================================================================
// Plain turns
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_text_turn_round_trips_through_the_harness() {
    let fake = FakeGemini::start(vec![ModelTurn::text("scripted-reply-7781")]).await;
    let mut agent = scripted_agent(&fake).spawn().await.expect("spawn");

    let response = agent
        .chat("hello from the scripted suite")
        .await
        .expect("turn");
    assert_eq!(response.text(), "scripted-reply-7781");
    assert!(response.errors().is_empty(), "{:?}", response.errors());
    assert!(
        response.usage().is_some(),
        "the scripted usage must surface"
    );

    // The harness reached the scripted host with the configured key, on the
    // configured model, carrying the prompt.
    let requests = fake.requests();
    assert_eq!(requests.len(), 1, "one model call for a text-only turn");
    let request = &requests[0];
    assert_eq!(request.header("x-goog-api-key"), Some(PLACEHOLDER_KEY));
    assert!(
        request.target.starts_with(&format!(
            "/v1beta/models/{}:streamGenerateContent",
            genai_rs::DEFAULT_MODEL
        )),
        "unexpected model path: {}",
        request.target
    );
    assert!(
        request
            .json()
            .to_string()
            .contains("hello from the scripted suite")
    );

    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_harness_version_is_the_supported_one() {
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let agent = scripted_agent(&fake).spawn().await.expect("spawn");
    assert_eq!(
        agent.harness_version(),
        Some(SUPPORTED_HARNESS_VERSION),
        "the harness under test must come from the pinned wheel \
         (pip install google-antigravity=={SUPPORTED_HARNESS_VERSION})"
    );
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Builtins through the pre-tool hook
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_builtin_call_allowed_by_the_pre_tool_hook_runs() {
    let workspace = scratch_dir("agy-scripted-allow");
    let path = write_file(&workspace, "inside.txt", "inside-marker-5521");
    let fake = FakeGemini::start(vec![view_file(&path), ModelTurn::text("read it")]).await;

    let seen: Arc<Mutex<Vec<ToolInvocation>>> = Arc::default();
    let hook_seen = Arc::clone(&seen);
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .on_pre_tool(move |invocation| {
            hook_seen.lock().unwrap().push(invocation.clone());
            PreToolDecision::Allow
        })
        .spawn()
        .await
        .expect("spawn");

    let (actions, _, response) = streamed_turn(&mut agent, "read the file").await;
    assert_eq!(response.text(), "read it");

    // The hook judged the call by its real name and arguments...
    let seen = seen.lock().unwrap().clone();
    let call = seen
        .iter()
        .find(|i| i.name == "view_file")
        .unwrap_or_else(|| panic!("the hook never saw view_file: {seen:?}"));
    assert_eq!(call.args["AbsolutePath"], path);
    // ...the stream reported it allowed...
    assert!(
        actions
            .iter()
            .any(|(name, decision)| name == "view_file" && *decision == ToolDecision::Allowed),
        "{actions:?}"
    );
    // ...and the builtin really ran: the file's content went back to the
    // model.
    assert!(
        function_response_text(&fake, 1).contains("inside-marker-5521"),
        "the model never got the file: {:?}",
        fake.function_responses(1)
    );

    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_builtin_call_denied_by_the_pre_tool_hook_never_runs() {
    let workspace = scratch_dir("agy-scripted-deny");
    let path = write_file(&workspace, "inside.txt", "denied-content-8813");
    let fake = FakeGemini::start(vec![view_file(&path), ModelTurn::text("could not read")]).await;

    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .on_pre_tool(|invocation| {
            if invocation.name == "view_file" {
                PreToolDecision::deny("blocked-by-scripted-hook-3391")
            } else {
                PreToolDecision::Allow
            }
        })
        .spawn()
        .await
        .expect("spawn");

    let (actions, _, response) = streamed_turn(&mut agent, "read the file").await;
    assert_eq!(response.text(), "could not read");
    // The harness turns a hook denial into an error step with no action
    // payload, so the action is named "error"; the decision carries the
    // hook's reason (see `TurnState::take_hook_denial`).
    assert!(
        actions.iter().any(|(_, decision)| matches!(
            decision,
            ToolDecision::Denied { reason } if reason.contains("blocked-by-scripted-hook-3391")
        )),
        "the denial must surface on the stream with its reason: {actions:?}"
    );
    let told = function_response_text(&fake, 1);
    assert!(
        told.contains("blocked-by-scripted-hook-3391"),
        "the model must be told why: {told}"
    );
    assert!(
        !told.contains("denied-content-8813"),
        "a denied view_file must not run: {told}"
    );

    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Custom tools through policies
// =============================================================================

static SCRIPTED_TOOL_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Returns a fixed lookup result and counts its calls.
#[tool(key(description = "The key to look up"))]
fn scripted_lookup(key: String) -> String {
    SCRIPTED_TOOL_CALLS.fetch_add(1, Ordering::SeqCst);
    format!("lookup-result-for-{key}-6604")
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_custom_tool_runs_and_a_policy_can_deny_it() {
    // Allowed: dispatched once, its result reaches the model, and the
    // post-tool hook sees it exactly once.
    let fake = FakeGemini::start(vec![
        ModelTurn::call("scripted_lookup", json!({"key": "alpha"})),
        ModelTurn::text("looked up"),
    ])
    .await;
    let outcomes: Arc<Mutex<Vec<ToolOutcome>>> = Arc::default();
    let hook_outcomes = Arc::clone(&outcomes);
    let before = SCRIPTED_TOOL_CALLS.load(Ordering::SeqCst);
    let mut agent = scripted_agent(&fake)
        .add_tool(ScriptedLookupCallable.declaration())
        .on_post_tool(move |outcome| hook_outcomes.lock().unwrap().push(outcome.clone()))
        .spawn()
        .await
        .expect("spawn");
    let (_, events, response) = streamed_turn(&mut agent, "look up alpha").await;
    assert_eq!(response.text(), "looked up");
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::ToolCallDispatched { name, .. } if name == "scripted_lookup")),
        "{events:?}"
    );
    assert!(function_response_text(&fake, 1).contains("lookup-result-for-alpha-6604"));
    let outcomes = outcomes.lock().unwrap().clone();
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| o.name == "scripted_lookup")
            .count(),
        1,
        "{outcomes:?}"
    );
    agent.shutdown().await.expect("shutdown");

    // Denied by policy: the tool never runs and the model is told why.
    let fake = FakeGemini::start(vec![
        ModelTurn::call("scripted_lookup", json!({"key": "beta"})),
        ModelTurn::text("denied"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_tool(ScriptedLookupCallable.declaration())
        .add_policy(policy::deny("scripted_lookup"))
        .spawn()
        .await
        .expect("spawn");
    let calls_before_denial = SCRIPTED_TOOL_CALLS.load(Ordering::SeqCst);
    let response = agent.chat("look up beta").await.expect("turn");
    assert_eq!(response.text(), "denied");
    assert_eq!(
        SCRIPTED_TOOL_CALLS.load(Ordering::SeqCst),
        calls_before_denial,
        "a denied custom tool must not run"
    );
    assert!(calls_before_denial > before, "the allowed call ran");
    // With a policy set the harness asks the pre-tool hook before it
    // dispatches a custom tool, so the denial comes back through the hook.
    let told = function_response_text(&fake, 1);
    assert!(
        told.contains("Denied by policy for tool 'scripted_lookup'"),
        "{told}"
    );
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// A custom Gemini host gets only the configured key
// =============================================================================

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_custom_host_harness_has_no_key_in_its_environment() {
    // With a custom host the harness starts without GEMINI_API_KEY and
    // GOOGLE_API_KEY: with them it falls back to the environment's key when
    // the configured one is empty and sends it to that host (verified
    // 2026-11-04). The commands it runs inherit its environment, which makes
    // the scrub visible here. `${VAR+set}` prints "set" for a variable that
    // is defined even when empty, as CI's GEMINI_API_KEY is.
    let workspace = scratch_dir("agy-scripted-env");
    let ws = workspace.path().canonicalize().unwrap();
    let fake = FakeGemini::start(vec![
        ModelTurn::call(
            "run_command",
            json!({
                "CommandLine": "echo \"gemini=[${GEMINI_API_KEY+set}] \
                                google=[${GOOGLE_API_KEY+set}] path=[${PATH+set}]\"",
                "Cwd": ws.to_string_lossy(),
                "WaitMsBeforeAsync": 5000,
                "toolSummary": "Environment check",
                "toolAction": "Checking"
            }),
        ),
        ModelTurn::text("checked"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(ws.to_string_lossy())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand))
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat("check the environment").await.expect("turn");
    assert_eq!(response.text(), "checked");
    let told = function_response_text(&fake, 1);
    assert!(
        told.contains("path=[set]"),
        "the command must have run with the harness's environment: {told}"
    );
    assert!(told.contains("gemini=[] google=[]"), "{told}");
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Workspace containment
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_workspace_containment_denies_a_read_outside_the_workspace() {
    let workspace = scratch_dir("agy-scripted-ws");
    // Not under the system temp directory: the harness allows that root
    // besides the workspaces (verified on 0.1.23), so a file there is not
    // "outside" for containment purposes.
    let outside = tempfile::Builder::new()
        .prefix("agy-scripted-outside")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("create dir outside the temp root");
    let secret = write_file(&outside, "secret.txt", "outside-secret-2207");

    // Contained (the default with a workspace): the harness refuses the
    // read, and the content never reaches the model.
    let fake = FakeGemini::start(vec![view_file(&secret), ModelTurn::text("done")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("read it").await.expect("turn");
    let told = function_response_text(&fake, 1);
    assert!(
        told.contains("denied"),
        "containment must refuse the read: {told}"
    );
    assert!(
        !told.contains("outside-secret-2207"),
        "content leaked: {told}"
    );
    agent.shutdown().await.expect("shutdown");

    // Opted out: the same read succeeds, which pins that the refusal above
    // came from the containment this crate asks for.
    let fake = FakeGemini::start(vec![view_file(&secret), ModelTurn::text("done")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .with_workspace_containment(false)
        .spawn()
        .await
        .expect("spawn");
    agent.chat("read it").await.expect("turn");
    assert!(
        function_response_text(&fake, 1).contains("outside-secret-2207"),
        "without containment the read should succeed: {:?}",
        fake.function_responses(1)
    );
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Cancellation, triggers and errors
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_cancel_halts_a_turn_waiting_on_the_model() {
    // The first model call answers only after a minute; the cancel must
    // end the turn long before that, and the agent must stay usable.
    let fake = Arc::new(
        FakeGemini::start(vec![
            ModelTurn::text("too-late-1450").delayed(Duration::from_secs(60)),
            ModelTurn::text("after-cancel-1451"),
        ])
        .await,
    );
    let mut agent = scripted_agent(&fake).spawn().await.expect("spawn");

    let cancel = agent.cancel_handle();
    let watcher = Arc::clone(&fake);
    let canceller = tokio::spawn(async move {
        // Cancel only once the turn is provably in flight.
        assert!(
            watcher.wait_for_requests(1, Duration::from_secs(20)).await,
            "the turn never reached the model"
        );
        cancel.cancel().await.expect("cancel");
    });

    let start = Instant::now();
    let response = agent.chat("this will be cancelled").await;
    canceller.await.expect("canceller");
    let elapsed = start.elapsed();
    // A halted turn resolves normally with what it had (see CancelHandle).
    let response = response.expect("a cancelled turn resolves, not fails");
    assert!(
        !response.text().contains("too-late-1450"),
        "the delayed reply cannot have arrived: {}",
        response.text()
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "cancel took {elapsed:?}; the turn waited for the model instead"
    );

    let next = agent.chat("still there?").await.expect("next turn");
    assert_eq!(next.text(), "after-cancel-1451");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_trigger_turn_reaches_the_model_and_chat_still_works() {
    let fake = FakeGemini::start(vec![ModelTurn::text("ack-3020")]).await;
    let mut agent = scripted_agent(&fake)
        .add_trigger(TriggerConfig::new(
            "trigger-tick-6612",
            Duration::from_secs(1),
        ))
        .spawn()
        .await
        .expect("spawn");

    // The idle agent delivers the trigger; the harness runs a turn on it
    // and calls the model with the trigger's message.
    assert!(
        fake.wait_for_requests(1, Duration::from_secs(20)).await,
        "no model call followed the trigger"
    );
    let first = fake.request_json(0).unwrap().to_string();
    assert!(first.contains("trigger-tick-6612"), "{first}");

    // The trigger turn is discarded before the user's, which completes.
    let response = agent.chat("user-turn-after-trigger").await.expect("turn");
    assert_eq!(response.text(), "ack-3020");
    assert!(
        fake.requests()
            .iter()
            .any(|r| r.json().to_string().contains("user-turn-after-trigger")),
        "the user's turn never reached the model"
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_model_bad_request_fails_the_turn() {
    let fake = FakeGemini::start(vec![ModelTurn::error(400, "scripted-bad-request-4410")]).await;
    let mut agent = scripted_agent(&fake).spawn().await.expect("spawn");

    let err = agent.chat("this request is bad").await.unwrap_err();
    let AntigravityError::Turn(message) = &err else {
        panic!("a 400 from the model backend must fail the turn, got {err:?}");
    };
    assert!(message.contains("HTTP 400"), "{message}");
    assert!(message.contains("scripted-bad-request-4410"), "{message}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_failed_builtin_surfaces_an_error_event_and_continues() {
    // A tool failure is not a turn failure: the harness reports an error
    // step, the model hears about it, and the turn finishes.
    let workspace = scratch_dir("agy-scripted-missing");
    let missing = workspace.path().canonicalize().unwrap().join("missing.txt");
    let fake = FakeGemini::start(vec![
        view_file(&missing.to_string_lossy()),
        ModelTurn::text("no such file"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .spawn()
        .await
        .expect("spawn");

    let (_, events, response) = streamed_turn(&mut agent, "read a missing file").await;
    assert_eq!(response.text(), "no such file");
    let errors: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Error { .. }))
        .collect();
    assert!(!errors.is_empty(), "no error event in {events:?}");
    assert!(!response.errors().is_empty());
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// The turn deadline covers client-side work
// =============================================================================

/// A custom tool that never returns in any reasonable time.
struct HangingTool;

#[async_trait]
impl CallableFunction for HangingTool {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("hanging_tool")
            .with_description("Never returns.")
            .build()
    }

    async fn call(&self, _args: Value) -> Result<Value, FunctionError> {
        tokio::time::sleep(Duration::from_secs(600)).await;
        Ok(json!("unreachable"))
    }
}

struct HangingService;

impl ToolService for HangingService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(HangingTool)]
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_turn_deadline_bounds_a_hung_custom_tool() {
    // Regression (#499): the deadline covered only waiting for harness
    // events, so a tool that hung held the turn indefinitely.
    let fake = FakeGemini::start(vec![
        ModelTurn::call("hanging_tool", json!({})),
        ModelTurn::text("recovered-9120"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .with_tool_service(Arc::new(HangingService))
        .with_turn_timeout(Duration::from_secs(2))
        .spawn()
        .await
        .expect("spawn");

    let start = Instant::now();
    let err = agent.chat("call the hanging tool").await.unwrap_err();
    let elapsed = start.elapsed();
    let AntigravityError::Timeout { operation, .. } = &err else {
        panic!("expected Timeout, got {err:?}");
    };
    assert!(
        operation.contains("custom tool 'hanging_tool'"),
        "the timeout must name what held the turn: {operation}"
    );
    // 2s budget plus the bounded halt-and-drain, nowhere near the tool's 600s.
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_turn_deadline_bounds_a_blocking_pre_tool_hook() {
    // A hook is a synchronous closure; one that blocks used to hold the
    // runtime thread, where no timer could fire.
    let workspace = scratch_dir("agy-scripted-hook");
    let path = write_file(&workspace, "f.txt", "x");
    let fake = FakeGemini::start(vec![view_file(&path), ModelTurn::text("done")]).await;
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let blocked = Mutex::new(blocked);
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .with_turn_timeout(Duration::from_secs(2))
        .on_pre_tool(move |_| {
            // Blocks until the test releases it (or 60s pass).
            let _ = blocked
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(60));
            PreToolDecision::Allow
        })
        .spawn()
        .await
        .expect("spawn");

    let start = Instant::now();
    let err = agent.chat("read it").await.unwrap_err();
    let elapsed = start.elapsed();
    let _ = release.send(());
    let AntigravityError::Timeout { operation, .. } = &err else {
        panic!("expected Timeout, got {err:?}");
    };
    assert!(
        operation.contains("pre-tool hook for 'view_file'"),
        "the timeout must name the hook: {operation}"
    );
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Process lifecycle
// =============================================================================

/// Pids of processes whose command line contains `pattern`.
fn pids_matching(pattern: &str) -> Vec<u32> {
    let output = std::process::Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .expect("pgrep");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_dropping_the_agent_kills_its_run_command_processes() {
    // Regression (#499): drop killed only the harness's pid. The harness
    // gives each run_command command a process group of its own, so the
    // command was orphaned and kept running.
    let workspace = scratch_dir("agy-scripted-run");
    let ws = workspace.path().canonicalize().unwrap();
    // A command line no other process has.
    let command = format!("sleep 3600.{}", std::process::id());
    let fake = FakeGemini::start(vec![
        ModelTurn::call(
            "run_command",
            json!({
                "CommandLine": command,
                "Cwd": ws.to_string_lossy(),
                "WaitMsBeforeAsync": 500,
                "toolSummary": "Long sleep",
                "toolAction": "Sleeping"
            }),
        ),
        ModelTurn::text("started"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(ws.to_string_lossy())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand))
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");

    // The turn does not end while the command runs (the harness waits in
    // STATE_WAITING_FOR_TASKS), so drive the stream, which answers the
    // pre-tool hook, only until the command is up.
    let mut pids = Vec::new();
    {
        let mut stream = agent
            .send_streaming("start the long command")
            .await
            .expect("stream");
        let deadline = Instant::now() + Duration::from_secs(30);
        while pids.is_empty() && Instant::now() < deadline {
            if let Ok(Some(event)) =
                tokio::time::timeout(Duration::from_millis(200), stream.next()).await
            {
                event.expect("stream event");
            }
            pids = pids_matching(&command);
        }
    }
    assert!(!pids.is_empty(), "the command never started");
    // On macOS (0.1.23, 2026-11-04) the command leads a group of its own,
    // which signalling only the harness's group misses. Printed rather than
    // asserted: the kill below must hold whichever group it is in.
    let pgid = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &pids[0].to_string()])
        .output()
        .expect("ps");
    println!(
        "command pids {pids:?}, process group {}",
        String::from_utf8_lossy(&pgid.stdout).trim()
    );

    drop(agent);

    let deadline = Instant::now() + Duration::from_secs(10);
    while !pids_matching(&command).is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let survivors = pids_matching(&command);
    for pid in &survivors {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    assert!(
        survivors.is_empty(),
        "run_command processes outlived the dropped agent: {survivors:?}"
    );
}
