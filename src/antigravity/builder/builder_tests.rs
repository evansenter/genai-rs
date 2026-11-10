use super::wire::workspace_announcement;
use super::*;
use crate::antigravity::{BuiltinTool, PreToolDecision, policy};

#[tokio::test]
async fn test_spawn_requires_policy_for_write_tools() {
    let err = AntigravityAgent::builder()
        .with_capabilities(Capabilities::all())
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected Config error, got {err:?}");
    };
    assert!(message.contains("safety"));
    assert!(message.contains("allow_all"));
}

#[tokio::test]
async fn test_spawn_requires_policy_for_mcp_servers() {
    let err = AntigravityAgent::builder()
        .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]))
        .spawn()
        .await
        .unwrap_err();
    assert!(matches!(err, AntigravityError::Config(_)));
}

/// A harness path that exists but cannot be executed, so `spawn()`
/// gets past the safety gate and discovery, then fails fast at process
/// launch (without ever falling through to a real installed harness).
fn unexecutable_harness() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("localharness");
    std::fs::write(&path, b"not a binary").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    (dir, path)
}

/// A stand-in harness that completes the stdio handshake with a WebSocket
/// port nothing listens on (1), then waits for stdin to close.
#[cfg(unix)]
fn harness_with_a_dead_port() -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("localharness");
    // The frame: a little-endian length of 2, then OutputConfig's field 1
    // (port) as the varint 1.
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf '\\002\\000\\000\\000\\010\\001'\ncat >/dev/null\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, path)
}

#[cfg(unix)]
#[tokio::test]
async fn test_a_failed_connect_is_init_failed_with_the_websocket_error_as_its_source() {
    let (_dir, path) = harness_with_a_dead_port();
    let err = AntigravityAgent::builder()
        .with_harness_path(&path)
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::InitFailed { message, .. } = &err else {
        panic!("expected InitFailed, got {err:?}");
    };
    let source = std::error::Error::source(&err).expect("the connect error is the source");
    let Some(AntigravityError::WebSocket {
        message: connect, ..
    }) = source.downcast_ref()
    else {
        panic!("the source is the WebSocket error: {source:?}");
    };
    assert!(connect.contains("port 1"), "{connect}");
    assert!(
        !err.to_string().contains(connect.as_str()),
        "Display leaves the cause to source(): {message}"
    );
}

#[tokio::test]
async fn test_spawn_write_tools_allowed_with_policy() {
    let (_dir, path) = unexecutable_harness();
    // With a policy the safety gate passes; the spawn then fails at
    // process launch, proving we got past the Config check.
    let err = AntigravityAgent::builder()
        .with_harness_path(&path)
        .with_capabilities(Capabilities::all())
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .unwrap_err();
    assert!(
        matches!(err, AntigravityError::HandshakeFailed { .. }),
        "unexpected error: {err:?}"
    );
}

#[tokio::test]
async fn test_spawn_write_tools_allowed_with_pre_tool_hook() {
    let (_dir, path) = unexecutable_harness();
    // Without subagents: a hook beside them needs with_ungated_subagents.
    let err = AntigravityAgent::builder()
        .with_harness_path(&path)
        .with_capabilities(Capabilities::all().disable(BuiltinTool::InvokeSubagent))
        .on_pre_tool(|_| PreToolDecision::Allow)
        .spawn()
        .await
        .unwrap_err();
    assert!(!matches!(err, AntigravityError::Config(_)));
}

#[tokio::test]
async fn test_spawn_model_without_api_key_is_config_error() {
    let err = AntigravityAgent::builder()
        .with_model(crate::DEFAULT_MODEL)
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected Config error, got {err:?}");
    };
    assert!(message.contains("with_api_key"));
}

#[test]
fn test_add_tool_service_appends() {
    struct NoTools;
    impl ToolService for NoTools {
        fn tools(&self) -> Vec<Arc<dyn crate::CallableFunction>> {
            Vec::new()
        }
    }
    let builder = AgentBuilder::default()
        .add_tool_service(Arc::new(NoTools))
        .add_tool_service(Arc::new(NoTools));
    assert_eq!(
        builder.tool_services.len(),
        2,
        "a second call keeps the first"
    );
}

#[test]
fn turn_budget_resolves_unset_to_the_default_and_opt_out_to_none() {
    // The distinction that matters: "never chose" must not be the same
    // as "chose unlimited", or the default could not exist at all.
    assert_eq!(
        TurnBudget::Unset.resolve(),
        Some(DEFAULT_TURN_TIMEOUT),
        "an untouched builder must carry a budget — an unbounded turn hangs \
         rather than errors when the harness stops signalling completion"
    );
    assert_eq!(TurnBudget::Unlimited.resolve(), None);
    let explicit = Duration::from_secs(7);
    assert_eq!(TurnBudget::Explicit(explicit).resolve(), Some(explicit));

    // And the builder wires each spelling to the right state.
    assert_eq!(AgentBuilder::default().turn_timeout, TurnBudget::Unset);
    assert_eq!(
        AgentBuilder::default()
            .with_turn_timeout(explicit)
            .turn_timeout,
        TurnBudget::Explicit(explicit)
    );
    assert_eq!(
        AgentBuilder::default().without_turn_timeout().turn_timeout,
        TurnBudget::Unlimited
    );
}

#[test]
fn harness_config_falls_back_to_a_real_model_when_none_is_set() {
    // `with_model` is optional, so this fallback is the id the harness
    // is actually asked to run. The model sweep that introduced
    // DEFAULT_MODEL briefly replaced it with the synthetic "test-model"
    // used by the serialization tests — which the literal guard cannot
    // catch, because removing a literal is exactly what it wants.
    let builder = AntigravityAgent::builder().with_api_key("test-key");
    let dispatcher = ToolDispatcher::new(builder.tools.clone(), &builder.tool_services);
    let config = builder.build_harness_config(&dispatcher);

    assert_eq!(config.models[0].name.as_deref(), Some(crate::DEFAULT_MODEL));
}

#[test]
fn test_builder_harness_config_assembly() {
    let builder = AntigravityAgent::builder()
        .with_api_key("test-key")
        .with_model(crate::DEFAULT_MODEL)
        .with_system_instructions("Be brief.")
        .add_workspace("/w1")
        .add_workspace("/w2")
        .with_conversation_id("resume-me")
        .with_response_schema(serde_json::json!({"type": "object"}))
        .with_app_data_dir("/data")
        .add_skills_path("/skills")
        .add_policy(policy::allow_all())
        .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]).with_name("git"))
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand));
    let dispatcher = ToolDispatcher::new(builder.tools.clone(), &builder.tool_services);
    let config = builder.build_harness_config(&dispatcher);

    assert_eq!(config.cascade_id.as_deref(), Some("resume-me"));
    assert_eq!(config.models.len(), 1);
    let model = &config.models[0];
    assert_eq!(model.name.as_deref(), Some(crate::DEFAULT_MODEL));
    assert_eq!(model.types, vec![protocol::ModelType::Text]);
    assert_eq!(
        model
            .gemini_api_endpoint
            .as_ref()
            .unwrap()
            .api_key
            .as_deref(),
        Some("test-key")
    );
    assert_eq!(config.workspaces.len(), 2);
    assert_eq!(
        config.finish_tool_schema_json.as_deref(),
        Some(r#"{"type":"object"}"#)
    );
    assert_eq!(config.app_data_dir.as_deref(), Some("/data"));
    assert_eq!(config.skills_paths, vec!["/skills"]);
    assert_eq!(config.mcp_servers[0].name.as_deref(), Some("git"));
    // Policies enable the pre-tool lifecycle hook.
    assert_eq!(config.enabled_hooks, vec![protocol::LifecycleHook::PreTool]);
    // A plain string is added to the harness's prompt (Python parity),
    // with no workspace note: the harness's prompt lists the workspaces.
    let instructions = config.system_instructions.as_ref().unwrap();
    assert!(instructions.custom.is_none());
    let sections = &instructions.appended.as_ref().unwrap().appended_sections;
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].content.as_deref(), Some("Be brief."));
    // run_command was enabled on top of the read-only set.
    let side_tools = config.harness_side_tools.as_ref().unwrap();
    assert!(side_tools.run_command.as_ref().unwrap().enabled);
    assert!(!side_tools.file_edit.unwrap().enabled);
}

#[tokio::test]
async fn test_spawn_rejects_zero_interval_trigger() {
    let err = AntigravityAgent::builder()
        .add_trigger(TriggerConfig::new("tick", Duration::ZERO))
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected Config error, got {err:?}");
    };
    assert!(message.contains("non-zero"));
    assert!(message.contains("tick"));
}

#[test]
fn test_builder_add_trigger_accumulates() {
    let builder = AntigravityAgent::builder()
        .add_trigger(TriggerConfig::new("a", Duration::from_secs(1)))
        .add_trigger(TriggerConfig::new("b", Duration::from_secs(2)));
    assert_eq!(builder.triggers.len(), 2);
    assert_eq!(builder.triggers[0].message, "a");
    assert_eq!(builder.triggers[1].interval, Duration::from_secs(2));
}

#[tokio::test]
async fn test_spawn_rejects_subagent_with_unregistered_tool() {
    let err = AntigravityAgent::builder()
        .add_subagent(Subagent::new("auditor").add_tool("not_registered"))
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected Config error, got {err:?}");
    };
    assert!(message.contains("auditor"));
    assert!(message.contains("not_registered"));
    assert!(message.contains("add_tool"));
}

#[tokio::test]
async fn test_spawn_rejects_duplicate_subagent_names() {
    let err = AntigravityAgent::builder()
        .add_subagent(Subagent::new("twin"))
        .add_subagent(Subagent::new("twin"))
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected Config error, got {err:?}");
    };
    assert!(message.contains("twin"));
    assert!(message.contains("unique"));
}

#[test]
fn test_builder_subagents_reach_harness_config() {
    let declaration = crate::FunctionDeclaration::builder("severity_classifier")
        .with_description("Classifies severity.")
        .build();
    let builder = AntigravityAgent::builder()
        .add_tool(declaration)
        .add_subagent(
            Subagent::new("auditor")
                .with_description("Audits files.")
                .add_tool("severity_classifier"),
        );
    let dispatcher = ToolDispatcher::new(builder.tools.clone(), &builder.tool_services);
    let config = builder.build_harness_config(&dispatcher);

    assert_eq!(config.custom_subagents.len(), 1);
    let subagent = &config.custom_subagents[0];
    assert_eq!(subagent.name.as_deref(), Some("auditor"));
    assert_eq!(subagent.description.as_deref(), Some("Audits files."));
    // The subagent carries the parent's full tool declaration.
    assert_eq!(subagent.tools.len(), 1);
    assert_eq!(
        subagent.tools[0].name.as_deref(),
        Some("severity_classifier")
    );
    assert!(subagent.tools[0].parameters_json_schema.is_some());
}

#[test]
fn test_builder_gemini_base_url_reaches_the_model_endpoint() {
    let builder = AntigravityAgent::builder()
        .with_api_key("test-key")
        .with_gemini_base_url("http://127.0.0.1:9");
    let config = builder.build_harness_config(&ToolDispatcher::new(vec![], &[]));
    let endpoint = config.models[0]
        .gemini_api_endpoint
        .as_ref()
        .expect("a Gemini endpoint");
    assert_eq!(endpoint.base_url.as_deref(), Some("http://127.0.0.1:9"));
    assert_eq!(endpoint.api_key.as_deref(), Some("test-key"));

    // Unset, the field stays off the wire and the harness uses its default.
    let default = AntigravityAgent::builder()
        .with_api_key("test-key")
        .build_harness_config(&ToolDispatcher::new(vec![], &[]));
    let endpoint = default.models[0].gemini_api_endpoint.as_ref().unwrap();
    assert_eq!(endpoint.base_url, None);
}

#[tokio::test]
async fn test_spawn_gemini_base_url_without_api_key_is_config_error() {
    // Without a key the harness sends its environment's GEMINI_API_KEY to
    // the custom host (verified 2026-11-04), so this must not get as far
    // as spawning it.
    let err = AntigravityAgent::builder()
        .with_gemini_base_url("http://127.0.0.1:9")
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected Config error, got {err:?}");
    };
    assert!(message.contains("with_api_key"), "got: {message}");
    assert!(message.contains("GEMINI_API_KEY"), "got: {message}");
}

#[tokio::test]
async fn test_spawn_gemini_base_url_with_a_blank_api_key_is_config_error() {
    // Regression: `with_api_key("")` passed an is_none() check, went on the
    // wire as an unset key, and the harness sent its environment's
    // GEMINI_API_KEY to the custom host (verified 2026-11-04). The usual
    // way to get here is `env::var("X").unwrap_or_default()`.
    for blank in ["", "   "] {
        let err = AntigravityAgent::builder()
            .with_api_key(blank)
            .with_gemini_base_url("http://127.0.0.1:9")
            .spawn()
            .await
            .unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("expected Config error for {blank:?}, got {err:?}");
        };
        assert!(message.contains("non-empty key"), "got: {message}");
    }
}

#[tokio::test]
async fn test_spawn_model_with_a_blank_api_key_is_config_error() {
    let err = AntigravityAgent::builder()
        .with_api_key("")
        .with_model(crate::DEFAULT_MODEL)
        .spawn()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AntigravityError::Config(m) if m.contains("with_api_key")),
        "got {err:?}"
    );
}

#[test]
fn test_builder_blank_api_key_sends_no_models() {
    let config = AntigravityAgent::builder()
        .with_api_key(" ")
        .build_harness_config(&ToolDispatcher::new(vec![], &[]));
    assert!(
        config.models.is_empty(),
        "a blank key must not reach the wire as a configured model"
    );
}

#[test]
fn test_builder_no_api_key_sends_no_models() {
    let builder = AntigravityAgent::builder();
    let dispatcher = ToolDispatcher::new(vec![], &[]);
    let config = builder.build_harness_config(&dispatcher);
    assert!(config.models.is_empty());
    assert!(config.enabled_hooks.is_empty());
}

#[test]
fn test_builder_with_workspace_replaces() {
    let builder = AntigravityAgent::builder()
        .add_workspace("/a")
        .with_workspace("/only");
    assert_eq!(builder.workspaces, vec!["/only"]);
}

#[test]
fn test_post_tool_hook_enables_post_tool_lifecycle_hook() {
    let builder = AntigravityAgent::builder().on_post_tool(|_| {});
    let dispatcher = ToolDispatcher::new(vec![], &[]);
    let config = builder.build_harness_config(&dispatcher);
    assert_eq!(
        config.enabled_hooks,
        vec![protocol::LifecycleHook::PostTool]
    );
}

// -------------------------------------------------------------------
// Workspace announcement (Item: announce workspace roots)
// -------------------------------------------------------------------

#[test]
fn test_workspace_announcement_lists_roots() {
    let note = workspace_announcement(&["/a".to_string(), "/b".to_string()]);
    assert!(note.contains("/a"));
    assert!(note.contains("/b"));
    assert!(note.contains("Workspace"));
}

#[test]
fn test_appended_instructions_keep_the_harness_prompt() {
    // #493 item 1: plain text is appended, never `custom`, and needs no
    // workspace note (the harness's own prompt lists the workspaces,
    // verified on 0.1.23).
    let builder = AntigravityAgent::builder()
        .with_system_instructions("Audit it.")
        .add_workspace("/repo");
    let config = builder.build_harness_config(&ToolDispatcher::new(vec![], &[]));
    let instructions = config.system_instructions.unwrap();
    assert!(instructions.custom.is_none(), "{instructions:?}");
    let appended = instructions.appended.unwrap();
    assert_eq!(appended.appended_sections.len(), 1);
    assert_eq!(
        appended.appended_sections[0].title.as_deref(),
        Some("user_system_instructions")
    );

    // No instructions at all: none sent, so the harness prompt is whole.
    let config = AntigravityAgent::builder()
        .add_workspace("/repo")
        .build_harness_config(&ToolDispatcher::new(vec![], &[]));
    assert!(config.system_instructions.is_none());
}

#[test]
fn test_builder_announces_workspace_with_custom_instructions() {
    let builder = AntigravityAgent::builder()
        .with_system_instructions(SystemInstructions::custom("Audit it."))
        .add_workspace("/repo");
    let dispatcher = ToolDispatcher::new(vec![], &[]);
    let config = builder.build_harness_config(&dispatcher);
    let parts = config.system_instructions.unwrap().custom.unwrap().part;
    assert_eq!(parts[0].text.as_deref(), Some("Audit it."));
    assert!(parts[1].text.as_ref().unwrap().contains("/repo"));
}

#[test]
fn test_builder_workspace_announcement_opt_out() {
    let builder = AntigravityAgent::builder()
        .with_system_instructions(SystemInstructions::custom("Audit it."))
        .add_workspace("/repo")
        .with_workspace_announcement(false);
    let dispatcher = ToolDispatcher::new(vec![], &[]);
    let config = builder.build_harness_config(&dispatcher);
    // Just the user's instruction, no announcement part.
    let parts = config.system_instructions.unwrap().custom.unwrap().part;
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].text.as_deref(), Some("Audit it."));
}

#[test]
fn test_builder_no_workspace_no_announcement() {
    // No workspace → nothing to announce, instructions untouched.
    let builder =
        AntigravityAgent::builder().with_system_instructions(SystemInstructions::custom("Hi"));
    let dispatcher = ToolDispatcher::new(vec![], &[]);
    let config = builder.build_harness_config(&dispatcher);
    let parts = config.system_instructions.unwrap().custom.unwrap().part;
    assert_eq!(parts.len(), 1);
}

#[test]
fn test_builder_contains_workspaces_by_default() {
    // The harness contains file tools only when a policy config is sent.
    let builder = AntigravityAgent::builder().add_workspace("/repo");
    let config = builder.build_harness_config(&ToolDispatcher::new(vec![], &[]));
    assert_eq!(
        config.policy_config,
        Some(protocol::PolicyConfig::default())
    );
    let wire = serde_json::to_value(&config).unwrap();
    assert_eq!(wire["policyConfig"], serde_json::json!({}));
}

#[test]
fn test_builder_workspace_containment_opt_out() {
    let builder = AntigravityAgent::builder()
        .add_workspace("/repo")
        .with_workspace_containment(false);
    let config = builder.build_harness_config(&ToolDispatcher::new(vec![], &[]));
    assert_eq!(config.policy_config, None);
}

#[test]
fn test_builder_no_workspace_no_policy_config() {
    // Nothing to contain to: no policy config, as before.
    let config =
        AntigravityAgent::builder().build_harness_config(&ToolDispatcher::new(vec![], &[]));
    assert_eq!(config.policy_config, None);
}

#[tokio::test]
async fn test_spawn_rejects_containment_without_a_workspace() {
    // Rejected before the harness is looked for, so no binary is needed.
    let err = AntigravityAgent::builder()
        .with_workspace_containment(true)
        .spawn()
        .await
        .expect_err("containment with no workspace must not spawn");
    assert!(
        matches!(&err, AntigravityError::Config(m) if m.contains("needs a workspace")),
        "{err:?}"
    );
}

#[test]
fn test_overlapping_mcp_server_names_are_found() {
    let names = ["git", "git_admin", "github", "gi", "git_admin_x"];
    let pairs = super::validate::overlapping_mcp_servers(names.into_iter());
    assert_eq!(
        pairs,
        [
            ("git", "git_admin"),
            ("git", "git_admin_x"),
            ("git_admin", "git_admin_x"),
        ]
    );
    assert!(super::validate::overlapping_mcp_servers(["git", "github"].into_iter()).is_empty());
}

// =============================================================================
// Subagents beside the gates they bypass (#520, D-042)
// =============================================================================

/// An agent offering subagents, past the write-tool safety gate with a
/// policy that refuses nothing.
fn subagent_agent(harness: &std::path::Path) -> AgentBuilder {
    AntigravityAgent::builder()
        .with_harness_path(harness)
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))
        .add_policy(policy::allow_all())
}

/// `run_command` options asking for the OS sandbox.
fn sandbox() -> crate::antigravity::RunCommandOptions {
    crate::antigravity::RunCommandOptions::new().with_sandbox(true)
}

/// A gate a subagent's own calls bypass, as the words the refusal names it
/// by and a function that sets it on a builder.
type Gate = (&'static str, fn(AgentBuilder) -> AgentBuilder);

/// Each gate a subagent's own calls bypass.
fn each_gate() -> Vec<Gate> {
    vec![
        ("pre-tool hook", |b| {
            b.on_pre_tool(|_| PreToolDecision::Allow)
        }),
        ("deny or confirm policy", |b| {
            b.add_policy(policy::deny("run_command"))
        }),
        ("deny or confirm policy", |b| {
            b.add_policy(policy::confirm("run_command"))
        }),
        ("deny or confirm policy", |b| {
            b.add_policy(policy::ask_user("run_command", |_, _| true))
        }),
        ("auto policy", |b| b.add_policy(policy::auto())),
        ("workspace containment", |b| b.add_workspace("/repo")),
        // Added to the builder's capabilities, so the subagent builtin
        // stays as it was.
        ("run_command sandbox", |b| {
            let capabilities = b
                .capabilities
                .clone()
                .enable(BuiltinTool::RunCommand)
                .with_run_command(sandbox());
            b.with_capabilities(capabilities)
        }),
    ]
}

#[tokio::test]
async fn test_spawn_refuses_subagents_beside_each_gate_without_the_opt_in() {
    let (_dir, path) = unexecutable_harness();
    for (gate, apply) in each_gate() {
        let err = apply(subagent_agent(&path)).spawn().await.unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("{gate}: expected a Config error, got {err:?}");
        };
        assert!(message.contains(gate), "{gate}: {message}");
        assert!(message.contains("invoke_subagent"), "{gate}: {message}");
        assert!(
            message.contains("with_ungated_subagents(true)"),
            "{gate}: {message}"
        );
    }
}

#[tokio::test]
async fn test_spawn_allows_subagents_beside_each_gate_with_the_opt_in() {
    let (_dir, path) = unexecutable_harness();
    for (gate, apply) in each_gate() {
        let err = apply(subagent_agent(&path))
            .with_ungated_subagents(true)
            .spawn()
            .await
            .unwrap_err();
        // Past validation: the spawn fails only at process launch.
        assert!(
            matches!(err, AntigravityError::HandshakeFailed { .. }),
            "{gate}: {err:?}"
        );
    }
}

#[tokio::test]
async fn test_spawn_names_every_gate_subagents_would_bypass() {
    let (_dir, path) = unexecutable_harness();
    let err = each_gate()
        .into_iter()
        .fold(subagent_agent(&path), |builder, (_, apply)| apply(builder))
        .with_define_subagent_tool(true)
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected a Config error, got {err:?}");
    };
    for (gate, _) in each_gate() {
        assert!(message.contains(gate), "{gate}: {message}");
    }
    assert!(
        message.contains("invoke_subagent") && message.contains("define_subagent"),
        "{message}"
    );
}

#[tokio::test]
async fn test_spawn_allows_subagents_without_a_gate() {
    let (_dir, path) = unexecutable_harness();
    // Policies that only allow refuse nothing, containment is off (no
    // workspace, or opted out), and a sandbox on a run_command the agent
    // does not have confines nothing: there is nothing for a subagent to
    // bypass.
    let ungated = [
        subagent_agent(&path),
        subagent_agent(&path).add_policy(policy::allow("view_file")),
        subagent_agent(&path)
            .add_workspace("/repo")
            .with_workspace_containment(false),
        subagent_agent(&path).with_capabilities(
            Capabilities::read_only()
                .enable(BuiltinTool::InvokeSubagent)
                .with_run_command(sandbox()),
        ),
    ];
    for builder in ungated {
        let err = builder.spawn().await.unwrap_err();
        assert!(
            matches!(err, AntigravityError::HandshakeFailed { .. }),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn test_spawn_refuses_define_subagent_beside_a_gate() {
    let (_dir, path) = unexecutable_harness();
    // define_subagent counts on its own, even with invoke_subagent off.
    let err = AntigravityAgent::builder()
        .with_harness_path(&path)
        .with_define_subagent_tool(true)
        .on_pre_tool(|_| PreToolDecision::Allow)
        .spawn()
        .await
        .unwrap_err();
    let AntigravityError::Config(message) = &err else {
        panic!("expected a Config error, got {err:?}");
    };
    assert!(message.contains("define_subagent"), "{message}");
    assert!(!message.contains("invoke_subagent"), "{message}");
}

#[tokio::test]
async fn test_spawn_allows_gates_without_subagents() {
    let (_dir, path) = unexecutable_harness();
    // Every gate, no subagent tools: nothing is refused, opt-in or not.
    let builder = each_gate().into_iter().fold(
        AntigravityAgent::builder()
            .with_harness_path(&path)
            .with_capabilities(Capabilities::read_only()),
        |b, (_, apply)| apply(b),
    );
    assert!(builder.sandboxes_commands() && builder.contains_workspaces());
    assert!(!builder.capabilities.is_enabled(BuiltinTool::InvokeSubagent));
    let err = builder.spawn().await.unwrap_err();
    assert!(
        matches!(err, AntigravityError::HandshakeFailed { .. }),
        "{err:?}"
    );
}
