use super::SUPPORTED_HARNESS_VERSION;
use super::process;
use std::time::Duration;
use thiserror::Error;

// =============================================================================
// Errors
// =============================================================================

/// Errors from the Antigravity harness client.
///
/// Spawn- and init-time variants carry the tail of the harness's stderr —
/// that is where the harness reports actionable problems (e.g. `no text
/// model configuration provided`).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AntigravityError {
    /// The `localharness` binary could not be found.
    #[error(
        "localharness binary not found. Searched: {searched:?}. \
         Install it with `pip install google-antigravity=={SUPPORTED_HARNESS_VERSION}` \
         or set {}.",
        process::HARNESS_PATH_ENV
    )]
    HarnessNotFound {
        /// The locations that were searched, in order.
        searched: Vec<String>,
    },
    /// The stdio handshake with the harness failed.
    #[error("harness handshake failed: {message}\nharness stderr:\n{stderr}")]
    HandshakeFailed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// The conversation could not be initialized.
    #[error("conversation initialization failed: {message}\nharness stderr:\n{stderr}")]
    InitFailed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// The harness closed the connection unexpectedly (it likely crashed).
    #[error("harness connection closed: {message}\nharness stderr:\n{stderr}")]
    ConnectionClosed {
        /// What went wrong.
        message: String,
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
    /// The turn failed (model backend error, cancellation, or pre-turn
    /// denial).
    #[error("agent turn failed: {0}")]
    Turn(String),
    /// WebSocket transport error.
    #[error("websocket error: {0}")]
    WebSocket(String),
    /// The harness sent something this client could not parse.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON (de)serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
