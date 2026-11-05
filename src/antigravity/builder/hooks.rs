//! Hook setters on [`AgentBuilder`].

use std::sync::Arc;

use super::AgentBuilder;
use crate::antigravity::{
    AgentQuestion, PreToolDecision, QuestionReply, ToolInvocation, ToolOutcome,
};

impl AgentBuilder {
    /// Sets a pre-tool hook, consulted before every tool dispatch and for
    /// `confirm(...)` policies.
    ///
    /// Like every hook, it runs on a thread of its own and is bounded by the
    /// turn deadline. A hook still running at the deadline is abandoned,
    /// not stopped: its thread runs on until the hook returns, but holds up
    /// neither runtime shutdown nor process exit. A call whose arguments the
    /// harness sent as invalid JSON is denied without consulting the hook,
    /// which would otherwise judge a placeholder.
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
    /// On harness 0.1.18 the `ask_question` call passes through the
    /// pre-tool hook *before* its `questions_request` arrives, so policies
    /// govern it like any other tool: `deny_all()` needs an
    /// `allow("ask_question")` beside it, and `deny("ask_question")`
    /// prevents questions. (On 0.1.10 question requests bypassed the policy
    /// engine.)
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
}
