//! Inline skills' optional fields (#504), each shown changing what the
//! model sees, or, for the two harness 0.1.23 ignores, shown changing
//! nothing so that the day it starts to honor them is noticed (the bridge
//! design's Python SDK parity section lists them with the reason).

use super::*;
use genai_rs::antigravity::{InlineSkill, McpServer};

/// A turn that looks up `probe-skill`, then answers.
fn looks_up_the_skill() -> Vec<ModelTurn> {
    vec![
        ModelTurn::call("lookup_skill", json!({"skill_name": "probe-skill"})),
        ModelTurn::text("looked-6201"),
    ]
}

/// `probe-skill`, described by `SKILL-DESCRIPTION-6202`.
fn probe_skill() -> InlineSkill {
    InlineSkill::new(
        "probe-skill",
        "SKILL-DESCRIPTION-6202",
        "SKILL-CONTENT-6203",
    )
}

/// A second skill, described by `OTHER-DESCRIPTION-6204`.
fn other_skill() -> InlineSkill {
    InlineSkill::new(
        "other-skill",
        "OTHER-DESCRIPTION-6204",
        "OTHER-CONTENT-6205",
    )
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_skill_metadata_and_dependent_skills_change_what_the_model_sees() {
    // `visibility: hidden` keeps a skill out of the prompt's list, and
    // the model can still look it up.
    let fake = FakeGemini::start(looks_up_the_skill()).await;
    let mut agent = scripted_agent(&fake)
        .add_inline_skill(probe_skill().add_metadata("visibility", "hidden"))
        .add_inline_skill(other_skill())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("use the skill").await.expect("turn");
    let prompt = fake.system_prompt(0);
    assert!(!prompt.contains("SKILL-DESCRIPTION-6202"), "{prompt}");
    assert!(prompt.contains("OTHER-DESCRIPTION-6204"), "{prompt}");
    let told = function_response_text(&fake, 1);
    assert!(told.contains("SKILL-CONTENT-6203"), "{told}");
    agent.shutdown().await.expect("shutdown");

    // A dependent skill is named, with its description, in the lookup's
    // answer; without one the answer is the content alone.
    for dependent in [false, true] {
        let fake = FakeGemini::start(looks_up_the_skill()).await;
        let mut skill = probe_skill();
        if dependent {
            skill = skill.add_dependent_skill("other-skill");
        }
        let mut agent = scripted_agent(&fake)
            .add_inline_skill(skill)
            .add_inline_skill(other_skill())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("use the skill").await.expect("turn");
        let told = function_response_text(&fake, 1);
        assert!(told.contains("SKILL-CONTENT-6203"), "{told}");
        assert_eq!(
            told.contains("OTHER-DESCRIPTION-6204"),
            dependent,
            "dependent {dependent}: {told}"
        );
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_skill_allowed_and_dependent_tools_change_nothing_on_this_harness() {
    // On 0.1.23 (2026-11-09) neither list changes the prompt, the lookup's
    // answer, the tools declared after it, or how a call is gated. When
    // this fails, the harness has started to honor one: model it, and
    // update the parity table and `InlineSkill`'s rustdoc.

    // A dependent tool, here a deferred MCP tool, is not declared after
    // the lookup.
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_echo_server.py");
    let widgets =
        McpServer::stdio("python3", [fixture.to_string_lossy().to_string()]).with_name("widgets");
    let fake = FakeGemini::start(looks_up_the_skill()).await;
    let mut agent = scripted_agent(&fake)
        .add_mcp_server(widgets)
        .add_policy(policy::allow_all())
        .add_inline_skill(probe_skill().add_dependent_tool("mcp_widgets_lookup_widget_code"))
        .spawn()
        .await
        .expect("spawn");
    agent.chat("use the skill").await.expect("turn");
    assert_eq!(fake.tool_names(1), fake.tool_names(0));
    let declared = fake.tool_names(1);
    assert!(
        !declared.contains(&"mcp_widgets_lookup_widget_code".to_string()),
        "{declared:?}"
    );
    agent.shutdown().await.expect("shutdown");

    // The allowed list neither grants nor restricts after the lookup: a
    // tool on it still goes through the policies, and one off it still
    // runs where the policies allow it.
    for on_the_list in [true, false] {
        let workspace = scratch_dir("agy-scripted-allowed");
        let file = write_file(&workspace, "a.txt", "ALLOWED-6206");
        let mut script = looks_up_the_skill();
        script.insert(1, view_file(&file));
        let fake = FakeGemini::start(script).await;
        let builder = scripted_agent(&fake).add_workspace(workspace.path().to_string_lossy());
        let builder = if on_the_list {
            builder
                .add_policy(policy::deny_all())
                .add_policy(policy::allow("lookup_skill"))
                .add_inline_skill(probe_skill().add_allowed_tool("view_file"))
        } else {
            builder
                .add_policy(policy::allow_all())
                .add_inline_skill(probe_skill().add_allowed_tool("list_dir"))
        };
        let mut agent = builder.spawn().await.expect("spawn");
        agent.chat("use the skill").await.expect("turn");
        let told = function_response_text(&fake, 2);
        if on_the_list {
            assert!(told.contains("Denied by policy"), "{told}");
            assert!(!told.contains("ALLOWED-6206"), "{told}");
        } else {
            assert!(told.contains("ALLOWED-6206"), "{told}");
        }
        agent.shutdown().await.expect("shutdown");
    }
}
