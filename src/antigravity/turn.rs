use super::agent::TurnGuard;
use super::hook_mapping::{
    confirmation_decision, map_question_reply, map_questions, normalize_tool_error,
    pre_tool_invocation, unwrap_result_string, unwrap_result_value,
};
use super::hooks;
use super::protocol::{
    self, HookDecision, HookVerdict, InputEvent, OutputEvent, OutputPayload, StepSource, StepState,
    StepTarget, StepUpdate, TrajectoryState,
};
use super::{
    AgentEvent, AntigravityAgent, AntigravityError, ErrorSeverity, PreToolDecision, ToolDecision,
    ToolInvocation, ToolOutcome, streaming,
};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::Duration;

/// Model backend HTTP codes that abort the turn (bad request / auth
/// failures cannot recover by retrying within the turn).
const FATAL_HTTP_CODES: [u32; 3] = [400, 401, 403];

// =============================================================================
// ChatResponse
// =============================================================================

/// The assembled result of one agent turn.
#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    text: String,
    thoughts: String,
    usage: Option<protocol::UsageMetadata>,
    structured_output: Option<Value>,
    errors: Vec<String>,
    stop_reason: Option<protocol::StopReason>,
}

impl ChatResponse {
    /// The final response text (the last completed model step directed at
    /// the user).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Concatenated thinking text from the turn's completed steps.
    #[must_use]
    pub fn thoughts(&self) -> &str {
        &self.thoughts
    }

    /// Token usage reported for the turn (last report wins).
    #[must_use]
    pub fn usage(&self) -> Option<&protocol::UsageMetadata> {
        self.usage.as_ref()
    }

    /// Structured output from the agent's `finish` action, when a response
    /// schema was configured.
    #[must_use]
    pub fn structured_output(&self) -> Option<&Value> {
        self.structured_output.as_ref()
    }

    /// Non-fatal errors the harness reported during the turn.
    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Why the harness stopped the turn early, when it did — e.g.
    /// [`StopReason::QuotaExhausted`](protocol::StopReason::QuotaExhausted).
    /// `None` for a normal completion.
    ///
    /// A stopped turn still *finishes*: it ends in the same terminal state
    /// as a completed one, so without this the only symptom is a short or
    /// empty [`text`](Self::text). New in harness 0.1.18.
    #[must_use]
    pub fn stop_reason(&self) -> Option<&protocol::StopReason> {
        self.stop_reason.as_ref()
    }
}

impl AntigravityAgent {
    pub(super) async fn process_event(
        &mut self,
        event: OutputEvent,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        if let Some(usage) = event.usage_metadata {
            turn.usage = Some(usage);
        }
        match event.payload {
            Some(OutputPayload::StepUpdate(step)) => self.process_step(*step, turn).await?,
            Some(OutputPayload::TrajectoryStateUpdate(update)) => {
                Self::process_trajectory_update(&update, turn)?;
            }
            Some(OutputPayload::ToolCall(call)) => self.process_tool_call(call, turn).await?,
            Some(OutputPayload::CallHookRequest(request)) => {
                self.process_hook_request(request, turn).await?;
            }
            Some(OutputPayload::SessionEndResponse(_)) => {}
            Some(OutputPayload::InitializeConversationResponse(_)) => {
                tracing::warn!("Unexpected initializeConversationResponse mid-turn; ignoring.");
            }
            Some(OutputPayload::Unknown { event_type, data }) => {
                tracing::warn!("Unknown harness event '{event_type}'; surfacing and continuing.");
                turn.queue
                    .push_back(AgentEvent::Unknown { event_type, data });
            }
            None => {}
        }
        Ok(())
    }

    async fn process_step(
        &mut self,
        step: StepUpdate,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        // Proto3 JSON omits default-valued scalars, so an absent
        // `step_index` *is* index 0 (and an absent `trajectory_id` is the
        // empty id) — mapping `None` to the default is the correct
        // decoding, not a key collision between distinct steps.
        let step_key = (
            step.trajectory_id.clone().unwrap_or_default(),
            step.step_index.unwrap_or_default(),
        );
        let is_subagent_step = non_blank(step.parent_trajectory_id.as_deref()).is_some();
        turn.observe_trajectory(step.trajectory_id.as_deref(), is_subagent_step);
        let is_main =
            !is_subagent_step && turn.main_trajectory.as_deref() == step.trajectory_id.as_deref();

        // Debounce bookkeeping: leaving the waiting state clears the
        // handled-request markers for the step.
        if step.state != Some(StepState::WaitingForUser) {
            turn.handled_waits.remove(&step_key);
        }

        // Deltas stream through from every trajectory (subagents
        // included), but only the model's own output — see `stream_deltas`.
        let (thinking_delta, text_delta) = stream_deltas(&step);
        if let Some(delta) = thinking_delta {
            turn.queue
                .push_back(AgentEvent::ThinkingDelta(delta.to_string()));
        }
        if let Some(delta) = text_delta {
            turn.queue
                .push_back(AgentEvent::TextDelta(delta.to_string()));
        }

        let is_terminal = matches!(step.state, Some(StepState::Done) | Some(StepState::Error));

        // Completed tool actions surface once. A denied action was already
        // announced (with its `Denied` decision) at its confirmation step,
        // so `announced_actions` dedups it here. A call the pre-tool *hook*
        // denied has no confirmation step: the harness turns it into an
        // error step carrying the hook's reason, which `take_hook_denial`
        // recognizes. Anything else reaching a terminal state executed.
        if is_terminal && let Some(action) = streaming::ToolAction::from_step(&step) {
            let decision = match turn.take_hook_denial(&step) {
                Some(reason) => ToolDecision::Denied { reason },
                None => ToolDecision::Allowed,
            };
            turn.announce_tool_action(&step_key, action, decision, step.trajectory_id.clone());
        }

        // Structured output from the finish action.
        if let Some(finish) = &step.finish
            && let Some(output) = &finish.output_string
            && !output.is_empty()
        {
            match serde_json::from_str(output) {
                Ok(value) => turn.structured_output = Some(value),
                Err(e) => tracing::warn!("Failed to parse structured output JSON: {e}"),
            }
        }

        // Errors: fatal model-backend codes abort the turn; everything else
        // is surfaced as an event and recorded (the harness retries or the
        // model reacts).
        if step.state == Some(StepState::Error) || step.error.is_some() {
            let message = non_blank(step.error.as_ref().and_then(|e| e.error_message.as_deref()))
                .or_else(|| non_blank(step.error_message.as_deref()))
                .or_else(|| non_blank(step.text.as_deref()))
                .unwrap_or("unknown harness error")
                .to_string();
            let http_code = step.error.as_ref().and_then(|e| e.http_code);
            if step.source == Some(StepSource::System)
                && http_code.is_some_and(|code| FATAL_HTTP_CODES.contains(&code))
            {
                return Err(AntigravityError::Turn(format!(
                    "model backend error (HTTP {}): {message}",
                    http_code.unwrap_or_default()
                )));
            }
            // An error step is terminal, so the harness re-delivers it on
            // each tick; dedup on the step key (a dedicated set, not
            // `announced_actions` — a step can carry both an action and an
            // error, and the two must each surface exactly once).
            if turn.announced_errors.insert(step_key.clone()) {
                turn.errors.push(message.clone());
                // Reaching here means the turn continues: the turn-aborting
                // case (system-source fatal HTTP code) returned above. A fatal
                // code that did *not* abort (non-system source) is still
                // serious, so it surfaces as `Severe`; everything else is
                // transient harness-internal noise (retried, model reacts).
                let severity = classify_error_severity(http_code);
                turn.queue
                    .push_back(AgentEvent::Error { message, severity });
            }
        }

        // Thinking text accumulates from completed steps.
        if step.state == Some(StepState::Done)
            && let Some(thinking) = &step.thinking
            && !thinking.is_empty()
            && turn.thought_steps.insert(step_key.clone())
        {
            if !turn.thoughts.is_empty() {
                turn.thoughts.push('\n');
            }
            turn.thoughts.push_str(thinking);
        }

        // Final-response candidate: completed model text directed at the
        // user, on the main trajectory. The last one wins.
        if is_main
            && step.source == Some(StepSource::Model)
            && step.state == Some(StepState::Done)
            && step.target == Some(StepTarget::User)
            && let Some(text) = &step.text
            && !text.is_empty()
        {
            turn.final_text = Some(text.clone());
        }

        // Waiting state: answer confirmation/question requests, debounced —
        // the harness re-broadcasts them on every internal tick.
        if step.state == Some(StepState::WaitingForUser) {
            if step.tool_confirmation_request.is_some()
                && turn.mark_wait_handled(&step_key, "tool_confirmation_request")
            {
                self.answer_tool_confirmation(&step, &step_key, turn)
                    .await?;
            }
            if let Some(questions) = &step.questions_request
                && turn.mark_wait_handled(&step_key, "questions_request")
            {
                self.answer_questions(&step, questions).await?;
            }
        }
        Ok(())
    }

    /// Policy-checks a pending harness-side tool and replies with a
    /// `tool_confirmation`. When the decision is a denial, the blocked
    /// action is surfaced as a [`AgentEvent::ToolAction`] with a
    /// [`ToolDecision::Denied`] marker (deduped against the terminal-step
    /// emission via `announced_actions`) so consumers see denied actions
    /// distinctly — a denied action still reaches a terminal step, but with
    /// no signal that it was blocked.
    async fn answer_tool_confirmation(
        &mut self,
        step: &StepUpdate,
        step_key: &(String, u32),
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let decision = confirmation_decision(step, &self.policy_engine, self.pre_tool.as_ref());
        let accepted = matches!(decision, PreToolDecision::Allow);
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
    async fn answer_questions(
        &mut self,
        step: &StepUpdate,
        request: &protocol::UserQuestionsRequest,
    ) -> Result<(), AntigravityError> {
        let batch = map_questions(request);
        let reply = match &self.questions {
            Some(hook) => Some(hook(&batch)),
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

    /// Routes a trajectory lifecycle update into the turn state.
    ///
    /// Only the *main* trajectory's terminal states decide the turn's
    /// fate: subagent trajectories go idle (and can be cancelled, e.g. by
    /// a pre-turn hook denial) while the parent keeps running — subagent
    /// failures surface through their step errors, not here. Associated
    /// fn (no `&self`) so the routing logic is unit-testable.
    fn process_trajectory_update(
        update: &protocol::TrajectoryStateUpdate,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        // A trajectory with a parent is a subagent's, whatever its id —
        // the positive signal harness 0.1.18 added. Without it (0.1.10, or
        // the root), fall back to "the first trajectory seen this turn".
        let is_subagent = non_blank(update.parent_trajectory_id.as_deref()).is_some();
        turn.observe_trajectory(update.trajectory_id.as_deref(), is_subagent);
        let is_main = !is_subagent
            && (turn.main_trajectory.is_none()
                || turn.main_trajectory.as_deref() == update.trajectory_id.as_deref());
        if is_main && let Some(reason) = &update.stop_reason {
            turn.stop_reason = Some(reason.clone());
        }
        let error = non_blank(update.error.as_deref());
        match update.state {
            Some(TrajectoryState::Idle) if is_main => {
                if let Some(error) = error {
                    return Err(AntigravityError::Turn(error.to_string()));
                }
                let response = turn.take_response();
                turn.finished = true;
                turn.queue
                    .push_back(AgentEvent::Finished(Box::new(response)));
            }
            Some(TrajectoryState::Cancelled) if is_main => {
                let message = error.unwrap_or("turn cancelled").to_string();
                return Err(AntigravityError::Turn(message));
            }
            _ => {
                // Running / waiting-for-tasks / subagent idle or
                // cancelled: nothing to do for the parent turn.
                //
                // An *unrecognized* main-trajectory state is different in
                // kind, though it lands here too: only `Idle` ends a
                // turn, so if the harness renamed the terminal state (as
                // 0.1.10 did — `STATE_IDLE` -> `STATE_FULLY_IDLE`) the
                // turn runs to its timeout with nothing else to show for
                // it. Record the value so the timeout can name the cause
                // instead of reporting a bare stall.
                if is_main
                    && let Some(state) = &update.state
                    && let Some(unknown) = state.unknown_state_type()
                {
                    turn.unknown_trajectory_states.insert(unknown.to_string());
                }
            }
        }
        Ok(())
    }

    /// Dispatches a custom (client-executed) tool call: policy check first
    /// (defense in depth), then execution through the crate's function
    /// registry / tool services, then a `tool_response` back to the
    /// harness.
    async fn process_tool_call(
        &mut self,
        call: protocol::ToolCall,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let id = call.id.clone().unwrap_or_default();
        let name = call.name.clone().unwrap_or_default();
        let args: Value = match call
            .arguments_json
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(serde_json::from_str)
        {
            Some(Ok(value)) => value,
            // `arguments` is the harness's `genai.Struct` encoding, not
            // plain JSON: handing it over raw gave the tool
            // `{"fields": [{"name": .., "value": {"stringValue": ..}}]}`
            // instead of its arguments.
            Some(Err(e)) => {
                tracing::warn!("Unparseable arguments for tool '{name}': {e}");
                call.arguments
                    .as_ref()
                    .and_then(protocol::decode_genai_struct)
                    .unwrap_or(Value::Null)
            }
            None => call
                .arguments
                .as_ref()
                .and_then(protocol::decode_genai_struct)
                .unwrap_or_else(|| Value::Object(Default::default())),
        };

        let invocation = ToolInvocation {
            name: name.clone(),
            args: args.clone(),
            id: Some(id.clone()),
        };
        let result = match hooks::decide(&self.policy_engine, self.pre_tool.as_ref(), &invocation) {
            PreToolDecision::Deny { reason } => {
                tracing::info!("Denying custom tool '{name}': {reason}");
                serde_json::json!({
                    "error": format!("Tool execution denied by policy: {reason}")
                })
            }
            PreToolDecision::Allow => {
                let result = self.dispatcher.execute(&name, args).await;
                if let Some(post_tool) = &self.post_tool {
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
                    post_tool(&outcome);
                    turn.reported_calls.insert(id.clone());
                }
                turn.queue.push_back(AgentEvent::ToolCallDispatched {
                    name: name.clone(),
                    id: id.clone(),
                });
                result
            }
        };
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
    async fn process_hook_request(
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
            let invocation = pre_tool_invocation(pre_tool_args);
            let verdict =
                match hooks::decide(&self.policy_engine, self.pre_tool.as_ref(), &invocation) {
                    PreToolDecision::Allow => HookVerdict {
                        decision: Some(HookDecision::Allow),
                        reason: None,
                    },
                    PreToolDecision::Deny { reason } => {
                        turn.hook_denials.push(HookDenial {
                            trajectory_id: pre_tool_args.trajectory_id.clone(),
                            reason: reason.clone(),
                        });
                        HookVerdict {
                            decision: Some(HookDecision::Deny),
                            reason: Some(reason),
                        }
                    }
                };
            response.pre_tool_result = Some(verdict);
        } else if let Some(post_tool_args) = &request.post_tool_args {
            // Harness 0.1.18 also fires its post-tool callback for custom
            // tools, which the dispatch path has already reported — once
            // per call is the contract.
            let already_reported = turn.take_reported_call(post_tool_args.call_id.as_deref());
            if let Some(post_tool) = &self.post_tool
                && !already_reported
            {
                post_tool(&ToolOutcome {
                    name: protocol::hook_tool_name(
                        post_tool_args.tool_name.as_deref().unwrap_or_default(),
                        post_tool_args.server_name.as_deref(),
                    ),
                    // Unwrap the `{"result": ...}` envelope for consistency
                    // with the custom-tool dispatch path.
                    result: post_tool_args.result.as_deref().map(unwrap_result_string),
                    error: normalize_tool_error(post_tool_args.error.clone()),
                });
            }
            response.empty_result = Some(protocol::EmptyResult {});
        } else {
            response.empty_result = Some(protocol::EmptyResult {});
        }
        self.session
            .send(&InputEvent::CallHookResponse(response))
            .await
    }
}

/// Classifies a harness error step's severity for
/// [`AgentEvent::Error`](streaming::AgentEvent). A fatal model-backend
/// HTTP code (400/401/403) that reached the event path — i.e. did *not* abort
/// the turn through the system-source [`AntigravityError::Turn`] path — is
/// still serious and surfaces as [`ErrorSeverity::Severe`]; every other
/// error is transient harness-internal noise the turn recovers from.
fn classify_error_severity(http_code: Option<u32>) -> ErrorSeverity {
    if http_code.is_some_and(|code| FATAL_HTTP_CODES.contains(&code)) {
        ErrorSeverity::Severe
    } else {
        ErrorSeverity::Transient
    }
}

/// The `(thinking, text)` deltas of a step that belong on the event stream.
///
/// Every step carries a `textDelta`, not just the model's answer: the echo
/// of the user's own input arrives as a `SOURCE_USER` step, and each tool
/// step's one-line summary (`"Weather check"`, `"Invoke haiku-writer
/// subagent"`) streams as the delta of a `TARGET_ENVIRONMENT` step.
/// Forwarding them all put the user's prompt and tool labels into
/// [`AgentEvent::TextDelta`]. Text therefore needs `SOURCE_MODEL` *and*
/// `TARGET_USER` — the reference SDK's `receive_chunks` filter. Thinking
/// needs only `SOURCE_MODEL`: reasoning ahead of a tool call is still the
/// model's (the reference SDK drops it; keeping it loses nothing).
fn stream_deltas(step: &StepUpdate) -> (Option<&str>, Option<&str>) {
    if step.source != Some(StepSource::Model) {
        return (None, None);
    }
    let thinking = step.thinking_delta.as_deref().filter(|d| !d.is_empty());
    let text = (step.target == Some(StepTarget::User))
        .then_some(step.text_delta.as_deref())
        .flatten()
        .filter(|d| !d.is_empty());
    (thinking, text)
}

/// Treats a blank wire string as absent.
///
/// Proto3 JSON is *allowed* to omit a default-valued string, but harness
/// 0.1.18 often emits it explicitly (`"thinking": ""`, `"serverName": ""`,
/// `"unavailableReason": ""`). Wherever presence carries meaning — an
/// error string that fails the turn, a parent id that marks a subagent —
/// `Some("")` must read as `None`.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.trim().is_empty())
}

// =============================================================================
// Per-turn state
// =============================================================================

/// A pre-tool hook denial this client sent, awaiting the error step the
/// harness turns it into (see [`TurnState::take_hook_denial`]).
struct HookDenial {
    trajectory_id: Option<String>,
    reason: String,
}

pub(super) struct TurnState {
    pub(super) queue: VecDeque<AgentEvent>,
    pub(super) finished: bool,
    /// Unrecognized *main-trajectory* states seen this turn. Populated
    /// only on the Evergreen `Unknown` path; drives the stall diagnostic
    /// when a turn times out without ever reaching a terminal state.
    unknown_trajectory_states: BTreeSet<String>,
    main_trajectory: Option<String>,
    handled_waits: HashMap<(String, u32), HashSet<&'static str>>,
    announced_actions: HashSet<(String, u32)>,
    announced_errors: HashSet<(String, u32)>,
    thought_steps: HashSet<(String, u32)>,
    final_text: Option<String>,
    thoughts: String,
    usage: Option<protocol::UsageMetadata>,
    structured_output: Option<Value>,
    errors: Vec<String>,
    stop_reason: Option<protocol::StopReason>,
    hook_denials: Vec<HookDenial>,
    /// Custom tool calls (by call id) whose outcome `on_post_tool` has
    /// already seen via the dispatch path.
    reported_calls: HashSet<String>,
    pub(super) timeout: Option<Duration>,
    pub(super) deadline: Option<tokio::time::Instant>,
    /// Held for the turn's lifetime; dropping the state marks the agent
    /// idle again (releasing deferred trigger deliveries). `None` for
    /// throwaway drain states, which run *inside* a caller that already
    /// holds the real guard.
    _turn_guard: Option<TurnGuard>,
}

impl TurnState {
    pub(super) fn new(timeout: Option<Duration>, turn_guard: Option<TurnGuard>) -> Self {
        Self {
            queue: VecDeque::new(),
            finished: false,
            unknown_trajectory_states: BTreeSet::new(),
            main_trajectory: None,
            handled_waits: HashMap::new(),
            announced_actions: HashSet::new(),
            announced_errors: HashSet::new(),
            thought_steps: HashSet::new(),
            final_text: None,
            thoughts: String::new(),
            usage: None,
            structured_output: None,
            errors: Vec::new(),
            stop_reason: None,
            hook_denials: Vec::new(),
            reported_calls: HashSet::new(),
            timeout,
            deadline: timeout.map(|t| tokio::time::Instant::now() + t),
            _turn_guard: turn_guard,
        }
    }

    /// Describes *why* a turn stalled, for the timeout error's
    /// `operation` field.
    ///
    /// A bare "agent turn timed out" is the least actionable error the
    /// bridge can raise: it looks identical whether the model is slow,
    /// the harness died quietly, or — the case this exists for — the
    /// harness renamed the terminal trajectory state and the bridge
    /// stopped recognizing the end of a turn. When unknown
    /// main-trajectory states were seen, name them and the likely cause,
    /// so the failure points at the version mismatch instead of looking
    /// like latency.
    ///
    /// The other version-mismatch shape is the harness *rejecting* a
    /// message this build sent. It reports that only on stderr (the 0.1.18
    /// `userInput` change: `failed to unmarshal InputEvent`), never on the
    /// socket, so the turn simply never starts; `stderr` is the harness's
    /// retained stderr tail, searched for that line.
    pub(super) fn stall_diagnosis(&self, stderr: &str) -> String {
        let mut causes = Vec::new();
        if !self.unknown_trajectory_states.is_empty() {
            let states: Vec<_> = self
                .unknown_trajectory_states
                .iter()
                .map(String::as_str)
                .collect();
            causes.push(format!(
                "never saw a terminal trajectory state; the harness sent \
                 unrecognized state(s) [{}] that this build does not treat as \
                 terminal",
                states.join(", ")
            ));
        }
        if let Some(line) = stderr
            .lines()
            .rev()
            .find(|line| line.contains("failed to unmarshal"))
        {
            causes.push(format!(
                "the harness rejected a message this session sent, reporting on \
                 its stderr: {:?}",
                line.trim()
            ));
        }
        if causes.is_empty() {
            return "agent turn".to_string();
        }
        format!(
            "agent turn ({} — most likely a harness/bridge version mismatch, \
             see antigravity::SUPPORTED_HARNESS_VERSION)",
            causes.join("; ")
        )
    }

    /// Records the turn's main (root) trajectory the first time a
    /// root-level trajectory id is seen. `is_subagent` — the event carried
    /// a parent trajectory id — never establishes it, so a subagent event
    /// arriving first cannot claim the turn.
    fn observe_trajectory(&mut self, trajectory_id: Option<&str>, is_subagent: bool) {
        if self.main_trajectory.is_none()
            && !is_subagent
            && let Some(id) = non_blank(trajectory_id)
        {
            self.main_trajectory = Some(id.to_string());
        }
    }

    /// Claims the pending hook denial a terminal error step reports, if
    /// any, returning its reason.
    ///
    /// Harness 0.1.18 gates every observed tool call through the pre-tool
    /// hook rather than a confirmation step, and answers a denial with a
    /// `STATE_ERROR` step whose `errorMessage` is the verdict's reason
    /// verbatim (and no action payload). The match is on that exact string
    /// — the one this client itself sent — in the same trajectory, so it is
    /// structural rather than message-sniffing: nothing the model or a tool
    /// wrote can produce it by accident.
    fn take_hook_denial(&mut self, step: &StepUpdate) -> Option<String> {
        if step.state != Some(StepState::Error) {
            return None;
        }
        let message = non_blank(step.error_message.as_deref())?;
        let index = self.hook_denials.iter().position(|denial| {
            denial.reason == message
                && (denial.trajectory_id.is_none()
                    || denial.trajectory_id.as_deref() == step.trajectory_id.as_deref())
        })?;
        Some(self.hook_denials.remove(index).reason)
    }

    /// Whether the harness post-tool callback for `call_id` duplicates an
    /// outcome the custom-tool dispatch path already reported (consuming
    /// the record). The harness echoes the tool call's own id as the
    /// callback's `callId`, so the match is exact; a callback with no id
    /// is never treated as a duplicate.
    fn take_reported_call(&mut self, call_id: Option<&str>) -> bool {
        non_blank(call_id).is_some_and(|id| self.reported_calls.remove(id))
    }

    /// Marks a waiting-state request as handled for the step; returns
    /// `true` on first sighting (the harness re-broadcasts requests on
    /// every internal tick while waiting).
    fn mark_wait_handled(&mut self, step_key: &(String, u32), kind: &'static str) -> bool {
        self.handled_waits
            .entry(step_key.clone())
            .or_default()
            .insert(kind)
    }

    /// Queues a [`AgentEvent::ToolAction`] for a step at most once, keyed on
    /// `step_key`. The first decision seen for a step wins: a `Denied`
    /// announcement at the confirmation step suppresses the later `Allowed`
    /// announcement at the same step's terminal delivery (the harness
    /// re-delivers terminal steps on every tick). Returns whether the event
    /// was queued.
    pub(super) fn announce_tool_action(
        &mut self,
        step_key: &(String, u32),
        action: streaming::ToolAction,
        decision: ToolDecision,
        trajectory_id: Option<String>,
    ) -> bool {
        if self.announced_actions.insert(step_key.clone()) {
            self.queue.push_back(AgentEvent::ToolAction {
                action: Box::new(action),
                decision,
                trajectory_id,
            });
            true
        } else {
            false
        }
    }

    fn take_response(&mut self) -> ChatResponse {
        ChatResponse {
            text: self.final_text.take().unwrap_or_default(),
            thoughts: std::mem::take(&mut self.thoughts),
            usage: self.usage.take(),
            structured_output: self.structured_output.take(),
            errors: std::mem::take(&mut self.errors),
            // UNSPECIFIED is the wire's "normal completion"; only a real
            // reason is worth surfacing.
            stop_reason: self
                .stop_reason
                .take()
                .filter(|r| *r != protocol::StopReason::Unspecified),
        }
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;
