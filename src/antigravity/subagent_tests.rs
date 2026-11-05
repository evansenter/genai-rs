use super::*;

fn parent_tool(name: &str) -> protocol::Tool {
    protocol::Tool {
        name: Some(name.to_string()),
        description: Some(format!("{name} description")),
        parameters_json_schema: Some(r#"{"type":"object"}"#.to_string()),
        response_json_schema: None,
    }
}

fn flat() -> SubagentWireContext<'static> {
    SubagentWireContext::default()
}

#[test]
fn test_subagent_defaults() {
    let subagent = Subagent::new("auditor");
    assert_eq!(subagent.name(), "auditor");
    assert!(subagent.tool_names().is_empty());
    assert!(subagent.allowed_subagents().is_none());
    let wire = subagent.to_wire(&[], flat());
    assert_eq!(wire.name.as_deref(), Some("auditor"));
    assert!(wire.description.is_none());
    assert!(wire.system_instructions.is_none());
    assert!(wire.tools.is_empty());
    assert!(wire.model.is_none());
    assert!(wire.skills_config.is_none());
    // Default capabilities: read-only builtins, explicit flags.
    let side_tools = wire.harness_side_tools.unwrap();
    assert!(side_tools.view_file.unwrap().enabled);
    assert!(!side_tools.run_command.unwrap().enabled);
    assert!(!side_tools.subagents.unwrap().enabled);
}

#[test]
fn test_subagent_to_wire_resolves_parent_tools() {
    let subagent = Subagent::new("auditor")
        .with_description("Audits files.")
        .with_system_instructions("Focus on injection vectors.")
        .add_tool("severity_classifier");
    assert_eq!(subagent.tool_names(), ["severity_classifier"]);

    let parent_tools = [parent_tool("other"), parent_tool("severity_classifier")];
    let wire = subagent.to_wire(&parent_tools, flat());
    assert_eq!(wire.description.as_deref(), Some("Audits files."));
    // A string is appended to the harness's subagent prompt (Python SDK
    // semantics), never a custom replacement.
    let instructions = wire.system_instructions.unwrap();
    assert!(instructions.custom.is_none());
    let appended = instructions.appended.unwrap();
    assert_eq!(appended.appended_sections.len(), 1);
    assert_eq!(
        appended.appended_sections[0].title.as_deref(),
        Some("user_system_instructions")
    );
    assert_eq!(
        appended.appended_sections[0].content.as_deref(),
        Some("Focus on injection vectors.")
    );
    // The full parent declaration is copied, not just the name.
    assert_eq!(wire.tools.len(), 1);
    assert_eq!(wire.tools[0].name.as_deref(), Some("severity_classifier"));
    assert_eq!(
        wire.tools[0].parameters_json_schema.as_deref(),
        Some(r#"{"type":"object"}"#)
    );
}

#[test]
fn test_subagent_workspace_note_goes_to_custom_instructions_only() {
    let context = SubagentWireContext {
        workspace_note: Some("ROOTS: /repo"),
        ..SubagentWireContext::default()
    };
    // Appended: the harness's subagent prompt already lists the workspaces
    // (verified on 0.1.23), so no note.
    let appended = Subagent::new("auditor")
        .with_system_instructions("Focus.")
        .to_wire(&[], context);
    let sections = appended
        .system_instructions
        .unwrap()
        .appended
        .unwrap()
        .appended_sections;
    assert_eq!(sections.len(), 1);

    // Custom: the prompt is replaced, so the roots come back as a part.
    let custom = Subagent::new("auditor")
        .with_system_instructions(SystemInstructions::custom("Focus."))
        .to_wire(&[], context);
    let parts = custom.system_instructions.unwrap().custom.unwrap().part;
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1].text.as_deref(), Some("ROOTS: /repo"));

    // No instructions: nothing to add a note to.
    assert!(
        Subagent::new("a")
            .to_wire(&[], context)
            .system_instructions
            .is_none()
    );
}

#[test]
fn test_subagent_controls_reach_harness_config() {
    // #493 item 3: per-subagent model, allow-list, skills, and nesting.
    let subagent = Subagent::new("lead")
        .with_model("subagent-model-x")
        .with_allowed_subagents(["helper"])
        .with_skills(SubagentSkills::none())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent));
    assert_eq!(
        subagent.allowed_subagents(),
        Some(&["helper".to_string()][..])
    );
    let nesting = SubagentWireContext {
        nesting: true,
        disable_define_subagent_tool: true,
        ..SubagentWireContext::default()
    };
    let wire = subagent.to_wire(&[], nesting);
    assert_eq!(
        wire.model.as_ref().and_then(|m| m.name.as_deref()),
        Some("subagent-model-x")
    );
    assert!(wire.skills_config.unwrap().none_config.is_some());
    let subagents = wire.harness_side_tools.unwrap().subagents.unwrap();
    assert!(subagents.enabled, "nesting allows invoke_subagent");
    assert_eq!(subagents.allowed_subagents, ["helper"]);
    assert!(subagents.disable_define_subagent_tool);
    assert_eq!(
        serde_json::to_value(&wire.model).unwrap(),
        serde_json::json!({"name": "subagent-model-x"}),
        "a subagent model carries only its name"
    );
}

#[test]
fn test_subagent_invoke_subagent_off_without_nesting() {
    // The harness default depth is 1 (flat): without nesting configured,
    // an explicit enable is turned off.
    let subagent = Subagent::new("nested")
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent));
    let wire = subagent.to_wire(&[], flat());
    assert!(!wire.harness_side_tools.unwrap().subagents.unwrap().enabled);
}

#[test]
fn test_subagent_wire_serialization_fixture() {
    let subagent = Subagent::new("auditor")
        .with_description("Audits files.")
        .with_system_instructions("Be thorough.")
        .add_tool("severity_classifier");
    let wire = subagent.to_wire(&[parent_tool("severity_classifier")], flat());
    let value = serde_json::to_value(&wire).unwrap();
    // Proto-JSON: camelCase fields, JSON-string schema.
    assert_eq!(
        value["systemInstructions"]["appended"]["appendedSections"][0]["content"],
        "Be thorough."
    );
    assert_eq!(value["tools"][0]["name"], "severity_classifier");
    assert_eq!(
        value["tools"][0]["parametersJsonSchema"],
        r#"{"type":"object"}"#
    );
    assert_eq!(value["harnessSideTools"]["subagents"]["enabled"], false);
    assert_eq!(value["harnessSideTools"]["viewFile"]["enabled"], true);
}

#[test]
fn test_agent_behavior_reaches_the_wire_only_when_set() {
    // Unset stays off the wire: the harness default is already
    // Autonomous, so sending it would change nothing but the golden.
    assert!(
        Subagent::new("a")
            .to_wire(&[], flat())
            .agent_behavior
            .is_none()
    );
    let wire = Subagent::new("a")
        .with_agent_behavior(AgentBehavior::Interactive)
        .to_wire(&[], flat());
    assert_eq!(
        serde_json::to_value(&wire).unwrap()["agentBehavior"],
        "AGENT_BEHAVIOR_INTERACTIVE"
    );
    for (behavior, wire) in [
        (AgentBehavior::Autonomous, "AGENT_BEHAVIOR_AUTONOMOUS"),
        (AgentBehavior::Interactive, "AGENT_BEHAVIOR_INTERACTIVE"),
        (AgentBehavior::Minimal, "AGENT_BEHAVIOR_MINIMAL"),
    ] {
        assert_eq!(behavior.to_wire().as_wire_str(), wire);
    }
}
