use super::*;

fn parent_tool(name: &str) -> protocol::Tool {
    protocol::Tool {
        name: Some(name.to_string()),
        description: Some(format!("{name} description")),
        parameters_json_schema: Some(r#"{"type":"object"}"#.to_string()),
        response_json_schema: None,
    }
}

#[test]
fn test_subagent_defaults() {
    let subagent = Subagent::new("auditor");
    assert_eq!(subagent.name(), "auditor");
    assert!(subagent.tool_names().is_empty());
    let wire = subagent.to_wire(&[], None);
    assert_eq!(wire.name.as_deref(), Some("auditor"));
    assert!(wire.description.is_none());
    assert!(wire.system_instructions.is_none());
    assert!(wire.tools.is_empty());
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
    let wire = subagent.to_wire(&parent_tools, None);
    assert_eq!(wire.description.as_deref(), Some("Audits files."));
    // Instructions are appended sections (reference SDK semantics),
    // never a custom replacement.
    let instructions = wire.system_instructions.unwrap();
    assert!(instructions.custom.is_none());
    let appended = instructions.appended.unwrap();
    assert_eq!(appended.appended_sections.len(), 1);
    assert_eq!(
        appended.appended_sections[0].title.as_deref(),
        Some("System")
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
fn test_subagent_to_wire_appends_workspace_note() {
    // With both user instructions and a workspace note, the subagent
    // gets two appended sections; with only the note, just the note.
    let with_both = Subagent::new("auditor")
        .with_system_instructions("Focus on injection vectors.")
        .to_wire(&[], Some("ROOTS: /repo"));
    let sections = with_both
        .system_instructions
        .unwrap()
        .appended
        .unwrap()
        .appended_sections;
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].title.as_deref(), Some("System"));
    assert_eq!(sections[1].title.as_deref(), Some("Workspace"));
    assert_eq!(sections[1].content.as_deref(), Some("ROOTS: /repo"));

    let note_only = Subagent::new("auditor").to_wire(&[], Some("ROOTS: /repo"));
    let sections = note_only
        .system_instructions
        .unwrap()
        .appended
        .unwrap()
        .appended_sections;
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].title.as_deref(), Some("Workspace"));
}

#[test]
fn test_subagent_start_subagent_forced_off() {
    // Nesting depth is not configurable yet: even when explicitly
    // enabled, the wire config disables the builtin.
    let subagent = Subagent::new("nested")
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent));
    let wire = subagent.to_wire(&[], None);
    assert!(!wire.harness_side_tools.unwrap().subagents.unwrap().enabled);
}

#[test]
fn test_subagent_wire_serialization_fixture() {
    let subagent = Subagent::new("auditor")
        .with_description("Audits files.")
        .with_system_instructions("Be thorough.")
        .add_tool("severity_classifier");
    let wire = subagent.to_wire(&[parent_tool("severity_classifier")], None);
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
            .to_wire(&[], None)
            .agent_behavior
            .is_none()
    );
    let wire = Subagent::new("a")
        .with_agent_behavior(AgentBehavior::Interactive)
        .to_wire(&[], None);
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
