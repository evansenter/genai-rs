//! Wire-level inspection of HTTP traffic.
//!
//! This module provides structured access to the raw requests, responses, and
//! streaming frames exchanged with the Gemini API. Every wire interaction is
//! surfaced as a [`WireEvent`]; implementations of [`WireInspector`] receive
//! those events as they happen.
//!
//! # Quick Start
//!
//! The zero-config path is the `LOUD_WIRE` environment variable, which
//! installs a [`LoudWirePrinter`] automatically when a `Client` is
//! constructed:
//!
//! ```bash
//! LOUD_WIRE=1 cargo run --example simple_interaction
//! ```
//!
//! `LOUD_WIRE` also accepts a comma-separated filter — see [`WireFilter`]
//! for the syntax. `1` (and the other "on" spellings) keeps the historical
//! everything-pretty-printed behavior; anything else narrows it:
//!
//! ```bash
//! LOUD_WIRE=summary                # one line per event
//! LOUD_WIRE=request,response       # HTTP only, no WebSocket noise
//! LOUD_WIRE=toolCall,summary       # one harness message type, one line each
//! ```
//!
//! For programmatic access, register inspectors on the client builder:
//!
//! ```no_run
//! use genai_rs::Client;
//! use genai_rs::wire::TracingForwarder;
//! use std::sync::Arc;
//!
//! let client = Client::builder("api-key".to_string())
//!     .add_wire_inspector(Arc::new(TracingForwarder::new()))
//!     .build()?;
//! # Ok::<(), genai_rs::GenaiError>(())
//! ```
//!
//! # Built-in Inspectors
//!
//! - [`LoudWirePrinter`]: pretty-printed, colored stderr output (what
//!   `LOUD_WIRE=1` gives you). Colors require the default-on `wire-color`
//!   feature; without it the output is plain text.
//! - [`TracingForwarder`]: forwards events to the [`tracing`] ecosystem at
//!   `DEBUG` level under the [`TRACING_TARGET`] (`genai_rs::wire`) target.
//!
//! # Correlation
//!
//! Each request is assigned a per-client monotonically increasing `id`.
//! All events for one HTTP request (request, status, body, SSE frames,
//! error body) share that id.

mod printer;
mod tracing_forwarder;

pub use printer::{LoudWirePrinter, WireFilter};
pub use tracing_forwarder::TracingForwarder;

use serde::Serialize;
use std::borrow::Cow;

/// The `tracing` target used by [`TracingForwarder`].
///
/// Enable it with an env-filter directive such as
/// `RUST_LOG=genai_rs::wire=debug`.
pub const TRACING_TARGET: &str = "genai_rs::wire";

/// A single wire-level event observed while talking to the API.
///
/// This enum is `#[non_exhaustive]`: new event kinds may be added in future
/// releases, so `match` statements must include a wildcard arm.
///
/// Events serialize with serde (useful for snapshot tests or shipping them to
/// external tooling); the variant is recorded in a `"kind"` tag field.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireEvent {
    /// An outgoing HTTP request.
    Request {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// HTTP method, plus stream annotations (e.g. `POST (stream)`).
        method: String,
        /// Full request URL (API keys are sent via header, never in the URL).
        url: String,
        /// JSON request body, if the request has one and it serialized cleanly.
        body: Option<serde_json::Value>,
    },
    /// The HTTP status line of a response.
    ResponseStatus {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// HTTP status code.
        status: u16,
    },
    /// The body of a successful response.
    ///
    /// Non-JSON bodies are preserved as a `serde_json::Value::String`.
    ResponseBody {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// Parsed response body.
        body: serde_json::Value,
    },
    /// The body of an error (non-2xx) response.
    ErrorBody {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// HTTP status code.
        status: u16,
        /// Raw error payload as returned by the server.
        body: String,
    },
    /// One event dispatched from an SSE stream.
    SseFrame {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// The event's `event:` field, if it had one.
        event_type: Option<String>,
        /// The event's `data:` payload (multiple `data:` lines joined with
        /// `\n`).
        data: String,
    },
    /// A file upload is starting.
    UploadStart {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// Display name or path of the file being uploaded.
        file_name: String,
        /// MIME type of the file.
        mime_type: String,
        /// Size of the file in bytes.
        size_bytes: u64,
    },
    /// A file upload completed successfully.
    UploadComplete {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// URI of the uploaded file.
        uri: String,
    },
    /// An Antigravity `localharness` process was spawned.
    ///
    /// For Antigravity sessions the correlation id is shared by every event
    /// of one harness session (spawn, WebSocket traffic, stderr).
    HarnessSpawn {
        /// Correlation id shared by all events of this harness session.
        id: u64,
        /// Filesystem path of the harness binary.
        path: String,
        /// OS process id, when available.
        pid: Option<u32>,
    },
    /// A proto-JSON message sent to the harness over its WebSocket.
    WsSend {
        /// Correlation id shared by all events of this harness session.
        id: u64,
        /// The JSON payload as sent.
        payload: serde_json::Value,
    },
    /// A proto-JSON message received from the harness over its WebSocket.
    WsReceive {
        /// Correlation id shared by all events of this harness session.
        id: u64,
        /// The JSON payload as received. Non-JSON frames are preserved as a
        /// `serde_json::Value::String`.
        payload: serde_json::Value,
    },
    /// A line of stderr output from the harness process.
    HarnessStderr {
        /// Correlation id shared by all events of this harness session.
        id: u64,
        /// One decoded stderr line (without the trailing newline).
        line: String,
    },
}

impl WireEvent {
    /// Returns the correlation id shared by all events of one HTTP request.
    #[must_use]
    pub fn id(&self) -> u64 {
        match self {
            Self::Request { id, .. }
            | Self::ResponseStatus { id, .. }
            | Self::ResponseBody { id, .. }
            | Self::ErrorBody { id, .. }
            | Self::SseFrame { id, .. }
            | Self::UploadStart { id, .. }
            | Self::UploadComplete { id, .. }
            | Self::HarnessSpawn { id, .. }
            | Self::WsSend { id, .. }
            | Self::WsReceive { id, .. }
            | Self::HarnessStderr { id, .. } => *id,
        }
    }
}

/// Receives [`WireEvent`]s as they happen.
///
/// Inspectors are registered via
/// [`ClientBuilder::add_wire_inspector`](crate::ClientBuilder::add_wire_inspector)
/// and are called synchronously on the request path, so implementations
/// should be fast and must not block. When no inspectors are installed the
/// library skips event construction entirely, so there is no cost in the
/// default configuration.
pub trait WireInspector: Send + Sync + 'static {
    /// Called once for each wire event.
    fn on_event(&self, event: &WireEvent);
}

/// The printer the `LOUD_WIRE` environment variable asks for, or `None`
/// when the variable is unset.
///
/// Single source of truth for the env gate: both `Client` and the
/// antigravity `AgentBuilder` install their zero-config inspector through
/// this, so a filter means the same thing on either path. (They diverged
/// once — the harness path re-implemented the gate as a bare `is_ok()` and
/// silently ignored the filter, which is exactly the surface where
/// filtering matters most.)
pub(crate) fn env_inspector() -> Option<LoudWirePrinter> {
    let raw = std::env::var("LOUD_WIRE").ok()?;
    // The value selects what to print — `1` keeps the historical
    // firehose, anything else filters. See `WireFilter`.
    Some(LoudWirePrinter::with_filter(WireFilter::parse(&raw)))
}

// =============================================================================
// Shared formatting helpers
// =============================================================================

/// Fields that should have their values truncated if too long.
/// These typically contain base64-encoded binary data.
const TRUNCATE_FIELDS: &[&str] = &["data", "signature"];

/// Fields whose values are secrets and must be fully redacted (never
/// printed, even partially), wherever they appear: third-party retrieval
/// credentials (`api_key` in the Exa/Parallel search configs), webhook
/// signing secrets (`new_signing_secret` on create, `secret` on rotate), and
/// credential material (`token`, `client_secret`, `refresh_token` in
/// Credentials request bodies).
const REDACT_FIELDS: &[&str] = &[
    "api_key",
    "new_signing_secret",
    "secret",
    "token",
    "client_secret",
    "refresh_token",
];

/// A field redacted only in context, because the name is too common to
/// redact everywhere: an `environment_variable` credential's secret, and an
/// environment variable's value inside an `env` map (`RemoteEnvironment`).
const CONTEXT_REDACT_FIELD: &str = "value";

/// Replacement value for redacted fields.
const REDACTED_PLACEHOLDER: &str = "[REDACTED]";

/// Maximum length before truncation (keep roughly the first 100 bytes,
/// never splitting a UTF-8 character).
const TRUNCATE_THRESHOLD: usize = 100;

/// Maximum bytes of a non-JSON body to print before truncating.
const RAW_BODY_LIMIT: usize = 1000;

/// Truncates a string to at most `max_bytes` bytes on a UTF-8 character
/// boundary, appending `"..."` when truncated.
fn truncate_utf8(s: &str, max_bytes: usize) -> Cow<'_, str> {
    if s.len() <= max_bytes {
        Cow::Borrowed(s)
    } else {
        // Find the last character whose END position fits within max_bytes.
        let truncate_at = s
            .char_indices()
            .take_while(|(i, c)| i + c.len_utf8() <= max_bytes)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        Cow::Owned(format!("{}...", &s[..truncate_at]))
    }
}

/// Truncate long base64-encoded fields and redact secret fields in a JSON
/// value.
///
/// Walks the JSON tree, truncates `"data"` and `"signature"` fields that
/// contain strings longer than 100 bytes, and replaces secret fields (see
/// [`REDACT_FIELDS`] and [`CONTEXT_REDACT_FIELD`]) with `"[REDACTED]"`
/// regardless of length. Text content and other fields are preserved in full.
fn truncate_long_fields(value: &mut serde_json::Value) {
    redact_walk(value, false);
}

/// The walk behind [`truncate_long_fields`]; `in_env` is true anywhere
/// beneath an `env` key.
fn redact_walk(value: &mut serde_json::Value, in_env: bool) {
    match value {
        serde_json::Value::Object(map) => {
            let redact_value = in_env
                || map.get("type").and_then(serde_json::Value::as_str)
                    == Some("environment_variable");
            for (key, val) in map.iter_mut() {
                let key = key.as_str();
                if REDACT_FIELDS.contains(&key) || (redact_value && key == CONTEXT_REDACT_FIELD) {
                    if !val.is_null() {
                        *val = serde_json::Value::String(REDACTED_PLACEHOLDER.to_string());
                    }
                } else if TRUNCATE_FIELDS.contains(&key) {
                    match val {
                        serde_json::Value::String(s) => {
                            if s.len() > TRUNCATE_THRESHOLD {
                                *s = truncate_utf8(s, TRUNCATE_THRESHOLD).into_owned();
                            }
                        }
                        // A `data`/`signature` key can hold structured
                        // payloads (e.g. Evergreen `Unknown` variants
                        // preserve raw JSON under `data`); recurse so
                        // secrets nested inside are still redacted.
                        _ => redact_walk(val, in_env),
                    }
                } else {
                    redact_walk(val, in_env || key == "env");
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_walk(item, in_env);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
