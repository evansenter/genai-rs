//! Harness option types: session budgets, model retries, `run_command`
//! settings, inline skills, subagent skills and session continuation.
//!
//! Each reaches the harness's `HarnessConfig` through an
//! [`AgentBuilder`](super::AgentBuilder) setter or
//! [`Capabilities`](super::Capabilities) /
//! [`Subagent`](super::Subagent); the Python SDK name is given on each type.

use std::collections::BTreeMap;
use std::time::Duration;

use super::protocol;

// =============================================================================
// Budgets
// =============================================================================

/// Session caps on model calls, tool calls and tokens (the Python SDK's
/// `BudgetConfig`). Set with
/// [`AgentBuilder::with_budget`](super::AgentBuilder::with_budget).
///
/// A turn that reaches a cap still *finishes*: the harness stops it and
/// [`ChatResponse::stop_reason`](super::ChatResponse::stop_reason) names
/// the cap (`StopReason::MaxToolCallsExceeded`, …).
///
/// ```
/// use genai_rs::antigravity::{Budget, BudgetScope};
///
/// let budget = Budget::new()
///     .with_max_tool_calls(20)
///     .with_max_total_tokens(200_000)
///     .with_scope(BudgetScope::ForwardLooking);
/// assert!(!budget.is_empty());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Budget {
    max_model_calls: Option<u32>,
    max_tool_calls: Option<u32>,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    max_total_tokens: Option<u64>,
    scope: BudgetScope,
}

/// Where a [`Budget`] starts counting (the Python SDK's `BudgetScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum BudgetScope {
    /// From the start of the conversation, a resumed one included.
    #[default]
    Lifetime,
    /// From when the session started or was resumed.
    ForwardLooking,
}

impl Budget {
    /// A budget with no caps; add them with the `with_*` methods.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Caps model calls.
    #[must_use]
    pub fn with_max_model_calls(mut self, calls: u32) -> Self {
        self.max_model_calls = Some(calls);
        self
    }

    /// Caps tool calls, whatever runs them.
    #[must_use]
    pub fn with_max_tool_calls(mut self, calls: u32) -> Self {
        self.max_tool_calls = Some(calls);
        self
    }

    /// Caps uncached input tokens (prompt tokens minus cached ones).
    #[must_use]
    pub fn with_max_input_tokens(mut self, tokens: u64) -> Self {
        self.max_input_tokens = Some(tokens);
        self
    }

    /// Caps output tokens (candidates plus thoughts).
    #[must_use]
    pub fn with_max_output_tokens(mut self, tokens: u64) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// Caps uncached input plus output tokens.
    #[must_use]
    pub fn with_max_total_tokens(mut self, tokens: u64) -> Self {
        self.max_total_tokens = Some(tokens);
        self
    }

    /// Where counting starts (default [`BudgetScope::Lifetime`]).
    #[must_use]
    pub fn with_scope(mut self, scope: BudgetScope) -> Self {
        self.scope = scope;
        self
    }

    /// Whether no cap is set (such a budget is not sent).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.max_model_calls.is_none()
            && self.max_tool_calls.is_none()
            && self.max_input_tokens.is_none()
            && self.max_output_tokens.is_none()
            && self.max_total_tokens.is_none()
    }

    /// The first cap that is zero, by name: the harness wants every cap at
    /// least 1 (as the Python SDK enforces).
    pub(crate) fn zero_cap(&self) -> Option<&'static str> {
        [
            ("max_model_calls", self.max_model_calls.map(u64::from)),
            ("max_tool_calls", self.max_tool_calls.map(u64::from)),
            ("max_input_tokens", self.max_input_tokens),
            ("max_output_tokens", self.max_output_tokens),
            ("max_total_tokens", self.max_total_tokens),
        ]
        .into_iter()
        .find(|(_, cap)| *cap == Some(0))
        .map(|(name, _)| name)
    }

    pub(crate) fn to_wire(self) -> Option<protocol::BudgetConfig> {
        // Above i32::MAX / i64::MAX a cap means "no cap"; saturate rather
        // than wrap into a negative the harness would reject.
        let calls = |cap: Option<u32>| cap.map(|c| i32::try_from(c).unwrap_or(i32::MAX));
        let tokens = |cap: Option<u64>| cap.map(|c| i64::try_from(c).unwrap_or(i64::MAX));
        (!self.is_empty()).then(|| protocol::BudgetConfig {
            max_model_calls: calls(self.max_model_calls),
            max_tool_calls: calls(self.max_tool_calls),
            max_input_tokens: tokens(self.max_input_tokens),
            max_output_tokens: tokens(self.max_output_tokens),
            max_total_tokens: tokens(self.max_total_tokens),
            scope: Some(match self.scope {
                BudgetScope::Lifetime => protocol::BudgetScope::Lifetime,
                BudgetScope::ForwardLooking => protocol::BudgetScope::ForwardLooking,
            }),
        })
    }
}

// =============================================================================
// Retries
// =============================================================================

/// How the harness retries failed model calls and unparseable model output
/// (the Python SDK's `RetryConfig`). Set with
/// [`AgentBuilder::with_retry`](super::AgentBuilder::with_retry). Anything
/// left unset keeps the harness's own default.
///
/// ```
/// use genai_rs::antigravity::RetryConfig;
/// use std::time::Duration;
///
/// let retry = RetryConfig::new()
///     .with_max_api_retries(5)
///     .with_initial_backoff(Duration::from_millis(500))
///     .with_backoff_multiplier(2.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RetryConfig {
    max_api_retries: Option<u32>,
    initial_backoff: Option<Duration>,
    backoff_multiplier: Option<f64>,
    jitter: Option<f64>,
    max_output_retries: Option<u32>,
}

impl RetryConfig {
    /// The harness defaults; override them with the `with_*` methods.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The Python SDK's `RetryConfig.benchmark()`: retry transient model
    /// API errors (429, 503) without limit, starting at one second, so a
    /// long evaluation run outlasts rate limiting.
    #[must_use]
    pub fn benchmark() -> Self {
        Self::new()
            .with_max_api_retries(u32::MAX)
            .with_initial_backoff(Duration::from_secs(1))
    }

    /// Retries of a model call that failed with a transient error. `0`
    /// fails the turn on the first one.
    #[must_use]
    pub fn with_max_api_retries(mut self, retries: u32) -> Self {
        self.max_api_retries = Some(retries);
        self
    }

    /// The first retry's backoff (millisecond resolution).
    #[must_use]
    pub fn with_initial_backoff(mut self, backoff: Duration) -> Self {
        self.initial_backoff = Some(backoff);
        self
    }

    /// The backoff's growth factor between retries.
    #[must_use]
    pub fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = Some(multiplier);
        self
    }

    /// The jitter range applied to each backoff.
    #[must_use]
    pub fn with_jitter(mut self, jitter: f64) -> Self {
        self.jitter = Some(jitter);
        self
    }

    /// Retries of model output the harness cannot parse (a malformed tool
    /// call).
    #[must_use]
    pub fn with_max_output_retries(mut self, retries: u32) -> Self {
        self.max_output_retries = Some(retries);
        self
    }

    pub(crate) fn to_wire(self) -> Option<protocol::RetryConfig> {
        let api = (self.max_api_retries.is_some()
            || self.initial_backoff.is_some()
            || self.backoff_multiplier.is_some()
            || self.jitter.is_some())
        .then(|| protocol::ModelApiRetryConfig {
            max_retries: self.max_api_retries,
            initial_sleep_duration_ms: self
                .initial_backoff
                .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX)),
            exponential_multiplier: self.backoff_multiplier,
            jitter_range: self.jitter,
        });
        let output = self
            .max_output_retries
            .map(|retries| protocol::ModelOutputRetryConfig {
                max_retries: Some(retries),
            });
        (api.is_some() || output.is_some()).then_some(protocol::RetryConfig {
            api_retry: api,
            model_output_retry: output,
        })
    }
}

// =============================================================================
// run_command
// =============================================================================

/// Settings for the `run_command` builtin (the Python SDK's
/// `RunCommandConfig`), set with
/// [`Capabilities::with_run_command`](super::Capabilities::with_run_command).
///
/// ```
/// use genai_rs::antigravity::{BuiltinTool, Capabilities, RunCommandOptions};
/// use std::time::Duration;
///
/// let caps = Capabilities::read_only()
///     .enable(BuiltinTool::RunCommand)
///     .with_run_command(RunCommandOptions::new().with_timeout(Duration::from_secs(30)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunCommandOptions {
    timeout: Option<Duration>,
    daemons: bool,
    sandbox: bool,
}

impl RunCommandOptions {
    /// The harness defaults (ten-minute commands, no daemons, no sandbox).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Kills a command that runs longer than `timeout` (millisecond
    /// resolution); the model is told it timed out.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Offers the model `IsDaemon` on `run_command`, for long-running
    /// background processes (dev servers, watchers) that do not hold the
    /// turn open.
    #[must_use]
    pub fn with_daemons(mut self, daemons: bool) -> Self {
        self.daemons = daemons;
        self
    }

    /// Runs commands inside the OS sandbox. Where the harness reports none
    /// available ([`AntigravityAgent::sandbox_status`](super::AntigravityAgent::sandbox_status)
    /// `available: Some(false)`, as on GitHub's Ubuntu 24.04 runners), a
    /// command fails instead of running unsandboxed (harness 0.1.23).
    #[must_use]
    pub fn with_sandbox(mut self, sandbox: bool) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// Whether a sandbox was asked for.
    #[must_use]
    pub fn sandbox(&self) -> bool {
        self.sandbox
    }

    pub(crate) fn to_wire(self, enabled: bool) -> protocol::RunCommandToolConfig {
        protocol::RunCommandToolConfig {
            enabled,
            max_timeout_ms: self
                .timeout
                .map(|t| u32::try_from(t.as_millis()).unwrap_or(u32::MAX).max(1)),
            enable_daemon_commands: self.daemons,
            enable_sandbox: self.sandbox,
        }
    }
}

// =============================================================================
// Skills
// =============================================================================

/// A skill defined in code rather than in a `SKILL.md` file (the Python
/// SDK's `InlineSkill`). Add with
/// [`AgentBuilder::add_inline_skill`](super::AgentBuilder::add_inline_skill).
///
/// The harness lists each skill's name and description in its prompt and
/// gives the model `list_skills`, `lookup_skill` and `lookup_skills`; the
/// content is what a lookup returns.
///
/// ```
/// use genai_rs::antigravity::InlineSkill;
///
/// let skill = InlineSkill::new(
///     "changelog-entry",
///     "How to write a CHANGELOG entry for this repository.",
///     "One sentence per change, past tense, with the issue number.",
/// )
/// .add_allowed_tool("view_file");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSkill {
    name: String,
    description: String,
    content: String,
    allowed_tools: Vec<String>,
    dependent_tools: Vec<String>,
    dependent_skills: Vec<String>,
    metadata: BTreeMap<String, String>,
}

impl InlineSkill {
    /// A skill: a unique `name`, the `description` the model chooses it
    /// by, and the `content` (Markdown instructions) it reads.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            content: content.into(),
            allowed_tools: Vec::new(),
            dependent_tools: Vec::new(),
            dependent_skills: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    /// Adds a tool the skill may use.
    #[must_use]
    pub fn add_allowed_tool(mut self, tool: impl Into<String>) -> Self {
        self.allowed_tools.push(tool.into());
        self
    }

    /// Adds a tool loaded when the skill is looked up.
    #[must_use]
    pub fn add_dependent_tool(mut self, tool: impl Into<String>) -> Self {
        self.dependent_tools.push(tool.into());
        self
    }

    /// Adds a skill this one depends on.
    #[must_use]
    pub fn add_dependent_skill(mut self, skill: impl Into<String>) -> Self {
        self.dependent_skills.push(skill.into());
        self
    }

    /// Adds a metadata entry (`visibility: hidden`, …).
    #[must_use]
    pub fn add_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// The skill's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn to_wire(&self) -> protocol::SkillSource {
        // The harness reads dependencies from metadata as JSON lists, as
        // the Python SDK writes them; an explicit metadata entry wins.
        let mut metadata = self.metadata.clone();
        for (key, list) in [
            ("dependent_tools", &self.dependent_tools),
            ("dependent_skills", &self.dependent_skills),
        ] {
            if !list.is_empty() {
                metadata
                    .entry(key.to_string())
                    .or_insert_with(|| serde_json::Value::from(list.clone()).to_string());
            }
        }
        protocol::SkillSource {
            skill: Some(protocol::Skill {
                name: Some(self.name.clone()),
                description: Some(self.description.clone()),
                content: Some(self.content.clone()),
                allowed_tools: self.allowed_tools.clone(),
                metadata,
            }),
        }
    }
}

/// Which skills a [`Subagent`](super::Subagent) sees (the Python SDK's
/// `SubagentSkillsConfig`). Unset, a subagent inherits all of its
/// parent's.
///
/// ```
/// use genai_rs::antigravity::SubagentSkills;
///
/// let only_two = SubagentSkills::inherit()
///     .add_skill_name("changelog-entry")
///     .add_skill_name("release-checklist");
/// let none = SubagentSkills::none();
/// let own = SubagentSkills::from_paths(["/repo/.skills/reviewer"]);
/// # let _ = (only_two, none, own);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentSkills {
    mode: SkillsMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SkillsMode {
    Inherit {
        skill_names: Vec<String>,
        extra_paths: Vec<String>,
    },
    None,
    Override {
        paths: Vec<String>,
    },
}

impl SubagentSkills {
    /// The parent's skills: all of them, or those named with
    /// [`add_skill_name`](Self::add_skill_name).
    #[must_use]
    pub fn inherit() -> Self {
        Self {
            mode: SkillsMode::Inherit {
                skill_names: Vec::new(),
                extra_paths: Vec::new(),
            },
        }
    }

    /// No skills, and no skill tools.
    #[must_use]
    pub fn none() -> Self {
        Self {
            mode: SkillsMode::None,
        }
    }

    /// Skills from these directories instead of the parent's. (The Python
    /// SDK's inline-skill override is refused by the local harness.)
    #[must_use]
    pub fn from_paths(paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            mode: SkillsMode::Override {
                paths: paths.into_iter().map(Into::into).collect(),
            },
        }
    }

    /// With [`inherit`](Self::inherit): keeps only the named parent skills.
    /// Ignored in the other modes.
    #[must_use]
    pub fn add_skill_name(mut self, name: impl Into<String>) -> Self {
        if let SkillsMode::Inherit { skill_names, .. } = &mut self.mode {
            skill_names.push(name.into());
        }
        self
    }

    /// With [`inherit`](Self::inherit): also searches this directory for
    /// `SKILL.md` files. Ignored in the other modes.
    #[must_use]
    pub fn add_extra_path(mut self, path: impl Into<String>) -> Self {
        if let SkillsMode::Inherit { extra_paths, .. } = &mut self.mode {
            extra_paths.push(path.into());
        }
        self
    }

    pub(crate) fn to_wire(&self) -> protocol::SubagentSkillsConfig {
        match &self.mode {
            SkillsMode::Inherit {
                skill_names,
                extra_paths,
            } => protocol::SubagentSkillsConfig {
                inherit_config: Some(protocol::SubagentSkillsInherit {
                    extra_skills_paths: extra_paths.clone(),
                    skill_names: skill_names.clone(),
                }),
                ..Default::default()
            },
            SkillsMode::None => protocol::SubagentSkillsConfig {
                none_config: Some(protocol::SubagentSkillsNone {}),
                ..Default::default()
            },
            SkillsMode::Override { paths } => protocol::SubagentSkillsConfig {
                override_config: Some(protocol::SubagentSkillsOverride {
                    skills_paths: paths.clone(),
                }),
                ..Default::default()
            },
        }
    }
}

// =============================================================================
// Session continuation
// =============================================================================

/// Whether [`with_conversation_id`](super::AgentBuilder::with_conversation_id)
/// must name a saved conversation, must not, or either (the Python SDK's
/// `SessionContinuationMode`). Set with
/// [`AgentBuilder::with_session_continuation`](super::AgentBuilder::with_session_continuation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionContinuation {
    /// Resume the conversation; `spawn()` fails when it does not exist.
    Resume,
    /// Resume it when it exists, else start it under that id.
    CreateOrResume,
    /// Start it; `spawn()` fails when it already exists.
    CreateOnly,
}

impl SessionContinuation {
    pub(crate) fn to_wire(self) -> protocol::SessionContinuationMode {
        match self {
            Self::Resume => protocol::SessionContinuationMode::Resume,
            Self::CreateOrResume => protocol::SessionContinuationMode::CreateOrResume,
            Self::CreateOnly => protocol::SessionContinuationMode::CreateOnly,
        }
    }
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
