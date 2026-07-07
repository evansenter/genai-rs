//! Client-side triggers: interval timers that inject
//! [`InputEvent::AutomatedTrigger`](super::protocol::InputEvent) messages
//! into the conversation outside a user turn.
//!
//! Register triggers with
//! [`AgentBuilder::add_trigger`](super::AgentBuilder::add_trigger); the
//! agent spawns one timer task per trigger after a successful `spawn()`,
//! mirroring the reference SDK's `TriggerRunner` (independent tasks, no
//! ordering guarantees, a failing trigger never crashes the session).
//!
//! # Delivery semantics
//!
//! - The first firing happens after the first `interval` elapses (not
//!   immediately) — same as the reference SDK's `every()` helper.
//! - A firing is **delivered only while the agent is idle** (no turn is
//!   currently being driven by `chat`/`send_streaming`). If a firing comes
//!   due while a turn is in flight, delivery is *deferred* until the agent
//!   becomes idle again. The reference SDK's `TriggerDelivery` enum names
//!   this mode `WAIT_IDLE` (its default `send_immediately` mode does not
//!   suit this crate's sequential turn loop, where nothing reads the
//!   WebSocket between turns of a busy agent).
//! - **Overlap suppression**: intervals missed while busy collapse into a
//!   single delivery — the timer restarts only after the deferred delivery
//!   lands, so a long turn produces one trigger message, not a backlog.
//! - Idleness is tracked client-side. A trigger delivered while idle starts
//!   a harness-side turn that runs unobserved: **its output is not
//!   surfaced** (no consumer reads the WebSocket between client-driven
//!   turns). The next `chat`/`send_streaming` call halts that turn if it is
//!   still running and discards its events before sending the new input, so
//!   trigger turns can never desync or leak into a user turn's response.
//!   The trigger's effects on conversation history (and any tool calls the
//!   harness completed before the halt) persist. Surfacing trigger-turn
//!   output through a dedicated consumer is a follow-up (see
//!   `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`).
//!
//! # Lifecycle
//!
//! Trigger tasks stop cleanly — no zombie timers:
//!
//! - [`AntigravityAgent::shutdown`](super::AntigravityAgent::shutdown)
//!   aborts them before closing the WebSocket;
//! - dropping the agent aborts them (the task handles abort on drop);
//! - a failed send (session gone) or a closed idle channel (agent dropped)
//!   ends the task on its own.

use std::future::Future;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::AntigravityError;

/// Configuration for a recurring client-side trigger.
///
/// Register with [`AgentBuilder::add_trigger`](super::AgentBuilder::add_trigger).
/// See the [module docs](self) for delivery semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TriggerConfig {
    /// Message injected into the conversation on each firing.
    pub message: String,
    /// Interval between firings. Must be non-zero (validated at
    /// `spawn()` time).
    pub interval: Duration,
}

impl TriggerConfig {
    /// Creates a trigger that injects `message` every `interval`.
    #[must_use]
    pub fn new(message: impl Into<String>, interval: Duration) -> Self {
        Self {
            message: message.into(),
            interval,
        }
    }
}

/// Owns the spawned trigger tasks; aborts them all on drop so a dropped or
/// shut-down agent can never leak timer tasks.
#[derive(Debug, Default)]
pub(crate) struct TriggerTasks {
    handles: Vec<JoinHandle<()>>,
}

impl TriggerTasks {
    pub(crate) fn push(&mut self, handle: JoinHandle<()>) {
        self.handles.push(handle);
    }

    pub(crate) fn abort_all(&mut self) {
        for handle in self.handles.drain(..) {
            handle.abort();
        }
    }
}

impl Drop for TriggerTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

/// Spawns the timer task for one trigger.
///
/// `idle` reports whether the agent is between turns (`true` = idle); the
/// sender side lives on the agent. `send` delivers the trigger message —
/// in production it wraps the session's shared sink handle, in tests a
/// channel.
///
/// The task ends on its own when the idle channel closes (agent dropped)
/// or a send fails (session closed); otherwise it runs until aborted.
pub(crate) fn spawn_trigger_task<F, Fut>(
    config: TriggerConfig,
    mut idle: watch::Receiver<bool>,
    send: F,
) -> JoinHandle<()>
where
    F: Fn(String) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), AntigravityError>> + Send,
{
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(config.interval).await;
            // Deliver only while idle; a firing that comes due mid-turn is
            // deferred until the turn ends. Missed intervals collapse into
            // this single deferred delivery (overlap suppression).
            if idle.wait_for(|is_idle| *is_idle).await.is_err() {
                tracing::debug!("Trigger task exiting: agent dropped");
                return;
            }
            if let Err(e) = send(config.message.clone()).await {
                tracing::warn!("Trigger delivery failed; stopping trigger task: {e}");
                return;
            }
            tracing::debug!(
                interval = ?config.interval,
                "Delivered automated trigger message"
            );
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_trigger_config_construction() {
        let trigger = TriggerConfig::new("check the queue", Duration::from_secs(60));
        assert_eq!(trigger.message, "check the queue");
        assert_eq!(trigger.interval, Duration::from_secs(60));
    }

    /// Spawns a trigger task whose sends are observable through an mpsc
    /// channel.
    fn observable_trigger(
        config: TriggerConfig,
        idle: watch::Receiver<bool>,
    ) -> (JoinHandle<()>, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = spawn_trigger_task(config, idle, move |message| {
            let tx = tx.clone();
            async move {
                tx.send(message)
                    .map_err(|e| AntigravityError::WebSocket(e.to_string()))
            }
        });
        (handle, rx)
    }

    #[tokio::test(start_paused = true)]
    async fn test_trigger_fires_repeatedly_when_idle() {
        let (_idle_tx, idle_rx) = watch::channel(true);
        let (handle, mut rx) =
            observable_trigger(TriggerConfig::new("ping", Duration::from_secs(60)), idle_rx);

        // Paused-clock auto-advance: each recv drives one interval.
        assert_eq!(rx.recv().await.as_deref(), Some("ping"));
        assert_eq!(rx.recv().await.as_deref(), Some("ping"));
        assert_eq!(rx.recv().await.as_deref(), Some("ping"));
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn test_trigger_defers_while_busy_and_collapses_missed_intervals() {
        let (idle_tx, idle_rx) = watch::channel(false); // busy
        let (handle, mut rx) = observable_trigger(
            TriggerConfig::new("check", Duration::from_millis(10)),
            idle_rx,
        );

        // Ten intervals elapse while the agent is busy: nothing delivered.
        tokio::time::sleep(Duration::from_millis(105)).await;
        assert!(rx.try_recv().is_err(), "must not deliver while busy");

        // Turn ends: exactly one deferred delivery for all missed intervals.
        idle_tx.send_replace(true);
        assert_eq!(rx.recv().await.as_deref(), Some("check"));
        // Before the next interval elapses, nothing else is queued.
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(
            rx.try_recv().is_err(),
            "missed intervals must collapse into one delivery"
        );
        handle.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn test_trigger_task_exits_when_idle_channel_closes() {
        let (idle_tx, idle_rx) = watch::channel(false);
        let (handle, _rx) =
            observable_trigger(TriggerConfig::new("x", Duration::from_millis(1)), idle_rx);
        // Dropping the sender (the agent) ends the task without an abort.
        drop(idle_tx);
        handle.await.expect("task exits cleanly, not aborted");
    }

    #[tokio::test(start_paused = true)]
    async fn test_trigger_task_exits_when_send_fails() {
        let (_idle_tx, idle_rx) = watch::channel(true);
        let handle = spawn_trigger_task(
            TriggerConfig::new("x", Duration::from_millis(1)),
            idle_rx,
            |_message| async { Err(AntigravityError::WebSocket("closed".to_string())) },
        );
        handle.await.expect("task exits cleanly after send failure");
    }

    #[tokio::test(start_paused = true)]
    async fn test_trigger_tasks_abort_on_drop() {
        let (_idle_tx, idle_rx) = watch::channel(false); // never idle: task runs forever
        let (handle, _rx) =
            observable_trigger(TriggerConfig::new("x", Duration::from_secs(1)), idle_rx);
        let abort_handle = handle.abort_handle();

        let mut tasks = TriggerTasks::default();
        tasks.push(handle);
        drop(tasks);

        while !abort_handle.is_finished() {
            tokio::task::yield_now().await;
        }
    }
}
