//! Opening and closing the conversation: the initialization exchange and
//! the session-start hook during `spawn()`, and the session-end hooks
//! during `shutdown()`.

use std::time::Duration;

use crate::antigravity::protocol::{self, InputEvent, OutputPayload};
use crate::antigravity::turn::TurnState;
use crate::antigravity::{AntigravityAgent, AntigravityError};

/// How long to wait for the `initializeConversationResponse` (and for the
/// session-start hook's request after it).
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long `shutdown()` waits for the session-end hooks to run.
const SESSION_END_TIMEOUT: Duration = Duration::from_secs(30);

impl AntigravityAgent {
    /// Answers the session-start hook, which the harness calls right after
    /// initialization when it is enabled. Bounded by [`INIT_TIMEOUT`].
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
                    return Err(AntigravityError::InitFailed {
                        message: "harness closed the connection before the session-start hook"
                            .to_string(),
                        stderr: self.harness.stderr_tail().await,
                    });
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(AntigravityError::InitFailed {
                        message: format!(
                            "the harness did not call the session-start hook within \
                             {INIT_TIMEOUT:?}"
                        ),
                        stderr: self.harness.stderr_tail().await,
                    });
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
    pub(super) async fn end_session(&mut self) {
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

    pub(in crate::antigravity) async fn initialize(
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
