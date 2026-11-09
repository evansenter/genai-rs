use super::hooks::LifecycleHooks;
use super::policies::PolicyEngine;
use super::process::HarnessProcess;
use super::prompt::{Prompt, user_input};
use super::protocol::{self, InputEvent, OutputEvent, OutputPayload, StepUpdate};
use super::session::{Session, SinkHandle};
use super::tools::ToolDispatcher;
use super::turn::TurnState;
use super::{
    AgentBuilder, AgentEvent, AgentEventStream, AntigravityError, ChatResponse, PostToolHook,
    PreToolHook, QuestionHook, triggers,
};
use std::sync::Arc;
use std::time::Duration;

/// How long to wait for the `initializeConversationResponse` (and for the
/// session-start hook's request after it).
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `shutdown()` waits for the session-end hooks to run.
const SESSION_END_TIMEOUT: Duration = Duration::from_secs(30);

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
    /// Shared with hook calls on their own threads (see
    /// `turn_requests::run_hook`).
    pub(super) policy_engine: Arc<PolicyEngine>,
    pub(super) pre_tool: Option<PreToolHook>,
    pub(super) questions: Option<QuestionHook>,
    pub(super) post_tool: Option<PostToolHook>,
    /// Session, turn, tool-error, compaction and stop hooks.
    pub(super) lifecycle: LifecycleHooks,
    pub(super) conversation_id: Option<String>,
    pub(super) initial_history: Vec<StepUpdate>,
    /// The OS command sandbox status the harness reported at init.
    pub(super) sandbox_status: Option<protocol::SandboxStatus>,
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
    /// `true` from the moment a turn starts until this client sees the
    /// harness end it (its `Finished`, its terminal error, or a timeout's
    /// halt-and-drain). A turn abandoned before that, by a dropped `chat`
    /// future or `send_streaming` stream, leaves it set, and the next
    /// [`begin_turn`](Self::begin_turn) halts and drains the harness's
    /// still-running turn before sending its own input.
    pub(super) turn_unfinished: bool,
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
/// Every harness from 0.1.10 through the pinned 0.1.23 answers a halt by
/// taking the trajectory to
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
/// `test_scripted_cancel_halts_a_turn_waiting_on_the_model`.
#[derive(Debug, Clone)]
pub struct CancelHandle {
    sink: SinkHandle,
}

impl CancelHandle {
    /// Sends a halt request for the current turn.
    ///
    /// # Errors
    ///
    /// [`AntigravityError::WebSocket`] when the halt request cannot be sent.
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

    /// Whether the harness can run `run_command` inside the OS sandbox, as
    /// it reported at start-up. With
    /// [`RunCommandOptions::with_sandbox`](super::RunCommandOptions::with_sandbox)
    /// on and `available` false, commands run unsandboxed (`spawn()` also
    /// logs a `warn!`). `None` when the harness reported nothing.
    #[must_use]
    pub fn sandbox_status(&self) -> Option<&protocol::SandboxStatus> {
        self.sandbox_status.as_ref()
    }

    /// The `google-antigravity` version of the running harness, read from
    /// the wheel install the binary sits in. `None` when the binary is not
    /// inside one (a hand-placed copy).
    ///
    /// `spawn()` only warns when this differs from
    /// [`SUPPORTED_HARNESS_VERSION`](super::SUPPORTED_HARNESS_VERSION). To
    /// refuse a mismatch, compare the two and shut the agent down:
    ///
    /// ```rust,no_run
    /// use genai_rs::antigravity::SUPPORTED_HARNESS_VERSION;
    ///
    /// # async fn run(agent: genai_rs::antigravity::AntigravityAgent) -> Result<(), Box<dyn std::error::Error>> {
    /// if agent.harness_version() != Some(SUPPORTED_HARNESS_VERSION) {
    ///     agent.shutdown().await?;
    ///     return Err("unsupported localharness version".into());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn harness_version(&self) -> Option<&str> {
        self.harness.version()
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
    ///
    /// # Errors
    ///
    /// [`AntigravityError::WebSocket`] when the halt request cannot be sent.
    pub async fn cancel(&self) -> Result<(), AntigravityError> {
        self.session.send(&InputEvent::HaltRequest(true)).await
    }

    /// Sends a message and drives the turn to completion.
    ///
    /// The message is a string, or a [`Prompt`] of text, [`Media`] and
    /// [`SlashCommand`](super::SlashCommand)s.
    ///
    /// If a [trigger](AgentBuilder::add_trigger) started a harness-side turn
    /// since the last call, that turn is halted and its events are discarded
    /// before this message is sent (see [`triggers`]).
    ///
    /// # Cancellation
    ///
    /// Dropping the future before it resolves (an outer
    /// `tokio::time::timeout`, a losing `select!` branch) abandons the turn.
    /// A custom tool it was running is aborted, as at the turn timeout, and
    /// a hook it was waiting on runs to completion with its answer
    /// discarded. The harness is still inside the turn, waiting on an answer
    /// that will not come or still working. The agent records that the turn
    /// never ended, and the next `chat` or
    /// [`send_streaming`](Self::send_streaming) halts it and discards its
    /// events before sending its own message (half a second when nothing is
    /// left running, five seconds at most), so that call gets its own
    /// reply. A turn that failed on this side before the harness ended it
    /// is halted the same way; one the harness failed itself (a pre-turn
    /// denial) is not. A fatal model error ends the whole session instead
    /// (see [`AntigravityError::Turn`]). To stop the harness at once
    /// instead, or to keep the turn's partial output, cancel through
    /// [`cancel_handle`](Self::cancel_handle) rather than dropping the
    /// future.
    ///
    /// # Errors
    ///
    /// [`AntigravityError::Config`] for an empty [`Prompt`];
    /// [`AntigravityError::Turn`] when the turn fails (a model backend error,
    /// a cancellation by the harness, or a pre-turn denial);
    /// [`AntigravityError::Timeout`] past the turn timeout;
    /// [`AntigravityError::ConnectionClosed`] or
    /// [`AntigravityError::WebSocket`] when the connection to the harness
    /// fails; and [`AntigravityError::Protocol`] when the turn ends without a
    /// `Finished` event.
    ///
    /// [`Media`]: super::Media
    pub async fn chat(
        &mut self,
        prompt: impl Into<Prompt>,
    ) -> Result<ChatResponse, AntigravityError> {
        let input = user_input(prompt)?;
        // Mark busy before sending so a trigger cannot slip in between.
        let guard = self.begin_turn().await;
        self.session.send(&input).await?;
        let mut turn = TurnState::new(self.turn_timeout, Some(guard));
        loop {
            match self.next_turn_event(&mut turn).await? {
                Some(AgentEvent::Finished(response)) => return Ok(*response),
                Some(_) => {}
                None => {
                    return Err(AntigravityError::protocol(
                        "turn ended without a Finished event".to_string(),
                    ));
                }
            }
        }
    }

    /// Sends a message and returns a stream of [`AgentEvent`]s for the
    /// turn. The stream ends after [`AgentEvent::Finished`]. The message is
    /// a string or a [`Prompt`], as for [`chat`](Self::chat).
    ///
    /// If a [trigger](AgentBuilder::add_trigger) started a harness-side turn
    /// since the last call, that turn is halted and its events are discarded
    /// before this message is sent (see [`triggers`]).
    ///
    /// # Cancellation
    ///
    /// Dropping the stream before [`AgentEvent::Finished`] abandons the
    /// turn, as dropping a [`chat`](Self::chat) future does: the next `chat`
    /// or `send_streaming` halts the harness's still-running turn and
    /// discards its events before sending its own message. To end the turn
    /// early and keep reading its partial output, use
    /// [`cancel_handle`](Self::cancel_handle) and read the stream to its
    /// end instead.
    ///
    /// # Errors
    ///
    /// [`AntigravityError::Config`] for an empty [`Prompt`], and
    /// [`AntigravityError::WebSocket`] when the message cannot be sent. The
    /// turn's own failures, as listed for [`chat`](Self::chat), arrive as the
    /// stream's items.
    pub async fn send_streaming(
        &mut self,
        prompt: impl Into<Prompt>,
    ) -> Result<AgentEventStream<'_>, AntigravityError> {
        let input = user_input(prompt)?;
        // Mark busy before sending so a trigger cannot slip in between.
        // The guard moves into the stream's turn state: dropping the
        // stream mid-turn marks the agent idle again.
        let guard = self.begin_turn().await;
        self.session.send(&input).await?;
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
    ///
    /// # Errors
    ///
    /// [`AntigravityError::HarnessExited`] when the harness exits with a
    /// failure status, which usually means it did not save the trajectory.
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
        if self.lifecycle.session_end.is_some() {
            self.end_session().await;
        }
        self.session.close().await;
        self.harness.shutdown().await
    }

    /// Waits for the next turn the agent starts on its own — a
    /// [trigger](AgentBuilder::add_trigger) delivery — for up to `wait`,
    /// drives it to completion and returns its response: the consumer for
    /// trigger-turn output. `Ok(None)` when no turn started in time.
    ///
    /// Triggers keep delivering while this waits (the agent counts as
    /// idle). Once the turn's first event arrives the agent is busy, so
    /// further deliveries wait for it to finish; the turn timeout applies
    /// from then. A turn that is already running or finished when this is
    /// called is consumed from where it stands.
    ///
    /// The next `chat`/`send_streaming` still makes sure no other trigger
    /// turn is running before it sends (a halt, which does nothing when no
    /// turn runs, and up to half a second of draining).
    ///
    /// # Errors
    ///
    /// [`AntigravityError::ConnectionClosed`] when the harness closes the
    /// connection while this waits, and a failed turn's errors, as for
    /// [`chat`](Self::chat).
    ///
    /// ```rust,no_run
    /// # use std::time::Duration;
    /// # async fn run(mut agent: genai_rs::antigravity::AntigravityAgent) -> Result<(), genai_rs::antigravity::AntigravityError> {
    /// // React to whatever the triggers start, between user turns.
    /// while let Some(response) = agent.next_trigger_turn(Duration::from_secs(60)).await? {
    ///     println!("trigger turn: {}", response.text());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn next_trigger_turn(
        &mut self,
        wait: Duration,
    ) -> Result<Option<ChatResponse>, AntigravityError> {
        let first = match tokio::time::timeout(wait, self.session.next_event()).await {
            Err(_) => return Ok(None),
            Ok(Ok(Some(event))) => event,
            Ok(Ok(None)) => {
                let stderr = self.harness.stderr_tail().await;
                return Err(AntigravityError::ConnectionClosed {
                    message: "harness closed the WebSocket while waiting for a trigger turn"
                        .to_string(),
                    stderr,
                });
            }
            Ok(Err(e)) => return Err(e),
        };
        // Busy from here: later deliveries wait for this turn. The flag is
        // left set, so the next chat still checks for a turn a racing
        // delivery may have started.
        let guard = {
            let _sync = self.turn_sync.lock().await;
            TurnGuard::begin(&self.idle)
        };
        self.turn_unfinished = true;
        let mut turn = TurnState::new(self.turn_timeout, Some(guard));
        self.process_turn_event(first, &mut turn).await?;
        loop {
            match self.next_turn_event(&mut turn).await? {
                Some(AgentEvent::Finished(response)) => return Ok(Some(*response)),
                Some(_) => {}
                None => {
                    return Err(AntigravityError::protocol(
                        "trigger turn ended without a Finished event".to_string(),
                    ));
                }
            }
        }
    }

    /// The delivery function trigger tasks share: flag the agent, then send
    /// the message through the out-of-band sink.
    pub(super) fn trigger_delivery(&self) -> triggers::Deliver {
        let sink = self.session.sink_handle();
        let fired = Arc::clone(&self.trigger_fired);
        Arc::new(move |message| {
            let sink = sink.clone();
            let fired = Arc::clone(&fired);
            Box::pin(async move {
                // Flag before sending so the next chat can never miss an
                // in-flight delivery (a false positive on send failure only
                // costs a no-op drain).
                fired.store(true, std::sync::atomic::Ordering::SeqCst);
                sink.send(&InputEvent::AutomatedTrigger(message)).await
            })
        })
    }

    /// Answers the session-start hook, which the harness calls right after
    /// initialization when it is enabled. Bounded by [`INIT_TIMEOUT`].
    pub(super) async fn start_session(&mut self) -> Result<(), AntigravityError> {
        if self.lifecycle.session_start.is_none() {
            return Ok(());
        }
        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        let mut state = TurnState::new(None, None);
        loop {
            let event = match tokio::time::timeout_at(deadline, self.session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => {
                    return Err(AntigravityError::init_failed(
                        "harness closed the connection before the session-start hook",
                        self.harness.stderr_tail().await,
                    ));
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(AntigravityError::init_failed(
                        format!(
                            "the harness did not call the session-start hook within \
                             {INIT_TIMEOUT:?}"
                        ),
                        self.harness.stderr_tail().await,
                    ));
                }
            };
            let is_start = matches!(
                &event.payload,
                Some(OutputPayload::CallHookRequest(request))
                    if request.hook_type == Some(protocol::LifecycleHook::OnSessionStart)
            );
            tokio::time::timeout_at(deadline, self.process_event(event, &mut state))
                .await
                .map_err(|_| AntigravityError::Timeout {
                    operation: "the session-start hook".to_string(),
                    timeout: INIT_TIMEOUT,
                })??;
            if is_start {
                return Ok(());
            }
        }
    }

    /// Runs the session-end hooks: asks the harness to end the session and
    /// answers its hook calls until it confirms. Best-effort and bounded by
    /// [`SESSION_END_TIMEOUT`]: shutdown proceeds either way.
    async fn end_session(&mut self) {
        if let Err(e) = self
            .session
            .send(&InputEvent::SessionEndRequest(true))
            .await
        {
            tracing::warn!("Could not ask the harness to end the session: {e}");
            return;
        }
        let deadline = tokio::time::Instant::now() + SESSION_END_TIMEOUT;
        let mut state = TurnState::new(None, None);
        loop {
            let event = match tokio::time::timeout_at(deadline, self.session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) | Ok(Err(_)) => return,
                Err(_) => {
                    tracing::warn!(
                        "The session-end hooks did not finish within {SESSION_END_TIMEOUT:?}."
                    );
                    return;
                }
            };
            if matches!(event.payload, Some(OutputPayload::SessionEndResponse(_))) {
                return;
            }
            match tokio::time::timeout_at(deadline, self.process_event(event, &mut state)).await {
                Ok(Ok(())) => state.queue.clear(),
                Ok(Err(e)) => tracing::debug!("Ignoring a late event during session end: {e}"),
                Err(_) => {
                    tracing::warn!(
                        "The session-end hook did not finish within {SESSION_END_TIMEOUT:?}."
                    );
                    return;
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Init
    // -------------------------------------------------------------------

    pub(super) async fn initialize(
        &mut self,
        init: protocol::InitializeConversationEvent,
    ) -> Result<(), AntigravityError> {
        self.session.send_raw(&init).await?;

        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        loop {
            let event = match tokio::time::timeout_at(deadline, self.session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::init_failed(
                        "harness closed the connection during initialization",
                        stderr,
                    ));
                }
                Ok(Err(e)) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::init_failed_by(
                        "reading the initialization response failed",
                        stderr,
                        e,
                    ));
                }
                Err(_) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::init_failed(
                        format!("no initialization response within {INIT_TIMEOUT:?}"),
                        stderr,
                    ));
                }
            };
            match event.payload {
                Some(OutputPayload::InitializeConversationResponse(response)) => {
                    self.conversation_id.clone_from(&response.cascade_id);
                    self.initial_history = response.history;
                    self.sandbox_status = response.sandbox_status;
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
            // The deadline covers handling the event as well as waiting for
            // it: custom tools and hooks run inside `process_event`, and a
            // hung one would otherwise hold the turn past its budget.
            let deadline = turn.deadline;
            let step = self.receive_and_process(turn);
            match deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, step).await {
                    Ok(result) => result?,
                    Err(_) => {
                        // The harness is still driving this turn. Without a
                        // halt, its remaining events (including its terminal
                        // trajectory-idle) would be consumed by the *next*
                        // turn and desync every turn after it. A drain that
                        // ran out of budget leaves the turn unfinished, so
                        // the next turn halts and drains again.
                        let drained = self.halt_and_drain("recovering from a turn timeout").await;
                        self.turn_unfinished = !drained;
                        let stderr = self.harness.stderr_tail().await;
                        return Err(AntigravityError::Timeout {
                            operation: turn.stall_diagnosis(&stderr, self.harness.version()),
                            timeout: turn.timeout.unwrap_or_default(),
                        });
                    }
                },
                None => step.await?,
            }
        }
    }

    /// Waits for the next harness event and handles it.
    async fn receive_and_process(&mut self, turn: &mut TurnState) -> Result<(), AntigravityError> {
        let Some(event) = self.session.next_event().await? else {
            let stderr = self.harness.stderr_tail().await;
            return Err(AntigravityError::ConnectionClosed {
                message: "harness closed the WebSocket mid-turn".to_string(),
                stderr,
            });
        };
        self.process_turn_event(event, turn).await
    }

    /// Handles one event of a turn this client drives, and clears
    /// `turn_unfinished` once the harness has ended the turn: completed, or
    /// failed by its main trajectory's terminal state (idle with an error,
    /// or cancelled, as on a pre-turn denial), the only way a trajectory
    /// update fails. Any other error may leave the harness mid-turn, so the
    /// next turn halts it.
    async fn process_turn_event(
        &mut self,
        event: OutputEvent,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let trajectory_update =
            matches!(event.payload, Some(OutputPayload::TrajectoryStateUpdate(_)));
        let result = self.process_event(event, turn).await;
        let harness_failed_it =
            trajectory_update && matches!(result, Err(AntigravityError::Turn { .. }));
        if turn.finished || harness_failed_it {
            self.turn_unfinished = false;
        }
        result
    }

    /// Marks the agent busy and, when a trigger delivered since the last
    /// client-driven turn or the last turn never ended (it was abandoned
    /// mid-flight, or failed on this side), halts and drains the harness's
    /// turn. Returns the guard that flips the agent back to idle on drop,
    /// with the new turn marked unfinished until its end is seen.
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
        // The unfinished flag stays set across the drain, so a turn
        // abandoned while draining is drained again by the next one.
        if self.turn_unfinished {
            self.halt_and_drain("discarding an abandoned or failed turn")
                .await;
        } else if trigger_fired {
            self.halt_and_drain("discarding a trigger-initiated turn")
                .await;
        }
        self.turn_unfinished = true;
        guard
    }

    /// Best-effort recovery when the harness may be driving (or have
    /// finished) a turn whose events nobody will consume — a turn that hit
    /// its per-turn timeout, one its caller abandoned, or one started by an
    /// automated trigger.
    ///
    /// Sends a halt, then drains events into a throwaway state — answering
    /// protocol-required requests (confirmations, hooks) so the harness
    /// never deadlocks, discarding everything surfaced — until the stream
    /// stays silent for [`HALT_DRAIN_SILENCE`] or [`HALT_DRAIN_BUDGET`]
    /// runs out. Stale-turn errors (including the halt's own cancellation)
    /// are logged and swallowed; only transport health matters here.
    ///
    /// Returns whether the stream went silent: `false` when the budget ran
    /// out or the transport failed first.
    async fn halt_and_drain(&mut self, why: &str) -> bool {
        // tests/antigravity_scripted/cancel.rs counts halts by this line.
        tracing::debug!("Halting and draining the harness turn ({why}).");
        if let Err(e) = self.session.send(&InputEvent::HaltRequest(true)).await {
            tracing::warn!("Failed to send halt request while {why}: {e}");
            return false;
        }
        let mut state = TurnState::draining();
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
                        return false;
                    }
                    return true;
                }
                Ok(Ok(Some(event))) => {
                    // Bounded like the wait: answering a request can run a
                    // hook, and a hung one must not hold the drain open.
                    let processed =
                        tokio::time::timeout_at(deadline, self.process_event(event, &mut state))
                            .await;
                    let Ok(processed) = processed else {
                        tracing::warn!(
                            "Drain budget exhausted while answering a harness request ({why}); \
                             the next turn may still observe stale events."
                        );
                        return false;
                    };
                    match processed {
                        Ok(()) => state.queue.clear(),
                        Err(AntigravityError::Turn { message, .. }) => {
                            // The stale turn's cancellation or its own
                            // fatal error: expected terminals. Keep
                            // draining until silence in case more turns
                            // (a second trigger) are queued behind it.
                            tracing::debug!("Drained stale-turn terminal ({why}): {message}");
                        }
                        Err(e) => {
                            tracing::warn!("Error while draining stale turn ({why}): {e}");
                            return false;
                        }
                    }
                }
                Ok(Ok(None)) => {
                    tracing::warn!("Harness closed the connection while {why}.");
                    return false;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Transport error while draining stale turn ({why}): {e}");
                    return false;
                }
            }
        }
    }
}

/// RAII marker for "a turn is being driven": construction flips the
/// agent's idle flag to busy, drop (turn completion, error, timeout, or a
/// dropped mid-turn `chat` future or stream) flips it back to idle,
/// releasing any deferred trigger deliveries. An abandoned turn may still
/// be running on the harness: `turn_unfinished` tells the next
/// [`begin_turn`](AntigravityAgent::begin_turn) to halt it.
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
