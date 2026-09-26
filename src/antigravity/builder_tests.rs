use super::*;
use crate::antigravity::policy;

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
    let err = AntigravityAgent::builder()
        .with_harness_path(&path)
        .with_capabilities(Capabilities::all())
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
    // Custom instructions round through the `custom.part` shape.
    let instructions = config.system_instructions.as_ref().unwrap();
    assert_eq!(
        instructions.custom.as_ref().unwrap().part[0]
            .text
            .as_deref(),
        Some("Be brief.")
    );
    // run_command was enabled on top of the read-only set.
    let side_tools = config.harness_side_tools.as_ref().unwrap();
    assert!(side_tools.run_command.unwrap().enabled);
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
fn test_build_system_instructions_composition() {
    // User + note: two custom parts, user text unmutated and first.
    let si = build_system_instructions(Some("Be brief."), Some("ROOTS")).unwrap();
    let parts = si.custom.unwrap().part;
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].text.as_deref(), Some("Be brief."));
    assert_eq!(parts[1].text.as_deref(), Some("ROOTS"));

    // Note only (no user instructions): an appended section preserves
    // the harness defaults.
    let si = build_system_instructions(None, Some("ROOTS")).unwrap();
    assert!(si.custom.is_none());
    let sections = si.appended.unwrap().appended_sections;
    assert_eq!(sections[0].title.as_deref(), Some("Workspace"));
    assert_eq!(sections[0].content.as_deref(), Some("ROOTS"));

    // User only: fully-custom single part.
    let si = build_system_instructions(Some("Hi"), None).unwrap();
    assert_eq!(si.custom.unwrap().part[0].text.as_deref(), Some("Hi"));

    // Neither: no instructions.
    assert!(build_system_instructions(None, None).is_none());
}

#[test]
fn test_builder_announces_workspace_by_default() {
    let builder = AntigravityAgent::builder()
        .with_system_instructions("Audit it.")
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
        .with_system_instructions("Audit it.")
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
    let builder = AntigravityAgent::builder().with_system_instructions("Hi");
    let dispatcher = ToolDispatcher::new(vec![], &[]);
    let config = builder.build_harness_config(&dispatcher);
    let parts = config.system_instructions.unwrap().custom.unwrap().part;
    assert_eq!(parts.len(), 1);
}
