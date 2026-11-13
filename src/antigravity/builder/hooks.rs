//! Hook setters on [`AgentBuilder`].
//!
//! Every hook is a synchronous closure that runs on a thread of its own,
//! bounded by the turn deadline (see [`AgentBuilder::on_pre_tool`]). A hook
//! that has async work to do can block on it there:
//! `tokio::runtime::Handle::current().block_on(..)` works inside any hook.

use std::sync::Arc;

use super::AgentBuilder;
use crate::antigravity::{
    AgentQuestion, CompactionEvent, PreToolDecision, QuestionReply, StopDecision, StopRequest,
    ToolFailure, ToolInvocation, ToolOutcome, TurnDecision, TurnEnd, TurnStart,
};

impl AgentBuilder {
    /// Sets a pre-tool hook, consulted before every tool call the agent
    /// itself makes (builtin, custom, MCP, `invoke_subagent`) and for
    /// `confirm(...)` policies. It may allow, deny, or
    /// [modify](PreToolDecision::modify) the call's arguments.
    ///
    /// A subagent's own tool calls never reach it (harness 0.1.23), so
    /// `spawn()` refuses an agent with this hook and subagents unless
    /// [`with_ungated_subagents`](Self::with_ungated_subagents) opts in.
    ///
    /// Like every hook, it runs on a thread of its own and is bounded by the
    /// turn deadline. A hook still running at the deadline is abandoned,
    /// not stopped: its thread runs on until the hook returns, but holds up
    /// neither runtime shutdown nor process exit. A call whose arguments the
    /// harness sent as invalid JSON is denied without consulting the hook,
    /// which would otherwise judge a placeholder.
    ///
    /// The hook is a plain closure; async work inside it can block on the
    /// runtime, which the hook's thread has entered:
    ///
    /// ```rust,no_run
    /// # use genai_rs::antigravity::{AntigravityAgent, PreToolDecision};
    /// # struct Audit;
    /// # impl Audit { async fn check(&self, _tool: &str) -> bool { true } }
    /// # let audit_service = std::sync::Arc::new(Audit);
    /// # let _builder = AntigravityAgent::builder()
    /// .on_pre_tool(move |call| {
    ///     let allowed = tokio::runtime::Handle::current()
    ///         .block_on(audit_service.check(&call.name));
    ///     if allowed { PreToolDecision::Allow } else { PreToolDecision::deny("audit") }
    /// })
    /// # ;
    /// ```
    #[must_use]
    pub fn on_pre_tool(
        mut self,
        hook: impl Fn(&ToolInvocation) -> PreToolDecision + Send + Sync + 'static,
    ) -> Self {
        self.pre_tool = Some(Arc::new(hook));
        self
    }

    /// Sets a questions hook, invoked when the agent asks the user
    /// questions via the `ask_question` builtin.
    ///
    /// The hook receives the whole question batch and returns answers (or
    /// cancels). Without a hook, every question is answered "unanswered"
    /// so the harness never deadlocks — that fallback covers only the
    /// *hookless* case.
    ///
    /// The hook is synchronous and the turn waits for it: **do not block in
    /// it** waiting for a human. Answer from policy or pre-collected state
    /// (e.g. a channel drained with `try_recv`). It runs on a thread of its
    /// own, so the turn deadline still bounds it (see
    /// [`on_pre_tool`](Self::on_pre_tool)), but every second it takes is
    /// spent from that budget.
    ///
    /// The hook only fires if [`BuiltinTool::AskQuestion`] is enabled —
    /// it is *not* in [`Capabilities::read_only()`] (the default), so on a
    /// default builder this hook is a no-op: the agent never asks, and
    /// `spawn()` emits a `warn!` for the hook-set-but-builtin-disabled
    /// combination. Enable it with
    /// `Capabilities::read_only().enable(BuiltinTool::AskQuestion)`, which
    /// as a write-capable capability also needs a policy or
    /// [`on_pre_tool`](Self::on_pre_tool) hook to pass the spawn-time
    /// gate — and set
    /// [`with_agent_behavior(AgentBehavior::Interactive)`](Self::with_agent_behavior),
    /// without which the model is told nobody will answer.
    ///
    /// On the pinned harness 0.1.23, as on 0.1.18, the `ask_question` call
    /// passes through the pre-tool hook *before* its `questions_request`
    /// arrives, so policies govern it like any other tool: `deny_all()`
    /// needs an `allow("ask_question")` beside it, and
    /// `deny("ask_question")` prevents questions. (On 0.1.10 question
    /// requests bypassed the policy engine.)
    ///
    /// [`BuiltinTool::AskQuestion`]: crate::antigravity::BuiltinTool::AskQuestion
    /// [`Capabilities::read_only()`]: crate::antigravity::Capabilities::read_only
    #[must_use]
    pub fn on_questions(
        mut self,
        hook: impl Fn(&[AgentQuestion]) -> QuestionReply + Send + Sync + 'static,
    ) -> Self {
        self.questions = Some(Arc::new(hook));
        self
    }

    /// Sets a post-tool hook, observing completed custom tool calls.
    ///
    /// It runs on a thread of its own and is bounded by the turn deadline,
    /// as [`on_pre_tool`](Self::on_pre_tool) describes.
    #[must_use]
    pub fn on_post_tool(mut self, hook: impl Fn(&ToolOutcome) + Send + Sync + 'static) -> Self {
        self.post_tool = Some(Arc::new(hook));
        self
    }

    /// Runs `hook` once the session starts, before `spawn()` returns.
    ///
    /// The wait for the hook, and its run, count against `spawn()`'s 30 s
    /// session-start budget: a hook that overruns it fails
    /// [`spawn()`](Self::spawn) with
    /// [`AntigravityError::InitFailed`](crate::antigravity::AntigravityError::InitFailed).
    #[must_use]
    pub fn on_session_start(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.lifecycle.session_start = Some(Arc::new(hook));
        self
    }

    /// Runs `hook` when the session ends gracefully, during
    /// [`shutdown`](crate::antigravity::AntigravityAgent::shutdown) (not on
    /// drop).
    #[must_use]
    pub fn on_session_end(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.lifecycle.session_end = Some(Arc::new(hook));
        self
    }

    /// Consulted before each turn starts, with the user's message. A
    /// [`TurnDecision::Deny`] refuses the turn: `chat` fails with
    /// [`AntigravityError::Turn`](crate::antigravity::AntigravityError::Turn)
    /// of kind [`TurnFailure::Denied`](crate::antigravity::TurnFailure::Denied),
    /// the reason as its message, and the agent stays usable.
    ///
    /// Not consulted on trigger turns (harness 0.1.23): a turn a
    /// [trigger](Self::add_trigger) starts runs without asking it, though
    /// its stop and post-turn hooks run. A gate meant for those turns
    /// belongs in the trigger, which decides whether to send; `spawn()`
    /// logs a warning when this hook and a trigger are both set.
    #[must_use]
    pub fn on_pre_turn(
        mut self,
        hook: impl Fn(&TurnStart) -> TurnDecision + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle.pre_turn = Some(Arc::new(hook));
        self
    }

    /// Runs `hook` after each turn with its final response text.
    #[must_use]
    pub fn on_post_turn(mut self, hook: impl Fn(&TurnEnd) + Send + Sync + 'static) -> Self {
        self.lifecycle.post_turn = Some(Arc::new(hook));
        self
    }

    /// Consulted when a tool call fails (a builtin, MCP or custom tool).
    /// Returning `Some(text)` replaces the error the model sees with
    /// `text`, to steer it toward a fix; `None` keeps the harness's.
    #[must_use]
    pub fn on_tool_error(
        mut self,
        hook: impl Fn(&ToolFailure) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle.tool_error = Some(Arc::new(hook));
        self
    }

    /// Runs `hook` when the harness compacts the history (see
    /// [`with_compaction_threshold`](Self::with_compaction_threshold)),
    /// with the summary that replaced it.
    #[must_use]
    pub fn on_compaction(
        mut self,
        hook: impl Fn(&CompactionEvent) + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle.compaction = Some(Arc::new(hook));
        self
    }

    /// Consulted when a turn is about to end. [`StopDecision::Continue`]
    /// keeps it going: the harness adds the reason as an instruction and
    /// calls the model again, within the same turn (and the same turn
    /// deadline). Check [`StopRequest::continuation_count`] and stop past a
    /// limit of your own, or the turn runs until its deadline.
    #[must_use]
    pub fn on_stop(
        mut self,
        hook: impl Fn(&StopRequest) -> StopDecision + Send + Sync + 'static,
    ) -> Self {
        self.lifecycle.stop = Some(Arc::new(hook));
        self
    }
}
