use super::*;
use serde_json::json;

#[test]
fn test_budget_reaches_the_wire_only_with_a_cap() {
    assert!(Budget::new().is_empty());
    assert!(Budget::new().to_wire().is_none());
    let wire = Budget::new()
        .with_max_model_calls(3)
        .with_max_tool_calls(7)
        .with_max_input_tokens(10)
        .with_max_output_tokens(20)
        .with_max_total_tokens(30)
        .with_scope(BudgetScope::ForwardLooking)
        .to_wire()
        .unwrap();
    assert_eq!(
        serde_json::to_value(wire).unwrap(),
        json!({
            "maxModelCalls": 3, "maxToolCalls": 7, "maxInputTokens": 10,
            "maxOutputTokens": 20, "maxTotalTokens": 30,
            "scope": "BUDGET_SCOPE_FORWARD_LOOKING"
        })
    );
    let lifetime = Budget::new().with_max_tool_calls(1).to_wire().unwrap();
    assert_eq!(
        lifetime.scope,
        Some(protocol::BudgetScope::Lifetime),
        "the scope is written explicitly, as the Python SDK does"
    );
}

#[test]
fn test_budget_saturates_and_names_a_zero_cap() {
    let wire = Budget::new()
        .with_max_tool_calls(u32::MAX)
        .with_max_total_tokens(u64::MAX)
        .to_wire()
        .unwrap();
    assert_eq!(wire.max_tool_calls, Some(i32::MAX));
    assert_eq!(wire.max_total_tokens, Some(i64::MAX));
    assert_eq!(Budget::new().with_max_tool_calls(1).zero_cap(), None);
    assert_eq!(
        Budget::new().with_max_output_tokens(0).zero_cap(),
        Some("max_output_tokens")
    );
}

#[test]
fn test_retry_config_writes_only_what_is_set() {
    assert!(RetryConfig::new().to_wire().is_none());
    let api_only = RetryConfig::new()
        .with_max_api_retries(0)
        .to_wire()
        .unwrap();
    assert_eq!(
        serde_json::to_value(api_only).unwrap(),
        json!({"apiRetry": {"maxRetries": 0}})
    );
    let full = RetryConfig::new()
        .with_max_api_retries(4)
        .with_initial_backoff(Duration::from_millis(250))
        .with_backoff_multiplier(1.5)
        .with_jitter(0.2)
        .with_max_output_retries(2)
        .to_wire()
        .unwrap();
    assert_eq!(
        serde_json::to_value(full).unwrap(),
        json!({
            "apiRetry": {
                "maxRetries": 4, "initialSleepDurationMs": 250,
                "exponentialMultiplier": 1.5, "jitterRange": 0.2
            },
            "modelOutputRetry": {"maxRetries": 2}
        })
    );
}

#[test]
fn test_retry_benchmark_matches_the_python_preset() {
    let wire = RetryConfig::benchmark().to_wire().unwrap();
    let api = wire.api_retry.unwrap();
    assert_eq!(api.max_retries, Some(u32::MAX));
    assert_eq!(api.initial_sleep_duration_ms, Some(1000));
    assert!(wire.model_output_retry.is_none());
}

#[test]
fn test_run_command_options_wire() {
    let defaults = RunCommandOptions::new().to_wire(true);
    assert_eq!(
        serde_json::to_value(defaults).unwrap(),
        json!({"enabled": true})
    );
    let wire = RunCommandOptions::new()
        .with_timeout(Duration::from_millis(1500))
        .with_daemons(true)
        .with_sandbox(true)
        .to_wire(false);
    assert_eq!(
        serde_json::to_value(wire).unwrap(),
        json!({
            "enabled": false, "maxTimeoutMs": 1500,
            "enableDaemonCommands": true, "enableSandbox": true
        })
    );
    // A sub-millisecond timeout is not "no timeout".
    let tiny = RunCommandOptions::new()
        .with_timeout(Duration::from_micros(10))
        .to_wire(true);
    assert_eq!(tiny.max_timeout_ms, Some(1));
}

#[test]
fn test_inline_skill_wire_carries_dependencies_in_metadata() {
    let skill = InlineSkill::new("probe", "When probing.", "Body.")
        .add_allowed_tool("view_file")
        .add_dependent_tool("view_file")
        .add_dependent_skill("other")
        .add_metadata("visibility", "hidden");
    assert_eq!(skill.name(), "probe");
    assert_eq!(
        serde_json::to_value(skill.to_wire()).unwrap(),
        json!({"skill": {
            "name": "probe", "description": "When probing.", "content": "Body.",
            "allowedTools": ["view_file"],
            "metadata": {
                "dependent_skills": "[\"other\"]",
                "dependent_tools": "[\"view_file\"]",
                "visibility": "hidden"
            }
        }})
    );
    // An explicit metadata entry wins over the derived one.
    let explicit = InlineSkill::new("p", "d", "c")
        .add_dependent_tool("a")
        .add_metadata("dependent_tools", "[\"b\"]");
    assert_eq!(
        explicit.to_wire().skill.unwrap().metadata["dependent_tools"],
        "[\"b\"]"
    );
}

#[test]
fn test_subagent_skills_modes() {
    assert_eq!(
        serde_json::to_value(
            SubagentSkills::inherit()
                .add_skill_name("a")
                .add_extra_path("/p")
                .to_wire()
        )
        .unwrap(),
        json!({"inheritConfig": {"extraSkillsPaths": ["/p"], "skillNames": ["a"]}})
    );
    assert_eq!(
        serde_json::to_value(SubagentSkills::none().to_wire()).unwrap(),
        json!({"noneConfig": {}})
    );
    assert_eq!(
        serde_json::to_value(
            SubagentSkills::from_paths(["/s"])
                .add_skill_name("ignored")
                .to_wire()
        )
        .unwrap(),
        json!({"overrideConfig": {"skillsPaths": ["/s"]}})
    );
}

#[test]
fn test_session_continuation_wire_values() {
    for (mode, wire) in [
        (SessionContinuation::Resume, "RESUME"),
        (SessionContinuation::CreateOrResume, "CREATE_OR_RESUME"),
        (SessionContinuation::CreateOnly, "CREATE_ONLY"),
    ] {
        assert_eq!(mode.to_wire().as_wire_str(), wire);
    }
}
