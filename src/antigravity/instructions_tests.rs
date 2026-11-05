use super::*;

#[test]
fn test_plain_text_is_appended_under_the_python_sdk_title() {
    let wire = SystemInstructions::from("Be brief.").to_wire(None);
    assert!(
        wire.custom.is_none(),
        "plain text must not replace the prompt"
    );
    let appended = wire.appended.unwrap();
    assert!(appended.custom_identity.is_none());
    assert_eq!(appended.appended_sections.len(), 1);
    assert_eq!(
        appended.appended_sections[0].title.as_deref(),
        Some("user_system_instructions")
    );
    assert_eq!(
        appended.appended_sections[0].content.as_deref(),
        Some("Be brief.")
    );
    assert_eq!(
        SystemInstructions::from(String::from("Be brief.")),
        SystemInstructions::appended("Be brief.")
    );
}

#[test]
fn test_harness_default_with_identity_and_sections() {
    let wire = SystemInstructions::harness_default()
        .with_identity("You are Tern.")
        .add_section("style", "Past tense.")
        .to_wire(None);
    let appended = wire.appended.unwrap();
    assert_eq!(appended.custom_identity.as_deref(), Some("You are Tern."));
    assert_eq!(appended.appended_sections.len(), 1);
    assert_eq!(
        appended.appended_sections[0].title.as_deref(),
        Some("style")
    );
}

#[test]
fn test_custom_replaces_and_carries_titles_identity_and_the_workspace_note() {
    let instructions = SystemInstructions::custom("Answer ok.")
        .with_title("rules")
        .with_identity("You are Ok-bot.")
        .add_section("extra", "Nothing else.");
    assert!(instructions.is_custom());
    let wire = instructions.to_wire(Some("ROOTS: /repo"));
    assert!(wire.appended.is_none());
    let parts = wire.custom.unwrap().part;
    let shape: Vec<(Option<&str>, Option<&str>)> = parts
        .iter()
        .map(|p| (p.title.as_deref(), p.text.as_deref()))
        .collect();
    assert_eq!(
        shape,
        vec![
            (Some("identity"), Some("You are Ok-bot.")),
            (Some("rules"), Some("Answer ok.")),
            (Some("extra"), Some("Nothing else.")),
            (None, Some("ROOTS: /repo")),
        ]
    );
}

#[test]
fn test_custom_wire_serialization_fixture() {
    let value = serde_json::to_value(
        SystemInstructions::custom("Hi")
            .with_title("t")
            .to_wire(None),
    )
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!({"custom": {"part": [{"text": "Hi", "title": "t"}]}})
    );
}

#[test]
fn test_with_title_on_empty_instructions_is_a_no_op() {
    let instructions = SystemInstructions::harness_default().with_title("x");
    assert_eq!(instructions, SystemInstructions::harness_default());
}
