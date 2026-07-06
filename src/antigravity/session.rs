//! WebSocket transport to the harness: connect (with retry), proto-JSON
//! send/receive, and wire inspection.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::ClientRequestBuilder;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::AntigravityError;
use super::protocol::{InputEvent, OutputEvent};
use crate::wire::{WireEvent, WireInspector};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Connection retry schedule: 5 attempts with 0.1s * 2^n backoff (mirrors
/// the reference SDK — the harness may need a moment to start listening
/// after writing its `OutputConfig`).
const CONNECT_ATTEMPTS: u32 = 5;
const CONNECT_BASE_DELAY: Duration = Duration::from_millis(100);

/// How long to wait for the WebSocket close handshake during shutdown (the
/// harness does not reply with a close frame).
const CLOSE_TIMEOUT: Duration = Duration::from_millis(500);

/// Session-scoped wire-inspection context.
///
/// All events of one harness session (spawn, WS traffic, stderr) share a
/// correlation id drawn from a process-global counter.
#[derive(Clone)]
pub(crate) struct WireContext {
    inspectors: Arc<[Arc<dyn WireInspector>]>,
    session_id: u64,
}

impl std::fmt::Debug for WireContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WireContext")
            .field("inspectors", &self.inspectors.len())
            .field("session_id", &self.session_id)
            .finish()
    }
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

impl WireContext {
    pub(crate) fn new(inspectors: Vec<Arc<dyn WireInspector>>) -> Self {
        Self {
            inspectors: inspectors.into(),
            session_id: SESSION_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// The correlation id shared by all events of this session.
    pub(crate) fn id(&self) -> u64 {
        self.session_id
    }

    /// Fans an event out to the inspectors. The closure is only invoked
    /// when at least one inspector is installed, so event construction is
    /// free in the default configuration.
    pub(crate) fn emit(&self, make: impl FnOnce() -> WireEvent) {
        if self.inspectors.is_empty() {
            return;
        }
        let event = make();
        for inspector in self.inspectors.iter() {
            inspector.on_event(&event);
        }
    }
}

/// A live WebSocket session with the harness.
///
/// The sink half is shared behind a mutex so that [`CancelHandle`]s
/// (`halt_request`) can write concurrently with a turn in progress; the
/// stream half is read sequentially by the agent's event loop.
///
/// [`CancelHandle`]: super::CancelHandle
pub(crate) struct Session {
    sink: Arc<tokio::sync::Mutex<SplitSink<WsStream, Message>>>,
    stream: SplitStream<WsStream>,
    wire: WireContext,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("wire", &self.wire)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Connects to `ws://localhost:{port}/` with the per-process token in
    /// the `x-goog-api-key` header, retrying with exponential backoff.
    pub(crate) async fn connect(
        port: i32,
        api_key: &str,
        wire: WireContext,
    ) -> Result<Self, AntigravityError> {
        let uri: Uri = format!("ws://localhost:{port}/")
            .parse()
            .map_err(|e| AntigravityError::WebSocket(format!("invalid harness URL: {e}")))?;
        let request = ClientRequestBuilder::new(uri).with_header("x-goog-api-key", api_key);

        let mut last_error = None;
        for attempt in 0..CONNECT_ATTEMPTS {
            match tokio_tungstenite::connect_async(request.clone()).await {
                Ok((ws, _response)) => {
                    let (sink, stream) = ws.split();
                    return Ok(Self {
                        sink: Arc::new(tokio::sync::Mutex::new(sink)),
                        stream,
                        wire,
                    });
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt + 1 < CONNECT_ATTEMPTS {
                        tokio::time::sleep(CONNECT_BASE_DELAY * 2u32.pow(attempt)).await;
                    }
                }
            }
        }
        Err(AntigravityError::WebSocket(format!(
            "failed to connect to harness WebSocket on port {port} after {CONNECT_ATTEMPTS} \
             attempts: {}",
            last_error.map_or_else(|| "unknown error".to_string(), |e| e.to_string())
        )))
    }

    /// A cloneable handle to the sink half, for out-of-band sends
    /// (`halt_request`) while a turn is being driven.
    pub(crate) fn sink_handle(&self) -> SinkHandle {
        SinkHandle {
            sink: Arc::clone(&self.sink),
            wire: self.wire.clone(),
        }
    }

    /// Sends one [`InputEvent`] as proto-JSON text.
    pub(crate) async fn send(&self, event: &InputEvent) -> Result<(), AntigravityError> {
        send_json(&self.sink, &self.wire, serde_json::to_value(event)?).await
    }

    /// Sends a raw JSON value (used for the `InitializeConversationEvent`,
    /// which is not an `InputEvent`).
    pub(crate) async fn send_raw(&self, value: serde_json::Value) -> Result<(), AntigravityError> {
        send_json(&self.sink, &self.wire, value).await
    }

    /// Reads the next [`OutputEvent`]. Returns `Ok(None)` when the harness
    /// closes the connection.
    ///
    /// Non-text frames are skipped (tungstenite answers pings internally);
    /// unparseable text frames are an error, since every harness message is
    /// proto-JSON.
    pub(crate) async fn next_event(&mut self) -> Result<Option<OutputEvent>, AntigravityError> {
        loop {
            let Some(message) = self.stream.next().await else {
                return Ok(None);
            };
            let message = message.map_err(|e| AntigravityError::WebSocket(e.to_string()))?;
            match message {
                Message::Text(text) => {
                    self.wire.emit(|| WireEvent::WsReceive {
                        id: self.wire.id(),
                        payload: serde_json::from_str(&text)
                            .unwrap_or_else(|_| serde_json::Value::String(text.to_string())),
                    });
                    let event: OutputEvent = serde_json::from_str(&text)?;
                    return Ok(Some(event));
                }
                Message::Close(_) => return Ok(None),
                _ => {
                    // Ping/pong/binary frames carry no protocol events.
                }
            }
        }
    }

    /// Initiates the close handshake, bounded by [`CLOSE_TIMEOUT`].
    pub(crate) async fn close(&self) {
        let mut sink = self.sink.lock().await;
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, sink.send(Message::Close(None))).await;
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, sink.flush()).await;
    }
}

async fn send_json(
    sink: &tokio::sync::Mutex<SplitSink<WsStream, Message>>,
    wire: &WireContext,
    value: serde_json::Value,
) -> Result<(), AntigravityError> {
    wire.emit(|| WireEvent::WsSend {
        id: wire.id(),
        payload: redact_for_inspection(value.clone()),
    });
    let text = serde_json::to_string(&value)?;
    sink.lock()
        .await
        .send(Message::Text(text.into()))
        .await
        .map_err(|e| AntigravityError::WebSocket(format!("send failed: {e}")))
}

/// Redacts credential fields in the *inspection copy* of an outgoing
/// payload (the harness receives the original). Today the only credential
/// in the protocol is the Gemini `apiKey` inside model endpoint configs.
fn redact_for_inspection(mut value: serde_json::Value) -> serde_json::Value {
    redact_keys(&mut value);
    value
}

fn redact_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if key == "apiKey" && entry.is_string() {
                    *entry = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_keys(entry);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items.iter_mut() {
                redact_keys(item);
            }
        }
        _ => {}
    }
}

/// Cloneable write-only handle used by [`CancelHandle`](super::CancelHandle).
#[derive(Clone)]
pub(crate) struct SinkHandle {
    sink: Arc<tokio::sync::Mutex<SplitSink<WsStream, Message>>>,
    wire: WireContext,
}

impl std::fmt::Debug for SinkHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SinkHandle").finish_non_exhaustive()
    }
}

impl SinkHandle {
    pub(crate) async fn send(&self, event: &InputEvent) -> Result<(), AntigravityError> {
        send_json(&self.sink, &self.wire, serde_json::to_value(event)?).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_redaction_masks_api_keys_everywhere() {
        let payload = json!({
            "config": {
                "models": [
                    {"geminiApiEndpoint": {"apiKey": "secret-1"}},
                    {"vertexEndpoint": {"apiKey": "secret-2"}}
                ],
                "tools": [{"name": "t"}]
            }
        });
        let redacted = redact_for_inspection(payload);
        assert_eq!(
            redacted["config"]["models"][0]["geminiApiEndpoint"]["apiKey"],
            "[REDACTED]"
        );
        assert_eq!(
            redacted["config"]["models"][1]["vertexEndpoint"]["apiKey"],
            "[REDACTED]"
        );
        // Non-credential content is untouched.
        assert_eq!(redacted["config"]["tools"][0]["name"], "t");
        assert!(!redacted.to_string().contains("secret-"));
    }

    #[test]
    fn test_redaction_leaves_plain_payloads_alone() {
        let payload = json!({"userInput": "what is an apiKey?"});
        assert_eq!(redact_for_inspection(payload.clone()), payload);
    }
}
