//! Offline tests against a local HTTP stub, via `ClientBuilder::with_base_url`.
//!
//! These cover client behavior that only shows on the wire — request shapes,
//! error mapping, SSE framing, and the auto-function loop — without an API
//! key. The stub is a few dozen lines of tokio rather than a mock-server
//! crate: it only needs to record requests and replay canned responses.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use genai_rs::{
    AutoFunctionStreamChunk, CallableFunction, Client, FunctionDeclaration, FunctionError,
    GenaiError, StreamChunk, ToolService,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// =============================================================================
// Stub server
// =============================================================================

/// One request as the stub received it.
#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    /// Path and query, e.g. `/v1beta/interactions?alt=sse`.
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("request body should be JSON")
    }
}

/// A canned response. Body parts are written in order, each after its delay,
/// and the connection is closed afterwards, so a body without a
/// `Content-Length` ends at EOF.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    delay: Duration,
    parts: Vec<(Duration, Vec<u8>)>,
    content_length: bool,
}

impl Reply {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            delay: Duration::ZERO,
            parts: vec![(Duration::ZERO, body.to_string().into_bytes())],
            content_length: true,
        }
    }

    fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "text/html".into())],
            delay: Duration::ZERO,
            parts: vec![(Duration::ZERO, body.as_bytes().to_vec())],
            content_length: true,
        }
    }

    /// A close-delimited `text/event-stream` body, sent as the given chunks.
    fn sse(chunks: &[&str]) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            delay: Duration::ZERO,
            parts: chunks
                .iter()
                .map(|c| (Duration::from_millis(5), c.as_bytes().to_vec()))
                .collect(),
            content_length: false,
        }
    }

    /// `{base}` in a value is replaced with the stub's base URL.
    fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

type Handler = dyn Fn(&Recorded, usize) -> Reply + Send + Sync;

struct Stub {
    base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Stub {
    /// Starts a stub answering request `n` (0-based) with `handler(req, n)`.
    async fn start(handler: impl Fn(&Recorded, usize) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let counter = Arc::new(AtomicUsize::new(0));

        let (task_requests, task_base) = (requests.clone(), base_url.clone());
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let (requests, handler, base, counter) = (
                    task_requests.clone(),
                    handler.clone(),
                    task_base.clone(),
                    counter.clone(),
                );
                tokio::spawn(async move {
                    serve(socket, requests, handler, base, counter).await;
                });
            }
        });

        Self { base_url, requests }
    }

    /// Answers requests with `replies` in order; any extra request gets a 500.
    async fn replying(replies: Vec<Reply>) -> Self {
        let replies = Mutex::new(replies.into_iter().map(Some).collect::<Vec<_>>());
        Self::start(move |_, n| {
            replies
                .lock()
                .unwrap()
                .get_mut(n)
                .and_then(Option::take)
                .unwrap_or_else(|| Reply::text(500, "unexpected request"))
        })
        .await
    }

    fn client(&self) -> Client {
        Client::builder("test-key".to_string())
            .with_base_url(&self.base_url)
            .build()
            .unwrap()
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

async fn serve(
    mut socket: TcpStream,
    requests: Arc<Mutex<Vec<Recorded>>>,
    handler: Arc<Handler>,
    base: String,
    counter: Arc<AtomicUsize>,
) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    let n = counter.fetch_add(1, Ordering::SeqCst);
    requests.lock().unwrap().push(request.clone());
    let reply = handler(&request, n);

    tokio::time::sleep(reply.delay).await;
    let mut head = format!("HTTP/1.1 {} Stub\r\nconnection: close\r\n", reply.status);
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {}\r\n", value.replace("{base}", &base)));
    }
    if reply.content_length {
        let len: usize = reply.parts.iter().map(|(_, p)| p.len()).sum();
        head.push_str(&format!("content-length: {len}\r\n"));
    }
    head.push_str("\r\n");
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for (delay, part) in reply.parts {
        tokio::time::sleep(delay).await;
        if socket.write_all(&part).await.is_err() || socket.flush().await.is_err() {
            return;
        }
    }
    let _ = socket.shutdown().await;
}

async fn read_request(socket: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let mut chunk = [0u8; 4096];
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let target = request_line.next()?.to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();

    let mut body = buffer[head_end + 4..].to_vec();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    if let Some(len) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        while body.len() < len {
            let mut chunk = [0u8; 65536];
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
    } else if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        while !body.ends_with(b"0\r\n\r\n") {
            let mut chunk = [0u8; 65536];
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body = decode_chunked(&body);
    }

    Some(Recorded {
        method,
        target,
        headers,
        body,
    })
}

fn decode_chunked(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(pos) = raw.windows(2).position(|w| w == b"\r\n") {
        let size =
            usize::from_str_radix(std::str::from_utf8(&raw[..pos]).unwrap_or("0"), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[pos + 2..pos + 2 + size]);
        raw = &raw[pos + 2 + size + 2..];
    }
    out
}

// =============================================================================
// Fixtures
// =============================================================================

fn function_call_response(id: &str, call_id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "status": "requires_action",
        "steps": [{"type": "function_call", "id": call_id, "name": name, "arguments": {}}]
    })
}

fn text_response(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "status": "completed",
        "steps": [{"type": "model_output", "content": [{"type": "text", "text": text}]}]
    })
}

fn declared_function_names(request: &Recorded) -> Vec<String> {
    request.json()["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter(|t| t["type"] == "function")
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// A `#[tool]` in this binary's global registry, shadowed by the service
/// function of the same name below.
#[genai_rs_macros::tool]
fn mock_shared_tool() -> String {
    "from registry".to_string()
}

/// A service-provided function returning a fixed marker.
struct ServiceTool {
    name: &'static str,
}

#[async_trait]
impl CallableFunction for ServiceTool {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder(self.name)
            .description("service-provided test function")
            .build()
    }

    async fn call(&self, _args: Value) -> Result<Value, FunctionError> {
        Ok(json!({"source": "service"}))
    }
}

struct Service(Vec<&'static str>);

impl ToolService for Service {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        self.0
            .iter()
            .map(|&name| Arc::new(ServiceTool { name }) as Arc<dyn CallableFunction>)
            .collect()
    }
}

// =============================================================================
// Request shape and base URL
// =============================================================================

#[tokio::test]
async fn base_url_prefix_and_standard_headers_are_used() {
    let stub = Stub::replying(vec![Reply::json(200, text_response("int-1", "hi"))]).await;
    let client = Client::builder("test-key".to_string())
        .with_base_url(format!("{}/proxy/", stub.base_url))
        .build()
        .unwrap();

    let response = client
        .interaction()
        .with_model("test-model")
        .with_text("Hello")
        .create()
        .await
        .unwrap();
    assert_eq!(response.as_text(), Some("hi"));

    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.target, "/proxy/v1beta/interactions");
    assert_eq!(request.header("x-goog-api-key"), Some("test-key"));
    assert_eq!(request.header("api-revision"), Some("2026-05-20"));
    assert!(request.json().get("stream").is_none());
}

#[tokio::test]
async fn execute_clears_a_stream_flag_set_by_hand() {
    let stub = Stub::replying(vec![Reply::json(200, text_response("int-1", "hi"))]).await;
    let client = stub.client();
    let mut request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hello")
        .build()
        .unwrap();
    request.stream = Some(true);

    client.execute(request).await.unwrap();
    assert!(stub.requests()[0].json().get("stream").is_none());
}

#[tokio::test]
async fn execute_stream_sends_stream_true() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"int-1\",\"status\":\"completed\"}}\n\n",
    ])])
    .await;
    let client = stub.client();
    // Built without touching `stream`, the way `execute_stream`'s own docs do.
    let request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hello")
        .build()
        .unwrap();

    let events: Vec<_> = client.execute_stream(request).collect().await;
    assert!(matches!(
        events.as_slice(),
        [Ok(event)] if matches!(event.chunk, StreamChunk::Completed(_))
    ));

    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(request.target, "/v1beta/interactions?alt=sse");
    assert_eq!(request.json()["stream"], json!(true));
}

#[tokio::test]
async fn get_interaction_stream_asks_for_sse_and_resumes() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"step.delta\",\"index\":0,\"delta\":{\"type\":\"text\",\"text\":\"b\"},\"event_id\":\"evt-4\"}\n\n",
    ])])
    .await;
    let client = stub.client();

    let events: Vec<_> = client
        .get_interaction_stream("int-1", Some("evt-3"))
        .collect()
        .await;
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].as_ref().unwrap().event_id.as_deref(),
        Some("evt-4")
    );

    let [request] = stub.requests().try_into().unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.target,
        "/v1beta/interactions/int-1?alt=sse&stream=true&last_event_id=evt-3"
    );
}

// =============================================================================
// SSE framing, end to end
// =============================================================================

async fn stream_texts(chunks: &[&str]) -> Vec<String> {
    let stub = Stub::replying(vec![Reply::sse(chunks)]).await;
    let client = stub.client();
    let request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .build()
        .unwrap();
    client
        .execute_stream(request)
        .map(|event| match event.unwrap().chunk {
            StreamChunk::StepDelta { delta, .. } => delta.as_text().unwrap_or_default().to_string(),
            StreamChunk::Completed(response) => format!("<completed {}>", response.all_text()),
            other => format!("<{other:?}>"),
        })
        .collect()
        .await
}

fn delta_event(text: &str) -> String {
    format!(
        "{{\"event_type\":\"step.delta\",\"index\":0,\"delta\":{{\"type\":\"text\",\"text\":\"{text}\"}}}}"
    )
}

#[tokio::test]
async fn sse_final_event_without_trailing_newline_is_delivered() {
    let first = format!("data: {}\n\n", delta_event("a"));
    let last = format!("data: {}", delta_event("b"));
    assert_eq!(stream_texts(&[&first, &last]).await, ["a", "b"]);
}

#[tokio::test]
async fn sse_multi_line_data_is_one_event() {
    let chunk = "data: {\"event_type\":\"step.delta\",\"index\":0,\ndata: \"delta\":{\"type\":\"text\",\"text\":\"joined\"}}\n\n";
    assert_eq!(stream_texts(&[chunk]).await, ["joined"]);
}

#[tokio::test]
async fn sse_event_split_across_reads_and_crlf_framing() {
    let event = format!("event: step.delta\r\ndata: {}\r\n\r\n", delta_event("x"));
    let (head, tail) = event.split_at(event.len() / 2);
    assert_eq!(stream_texts(&[head, tail]).await, ["x"]);
}

#[tokio::test]
async fn sse_comments_ids_and_done_marker_are_ignored() {
    let chunk = format!(
        ": keep-alive\n\nid: 1\nretry: 100\ndata: {}\n\ndata: [DONE]\n\n",
        delta_event("y")
    );
    assert_eq!(stream_texts(&[&chunk]).await, ["y"]);
}

#[tokio::test]
async fn sse_step_accumulation_fills_the_completed_response() {
    let chunks = [
        "data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"model_output\",\"content\":[]}}\n\n".to_string(),
        format!("data: {}\n\n", delta_event("Hel")),
        format!("data: {}\n\n", delta_event("lo")),
        "data: {\"event_type\":\"step.stop\",\"index\":0}\n\n".to_string(),
        "data: {\"event_type\":\"interaction.completed\",\"interaction\":{\"id\":\"i\",\"status\":\"completed\"}}\n\n".to_string(),
    ];
    let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let texts = stream_texts(&chunks).await;
    assert_eq!(texts.last().map(String::as_str), Some("<completed Hello>"));
}

// =============================================================================
// Error mapping
// =============================================================================

#[tokio::test]
async fn api_error_carries_request_id_retry_after_and_envelope_message() {
    let stub = Stub::replying(vec![
        Reply::json(
            429,
            json!({"error": {"message": "Quota exceeded for this project.", "code": "resource_exhausted"}}),
        )
        .header("x-goog-request-id", "req-42")
        .header("retry-after", "7"),
    ])
    .await;

    let err = stub.client().get_interaction("int-1").await.unwrap_err();
    match &err {
        GenaiError::Api {
            status_code,
            message,
            request_id,
            retry_after,
        } => {
            assert_eq!(*status_code, 429);
            assert_eq!(
                message,
                "resource_exhausted: Quota exceeded for this project."
            );
            assert_eq!(request_id.as_deref(), Some("req-42"));
            assert_eq!(*retry_after, Some(Duration::from_secs(7)));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert!(err.is_retryable());
    assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
}

#[tokio::test]
async fn api_error_standard_envelope_and_raw_bodies() {
    let long_message = "x".repeat(400);
    let stub = Stub::replying(vec![
        Reply::json(
            400,
            json!({"error": {"code": 400, "message": long_message, "status": "INVALID_ARGUMENT"}}),
        ),
        Reply::text(502, "<html>Bad Gateway</html>"),
    ])
    .await;
    let client = stub.client();

    let err = client.get_interaction("int-1").await.unwrap_err();
    assert!(
        matches!(&err, GenaiError::Api { status_code: 400, message, .. }
            if *message == format!("INVALID_ARGUMENT: {long_message}")),
        "long envelope messages must not be truncated: {err:?}"
    );
    assert!(!err.is_retryable());

    let err = client.get_interaction("int-1").await.unwrap_err();
    assert!(
        matches!(&err, GenaiError::Api { status_code: 502, message, .. }
            if message == "<html>Bad Gateway</html>"),
        "{err:?}"
    );
    assert!(err.is_retryable());
}

#[tokio::test]
async fn unparseable_success_body_is_malformed_response() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({"id": 42})),
        Reply::text(200, "<html>captive portal</html>"),
    ])
    .await;
    let client = stub.client();
    let err = client.get_interaction("int-1").await.unwrap_err();
    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    let err = client.get_interaction("int-1").await.unwrap_err();
    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
    assert!(!err.is_retryable());
}

// =============================================================================
// Files
// =============================================================================

#[tokio::test]
async fn file_upload_uses_the_base_url_for_both_legs() {
    let stub = Stub::replying(vec![
        Reply::json(200, json!({})).header("x-goog-upload-url", "{base}/upload-session/1"),
        Reply::json(
            200,
            json!({"file": {"name": "files/abc", "mimeType": "text/plain", "uri": "https://x/files/abc", "state": "ACTIVE"}}),
        ),
    ])
    .await;

    let file = stub
        .client()
        .upload_file_bytes(b"hello".to_vec(), "text/plain", Some("greeting.txt"))
        .await
        .unwrap();
    assert_eq!(file.name, "files/abc");

    let [start, finish] = stub.requests().try_into().unwrap();
    assert_eq!(start.target, "/upload/v1beta/files");
    assert_eq!(start.header("x-goog-upload-command"), Some("start"));
    assert_eq!(start.json()["file"]["displayName"], "greeting.txt");
    assert_eq!(finish.target, "/upload-session/1");
    assert_eq!(
        finish.header("x-goog-upload-command"),
        Some("upload, finalize")
    );
    assert_eq!(finish.body, b"hello");
}

#[tokio::test]
async fn file_upload_without_session_url_is_malformed_response() {
    let stub = Stub::replying(vec![Reply::json(200, json!({}))]).await;
    let err = stub
        .client()
        .upload_file_bytes(b"hello".to_vec(), "text/plain", None)
        .await
        .unwrap_err();
    assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
}

#[tokio::test]
async fn wait_for_file_ready_polls_then_returns_active() {
    let file = |state: &str| json!({"name": "files/abc", "mimeType": "video/mp4", "uri": "u", "state": state});
    let stub = Stub::replying(vec![
        Reply::json(200, file("PROCESSING")),
        Reply::json(200, file("ACTIVE")),
    ])
    .await;
    let client = stub.client();
    let metadata = serde_json::from_value(file("PROCESSING")).unwrap();

    let ready = client
        .wait_for_file_ready(&metadata, Duration::from_millis(10), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(ready.is_active());
    let targets: Vec<_> = stub.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(targets, ["/v1beta/files/abc", "/v1beta/files/abc"]);
}

#[tokio::test]
async fn wait_for_file_ready_failure_is_terminal_not_retryable() {
    let failed = json!({
        "name": "files/abc", "mimeType": "video/mp4", "uri": "u", "state": "FAILED",
        "error": {"code": 13, "message": "transcoding failed"}
    });
    let stub = Stub::replying(vec![Reply::json(200, failed)]).await;
    let metadata = serde_json::from_value(
        json!({"name": "files/abc", "mimeType": "video/mp4", "uri": "u", "state": "PROCESSING"}),
    )
    .unwrap();

    let err = stub
        .client()
        .wait_for_file_ready(&metadata, Duration::from_millis(10), Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(matches!(err, GenaiError::Internal(_)), "{err:?}");
    assert!(!err.is_retryable(), "a failed file never recovers: {err:?}");
    assert!(err.to_string().contains("transcoding failed"), "{err}");
}

// =============================================================================
// Auto-function loop
// =============================================================================

#[tokio::test]
async fn auto_functions_stop_at_max_loops() {
    let stub = Stub::start(|_, n| {
        Reply::json(
            200,
            function_call_response(&format!("int-{n}"), &format!("call-{n}"), "svc_fn"),
        )
    })
    .await;

    let result = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("loop forever")
        .with_tool_service(Arc::new(Service(vec!["svc_fn"])))
        .with_max_function_call_loops(2)
        .create_with_auto_functions()
        .await
        .unwrap();

    assert!(result.reached_max_loops);
    assert_eq!(result.executions.len(), 2);
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    // The second round sends the first round's result, chained to it.
    let second = requests[1].json();
    assert_eq!(second["previous_interaction_id"], "int-0");
    assert_eq!(second["input"][0]["type"], "function_result");
    assert_eq!(second["input"][0]["call_id"], "call-0");
}

#[tokio::test]
async fn auto_functions_with_zero_loops_is_an_error_without_a_request() {
    let stub = Stub::replying(vec![]).await;
    let err = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .with_max_function_call_loops(0)
        .create_with_auto_functions()
        .await
        .unwrap_err();
    assert!(matches!(err, GenaiError::InvalidInput(_)), "{err:?}");
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn auto_functions_timeout_applies_per_call() {
    let stub = Stub::replying(vec![
        Reply::json(200, text_response("int-1", "late")).delayed(Duration::from_secs(2)),
    ])
    .await;
    let err = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .with_timeout(Duration::from_millis(100))
        .create_with_auto_functions()
        .await
        .unwrap_err();
    assert!(
        matches!(err, GenaiError::Timeout(d) if d == Duration::from_millis(100)),
        "{err:?}"
    );
}

#[tokio::test]
async fn service_function_shadows_the_registry_and_is_declared_once() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            function_call_response("int-1", "call-1", "mock_shared_tool"),
        ),
        Reply::json(200, text_response("int-2", "done")),
    ])
    .await;

    let result = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("use the tool")
        .with_tool_service(Arc::new(Service(vec!["mock_shared_tool"])))
        .create_with_auto_functions()
        .await
        .unwrap();
    assert!(!result.reached_max_loops);

    let requests = stub.requests();
    let declared = declared_function_names(&requests[0]);
    assert_eq!(
        declared.iter().filter(|n| *n == "mock_shared_tool").count(),
        1,
        "{declared:?}"
    );
    // The service implementation ran, not the registry one.
    assert_eq!(
        requests[1].json()["input"][0]["result"],
        json!({"source": "service"})
    );
}

#[tokio::test]
async fn tool_service_functions_are_declared_alongside_explicit_tools() {
    let stub = Stub::replying(vec![Reply::json(200, text_response("int-1", "ok"))]).await;

    stub.client()
        .interaction()
        .with_model("test-model")
        .with_text("search and compute")
        .with_google_search()
        .with_tool_service(Arc::new(Service(vec!["svc_fn"])))
        .create_with_auto_functions()
        .await
        .unwrap();

    let request = &stub.requests()[0];
    let tools = request.json()["tools"].clone();
    assert!(
        tools
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["type"] == "google_search"),
        "{tools}"
    );
    // Explicit tools replace registry discovery, but not the service.
    assert_eq!(declared_function_names(request), ["svc_fn"]);
}

#[tokio::test]
async fn streaming_auto_functions_surface_in_stream_errors() {
    let stub = Stub::replying(vec![Reply::sse(&[
        "data: {\"event_type\":\"interaction.created\",\"interaction\":{\"id\":\"int-1\",\"status\":\"in_progress\"}}\n\n",
        "data: {\"event_type\":\"error\",\"error\":{\"message\":\"backend overloaded\",\"code\":\"unavailable\"}}\n\n",
    ])])
    .await;

    let events: Vec<_> = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .create_stream_with_auto_functions()
        .collect()
        .await;

    let last = events.last().expect("at least one item");
    match last {
        Err(GenaiError::Stream { message, code }) => {
            assert_eq!(message, "backend overloaded");
            assert_eq!(code.as_deref(), Some("unavailable"));
        }
        other => panic!("expected GenaiError::Stream, got {other:?}"),
    }
    assert!(
        events.iter().all(
            |e| !matches!(e, Ok(ev) if matches!(ev.chunk, AutoFunctionStreamChunk::Complete(_)))
        ),
    );
}
