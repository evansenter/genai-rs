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

/// How long one connect attempt (the TCP connect and the WebSocket upgrade)
/// may take before it counts as failed. Both go to a harness on localhost
/// that has already written its port, so a healthy one answers in
/// milliseconds; 5 s leaves room for a loaded CI runner. A harness that
/// accepts the connection but never answers the upgrade then fails
/// `spawn()` after about 26.5 s (5 attempts and the backoff between them)
/// instead of holding it forever.
const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// the `x-goog-api-key` header, retrying with exponential backoff. An
    /// attempt that takes over [`CONNECT_ATTEMPT_TIMEOUT`] counts as failed.
    pub(crate) async fn connect(
        port: i32,
        api_key: &str,
        wire: WireContext,
    ) -> Result<Self, AntigravityError> {
        let url = format!("ws://localhost:{port}/");
        let uri: Uri = url.parse().map_err(|e| AntigravityError::WebSocket {
            message: format!("invalid harness URL {url}"),
            source: Box::new(e),
        })?;
        let request = ClientRequestBuilder::new(uri).with_header("x-goog-api-key", api_key);

        let mut attempt = 0;
        loop {
            let connected = tokio::time::timeout(
                CONNECT_ATTEMPT_TIMEOUT,
                tokio_tungstenite::connect_async(request.clone()),
            )
            .await
            .unwrap_or_else(|_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("no WebSocket upgrade within {CONNECT_ATTEMPT_TIMEOUT:?}"),
                )
                .into())
            });
            match connected {
                Ok((ws, _response)) => {
                    let (sink, stream) = ws.split();
                    return Ok(Self {
                        sink: Arc::new(tokio::sync::Mutex::new(sink)),
                        stream,
                        wire,
                    });
                }
                Err(e) if attempt + 1 < CONNECT_ATTEMPTS => {
                    tracing::debug!(
                        "harness WebSocket connect attempt {} failed: {e}",
                        attempt + 1
                    );
                    tokio::time::sleep(CONNECT_BASE_DELAY * 2u32.pow(attempt)).await;
                    attempt += 1;
                }
                Err(e) => {
                    return Err(AntigravityError::WebSocket {
                        message: format!(
                            "failed to connect to harness WebSocket on port {port} after \
                             {CONNECT_ATTEMPTS} attempts"
                        ),
                        source: Box::new(e),
                    });
                }
            }
        }
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
        send_json(&self.sink, &self.wire, encode(event)?).await
    }

    /// Sends any other message as proto-JSON text (the
    /// `InitializeConversationEvent`, which is not an `InputEvent`).
    pub(crate) async fn send_raw(
        &self,
        message: &impl serde::Serialize,
    ) -> Result<(), AntigravityError> {
        send_json(&self.sink, &self.wire, encode(message)?).await
    }

    /// Reads the next [`OutputEvent`]. Returns `Ok(None)` when the harness
    /// closes the connection.
    ///
    /// Non-text frames are skipped (tungstenite answers pings internally);
    /// an unparsable text frame is an [`AntigravityError::Protocol`] error
    /// with the parse error as its source, since every harness message is
    /// proto-JSON.
    pub(crate) async fn next_event(&mut self) -> Result<Option<OutputEvent>, AntigravityError> {
        loop {
            let Some(message) = self.stream.next().await else {
                return Ok(None);
            };
            let message = message.map_err(|e| AntigravityError::WebSocket {
                message: "read failed".to_string(),
                source: Box::new(e),
            })?;
            match message {
                Message::Text(text) => {
                    self.wire.emit(|| WireEvent::WsReceive {
                        id: self.wire.id(),
                        payload: serde_json::from_str(&text)
                            .unwrap_or_else(|_| serde_json::Value::String(text.to_string())),
                    });
                    return serde_json::from_str(&text).map(Some).map_err(|e| {
                        AntigravityError::protocol_from(
                            "could not parse a message from the harness",
                            e,
                        )
                    });
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

/// A message for the harness as a JSON value.
fn encode(message: &impl serde::Serialize) -> Result<serde_json::Value, AntigravityError> {
    serde_json::to_value(message).map_err(|e| {
        AntigravityError::protocol_from("could not encode a message for the harness", e)
    })
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
    let text = value.to_string();
    sink.lock()
        .await
        .send(Message::Text(text.into()))
        .await
        .map_err(|e| AntigravityError::WebSocket {
            message: "send failed".to_string(),
            source: Box::new(e),
        })
}

/// Redacts credential fields in the *inspection copy* of an outgoing
/// payload (the harness receives the original). Credentials in the
/// protocol: the Gemini `apiKey` inside model endpoint configs, plus the
/// pure credential-carrier maps — MCP stdio `env`, MCP HTTP `headers`, and
/// model-endpoint `httpHeaders` — whose values are API keys and
/// `Authorization` tokens by design.
fn redact_for_inspection(mut value: serde_json::Value) -> serde_json::Value {
    redact_keys(&mut value);
    value
}

/// Map-valued keys whose every entry is treated as a credential.
const REDACT_MAP_KEYS: &[&str] = &["env", "headers", "httpHeaders"];

fn redact_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, entry) in map.iter_mut() {
                if key == "apiKey" && entry.is_string() {
                    *entry = serde_json::Value::String("[REDACTED]".to_string());
                } else if REDACT_MAP_KEYS.contains(&key.as_str()) {
                    if let serde_json::Value::Object(entries) = entry {
                        for value in entries.values_mut() {
                            if value.is_string() {
                                *value = serde_json::Value::String("[REDACTED]".to_string());
                            }
                        }
                    }
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
        send_json(&self.sink, &self.wire, encode(event)?).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test(start_paused = true)]
    async fn test_failed_connect_keeps_the_transport_error_as_its_source() {
        // A port nothing listens on: bind one, then free it.
        let listener = std::net::TcpListener::bind("localhost:0").expect("bind a local port");
        let port = i32::from(listener.local_addr().expect("local address").port());
        drop(listener);

        let error = Session::connect(port, "token", WireContext::new(Vec::new()))
            .await
            .expect_err("nothing listens on the port");
        let AntigravityError::WebSocket { message, source } = &error else {
            panic!("expected a WebSocket error: {error:?}");
        };
        assert!(message.contains(&format!("port {port}")), "{message}");
        assert!(
            source
                .downcast_ref::<tokio_tungstenite::tungstenite::Error>()
                .is_some(),
            "{source:?}"
        );
        let chained = std::error::Error::source(&error).expect("source() is the transport error");
        assert_eq!(chained.to_string(), source.to_string());
        assert!(!error.to_string().contains(&source.to_string()), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn test_a_connect_the_harness_never_answers_times_out_each_attempt() {
        // Accepts TCP (the kernel completes the handshake) but never answers
        // the WebSocket upgrade: each attempt used to wait forever.
        let listener = std::net::TcpListener::bind("localhost:0").expect("bind a local port");
        let port = i32::from(listener.local_addr().expect("local address").port());

        let started = tokio::time::Instant::now();
        let connect = Session::connect(port, "token", WireContext::new(Vec::new()));
        let error = tokio::time::timeout(Duration::from_secs(120), connect)
            .await
            .expect("connect gives up rather than hanging")
            .expect_err("nothing answers the upgrade");
        let AntigravityError::WebSocket { message, source } = &error else {
            panic!("expected a WebSocket error: {error:?}");
        };
        assert!(
            message.contains(&format!("after {CONNECT_ATTEMPTS} attempts")),
            "{message}"
        );
        let Some(tokio_tungstenite::tungstenite::Error::Io(io)) = source.downcast_ref() else {
            panic!("the last attempt's timeout is the source: {source:?}");
        };
        assert_eq!(io.kind(), std::io::ErrorKind::TimedOut, "{io}");
        // Every attempt waited its timeout, then the backoff.
        let backoff: Duration = (0..CONNECT_ATTEMPTS - 1)
            .map(|n| CONNECT_BASE_DELAY * 2u32.pow(n))
            .sum();
        let elapsed = started.elapsed();
        assert!(
            elapsed >= CONNECT_ATTEMPT_TIMEOUT * CONNECT_ATTEMPTS + backoff,
            "gave up after {elapsed:?}"
        );
        assert!(
            elapsed < CONNECT_ATTEMPT_TIMEOUT * (CONNECT_ATTEMPTS + 1) + backoff,
            "{elapsed:?}"
        );
        drop(listener);
    }

    #[tokio::test]
    async fn test_an_unparsable_harness_frame_is_a_protocol_error_with_its_cause() {
        let listener = tokio::net::TcpListener::bind("localhost:0")
            .await
            .expect("bind a local port");
        let port = i32::from(listener.local_addr().expect("local address").port());
        let harness = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let mut ws = tokio_tungstenite::accept_async(socket)
                .await
                .expect("upgrade");
            ws.send(Message::Text("not proto-json".into()))
                .await
                .expect("send");
            ws
        });
        let mut session = Session::connect(port, "token", WireContext::new(Vec::new()))
            .await
            .expect("connect");
        let error = session
            .next_event()
            .await
            .expect_err("the frame is not JSON");
        let AntigravityError::Protocol { message, source } = &error else {
            panic!("expected a protocol error: {error:?}");
        };
        assert!(message.contains("could not parse"), "{message}");
        let cause = source.as_ref().expect("the parse error is the source");
        assert!(
            cause.downcast_ref::<serde_json::Error>().is_some(),
            "{cause:?}"
        );
        let chained = std::error::Error::source(&error).expect("source() is the parse error");
        assert_eq!(chained.to_string(), cause.to_string());
        drop(harness.await);
    }

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

    #[test]
    fn test_redaction_masks_mcp_and_endpoint_credentials() {
        let payload = json!({
            "config": {
                "models": [
                    {"geminiApiEndpoint": {
                        "apiKey": "secret-1",
                        "httpHeaders": {"x-custom-auth": "secret-2"}
                    }}
                ],
                "mcpServers": [
                    {"stdio": {
                        "command": "my-mcp",
                        "env": {"GITHUB_TOKEN": "secret-3", "MODE": "fast"}
                    }},
                    {"http": {
                        "url": "https://internal/mcp",
                        "headers": {"Authorization": "Bearer secret-4"}
                    }}
                ]
            }
        });
        let redacted = redact_for_inspection(payload);
        let text = redacted.to_string();
        assert!(!text.contains("secret-"), "credential leaked: {text}");
        // Structure survives: command/url untouched, header keys visible.
        assert_eq!(
            redacted["config"]["mcpServers"][0]["stdio"]["command"],
            "my-mcp"
        );
        assert_eq!(
            redacted["config"]["mcpServers"][1]["http"]["url"],
            "https://internal/mcp"
        );
        assert_eq!(
            redacted["config"]["mcpServers"][1]["http"]["headers"]["Authorization"],
            "[REDACTED]"
        );
        // Non-string map values (none today) and non-credential env values are
        // both redacted uniformly — env/headers are credential carriers by
        // design, so over-redaction is the correct trade.
        assert_eq!(
            redacted["config"]["mcpServers"][0]["stdio"]["env"]["MODE"],
            "[REDACTED]"
        );
    }
}
