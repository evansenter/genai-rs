//! `HarnessConfig` option messages: budgets, retries, compaction, output
//! truncation, skills, subagent and `run_command` settings, and the
//! harness-side policy evaluator's rules.

use super::{BudgetScope, RuleDecision, ToolToggle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `SubagentsConfig` — the subagent builtins' settings
/// (`HarnessSideTools.subagents`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubagentsConfig {
    /// Whether the agent may invoke subagents.
    pub enabled: bool,
    /// How deep subagents may nest (`0` = the harness default, 1: flat).
    /// Read from the root agent's config only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_nesting_depth: Option<i32>,
    /// The subagents this agent may invoke (empty = all).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub allowed_subagents: Vec<String>,
    /// Hides `define_subagent`, with which the model writes its own
    /// subagents at runtime.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub disable_define_subagent_tool: bool,
}

impl From<ToolToggle> for SubagentsConfig {
    fn from(toggle: ToolToggle) -> Self {
        Self {
            enabled: toggle.enabled,
            ..Default::default()
        }
    }
}

/// `RunCommandToolConfig` — the `run_command` builtin's settings
/// (`HarnessSideTools.run_command`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunCommandToolConfig {
    /// Whether the agent may run commands.
    pub enabled: bool,
    /// Per-command time limit in milliseconds (`0` = the harness default,
    /// ten minutes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_timeout_ms: Option<u32>,
    /// Offers `IsDaemon` on the tool, for long-running background
    /// processes that do not hold the turn open.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub enable_daemon_commands: bool,
    /// Runs commands in the OS sandbox where the platform has one.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub enable_sandbox: bool,
}

impl From<ToolToggle> for RunCommandToolConfig {
    fn from(toggle: ToolToggle) -> Self {
        Self {
            enabled: toggle.enabled,
            ..Default::default()
        }
    }
}

/// `ToolOutputTruncation` (oneof `strategy`; this crate writes `truncate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutputTruncation {
    /// Cut long tool output to a token budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncate: Option<TruncateStrategy>,
}

/// `ToolOutputTruncation.TruncateStrategy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TruncateStrategy {
    /// Estimated-token budget for one tool output (`0` disables
    /// truncation).
    pub max_tokens: i32,
}

/// `RetryConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetryConfig {
    /// Retries of model API calls that fail with a transient error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_retry: Option<ModelApiRetryConfig>,
    /// Retries of model output the harness cannot parse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_output_retry: Option<ModelOutputRetryConfig>,
}

/// `ModelAPIRetryConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelApiRetryConfig {
    /// Maximum retries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// First backoff, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_sleep_duration_ms: Option<u32>,
    /// Backoff growth factor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exponential_multiplier: Option<f64>,
    /// Jitter range applied to each backoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter_range: Option<f64>,
}

/// `ModelOutputRetryConfig`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelOutputRetryConfig {
    /// Maximum retries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
}

/// `BudgetConfig` — caps whose breach stops a turn with a
/// [`StopReason`](super::StopReason).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BudgetConfig {
    /// Model calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_model_calls: Option<i32>,
    /// Tool calls, from any source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<i32>,
    /// Uncached input tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<i64>,
    /// Output tokens (candidates plus thoughts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    /// Uncached input plus output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<i64>,
    /// Where the counting starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<BudgetScope>,
}

/// `CompactionConfig`. Of its fields this crate writes only
/// `token_threshold`, as the Python SDK does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CompactionConfig {
    /// History size, in tokens, past which the harness compacts it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_threshold: Option<u32>,
}

/// `SkillsConfig` — skills defined in the config rather than found on
/// disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillsConfig {
    /// Turns the skills tools on.
    pub enabled: bool,
    /// The skills.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skills: Vec<SkillSource>,
}

/// `SkillSource` (oneof `source`; this crate writes `skill`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillSource {
    /// An inline skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<Skill>,
}

/// `genai.skills.Skill`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// Unique name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When the model should use it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The instructions the model reads when it looks the skill up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tools the skill may use.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub allowed_tools: Vec<String>,
    /// Free-form metadata. `dependent_tools` and `dependent_skills` hold
    /// JSON lists the harness parses.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub metadata: BTreeMap<String, String>,
}

/// `SubagentSkillsConfig` (oneof `mode`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSkillsConfig {
    /// Inherit the parent's skills, optionally filtered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherit_config: Option<SubagentSkillsInherit>,
    /// No skills.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none_config: Option<SubagentSkillsNone>,
    /// Replace the parent's skills.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_config: Option<SubagentSkillsOverride>,
}

/// `SubagentSkillsConfig.InheritConfig`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSkillsInherit {
    /// Extra directories to search for `SKILL.md` files.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extra_skills_paths: Vec<String>,
    /// The parent skills to keep (empty = all).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skill_names: Vec<String>,
}

/// `SubagentSkillsConfig.NoneConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SubagentSkillsNone {}

/// `SubagentSkillsConfig.OverrideConfig`. Inline `skill_sources` are not
/// written: the Python SDK refuses them on the local harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSkillsOverride {
    /// Directories to search for `SKILL.md` files.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub skills_paths: Vec<String>,
}

/// `PolicyRule` — one rule for the harness-side policy evaluator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PolicyRule {
    /// The tool the rule targets (`"*"` for every tool).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The MCP server, for a rule over MCP tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    /// A label (the harness quotes it in its default denial message).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the rule decides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<RuleDecision>,
    /// The message the model sees on a denial.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_reason: Option<String>,
    /// Whether the harness must ask the client to evaluate the rule.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub is_dynamic: bool,
    /// The id a `policyDecisionRequest` for a dynamic rule carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

/// `AutoPolicyConfig` — the harness's model-judged safety assessment of
/// risky tool calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoPolicyConfig {
    /// Turns the assessment on.
    pub enabled: bool,
    /// The assessing model (empty = the harness's choice).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
