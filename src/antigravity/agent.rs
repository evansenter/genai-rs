use super::hooks::PolicyEngine;
use super::process::HarnessProcess;
use super::protocol::{self, InputEvent, OutputPayload, StepUpdate};
use super::session::{Session, SinkHandle};
use super::tools::ToolDispatcher;
use super::turn::TurnState;
use super::{
    AgentBuilder, AgentEvent, AgentEventStream, AntigravityError, ChatResponse, PostToolHook,
    PreToolHook, QuestionHook, triggers,
};
use std::sync::Arc;
use std::time::Duration;

/// How long to wait for the `initializeConversationResponse`.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Overall budget for halting-and-draining an orphaned harness turn (one
/// that timed out, or one started by a trigger) before giving up.
const HALT_DRAIN_BUDGET: Duration = Duration::from_secs(5);

/// How long the event stream must stay silent during a halt-and-drain
/// before it is considered fully drained.
const HALT_DRAIN_SILENCE: Duration = Duration::from_millis(500);

// =============================================================================
// Agent
// =============================================================================

/// A running Antigravity agent session.
///
/// Created with [`AntigravityAgent::builder`]. One turn runs at a time:
/// [`chat`](Self::chat) drives it to completion, and
/// [`send_streaming`](Self::send_streaming) exposes the same loop as an
/// event stream. Call [`shutdown`](Self::shutdown) for a graceful exit (the
/// harness then persists trajectories); dropping the agent kills the
/// harness process without persistence.
pub struct AntigravityAgent {
    pub(super) harness: HarnessProcess,
    pub(super) session: Session,
    pub(super) dispatcher: ToolDispatcher,
    pub(super) policy_engine: PolicyEngine,
    pub(super) pre_tool: Option<PreToolHook>,
    pub(super) questions: Option<QuestionHook>,
    pub(super) post_tool: Option<PostToolHook>,
    pub(super) conversation_id: Option<String>,
    pub(super) initial_history: Vec<StepUpdate>,
    pub(super) turn_timeout: Option<Duration>,
    /// `true` while no turn is being driven; trigger tasks watch this to
    /// defer deliveries (see [`triggers`]).
    pub(super) idle: Arc<tokio::sync::watch::Sender<bool>>,
    /// Set by trigger tasks on each delivery. A delivered trigger starts a
    /// harness-side turn nobody consumes; the next `chat`/`send_streaming`
    /// checks this flag and halts-and-drains that turn before sending its
    /// own input, so stale events cannot desync the user's turn.
    pub(super) trigger_fired: Arc<std::sync::atomic::AtomicBool>,
    /// Serializes trigger delivery against turn begin. A trigger task
    /// holds this lock across [idle re-check → set `trigger_fired` →
    /// send]; [`begin_turn`](Self::begin_turn) holds it across [mark busy
    /// → consume `trigger_fired`]. Without it, a trigger that passed its
    /// idle check could deliver *after* the new turn consumed the flag,
    /// injecting its message into the user's turn (see [`triggers`]).
    pub(super) turn_sync: Arc<tokio::sync::Mutex<()>>,
    /// Timer tasks spawned for [`AgentBuilder::add_trigger`] configs;
    /// aborted on shutdown and on drop.
    pub(super) trigger_tasks: triggers::TriggerTasks,
}

impl std::fmt::Debug for AntigravityAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AntigravityAgent")
            .field("harness", &self.harness)
            .field("conversation_id", &self.conversation_id)
            .field("dispatcher", &self.dispatcher)
            .finish_non_exhaustive()
    }
}

/// Cancels an in-flight turn from outside the turn-driving future (e.g.
/// while consuming an [`AgentEventStream`]). Obtain via
/// [`AntigravityAgent::cancel_handle`]; cheap to clone.
///
/// # What a cancelled turn returns
///
/// Harness 0.1.10 (and still 0.1.18) answers a halt by taking the trajectory to
/// `STATE_FULLY_IDLE` — the same terminal state as a natural completion,
/// not `STATE_CANCELLED`. So the in-flight `chat`/stream **resolves
/// normally**, returning whatever partial output the turn had produced;
/// it does not fail with [`AntigravityError::Turn`]. Treat `cancel` as
/// "stop early and keep what you have", and track the cancellation on
/// your side if you need to tell a halted turn from a completed one.
///
/// [`AntigravityError::Turn`] remains the outcome when the *harness*
/// cancels a turn itself (`STATE_CANCELLED`), which is a different event
/// from this handle's halt request. Pinned by
/// `test_antigravity_cancel_handle_halts_an_in_flight_turn`.
#[derive(Debug, Clone)]
pub struct CancelHandle {
    sink: SinkHandle,
}

impl CancelHandle {
    /// Sends a halt request for the current turn.
    pub async fn cancel(&self) -> Result<(), AntigravityError> {
        self.sink.send(&InputEvent::HaltRequest(true)).await
    }
}

impl AntigravityAgent {
    /// Starts building an agent.
    #[must_use]
    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    /// The conversation id assigned by the harness. Persist it together
    /// with [`AgentBuilder::with_save_dir`] to resume the session later.
    #[must_use]
    pub fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    /// Steps restored from a saved conversation, when resuming.
    #[must_use]
    pub fn initial_history(&self) -> &[StepUpdate] {
        &self.initial_history
    }

    /// Returns a handle that can cancel an in-flight turn.
    #[must_use]
    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle {
            sink: self.session.sink_handle(),
        }
    }

    /// Cancels the current turn (equivalent to
    /// [`CancelHandle::cancel`]).
    pub async fn cancel(&self) -> Result<(), AntigravityError> {
        self.session.send(&InputEvent::HaltRequest(true)).await
    }

    /// Sends a message and drives the turn to completion.
    ///
    /// If a [trigger](AgentBuilder::add_trigger) started a harness-side turn
    /// since the last call, that turn is halted and its events are discarded
    /// before this message is sent (see [`triggers`]).
    pub async fn chat(
        &mut self,
        prompt: impl Into<String>,
    ) -> Result<ChatResponse, AntigravityError> {
        // Mark busy before sending so a trigger cannot slip in between.
        let guard = self.begin_turn().await;
        self.session.send(&InputEvent::user_text(prompt)).await?;
        let mut turn = TurnState::new(self.turn_timeout, Some(guard));
        loop {
            match self.next_turn_event(&mut turn).await? {
                Some(AgentEvent::Finished(response)) => return Ok(*response),
                Some(_) => {}
                None => {
                    return Err(AntigravityError::Protocol(
                        "turn ended without a Finished event".to_string(),
                    ));
                }
            }
        }
    }

    /// Sends a message and returns a stream of [`AgentEvent`]s for the
    /// turn. The stream ends after [`AgentEvent::Finished`].
    ///
    /// If a [trigger](AgentBuilder::add_trigger) started a harness-side turn
    /// since the last call, that turn is halted and its events are discarded
    /// before this message is sent (see [`triggers`]).
    pub async fn send_streaming(
        &mut self,
        prompt: impl Into<String>,
    ) -> Result<AgentEventStream<'_>, AntigravityError> {
        // Mark busy before sending so a trigger cannot slip in between.
        // The guard moves into the stream's turn state: dropping the
        // stream mid-turn marks the agent idle again.
        let guard = self.begin_turn().await;
        self.session.send(&InputEvent::user_text(prompt)).await?;
        let timeout = self.turn_timeout;
        let stream = async_stream::try_stream! {
            let mut turn = TurnState::new(timeout, Some(guard));
            while let Some(event) = self.next_turn_event(&mut turn).await? {
                let finished = matches!(event, AgentEvent::Finished(_));
                yield event;
                if finished {
                    break;
                }
            }
        };
        Ok(AgentEventStream::new(Box::pin(stream)))
    }

    /// Gracefully shuts down: closes the WebSocket (the harness serializes
    /// its trajectory), closes stdin (EOF triggers the harness's clean
    /// exit), then escalates to SIGTERM and SIGKILL if it lingers.
    pub async fn shutdown(mut self) -> Result<(), AntigravityError> {
        // Summarize protocol drift once, at the natural end of a session.
        // Individual `warn!`s scroll past mid-run; the aggregate is what
        // tells you the harness spoke a dialect this build only partly
        // understands — the failure mode that otherwise presents as
        // "it just didn't do anything".
        let drift = protocol::drift_report();
        if !drift.is_empty() {
            tracing::warn!(
                "This process has seen {} unrecognized antigravity wire value(s) — this \
                 build may not fully understand this harness (see \
                 SUPPORTED_HARNESS_VERSION). Cumulative across agents in this process: {:?}",
                drift.values().sum::<usize>(),
                drift
            );
        }
        // Stop trigger timers first so nothing writes to the closing socket.
        self.trigger_tasks.abort_all();
        self.session.close().await;
        self.harness.shutdown().await
    }

    // -------------------------------------------------------------------
    // Init
    // -------------------------------------------------------------------

    pub(super) async fn initialize(
        &mut self,
        init: protocol::InitializeConversationEvent,
    ) -> Result<(), AntigravityError> {
        self.session.send_raw(serde_json::to_value(&init)?).await?;

        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        loop {
            let event = match tokio::time::timeout_at(deadline, self.session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::InitFailed {
                        message: "harness closed the connection during initialization".to_string(),
                        stderr,
                    });
                }
                Ok(Err(e)) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::InitFailed {
                        message: e.to_string(),
                        stderr,
                    });
                }
                Err(_) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::InitFailed {
                        message: format!("no initialization response within {INIT_TIMEOUT:?}"),
                        stderr,
                    });
                }
            };
            match event.payload {
                Some(OutputPayload::InitializeConversationResponse(response)) => {
                    self.conversation_id = response.cascade_id.clone();
                    self.initial_history = response.history;
                    return Ok(());
                }
                Some(other) => {
                    tracing::debug!(
                        "Ignoring pre-init event while waiting for initialization: {other:?}"
                    );
                }
                None => {}
            }
        }
    }

    // -------------------------------------------------------------------
    // Turn loop
    // -------------------------------------------------------------------

    /// Returns the next agent event for the running turn, or `None` once
    /// the turn has finished and all events were drained.
    async fn next_turn_event(
        &mut self,
        turn: &mut TurnState,
    ) -> Result<Option<AgentEvent>, AntigravityError> {
        loop {
            if let Some(event) = turn.queue.pop_front() {
                return Ok(Some(event));
            }
            if turn.finished {
                return Ok(None);
            }
            let next = self.session.next_event();
            let event = match turn.deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, next).await {
                    Ok(result) => result?,
                    Err(_) => {
                        // The harness is still driving this turn. Without a
                        // halt, its remaining events (including its terminal
                        // trajectory-idle) would be consumed by the *next*
                        // turn and desync every turn after it.
                        self.halt_and_drain("recovering from a turn timeout").await;
                        let stderr = self.harness.stderr_tail().await;
                        return Err(AntigravityError::Timeout {
                            operation: turn.stall_diagnosis(&stderr),
                            timeout: turn.timeout.unwrap_or_default(),
                        });
                    }
                },
                None => next.await?,
            };
            let Some(event) = event else {
                let stderr = self.harness.stderr_tail().await;
                return Err(AntigravityError::ConnectionClosed {
                    message: "harness closed the WebSocket mid-turn".to_string(),
                    stderr,
                });
            };
            self.process_event(event, turn).await?;
        }
    }

    /// Marks the agent busy and, when a trigger delivered since the last
    /// client-driven turn, halts and drains the trigger-initiated harness
    /// turn. Returns the guard that flips the agent back to idle on drop.
    ///
    /// Invariant (shared with [`triggers::spawn_trigger_task`]): flipping
    /// the idle flag and consuming `trigger_fired` happen under
    /// `turn_sync`, the same lock a trigger task holds across its [idle
    /// re-check → set flag → send] window. This makes the fire decision
    /// and the turn begin mutually exclusive — without it, a trigger that
    /// already passed its idle check could set the flag and send *after*
    /// the `swap` below returned `false`, delivering its message into the
    /// turn we are about to start with nobody ever draining it.
    async fn begin_turn(&mut self) -> TurnGuard {
        let (guard, trigger_fired) = {
            let _sync = self.turn_sync.lock().await;
            let guard = TurnGuard::begin(&self.idle);
            let fired = self
                .trigger_fired
                .swap(false, std::sync::atomic::Ordering::SeqCst);
            (guard, fired)
        };
        // Drain outside the critical section: the agent is already marked
        // busy, so trigger tasks defer and nothing new can fire meanwhile.
        if trigger_fired {
            self.halt_and_drain("discarding a trigger-initiated turn")
                .await;
        }
        guard
    }

    /// Best-effort recovery when the harness may be driving (or have
    /// finished) a turn whose events nobody will consume — a turn that hit
    /// its per-turn timeout, or one started by an automated trigger.
    ///
    /// Sends a halt, then drains events into a throwaway state — answering
    /// protocol-required requests (confirmations, hooks) so the harness
    /// never deadlocks, discarding everything surfaced — until the stream
    /// stays silent for [`HALT_DRAIN_SILENCE`] or [`HALT_DRAIN_BUDGET`]
    /// runs out. Stale-turn errors (including the halt's own cancellation)
    /// are logged and swallowed; only transport health matters here.
    async fn halt_and_drain(&mut self, why: &str) {
        tracing::debug!("Halting and draining the harness turn ({why}).");
        if let Err(e) = self.session.send(&InputEvent::HaltRequest(true)).await {
            tracing::warn!("Failed to send halt request while {why}: {e}");
            return;
        }
        let mut state = TurnState::new(None, None);
        let deadline = tokio::time::Instant::now() + HALT_DRAIN_BUDGET;
        loop {
            let silence = tokio::time::Instant::now() + HALT_DRAIN_SILENCE;
            match tokio::time::timeout_at(silence.min(deadline), self.session.next_event()).await {
                Err(_) => {
                    // Silent for the grace window: drained (or nothing was
                    // running and the halt was a no-op). A budget overrun
                    // instead means the stale turn is still streaming.
                    if tokio::time::Instant::now() >= deadline {
                        tracing::warn!(
                            "Drain budget exhausted while {why}; the next turn may still \
                             observe stale events."
                        );
                    }
                    return;
                }
                Ok(Ok(Some(event))) => {
                    match self.process_event(event, &mut state).await {
                        Ok(()) => state.queue.clear(),
                        Err(AntigravityError::Turn(message)) => {
                            // The stale turn's cancellation or its own
                            // fatal error: expected terminals. Keep
                            // draining until silence in case more turns
                            // (a second trigger) are queued behind it.
                            tracing::debug!("Drained stale-turn terminal ({why}): {message}");
                        }
                        Err(e) => {
                            tracing::warn!("Error while draining stale turn ({why}): {e}");
                            return;
                        }
                    }
                }
                Ok(Ok(None)) => {
                    tracing::warn!("Harness closed the connection while {why}.");
                    return;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Transport error while draining stale turn ({why}): {e}");
                    return;
                }
            }
        }
    }
}

/// RAII marker for "a turn is being driven": construction flips the
/// agent's idle flag to busy, drop (turn completion, error, timeout, or a
/// dropped mid-turn stream) flips it back to idle, releasing any deferred
/// trigger deliveries.
pub(super) struct TurnGuard {
    idle: Arc<tokio::sync::watch::Sender<bool>>,
}

impl TurnGuard {
    fn begin(idle: &Arc<tokio::sync::watch::Sender<bool>>) -> Self {
        idle.send_replace(false);
        Self {
            idle: Arc::clone(idle),
        }
    }
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.idle.send_replace(true);
    }
}
