//! Answers to the lifecycle hooks beyond the tool hooks: session start and
//! end, pre- and post-turn, tool errors, compaction and stop. Each runs the
//! user's hook through [`run_hook`], so the turn deadline bounds it, and
//! maps the result onto the hook's wire answer.

use super::hooks::{
    CompactionEvent, StopDecision, StopRequest, ToolFailure, TurnDecision, TurnEnd, TurnStart,
};
use super::protocol::{self, HookDecision, HookVerdict, LifecycleHook};
use super::turn::TurnState;
use super::turn_requests::run_hook;
use super::{AntigravityAgent, AntigravityError};

impl AntigravityAgent {
    /// Fills `response` for a hook request that is not a tool hook. A hook
    /// this client did not set (or a kind it does not know) gets an empty
    /// result, so the harness never waits.
    pub(super) async fn answer_lifecycle_hook(
        &mut self,
        request: &protocol::CallHookRequest,
        response: &mut protocol::CallHookResponse,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let hooks = self.lifecycle.clone();
        response.empty_result = Some(protocol::EmptyResult {});

        if let Some(args) = &request.pre_turn_args {
            let decision = match hooks.pre_turn {
                Some(hook) => {
                    let start = turn_start(args);
                    turn.running = Some("the pre-turn hook".to_string());
                    let decision = run_hook(move || hook(&start)).await?;
                    turn.running = None;
                    decision
                }
                None => TurnDecision::Allow,
            };
            response.empty_result = None;
            response.pre_turn_result = Some(pre_turn_verdict(decision));
        } else if let Some(args) = &request.post_turn_args {
            if let Some(hook) = hooks.post_turn {
                let end = TurnEnd {
                    response_text: args.response_text.clone().unwrap_or_default(),
                    trajectory_id: args.trajectory_id.clone(),
                };
                turn.running = Some("the post-turn hook".to_string());
                run_hook(move || hook(&end)).await?;
                turn.running = None;
            }
        } else if let Some(args) = &request.on_tool_error_args {
            if let Some(hook) = hooks.tool_error {
                let failure = tool_failure(args);
                turn.running = Some(format!("the tool-error hook for '{}'", failure.name));
                let replacement = run_hook(move || hook(&failure)).await?;
                turn.running = None;
                if let Some(message) = replacement.filter(|m| !m.trim().is_empty()) {
                    response.empty_result = None;
                    response.on_tool_error_result = Some(protocol::OnToolErrorResult {
                        custom_error_message: Some(message.trim().to_string()),
                    });
                }
            }
        } else if let Some(args) = &request.on_compaction_args {
            if let Some(hook) = hooks.compaction {
                let event = CompactionEvent {
                    summary: args.summary.clone().unwrap_or_default(),
                    trajectory_id: args.trajectory_id.clone(),
                    step_index: args.step_index,
                };
                turn.running = Some("the compaction hook".to_string());
                run_hook(move || hook(&event)).await?;
                turn.running = None;
            }
        } else if let Some(args) = &request.stop_args {
            let decision = match hooks.stop {
                Some(hook) => {
                    let stop = stop_request(args);
                    turn.running = Some("the stop hook".to_string());
                    let decision = run_hook(move || hook(&stop)).await?;
                    turn.running = None;
                    decision
                }
                None => StopDecision::Stop,
            };
            response.empty_result = None;
            response.stop_result = Some(stop_result(decision));
        } else {
            let hook = match request.hook_type {
                Some(LifecycleHook::OnSessionStart) => hooks.session_start,
                Some(LifecycleHook::OnSessionEnd) => hooks.session_end,
                _ => None,
            };
            if let Some(hook) = hook {
                turn.running = Some("a session hook".to_string());
                run_hook(move || hook()).await?;
                turn.running = None;
            } else if !matches!(
                request.hook_type,
                Some(LifecycleHook::OnSessionStart | LifecycleHook::OnSessionEnd)
            ) {
                tracing::warn!(
                    "Answering an unrecognized hook request ({:?}) with an empty result.",
                    request.hook_type
                );
            }
        }
        Ok(())
    }
}

/// The hook-facing view of a pre-turn request: the text parts, joined.
pub(super) fn turn_start(args: &protocol::PreTurnArgs) -> TurnStart {
    let text = args
        .user_input
        .iter()
        .flat_map(|input| input.parts.iter())
        .filter_map(|part| part.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    TurnStart {
        text,
        trajectory_id: args.trajectory_id.clone(),
    }
}

pub(super) fn pre_turn_verdict(decision: TurnDecision) -> HookVerdict {
    match decision {
        TurnDecision::Allow => HookVerdict {
            decision: Some(HookDecision::Allow),
            ..Default::default()
        },
        TurnDecision::Deny { reason } => HookVerdict {
            decision: Some(HookDecision::Deny),
            reason: Some(reason),
            ..Default::default()
        },
    }
}

pub(super) fn tool_failure(args: &protocol::OnToolErrorArgs) -> ToolFailure {
    ToolFailure {
        name: protocol::hook_tool_name(
            args.tool_name.as_deref().unwrap_or_default(),
            args.server_name.as_deref(),
        ),
        error: args.error_message.clone().unwrap_or_default(),
        call_id: args.call_id.clone().filter(|id| !id.is_empty()),
        trajectory_id: args.trajectory_id.clone(),
    }
}

pub(super) fn stop_request(args: &protocol::StopArgs) -> StopRequest {
    StopRequest {
        response_text: args.response_text.clone().unwrap_or_default(),
        trajectory_id: args.trajectory_id.clone(),
        continuation_count: args
            .continuation_count
            .and_then(|count| u32::try_from(count).ok())
            .unwrap_or_default(),
        // UNSPECIFIED is the wire's "no early stop".
        stop_reason: args
            .stop_reason
            .clone()
            .filter(|reason| *reason != protocol::StopReason::Unspecified),
        error: args.error_message.clone().filter(|e| !e.trim().is_empty()),
    }
}

pub(super) fn stop_result(decision: StopDecision) -> protocol::StopResult {
    match decision {
        StopDecision::Stop => protocol::StopResult {
            decision: Some(protocol::StopDecision::AllowStop),
            reason: None,
        },
        StopDecision::Continue { reason } => protocol::StopResult {
            decision: Some(protocol::StopDecision::Continue),
            reason: Some(reason.trim().to_string()),
        },
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
