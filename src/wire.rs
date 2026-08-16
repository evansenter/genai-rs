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
    /// A frame observed on an SSE stream.
    SseFrame {
        /// Correlation id shared by all events of this request.
        id: u64,
        /// The value of an `event:` line, when the frame is an event-type
        /// line. `None` for `data:` payload frames.
        event_type: Option<String>,
        /// The raw `data:` payload. Empty for `event:`-only frames.
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

// =============================================================================
// Shared formatting helpers
// =============================================================================

/// Fields that should have their values truncated if too long.
/// These typically contain base64-encoded binary data.
const TRUNCATE_FIELDS: &[&str] = &["data", "signature"];

/// Fields whose values are secrets and must be fully redacted (never
/// printed, even partially). Covers third-party retrieval credentials
/// (e.g. Exa/Parallel `api_key` in search configs) and webhook signing
/// secrets (`new_signing_secret` on create, `secret` on rotate — both
/// returned exactly once by the API).
const REDACT_FIELDS: &[&str] = &["api_key", "new_signing_secret", "secret"];

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
/// contain strings longer than 100 bytes, and replaces secret fields (e.g.
/// `"api_key"`) with `"[REDACTED]"` regardless of length. Text content and
/// other fields are preserved in full.
fn truncate_long_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if REDACT_FIELDS.contains(&key.as_str()) {
                    if !val.is_null() {
                        *val = serde_json::Value::String(REDACTED_PLACEHOLDER.to_string());
                    }
                } else if TRUNCATE_FIELDS.contains(&key.as_str()) {
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
                        _ => truncate_long_fields(val),
                    }
                } else {
                    truncate_long_fields(val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                truncate_long_fields(item);
            }
        }
        _ => {}
    }
}

// =============================================================================
// Color abstraction (feature-gated)
// =============================================================================

#[cfg(feature = "wire-color")]
mod paint {
    use colored::Colorize;

    pub fn bold(s: &str) -> String {
        s.bold().to_string()
    }
    pub fn dimmed(s: &str) -> String {
        s.dimmed().to_string()
    }
    pub fn green(s: &str) -> String {
        s.green().to_string()
    }
    pub fn red(s: &str) -> String {
        s.red().to_string()
    }
    pub fn green_bold(s: &str) -> String {
        s.green().bold().to_string()
    }
    pub fn yellow_bold(s: &str) -> String {
        s.yellow().bold().to_string()
    }
    pub fn magenta_bold(s: &str) -> String {
        s.magenta().bold().to_string()
    }
    pub fn cyan_bold(s: &str) -> String {
        s.cyan().bold().to_string()
    }
    pub fn red_bold(s: &str) -> String {
        s.red().bold().to_string()
    }
    pub fn blue_bold(s: &str) -> String {
        s.blue().bold().to_string()
    }

    /// Colorize JSON for terminal output, or `None` if colorization fails.
    pub fn json(value: &serde_json::Value) -> Option<String> {
        colored_json::to_colored_json_auto(value).ok()
    }
}

#[cfg(not(feature = "wire-color"))]
mod paint {
    pub fn bold(s: &str) -> String {
        s.to_string()
    }
    pub fn dimmed(s: &str) -> String {
        s.to_string()
    }
    pub fn green(s: &str) -> String {
        s.to_string()
    }
    pub fn red(s: &str) -> String {
        s.to_string()
    }
    pub fn green_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn yellow_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn magenta_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn cyan_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn red_bold(s: &str) -> String {
        s.to_string()
    }
    pub fn blue_bold(s: &str) -> String {
        s.to_string()
    }

    /// Without the `wire-color` feature there is no colorizer; callers fall
    /// back to plain pretty-printed JSON.
    pub fn json(_value: &serde_json::Value) -> Option<String> {
        None
    }
}

// =============================================================================
// LoudWirePrinter
// =============================================================================

// =============================================================================
// WireFilter
// =============================================================================

/// Which events [`LoudWirePrinter`] should print, and how loudly.
///
/// Parsed from the `LOUD_WIRE` environment variable. The firehose is the
/// right default for a single request, and the wrong one the moment a
/// harness session is involved: a few turns produce thousands of
/// pretty-printed lines, and finding the one message that matters means
/// grepping raw JSON out of the scrollback.
///
/// # Syntax
///
/// `LOUD_WIRE` takes a comma-separated list. `1`, `true`, or any empty
/// value means "everything, pretty-printed" — the historical behavior.
///
/// | Selector | Keeps |
/// |----------|-------|
/// | `request` | HTTP requests |
/// | `response` | HTTP responses (status, body, error bodies) |
/// | `sse` | SSE frames |
/// | `ws` | Every WebSocket message |
/// | `harness` | Harness spawn and stderr lines |
/// | `upload` | File-upload start/complete |
/// | *anything else* | A WebSocket payload whose top-level key matches (e.g. `stepUpdate`, `toolCall`), or whose *nested* key one level in matches (e.g. `mcpTool`, `runCommand` — the step actions that all live under `stepUpdate`) |
/// | `summary` | Modifier: one line per event instead of full bodies |
///
/// A value that matches no category and no payload key selects **nothing**,
/// so `LOUD_WIRE=0`, `false`, `off` and `verbose` all print silence. Note
/// that this is a behavior change: the gate used to be "is the variable set
/// at all", so those values previously produced the full firehose. If
/// output has gone missing, an unrecognized selector is the first thing to
/// check.
///
/// # Examples
///
/// ```bash
/// LOUD_WIRE=1                      # everything (unchanged)
/// LOUD_WIRE=summary                # everything, one line each
/// LOUD_WIRE=stepUpdate             # only stepUpdate WS payloads
/// LOUD_WIRE=toolCall,summary       # tool calls, one line each
/// LOUD_WIRE=request,response       # HTTP only, no WebSocket noise
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireFilter {
    /// Lowercased selectors. Empty means "keep everything".
    selectors: Vec<String>,
    /// One line per event instead of pretty-printed bodies.
    summary: bool,
}

impl WireFilter {
    /// A filter that keeps every event, pretty-printed.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            selectors: Vec::new(),
            summary: false,
        }
    }

    /// Parses a `LOUD_WIRE` value. See the type docs for the syntax.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut selectors = Vec::new();
        let mut summary = false;
        let mut keep_all = false;
        for token in raw.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                // The historical "on" values select everything.
                "1" | "true" | "yes" | "on" | "all" => keep_all = true,
                "summary" => summary = true,
                other => selectors.push(other.to_string()),
            }
        }
        // Deliberately after the loop rather than an early return: `summary`
        // is a modifier, so `1,summary` and `summary,1` must mean the same
        // thing. Returning on the "on" arm would honor only the latter.
        if keep_all {
            selectors.clear();
        }
        Self { selectors, summary }
    }

    /// True when bodies should be collapsed to one line per event.
    #[must_use]
    pub const fn is_summary(&self) -> bool {
        self.summary
    }

    /// The category name an event belongs to, for selector matching.
    const fn category(event: &WireEvent) -> &'static str {
        match event {
            WireEvent::Request { .. } => "request",
            WireEvent::ResponseStatus { .. }
            | WireEvent::ResponseBody { .. }
            | WireEvent::ErrorBody { .. } => "response",
            WireEvent::SseFrame { .. } => "sse",
            WireEvent::UploadStart { .. } | WireEvent::UploadComplete { .. } => "upload",
            WireEvent::HarnessSpawn { .. } | WireEvent::HarnessStderr { .. } => "harness",
            WireEvent::WsSend { .. } | WireEvent::WsReceive { .. } => "ws",
        }
    }

    /// Whether this event should be printed.
    #[must_use]
    pub fn allows(&self, event: &WireEvent) -> bool {
        if self.selectors.is_empty() {
            return true;
        }
        let category = Self::category(event);
        if self.selectors.iter().any(|s| s == category) {
            return true;
        }
        // Otherwise a selector may name a WebSocket payload's oneof key,
        // which is the granularity that actually matters when reading a
        // harness session (`stepUpdate` vs `toolCall` vs `userInput`).
        // Received frames get the extra nested-action granularity below;
        // sends are matched on their arm alone, for the same reason
        // `ws_payload_keys` only qualifies receives. Selection and summary
        // rendering must agree about what a line is *about*, and an
        // `InputEvent` arm has no action to descend into.
        let (payload, harness_receive) = match event {
            WireEvent::WsSend { payload, .. } => (payload, false),
            WireEvent::WsReceive { payload, .. } => (payload, true),
            _ => return false,
        };
        payload.as_object().is_some_and(|map| {
            map.iter()
                // Gated the same way `payload_keys_inner` gates it, so a
                // key that renders on a line can also select it. Latent
                // either way today — an `InputEvent` serializes as a lone
                // oneof key and carries no envelope — but the two sides
                // drifting is the failure this pairing exists to prevent.
                .filter(|(k, _)| !(harness_receive && is_envelope_key(k)))
                .any(|(key, value)| {
                    if self.selectors.contains(&key.to_ascii_lowercase()) {
                        return true;
                    }
                    if !harness_receive {
                        return false;
                    }
                    // ...and one level deeper, because the granularity a
                    // reader usually wants is *which action* a step carried,
                    // and every builtin action (`mcpTool`, `runCommand`,
                    // `viewFile`, …) hides under the single `stepUpdate` key.
                    // Without this, `LOUD_WIRE=mcpTool` silently matches
                    // nothing — which is exactly what it did until an example
                    // advertised it and printed silence.
                    //
                    // Restricted to *object-valued* nested keys, which is
                    // what an action is — matching a scalar like `text`
                    // would print a line labelled with some unrelated
                    // action rather than the key that matched. And only one
                    // level: deeper would match leaf field names across
                    // unrelated messages, since selectors are not scoped by
                    // message type.
                    nested_action_keys(value)
                        .iter()
                        .any(|nested| self.selectors.contains(&nested.to_ascii_lowercase()))
                })
        })
    }
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

/// The object-valued keys one level inside a payload value — the step
/// *actions* (`mcpTool`, `runCommand`, `viewFile`, …), as opposed to a
/// step's scalar fields (`text`, `stepIndex`, `state`).
///
/// Shared by selection and summary rendering so the two cannot disagree
/// about what a line is "about": a nested selector matches exactly the
/// keys the label can name.
fn nested_action_keys(value: &serde_json::Value) -> Vec<&str> {
    value.as_object().map_or_else(Vec::new, |inner| {
        inner
            .iter()
            .filter(|(_, v)| v.is_object())
            .map(|(k, _)| k.as_str())
            .collect()
    })
}

/// Envelope bookkeeping that rides alongside a harness message rather
/// than being one, and is therefore useless as a selector: matching on
/// `seqNum` would keep the whole stream, and `usageUpdate` would keep
/// every message that happens to carry usage.
///
/// Deliberately the same set the protocol deserializer strips before it
/// picks the oneof arm (`OutputEvent::deserialize` removes `seqNum`,
/// `timestampMicros` and the usage keys) — selection, summary rendering
/// and deserialization should agree on what a message *is*. Excluded from
/// both selector matching and summary rendering for that reason.
fn is_envelope_key(key: &str) -> bool {
    matches!(
        key,
        "seqNum" | "timestampMicros" | "usageMetadata" | "usageUpdate"
    )
}

/// Pretty-prints wire events to stderr.
///
/// This is the inspector installed automatically when the `LOUD_WIRE`
/// environment variable is set at `Client` construction time. Output format:
///
/// - Green `>>>` for outgoing requests, red `<<<` for incoming responses
/// - Timestamps and request ids (`[REQ#N]` / `[RES#N]`) for correlation
/// - Request ids use alternating colors (even/odd) for visual distinction:
///   `[REQ#N]` green (even) / yellow (odd); `[RES#N]` magenta (even) /
///   cyan (odd)
/// - SSE frames labelled in blue
/// - Pretty-printed (and, with the `wire-color` feature, colored) JSON
/// - Base64-heavy `data`/`signature` fields truncated to keep output readable
/// - Secret fields (e.g. third-party retrieval `api_key`s) fully redacted
#[derive(Debug, Clone, Default)]
pub struct LoudWirePrinter {
    filter: WireFilter,
}

impl LoudWirePrinter {
    /// Creates a printer that prints every event, pretty-printed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            filter: WireFilter::all(),
        }
    }

    /// Creates a printer restricted by `filter` (see [`WireFilter`]).
    #[must_use]
    pub const fn with_filter(filter: WireFilter) -> Self {
        Self { filter }
    }

    /// One-line rendering, for `LOUD_WIRE=summary`. Enough to see the
    /// shape and order of a session without the bodies that make a
    /// harness run unreadable.
    fn print_summary(&self, event: &WireEvent) {
        let (id, label, detail) = match event {
            WireEvent::Request {
                id, method, url, ..
            } => (*id, "REQ", format!("{method} {url}")),
            WireEvent::ResponseStatus { id, status } => (*id, "RES", format!("status {status}")),
            WireEvent::ResponseBody { id, body } => (*id, "RES", Self::payload_keys(body)),
            WireEvent::ErrorBody { id, status, body } => {
                (*id, "ERR", format!("status {status}, {} bytes", body.len()))
            }
            WireEvent::SseFrame { id, event_type, .. } => {
                (*id, "SSE", event_type.clone().unwrap_or_else(|| "-".into()))
            }
            WireEvent::UploadStart {
                id,
                file_name,
                size_bytes,
                ..
            } => (*id, "UP", format!("{file_name} ({size_bytes} bytes)")),
            WireEvent::UploadComplete { id, uri } => (*id, "UP", format!("done {uri}")),
            WireEvent::HarnessSpawn { id, path, pid } => {
                (*id, "HARNESS", format!("{path} (pid {pid:?})"))
            }
            WireEvent::HarnessStderr { id, line } => (*id, "STDERR", line.clone()),
            WireEvent::WsSend { id, payload } => (*id, "WS>", Self::payload_keys(payload)),
            WireEvent::WsReceive { id, payload } => (*id, "WS<", Self::ws_payload_keys(payload)),
        };
        eprintln!(
            "{} {} [#{id}] {label:<7} {detail}",
            paint::bold("[LOUD_WIRE]"),
            Self::timestamp()
        );
    }

    /// The oneof key(s) of a WebSocket payload — the part that says what
    /// the message *is*.
    ///
    /// The harness does send messages whose only non-envelope content is
    /// usage (a bare `seqNum` + `usageUpdate` deserializes with no payload
    /// at all), so an empty key list is a real message and not a bug.
    /// Label it rather than printing a blank detail column, which in the
    /// one format meant for skimming would read as a rendering fault. The
    /// label stays neutral because this also renders HTTP response bodies,
    /// where there is no envelope to speak of — and those stay unqualified
    /// for the same reason (see `ws_payload_keys`).
    fn payload_keys(payload: &serde_json::Value) -> String {
        Self::payload_keys_inner(payload, false)
    }

    /// `payload_keys` for harness WebSocket messages the crate *receives*,
    /// qualifying a step with the action it carried.
    ///
    /// Receive-only on purpose. `InputEvent` arms have no actions, so the
    /// argument that excludes HTTP bodies applies to sends too — and
    /// `questionResponse` carries an object-valued `response` field, which
    /// would both render as `questionResponse/response` and be selected by
    /// `LOUD_WIRE=response`, the category selector for HTTP responses.
    fn ws_payload_keys(payload: &serde_json::Value) -> String {
        Self::payload_keys_inner(payload, true)
    }

    fn payload_keys_inner(payload: &serde_json::Value, harness_receive: bool) -> String {
        payload.as_object().map_or_else(
            || "(non-object)".to_string(),
            |m| {
                let keys: Vec<String> = m
                    .iter()
                    // Envelope stripping is a harness-wire notion. On a
                    // Gemini HTTP response `usageMetadata` is a real
                    // top-level field, and stripping it there could render
                    // a body as `(no payload keys)` instead of naming what
                    // came back.
                    .filter(|(k, _)| !(harness_receive && is_envelope_key(k)))
                    .map(|(key, value)| {
                        // Qualify a harness message with the action it
                        // carried: a bare "stepUpdate" says almost nothing,
                        // while "stepUpdate/mcpTool" says which action ran
                        // — and names exactly what a nested selector can
                        // match, so asking for `mcpTool` cannot produce a
                        // line labelled with a different key.
                        //
                        // WebSocket only. HTTP response bodies share this
                        // renderer and have no actions, so qualifying them
                        // would invent structure that isn't there
                        // (`interaction/outputs`).
                        if !harness_receive {
                            return key.clone();
                        }
                        let actions = nested_action_keys(value);
                        if actions.is_empty() {
                            key.clone()
                        } else {
                            // Every action, not just the first: a step
                            // carries one in practice, but naming only the
                            // lowest-sorting of several would reintroduce
                            // the label/selector mismatch in a rarer shape.
                            format!("{key}/{}", actions.join("+"))
                        }
                    })
                    .collect();
                if keys.is_empty() {
                    "(no payload keys)".to_string()
                } else {
                    keys.join(", ")
                }
            },
        )
    }

    /// Format the current timestamp for log output (ISO 8601 UTC).
    fn timestamp() -> String {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    /// Log prefix with timestamp and request ID (for outgoing requests).
    /// Colors alternate: green (even) / yellow (odd) for visual distinction.
    fn request_prefix(request_id: u64) -> String {
        let ts = paint::dimmed(&Self::timestamp());
        let req_label = format!("[REQ#{request_id}]");
        let colored_label = if request_id.is_multiple_of(2) {
            paint::green_bold(&req_label)
        } else {
            paint::yellow_bold(&req_label)
        };
        format!("{} {} {}", paint::bold("[LOUD_WIRE]"), ts, colored_label)
    }

    /// Log prefix with timestamp and response ID (for incoming responses).
    /// Colors alternate: magenta (even) / cyan (odd) for visual distinction.
    fn response_prefix(request_id: u64) -> String {
        let ts = paint::dimmed(&Self::timestamp());
        let res_label = format!("[RES#{request_id}]");
        let colored_label = if request_id.is_multiple_of(2) {
            paint::magenta_bold(&res_label)
        } else {
            paint::cyan_bold(&res_label)
        };
        format!("{} {} {}", paint::bold("[LOUD_WIRE]"), ts, colored_label)
    }

    /// Pretty-print a JSON value line-by-line under the given prefix,
    /// truncating base64-heavy fields.
    fn print_json(prefix: &str, value: &serde_json::Value) {
        let mut value = value.clone();
        truncate_long_fields(&mut value);
        if let Some(colored) = paint::json(&value) {
            for line in colored.lines() {
                eprintln!("{prefix} {line}");
            }
        } else if let Ok(pretty) = serde_json::to_string_pretty(&value) {
            for line in pretty.lines() {
                eprintln!("{prefix} {line}");
            }
        }
    }

    fn print_request(id: u64, method: &str, url: &str, body: Option<&serde_json::Value>) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");

        eprintln!("{prefix} {direction} {method} {url}");

        if let Some(body) = body {
            eprintln!("{prefix} {}:", paint::green("Body"));
            Self::print_json(&prefix, body);
        }
    }

    fn print_response_status(id: u64, status: u16) {
        let prefix = Self::response_prefix(id);
        let direction = paint::red_bold("<<<");
        let status_text = if status < 300 {
            paint::green(&format!("{status} OK"))
        } else {
            paint::red(&format!("{status} ERROR"))
        };

        eprintln!("{prefix} {direction} {status_text}");
    }

    fn print_response_body(id: u64, body: &serde_json::Value) {
        let prefix = Self::response_prefix(id);

        // Non-JSON bodies are carried as a top-level string: print raw
        // (truncated for safety) instead of as a JSON-quoted string.
        if let serde_json::Value::String(raw) = body {
            let truncated = truncate_utf8(raw, RAW_BODY_LIMIT);
            eprintln!("{prefix} {}: {truncated}", paint::red("Response"));
            return;
        }

        eprintln!("{prefix} {}:", paint::red("Response"));
        Self::print_json(&prefix, body);
    }

    fn print_error_body(id: u64, status: u16, body: &str) {
        let prefix = Self::response_prefix(id);
        let label = paint::red_bold(&format!("Error ({status})"));

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
            eprintln!("{prefix} {label}:");
            Self::print_json(&prefix, &parsed);
        } else {
            let truncated = truncate_utf8(body, RAW_BODY_LIMIT);
            eprintln!("{prefix} {label}: {truncated}");
        }
    }

    fn print_sse_frame(id: u64, event_type: Option<&str>, data: &str) {
        let prefix = Self::response_prefix(id);
        let label = paint::blue_bold("SSE");

        if let Some(event_type) = event_type {
            eprintln!("{prefix} {label} event: {event_type}");
        }

        if data.is_empty() {
            return;
        }

        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
            eprintln!("{prefix} {label}:");
            Self::print_json(&prefix, &parsed);
        } else {
            eprintln!("{prefix} {label}: {data}");
        }
    }

    fn print_upload_start(id: u64, file_name: &str, mime_type: &str, size_bytes: u64) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");
        let size_mb = size_bytes as f64 / 1_048_576.0;

        eprintln!(
            "{prefix} {direction} {} \"{file_name}\" ({mime_type}, {size_mb:.2} MB)",
            paint::green_bold("UPLOAD")
        );
    }

    fn print_upload_complete(id: u64, uri: &str) {
        let prefix = Self::response_prefix(id);
        let direction = paint::red_bold("<<<");

        eprintln!(
            "{prefix} {direction} {} {uri}",
            paint::green_bold("UPLOADED")
        );
    }

    fn print_harness_spawn(id: u64, path: &str, pid: Option<u32>) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");
        let pid_text = pid.map_or_else(|| "?".to_string(), |p| p.to_string());

        eprintln!(
            "{prefix} {direction} {} {path} (pid {pid_text})",
            paint::green_bold("HARNESS")
        );
    }

    fn print_ws_send(id: u64, payload: &serde_json::Value) {
        let prefix = Self::request_prefix(id);
        let direction = paint::green_bold(">>>");

        eprintln!("{prefix} {direction} {}:", paint::green("WS Send"));
        Self::print_json(&prefix, payload);
    }

    fn print_ws_receive(id: u64, payload: &serde_json::Value) {
        let prefix = Self::response_prefix(id);
        let direction = paint::red_bold("<<<");

        eprintln!("{prefix} {direction} {}:", paint::red("WS Receive"));
        Self::print_json(&prefix, payload);
    }

    fn print_harness_stderr(id: u64, line: &str) {
        let prefix = Self::response_prefix(id);
        let label = paint::blue_bold("STDERR");
        let truncated = truncate_utf8(line, RAW_BODY_LIMIT);

        eprintln!("{prefix} {label}: {truncated}");
    }
}

impl WireInspector for LoudWirePrinter {
    fn on_event(&self, event: &WireEvent) {
        if !self.filter.allows(event) {
            return;
        }
        if self.filter.is_summary() {
            self.print_summary(event);
            return;
        }
        match event {
            WireEvent::Request {
                id,
                method,
                url,
                body,
            } => Self::print_request(*id, method, url, body.as_ref()),
            WireEvent::ResponseStatus { id, status } => Self::print_response_status(*id, *status),
            WireEvent::ResponseBody { id, body } => Self::print_response_body(*id, body),
            WireEvent::ErrorBody { id, status, body } => {
                Self::print_error_body(*id, *status, body);
            }
            WireEvent::SseFrame {
                id,
                event_type,
                data,
            } => Self::print_sse_frame(*id, event_type.as_deref(), data),
            WireEvent::UploadStart {
                id,
                file_name,
                mime_type,
                size_bytes,
            } => Self::print_upload_start(*id, file_name, mime_type, *size_bytes),
            WireEvent::UploadComplete { id, uri } => Self::print_upload_complete(*id, uri),
            WireEvent::HarnessSpawn { id, path, pid } => {
                Self::print_harness_spawn(*id, path, *pid);
            }
            WireEvent::WsSend { id, payload } => Self::print_ws_send(*id, payload),
            WireEvent::WsReceive { id, payload } => Self::print_ws_receive(*id, payload),
            WireEvent::HarnessStderr { id, line } => Self::print_harness_stderr(*id, line),
        }
    }
}

// =============================================================================
// TracingForwarder
// =============================================================================

/// Forwards wire events to the [`tracing`] ecosystem.
///
/// Events are emitted at `DEBUG` level to the [`TRACING_TARGET`]
/// (`genai_rs::wire`) target with structured fields, including the JSON
/// body serialized as a string. Enable it with:
///
/// ```bash
/// RUST_LOG=genai_rs::wire=debug cargo run --example simple_interaction
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingForwarder;

impl TracingForwarder {
    /// Creates a new forwarder.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// Renders a JSON body for tracing output with the same redaction and
/// truncation guarantees as [`LoudWirePrinter`] (secret fields replaced,
/// long base64 fields truncated).
fn redacted_body_string(body: &serde_json::Value) -> String {
    let mut value = body.clone();
    truncate_long_fields(&mut value);
    value.to_string()
}

/// Like [`redacted_body_string`] but for raw string payloads (error bodies,
/// SSE `data:` frames): JSON payloads are redacted structurally; non-JSON
/// payloads pass through unchanged.
fn redacted_raw_string(raw: &str) -> Cow<'_, str> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(mut value) => {
            truncate_long_fields(&mut value);
            Cow::Owned(value.to_string())
        }
        Err(_) => Cow::Borrowed(raw),
    }
}

impl WireInspector for TracingForwarder {
    fn on_event(&self, event: &WireEvent) {
        use tracing::Level;

        match event {
            WireEvent::Request {
                id,
                method,
                url,
                body,
            } => {
                let body = body.as_ref().map(redacted_body_string);
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "request",
                    id,
                    method = %method,
                    url = %url,
                    body = body.as_deref(),
                    "wire request"
                );
            }
            WireEvent::ResponseStatus { id, status } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "response_status",
                    id,
                    status,
                    "wire response status"
                );
            }
            WireEvent::ResponseBody { id, body } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "response_body",
                    id,
                    body = %redacted_body_string(body),
                    "wire response body"
                );
            }
            WireEvent::ErrorBody { id, status, body } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "error_body",
                    id,
                    status,
                    body = %redacted_raw_string(body),
                    "wire error body"
                );
            }
            WireEvent::SseFrame {
                id,
                event_type,
                data,
            } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "sse_frame",
                    id,
                    event_type = event_type.as_deref(),
                    data = %redacted_raw_string(data),
                    "wire sse frame"
                );
            }
            WireEvent::UploadStart {
                id,
                file_name,
                mime_type,
                size_bytes,
            } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "upload_start",
                    id,
                    file_name = %file_name,
                    mime_type = %mime_type,
                    size_bytes,
                    "wire upload start"
                );
            }
            WireEvent::UploadComplete { id, uri } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "upload_complete",
                    id,
                    uri = %uri,
                    "wire upload complete"
                );
            }
            WireEvent::HarnessSpawn { id, path, pid } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "harness_spawn",
                    id,
                    path = %path,
                    pid,
                    "wire harness spawn"
                );
            }
            WireEvent::WsSend { id, payload } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "ws_send",
                    id,
                    payload = %payload,
                    "wire ws send"
                );
            }
            WireEvent::WsReceive { id, payload } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "ws_receive",
                    id,
                    payload = %payload,
                    "wire ws receive"
                );
            }
            WireEvent::HarnessStderr { id, line } => {
                tracing::event!(
                    target: "genai_rs::wire",
                    Level::DEBUG,
                    kind = "harness_stderr",
                    id,
                    line = %line,
                    "wire harness stderr"
                );
            }
        }
    }
}

#[cfg(test)]
mod filter_tests {
    use super::{WireEvent, WireFilter};
    use serde_json::json;

    fn ws(payload: serde_json::Value) -> WireEvent {
        WireEvent::WsReceive { id: 1, payload }
    }

    fn request() -> WireEvent {
        WireEvent::Request {
            id: 1,
            method: "POST".into(),
            url: "https://x/y".into(),
            body: None,
        }
    }

    #[test]
    fn on_values_keep_everything() {
        // The historical spellings must not start filtering anything out.
        for raw in ["1", "true", "yes", "on", "all", "", "  "] {
            let f = WireFilter::parse(raw);
            assert!(f.allows(&request()), "{raw:?} should keep requests");
            assert!(
                f.allows(&ws(json!({"stepUpdate": {}}))),
                "{raw:?} should keep ws"
            );
            assert!(!f.is_summary(), "{raw:?} should not imply summary");
        }
    }

    #[test]
    fn category_selectors_filter_by_event_kind() {
        let f = WireFilter::parse("request");
        assert!(f.allows(&request()));
        assert!(!f.allows(&ws(json!({"stepUpdate": {}}))));
        assert!(!f.allows(&WireEvent::ResponseStatus { id: 1, status: 200 }));

        let f = WireFilter::parse("response");
        assert!(f.allows(&WireEvent::ResponseStatus { id: 1, status: 200 }));
        assert!(!f.allows(&request()));

        // `ws` keeps every WebSocket message regardless of payload key.
        let f = WireFilter::parse("ws");
        assert!(f.allows(&ws(json!({"toolCall": {}}))));
        assert!(f.allows(&ws(json!({"anythingElse": {}}))));
        assert!(!f.allows(&request()));
    }

    #[test]
    fn payload_key_selectors_pick_out_one_message_type() {
        // The granularity that actually matters when reading a harness
        // session: which oneof arm, not which transport.
        let f = WireFilter::parse("stepUpdate");
        assert!(f.allows(&ws(json!({"stepUpdate": {"text": "hi"}}))));
        assert!(!f.allows(&ws(json!({"toolCall": {}}))));
        assert!(!f.allows(&request()));

        // Case-insensitive, and envelope metadata does not count as a match.
        let f = WireFilter::parse("STEPUPDATE");
        assert!(f.allows(&ws(json!({"stepUpdate": {}}))));
        let f = WireFilter::parse("seqNum");
        assert!(!f.allows(&ws(json!({"seqNum": "1", "toolCall": {}}))));

        // Usage rides along with a message rather than being one, and the
        // deserializer strips it before picking the oneof arm — so it must
        // not select either, or it would keep every message carrying usage.
        for envelope in ["usageUpdate", "usageMetadata"] {
            let f = WireFilter::parse(envelope);
            assert!(
                !f.allows(&ws(json!({"stepUpdate": {}, envelope: {"total": {}}}))),
                "{envelope:?} must not act as a selector"
            );
        }
        // ...and the message it rides on still selects normally.
        let f = WireFilter::parse("stepUpdate");
        assert!(f.allows(&ws(json!({"stepUpdate": {}, "usageMetadata": {}}))));
    }

    #[test]
    fn selectors_compose_and_summary_is_a_modifier() {
        let f = WireFilter::parse("stepUpdate,toolCall");
        assert!(f.allows(&ws(json!({"stepUpdate": {}}))));
        assert!(f.allows(&ws(json!({"toolCall": {}}))));
        assert!(!f.allows(&ws(json!({"userInput": "x"}))));

        // `summary` alone changes rendering, not selection.
        let f = WireFilter::parse("summary");
        assert!(f.is_summary());
        assert!(f.allows(&request()));
        assert!(f.allows(&ws(json!({"anything": {}}))));

        // ...and combines with selectors.
        let f = WireFilter::parse("toolCall,summary");
        assert!(f.is_summary());
        assert!(f.allows(&ws(json!({"toolCall": {}}))));
        assert!(!f.allows(&request()));
    }

    #[test]
    fn nested_selectors_reach_step_actions() {
        // The case this exists for: every builtin action lives one level
        // under `stepUpdate`, so a top-level-only match made the most
        // useful selector ("show me the MCP calls") match nothing.
        let step_with_mcp = ws(json!({
            "seqNum": "3",
            "stepUpdate": {
                "stepIndex": 2,
                "mcpTool": {"serverName": "widgets", "toolName": "list_widgets"},
            },
        }));
        let step_with_view = ws(json!({
            "stepUpdate": {"stepIndex": 4, "viewFile": {"path": "/tmp/x"}},
        }));

        let f = WireFilter::parse("mcpTool");
        assert!(f.allows(&step_with_mcp));
        assert!(
            !f.allows(&step_with_view),
            "a nested selector must still discriminate between actions"
        );

        // The enclosing key keeps working, and still matches both.
        let f = WireFilter::parse("stepUpdate");
        assert!(f.allows(&step_with_mcp));
        assert!(f.allows(&step_with_view));

        // Only one level deep: a leaf inside the action is not a selector,
        // or selectors would start colliding across unrelated messages.
        let f = WireFilter::parse("serverName");
        assert!(!f.allows(&step_with_mcp));

        // Scalar fields are not selectors either: `allows` and the summary
        // qualifier share `nested_action_keys`, so a selector can only
        // match something the label is able to name.
        let f = WireFilter::parse("stepIndex");
        assert!(!f.allows(&step_with_mcp));
    }

    #[test]
    fn payload_keys_names_the_message_and_labels_the_empty_case() {
        use super::LoudWirePrinter;

        // The case that matters: on the harness path, envelope and payload
        // keys together must render as the payload alone, using the same
        // `is_envelope_key` filter selection uses. A divergence between
        // what a summary shows and what a selector matches would surface
        // here first.
        assert_eq!(
            LoudWirePrinter::ws_payload_keys(&json!({
                "seqNum": "7",
                "timestampMicros": "1",
                "stepUpdate": {"text": "hi"},
            })),
            "stepUpdate"
        );

        // A real message whose only content is usage — not a bug, so it
        // gets a label rather than a blank detail column. Neutral wording
        // because this renders HTTP bodies too.
        assert_eq!(
            LoudWirePrinter::ws_payload_keys(&json!({"seqNum": "7", "usageUpdate": {"total": {}}})),
            "(no payload keys)"
        );

        // Non-JSON frames arrive as a bare string.
        assert_eq!(
            LoudWirePrinter::payload_keys(&json!("not an object")),
            "(non-object)"
        );

        // A step carrying an action is qualified with it, so summary lines
        // say which action ran and agree with what a nested selector
        // matched on.
        assert_eq!(
            LoudWirePrinter::ws_payload_keys(&json!({
                "seqNum": "3",
                "stepUpdate": {
                    "stepIndex": 2,
                    "text": "List widgets",
                    "mcpTool": {"serverName": "widgets"},
                },
            })),
            "stepUpdate/mcpTool"
        );
        // Scalar-only payloads are unqualified, exactly as before.
        assert_eq!(
            LoudWirePrinter::ws_payload_keys(&json!({
                "trajectoryStateUpdate": {"state": "STATE_FULLY_IDLE", "trajectoryId": "t-0"},
            })),
            "trajectoryStateUpdate"
        );
        // HTTP bodies go through the unqualified path, so a nested object
        // in a response body is not dressed up as an action.
        assert_eq!(
            LoudWirePrinter::payload_keys(&json!({"interaction": {"outputs": {}}})),
            "interaction"
        );
    }

    #[test]
    fn send_frames_and_http_bodies_are_not_dressed_up_as_actions() {
        use super::LoudWirePrinter;

        // An InputEvent arm carrying an object-valued field is still just
        // that arm. Qualifying it would print `questionResponse/response`
        // and collide with `response`, the HTTP category selector.
        let question_response = json!({
            "questionResponse": {"response": {"answers": []}},
        });
        assert_eq!(
            LoudWirePrinter::payload_keys(&question_response),
            "questionResponse"
        );

        // usageMetadata is envelope bookkeeping on the harness wire...
        assert_eq!(
            LoudWirePrinter::ws_payload_keys(&json!({"seqNum": "1", "usageMetadata": {}})),
            "(no payload keys)"
        );
        // ...but a real field on a Gemini HTTP response, so the HTTP path
        // must still name it rather than reporting an empty body.
        assert_eq!(
            LoudWirePrinter::payload_keys(&json!({"usageMetadata": {"totalTokens": 7}})),
            "usageMetadata"
        );

        // Selection has to agree with that labelling, or `LOUD_WIRE=response`
        // would keep a line rendered `questionResponse` — a selector matching
        // nothing the printed label contains.
        let send = WireEvent::WsSend {
            id: 1,
            payload: question_response.clone(),
        };
        assert!(
            !WireFilter::parse("response").allows(&send),
            "a send frame must not be selected by an action nested inside its arm"
        );
        // The arm itself still selects it.
        assert!(WireFilter::parse("questionResponse").allows(&send));
        // And a received frame keeps the nested granularity it was added for.
        assert!(WireFilter::parse("mcpTool").allows(&WireEvent::WsReceive {
            id: 1,
            payload: json!({"stepUpdate": {"mcpTool": {"name": "x"}}}),
        }));

        // Envelope stripping is gated the same way on both sides. A
        // received frame's seqNum neither renders nor selects...
        let received = WireEvent::WsReceive {
            id: 1,
            payload: json!({"seqNum": "7", "stepUpdate": {"text": "hi"}}),
        };
        assert!(!WireFilter::parse("seqNum").allows(&received));
        // ...while on a send it does both, rather than one without the other.
        let send_with_envelope = WireEvent::WsSend {
            id: 1,
            payload: json!({"seqNum": "7", "userInput": {}}),
        };
        assert!(WireFilter::parse("seqNum").allows(&send_with_envelope));
        assert!(
            LoudWirePrinter::payload_keys(&json!({"seqNum": "7", "userInput": {}}))
                .contains("seqNum")
        );
    }

    #[test]
    fn env_inspector_applies_the_filter_from_the_variable() {
        use super::env_inspector;

        // The nextest-per-process claim that used to justify unguarded
        // mutation here was false under `cargo test` — `Client::builder()`
        // reads LOUD_WIRE at construction, so a client built concurrently
        // with this window sees a stray printer (#418). Same guard as the
        // client.rs mutator; it also restores the ambient value on drop.
        let guard = crate::test_subscriber::LoudWireGuard::acquire();

        guard.set("toolCall,summary");
        let printer = env_inspector().expect("LOUD_WIRE set should yield a printer");
        assert!(printer.filter.is_summary());
        assert!(printer.filter.allows(&ws(json!({"toolCall": {}}))));
        assert!(!printer.filter.allows(&request()));

        // The historical spelling still means everything.
        guard.set("1");
        let printer = env_inspector().expect("printer");
        assert_eq!(printer.filter, WireFilter::all());

        guard.unset();
        assert!(
            env_inspector().is_none(),
            "an unset LOUD_WIRE must install nothing"
        );
    }

    #[test]
    fn summary_survives_an_on_value_in_either_order() {
        // Even alongside an "on" value, summary survives — in either order.
        // `summary` is a modifier, so token position must not change what
        // the value means.
        for raw in ["summary,1", "1,summary", "stepUpdate,1,summary"] {
            let f = WireFilter::parse(raw);
            assert!(f.is_summary(), "{raw:?} should keep summary");
            assert!(f.allows(&request()), "{raw:?} should keep everything");
            assert!(
                f.allows(&ws(json!({"toolCall": {}}))),
                "{raw:?}: an \"on\" value overrides narrower selectors"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_events() -> Vec<WireEvent> {
        vec![
            WireEvent::Request {
                id: 1,
                method: "POST".to_string(),
                url: "https://example.com/v1beta/interactions".to_string(),
                body: Some(serde_json::json!({
                    "model": "test-model",
                    "data": "A".repeat(200),
                })),
            },
            WireEvent::Request {
                id: 2,
                method: "GET".to_string(),
                url: "https://example.com/v1beta/interactions/abc".to_string(),
                body: None,
            },
            WireEvent::ResponseStatus { id: 1, status: 200 },
            WireEvent::ResponseStatus { id: 1, status: 500 },
            WireEvent::ResponseBody {
                id: 1,
                body: serde_json::json!({"status": "completed"}),
            },
            WireEvent::ResponseBody {
                id: 1,
                body: serde_json::Value::String("not json".repeat(300)),
            },
            WireEvent::ErrorBody {
                id: 1,
                status: 429,
                body: r#"{"error": {"message": "quota"}}"#.to_string(),
            },
            WireEvent::ErrorBody {
                id: 1,
                status: 503,
                body: "plain text error \u{4e16}\u{754c}".repeat(100),
            },
            WireEvent::SseFrame {
                id: 1,
                event_type: None,
                data: r#"{"event_type": "step.delta"}"#.to_string(),
            },
            WireEvent::SseFrame {
                id: 1,
                event_type: Some("interaction.completed".to_string()),
                data: String::new(),
            },
            WireEvent::SseFrame {
                id: 1,
                event_type: None,
                data: "not json".to_string(),
            },
            WireEvent::UploadStart {
                id: 3,
                file_name: "video.mp4".to_string(),
                mime_type: "video/mp4".to_string(),
                size_bytes: 157_286_400,
            },
            WireEvent::UploadComplete {
                id: 3,
                uri: "https://example.com/files/abc".to_string(),
            },
            WireEvent::HarnessSpawn {
                id: 4,
                path: "/usr/local/bin/localharness".to_string(),
                pid: Some(4242),
            },
            WireEvent::HarnessSpawn {
                id: 4,
                path: "/usr/local/bin/localharness".to_string(),
                pid: None,
            },
            WireEvent::WsSend {
                id: 4,
                payload: serde_json::json!({"userInput": "hello"}),
            },
            WireEvent::WsReceive {
                id: 4,
                payload: serde_json::json!({"stepUpdate": {"textDelta": "hi"}}),
            },
            WireEvent::WsReceive {
                id: 4,
                payload: serde_json::Value::String("not json".to_string()),
            },
            WireEvent::HarnessStderr {
                id: 4,
                line: "harness diagnostic \u{4e16}\u{754c}".repeat(100),
            },
        ]
    }

    #[test]
    fn test_truncate_utf8_short_string() {
        assert_eq!(truncate_utf8("short", 100), "short");
    }

    #[test]
    fn test_truncate_utf8_exact_boundary() {
        let s = "a".repeat(100);
        assert_eq!(truncate_utf8(&s, 100), s);
    }

    #[test]
    fn test_truncate_utf8_ascii() {
        let s = "a".repeat(200);
        let result = truncate_utf8(&s, 100);
        assert_eq!(result.len(), 103); // 100 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_utf8_multibyte_no_panic() {
        // 4-byte emoji straddling the truncation point must not panic and
        // must not be split mid-character.
        let s = "x".repeat(99) + "🎉🎉🎉";
        let result = truncate_utf8(&s, 100);
        assert!(result.ends_with("..."));
        assert!(!result.contains('\u{FFFD}'));
        assert_eq!(&result[..99], &"x".repeat(99));
        // 99 x's, emoji doesn't fit in the last byte, so cut at 99.
        assert_eq!(result.len(), 102); // 99 + "..."

        // Also exercise a string that is entirely multibyte.
        let cjk = "\u{4e16}\u{754c}".repeat(60); // 3 bytes per char, 360 bytes
        let result = truncate_utf8(&cjk, 100);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 103);
        // Must be valid UTF-8 by construction; check boundary integrity.
        assert!(result.is_char_boundary(result.len() - 3));
    }

    #[test]
    fn test_truncate_long_fields_char_boundary_safe() {
        // A "data" field where byte 100 falls inside a multibyte char.
        let payload = "x".repeat(99) + &"🎉".repeat(10);
        let mut value = serde_json::json!({ "data": payload, "text": "🎉".repeat(50) });
        truncate_long_fields(&mut value); // Must not panic.

        let data = value["data"].as_str().unwrap();
        assert!(data.ends_with("..."));
        // Text fields are never truncated.
        assert_eq!(value["text"].as_str().unwrap().chars().count(), 50);
    }

    #[test]
    fn test_truncate_long_fields_nested() {
        let mut value = serde_json::json!({
            "model": "gemini",
            "content": {"data": "C".repeat(150), "signature": "S".repeat(150)},
            "items": [{"data": "D".repeat(150)}],
        });
        truncate_long_fields(&mut value);
        assert!(value["content"]["data"].as_str().unwrap().ends_with("..."));
        assert!(
            value["content"]["signature"]
                .as_str()
                .unwrap()
                .ends_with("...")
        );
        assert!(value["items"][0]["data"].as_str().unwrap().ends_with("..."));
        assert_eq!(value["model"], "gemini");
    }

    #[test]
    fn test_truncate_long_fields_short_values_untouched() {
        let mut value = serde_json::json!({"data": "short", "signature": "sig"});
        truncate_long_fields(&mut value);
        assert_eq!(value["data"], "short");
        assert_eq!(value["signature"], "sig");
    }

    #[test]
    fn test_redact_fields_api_key_fully_redacted() {
        // Short api_key values must be redacted, not merely truncated
        // (truncation leaves keys under the threshold fully intact).
        let mut value = serde_json::json!({
            "tools": [{
                "retrieval": {
                    "exa_ai_search_config": {"api_key": "exa-secret-key"},
                    "parallel_ai_search_config": {"api_key": "par-secret-key"}
                }
            }],
            "api_key": "top-level-secret"
        });
        truncate_long_fields(&mut value);
        let rendered = value.to_string();
        assert!(!rendered.contains("exa-secret-key"));
        assert!(!rendered.contains("par-secret-key"));
        assert!(!rendered.contains("top-level-secret"));
        assert_eq!(
            value["tools"][0]["retrieval"]["exa_ai_search_config"]["api_key"],
            "[REDACTED]"
        );
        assert_eq!(
            value["tools"][0]["retrieval"]["parallel_ai_search_config"]["api_key"],
            "[REDACTED]"
        );
        assert_eq!(value["api_key"], "[REDACTED]");
    }

    #[test]
    fn test_truncate_fields_with_structured_values_still_redact_nested_secrets() {
        // A `data` (or `signature`) key can hold an object or array — e.g.
        // an Evergreen Unknown variant preserving raw JSON under `data`.
        // The walk must recurse into those subtrees so nested secrets are
        // still redacted, not skip them because the value is not a string.
        let mut value = serde_json::json!({
            "data": {"api_key": "nested-secret", "note": "kept"},
            "wrapper": {"data": [{"new_signing_secret": "whsec_nested"}]},
        });
        truncate_long_fields(&mut value);
        let rendered = value.to_string();
        assert!(!rendered.contains("nested-secret"), "leaked: {rendered}");
        assert!(!rendered.contains("whsec_nested"), "leaked: {rendered}");
        assert_eq!(value["data"]["api_key"], "[REDACTED]");
        assert_eq!(value["data"]["note"], "kept");
        assert_eq!(
            value["wrapper"]["data"][0]["new_signing_secret"],
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_fields_null_api_key_left_null() {
        // An absent/null key is not a secret; keep the JSON shape honest.
        let mut value = serde_json::json!({"api_key": null});
        truncate_long_fields(&mut value);
        assert!(value["api_key"].is_null());
    }

    #[test]
    fn test_redact_fields_webhook_signing_secrets() {
        // create_webhook returns new_signing_secret; rotate returns secret.
        // Both are one-time values and must never reach inspector output.
        let mut value = serde_json::json!({
            "id": "wh1bare0pq",
            "new_signing_secret": "whsec_create-secret",
            "secret": "whsec_rotated-secret"
        });
        truncate_long_fields(&mut value);
        let rendered = value.to_string();
        assert!(!rendered.contains("whsec_"), "secret leaked: {rendered}");
        assert_eq!(value["new_signing_secret"], "[REDACTED]");
        assert_eq!(value["secret"], "[REDACTED]");
        assert_eq!(value["id"], "wh1bare0pq");
    }

    #[test]
    fn test_tracing_forwarder_body_rendering_redacts() {
        // TracingForwarder must apply the same redaction guarantees as
        // LoudWirePrinter to JSON bodies and raw string payloads.
        let body = serde_json::json!({"secret": "whsec_x", "api_key": "k"});
        let rendered = redacted_body_string(&body);
        assert!(!rendered.contains("whsec_x"));
        assert!(!rendered.contains("\"k\""));

        let raw_json = r#"{"new_signing_secret":"whsec_y"}"#;
        assert!(!redacted_raw_string(raw_json).contains("whsec_y"));

        // Non-JSON payloads pass through unchanged.
        assert_eq!(redacted_raw_string("plain text"), "plain text");
    }

    #[test]
    fn test_loud_wire_printer_smoke_all_variants() {
        // No assertions on the output itself (it goes to stderr); this
        // guards against panics in formatting, including UTF-8 truncation.
        let printer = LoudWirePrinter::new();
        for event in sample_events() {
            printer.on_event(&event);
        }
    }

    #[test]
    fn test_tracing_forwarder_smoke_all_variants() {
        let forwarder = TracingForwarder::new();
        for event in sample_events() {
            forwarder.on_event(&event);
        }
    }

    #[test]
    fn test_tracing_forwarder_emits_to_wire_target() {
        let targets = crate::test_subscriber::capture_targets(|| {
            TracingForwarder::new().on_event(&WireEvent::ResponseStatus { id: 7, status: 200 });
        });
        assert_eq!(targets.as_slice(), [TRACING_TARGET]);
    }

    #[test]
    fn test_wire_event_id_accessor() {
        for event in sample_events() {
            assert!(event.id() > 0);
        }
    }

    #[test]
    fn test_wire_event_serializes_with_kind_tag() {
        let event = WireEvent::ResponseStatus { id: 4, status: 200 };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "response_status");
        assert_eq!(json["id"], 4);
        assert_eq!(json["status"], 200);

        let event = WireEvent::SseFrame {
            id: 1,
            event_type: Some("interaction.completed".to_string()),
            data: "{}".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "sse_frame");
        assert_eq!(json["event_type"], "interaction.completed");

        let event = WireEvent::HarnessStderr {
            id: 9,
            line: "harness log".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "harness_stderr");
        assert_eq!(json["id"], 9);
        assert_eq!(json["line"], "harness log");

        let event = WireEvent::WsSend {
            id: 9,
            payload: serde_json::json!({"userInput": "hi"}),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "ws_send");
        assert_eq!(json["payload"]["userInput"], "hi");
    }
}
