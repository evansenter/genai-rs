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

use crate::redact::REDACTED;
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
///
/// The fields and the serialized form hold the bodies as sent and received,
/// secrets included, except a [`WsSend`](Self::WsSend) payload, whose
/// secrets the session redacts before any inspector sees it. `Debug` prints
/// the secrets `LOUD_WIRE` redacts as `[REDACTED]`, so an inspector that
/// logs `{event:?}` does not undo the built-in inspectors' redaction; it
/// truncates nothing.
#[non_exhaustive]
#[derive(Clone, Serialize)]
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
        /// The JSON payload as sent, with the secrets `LOUD_WIRE` redacts
        /// already shown as `[REDACTED]` (the harness receives the
        /// original), so the harness's API key and its header and
        /// environment values reach no inspector.
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

// A request body can hold a retrieval `api_key`, MCP header values or a
// credential's secret, and a response body a rotated signing secret: printed
// through the wire's own walk (D-067). Matched without a wildcard, so
// a new event kind fails to compile here until it is printed or redacted.
impl std::fmt::Debug for WireEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request {
                id,
                method,
                url,
                body,
            } => f
                .debug_struct("Request")
                .field("id", id)
                .field("method", method)
                .field("url", url)
                .field("body", &body.as_ref().map(crate::redact::json))
                .finish(),
            Self::ResponseStatus { id, status } => f
                .debug_struct("ResponseStatus")
                .field("id", id)
                .field("status", status)
                .finish(),
            Self::ResponseBody { id, body } => f
                .debug_struct("ResponseBody")
                .field("id", id)
                .field("body", &crate::redact::json(body))
                .finish(),
            Self::ErrorBody { id, status, body } => f
                .debug_struct("ErrorBody")
                .field("id", id)
                .field("status", status)
                .field("body", &crate::redact::raw_json(body))
                .finish(),
            Self::SseFrame {
                id,
                event_type,
                data,
            } => f
                .debug_struct("SseFrame")
                .field("id", id)
                .field("event_type", event_type)
                .field("data", &crate::redact::raw_json(data))
                .finish(),
            Self::UploadStart {
                id,
                file_name,
                mime_type,
                size_bytes,
            } => f
                .debug_struct("UploadStart")
                .field("id", id)
                .field("file_name", file_name)
                .field("mime_type", mime_type)
                .field("size_bytes", size_bytes)
                .finish(),
            Self::UploadComplete { id, uri } => f
                .debug_struct("UploadComplete")
                .field("id", id)
                .field("uri", uri)
                .finish(),
            Self::HarnessSpawn { id, path, pid } => f
                .debug_struct("HarnessSpawn")
                .field("id", id)
                .field("path", path)
                .field("pid", pid)
                .finish(),
            Self::WsSend { id, payload } => f
                .debug_struct("WsSend")
                .field("id", id)
                .field("payload", &crate::redact::json(payload))
                .finish(),
            Self::WsReceive { id, payload } => f
                .debug_struct("WsReceive")
                .field("id", id)
                .field("payload", &crate::redact::json(payload))
                .finish(),
            Self::HarnessStderr { id, line } => f
                .debug_struct("HarnessStderr")
                .field("id", id)
                .field("line", line)
                .finish(),
        }
    }
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
/// signing secrets (`new_signing_secret` on create, `secret` on rotate),
/// credential material (`token`, `client_secret`, `refresh_token` in
/// Credentials request bodies), and the Gemini key in an Antigravity
/// harness's model endpoint (`apiKey`, camelCase on the harness wire).
const REDACT_FIELDS: &[&str] = &[
    "api_key",
    "new_signing_secret",
    "secret",
    "token",
    "client_secret",
    "refresh_token",
    "apiKey",
];

/// A field redacted only in context, because the name is too common to
/// redact everywhere: an `environment_variable` credential's secret, and an
/// environment variable's value inside an `env` map (`RemoteEnvironment`).
const CONTEXT_REDACT_FIELD: &str = "value";

/// Fields holding a header map whose *values* are secret while the names
/// are worth seeing: an MCP server's `headers` (`{"Authorization": "Bearer
/// ..."}`), an environment allowlist entry's `transform` list of header
/// maps, and an Antigravity model endpoint's `httpHeaders`. Every string
/// beneath them is redacted; the keys stay.
const REDACT_VALUES_BENEATH: &[&str] = &["headers", "transform", "httpHeaders"];

/// A map of environment variables, whose names are worth seeing and whose
/// values are secret: a `RemoteEnvironment`'s `env`, where each variable is
/// an object and its [`CONTEXT_REDACT_FIELD`] is the value, and an
/// Antigravity MCP stdio server's `env`, where each value is a string.
const ENV_FIELD: &str = "env";

/// A JSON Schema's map of property names to their schemas (a function
/// declaration's `parameters`, a response format): its keys name
/// properties, not fields, so a property called `env` or `api_key` holds a
/// schema, never a secret. Read so only beside `"type": "object"`, and only
/// for a value that is itself a schema ([`SCHEMA_KEYWORDS`]); any other
/// value under it is data, read as any entry is.
const SCHEMA_PROPERTIES: &str = "properties";

/// The keys one of which a property's schema holds: an object without any
/// of them under [`SCHEMA_PROPERTIES`] is data (a call's arguments to a
/// parameter named `properties`, say), not a schema.
const SCHEMA_KEYWORDS: &[&str] = &[
    "type",
    "$ref",
    "properties",
    "items",
    "enum",
    "const",
    "anyOf",
    "oneOf",
    "allOf",
];

/// Maximum length before truncation (keep roughly the first 100 bytes,
/// never splitting a UTF-8 character).
const TRUNCATE_THRESHOLD: usize = 100;

/// Maximum bytes of a non-JSON body to print before truncating.
const RAW_BODY_LIMIT: usize = 1000;

/// Truncates a string to at most `max_bytes` bytes on a UTF-8 character
/// boundary, appending `"..."` when truncated. Also shortens the body
/// previews in error messages (`http::error_helpers`).
pub(crate) fn truncate_utf8(s: &str, max_bytes: usize) -> Cow<'_, str> {
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
/// [`REDACT_FIELDS`], [`REDACT_VALUES_BENEATH`], [`ENV_FIELD`] and
/// [`CONTEXT_REDACT_FIELD`]; not the property names of a schema's
/// [`SCHEMA_PROPERTIES`]) with [`REDACTED`] regardless of length. Text
/// content and other fields are preserved in full.
fn truncate_long_fields(value: &mut serde_json::Value) {
    redact_walk(value, false, true);
}

/// Redacts the secret fields in a JSON value as [`truncate_long_fields`]
/// does, truncating nothing: what `Debug` prints in place of a body
/// (`crate::redact::json`), and what an Antigravity session sends its
/// inspectors in place of an outgoing message, so `Debug`, the harness and
/// the wire share one list.
pub(crate) fn redact_secrets(value: &mut serde_json::Value) {
    redact_walk(value, false, false);
}

/// [`redact_secrets`] for a JSON object held as a map (an `extra`), whose
/// own keys are read as an object's are.
pub(crate) fn redact_map_secrets(map: &mut serde_json::Map<String, serde_json::Value>) {
    redact_entries(map, false, false);
}

/// Replaces every string in `value` with the placeholder, keeping object
/// keys and structure.
pub(crate) fn redact_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => *s = REDACTED.to_string(),
        serde_json::Value::Object(map) => map.values_mut().for_each(redact_strings),
        serde_json::Value::Array(arr) => arr.iter_mut().for_each(redact_strings),
        _ => {}
    }
}

/// The walk behind [`truncate_long_fields`] and [`redact_secrets`]; `in_env`
/// is true anywhere beneath an [`ENV_FIELD`] key, and `truncate` shortens
/// the long base64 fields too.
fn redact_walk(value: &mut serde_json::Value, in_env: bool, truncate: bool) {
    match value {
        serde_json::Value::Object(map) => redact_entries(map, in_env, truncate),
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_walk(item, in_env, truncate);
            }
        }
        _ => {}
    }
}

/// One object's entries, for [`redact_walk`].
fn redact_entries(
    map: &mut serde_json::Map<String, serde_json::Value>,
    in_env: bool,
    truncate: bool,
) {
    let redact_value = in_env
        || map.get("type").and_then(serde_json::Value::as_str) == Some("environment_variable");
    let is_schema = map
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("object"));
    for (key, val) in map.iter_mut() {
        let key = key.as_str();
        if REDACT_VALUES_BENEATH.contains(&key) {
            redact_strings(val);
        } else if REDACT_FIELDS.contains(&key) || (redact_value && key == CONTEXT_REDACT_FIELD) {
            if !val.is_null() {
                *val = serde_json::Value::String(REDACTED.to_string());
            }
        } else if key == ENV_FIELD {
            redact_env(val, truncate);
        } else if key == SCHEMA_PROPERTIES
            && is_schema
            && let serde_json::Value::Object(properties) = val
        {
            redact_schema_properties(properties, in_env, truncate);
        } else if TRUNCATE_FIELDS.contains(&key) {
            match val {
                serde_json::Value::String(s) => {
                    if truncate && s.len() > TRUNCATE_THRESHOLD {
                        *s = truncate_utf8(s, TRUNCATE_THRESHOLD).into_owned();
                    }
                }
                // A `data`/`signature` key can hold structured
                // payloads (e.g. Evergreen `Unknown` variants
                // preserve raw JSON under `data`); recurse so
                // secrets nested inside are still redacted.
                _ => redact_walk(val, in_env, truncate),
            }
        } else {
            redact_walk(val, in_env, truncate);
        }
    }
}

/// A schema's [`SCHEMA_PROPERTIES`]: each value that is a schema is walked
/// as one, its property name not read; every other entry is read as any
/// object's entry is, so a secret sent as data under the key stays redacted.
fn redact_schema_properties(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    in_env: bool,
    truncate: bool,
) {
    let mut data = serde_json::Map::new();
    for (name, value) in properties.iter_mut() {
        let schema = value
            .as_object()
            .is_some_and(|object| SCHEMA_KEYWORDS.iter().any(|key| object.contains_key(*key)));
        if schema {
            redact_walk(value, in_env, truncate);
        } else {
            data.insert(name.clone(), value.take());
        }
    }
    redact_entries(&mut data, in_env, truncate);
    for (name, value) in data {
        properties.insert(name, value);
    }
}

/// An [`ENV_FIELD`]'s value: a string in it, as the whole `env` or as an
/// entry of its map or list, is a variable's value; beneath an entry that
/// is an object, the [`CONTEXT_REDACT_FIELD`] is, and a credential's ID
/// beside it is not.
fn redact_env(value: &mut serde_json::Value, truncate: bool) {
    let entries: Box<dyn Iterator<Item = &mut serde_json::Value>> = match value {
        serde_json::Value::Object(map) => Box::new(map.values_mut()),
        serde_json::Value::Array(arr) => Box::new(arr.iter_mut()),
        other => Box::new(std::iter::once(other)),
    };
    for entry in entries {
        if entry.is_string() {
            *entry = serde_json::Value::String(REDACTED.to_string());
        } else {
            redact_walk(entry, true, truncate);
        }
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
