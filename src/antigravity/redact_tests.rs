use std::collections::BTreeMap;

use crate::antigravity::handshake::{InputConfig, OutputConfig};
use crate::antigravity::protocol::{
    GeminiApiEndpoint, HarnessConfig, InitializeConversationEvent, McpHttpTransport,
    McpServerConfig, McpStdioTransport, ModelConfig, VertexEndpoint,
};
use crate::antigravity::session::{Session, WireContext};
use crate::antigravity::{AntigravityAgent, McpServer};
use crate::redact::REDACTED;
use crate::wire::{WireEvent, WireInspector};
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

/// The secret every value below holds; no `Debug` output may contain it.
const SECRET: &str = "sentinel-secret-5150";
/// A secret's name, which `Debug` keeps.
const NAME: &str = "X-Sentinel-Name";

fn secrets() -> BTreeMap<String, String> {
    BTreeMap::from([(NAME.to_string(), SECRET.to_string())])
}

/// Asserts `debug` hides the secret and keeps its name.
fn assert_redacted(what: &str, debug: &str, keeps_name: bool) {
    assert!(
        !debug.contains(SECRET),
        "{what}'s Debug prints the secret: {debug}"
    );
    assert!(
        debug.contains(REDACTED),
        "{what}'s Debug marks no redaction: {debug}"
    );
    if keeps_name {
        assert!(
            debug.contains(NAME),
            "{what}'s Debug drops the name: {debug}"
        );
    }
}

fn gemini_endpoint() -> GeminiApiEndpoint {
    GeminiApiEndpoint {
        base_url: Some("http://localhost:1".to_string()),
        http_headers: secrets(),
        api_key: Some(SECRET.to_string()),
        options: None,
    }
}

fn mcp_servers() -> Vec<McpServerConfig> {
    vec![
        McpServerConfig {
            name: Some("stdio".to_string()),
            stdio: Some(McpStdioTransport {
                command: Some("server".to_string()),
                args: Vec::new(),
                env: secrets(),
            }),
            ..Default::default()
        },
        McpServerConfig {
            name: Some("http".to_string()),
            http: Some(McpHttpTransport {
                url: Some("http://localhost:2/mcp".to_string()),
                headers: secrets(),
            }),
            ..Default::default()
        },
    ]
}

#[test]
fn test_debug_never_prints_a_secret() {
    let vertex = VertexEndpoint {
        http_headers: secrets(),
        project: Some("project".to_string()),
        ..Default::default()
    };
    let config = HarnessConfig {
        models: vec![
            ModelConfig {
                gemini_api_endpoint: Some(gemini_endpoint()),
                ..Default::default()
            },
            ModelConfig {
                vertex_endpoint: Some(vertex.clone()),
                ..Default::default()
            },
        ],
        mcp_servers: mcp_servers(),
        ..Default::default()
    };
    let builder = AntigravityAgent::builder()
        .with_api_key(SECRET)
        .add_model_header(NAME, SECRET)
        .add_harness_env(NAME, SECRET)
        .add_mcp_server(McpServer::stdio("server", ["--flag"]).add_env(NAME, SECRET));
    let input = InputConfig {
        env: secrets(),
        ..Default::default()
    };
    let output = OutputConfig {
        port: 1,
        api_key: SECRET.to_string(),
    };

    let cases = [
        (
            "GeminiApiEndpoint",
            format!("{:?}", gemini_endpoint()),
            true,
        ),
        ("VertexEndpoint", format!("{vertex:?}"), true),
        (
            "McpStdioTransport",
            format!("{:?}", mcp_servers()[0].stdio),
            true,
        ),
        (
            "McpHttpTransport",
            format!("{:?}", mcp_servers()[1].http),
            true,
        ),
        ("HarnessConfig", format!("{config:?}"), true),
        (
            "InitializeConversationEvent",
            format!(
                "{:?}",
                InitializeConversationEvent {
                    config: Some(config.clone()),
                }
            ),
            true,
        ),
        (
            "McpServer (stdio)",
            format!(
                "{:?}",
                McpServer::stdio("server", ["--flag"]).add_env(NAME, SECRET)
            ),
            true,
        ),
        (
            "McpServer (http)",
            format!(
                "{:?}",
                McpServer::http("http://localhost:2/mcp").add_header(NAME, SECRET)
            ),
            true,
        ),
        ("AgentBuilder", format!("{builder:?}"), true),
        ("InputConfig", format!("{input:?}"), true),
        ("OutputConfig", format!("{output:?}"), false),
    ];
    for (what, debug, keeps_name) in cases {
        assert_redacted(what, &debug, keeps_name);
    }
}

#[test]
fn test_an_unset_secret_prints_as_unset() {
    let endpoint = GeminiApiEndpoint::default();
    let debug = format!("{endpoint:?}");
    assert!(debug.contains("api_key: None"), "{debug}");
    assert!(debug.contains("http_headers: {}"), "{debug}");
}

/// Records every wire event.
#[derive(Default)]
struct Recorder(Mutex<Vec<WireEvent>>);

impl WireInspector for Recorder {
    fn on_event(&self, event: &WireEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

/// An outgoing message reaches the inspectors through the wire's own walk,
/// and the harness gets the original (D-067). A message the harness sends
/// back reaches them as received, as an HTTP response body does, and its
/// `Debug` redacts the same keys.
#[tokio::test]
async fn test_a_session_redacts_what_its_inspectors_see() {
    let listener = tokio::net::TcpListener::bind("localhost:0")
        .await
        .expect("bind a local port");
    let port = i32::from(listener.local_addr().expect("local address").port());
    let reply = serde_json::json!({"echo": {"apiKey": SECRET, "httpHeaders": {NAME: SECRET}}});
    let harness = tokio::spawn({
        let reply = reply.to_string();
        async move {
            let (socket, _) = listener.accept().await.expect("accept");
            let mut ws = tokio_tungstenite::accept_async(socket)
                .await
                .expect("upgrade");
            let Some(Ok(Message::Text(received))) = ws.next().await else {
                panic!("no text frame from the client");
            };
            ws.send(Message::Text(reply.into())).await.expect("reply");
            received.to_string()
        }
    });
    let recorder = Arc::new(Recorder::default());
    let mut session = Session::connect(
        port,
        "token",
        WireContext::new(vec![recorder.clone() as Arc<dyn WireInspector>]),
    )
    .await
    .expect("connect");
    let init = InitializeConversationEvent {
        config: Some(HarnessConfig {
            models: vec![ModelConfig {
                gemini_api_endpoint: Some(gemini_endpoint()),
                ..Default::default()
            }],
            mcp_servers: mcp_servers(),
            ..Default::default()
        }),
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    session
        .send_before(&init, deadline)
        .await
        .expect("within the deadline")
        .expect("sent");
    // The reply is no harness message, so it does not parse; it was
    // inspected before that.
    let _ = session.next_event().await;
    let received = harness.await.expect("the harness side ends");
    assert!(received.contains(SECRET), "the harness got {received}");

    let events = recorder.0.lock().unwrap();
    let [
        WireEvent::WsSend { payload: sent, .. },
        WireEvent::WsReceive { payload, .. },
    ] = events.as_slice()
    else {
        panic!("expected one send and one receive: {events:?}");
    };
    assert_redacted("WsSend's payload", &sent.to_string(), true);
    assert_eq!(payload, &reply, "a receive reaches inspectors as received");
    assert_redacted("WsReceive's Debug", &format!("{:?}", events[1]), true);
}
