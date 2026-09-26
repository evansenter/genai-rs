//! Agent configuration: [`AgentConfig`] and the typed configs that build
//! it, plus [`ThinkingSummaries`]. See the [parent module](super).

use serde::{Deserialize, Serialize};

use crate::wire_enum::wire_enum;

// =============================================================================
// Agent Configuration Types
// =============================================================================

wire_enum! {
    /// Thinking summaries configuration for agent output.
    ///
    /// When using thinking mode (via `with_thinking_level`), you can control
    /// whether the model's reasoning process is summarized in the output.
    pub enum ThinkingSummaries {
        /// Automatically include thinking summaries (default when thinking is enabled)
        Auto = "auto" | "THINKING_SUMMARIES_AUTO",
        /// Do not include thinking summaries
        None = "none" | "THINKING_SUMMARIES_NONE",
    }
    unknown(summaries_type, unknown_summaries_type)
}

impl ThinkingSummaries {
    /// Convert to the `agent_config` wire format (`"auto"` / `"none"`).
    ///
    /// This used to emit the SCREAMING_CASE `THINKING_SUMMARIES_*` form,
    /// which the API accepted when `DeepResearchConfig` was written.
    /// Verified live 2026-08-10, it no longer does:
    ///
    /// ```text
    /// The value 'THINKING_SUMMARIES_AUTO' is not supported for
    /// 'agent_config.thinking_summaries'. Supported values: 'auto', 'none'.
    /// ```
    ///
    /// So both contexts now take the lowercase spelling and this agrees
    /// with [`Serialize`]. The seam is kept rather than inlined because
    /// the two spellings diverged once and could again; deserialization
    /// still accepts either form (Evergreen).
    #[must_use]
    pub fn to_agent_config_value(&self) -> serde_json::Value {
        match self {
            ThinkingSummaries::Auto => serde_json::Value::String("auto".to_string()),
            ThinkingSummaries::None => serde_json::Value::String("none".to_string()),
            ThinkingSummaries::Unknown { summaries_type, .. } => {
                // For unknown values, preserve the original format
                serde_json::Value::String(summaries_type.clone())
            }
        }
    }
}

/// Agent-specific configuration for specialized agents.
///
/// This is a thin wrapper around JSON that provides full forward compatibility.
/// Use typed config structs like [`DeepResearchConfig`] for compile-time guidance,
/// or construct directly from JSON for unknown/future agent types.
///
/// # Usage
///
/// ## Typed configs (recommended for known agents)
/// ```
/// use genai_rs::{AgentConfig, DeepResearchConfig, ThinkingSummaries};
///
/// let config: AgentConfig = DeepResearchConfig::new()
///     .with_thinking_summaries(ThinkingSummaries::Auto)
///     .into();
/// ```
///
/// ## Raw JSON (for unknown/future agents)
/// ```
/// use genai_rs::AgentConfig;
///
/// let config = AgentConfig::from_value(serde_json::json!({
///     "type": "future-agent",
///     "newOption": true
/// }));
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentConfig(serde_json::Value);

impl AgentConfig {
    /// Create an agent config from a raw JSON value.
    ///
    /// Use this for unknown or future agent types that don't have typed config structs.
    #[must_use]
    pub fn from_value(value: serde_json::Value) -> Self {
        Self(value)
    }

    /// Access the underlying JSON value.
    #[must_use]
    pub fn as_value(&self) -> &serde_json::Value {
        &self.0
    }

    /// Get the agent config type (e.g., "deep-research", "dynamic").
    #[must_use]
    pub fn config_type(&self) -> Option<&str> {
        self.0.get("type").and_then(|v| v.as_str())
    }
}

wire_enum! {
    /// Visualization mode for the Deep Research agent.
    ///
    /// Controls whether the agent includes visualizations in its response.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"off"`, `"auto"`.
    pub enum Visualization {
        /// No visualizations in the response.
        Off = "off",
        /// The agent decides when to include visualizations.
        Auto = "auto",
    }
    unknown(visualization_type, unknown_visualization_type)
}

/// Configuration for Deep Research agent.
///
/// Deep Research agent performs comprehensive research tasks
/// and can optionally include thinking summaries, visualizations,
/// collaborative planning, and BigQuery access.
///
/// Known Deep Research agent IDs: `deep-research-pro-preview-12-2025`,
/// `deep-research-preview-04-2026`, `deep-research-max-preview-04-2026`.
/// See `docs/AGENTS_AND_BACKGROUND.md`.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, DeepResearchConfig, ThinkingSummaries, Visualization};
///
/// let config: AgentConfig = DeepResearchConfig::new()
///     .with_thinking_summaries(ThinkingSummaries::Auto)
///     .with_visualization(Visualization::Auto)
///     .with_collaborative_planning(true)
///     .with_bigquery_tool(true)
///     .into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct DeepResearchConfig {
    thinking_summaries: Option<ThinkingSummaries>,
    visualization: Option<Visualization>,
    collaborative_planning: Option<bool>,
    enable_bigquery_tool: Option<bool>,
}

impl DeepResearchConfig {
    /// Create a new Deep Research configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set thinking summaries mode.
    ///
    /// Controls whether the agent's reasoning process is summarized in output.
    #[must_use]
    pub fn with_thinking_summaries(mut self, summaries: ThinkingSummaries) -> Self {
        self.thinking_summaries = Some(summaries);
        self
    }

    /// Set the visualization mode (`off` | `auto`).
    #[must_use]
    pub fn with_visualization(mut self, visualization: Visualization) -> Self {
        self.visualization = Some(visualization);
        self
    }

    /// Enable (or disable) human-in-the-loop planning.
    ///
    /// When `true`, the agent first returns a research plan and only proceeds
    /// after the user confirms the plan in the next turn.
    #[must_use]
    pub fn with_collaborative_planning(mut self, enabled: bool) -> Self {
        self.collaborative_planning = Some(enabled);
        self
    }

    /// Enable (or disable) the BigQuery tool for the Deep Research agent.
    ///
    /// Server-side constraint (verified live 2026-07): the Gemini API
    /// rejects `agent_config.enable_bigquery_tool` — "not available on the
    /// Gemini API but it is available on the Gemini Enterprise Agent
    /// Platform" (Vertex-only).
    #[must_use]
    pub fn with_bigquery_tool(mut self, enabled: bool) -> Self {
        self.enable_bigquery_tool = Some(enabled);
        self
    }
}

impl From<DeepResearchConfig> for AgentConfig {
    fn from(config: DeepResearchConfig) -> Self {
        let mut map = serde_json::Map::new();
        map.insert(
            "type".into(),
            serde_json::Value::String("deep-research".into()),
        );
        if let Some(ts) = config.thinking_summaries {
            // Use agent_config format (THINKING_SUMMARIES_*), not generation_config format (auto/none)
            map.insert("thinking_summaries".into(), ts.to_agent_config_value());
        }
        if let Some(visualization) = config.visualization {
            map.insert(
                "visualization".into(),
                serde_json::to_value(&visualization)
                    .expect("Visualization serialization is infallible"),
            );
        }
        if let Some(planning) = config.collaborative_planning {
            map.insert(
                "collaborative_planning".into(),
                serde_json::Value::Bool(planning),
            );
        }
        if let Some(bigquery) = config.enable_bigquery_tool {
            map.insert(
                "enable_bigquery_tool".into(),
                serde_json::Value::Bool(bigquery),
            );
        }
        AgentConfig(serde_json::Value::Object(map))
    }
}

/// Configuration for Dynamic agent.
///
/// Dynamic agents adapt their behavior based on the task.
/// Currently has no configurable options.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, DynamicConfig};
///
/// let config: AgentConfig = DynamicConfig::new().into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct DynamicConfig;

impl DynamicConfig {
    /// Create a new Dynamic agent configuration.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl From<DynamicConfig> for AgentConfig {
    fn from(_: DynamicConfig) -> Self {
        AgentConfig(serde_json::json!({"type": "dynamic"}))
    }
}

/// Configuration for the server-side Antigravity coding agent.
///
/// This configures [`DEFAULT_ANTIGRAVITY_AGENT`](crate::DEFAULT_ANTIGRAVITY_AGENT) interactions that
/// run in Google's sandbox (an
/// [`environment`](super::InteractionRequest::environment) is **required** for that
/// agent) — distinct from the local-harness bridge in the `antigravity`
/// module (feature `antigravity`), which runs the agent on your
/// machine. The bare `antigravity` string is only the `agent_config` *type*
/// discriminant (which [`From`] sets), not an agent ID.
///
/// Probe notes (verified live 2026-08-09, standard API key):
/// `agent_config: {"type": "antigravity"}` and `max_total_tokens` are
/// **accepted** on `antigravity-preview-05-2026` (the server's validation
/// error enumerates the supported config types as `dynamic`,
/// `deep-research`, `code-mender`, `antigravity`). Setting `model` to a
/// value the agent doesn't offer returns 404 `not_found` — as
/// `gemini-3.6-flash` did when this was recorded, despite being a valid
/// model for ordinary interactions. The agent's model catalog is not
/// enumerable on a standard key, so leave `model` unset unless you know an
/// accepted value.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, AntigravityConfig};
///
/// let config: AgentConfig = AntigravityConfig::new()
///     .with_max_total_tokens(200_000)
///     .into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct AntigravityConfig {
    model: Option<String>,
    max_total_tokens: Option<i64>,
}

impl AntigravityConfig {
    /// Create a new Antigravity agent configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the model the agent uses for its reasoning loop.
    ///
    /// A value the agent does not offer fails the interaction with 404
    /// `not_found` — see the [type docs](AntigravityConfig) for the probe
    /// notes; leave unset unless you know an accepted value.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Cap the total tokens the agent may consume across its whole run.
    #[must_use]
    pub fn with_max_total_tokens(mut self, max_total_tokens: i64) -> Self {
        self.max_total_tokens = Some(max_total_tokens);
        self
    }
}

impl From<AntigravityConfig> for AgentConfig {
    fn from(config: AntigravityConfig) -> Self {
        let mut map = serde_json::Map::new();
        map.insert(
            "type".into(),
            serde_json::Value::String("antigravity".into()),
        );
        if let Some(model) = config.model {
            map.insert("model".into(), serde_json::Value::String(model));
        }
        if let Some(max) = config.max_total_tokens {
            map.insert("max_total_tokens".into(), serde_json::Value::from(max));
        }
        AgentConfig(serde_json::Value::Object(map))
    }
}

#[cfg(test)]
#[path = "agent_config_tests.rs"]
mod tests;
