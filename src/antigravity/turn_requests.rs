//! The harness's requests within a turn, and this client's answers: custom
//! tool calls, the tool hooks (pre-tool and post-tool), the auto policy's
//! decision requests, tool confirmations and user questions. The other
//! lifecycle hooks are in `lifecycle.rs`. Every user hook runs through
//! [`run_hook`], so the turn deadline bounds it.

use super::hook_mapping::{
    confirmation_decision, map_question_reply, map_questions, normalize_tool_error, pre_tool_gate,
    tool_call_arguments, unwrap_result_string, unwrap_result_value,
};
use super::policies::{self, PolicyDecision};
use super::protocol::{
    self, HookDecision, HookVerdict, InputEvent, PolicyEvaluationOutcome, StepUpdate,
};
use super::turn::{HookDenial, TurnState};
use super::{
    AgentEvent, AntigravityAgent, AntigravityError, PreToolDecision, ToolDecision, ToolInvocation,
    ToolOutcome, TurnFailure, streaming,
};
use serde_json::Value;
use std::sync::Arc;

impl AntigravityAgent {
    /// Policy-checks a pending harness-side tool and replies with a
    /// `tool_confirmation`. When the decision is a denial, the blocked
    /// action is surfaced as a [`AgentEvent::ToolAction`] with a
    /// [`ToolDecision::Denied`] marker (deduped against the terminal-step
    /// emission via `announced_actions`) so consumers see denied actions
    /// distinctly — a denied action still reaches a terminal step, but with
    /// no signal that it was blocked.
    pub(super) async fn answer_tool_confirmation(
        &mut self,
        step: &StepUpdate,
        step_key: &(String, u32),
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let decision = match self.pre_tool.clone() {
            None => confirmation_decision(step, &self.policy_engine, None),
            Some(hook) => {
                let (engine, owned) = (Arc::clone(&self.policy_engine), step.clone());
                turn.running = Some("the pre-tool hook for a tool confirmation".to_string());
                let decision =
                    run_hook(move || confirmation_decision(&owned, &engine, Some(&hook))).await?;
                turn.running = None;
                decision
            }
        };
        // `confirmation_decision` never returns `Modify`: a confirmation
        // cannot carry arguments, so it turns one into a denial.
        let accepted = decision.allows();
        if let PreToolDecision::Deny { reason } = &decision
            && let Some(action) = streaming::ToolAction::from_step(step)
        {
            turn.announce_tool_action(
                step_key,
                action,
                ToolDecision::Denied {
                    reason: reason.clone(),
                },
                step.trajectory_id.clone(),
            );
        }
        self.session
            .send(&InputEvent::ToolConfirmation(protocol::ToolConfirmation {
                trajectory_id: step.trajectory_id.clone().unwrap_or_default(),
                step_index: step.step_index.unwrap_or_default(),
                accepted,
            }))
            .await
    }

    /// Replies to a `questions_request`.
    ///
    /// With an [`AgentBuilder::on_questions`] hook set, the batch is handed
    /// to the hook and its answers (or cancellation) are relayed. Without
    /// one, every question is answered "unanswered" so the harness never
    /// deadlocks (the protocol requires a response).
    ///
    /// [`AgentBuilder::on_questions`]: super::AgentBuilder::on_questions
    pub(super) async fn answer_questions(
        &mut self,
        step: &StepUpdate,
        request: &protocol::UserQuestionsRequest,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let batch = map_questions(request);
        let reply = match self.questions.clone() {
            Some(hook) => {
                let owned = batch.clone();
                turn.running = Some("the questions hook".to_string());
                let reply = run_hook(move || hook(&owned)).await?;
                turn.running = None;
                Some(reply)
            }
            None => {
                tracing::warn!(
                    "Harness asked {} user question(s) but no questions hook \
                     is set; answering as unanswered. Set `.on_questions(..)` on the \
                     builder to answer interactively, or disable the ask_question \
                     builtin (Capabilities) to prevent this.",
                    request.questions.len()
                );
                None
            }
        };
        let mut response = map_question_reply(reply, &batch);
        response.trajectory_id = step.trajectory_id.clone().unwrap_or_default();
        response.step_index = step.step_index.unwrap_or_default();
        self.session
            .send(&InputEvent::QuestionResponse(response))
            .await
    }

    /// Dispatches a custom (client-executed) tool call through the crate's
    /// function registry / tool services, then sends a `tool_response` back
    /// to the harness.
    ///
    /// Policies and the pre-tool hook already judged the call: the harness
    /// asks the pre-tool callback before it dispatches a custom tool
    /// (verified on 0.1.18 and 0.1.23), and sends its arguments as the hook
    /// left them. Judging it again here called the hook twice per call.
    pub(super) async fn process_tool_call(
        &mut self,
        call: protocol::ToolCall,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let id = call.id.clone().unwrap_or_default();
        let name = call.name.clone().unwrap_or_default();
        // Fail closed: a tool run on a placeholder (or a hook judging one)
        // is worse than a call the model sees fail and can retry.
        let args = match tool_call_arguments(&call) {
            Ok(args) => args,
            Err(error) => {
                tracing::warn!("Not running custom tool '{name}': {error}");
                return self
                    .session
                    .send(&InputEvent::ToolResponse(protocol::ToolResponse {
                        id,
                        response_json: Some(
                            serde_json::json!({
                                "error": format!(
                                    "Tool execution denied: the arguments could not be parsed \
                                     ({error})"
                                )
                            })
                            .to_string(),
                        ),
                        ..Default::default()
                    }))
                    .await;
            }
        };

        turn.running = Some(format!("custom tool '{name}'"));
        let result = self.dispatcher.execute(&name, args).await;
        turn.running = None;
        if let Some(post_tool) = self.post_tool.clone() {
            // Hand the hook the inner value, not the `{"result": ...}`
            // wire envelope the harness expects (Item: unwrap
            // ToolOutcome.result). The error branch keeps the
            // envelope's error string.
            let error = normalize_tool_error(
                result
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
            let outcome = ToolOutcome {
                name: name.clone(),
                result: error.is_none().then(|| unwrap_result_value(&result)),
                error,
            };
            turn.running = Some(format!("the post-tool hook for '{name}'"));
            run_hook(move || post_tool(&outcome)).await?;
            turn.running = None;
            turn.reported_calls.insert(id.clone());
        }
        turn.queue.push_back(AgentEvent::ToolCallDispatched {
            name: name.clone(),
            id: id.clone(),
        });
        self.session
            .send(&InputEvent::ToolResponse(protocol::ToolResponse {
                id,
                response_json: Some(result.to_string()),
                ..Default::default()
            }))
            .await
    }

    /// Answers a lifecycle hook callback. The protocol requires a response
    /// for every request, so unknown hook kinds get an `empty_result`.
    pub(super) async fn process_hook_request(
        &mut self,
        request: protocol::CallHookRequest,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let request_id = request.request_id.clone().unwrap_or_default();
        let mut response = protocol::CallHookResponse {
            request_id,
            ..Default::default()
        };
        if let Some(pre_tool_args) = &request.pre_tool_args {
            let (invocation, decision) = match pre_tool_gate(pre_tool_args, self.pre_tool.is_some())
            {
                Ok(invocation) => {
                    let decision = self.decide(invocation.clone(), turn).await?;
                    (invocation, decision)
                }
                Err(denial) => (protocol_invocation(pre_tool_args), denial),
            };
            response.pre_tool_result = Some(match decision {
                PreToolDecision::Allow => HookVerdict {
                    decision: Some(HookDecision::Allow),
                    ..Default::default()
                },
                PreToolDecision::Modify { args } => HookVerdict {
                    decision: Some(HookDecision::Allow),
                    modified_args: Some(protocol::encode_genai_struct(&args)),
                    ..Default::default()
                },
                PreToolDecision::Deny { reason } => {
                    turn.hook_denials.push(HookDenial {
                        trajectory_id: pre_tool_args.trajectory_id.clone(),
                        reason: reason.clone(),
                        name: invocation.name,
                        args: invocation.args,
                    });
                    HookVerdict {
                        decision: Some(HookDecision::Deny),
                        reason: Some(reason),
                        ..Default::default()
                    }
                }
            });
        } else if let Some(post_tool_args) = &request.post_tool_args {
            // Harness 0.1.18 also fires its post-tool callback for custom
            // tools, which the dispatch path has already reported — once
            // per call is the contract.
            let already_reported = turn.take_reported_call(post_tool_args.call_id.as_deref());
            if let Some(post_tool) = self.post_tool.clone()
                && !already_reported
            {
                let outcome = ToolOutcome {
                    name: protocol::hook_tool_name(
                        post_tool_args.tool_name.as_deref().unwrap_or_default(),
                        post_tool_args.server_name.as_deref(),
                    ),
                    // Unwrap the `{"result": ...}` envelope for consistency
                    // with the custom-tool dispatch path.
                    result: post_tool_args.result.as_deref().map(unwrap_result_string),
                    error: normalize_tool_error(post_tool_args.error.clone()),
                };
                turn.running = Some(format!("the post-tool hook for '{}'", outcome.name));
                run_hook(move || post_tool(&outcome)).await?;
                turn.running = None;
            }
            response.empty_result = Some(protocol::EmptyResult {});
        } else {
            self.answer_lifecycle_hook(&request, &mut response, turn)
                .await?;
        }
        self.session
            .send(&InputEvent::CallHookResponse(Box::new(response)))
            .await
    }

    /// Answers a `policyDecisionRequest`: the auto policy flagged a call
    /// (rule id `"auto"`), and its handler, or a denial, decides. This
    /// client sends no dynamic rules of its own, so any other rule id is
    /// denied (fail closed).
    pub(super) async fn process_policy_decision(
        &mut self,
        request: protocol::PolicyDecisionRequest,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let args = request.tool_args.clone().unwrap_or_default();
        let invocation = super::hook_mapping::pre_tool_invocation(&args);
        let rule = request.rule_id.as_deref().unwrap_or_default();
        let decision = if rule == "auto" && self.policy_engine.auto().is_some() {
            let engine = Arc::clone(&self.policy_engine);
            let (owned, reason) = (invocation.clone(), request.reason.clone());
            turn.running = Some(format!("the auto policy handler for '{}'", invocation.name));
            let decision =
                run_hook(move || policies::decide_flagged(&engine, &owned, reason.as_deref()))
                    .await?;
            turn.running = None;
            decision
        } else {
            tracing::warn!(
                "Harness asked about policy rule '{rule}', which this client did not send; \
                 denying '{}'.",
                invocation.name
            );
            PreToolDecision::deny(format!(
                "Tool '{}' denied: unknown policy rule '{rule}' (failing closed).",
                invocation.name
            ))
        };
        let (outcome, deny_reason) = match decision {
            PreToolDecision::Deny { reason } => {
                turn.hook_denials.push(HookDenial {
                    trajectory_id: args.trajectory_id.clone(),
                    reason: reason.clone(),
                    name: invocation.name,
                    args: invocation.args,
                });
                (PolicyEvaluationOutcome::Deny, Some(reason))
            }
            PreToolDecision::Allow | PreToolDecision::Modify { .. } => {
                (PolicyEvaluationOutcome::Allow, None)
            }
        };
        self.session
            .send(&InputEvent::PolicyDecisionResponse(
                protocol::PolicyDecisionResponse {
                    request_id: request.request_id.unwrap_or_default(),
                    outcome: Some(outcome),
                    deny_reason,
                },
            ))
            .await
    }

    /// The policy-then-hook verdict for a tool call. User code (the
    /// pre-tool hook, a policy's condition or handler) runs through
    /// [`run_hook`] so the turn deadline bounds it; a verdict that needs
    /// none — no callbacks, or a plain deny — stays on this thread.
    async fn decide(
        &self,
        invocation: ToolInvocation,
        turn: &mut TurnState,
    ) -> Result<PreToolDecision, AntigravityError> {
        let plain_deny = !self.policy_engine.has_callbacks()
            && self.policy_engine.evaluate(&invocation) == Some(PolicyDecision::Deny);
        if plain_deny || (self.pre_tool.is_none() && !self.policy_engine.has_callbacks()) {
            return Ok(policies::decide(
                &self.policy_engine,
                self.pre_tool.as_ref(),
                &invocation,
            ));
        }
        let engine = Arc::clone(&self.policy_engine);
        let hook = self.pre_tool.clone();
        turn.running = Some(format!("the pre-tool hook for '{}'", invocation.name));
        let decision =
            run_hook(move || policies::decide(&engine, hook.as_ref(), &invocation)).await?;
        turn.running = None;
        Ok(decision)
    }
}

/// The invocation a denial names when the arguments did not parse.
fn protocol_invocation(args: &protocol::PreToolArgs) -> ToolInvocation {
    super::hook_mapping::pre_tool_invocation(args)
}

/// Runs a user hook on a thread of its own.
///
/// Hooks are synchronous closures. Called inline, one that blocks holds the
/// runtime thread, and the turn deadline (a timer polled by that same task)
/// can never fire. On its own thread, the turn waits on a channel the
/// deadline can abandon. The hook's thread is not stopped: it runs to
/// completion and its result is discarded.
///
/// The thread is detached rather than taken from tokio's blocking pool,
/// because dropping a runtime waits for the blocking tasks still running on
/// it. A hook abandoned at the deadline and still blocked would then hang
/// the end of `#[tokio::main]`; a detached thread holds up neither runtime
/// shutdown nor process exit. The thread enters the caller's runtime, so a
/// hook can still use `Handle::current()` as it could on the blocking pool.
///
/// A panicking hook re-raises its panic here, exactly as an inline call
/// would.
pub(super) async fn run_hook<T, F>(hook: F) -> Result<T, AntigravityError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let runtime = tokio::runtime::Handle::current();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("antigravity-hook".to_string())
        .spawn(move || {
            let _runtime = runtime.enter();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(hook));
            // Fails only when the turn stopped waiting (its deadline fired),
            // and then nobody wants the result.
            let _ = sender.send(outcome);
        })
        .map_err(|e| {
            AntigravityError::turn(
                TurnFailure::Hook,
                format!("could not start a hook's thread: {e}"),
            )
        })?;
    match receiver.await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => Err(AntigravityError::turn(
            TurnFailure::Hook,
            "a hook's thread ended without a result",
        )),
    }
}

#[cfg(test)]
#[path = "turn_requests_tests.rs"]
mod tests;
