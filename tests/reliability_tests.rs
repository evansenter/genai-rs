//! Offline reliability tests against the local HTTP stub: the default
//! timeouts, how a cut-off stream is classified, the stop at a stream's
//! terminal event, and `Retry-After` in a poll. See `docs/RELIABILITY.md`.
//!
//! The paused-clock tests do real localhost I/O. Tokio advances a paused
//! clock to the next timer whenever the runtime is idle, even while I/O is
//! in flight, so they hold a blocking task (which stops the auto-advance)
//! until the stub has the request: from then on the only timers are the
//! stub's delays and any timeout the client has.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use common::http_stub::{Reply, Stub};
use futures_util::StreamExt;
use genai_rs::{
    AutoFunctionStreamChunk, CallableFunction, FunctionDeclaration, FunctionError, GenaiError,
    PollOptions, StreamChunk, StreamEvent, ToolService,
};
use serde_json::{Value, json};

const HOUR: Duration = Duration::from_secs(3600);

fn completed(id: &str, status: &str) -> String {
    format!(
        "data: {{\"event_type\":\"interaction.completed\",\"interaction\":{{\"id\":\"{id}\",\"status\":\"{status}\"}}}}\n\n"
    )
}

fn delta(text: &str) -> String {
    format!(
        "data: {{\"event_type\":\"step.delta\",\"index\":0,\"delta\":{{\"type\":\"text\",\"text\":\"{text}\"}}}}\n\n"
    )
}

/// An SSE reply in chunked transfer encoding that stops before the final
/// zero-length chunk: to the client, a transport failure mid-body, the way a
/// reset reads.
fn sse_then_transport_failure(events: &[&str]) -> Reply {
    let mut reply = Reply::sse(&[]);
    reply
        .headers
        .push(("transfer-encoding".into(), "chunked".into()));
    reply.parts = events
        .iter()
        .map(|event| {
            let mut chunk = format!("{:x}\r\n", event.len()).into_bytes();
            chunk.extend_from_slice(event.as_bytes());
            chunk.extend_from_slice(b"\r\n");
            (Duration::from_millis(5), chunk)
        })
        .collect();
    reply
}

async fn stream(stub: &Stub) -> Vec<Result<StreamEvent, GenaiError>> {
    let client = stub.client();
    let request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .build()
        .unwrap();
    client.execute_stream(request).collect().await
}

/// Holds the paused clock still until `release` is called (from the stub's
/// handler, once the request is in). Spawn it before sending.
fn hold_clock() -> (
    tokio::task::JoinHandle<()>,
    impl Fn() + Send + Sync + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let gate = tokio::task::spawn_blocking(move || {
        let _ = rx.recv();
    });
    let tx = Mutex::new(Some(tx));
    let release = move || {
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    };
    (gate, release)
}

// =============================================================================
// Default timeouts
// =============================================================================

/// No default read or total timeout: a non-streaming answer's headers come
/// only after generation finishes, and an hour later they still arrive.
#[tokio::test(start_paused = true)]
async fn default_client_waits_an_hour_for_the_response_headers() {
    let (gate, release) = hold_clock();
    let stub = Stub::start(move |_, _| {
        release();
        Reply::json(
            200,
            json!({
                "id": "int-1",
                "status": "completed",
                "steps": [{"type": "model_output", "content": [{"type": "text", "text": "late"}]}]
            }),
        )
        .delayed(HOUR)
    })
    .await;

    let start = tokio::time::Instant::now();
    // The stub's client sets only a base URL: every timeout is the default.
    let response = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .create()
        .await
        .unwrap();

    assert_eq!(response.as_text(), Some("late"));
    assert!(start.elapsed() >= HOUR, "{:?}", start.elapsed());
    gate.await.unwrap();
}

/// No default read timeout on a stream either: an hour between events is
/// waited out.
#[tokio::test(start_paused = true)]
async fn default_client_waits_an_hour_between_stream_events() {
    let (gate, release) = hold_clock();
    let stub = Stub::start(move |_, _| {
        release();
        let mut reply = Reply::sse(&[&delta("first"), &completed("int-1", "completed")]);
        reply.parts[1].0 = HOUR;
        reply
    })
    .await;

    let start = tokio::time::Instant::now();
    let events = stream(&stub).await;

    assert!(
        matches!(events.as_slice(), [Ok(_), Ok(last)] if last.is_complete()),
        "{events:?}"
    );
    assert!(start.elapsed() >= HOUR, "{:?}", start.elapsed());
    gate.await.unwrap();
}

/// The documented way to test against a local server on a paused clock
/// without holding it: `Duration::MAX` turns the connect timeout off, so no
/// timer is armed while the connect is in flight. (With the default, auto-
/// advance fires the 30 s timeout first on most runs.)
#[tokio::test(start_paused = true)]
async fn paused_clock_requests_succeed_with_the_connect_timeout_off() {
    for attempt in 0..20 {
        let stub = Stub::replying(vec![Reply::json(
            200,
            json!({"id": "int-1", "status": "completed"}),
        )])
        .await;
        let client = genai_rs::Client::builder("test-key".to_string())
            .with_base_url(&stub.base_url)
            .with_connect_timeout(Duration::MAX)
            .build()
            .unwrap();
        let start = tokio::time::Instant::now();
        let result = client
            .interaction()
            .with_model("test-model")
            .with_text("Hi")
            .create()
            .await;
        assert!(result.is_ok(), "attempt {attempt}: {result:?}");
        assert_eq!(start.elapsed(), Duration::ZERO, "attempt {attempt}");
    }
}

// =============================================================================
// Cut-off streams: one error, retryable, however the body ends
// =============================================================================

#[tokio::test]
async fn stream_closed_mid_event_is_interrupted() {
    let stub = Stub::replying(vec![Reply::sse(&[
        &delta("a"),
        "data: {\"event_type\":\"step.delta\",\"index\":0,\"del",
    ])])
    .await;

    let events = stream(&stub).await;
    let [Ok(_), Err(err)] = events.as_slice() else {
        panic!("expected a delta, then the cut-off: {events:?}");
    };
    assert!(
        matches!(err, GenaiError::StreamInterrupted { source: None, .. }),
        "{err:?}"
    );
    assert!(err.is_retryable());
}

#[tokio::test]
async fn transport_failure_mid_body_is_interrupted_with_its_source() {
    let stub = Stub::replying(vec![sse_then_transport_failure(&[&delta("a")])]).await;

    let events = stream(&stub).await;
    let [Ok(_), Err(err)] = events.as_slice() else {
        panic!("expected a delta, then the cut-off: {events:?}");
    };
    assert!(
        matches!(
            err,
            GenaiError::StreamInterrupted {
                source: Some(_),
                ..
            }
        ),
        "{err:?}"
    );
    assert!(err.is_retryable());
}

#[tokio::test]
async fn non_sse_success_body_is_malformed_not_interrupted() {
    // A body that was never a stream (here the plain JSON interaction a
    // GET without `stream=true` returns) is not a cut-off.
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({"id": "int-1", "status": "completed"}),
    )])
    .await;

    let events = stream(&stub).await;
    let [Err(err)] = events.as_slice() else {
        panic!("expected one error: {events:?}");
    };
    assert!(
        matches!(err, GenaiError::MalformedResponse(m) if m.contains("application/json")),
        "{err:?}"
    );
    assert!(!err.is_retryable());
}

// =============================================================================
// Nothing is read after the terminal event
// =============================================================================

#[tokio::test]
async fn stream_ends_at_completed_despite_a_bad_frame_after_it() {
    let stub = Stub::replying(vec![Reply::sse(&[
        &delta("a"),
        &completed("int-1", "completed"),
        "data: {not json}\n\n",
    ])])
    .await;

    let events = stream(&stub).await;
    assert!(
        matches!(events.as_slice(), [Ok(_), Ok(last)] if last.is_complete()),
        "{events:?}"
    );
}

#[tokio::test]
async fn stream_ends_at_completed_despite_a_transport_failure_after_it() {
    let stub = Stub::replying(vec![sse_then_transport_failure(&[
        &delta("a"),
        &completed("int-1", "completed"),
    ])])
    .await;

    let events = stream(&stub).await;
    assert!(
        matches!(events.as_slice(), [Ok(_), Ok(last)] if last.is_complete()),
        "{events:?}"
    );
}

struct Echo;

#[async_trait]
impl CallableFunction for Echo {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("echo")
            .with_description("returns a fixed value")
            .build()
    }

    async fn call(&self, _args: Value) -> Result<Value, FunctionError> {
        Ok(json!({"echoed": true}))
    }
}

struct EchoService;

impl ToolService for EchoService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(Echo)]
    }
}

/// Each round's body fails after `interaction.completed`. Before the stop
/// at the terminal event, that late error failed the finished round and so
/// the whole run.
#[tokio::test]
async fn auto_function_stream_survives_late_errors_after_each_round() {
    let call = "data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"function_call\",\"id\":\"call-1\",\"name\":\"echo\",\"arguments\":{}}}\n\n";
    let stub = Stub::replying(vec![
        sse_then_transport_failure(&[call, &completed("int-1", "requires_action")]),
        sse_then_transport_failure(&[&delta("done"), &completed("int-2", "completed")]),
    ])
    .await;

    let events: Vec<_> = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .with_tool_service(Arc::new(EchoService))
        .create_stream_with_auto_functions()
        .collect()
        .await;

    let chunks: Vec<_> = events
        .into_iter()
        .map(|event| event.expect("no late error reaches the caller").chunk)
        .collect();
    assert!(
        chunks
            .iter()
            .any(|c| matches!(c, AutoFunctionStreamChunk::FunctionResults(r) if r.len() == 1)),
        "{chunks:?}"
    );
    assert!(
        matches!(chunks.last(), Some(AutoFunctionStreamChunk::Complete(r)) if r.id.as_deref() == Some("int-2")),
        "{chunks:?}"
    );
    assert_eq!(stub.requests().len(), 2);
}

#[tokio::test]
async fn plain_stream_ends_at_an_error_event() {
    let stub = Stub::replying(vec![sse_then_transport_failure(&[
        "data: {\"event_type\":\"error\",\"error\":{\"message\":\"blocked\",\"code\":\"recitation\"}}\n\n",
        &delta("never"),
    ])])
    .await;

    let events = stream(&stub).await;
    assert!(
        matches!(
            events.as_slice(),
            [Ok(StreamEvent {
                chunk: StreamChunk::Error { .. },
                ..
            })]
        ),
        "{events:?}"
    );
}

// =============================================================================
// Retry-After while polling
// =============================================================================

/// A 429's `Retry-After` reaches the poll: the next status read waits for
/// it, not the 10 ms interval. (Real time, one second.)
#[tokio::test]
async fn wait_until_active_waits_out_a_429_retry_after() {
    let file = |state: &str| json!({"name": "files/abc", "mimeType": "video/mp4", "uri": "u", "state": state});
    let arrivals = Arc::new(Mutex::new(Vec::new()));
    let seen = arrivals.clone();
    let replies = Mutex::new(vec![
        Some(
            Reply::json(
                429,
                json!({"error": {"code": 429, "message": "slow down", "status": "RESOURCE_EXHAUSTED"}}),
            )
            .header("retry-after", "1"),
        ),
        Some(Reply::json(200, file("ACTIVE"))),
    ]);
    let stub = Stub::start(move |_, n| {
        seen.lock().unwrap().push(std::time::Instant::now());
        replies.lock().unwrap()[n].take().unwrap()
    })
    .await;

    let poll = PollOptions::new()
        .with_poll_interval(Duration::from_millis(10))
        .with_timeout(Duration::from_secs(30));
    let ready = stub
        .client()
        .files()
        .wait_until_active("files/abc", poll)
        .await
        .unwrap();

    assert!(ready.is_active());
    let arrivals = arrivals.lock().unwrap();
    assert_eq!(arrivals.len(), 2);
    let gap = arrivals[1] - arrivals[0];
    assert!(gap >= Duration::from_secs(1), "polled again after {gap:?}");
}
