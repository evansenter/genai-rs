use super::hooks::{self, PolicyEngine};
use super::protocol::{self, StepUpdate};
use super::{
    AgentQuestion, PreToolDecision, PreToolHook, QuestionReply, ToolInvocation, streaming,
};
use serde_json::Value;

// =============================================================================
// Pre-tool hook mapping
// =============================================================================

/// Builds the policy-facing [`ToolInvocation`] for a harness pre-tool hook
/// callback. Pure, for unit testing.
///
/// Two normalizations, both observed on the 0.1.18 wire, so that one
/// policy rule matches the same call on every path it can arrive by:
///
/// - **Name**: the harness sends a builtin's `StepUpdate` field name
///   (`invoke_subagent`) and an MCP tool's bare name beside its
///   `server_name`; policies target `start_subagent` and
///   `mcp_<server>_<tool>` (see [`protocol::PreToolArgs`]).
/// - **MCP arguments**: the harness wraps them as
///   `{"Arguments": {..}, "ServerName": .., "ToolName": ..}`. The inner
///   object is what the model passed, and what [`ToolAction::args`]
///   reports for the same call, so a predicate like `args["repo"]` sees
///   one shape.
///
/// [`ToolAction::args`]: super::ToolAction::args
pub(super) fn pre_tool_invocation(args: &protocol::PreToolArgs) -> ToolInvocation {
    let server = args.server_name.as_deref().filter(|s| !s.is_empty());
    let name = protocol::hook_tool_name(args.tool_name.as_deref().unwrap_or_default(), server);
    let mut value = args
        .arguments_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| Value::Object(Default::default()));
    // Keyed on the wrapper's own `ToolName` sibling as well as the server,
    // so an unwrapped payload whose tool happens to take an `Arguments`
    // parameter is left alone.
    if server.is_some()
        && let Value::Object(map) = &value
        && map.contains_key("ToolName")
        && let Some(inner @ Value::Object(_)) = map.get("Arguments")
    {
        value = inner.clone();
    }
    ToolInvocation {
        name,
        args: value,
        id: args.call_id.clone().filter(|id| !id.is_empty()),
    }
}

// =============================================================================
// Tool-confirmation decision
// =============================================================================

/// Inserts the step's free-text confirmation prompt into the args map, when
/// both are present.
fn insert_request_text(step: &StepUpdate, args: &mut Value) {
    if let Some(request_text) = &step.request_text
        && let Value::Object(map) = args
    {
        map.insert(
            "request_text".to_string(),
            Value::String(request_text.clone()),
        );
    }
}

/// Decides whether a pending harness-side `tool_confirmation_request` is
/// accepted. Pure decision logic, separated from the wire reply for unit
/// testing.
///
/// Three cases (the request itself is an empty marker on the wire — the
/// step's action fields are the only discriminator, verified against the
/// harness 0.1.5 and 0.1.18 protos):
///
/// 1. **Recognized action** — the normal policy/hook decision.
/// 2. **No action and no unrecognized step fields** — a pre-request
///    notification for a host-side (client-executed) tool; a step carrying
///    only a `customTool` record is the same case. Auto-approved,
///    mirroring the reference SDK: the concrete call follows as a
///    `tool_call` with its own policy check, so nothing is bypassed.
///
/// On harness 0.1.18 no observed flow — builtins, custom tools, MCP,
/// subagents — waits on a confirmation at all: every call is gated by the
/// pre-tool hook instead (see `pre_tool_invocation`). This path is kept
/// because the protocol still defines it, and the reference SDK still
/// answers it.
/// 3. **No recognized action but unrecognized step fields** — most likely a
///    harness builtin newer than this client, whose confirmation is its
///    *only* gate. Fails closed unless a policy rule (wildcard `allow_all`
///    or an exact rule naming the unknown wire field) or the pre-tool hook
///    allows it; a `warn!` records the unknown fields either way
///    (Evergreen: reply and continue — never deadlock the harness).
pub(super) fn confirmation_decision(
    step: &StepUpdate,
    engine: &PolicyEngine,
    pre_tool: Option<&PreToolHook>,
) -> PreToolDecision {
    if let Some(action) = streaming::ToolAction::from_step(step) {
        let name = action.tool_name();
        let mut args = action.args();
        insert_request_text(step, &mut args);
        let invocation = ToolInvocation {
            name: name.clone(),
            args,
            id: None,
        };
        let decision = hooks::decide(engine, pre_tool, &invocation);
        if let PreToolDecision::Deny { reason } = &decision {
            tracing::info!("Rejecting harness tool '{name}': {reason}");
        }
        return decision;
    }

    if step.extra.is_empty() {
        // Case 2: genuine pre-request notification.
        tracing::debug!("Auto-approving a pre-request host-tool confirmation.");
        return PreToolDecision::Allow;
    }

    // Case 3: unrecognized fields — evaluate policies against the first
    // unknown wire field name (a new action field is the expected shape).
    let mut keys: Vec<&str> = step.extra.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let name = keys.first().copied().unwrap_or_default().to_string();
    let mut args = Value::Object(step.extra.clone());
    insert_request_text(step, &mut args);
    let invocation = ToolInvocation {
        name: name.clone(),
        args,
        id: None,
    };
    let decision = hooks::decide_with_default(
        engine,
        pre_tool,
        &invocation,
        PreToolDecision::deny(
            "unrecognized tool confirmation matched no policy rule (failing closed)",
        ),
    );
    let accepted = matches!(decision, PreToolDecision::Allow);
    tracing::warn!(
        "Unrecognized tool confirmation (unknown step fields {keys:?}); {} '{name}'. \
         Add a policy rule for the wire field name to control this explicitly.",
        if accepted { "allowing" } else { "denying" }
    );
    decision
}

/// Unwraps the dispatcher's result envelope for a post-tool hook. A scalar
/// tool return is wrapped as `{"result": X}` before going to the harness;
/// hooks want the inner `X` (its string form, or the value serialized when
/// non-string). A verbatim object return (any shape other than a lone
/// `result` key) is passed through serialized.
///
/// Edge case: a custom tool that legitimately returns a single-key object
/// `{"result": X}` is indistinguishable from a wrapped scalar and will be
/// unwrapped to `X` — the same inherent ambiguity as the dispatcher's
/// scalar-wrapping. Return a multi-key object to preserve the outer shape.
pub(super) fn unwrap_result_value(value: &Value) -> String {
    if let Value::Object(map) = value
        && map.len() == 1
        && let Some(inner) = map.get("result")
    {
        return match inner {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    value.to_string()
}

/// Normalizes a tool error into "errored or not".
///
/// The harness populates `PostToolArgs.error` with `""` — protobuf's
/// default for an unset string — on calls that **succeeded**. Passing that
/// through verbatim makes `ToolOutcome::error.is_some()` true for every
/// successful harness-executed builtin, which is precisely the check the
/// field's own docs invite ("the error, if it failed"). Treating blank as
/// absent keeps `is_some()` meaning what it says, on both the custom-tool
/// dispatch path and the harness post-tool path.
pub(super) fn normalize_tool_error(error: Option<String>) -> Option<String> {
    error.filter(|e| !e.trim().is_empty())
}

/// Like [`unwrap_result_value`], for a harness-supplied result *string*
/// (the `PostToolArgs.result` wire field). Only a lone-`result` JSON object
/// envelope is unwrapped; any other payload (plain text, a multi-key object)
/// is handed back verbatim.
pub(super) fn unwrap_result_string(raw: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(raw)
        && let Value::Object(map) = &value
        && map.len() == 1
        && map.contains_key("result")
    {
        return unwrap_result_value(&value);
    }
    raw.to_string()
}

/// Maps the harness's question batch into the hook-facing view.
///
/// `question`/`choices`/`is_multi_select` fall back to empty/false when
/// `multiple_choice` (or an inner field) is absent — `choices` is the
/// index space [`hooks::QuestionAnswer::Choices`] selections refer to,
/// so the substitution is observable to the hook and pinned by tests.
/// `unknown_type` (the field hooks branch on via
/// [`AgentQuestion::is_unknown_type`]) is set exactly when the
/// `multiple_choice` arm is absent from the wire.
/// Free function so the mapping is unit-testable.
pub(super) fn map_questions(request: &protocol::UserQuestionsRequest) -> Vec<AgentQuestion> {
    request
        .questions
        .iter()
        .map(|q| {
            let mc = q.multiple_choice.as_ref();
            if mc.is_none() {
                // A future question type reaches the hook flagged via
                // `unknown_type` (is_unknown_type()) with its payload
                // merged into `extra`; this warn is the log-side signal
                // alongside that programmatic one (mirrors unknown-action
                // handling).
                tracing::warn!(
                    "Question with no multiple_choice arm (unknown type?); \
                     hook sees a blank question. Extra fields: {:?}",
                    q.extra.keys().collect::<Vec<_>>()
                );
            }
            if let Some(m) = mc
                && m.question.is_none()
            {
                tracing::warn!(
                    "multiple_choice present but its question text is absent; \
                     hook sees an empty question above {} choice(s)",
                    m.choices.len()
                );
            }
            // Merge outer (UserQuestion) and inner (MultipleChoice)
            // unmodeled fields so the hook-facing shape is lossless at
            // both levels; a key collision (none exist in the protocol
            // today) resolves to the inner value.
            let mut extra = q.extra.clone();
            if let Some(m) = mc {
                extra.extend(m.extra.clone());
            }
            AgentQuestion {
                question: mc.and_then(|m| m.question.clone()).unwrap_or_default(),
                choices: mc.map(|m| m.choices.clone()).unwrap_or_default(),
                is_multi_select: mc.and_then(|m| m.is_multi_select).unwrap_or(false),
                extra,
                unknown_type: mc.is_none(),
            }
        })
        .collect()
}

/// Maps a questions-hook reply (or the hookless `None` fallback) onto the
/// protocol's `cancelled`/`response` oneof.
///
/// A short answer list is padded with "unanswered", a long one truncated,
/// both with a `warn!`; hook-selected choice indices are validated against
/// the question batch (out-of-range or single-select violations `warn!`
/// but are still relayed — the harness owns the final verdict).
/// `trajectory_id`/`step_index` are left for the caller to fill in. Free
/// function so the mapping is unit-testable.
pub(super) fn map_question_reply(
    reply: Option<QuestionReply>,
    questions: &[AgentQuestion],
) -> protocol::UserQuestionsResponse {
    let question_count = questions.len();
    let (cancelled, answers) = match reply {
        Some(QuestionReply::Cancel) => (true, Vec::new()),
        Some(QuestionReply::Answers(answers)) => {
            if answers.len() != question_count {
                tracing::warn!(
                    "Questions hook returned {} answer(s) for {} question(s); \
                     padding with unanswered / dropping extras",
                    answers.len(),
                    question_count
                );
            }
            let mapped = answers
                .into_iter()
                .map(Some)
                .chain(std::iter::repeat_with(|| None))
                .take(question_count)
                .zip(questions.iter())
                .map(|(answer, question)| {
                    // An unknown-type question has no rendered text or
                    // choices — answering it is guesswork, and Freeform
                    // (the natural attempt) would otherwise relay with no
                    // signal at all, unlike the index checks below.
                    if question.unknown_type
                        && !matches!(answer, None | Some(hooks::QuestionAnswer::Unanswered))
                    {
                        tracing::warn!(
                            "Questions hook answered a question whose type this crate does \
                             not model; relaying anyway (prefer Cancel or Unanswered — see \
                             AgentQuestion::is_unknown_type)"
                        );
                    }
                    match answer {
                        None | Some(hooks::QuestionAnswer::Unanswered) => {
                            protocol::UserQuestionAnswer::unanswered()
                        }
                        Some(hooks::QuestionAnswer::Freeform(text)) => {
                            protocol::UserQuestionAnswer {
                                unanswered: None,
                                multiple_choice_answer: Some(protocol::MultipleChoiceAnswer {
                                    selected_choice_indices: Vec::new(),
                                    freeform_response: Some(text),
                                }),
                            }
                        }
                        Some(hooks::QuestionAnswer::Choices { selected, freeform }) => {
                            for &idx in &selected {
                                if idx >= question.choices.len() {
                                    tracing::warn!(
                                        "Questions hook selected index {idx} for a question \
                                     with {} choice(s); relaying anyway",
                                        question.choices.len()
                                    );
                                }
                            }
                            if selected.len() > 1 && !question.is_multi_select {
                                tracing::warn!(
                                    "Questions hook selected {} choices for a single-select \
                                 question; relaying anyway",
                                    selected.len()
                                );
                            }
                            if selected.is_empty() && freeform.is_none() {
                                tracing::warn!(
                                    "Questions hook returned an empty Choices answer with no \
                                 freeform (did you mean Unanswered?); relaying anyway"
                                );
                            }
                            // The wire type is i32; an index that cannot fit
                            // is absurd (no real choice list is that long) and
                            // drops with a warn rather than wrapping.
                            let selected_choice_indices = selected
                                .iter()
                                .filter_map(|&idx| {
                                    i32::try_from(idx)
                                        .map_err(|_| {
                                            tracing::warn!(
                                                "Questions hook selected index {idx} \
                                             exceeds the i32 wire range; dropping"
                                            );
                                        })
                                        .ok()
                                })
                                .collect();
                            protocol::UserQuestionAnswer {
                                unanswered: None,
                                multiple_choice_answer: Some(protocol::MultipleChoiceAnswer {
                                    selected_choice_indices,
                                    freeform_response: freeform,
                                }),
                            }
                        }
                    }
                })
                .collect();
            (false, mapped)
        }
        None => (
            false,
            (0..question_count)
                .map(|_| protocol::UserQuestionAnswer::unanswered())
                .collect(),
        ),
    };
    protocol::UserQuestionsResponse {
        trajectory_id: String::new(),
        step_index: 0,
        cancelled: cancelled.then_some(true),
        response: (!cancelled).then_some(protocol::QuestionsResponse { answers }),
    }
}

#[cfg(test)]
#[path = "hook_mapping_tests.rs"]
mod tests;
