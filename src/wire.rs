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
            | Self::UploadComplete { id, .. } => *id,
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

/// Truncate long base64-encoded fields in a JSON value.
///
/// Walks the JSON tree and truncates `"data"` and `"signature"` fields
/// that contain strings longer than 100 bytes. Text content and
/// other fields are preserved in full.
fn truncate_long_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if TRUNCATE_FIELDS.contains(&key.as_str()) {
                    if let serde_json::Value::String(s) = val
                        && s.len() > TRUNCATE_THRESHOLD
                    {
                        *s = truncate_utf8(s, TRUNCATE_THRESHOLD).into_owned();
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
#[derive(Debug, Clone, Copy, Default)]
pub struct LoudWirePrinter;

impl LoudWirePrinter {
    /// Creates a new printer.
    #[must_use]
    pub const fn new() -> Self {
        Self
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
}

impl WireInspector for LoudWirePrinter {
    fn on_event(&self, event: &WireEvent) {
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
                let body = body.as_ref().map(ToString::to_string);
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
                    body = %body,
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
                    body = %body,
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
                    data = %data,
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
                    "model": "gemini-3-flash-preview",
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
                data: r#"{"event_type": "content.delta"}"#.to_string(),
            },
            WireEvent::SseFrame {
                id: 1,
                event_type: Some("interaction.complete".to_string()),
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
        use std::sync::{Arc, Mutex};
        use tracing::span;

        /// Minimal subscriber that records the target of every event.
        struct Recorder {
            targets: Arc<Mutex<Vec<String>>>,
        }

        impl tracing::Subscriber for Recorder {
            fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
                span::Id::from_u64(1)
            }
            fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}
            fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                self.targets
                    .lock()
                    .unwrap()
                    .push(event.metadata().target().to_string());
            }
            fn enter(&self, _span: &span::Id) {}
            fn exit(&self, _span: &span::Id) {}
        }

        let targets = Arc::new(Mutex::new(Vec::new()));
        let recorder = Recorder {
            targets: Arc::clone(&targets),
        };

        tracing::subscriber::with_default(recorder, || {
            TracingForwarder::new().on_event(&WireEvent::ResponseStatus { id: 7, status: 200 });
        });

        let targets = targets.lock().unwrap();
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
            event_type: Some("interaction.complete".to_string()),
            data: "{}".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "sse_frame");
        assert_eq!(json["event_type"], "interaction.complete");
    }
}
