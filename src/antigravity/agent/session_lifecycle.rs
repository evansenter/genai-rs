//! Opening and closing the conversation: the initialization exchange and
//! the session-start hook during `spawn()`, and the session-end hooks
//! during `shutdown()`.

use std::time::Duration;

use crate::antigravity::protocol::{self, InputEvent, OutputPayload};
use crate::antigravity::turn::TurnState;
use crate::antigravity::{AntigravityAgent, AntigravityError};

/// How long the initialization may take, sending its message included,
/// until the `initializeConversationResponse` arrives (and, separately, how
/// long to wait for the session-start hook's request after it).
pub(super) const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `shutdown()` gives the session-end hooks, sending the request
/// to run them included.
pub(super) const SESSION_END_TIMEOUT: Duration = Duration::from_secs(30);

impl AntigravityAgent {
    /// Answers the session-start hook, which the harness calls right after
    /// initialization when it is enabled. Bounded by [`INIT_TIMEOUT`], and
    /// part of `spawn()`'s start-up as [`Self::initialize`] is: every
    /// failure is [`AntigravityError::InitFailed`] with the harness's
    /// stderr tail, and the error that caused it, if any, as its source.
    pub(in crate::antigravity) async fn start_session(&mut self) -> Result<(), AntigravityError> {
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
                Ok(Err(e)) => {
                    return Err(AntigravityError::init_failed_by(
                        "reading the harness's messages before the session-start hook failed",
                        self.harness.stderr_tail().await,
                        e,
                    ));
                }
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
            match tokio::time::timeout_at(deadline, self.process_event(event, &mut state)).await {
                Ok(Ok(())) if is_start => return Ok(()),
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let step = if is_start {
                        "answering the session-start hook failed"
                    } else {
                        "handling a harness message before the session-start hook failed"
                    };
                    return Err(AntigravityError::init_failed_by(
                        step,
                        self.harness.stderr_tail().await,
                        e,
                    ));
                }
                Err(_) => {
                    return Err(AntigravityError::init_failed(
                        format!("the session-start hook did not finish within {INIT_TIMEOUT:?}"),
                        self.harness.stderr_tail().await,
                    ));
                }
            }
        }
    }

    /// Runs the session-end hooks: asks the harness to end the session and
    /// answers its hook calls until it confirms. Best-effort and bounded by
    /// [`SESSION_END_TIMEOUT`]: shutdown proceeds either way.
    pub(super) async fn end_session(&mut self) {
        let deadline = tokio::time::Instant::now() + SESSION_END_TIMEOUT;
        let request = InputEvent::SessionEndRequest(true);
        match self.session.send_before(&request, deadline).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("Could not ask the harness to end the session: {e}");
                return;
            }
            Err(_) => {
                tracing::warn!(
                    "The harness did not take the session-end request within \
                     {SESSION_END_TIMEOUT:?}."
                );
                return;
            }
        }
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
            if event
                .payload
                .as_ref()
                .is_some_and(OutputPayload::is_session_end_response)
            {
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

    /// Sends the initialization and waits for its answer, all within
    /// [`INIT_TIMEOUT`]. Every failure is
    /// [`AntigravityError::InitFailed`] with the harness's stderr tail.
    pub(in crate::antigravity) async fn initialize(
        &mut self,
        init: protocol::InitializeConversationEvent,
    ) -> Result<(), AntigravityError> {
        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        match self.session.send_before(&init, deadline).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                return Err(AntigravityError::init_failed_by(
                    "sending the initialization message failed",
                    self.harness.stderr_tail().await,
                    e,
                ));
            }
            Err(_) => {
                return Err(AntigravityError::init_failed(
                    format!(
                        "the harness did not take the initialization message within \
                         {INIT_TIMEOUT:?}"
                    ),
                    self.harness.stderr_tail().await,
                ));
            }
        }
        loop {
            let event = match tokio::time::timeout_at(deadline, self.session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => {
                    return Err(AntigravityError::init_failed(
                        "harness closed the connection during initialization",
                        self.harness.stderr_tail().await,
                    ));
                }
                Ok(Err(e)) => {
                    return Err(AntigravityError::init_failed_by(
                        "reading the initialization response failed",
                        self.harness.stderr_tail().await,
                        e,
                    ));
                }
                Err(_) => {
                    return Err(AntigravityError::init_failed(
                        format!("no initialization response within {INIT_TIMEOUT:?}"),
                        self.harness.stderr_tail().await,
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
}

#[cfg(test)]
#[path = "session_lifecycle_tests.rs"]
mod tests;
