//! Hook types for Antigravity agents: what each hook is handed and what it
//! returns. Tool policies live in `policies.rs`.

use serde_json::Value;
use std::sync::Arc;

/// A tool call the agent is about to make (or asking permission for).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ToolInvocation {
    /// The tool name (see the module docs for naming of builtins/MCP tools).
    pub name: String,
    /// The tool arguments.
    pub args: Value,
    /// The harness's correlation id for custom tool calls, when available.
    pub id: Option<String>,
}

impl ToolInvocation {
    /// A call to `name` with `args`, so policy conditions and handlers can
    /// be unit-tested without a harness (the struct is `#[non_exhaustive]`).
    ///
    /// ```
    /// use genai_rs::antigravity::ToolInvocation;
    ///
    /// let call = ToolInvocation::new("run_command", serde_json::json!({"CommandLine": "ls"}));
    /// assert_eq!(call.args["CommandLine"], "ls");
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, args: Value) -> Self {
        Self {
            name: name.into(),
            args,
            id: None,
        }
    }
}

/// The outcome of a completed tool call, passed to post-tool hooks.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ToolOutcome {
    /// The tool name.
    pub name: String,
    /// The result, when the call succeeded.
    pub result: Option<String>,
    /// The error, when the call failed.
    pub error: Option<String>,
}

/// A pre-tool hook's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreToolDecision {
    /// Let the tool run.
    Allow,
    /// Block the tool; the reason is surfaced to the model.
    Deny {
        /// Why the call was blocked.
        reason: String,
    },
    /// Let the tool run with these arguments in place of the model's (the
    /// Python SDK's `HookResult(modified_args=...)`). The harness runs a
    /// builtin, MCP or custom tool on the replacement.
    Modify {
        /// The arguments the tool runs with: a JSON object in the tool's
        /// own schema (`AbsolutePath`, `CommandLine`, …).
        args: Value,
    },
}

impl PreToolDecision {
    /// A deny decision with the given reason.
    #[must_use]
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    /// An allow decision that replaces the call's arguments.
    #[must_use]
    pub fn modify(args: Value) -> Self {
        Self::Modify { args }
    }

    /// Whether the call may run (as it is, or modified).
    #[must_use]
    pub fn allows(&self) -> bool {
        !matches!(self, Self::Deny { .. })
    }
}

/// A synchronous pre-tool hook: inspect the call, return a verdict.
pub type PreToolHook = Arc<dyn Fn(&ToolInvocation) -> PreToolDecision + Send + Sync>;

/// A synchronous post-tool hook: observe completed tool calls.
pub type PostToolHook = Arc<dyn Fn(&ToolOutcome) + Send + Sync>;

// =============================================================================
// Lifecycle hooks
// =============================================================================

/// A turn about to start, handed to an
/// [`on_pre_turn`](crate::antigravity::AgentBuilder::on_pre_turn) hook.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TurnStart {
    /// The user's message (its text parts, joined).
    pub text: String,
    /// The trajectory the turn runs on.
    pub trajectory_id: Option<String>,
}

/// A pre-turn hook's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TurnDecision {
    /// Run the turn.
    Allow,
    /// Refuse it: the turn fails with
    /// [`AntigravityError::Turn`](crate::antigravity::AntigravityError::Turn)
    /// carrying the reason, and the agent stays usable.
    Deny {
        /// Why the turn was refused.
        reason: String,
    },
}

impl TurnDecision {
    /// A deny decision with the given reason.
    #[must_use]
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }
}

/// A finished turn, handed to an
/// [`on_post_turn`](crate::antigravity::AgentBuilder::on_post_turn) hook.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TurnEnd {
    /// The turn's final response text.
    pub response_text: String,
    /// The trajectory the turn ran on.
    pub trajectory_id: Option<String>,
}

/// A failed tool call, handed to an
/// [`on_tool_error`](crate::antigravity::AgentBuilder::on_tool_error) hook.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolFailure {
    /// The tool, named as policies name it (`view_file`,
    /// `mcp_<server>_<tool>`, a custom tool's name).
    pub name: String,
    /// The harness's error text.
    pub error: String,
    /// The harness's correlation id for the call.
    pub call_id: Option<String>,
    /// The trajectory that made the call.
    pub trajectory_id: Option<String>,
}

/// A history compaction, handed to an
/// [`on_compaction`](crate::antigravity::AgentBuilder::on_compaction) hook.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompactionEvent {
    /// The summary that replaced the compacted history.
    pub summary: String,
    /// The compacted trajectory.
    pub trajectory_id: Option<String>,
    /// The compaction step's index.
    pub step_index: Option<u32>,
}

/// The root trajectory about to go idle, handed to an
/// [`on_stop`](crate::antigravity::AgentBuilder::on_stop) hook.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StopRequest {
    /// The turn's response text so far.
    pub response_text: String,
    /// The stopping trajectory.
    pub trajectory_id: Option<String>,
    /// How many times a stop hook has already kept this turn going. Return
    /// [`StopDecision::Stop`] past a limit of your own, or the turn never
    /// ends.
    pub continuation_count: u32,
    /// Why the harness is stopping the turn early, when it is (a budget
    /// cap, quota exhaustion).
    pub stop_reason: Option<crate::antigravity::protocol::StopReason>,
    /// The error that is ending the turn, when one is.
    pub error: Option<String>,
}

/// A stop hook's verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopDecision {
    /// Let the turn end.
    Stop,
    /// Keep going: the harness adds `reason` as an instruction and calls
    /// the model again, in the same turn.
    Continue {
        /// The instruction for the next model call.
        reason: String,
    },
}

impl StopDecision {
    /// A continue decision with the given instruction.
    #[must_use]
    pub fn continue_with(reason: impl Into<String>) -> Self {
        Self::Continue {
            reason: reason.into(),
        }
    }
}

type SessionHook = Arc<dyn Fn() + Send + Sync>;
type PreTurnHook = Arc<dyn Fn(&TurnStart) -> TurnDecision + Send + Sync>;
type PostTurnHook = Arc<dyn Fn(&TurnEnd) + Send + Sync>;
type ToolErrorHook = Arc<dyn Fn(&ToolFailure) -> Option<String> + Send + Sync>;
type CompactionHook = Arc<dyn Fn(&CompactionEvent) + Send + Sync>;
type StopHook = Arc<dyn Fn(&StopRequest) -> StopDecision + Send + Sync>;

/// The lifecycle hooks beyond the tool and question hooks, as set on the
/// builder. Each runs on a thread of its own, like every hook.
#[derive(Clone, Default)]
pub(crate) struct LifecycleHooks {
    pub(crate) session_start: Option<SessionHook>,
    pub(crate) session_end: Option<SessionHook>,
    pub(crate) pre_turn: Option<PreTurnHook>,
    pub(crate) post_turn: Option<PostTurnHook>,
    pub(crate) tool_error: Option<ToolErrorHook>,
    pub(crate) compaction: Option<CompactionHook>,
    pub(crate) stop: Option<StopHook>,
}

impl LifecycleHooks {
    /// The harness lifecycle points these hooks need callbacks for.
    pub(crate) fn enabled(&self) -> Vec<crate::antigravity::protocol::LifecycleHook> {
        use crate::antigravity::protocol::LifecycleHook as Hook;
        [
            (self.session_start.is_some(), Hook::OnSessionStart),
            (self.session_end.is_some(), Hook::OnSessionEnd),
            (self.pre_turn.is_some(), Hook::PreTurn),
            (self.post_turn.is_some(), Hook::PostTurn),
            (self.tool_error.is_some(), Hook::OnToolError),
            (self.compaction.is_some(), Hook::OnCompaction),
            (self.stop.is_some(), Hook::Stop),
        ]
        .into_iter()
        .filter_map(|(set, hook)| set.then_some(hook))
        .collect()
    }
}

impl std::fmt::Debug for LifecycleHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleHooks")
            .field("enabled", &self.enabled())
            .finish()
    }
}

/// A question the agent asked via the `ask_question` builtin.
///
/// Delivered to the hook set with
/// [`on_questions`](crate::antigravity::AgentBuilder::on_questions).
/// Multiple-choice today — but a shape this crate doesn't model arrives
/// with [`is_unknown_type`](Self::is_unknown_type) set and its raw
/// payload in [`extra`](Self::extra); check it before rendering.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AgentQuestion {
    /// The question text.
    pub question: String,
    /// The predefined choices.
    pub choices: Vec<String>,
    /// Whether more than one choice may be selected.
    pub is_multi_select: bool,
    /// Unmodeled wire fields (Evergreen), preserved from the harness's
    /// question object. Non-empty whenever the wire carried fields this
    /// crate doesn't model — a modeled multiple-choice question can carry
    /// unmodeled siblings too, so use [`Self::is_unknown_type`] (not this
    /// map's emptiness) to tell whether the question's *type* itself is
    /// unmodeled. Merged from two levels — the question object and its
    /// `multiple_choice` arm — with the inner value winning should the
    /// protocol ever grow a colliding key.
    pub extra: serde_json::Map<String, serde_json::Value>,
    /// True when the wire question carried no `multiple_choice` arm.
    pub(crate) unknown_type: bool,
}

impl AgentQuestion {
    /// Creates a question — primarily so hook authors can build fixtures to
    /// unit-test their `on_questions` closures without spawning a harness
    /// (the struct is `#[non_exhaustive]`, so it has no literal syntax
    /// downstream).
    ///
    /// ```
    /// use genai_rs::antigravity::{AgentQuestion, QuestionAnswer, QuestionReply};
    ///
    /// // The closure under test — same shape as an `on_questions` hook.
    /// let hook = |questions: &[AgentQuestion]| {
    ///     QuestionReply::Answers(
    ///         questions
    ///             .iter()
    ///             .map(|q| {
    ///                 if q.is_unknown_type() || q.choices.is_empty() {
    ///                     QuestionAnswer::Unanswered
    ///                 } else {
    ///                     QuestionAnswer::Choices { selected: vec![0], freeform: None }
    ///                 }
    ///             })
    ///             .collect(),
    ///     )
    /// };
    ///
    /// let fixture = [AgentQuestion::new(
    ///     "Proceed with the migration?",
    ///     ["yes", "no"],
    ///     false,
    /// )];
    /// let QuestionReply::Answers(answers) = hook(&fixture) else {
    ///     panic!("expected answers");
    /// };
    /// assert_eq!(
    ///     answers,
    ///     vec![QuestionAnswer::Choices { selected: vec![0], freeform: None }]
    /// );
    /// ```
    #[must_use]
    pub fn new(
        question: impl Into<String>,
        choices: impl IntoIterator<Item = impl Into<String>>,
        is_multi_select: bool,
    ) -> Self {
        Self {
            question: question.into(),
            choices: choices.into_iter().map(Into::into).collect(),
            is_multi_select,
            extra: serde_json::Map::new(),
            unknown_type: false,
        }
    }

    /// Creates an unknown-type question fixture — empty text and choices,
    /// [`Self::is_unknown_type`] true, with `extra` carrying the raw
    /// payload — so hook authors can unit-test their cancel-on-unknown
    /// branch, the state [`Self::new`] deliberately cannot produce.
    ///
    /// ```
    /// use genai_rs::antigravity::AgentQuestion;
    ///
    /// let q = AgentQuestion::unknown(
    ///     [("freeText".to_string(), serde_json::json!({"maxLen": 80}))]
    ///         .into_iter()
    ///         .collect(),
    /// );
    /// assert!(q.is_unknown_type());
    /// assert!(q.choices.is_empty());
    /// ```
    #[must_use]
    pub fn unknown(extra: serde_json::Map<String, serde_json::Value>) -> Self {
        Self {
            question: String::new(),
            choices: Vec::new(),
            is_multi_select: false,
            extra,
            unknown_type: true,
        }
    }

    /// True when the harness sent a question shape this crate doesn't model
    /// (no `multiple_choice` arm on the wire). The raw payload is in
    /// [`Self::extra`]; a hook may prefer [`QuestionReply::Cancel`] over
    /// answering a question it can't render.
    #[must_use]
    pub const fn is_unknown_type(&self) -> bool {
        self.unknown_type
    }
}

/// Answer to a single [`AgentQuestion`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum QuestionAnswer {
    /// Select choices by zero-based index into
    /// [`AgentQuestion::choices`], optionally adding freeform text.
    ///
    /// Indices are relayed to the harness as-is: out-of-range indices and
    /// multiple selections on a single-select question are `warn!`ed but
    /// not filtered — the harness owns the final verdict.
    Choices {
        /// Zero-based indices of the selected choices. Converted to the
        /// harness's `i32` wire type at the protocol boundary (an index
        /// that cannot fit is warn'ed and dropped there).
        selected: Vec<usize>,
        /// Optional freeform text alongside the selection.
        freeform: Option<String>,
    },
    /// A freeform text answer with no choice selected.
    ///
    /// On the wire this is identical to a [`Choices`](Self::Choices)
    /// answer with an empty `selected` and the text in `freeform` — it is
    /// the ergonomic spelling of the no-selection case, not a distinct
    /// answer shape.
    Freeform(String),
    /// Leave this question unanswered.
    Unanswered,
}

/// Reply to a whole `ask_question` batch.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum QuestionReply {
    /// One answer per question, in order. A short reply is padded with
    /// [`QuestionAnswer::Unanswered`]; extra answers are dropped.
    Answers(Vec<QuestionAnswer>),
    /// Cancel the question interaction.
    ///
    /// On the wire this sets the reply's `cancelled` flag with no answers
    /// (pinned by test). What the harness then does with the in-flight
    /// turn — resolve with the trajectory as it stands, or end it another
    /// way — is harness-owned and not live-verified; treat this as "stop
    /// asking, without answers" rather than a documented turn outcome.
    Cancel,
}

/// Hook invoked when the agent asks the user questions
/// (`ask_question` builtin). Return answers or cancel; without a hook the
/// harness receives "unanswered" for every question.
pub type QuestionHook = Arc<dyn Fn(&[AgentQuestion]) -> QuestionReply + Send + Sync>;
