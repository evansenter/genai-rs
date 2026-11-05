//! Model, subagent and harness option setters on [`AgentBuilder`]: the
//! Python SDK's model options, subagent controls and `AgentConfig` /
//! `CapabilitiesConfig` settings (#493).

use std::collections::BTreeMap;

use super::AgentBuilder;
use crate::antigravity::{Budget, InlineSkill, RetryConfig, SessionContinuation};
use crate::{ServiceTier, ThinkingLevel};

/// The options these setters collect, read by `validate.rs` and `wire.rs`.
#[derive(Debug, Clone, Default)]
pub(super) struct HarnessOptions {
    pub(super) thinking_level: Option<ThinkingLevel>,
    pub(super) service_tier: Option<ServiceTier>,
    pub(super) max_output_tokens: Option<u32>,
    pub(super) image_model: Option<String>,
    pub(super) model_headers: BTreeMap<String, String>,
    pub(super) max_subagent_depth: Option<u32>,
    pub(super) allowed_subagents: Option<Vec<String>>,
    pub(super) define_subagent_tool: bool,
    pub(super) budget: Option<Budget>,
    pub(super) retry: Option<RetryConfig>,
    pub(super) compaction_threshold: Option<u32>,
    pub(super) tool_output_truncation: Option<u32>,
    pub(super) mcp_max_output_bytes: Option<u32>,
    pub(super) inline_skills: Vec<InlineSkill>,
    pub(super) session_continuation: Option<SessionContinuation>,
    pub(super) harness_env: BTreeMap<String, String>,
}

impl HarnessOptions {
    /// Whether any option that rides on the model config is set; those need
    /// an API key, since without one no model config is sent.
    pub(super) fn needs_model_config(&self) -> Option<&'static str> {
        [
            (self.thinking_level.is_some(), "with_thinking_level"),
            (self.service_tier.is_some(), "with_service_tier"),
            (self.max_output_tokens.is_some(), "with_max_output_tokens"),
            (self.image_model.is_some(), "with_image_model"),
            (!self.model_headers.is_empty(), "add_model_header"),
        ]
        .into_iter()
        .find_map(|(set, setter)| set.then_some(setter))
    }
}

impl AgentBuilder {
    // -------------------------------------------------------------------
    // Model options (Python: GeminiModelOptions, ModelTarget, endpoints)
    // -------------------------------------------------------------------

    /// Sets how much the model reasons before it answers (Python:
    /// `GeminiModelOptions.thinking_level`; its `.eval()` preset uses
    /// [`ThinkingLevel::High`]). The harness sends it as
    /// `generationConfig.thinkingConfig.thinkingLevel` with thoughts
    /// included. Not every model takes every level. Needs
    /// [`Self::with_api_key`].
    #[must_use]
    pub fn with_thinking_level(mut self, level: ThinkingLevel) -> Self {
        self.options.thinking_level = Some(level);
        self
    }

    /// Sets the inference service tier (Python:
    /// `GeminiModelOptions.service_tier`), sent as the request's
    /// `serviceTier`. [`ServiceTier::Deferred`] is Vertex-only. Needs
    /// [`Self::with_api_key`].
    #[must_use]
    pub fn with_service_tier(mut self, tier: ServiceTier) -> Self {
        self.options.service_tier = Some(tier);
        self
    }

    /// Caps each model call's output (Python:
    /// `GeminiModelOptions.max_output_tokens`; harness default 65535).
    /// Must be positive. Needs [`Self::with_api_key`].
    #[must_use]
    pub fn with_max_output_tokens(mut self, tokens: u32) -> Self {
        self.options.max_output_tokens = Some(tokens);
        self
    }

    /// Sets the model the `generate_image` builtin calls (default, when
    /// [`BuiltinTool::GenerateImage`](crate::antigravity::BuiltinTool::GenerateImage)
    /// is enabled: [`DEFAULT_IMAGE_MODEL`](crate::DEFAULT_IMAGE_MODEL)). It
    /// shares the text model's endpoint. Needs [`Self::with_api_key`].
    #[must_use]
    pub fn with_image_model(mut self, model: impl Into<String>) -> Self {
        self.options.image_model = Some(model.into());
        self
    }

    /// Adds an HTTP header to every model request (Python:
    /// `ModelEndpoint.http_headers`), for a gateway that routes or meters
    /// on one. Needs [`Self::with_api_key`].
    #[must_use]
    pub fn add_model_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.model_headers.insert(name.into(), value.into());
        self
    }

    // -------------------------------------------------------------------
    // Subagent controls (Python: CapabilitiesConfig, SubagentCapabilities)
    // -------------------------------------------------------------------

    /// Lets subagents nest: `2` lets a subagent invoke a subagent of its
    /// own (Python: `CapabilitiesConfig.max_subagent_depth`; the harness
    /// default is 1, flat). A subagent then needs
    /// [`BuiltinTool::InvokeSubagent`](crate::antigravity::BuiltinTool::InvokeSubagent)
    /// in its capabilities to use it. Needs the builtin enabled on this
    /// agent; `spawn()` fails with [`AntigravityError::Config`] otherwise,
    /// or on `0`.
    ///
    /// [`AntigravityError::Config`]: crate::antigravity::AntigravityError::Config
    #[must_use]
    pub fn with_max_subagent_depth(mut self, depth: u32) -> Self {
        self.options.max_subagent_depth = Some(depth);
        self
    }

    /// Limits which subagents this agent may invoke (Python:
    /// `CapabilitiesConfig.allowed_subagents`; default: all of them). Every
    /// name must be a subagent added with [`Self::add_subagent`], and the
    /// subagent builtin must be enabled; `spawn()` checks both.
    #[must_use]
    pub fn with_allowed_subagents(
        mut self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.options.allowed_subagents = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Offers the model `define_subagent`, with which it writes new
    /// subagents (instructions and tools of its own choosing) at runtime.
    /// **Off by default**: enabling the subagent builtin gives the model
    /// your static subagents, `invoke_subagent`, `manage_subagents` and
    /// `send_message`, but not this. The Python SDK leaves it on (it has no
    /// setting for it).
    #[must_use]
    pub fn with_define_subagent_tool(mut self, enabled: bool) -> Self {
        self.options.define_subagent_tool = enabled;
        self
    }

    // -------------------------------------------------------------------
    // Harness config (Python: AgentConfig, CapabilitiesConfig)
    // -------------------------------------------------------------------

    /// Caps the session's model calls, tool calls and tokens (Python:
    /// `budget_config`). A turn that reaches a cap ends early, with
    /// [`ChatResponse::stop_reason`](crate::antigravity::ChatResponse::stop_reason)
    /// naming it.
    #[must_use]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.options.budget = Some(budget);
        self
    }

    /// Sets how the harness retries failed model calls and unparseable
    /// model output (Python: `retry_config`).
    #[must_use]
    pub fn with_retry(mut self, retry: RetryConfig) -> Self {
        self.options.retry = Some(retry);
        self
    }

    /// Compacts the conversation history once it passes `tokens` (Python:
    /// `CompactionConfig.token_threshold`). Compaction costs a model call
    /// that writes the summary;
    /// [`on_compaction`](Self::on_compaction) observes it. The threshold
    /// must leave room for the harness's own prompt and tool declarations
    /// (about 2,700 tokens on 0.1.23 with the read-only tools), or the turn
    /// fails.
    #[must_use]
    pub fn with_compaction_threshold(mut self, tokens: u32) -> Self {
        self.options.compaction_threshold = Some(tokens);
        self
    }

    /// Cuts each tool's output to about `max_tokens` estimated tokens
    /// before the model sees it (Python: `tool_output_truncation_config`;
    /// `0` turns truncation off).
    #[must_use]
    pub fn with_tool_output_truncation(mut self, max_tokens: u32) -> Self {
        self.options.tool_output_truncation = Some(max_tokens);
        self
    }

    /// Writes an MCP tool's output to a file, and gives the model the
    /// path, once it passes `bytes` (Python: `mcp_max_output_bytes`;
    /// harness default 4096). Must be positive.
    #[must_use]
    pub fn with_mcp_max_output_bytes(mut self, bytes: u32) -> Self {
        self.options.mcp_max_output_bytes = Some(bytes);
        self
    }

    /// Adds a skill defined in code (Python: `inline_skills`). Cannot be
    /// combined with [`Self::add_skills_path`]: the harness supports one
    /// source or the other, and `spawn()` refuses both.
    #[must_use]
    pub fn add_inline_skill(mut self, skill: InlineSkill) -> Self {
        self.options.inline_skills.push(skill);
        self
    }

    /// Sets whether [`Self::with_conversation_id`] must name a saved
    /// conversation, must not, or either (Python:
    /// `session_continuation_mode`). With
    /// [`SessionContinuation::Resume`] and no saved conversation of that
    /// id, `spawn()` fails with the harness's reason (`conversation "…" not
    /// found`).
    #[must_use]
    pub fn with_session_continuation(mut self, mode: SessionContinuation) -> Self {
        self.options.session_continuation = Some(mode);
        self
    }

    /// Adds an environment variable for the harness process (Python:
    /// `env`), which commands it runs (`run_command`) and stdio MCP servers
    /// inherit. With [`Self::with_gemini_base_url`] set, `GEMINI_API_KEY`
    /// and `GOOGLE_API_KEY` are refused (`spawn()` fails): the harness
    /// would send them to the custom host.
    #[must_use]
    pub fn add_harness_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.harness_env.insert(key.into(), value.into());
        self
    }
}
