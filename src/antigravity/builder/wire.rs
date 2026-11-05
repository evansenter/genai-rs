//! Turns an [`AgentBuilder`] into the harness's `HarnessConfig`.

use super::AgentBuilder;
use crate::antigravity::protocol;
use crate::antigravity::tools::ToolDispatcher;
use crate::antigravity::{AgentBehavior, McpServer};

impl AgentBuilder {
    pub(super) fn build_harness_config(
        &self,
        dispatcher: &ToolDispatcher,
    ) -> protocol::HarnessConfig {
        let mut enabled_hooks = Vec::new();
        if !self.policies.is_empty() || self.pre_tool.is_some() {
            enabled_hooks.push(protocol::LifecycleHook::PreTool);
        }
        if self.post_tool.is_some() {
            enabled_hooks.push(protocol::LifecycleHook::PostTool);
        }

        let models = self
            .api_key()
            .map(|api_key| {
                vec![protocol::ModelConfig {
                    // A real model id, not a placeholder: this is the id the
                    // harness is actually asked to run when the caller never
                    // called `with_model`.
                    name: Some(
                        self.model
                            .clone()
                            .unwrap_or_else(|| crate::DEFAULT_MODEL.to_string()),
                    ),
                    types: vec![protocol::ModelType::Text],
                    gemini_api_endpoint: Some(protocol::GeminiApiEndpoint {
                        api_key: Some(api_key.to_string()),
                        base_url: self.gemini_base_url.clone(),
                        ..Default::default()
                    }),
                    vertex_endpoint: None,
                }]
            })
            .unwrap_or_default();

        let tools = dispatcher.harness_declarations();
        // Announce the workspace roots to the model (opt-outable). The note
        // is composed here at send time; the stored `system_instructions`
        // string is never mutated. Subagents get the same note appended
        // (their trajectories don't inherit the parent's context).
        let workspace_note = (self.announce_workspaces() && !self.workspaces.is_empty())
            .then(|| workspace_announcement(&self.workspaces));
        let custom_subagents = self
            .subagents
            .iter()
            .map(|subagent| subagent.to_wire(&tools, workspace_note.as_deref()))
            .collect();

        protocol::HarnessConfig {
            cascade_id: self.conversation_id.clone(),
            system_instructions: build_system_instructions(
                self.system_instructions.as_deref(),
                workspace_note.as_deref(),
            ),
            tools,
            harness_side_tools: Some(self.capabilities.to_harness_side_tools()),
            compaction_threshold: None,
            workspaces: self
                .workspaces
                .iter()
                .map(protocol::Workspace::filesystem)
                .collect(),
            skills_paths: self.skills_paths.clone(),
            finish_tool_schema_json: self.response_schema.as_ref().map(ToString::to_string),
            initial_trajectory: None,
            app_data_dir: self.app_data_dir.clone(),
            mcp_servers: self.mcp_servers.iter().map(McpServer::to_wire).collect(),
            models,
            enabled_hooks,
            custom_subagents,
            agent_behavior: self.agent_behavior.map(AgentBehavior::to_wire),
            // spawn() rejects an explicit opt-in with no workspace.
            policy_config: (self.workspace_containment.unwrap_or(true)
                && !self.workspaces.is_empty())
            .then(protocol::PolicyConfig::default),
        }
    }
}

/// Builds the workspace-announcement note (Item: announce workspace roots).
/// A concise, clearly delimited block listing the configured root(s) so the
/// model uses real paths instead of guessing. The roots are announced exactly
/// as configured (the same strings the harness roots its tools at), so the
/// wording makes no claim about them being absolute — a caller may pass a
/// relative path, and the model must use the same string the harness does.
pub(super) fn workspace_announcement(workspaces: &[String]) -> String {
    let roots = workspaces
        .iter()
        .map(|root| format!("- {root}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "=== Workspace ===\n\
         Your workspace is rooted at the following path(s). Use these exact \
         paths for all file and directory operations; do not guess or invent \
         workspace paths, and stay within these root(s):\n{roots}\n\
         === End Workspace ==="
    )
}

/// Composes the effective parent system instructions from the user's
/// instructions (never mutated) and an optional workspace-announcement note.
///
/// - With a note and user instructions: the note is a second `custom` part
///   (keeps the user's text intact, still fully custom).
/// - With a note and no user instructions: the note is an *appended* section
///   so the harness's default instructions are preserved (not replaced).
/// - With no note: the user's instructions as fully-custom text, or `None`.
pub(super) fn build_system_instructions(
    user_instructions: Option<&str>,
    workspace_note: Option<&str>,
) -> Option<protocol::SystemInstructions> {
    match (user_instructions, workspace_note) {
        (Some(text), Some(note)) => Some(protocol::SystemInstructions {
            custom: Some(protocol::CustomSystemInstructions {
                part: vec![
                    protocol::SystemInstructionPart {
                        text: Some(text.to_string()),
                    },
                    protocol::SystemInstructionPart {
                        text: Some(note.to_string()),
                    },
                ],
            }),
            appended: None,
        }),
        (Some(text), None) => Some(protocol::SystemInstructions::custom_text(text.to_string())),
        (None, Some(note)) => Some(protocol::SystemInstructions {
            custom: None,
            appended: Some(protocol::AppendedSystemInstructions {
                custom_identity: None,
                appended_sections: vec![protocol::InstructionSection {
                    title: Some("Workspace".to_string()),
                    content: Some(note.to_string()),
                }],
            }),
        }),
        (None, None) => None,
    }
}
