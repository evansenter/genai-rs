use super::{WireEvent, WireInspector, truncate_long_fields};
use std::borrow::Cow;

// =============================================================================
// TracingForwarder
// =============================================================================

/// Forwards wire events to the [`tracing`] ecosystem.
///
/// Events are emitted at `DEBUG` level to the [`TRACING_TARGET`](super::TRACING_TARGET)
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
/// truncation guarantees as [`LoudWirePrinter`](super::LoudWirePrinter) (secret fields replaced,
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
#[path = "tracing_forwarder_tests.rs"]
mod tests;
