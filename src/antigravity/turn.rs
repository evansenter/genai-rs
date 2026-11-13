use super::agent::TurnGuard;
use super::protocol::{
    self, OutputEvent, OutputPayload, StepSource, StepState, StepTarget, StepUpdate,
    TrajectoryState,
};
use super::{
    AgentEvent, AntigravityAgent, AntigravityError, ErrorSeverity, ToolDecision, TurnFailure,
    streaming,
};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::Duration;

/// Model backend HTTP codes that abort the turn (bad request / auth
/// failures cannot recover by retrying within the turn).
const FATAL_HTTP_CODES: [u32; 3] = [400, 401, 403];

/// The HTTP error statuses: a system error step's `httpCode` outside them
/// is not the model backend's (the harness puts gRPC codes there too).
const HTTP_ERROR_STATUSES: std::ops::Range<u32> = 400..600;

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
                self.process_hook_request(*request, turn).await?;
            }
            Some(OutputPayload::PolicyDecisionRequest(request)) => {
                self.process_policy_decision(request, turn).await?;
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
        // so `announced_actions` dedups it here. A call the pre-tool hook (or
        // the auto policy's verdict) denied has no confirmation step: the
        // harness turns it into an error step carrying the reason and no
        // action, which `take_hook_denial` recognizes and names from what
        // the hook was handed. Anything else reaching a terminal state
        // executed.
        if is_terminal {
            if let Some(denial) = turn.take_hook_denial(&step) {
                turn.announce_tool_action(
                    &step_key,
                    streaming::ToolAction::Denied {
                        name: denial.name,
                        args: denial.args,
                    },
                    ToolDecision::Denied {
                        reason: denial.reason,
                    },
                    step.trajectory_id.clone(),
                );
            } else if let Some(action) = streaming::ToolAction::from_step(&step) {
                turn.announce_tool_action(
                    &step_key,
                    action,
                    ToolDecision::Allowed,
                    step.trajectory_id.clone(),
                );
            }
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
            if let Some(status) = backend_status(&step) {
                if FATAL_HTTP_CODES.contains(&status) {
                    return Err(AntigravityError::turn(
                        TurnFailure::ModelBackend { status },
                        message,
                    ));
                }
                // Any other status the harness may retry; if the turn then
                // fails, this is why.
                turn.backend_status = Some(status);
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
                self.answer_questions(&step, questions, turn).await?;
            }
        }
        Ok(())
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
        // Harness 0.1.23 can report the error that ends its run on a
        // non-terminal update (a 503 while waiting for subagents) and then
        // go idle without it, so the main trajectory's first error is kept
        // for the terminal update.
        if is_main && let Some(error) = non_blank(update.error.as_deref()) {
            turn.trajectory_error
                .get_or_insert_with(|| error.to_string());
        }
        let error = non_blank(update.error.as_deref()).or(turn.trajectory_error.as_deref());
        // A turn the pre-turn hook refused ends in one of the failing
        // states below, carrying the hook's reason.
        let denied = turn.pre_turn_denied;
        let failure = |otherwise| {
            if denied {
                TurnFailure::Denied
            } else {
                otherwise
            }
        };
        match update.state {
            Some(TrajectoryState::Idle) if is_main => {
                if let Some(error) = error {
                    // An error ending the turn after the model backend
                    // failed a call (a 503 whose retries ran out) is that
                    // failure.
                    let kind = turn
                        .backend_status
                        .map_or(TurnFailure::Trajectory, |status| {
                            TurnFailure::ModelBackend { status }
                        });
                    return Err(AntigravityError::turn(failure(kind), error));
                }
                let response = turn.take_response();
                turn.finished = true;
                turn.queue
                    .push_back(AgentEvent::Finished(Box::new(response)));
            }
            Some(TrajectoryState::Cancelled) if is_main => {
                return Err(AntigravityError::turn(
                    failure(TurnFailure::Cancelled),
                    error.unwrap_or("no reason given"),
                ));
            }
            _ => {
                // Running / waiting-for-tasks / waiting-for-subagents /
                // subagent idle or cancelled: nothing to do for the parent
                // turn.
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
}

/// The HTTP status of a model call the backend failed, from a system error
/// step. The step's `httpCode` holds other codes too, which are not the
/// backend's.
fn backend_status(step: &StepUpdate) -> Option<u32> {
    // On 0.1.23 (scripted, 2026-11-09) a 503 with no retries left comes as
    // 503, and an MCP server that failed to load as 2, gRPC's UNKNOWN, on
    // the same "Agent execution terminated" step; either way the harness
    // then ends its run and closes the WebSocket.
    let status = step.error.as_ref().and_then(|e| e.http_code)?;
    (step.source == Some(StepSource::System) && HTTP_ERROR_STATUSES.contains(&status))
        .then_some(status)
}

/// Classifies a harness error step's severity for
/// [`AgentEvent::Error`](streaming::AgentEvent). A fatal model-backend
/// HTTP code (400/401/403) that reached the event path — i.e. did *not* abort
/// the turn as a [`TurnFailure::ModelBackend`] — is
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
///
/// A step in an error state streams nothing: harness 0.1.23 puts the error
/// text in the delta of a `SOURCE_MODEL`/`TARGET_USER` error step (a hook
/// denial, for one), which would otherwise read as the model's answer. The
/// error surfaces as [`AgentEvent::Error`] instead, as in the Python SDK.
fn stream_deltas(step: &StepUpdate) -> (Option<&str>, Option<&str>) {
    if step.source != Some(StepSource::Model) || step.state == Some(StepState::Error) {
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

/// A denial this client sent (from its policies, pre-tool hook, or an auto
/// policy verdict), awaiting the error step the harness turns it into (see
/// [`TurnState::take_hook_denial`]). It keeps the call's name and arguments,
/// which that error step does not carry.
#[derive(Debug, Clone)]
pub(super) struct HookDenial {
    pub(super) trajectory_id: Option<String>,
    pub(super) reason: String,
    pub(super) name: String,
    pub(super) args: Value,
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
    pub(super) hook_denials: Vec<HookDenial>,
    /// Custom tool calls (by call id) whose outcome `on_post_tool` has
    /// already seen via the dispatch path.
    pub(super) reported_calls: HashSet<String>,
    /// The client-side work in flight (a custom tool or a hook), while it
    /// runs. Still set when the turn deadline cancels it, which is how the
    /// timeout error names what held the turn.
    pub(super) running: Option<String>,
    pub(super) timeout: Option<Duration>,
    pub(super) deadline: Option<tokio::time::Instant>,
    /// Held for the turn's lifetime; dropping the state marks the agent
    /// idle again (releasing deferred trigger deliveries). `None` for
    /// throwaway drain states, which run *inside* a caller that already
    /// holds the real guard.
    _turn_guard: Option<TurnGuard>,
    /// The turn is being halted and drained, with no one to consume it:
    /// its per-turn lifecycle hooks get the answer they would get with no
    /// hook set, instead of running user code.
    pub(super) draining: bool,
    /// The pre-turn hook refused this turn, so its failing end is a
    /// [`TurnFailure::Denied`].
    pub(super) pre_turn_denied: bool,
    /// The status of the last model call the backend failed this turn
    /// (from a system error step), so a turn the harness then ends with an
    /// error is a [`TurnFailure::ModelBackend`].
    backend_status: Option<u32>,
    /// The first error a main-trajectory state update carried this turn,
    /// which fails the turn when it ends even if the terminal update has
    /// none.
    trajectory_error: Option<String>,
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
            running: None,
            timeout,
            // A timeout too large for the clock (`Duration::MAX`) is no
            // deadline, as it is for `tokio::time::sleep`.
            deadline: timeout.and_then(|t| tokio::time::Instant::now().checked_add(t)),
            _turn_guard: turn_guard,
            draining: false,
            pre_turn_denied: false,
            backend_status: None,
            trajectory_error: None,
        }
    }

    /// Takes the guard that marks the agent busy for the turn's lifetime.
    /// The turn's state, and so its deadline, exists before the guard: the
    /// wait for the lock the guard is taken under counts against it.
    pub(super) fn hold(&mut self, guard: TurnGuard) {
        self._turn_guard = Some(guard);
    }

    /// The turn's timeout error, `cause` saying what held it.
    pub(super) fn timed_out(&self, cause: &str) -> AntigravityError {
        AntigravityError::Timeout {
            operation: format!("agent turn ({cause})"),
            timeout: self.timeout.unwrap_or_default(),
        }
    }

    /// The throwaway state `halt_and_drain` answers a stale turn's requests
    /// with.
    pub(super) fn draining() -> Self {
        Self {
            draining: true,
            ..Self::new(None, None)
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
    ///
    /// Two more cases name themselves: a custom tool or hook of this
    /// client's that was still running at the deadline (the stall is on
    /// this side, not the harness's), and a harness whose installed
    /// version (`harness_version`) differs from
    /// [`SUPPORTED_HARNESS_VERSION`](super::SUPPORTED_HARNESS_VERSION).
    pub(super) fn stall_diagnosis(&self, stderr: &str, harness_version: Option<&str>) -> String {
        if let Some(running) = &self.running {
            return format!("agent turn ({running} was still running at the deadline)");
        }
        let mismatch = super::process::version_mismatch(harness_version);
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
            return match mismatch {
                Some(mismatch) => format!("agent turn ({mismatch})"),
                None => "agent turn".to_string(),
            };
        }
        causes.extend(mismatch);
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
    /// Since harness 0.1.18 the harness gates the agent's own tool calls
    /// through the pre-tool hook rather than a confirmation step (a
    /// subagent's own calls are not gated), and answers a denial with a
    /// `STATE_ERROR` step whose `errorMessage` is the verdict's reason
    /// verbatim (and no action payload). The match is on that exact string
    /// — the one this client itself sent — in the same trajectory, so it is
    /// structural rather than message-sniffing: nothing the model or a tool
    /// wrote can produce it by accident.
    fn take_hook_denial(&mut self, step: &StepUpdate) -> Option<HookDenial> {
        if step.state != Some(StepState::Error) {
            return None;
        }
        let message = non_blank(step.error_message.as_deref())?;
        let index = self.hook_denials.iter().position(|denial| {
            denial.reason == message
                && (denial.trajectory_id.is_none()
                    || denial.trajectory_id.as_deref() == step.trajectory_id.as_deref())
        })?;
        Some(self.hook_denials.remove(index))
    }

    /// Whether the harness post-tool callback for `call_id` duplicates an
    /// outcome the custom-tool dispatch path already reported (consuming
    /// the record). The harness echoes the tool call's own id as the
    /// callback's `callId`, so the match is exact; a callback with no id
    /// is never treated as a duplicate.
    pub(super) fn take_reported_call(&mut self, call_id: Option<&str>) -> bool {
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
