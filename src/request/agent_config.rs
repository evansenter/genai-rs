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
    /// which the API accepted when `DeepResearchConfig` was written. It no
    /// longer does:
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
    /// Server-side constraint: the Gemini API
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
/// On a standard API key, `agent_config: {"type": "antigravity"}`,
/// `max_total_tokens` and `model` are **accepted** on
/// [`DEFAULT_ANTIGRAVITY_AGENT`](crate::DEFAULT_ANTIGRAVITY_AGENT). The
/// server's validation error enumerates the supported config types as
/// `dynamic`, `deep-research`, `code-mender`, `antigravity`. The agent
/// defaults to `gemini-3.8-flash`; Google's Antigravity Agent guide lists
/// the 3.8/3.7/3.6/3.5 Flash models and `gemini-3.5-flash-lite` as the other
/// `model` values. A value the agent doesn't offer returns 404 `not_found`.
/// Which models were probed is under AntigravityConfig in
/// `docs/ENUM_WIRE_FORMATS.md`.
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
    /// `not_found`; see the [type docs](AntigravityConfig) for the accepted
    /// values.
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

wire_enum! {
    /// What a CodeMender find request does with a finding.
    ///
    /// # Wire Format
    ///
    /// Lowercase strings: `"scan"`, `"verify"` (google-genai 2.34.0).
    pub enum CodeMenderMode {
        /// Scan the source for findings.
        Scan = "scan",
        /// Verify a known finding.
        Verify = "verify",
    }
    unknown(mode_type, unknown_mode_type)
}

/// A source file handed to the CodeMender agent: its path and its content.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CodeMenderFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

impl CodeMenderFile {
    /// A file at `path` holding `content`.
    #[must_use]
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            content: Some(content.into()),
        }
    }
}

/// The find request of a [`CodeMenderConfig`]: what to look for, in which
/// files.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CodeMenderFindRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finding_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<CodeMenderMode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_files: Vec<CodeMenderFile>,
}

impl CodeMenderFindRequest {
    /// An empty find request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Describes what to find.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Names the finding to work on.
    #[must_use]
    pub fn with_finding_id(mut self, finding_id: impl Into<String>) -> Self {
        self.finding_id = Some(finding_id.into());
        self
    }

    /// Scans for findings or verifies one.
    #[must_use]
    pub fn with_mode(mut self, mode: CodeMenderMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Adds a source file to search.
    #[must_use]
    pub fn add_source_file(mut self, file: CodeMenderFile) -> Self {
        self.source_files.push(file);
        self
    }
}

/// The fix request of a [`CodeMenderConfig`]: what to fix, in which files.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CodeMenderFixRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finding_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_files: Vec<CodeMenderFile>,
}

impl CodeMenderFixRequest {
    /// An empty fix request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Describes the fix.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Names the finding to fix.
    #[must_use]
    pub fn with_finding_id(mut self, finding_id: impl Into<String>) -> Self {
        self.finding_id = Some(finding_id.into());
        self
    }

    /// Adds a source file to fix.
    #[must_use]
    pub fn add_source_file(mut self, file: CodeMenderFile) -> Self {
        self.source_files.push(file);
        self
    }
}

/// Configuration for the CodeMender agent (`agent_config.type:
/// "code-mender"`), from the google-genai 2.34.0 bindings.
///
/// **Vertex-only.** The Gemini API answers `400 The value 'code-mender' is
/// not supported for 'agent_config.type' on the Gemini API, it is allowed on
/// the Gemini Enterprise Agent Platform`, so it is kept
/// and documented (D-005) but cannot be exercised here.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, CodeMenderConfig, CodeMenderFile, CodeMenderFindRequest, CodeMenderMode};
///
/// let config: AgentConfig = CodeMenderConfig::new()
///     .with_find_request(
///         CodeMenderFindRequest::new()
///             .with_mode(CodeMenderMode::Scan)
///             .add_source_file(CodeMenderFile::new("src/main.rs", "fn main() {}")),
///     )
///     .with_max_rounds(3)
///     .into();
/// assert_eq!(config.config_type(), Some("code-mender"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct CodeMenderConfig {
    model: Option<String>,
    session_id: Option<String>,
    max_rounds: Option<i64>,
    find_request: Option<CodeMenderFindRequest>,
    fix_request: Option<CodeMenderFixRequest>,
}

impl CodeMenderConfig {
    /// Create a new CodeMender configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the model the agent uses.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Continues the given CodeMender session.
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Caps the rounds of the session (wire: `session_config.max_rounds`).
    #[must_use]
    pub fn with_max_rounds(mut self, max_rounds: i64) -> Self {
        self.max_rounds = Some(max_rounds);
        self
    }

    /// Asks the agent to find issues.
    #[must_use]
    pub fn with_find_request(mut self, request: CodeMenderFindRequest) -> Self {
        self.find_request = Some(request);
        self
    }

    /// Asks the agent to fix an issue.
    #[must_use]
    pub fn with_fix_request(mut self, request: CodeMenderFixRequest) -> Self {
        self.fix_request = Some(request);
        self
    }
}

impl From<CodeMenderConfig> for AgentConfig {
    fn from(config: CodeMenderConfig) -> Self {
        // The request types hold only strings, lists and string enums.
        let to_value = |value: serde_json::Result<serde_json::Value>| {
            value.expect("CodeMender request serialization is infallible")
        };
        let mut map = serde_json::Map::new();
        map.insert(
            "type".into(),
            serde_json::Value::String("code-mender".into()),
        );
        if let Some(model) = config.model {
            map.insert("model".into(), serde_json::Value::String(model));
        }
        if let Some(session_id) = config.session_id {
            map.insert("session_id".into(), serde_json::Value::String(session_id));
        }
        if let Some(max_rounds) = config.max_rounds {
            map.insert(
                "session_config".into(),
                serde_json::json!({ "max_rounds": max_rounds }),
            );
        }
        if let Some(find) = &config.find_request {
            map.insert("find_request".into(), to_value(serde_json::to_value(find)));
        }
        if let Some(fix) = &config.fix_request {
            map.insert("fix_request".into(), to_value(serde_json::to_value(fix)));
        }
        AgentConfig(serde_json::Value::Object(map))
    }
}

#[cfg(test)]
#[path = "agent_config_tests.rs"]
mod tests;
