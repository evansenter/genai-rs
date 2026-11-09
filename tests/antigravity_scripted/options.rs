//! The Python SDK's harness options (#493 items 1-4), each shown changing
//! what the harness does: the model request it sends, the tools it
//! declares, or what a tool call returns.

use super::*;
use genai_rs::antigravity::protocol::StopReason;
use genai_rs::antigravity::{
    Budget, InlineSkill, RetryConfig, RunCommandOptions, SessionContinuation, Subagent,
    SystemInstructions,
};

// =============================================================================
// System instructions (item 1)
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_appended_instructions_keep_the_harness_prompt() {
    let workspace = scratch_dir("agy-scripted-si");
    let root = workspace.path().canonicalize().unwrap();
    let root = root.to_string_lossy();

    // A skill on disk, which the harness lists in its own prompt.
    let skills = scratch_dir("agy-scripted-skills");
    std::fs::create_dir_all(skills.path().join("disk-skill")).unwrap();
    std::fs::write(
        skills.path().join("disk-skill/SKILL.md"),
        "---
name: disk-skill
description: DISK-SKILL-4474 for probes.
---
Body.
",
    )
    .unwrap();

    // A string is added to the harness's prompt, which keeps its workspace
    // list (no note of ours is needed) and its skills list.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(root.as_ref())
        .add_skills_path(skills.path().to_string_lossy())
        .with_system_instructions("APPENDED-MARK-4471")
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let prompt = fake.system_prompt(0);
    assert!(
        prompt.contains("<user_system_instructions>\nAPPENDED-MARK-4471"),
        "{prompt}"
    );
    assert!(prompt.contains("Active Workspaces"), "{prompt}");
    assert!(prompt.contains(root.as_ref()), "{prompt}");
    assert!(!prompt.contains("=== Workspace ==="), "{prompt}");
    assert!(prompt.contains("DISK-SKILL-4474"), "{prompt}");
    agent.shutdown().await.expect("shutdown");

    // The harness's prompt with its identity replaced.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .with_system_instructions(
            SystemInstructions::harness_default().with_identity("You are IDENTITY-MARK-4472."),
        )
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let prompt = fake.system_prompt(0);
    assert!(
        prompt.starts_with("<identity>\nYou are IDENTITY-MARK-4472."),
        "{prompt}"
    );
    agent.shutdown().await.expect("shutdown");

    // Custom instructions replace it: no harness workspace list, so the
    // workspace note comes back.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(root.as_ref())
        .with_system_instructions(SystemInstructions::custom("CUSTOM-MARK-4473"))
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let prompt = fake.system_prompt(0);
    assert!(prompt.contains("CUSTOM-MARK-4473"), "{prompt}");
    assert!(!prompt.contains("Active Workspaces"), "{prompt}");
    assert!(prompt.contains("=== Workspace ==="), "{prompt}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_section_titles_and_the_workspace_setter_reach_the_prompt() {
    let workspace = scratch_dir("agy-scripted-ws");
    let decoy = scratch_dir("agy-scripted-ws-decoy");
    let root = workspace.path().canonicalize().unwrap();
    let decoy = decoy.path().canonicalize().unwrap();

    // A retitled appended section; `with_workspace` replaces the workspace
    // added before it.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(decoy.to_string_lossy())
        .with_workspace(root.to_string_lossy())
        .with_system_instructions(
            SystemInstructions::appended("TITLED-MARK-4475").with_title("retitled_section"),
        )
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let prompt = fake.system_prompt(0);
    assert!(
        prompt.contains("<retitled_section>\nTITLED-MARK-4475"),
        "{prompt}"
    );
    assert!(!prompt.contains("<user_system_instructions>"), "{prompt}");
    assert!(prompt.contains(root.to_string_lossy().as_ref()), "{prompt}");
    assert!(
        !prompt.contains(decoy.to_string_lossy().as_ref()),
        "{prompt}"
    );
    agent.shutdown().await.expect("shutdown");

    // A retitled custom prompt: its tag is the title, not `System`.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .with_system_instructions(
            SystemInstructions::custom("CUSTOM-TITLED-4476").with_title("custom_title"),
        )
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let prompt = fake.system_prompt(0);
    assert!(prompt.contains("<custom_title>"), "{prompt}");
    assert!(prompt.contains("CUSTOM-TITLED-4476"), "{prompt}");
    assert!(!prompt.contains("<System>"), "{prompt}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_capabilities_choose_the_declared_builtins() {
    let cases = [
        (Capabilities::read_only(), "view_file", true),
        (
            Capabilities::read_only().disable(BuiltinTool::ViewFile),
            "view_file",
            false,
        ),
        (Capabilities::read_only(), "run_command", false),
        (
            Capabilities::read_only().enable(BuiltinTool::RunCommand),
            "run_command",
            true,
        ),
    ];
    for (capabilities, tool, declared) in cases {
        let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
        let mut agent = scripted_agent(&fake)
            .with_capabilities(capabilities)
            .add_policy(policy::allow_all())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("hi").await.expect("turn");
        let tools = fake.tool_names(0);
        assert_eq!(
            tools.iter().any(|t| t == tool),
            declared,
            "{tool}: {tools:?}"
        );
        agent.shutdown().await.expect("shutdown");
    }
}

// =============================================================================
// Model options (item 2)
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_model_options_reach_the_model_request() {
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .with_thinking_level(genai_rs::ThinkingLevel::Low)
        .with_service_tier(genai_rs::ServiceTier::Flex)
        .with_max_output_tokens(1234)
        .add_model_header("x-scripted-header", "header-value-7")
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let request = fake.request_json(0).unwrap();
    assert_eq!(request["generationConfig"]["maxOutputTokens"], 1234);
    assert_eq!(
        request["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "low"
    );
    assert_eq!(request["serviceTier"], "flex");
    assert_eq!(
        fake.requests()[0].header("x-scripted-header"),
        Some("header-value-7")
    );
    agent.shutdown().await.expect("shutdown");

    // The harness's fifth level, which the Interactions API's ThinkingLevel
    // does not name, passes through as an unknown value.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .with_thinking_level(genai_rs::ThinkingLevel::Unknown {
            level_type: "extra_high".to_string(),
            data: json!("extra_high"),
        })
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    assert_eq!(
        fake.request_json(0).unwrap()["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "extra_high"
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_generate_image_calls_the_image_model() {
    // Without an image model config the harness fails every generate_image
    // call ("model is empty", 0.1.23); the bridge now sends one.
    let workspace = scratch_dir("agy-scripted-image");
    let fake = FakeGemini::routed(|request, _| {
        if request.target.contains(":generateContent") {
            ModelTurn::error(500, "scripted image backend")
        } else if request.json().to_string().contains("functionResponse") {
            ModelTurn::text("drew it")
        } else {
            ModelTurn::call(
                "generate_image",
                json!({"Prompt": "a red square", "ImageName": "red_square"}),
            )
        }
    })
    .await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .with_image_model("scripted-image-model")
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::GenerateImage))
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("draw").await.expect("turn");
    assert!(
        fake.requests().iter().any(|r| r
            .target
            .starts_with("/v1beta/models/scripted-image-model:generateContent")),
        "the image model was never called: {:?}",
        fake.requests()
            .iter()
            .map(|r| &r.target)
            .collect::<Vec<_>>()
    );
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Harness config (item 4)
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_budget_stops_the_turn_with_its_stop_reason() {
    let workspace = scratch_dir("agy-scripted-budget");
    let a = write_file(&workspace, "a.txt", "a");
    let b = write_file(&workspace, "b.txt", "b");
    let fake = FakeGemini::start(vec![view_file(&a), view_file(&b), ModelTurn::text("x")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .with_budget(Budget::new().with_max_tool_calls(1))
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat("read both").await.expect("turn");
    assert_eq!(
        response.stop_reason(),
        Some(&StopReason::MaxToolCallsExceeded)
    );
    // The second call never ran.
    assert_eq!(fake.requests().len(), 1, "the cap ends the turn");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_retry_config_decides_whether_a_503_ends_the_turn() {
    let unavailable = || ModelTurn::error(503, "scripted-unavailable");
    // No retries: the first 503 ends the turn.
    let fake = FakeGemini::start(vec![unavailable(), ModelTurn::text("recovered-2201")]).await;
    let mut agent = scripted_agent(&fake)
        .with_retry(RetryConfig::new().with_max_api_retries(0))
        .spawn()
        .await
        .expect("spawn");
    let err = agent.chat("hi").await.unwrap_err();
    let AntigravityError::Turn { kind, .. } = &err else {
        panic!("a 503 with no retries left must fail the turn: {err:?}");
    };
    assert_eq!(*kind, TurnFailure::ModelBackend { status: 503 });
    // The harness's run ended with the turn, as `ends_session()` says.
    assert!(kind.ends_session());
    let next = agent.chat("again").await.unwrap_err();
    assert!(
        matches!(
            next,
            AntigravityError::WebSocket { .. } | AntigravityError::ConnectionClosed { .. }
        ),
        "the call after a 503 that ended the turn must fail on the connection, got {next:?}"
    );
    drop(agent);

    // Retries: the harness calls again and the turn recovers.
    let fake = FakeGemini::start(vec![unavailable(), ModelTurn::text("recovered-2201")]).await;
    let mut agent = scripted_agent(&fake)
        .with_retry(
            RetryConfig::new()
                .with_max_api_retries(2)
                .with_initial_backoff(Duration::from_millis(10)),
        )
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat("hi").await.expect("turn");
    assert_eq!(response.text(), "recovered-2201");
    agent.shutdown().await.expect("shutdown");
}

/// A `run_command` call as the model makes it. Unix only, as the tests that
/// use it are.
#[cfg(unix)]
fn run_command(command: &str, cwd: &std::path::Path, wait_ms: u64) -> ModelTurn {
    ModelTurn::call(
        "run_command",
        json!({
            "CommandLine": command,
            "Cwd": cwd.to_string_lossy(),
            "WaitMsBeforeAsync": wait_ms,
            "toolSummary": "Command",
            "toolAction": "Running"
        }),
    )
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_run_command_options_and_truncation_change_what_runs() {
    let workspace = scratch_dir("agy-scripted-cmd");
    let ws = workspace.path().canonicalize().unwrap();
    let words = "for i in $(seq 1 200); do printf 'word%03d ' $i; done";
    let agent_with = |fake: &FakeGemini, caps: Capabilities| {
        scripted_agent(fake)
            .add_workspace(ws.to_string_lossy())
            .with_capabilities(caps)
            .add_policy(policy::allow_all())
    };
    let shell = || Capabilities::read_only().enable(BuiltinTool::RunCommand);

    // Truncation cuts a long output; daemons put IsDaemon on the schema.
    let fake =
        FakeGemini::start(vec![run_command(words, &ws, 5000), ModelTurn::text("done")]).await;
    let mut agent = agent_with(
        &fake,
        shell().with_run_command(RunCommandOptions::new().with_daemons(true)),
    )
    .with_tool_output_truncation(10)
    .spawn()
    .await
    .expect("spawn");
    agent.chat("print").await.expect("turn");
    let told = function_response_text(&fake, 1);
    assert!(told.contains("truncated"), "{told}");
    assert!(!told.contains("word001"), "{told}");
    let schema = fake.request_json(0).unwrap().to_string();
    assert!(schema.contains("IsDaemon"), "daemons expose IsDaemon");
    agent.shutdown().await.expect("shutdown");

    // The same command untruncated, and without IsDaemon.
    let fake =
        FakeGemini::start(vec![run_command(words, &ws, 5000), ModelTurn::text("done")]).await;
    let mut agent = agent_with(&fake, shell()).spawn().await.expect("spawn");
    agent.chat("print").await.expect("turn");
    assert!(function_response_text(&fake, 1).contains("word001"));
    assert!(
        !fake
            .request_json(0)
            .unwrap()
            .to_string()
            .contains("IsDaemon")
    );
    agent.shutdown().await.expect("shutdown");

    // A command past its timeout is killed and reported.
    let fake = FakeGemini::start(vec![
        run_command("sleep 30; echo slept", &ws, 10_000),
        ModelTurn::text("gave up"),
    ])
    .await;
    let mut agent = agent_with(
        &fake,
        shell()
            .with_run_command(RunCommandOptions::new().with_timeout(Duration::from_millis(1500))),
    )
    .spawn()
    .await
    .expect("spawn");
    let start = Instant::now();
    agent.chat("sleep").await.expect("turn");
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
    let told = function_response_text(&fake, 1);
    assert!(told.contains("timed out"), "{told}");
    assert!(!told.contains("slept"), "{told}");
    agent.shutdown().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_harness_env_reaches_commands() {
    let workspace = scratch_dir("agy-scripted-env2");
    let ws = workspace.path().canonicalize().unwrap();
    let fake = FakeGemini::start(vec![
        run_command("echo \"probe=[$AGY_SCRIPTED_PROBE]\"", &ws, 5000),
        ModelTurn::text("checked"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(ws.to_string_lossy())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand))
        .add_policy(policy::allow_all())
        .add_harness_env("AGY_SCRIPTED_PROBE", "env-value-6630")
        .spawn()
        .await
        .expect("spawn");
    agent.chat("check").await.expect("turn");
    let told = function_response_text(&fake, 1);
    assert!(told.contains("probe=[env-value-6630]"), "{told}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_compaction_threshold_compacts_and_fires_the_hook() {
    // The threshold must clear the harness's own prompt and tools (about
    // 2,700 tokens on 0.1.23) and a long first turn must pass it.
    let long_prompt = (0..1500)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let fake = FakeGemini::start(vec![
        ModelTurn::text("first-answer"),
        ModelTurn::text("SUMMARY-MARK-8812"),
        ModelTurn::text("second-answer"),
    ])
    .await;
    let summaries: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = Arc::clone(&summaries);
    let mut agent = scripted_agent(&fake)
        .with_compaction_threshold(4000)
        .on_compaction(move |event| seen.lock().unwrap().push(event.summary.clone()))
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat(long_prompt).await.expect("turn");
    assert_eq!(response.text(), "first-answer");
    assert_eq!(
        summaries.lock().unwrap().as_slice(),
        ["SUMMARY-MARK-8812"],
        "the compaction hook gets the model's summary"
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_inline_skills_reach_the_model() {
    let fake = FakeGemini::start(vec![
        ModelTurn::call("lookup_skill", json!({"skill_name": "scripted-skill"})),
        ModelTurn::text("looked"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_inline_skill(InlineSkill::new(
            "scripted-skill",
            "SKILL-DESCRIPTION-3301",
            "SKILL-CONTENT-3302",
        ))
        .spawn()
        .await
        .expect("spawn");
    agent.chat("use the skill").await.expect("turn");
    let tools = fake.tool_names(0);
    assert!(tools.contains(&"lookup_skill".to_string()), "{tools:?}");
    assert!(fake.system_prompt(0).contains("SKILL-DESCRIPTION-3301"));
    assert!(
        function_response_text(&fake, 1).contains("SKILL-CONTENT-3302"),
        "{}",
        function_response_text(&fake, 1)
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_response_schema_is_the_finish_tools_parameters() {
    let schema = json!({
        "type": "object",
        "properties": {"code": {"type": "integer"}},
        "required": ["code"]
    });
    let fake = FakeGemini::start(vec![
        ModelTurn::call("finish", json!({"code": 4417})),
        ModelTurn::text("unreached"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .with_response_schema(schema.clone())
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat("answer").await.expect("turn");
    let request = fake.request_json(0).unwrap();
    let finish = request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["functionDeclarations"].as_array())
        .flatten()
        .find(|declaration| declaration["name"] == "finish")
        .unwrap_or_else(|| panic!("no finish tool: {request}"));
    assert_eq!(finish["parametersJsonSchema"], schema, "{finish}");
    assert_eq!(response.structured_output(), Some(&json!({"code": 4417})));
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_app_data_dir_holds_the_conversations_working_files() {
    let app_data = scratch_dir("agy-scripted-appdata");
    let dir = app_data.path().to_string_lossy().into_owned();
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .with_app_data_dir(&dir)
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let conversation = agent
        .conversation_id()
        .expect("a conversation id")
        .to_string();
    agent.shutdown().await.expect("shutdown");

    let prompt = fake.system_prompt(0);
    assert!(
        prompt.contains(&format!("App Data Directory: {dir}")),
        "{prompt}"
    );
    let transcript = app_data.path().join(format!(
        "brain/{conversation}/.system_generated/logs/transcript.jsonl"
    ));
    assert!(
        transcript.is_file(),
        "the harness keeps the conversation's transcript under the app data directory: {}",
        transcript.display()
    );
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_session_continuation_resume_needs_a_saved_conversation() {
    let save_dir = scratch_dir("agy-scripted-resume");
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let err = scripted_agent(&fake)
        .with_save_dir(save_dir.path().to_string_lossy())
        .with_conversation_id("0000000000000000000000000000000000ab")
        .with_session_continuation(SessionContinuation::Resume)
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::InitFailed { stderr, .. } = &err else {
        panic!("a missing conversation must fail init, got {err:?}");
    };
    assert!(stderr.contains("cannot resume"), "{stderr}");
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_sandbox_confines_what_commands_write() {
    // The sandbox lets a command write to the workspace and the temp
    // directories, so the probe writes under `target/` instead: outside
    // both, and gitignored.
    let workspace = scratch_dir("agy-scripted-sandbox");
    let ws = workspace.path().canonicalize().unwrap();
    let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    let outside = tempfile::Builder::new()
        .prefix("agy-sandbox-probe")
        .tempdir_in(&target_dir)
        .expect("create probe dir");
    let file = outside.path().canonicalize().unwrap().join("written.txt");
    let write = format!("echo hi > '{}' && echo WROTE-6620", file.display());

    for sandbox in [false, true] {
        let _ = std::fs::remove_file(&file);
        let fake = FakeGemini::start(vec![
            run_command(&write, &ws, 5000),
            ModelTurn::text("done"),
        ])
        .await;
        let mut agent = scripted_agent(&fake)
            .add_workspace(ws.to_string_lossy())
            .with_capabilities(
                Capabilities::read_only()
                    .enable(BuiltinTool::RunCommand)
                    .with_run_command(RunCommandOptions::new().with_sandbox(sandbox)),
            )
            .add_policy(policy::allow_all())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("write").await.expect("turn");
        let available = agent
            .sandbox_status()
            .expect("0.1.23 reports a sandbox status")
            .available;
        let told = function_response_text(&fake, 1);
        println!("sandbox={sandbox}, available={available:?}");
        if sandbox && available == Some(true) {
            assert!(!file.exists(), "the sandbox let the write through: {told}");
            assert!(!told.contains("WROTE-6620"), "{told}");
        } else if sandbox {
            // Sandbox asked for on a host where the harness has none (GitHub's
            // Ubuntu 24.04 runners report `available: false`): the command
            // fails instead of running unsandboxed (CI, 2026-11-05).
            assert!(
                !file.exists(),
                "an unavailable sandbox ran the write: {told}"
            );
            assert!(told.contains("\"error\""), "expected a tool error: {told}");
        } else {
            // Unsandboxed: the write runs.
            assert!(file.exists(), "{told}");
            assert!(told.contains("WROTE-6620"), "{told}");
        }
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_every_option_together_is_accepted_and_runs_a_turn() {
    // One config carrying every option at once: the harness must accept it
    // at init and run a turn under it.
    let workspace = scratch_dir("agy-scripted-all");
    let fake = FakeGemini::start(vec![ModelTurn::text("all-options-6601")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .with_system_instructions(
            SystemInstructions::harness_default()
                .with_identity("You are a scripted agent.")
                .add_section("style", "Be brief."),
        )
        .with_thinking_level(genai_rs::ThinkingLevel::High)
        .with_service_tier(genai_rs::ServiceTier::Standard)
        .with_max_output_tokens(2048)
        .with_image_model("scripted-image-model")
        .add_model_header("x-scripted", "1")
        .with_capabilities(
            Capabilities::read_only()
                .enable(BuiltinTool::InvokeSubagent)
                .enable(BuiltinTool::RunCommand)
                .with_run_command(
                    RunCommandOptions::new()
                        .with_timeout(Duration::from_secs(30))
                        .with_daemons(true)
                        .with_sandbox(true),
                ),
        )
        .add_subagent(
            Subagent::new("helper")
                .with_description("Helps.")
                .with_model("scripted-subagent-model")
                .with_skills(genai_rs::antigravity::SubagentSkills::none()),
        )
        .with_max_subagent_depth(2)
        .with_allowed_subagents(["helper"])
        .with_ungated_subagents(true)
        .with_budget(Budget::new().with_max_model_calls(10))
        .with_retry(RetryConfig::new().with_max_api_retries(1))
        .with_compaction_threshold(100_000)
        .with_tool_output_truncation(2000)
        .with_mcp_max_output_bytes(8192)
        .add_inline_skill(InlineSkill::new("s", "A skill.", "Body."))
        .with_session_continuation(SessionContinuation::CreateOrResume)
        .add_harness_env("AGY_SCRIPTED_ALL", "1")
        .add_policy(policy::allow_all())
        .add_policy(policy::auto())
        .on_session_start(|| {})
        .on_stop(|_| genai_rs::antigravity::StopDecision::Stop)
        .spawn()
        .await
        .expect("the harness accepts every option at init");
    let response = agent.chat("hi").await.expect("turn");
    assert_eq!(response.text(), "all-options-6601");
    agent.shutdown().await.expect("shutdown");
}

// =============================================================================
// Prompts: media and slash commands (Python: `Agent.chat(types.Content)`)
// =============================================================================

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_prompt_media_and_slash_commands_reach_the_model() {
    use genai_rs::antigravity::{Media, Prompt, SlashCommand};

    // Media goes to the model as inline data, after the text.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake).spawn().await.expect("spawn");
    let prompt = Prompt::new().add_text("PROMPT-TEXT-4401").add_media(
        Media::new(b"PNG-BYTES-4402".to_vec(), "image/png")
            .with_description("DESCRIPTION-MARK-4404"),
    );
    agent.chat(prompt).await.expect("turn");
    let request = fake.request_json(0).unwrap();
    // Sent to the harness, which does not pass it on (0.1.23): when this
    // fails, the harness has started to, and the parity table and
    // `Media::with_description` should say so.
    assert!(
        !request.to_string().contains("DESCRIPTION-MARK-4404"),
        "{request}"
    );
    let parts = &request["contents"][0]["parts"];
    assert!(
        parts[0]["text"]
            .as_str()
            .is_some_and(|text| text.contains("PROMPT-TEXT-4401")),
        "{parts}"
    );
    assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png", "{parts}");
    assert_eq!(
        parts[1]["inlineData"]["data"], "UE5HLUJZVEVTLTQ0MDI=",
        "base64 of the bytes: {parts}"
    );
    agent.shutdown().await.expect("shutdown");

    // A slash command reaches the model as its `/name` before the text.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake).spawn().await.expect("spawn");
    let prompt = Prompt::new()
        .add_slash_command(SlashCommand::Plan)
        .add_text("PLAN-TEXT-4403");
    agent.chat(prompt).await.expect("turn");
    let text = fake.request_json(0).unwrap()["contents"][0]["parts"][0]["text"].to_string();
    assert!(text.contains("/plan PLAN-TEXT-4403"), "{text}");
    agent.shutdown().await.expect("shutdown");
}
