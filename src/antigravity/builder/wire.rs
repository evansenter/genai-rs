//! Turns an [`AgentBuilder`] into the harness's `HarnessConfig`.

use super::AgentBuilder;
use crate::antigravity::policies::PolicyEngine;
use crate::antigravity::protocol;
use crate::antigravity::subagent::SubagentWireContext;
use crate::antigravity::tools::ToolDispatcher;
use crate::antigravity::{
    AgentBehavior, Budget, BuiltinTool, InlineSkill, McpServer, RetryConfig, SessionContinuation,
};

impl AgentBuilder {
    pub(super) fn build_harness_config(
        &self,
        dispatcher: &ToolDispatcher,
    ) -> protocol::HarnessConfig {
        let engine = &PolicyEngine::new(self.policies.clone());
        let options = &self.options;
        let mut enabled_hooks = Vec::new();
        // Client-evaluated policies and the pre-tool hook both answer the
        // harness's pre-tool callback; the auto policy is harness-side.
        if engine.has_rules() || self.pre_tool.is_some() {
            enabled_hooks.push(protocol::LifecycleHook::PreTool);
        }
        if self.post_tool.is_some() {
            enabled_hooks.push(protocol::LifecycleHook::PostTool);
        }
        enabled_hooks.extend(self.lifecycle.enabled());

        let tools = dispatcher.harness_declarations();
        // Custom instructions replace the harness's prompt, and with it the
        // workspace list, so they get the roots back as a note (opt-outable).
        let workspace_note = (self.announce_workspaces() && !self.workspaces.is_empty())
            .then(|| workspace_announcement(&self.workspaces));
        let subagent_context = SubagentWireContext {
            workspace_note: workspace_note.as_deref(),
            nesting: options.max_subagent_depth.is_some_and(|depth| depth >= 2),
            disable_define_subagent_tool: !options.define_subagent_tool,
        };
        let custom_subagents = self
            .subagents
            .iter()
            .map(|subagent| subagent.to_wire(&tools, subagent_context))
            .collect();
        let system_instructions = self.system_instructions.as_ref().map(|instructions| {
            let note = instructions
                .is_custom()
                .then_some(workspace_note.as_deref())
                .flatten();
            instructions.to_wire(note)
        });

        let mut harness_side_tools = self.capabilities.to_harness_side_tools();
        harness_side_tools.subagents = Some(protocol::SubagentsConfig {
            enabled: self.capabilities.is_enabled(BuiltinTool::InvokeSubagent),
            max_nesting_depth: options
                .max_subagent_depth
                .map(|depth| i32::try_from(depth).unwrap_or(i32::MAX)),
            allowed_subagents: options.allowed_subagents.clone().unwrap_or_default(),
            disable_define_subagent_tool: !options.define_subagent_tool,
        });

        let compaction_threshold = options.compaction_threshold;
        protocol::HarnessConfig {
            cascade_id: self.conversation_id.clone(),
            system_instructions,
            tools,
            harness_side_tools: Some(harness_side_tools),
            // The legacy spelling, written beside `compaction_config` as the
            // Python SDK does.
            compaction_threshold,
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
            models: self.models(),
            enabled_hooks,
            custom_subagents,
            agent_behavior: self.agent_behavior.map(AgentBehavior::to_wire),
            policy_config: self.policy_config(engine),
            tool_output_truncation: options.tool_output_truncation.map(|max_tokens| {
                protocol::ToolOutputTruncation {
                    truncate: Some(protocol::TruncateStrategy {
                        max_tokens: i32::try_from(max_tokens).unwrap_or(i32::MAX),
                    }),
                }
            }),
            session_continuation_mode: options
                .session_continuation
                .map(SessionContinuation::to_wire),
            retry_config: options.retry.and_then(RetryConfig::to_wire),
            budget_config: options.budget.and_then(Budget::to_wire),
            compaction_config: compaction_threshold.map(|tokens| protocol::CompactionConfig {
                token_threshold: Some(tokens),
            }),
            skills_config: (!options.inline_skills.is_empty()).then(|| protocol::SkillsConfig {
                enabled: true,
                skills: options
                    .inline_skills
                    .iter()
                    .map(InlineSkill::to_wire)
                    .collect(),
            }),
            mcp_max_output_bytes: options
                .mcp_max_output_bytes
                .map(|bytes| i32::try_from(bytes).unwrap_or(i32::MAX)),
        }
    }

    /// The model configs: the text model (with its options), and the image
    /// model `generate_image` calls when that builtin is on. Empty without
    /// an API key: the harness then uses its environment's key and its own
    /// default model, and `spawn()` has refused any model option.
    fn models(&self) -> Vec<protocol::ModelConfig> {
        let Some(api_key) = self.api_key() else {
            return Vec::new();
        };
        let options = &self.options;
        let endpoint = |model_options| protocol::GeminiApiEndpoint {
            api_key: Some(api_key.to_string()),
            base_url: self.gemini_base_url.clone(),
            http_headers: options.model_headers.clone(),
            options: model_options,
        };
        let text_options = (options.thinking_level.is_some()
            || options.service_tier.is_some()
            || options.max_output_tokens.is_some())
        .then(|| protocol::GeminiModelOptions {
            thinking_level: options.thinking_level.as_ref().map(ToString::to_string),
            service_tier: options.service_tier.as_ref().map(ToString::to_string),
            max_output_tokens: options
                .max_output_tokens
                .map(|tokens| i32::try_from(tokens).unwrap_or(i32::MAX)),
        });
        let mut models = vec![protocol::ModelConfig {
            // A real model id, not a placeholder: this is the id the
            // harness is actually asked to run when the caller never
            // called `with_model`.
            name: Some(
                self.model
                    .clone()
                    .unwrap_or_else(|| crate::DEFAULT_MODEL.to_string()),
            ),
            types: vec![protocol::ModelType::Text],
            gemini_api_endpoint: Some(endpoint(text_options)),
            vertex_endpoint: None,
        }];
        // Without an image model the harness fails every generate_image
        // call ("model is empty", verified on 0.1.23); the Python SDK
        // always configures one.
        if options.image_model.is_some() || self.capabilities.is_enabled(BuiltinTool::GenerateImage)
        {
            models.push(protocol::ModelConfig {
                name: Some(
                    options
                        .image_model
                        .clone()
                        .unwrap_or_else(|| crate::DEFAULT_IMAGE_MODEL.to_string()),
                ),
                types: vec![protocol::ModelType::Image],
                gemini_api_endpoint: Some(endpoint(None)),
                vertex_endpoint: None,
            });
        }
        models
    }

    /// The harness-side policy config, when there is anything for it to
    /// do: workspace containment, or the auto policy. Sending one turns
    /// containment on unless it says otherwise, so a config sent for the
    /// auto policy alone writes containment off explicitly.
    fn policy_config(&self, engine: &PolicyEngine) -> Option<protocol::PolicyConfig> {
        // spawn() rejects an explicit opt-in with no workspace.
        let contain = self.workspace_containment.unwrap_or(true) && !self.workspaces.is_empty();
        let auto_config = engine.auto().map(|_| protocol::AutoPolicyConfig {
            enabled: true,
            model: None,
        });
        if !contain && auto_config.is_none() {
            return None;
        }
        Some(protocol::PolicyConfig {
            rules: Vec::new(),
            workspace_containment: (!contain).then_some(protocol::WorkspaceContainment::Disabled),
            auto_config,
        })
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
