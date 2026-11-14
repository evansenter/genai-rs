use super::SUPPORTED_HARNESS_VERSION;
use super::process;
use std::time::Duration;
use thiserror::Error;

// =============================================================================
// Errors
// =============================================================================

/// The cause an error carries as its `source()`.
pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Errors from the Antigravity harness client.
///
/// The variants for a harness that failed to start or went away
/// ([`HandshakeFailed`](Self::HandshakeFailed),
/// [`InitFailed`](Self::InitFailed),
/// [`ConnectionClosed`](Self::ConnectionClosed) and
/// [`HarnessExited`](Self::HarnessExited)) carry the tail of the harness's
/// stderr — that is where the harness reports actionable problems (e.g.
/// `no text model configuration provided`).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AntigravityError {
    /// The `localharness` binary could not be found: the path
    /// [`with_harness_path`](super::AgentBuilder::with_harness_path) or
    /// `ANTIGRAVITY_HARNESS_PATH` named is not a file (the only location
    /// listed), or the search found none.
    #[error(
        "localharness binary not found. Searched: {searched:?}. {}",
        not_found_advice(searched)
    )]
    HarnessNotFound {
        /// The locations that were searched, in order.
        searched: Vec<String>,
    },
    /// The harness could not be started, or the stdio handshake with it
    /// failed: `spawn()` reports a binary that would not run, and a reply
    /// that did not arrive in time or could not be decoded, here.
    #[error("harness handshake failed: {message}\nharness stderr:\n{stderr}")]
    HandshakeFailed {
        /// What was being done, or what went wrong when nothing failed
        /// with an error of its own (no reply in time).
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
        /// The error that failed the step, when one did: the I/O error
        /// starting the binary or exchanging the handshake, or the error
        /// decoding the harness's reply. Also returned by
        /// [`source()`](std::error::Error::source), and not repeated in
        /// this error's `Display`.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },
    /// The conversation could not be initialized. After the handshake,
    /// `spawn()` reports every failure here: connecting to the harness,
    /// the initialization exchange, and the session-start hook. A harness
    /// that dies after the handshake, while `spawn()` waits on it, fails it
    /// with this variant, not [`ConnectionClosed`](Self::ConnectionClosed).
    #[error("conversation initialization failed: {message}\nharness stderr:\n{stderr}")]
    InitFailed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
        /// The error that failed the step, when one did: the
        /// [`WebSocket`](Self::WebSocket) error of a connect that never
        /// succeeded or of a send that failed, the error reading the
        /// harness's messages, or, while `spawn()` waits for the
        /// session-start hook
        /// ([`on_session_start`](super::AgentBuilder::on_session_start)),
        /// the error handling a harness message: the hook's answer that
        /// failed to send, or the [`Turn`](Self::Turn) error of a message
        /// that fails a turn (a cancelled trajectory, say). Also returned
        /// by [`source()`](std::error::Error::source), and not repeated in
        /// this error's `Display`.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },
    /// The harness closed the connection unexpectedly (it likely crashed),
    /// after `spawn()` returned: mid-turn, while
    /// [`next_trigger_turn`](super::AntigravityAgent::next_trigger_turn)
    /// waits, or, without sending its message, on a call that starts or
    /// waits for a turn ([`chat`](super::AntigravityAgent::chat),
    /// [`send_streaming`](super::AntigravityAgent::send_streaming),
    /// `next_trigger_turn`) once the session has ended: after a turn whose
    /// failure ended it ([`TurnFailure::ends_session`]), a trigger's turn
    /// that the call halted and discarded included, or after the harness
    /// closed the connection while a call discarded an earlier turn. A
    /// trigger's [`TriggerContext::send`](super::TriggerContext::send)
    /// fails with it, with an empty `stderr`, once the agent is gone.
    #[error("harness connection closed: {message}\nharness stderr:\n{stderr}")]
    ConnectionClosed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// The harness exited with a failure status when asked to shut down
    /// (`shutdown()`), which usually means it did not save the trajectory.
    #[error("harness exited with {}\nharness stderr:\n{stderr}", exit_description(*code))]
    HarnessExited {
        /// The exit code, when the process exited normally (`None` when a
        /// signal ended it).
        code: Option<i32>,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// A custom tool could not be dispatched.
    #[error("tool dispatch failed for '{name}': {message}")]
    ToolDispatch {
        /// The tool name.
        name: String,
        /// What went wrong.
        message: String,
    },
    /// The agent configuration is invalid.
    #[error("invalid agent configuration: {0}")]
    Config(String),
    /// An operation exceeded its time budget.
    #[error("{operation} timed out after {timeout:?}")]
    Timeout {
        /// The operation that timed out.
        operation: String,
        /// The configured budget.
        timeout: Duration,
    },
    /// The turn failed. `kind` says why, and
    /// [`TurnFailure::ends_session`] whether the agent can run another
    /// turn: match on it, not on `message`.
    ///
    /// When it cannot ([`TurnFailure::ModelBackend`] and
    /// [`TurnFailure::Trajectory`], on harness 0.1.23), the harness ends
    /// its run and closes its WebSocket, and the agent's next turn (a
    /// `chat`, `send_streaming` or `next_trigger_turn`) fails with
    /// [`ConnectionClosed`](Self::ConnectionClosed) without sending its
    /// message. A trigger's turn that fails so, unconsumed, never comes
    /// back as this error: the `chat` or `send_streaming` that halts and
    /// discards it fails with `ConnectionClosed` instead, naming the
    /// failure. Spawn a new agent after one.
    #[error("agent turn failed ({kind}): {message}")]
    Turn {
        /// Why the turn failed.
        kind: TurnFailure,
        /// The harness's message, or the pre-turn hook's reason for a
        /// [`TurnFailure::Denied`] turn.
        message: String,
    },
    /// WebSocket transport error: a read or send on the connection to the
    /// harness failed. A connect, read or send that fails during `spawn()`
    /// (the session-start hook's included) is
    /// [`InitFailed`](Self::InitFailed) instead, with this error as its
    /// source, beside the harness's `stderr`.
    #[error("websocket error: {message}")]
    WebSocket {
        /// What was being done: connecting, reading or sending.
        message: String,
        /// The transport's error, also returned by
        /// [`source()`](std::error::Error::source), and not repeated in
        /// this error's `Display`.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    /// The harness broke the protocol: it sent a message this client could
    /// not parse, or ended a turn without its final event. A message this
    /// client could not encode for the harness is reported here too.
    #[error("protocol error: {message}")]
    Protocol {
        /// What went wrong.
        message: String,
        /// The parse or encode error, when one caused it, also returned by
        /// [`source()`](std::error::Error::source), and not repeated in
        /// this error's `Display`.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },
    /// A local I/O operation failed: reading a file
    /// ([`Media::from_file`](super::Media::from_file)), or harness
    /// discovery cut short by the runtime shutting down.
    #[error("I/O error: {message}")]
    Io {
        /// What was being done, naming the path when there is one.
        message: String,
        /// The I/O error, also returned by
        /// [`source()`](std::error::Error::source), and not repeated in
        /// this error's `Display`.
        #[source]
        source: std::io::Error,
    },
}

/// Why a turn failed: the `kind` of [`AntigravityError::Turn`].
///
/// [`ends_session`](Self::ends_session) says whether the agent can run
/// another turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TurnFailure {
    /// The model backend failed a model call, and the turn with it: with a
    /// status the harness does not retry (400, 401 or 403), or with one
    /// whose retries ran out (a 503 under
    /// [`RetryConfig::with_max_api_retries(0)`](crate::antigravity::RetryConfig::with_max_api_retries),
    /// say) on a call of the agent's own rather than a subagent's. The
    /// harness ends the session too ([`ends_session`](Self::ends_session)).
    ModelBackend {
        /// The HTTP status the model backend answered with.
        status: u32,
    },
    /// The pre-turn hook
    /// ([`on_pre_turn`](crate::antigravity::AgentBuilder::on_pre_turn))
    /// refused the turn; the error's `message` is its reason. No model call
    /// was made, and the agent stays usable.
    Denied,
    /// The harness cancelled the turn of its own accord, other than for a
    /// pre-turn denial ([`Denied`](Self::Denied)). A turn stopped by
    /// [`CancelHandle::cancel`](crate::antigravity::CancelHandle::cancel)
    /// resolves normally instead, with the output it had. Not taken to end
    /// the session.
    Cancelled,
    /// The harness ended the turn with an error of its own, not the model
    /// backend's: an MCP server that failed to load, say. On harness
    /// 0.1.23 its run ends with the turn
    /// ([`ends_session`](Self::ends_session)).
    Trajectory,
    /// A hook could not run: its thread could not start, or ended without
    /// a result. The harness may still be mid-turn; the agent's next turn
    /// halts it first.
    Hook,
}

impl TurnFailure {
    /// Whether the harness ended the session with the turn, so the agent's
    /// next turn (a `chat`, `send_streaming` or `next_trigger_turn`) fails
    /// with [`ConnectionClosed`](AntigravityError::ConnectionClosed),
    /// without sending its message, and a new agent must be spawned. True for
    /// [`ModelBackend`](Self::ModelBackend) and
    /// [`Trajectory`](Self::Trajectory), whose errors end the harness's run
    /// on harness 0.1.23.
    #[must_use]
    pub const fn ends_session(&self) -> bool {
        matches!(self, Self::ModelBackend { .. } | Self::Trajectory)
    }
}

impl std::fmt::Display for TurnFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelBackend { status } => write!(f, "model backend error, HTTP {status}"),
            Self::Denied => f.write_str("denied by the pre-turn hook"),
            Self::Cancelled => f.write_str("cancelled by the harness"),
            Self::Trajectory => f.write_str("the harness ended it with an error"),
            Self::Hook => f.write_str("a hook could not run"),
        }
    }
}

/// What fixes a [`AntigravityError::HarnessNotFound`]: the path the caller
/// named, when discovery stopped at it (a fix elsewhere would not be read),
/// or else installing the harness.
fn not_found_advice(searched: &[String]) -> String {
    let named = |label: &str| matches!(searched, [only] if only.starts_with(label));
    if named(process::EXPLICIT_PATH_LABEL) {
        "Point `with_harness_path` at the localharness binary.".to_string()
    } else if named(&format!("{}=", process::HARNESS_PATH_ENV)) {
        format!(
            "Point {0} at the localharness binary, or unset it to search for one.",
            process::HARNESS_PATH_ENV
        )
    } else {
        format!(
            "Install it with `pip install google-antigravity=={SUPPORTED_HARNESS_VERSION}` \
             or set {}.",
            process::HARNESS_PATH_ENV
        )
    }
}

fn exit_description(code: Option<i32>) -> String {
    code.map_or_else(|| "a signal".to_string(), |code| format!("code {code}"))
}

impl AntigravityError {
    /// A [`Protocol`](Self::Protocol) error with no underlying cause.
    pub(crate) fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol {
            message: message.into(),
            source: None,
        }
    }

    /// A [`Protocol`](Self::Protocol) error caused by `source`, a parse or
    /// encode error; `message` says what was being done.
    pub(crate) fn protocol_from(
        message: &str,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Protocol {
            message: message.to_string(),
            source: Some(Box::new(source)),
        }
    }

    /// A [`Turn`](Self::Turn) error of `kind`.
    pub(crate) fn turn(kind: TurnFailure, message: impl Into<String>) -> Self {
        Self::Turn {
            kind,
            message: message.into(),
        }
    }

    /// A [`HandshakeFailed`](Self::HandshakeFailed) error, caused by
    /// `source` when there is one; `message` says what was being done.
    pub(crate) fn handshake_failed(
        message: String,
        stderr: String,
        source: Option<BoxError>,
    ) -> Self {
        Self::HandshakeFailed {
            message,
            stderr,
            source,
        }
    }

    /// An [`InitFailed`](Self::InitFailed) error with no underlying cause.
    pub(crate) fn init_failed(message: impl Into<String>, stderr: String) -> Self {
        Self::InitFailed {
            message: message.into(),
            stderr,
            source: None,
        }
    }

    /// An [`InitFailed`](Self::InitFailed) error caused by `source`;
    /// `message` says what was being done.
    pub(crate) fn init_failed_by(
        message: &str,
        stderr: String,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::InitFailed {
            message: message.to_string(),
            stderr,
            source: Some(Box::new(source)),
        }
    }

    /// Appends a known harness version mismatch to an init failure's
    /// message: a harness that rejects this build's config at init (0.1.18
    /// refusing 0.1.23's `runWorkflow`) says so only on stderr, and the
    /// version is the likeliest explanation.
    pub(crate) fn with_version_note(self, harness_version: Option<&str>) -> Self {
        match (self, process::version_mismatch(harness_version)) {
            (
                Self::InitFailed {
                    message,
                    stderr,
                    source,
                },
                Some(note),
            ) => Self::InitFailed {
                message: format!("{message} ({note})"),
                stderr,
                source,
            },
            (error, _) => error,
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
