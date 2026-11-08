//! Offline reliability tests against the local HTTP stub: the default
//! timeouts, how a cut-off stream is classified, the read of a stream's tail
//! after its terminal event, and `Retry-After` and the retryable statuses in
//! a poll. See `docs/RELIABILITY.md`; one test checks that its backon recipe
//! matches `examples/retry_with_backoff.rs`.
//!
//! The paused-clock tests do real localhost I/O. Tokio advances a paused
//! clock to the next timer whenever the runtime is idle, even while I/O is
//! in flight, so the ones that need the client's real defaults hold a
//! blocking task (which stops the auto-advance) until the stub has the
//! request: from then on the only timers are the stub's delays and any
//! timeout the client has. A test that sends several requests uses
//! `Stub::paused_clock_client`, which holds the clock for each request
//! until its response is read. The other way, turning the connect timeout
//! off with `Duration::MAX`, is tested here too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test reports a failure by panicking"
)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::http_stub::{Reply, Stub, hold_clock};
use futures_util::StreamExt;
use genai_rs::{
    AutoFunctionStreamChunk, CallableFunction, FunctionDeclaration, FunctionFuture, GenaiError,
    PollOptions, StreamChunk, StreamEvent, ToolService,
};
use serde_json::{Value, json};

const HOUR: Duration = Duration::from_secs(3600);

/// How long a stream reads the body's tail after its terminal event before
/// giving up: the crate-private `STREAM_TAIL_LIMIT`, as
/// `docs/RELIABILITY.md` documents it.
const TAIL_LIMIT: Duration = Duration::from_secs(5);

/// What the API sends after `interaction.completed`, about a second before
/// it closes the connection (live 2026-11-05).
const TAIL: &str = "event: done\ndata: [DONE]\n\n";

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

/// An SSE reply of `events`, then [`TAIL`] `after` the last of them, then
/// the close.
fn sse_with_tail(events: &[&str], after: Duration) -> Reply {
    let mut parts = events.to_vec();
    parts.push(TAIL);
    let mut reply = Reply::sse(&parts);
    reply.parts.last_mut().unwrap().0 = after;
    reply
}

/// [`sse_with_tail`] for a paused clock held until the terminal event
/// (`Stub::paused_clock_client`): the events come at once, since no time
/// passes while the clock is held, and only the tail waits.
fn held_sse_with_tail(events: &[&str], after: Duration) -> Reply {
    let mut reply = sse_with_tail(events, after);
    let tail = reply.parts.len() - 1;
    for part in &mut reply.parts[..tail] {
        part.0 = Duration::ZERO;
    }
    reply
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

/// The one exception: a body that ends right after an event this crate
/// doesn't recognize, which may be a new terminal event (Evergreen). The
/// plain stream ends cleanly. The auto-function loop can't go on without
/// the round's `Completed`, so that round ends as a cut-off one does,
/// retryable; it used to be a permanent `MalformedResponse`.
#[tokio::test]
async fn stream_ending_on_an_unknown_event_is_clean_but_interrupts_an_auto_function_round() {
    let body = || {
        Reply::sse(&[
            &delta("a"),
            "data: {\"event_type\":\"interaction.brand_new_end\",\"interaction\":{\"id\":\"int-1\"}}\n\n",
        ])
    };

    let stub = Stub::replying(vec![body()]).await;
    let events = stream(&stub).await;
    assert!(
        matches!(events.as_slice(), [Ok(_), Ok(last)] if last.chunk.is_unknown()),
        "the plain stream ends cleanly: {events:?}"
    );

    let stub = Stub::replying(vec![body()]).await;
    let events: Vec<_> = stub
        .client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .with_tool_service(Arc::new(EchoService))
        .create_stream_with_auto_functions()
        .collect()
        .await;
    let [Ok(first), Err(err)] = events.as_slice() else {
        panic!("expected the delta, then the error: {events:?}");
    };
    assert!(
        matches!(first.chunk, AutoFunctionStreamChunk::Delta(_)),
        "{first:?}"
    );
    assert!(
        matches!(err, GenaiError::StreamInterrupted { source: None, .. }),
        "{err:?}"
    );
    assert!(err.is_retryable());
    assert_eq!(stub.requests().len(), 1, "no round after the failed one");
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

/// The Content-Type check rejects only a declared type other than
/// `text/event-stream`: parameters and letter case pass, and so does a
/// response with no `Content-Type`, as from a proxy that strips it. A proxy
/// that rewrites it to another type fails (the CHANGELOG's migration).
#[tokio::test]
async fn stream_content_type_check_accepts_parameters_and_a_missing_header() {
    let sse = |content_type: Option<&str>| {
        let mut reply = Reply::sse(&[&delta("a"), &completed("int-1", "completed")]);
        reply.headers = content_type
            .map(|t| vec![("content-type".to_string(), t.to_string())])
            .unwrap_or_default();
        reply
    };
    for content_type in [
        Some("text/event-stream; charset=utf-8"),
        Some("Text/Event-Stream"),
        None,
    ] {
        let stub = Stub::replying(vec![sse(content_type)]).await;
        let events = stream(&stub).await;
        assert!(
            matches!(events.as_slice(), [Ok(_), Ok(last)] if last.is_complete()),
            "{content_type:?}: {events:?}"
        );
    }

    let stub = Stub::replying(vec![sse(Some("application/octet-stream"))]).await;
    let events = stream(&stub).await;
    assert!(
        matches!(events.as_slice(), [Err(GenaiError::MalformedResponse(m))]
            if m.contains("application/octet-stream")),
        "{events:?}"
    );
}

// =============================================================================
// After the terminal event: the tail is read to the end, never surfaced
// =============================================================================

/// The terminal event arrives at once, but the stream ends only with the
/// body, once the server's tail is in: the stored interaction is readable
/// only then (live 2026-11-05). Real time, since a paused clock can skip
/// past localhost I/O to the tail limit.
#[tokio::test]
async fn stream_ends_once_the_tail_after_completed_has_arrived() {
    let after = Duration::from_millis(500);
    let stub = Stub::replying(vec![sse_with_tail(
        &[&delta("a"), &completed("int-1", "completed")],
        after,
    )])
    .await;
    let client = stub.client();
    let request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .build()
        .unwrap();

    let start = std::time::Instant::now();
    let mut stream = client.execute_stream(request);
    let mut completed_after = None;
    while let Some(event) = stream.next().await {
        if event.unwrap().is_complete() {
            completed_after = Some(start.elapsed());
        }
    }
    let ended_after = start.elapsed();

    let completed_after = completed_after.expect("a Completed event");
    assert!(
        completed_after < after,
        "Completed waited for the tail: {completed_after:?}"
    );
    assert!(
        ended_after >= after,
        "ended before the tail: {ended_after:?}"
    );
    assert!(
        ended_after < TAIL_LIMIT,
        "ended at the limit, not the tail: {ended_after:?}"
    );
}

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

/// The tail is read as bytes, not events: a frame that doesn't parse
/// neither surfaces nor stops the read before the body ends.
#[tokio::test(start_paused = true)]
async fn bad_frame_after_completed_does_not_stop_the_tail_read() {
    let after = Duration::from_millis(300);
    let stub = Stub::replying(vec![held_sse_with_tail(
        &[&completed("int-1", "completed"), "data: {not json}\n\n"],
        after,
    )])
    .await;

    let client = stub.paused_clock_client();
    let request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .build()
        .unwrap();

    let start = tokio::time::Instant::now();
    let events: Vec<_> = client.execute_stream(request).collect().await;
    assert!(
        matches!(events.as_slice(), [Ok(last)] if last.is_complete()),
        "{events:?}"
    );
    assert!(start.elapsed() >= after, "{:?}", start.elapsed());
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

/// A connection left open after the terminal event (the default client has
/// no read timeout) is closed at the tail limit, and the stream ends
/// cleanly. The clock is held until `Completed` is in, so the next poll arms
/// the limit before auto-advance can skip to the stub's hour.
#[tokio::test(start_paused = true)]
async fn tail_that_never_ends_is_cut_at_the_limit() {
    let (gate, release) = hold_clock();
    let mut reply = sse_with_tail(&[&delta("a"), &completed("int-1", "completed")], HOUR);
    // No time passes while the clock is held.
    reply.parts[0].0 = Duration::ZERO;
    reply.parts[1].0 = Duration::ZERO;
    let stub = Stub::replying(vec![reply]).await;
    let client = stub.client();
    let request = client
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .build()
        .unwrap();

    let mut stream = client.execute_stream(request);
    loop {
        let event = stream
            .next()
            .await
            .expect("the stream ended early")
            .unwrap();
        if event.is_complete() {
            break;
        }
    }
    release();
    let start = tokio::time::Instant::now();
    let rest: Vec<_> = stream.collect().await;

    assert!(rest.is_empty(), "{rest:?}");
    let waited = start.elapsed();
    assert!(
        waited >= TAIL_LIMIT && waited < TAIL_LIMIT + Duration::from_secs(1),
        "{waited:?}"
    );
    gate.await.unwrap();
}

/// A per-chunk timeout shorter than the tail doesn't fail a finished
/// stream: the read after the terminal event is bounded by the tail limit
/// instead. The clock is held until `Completed` is in, so the 100 ms timeout
/// can't fire on localhost I/O before it; the tail then takes 300 ms of
/// virtual time.
#[tokio::test(start_paused = true)]
async fn per_chunk_timeout_does_not_bound_the_tail_read() {
    let after = Duration::from_millis(300);
    let stub = Stub::replying(vec![held_sse_with_tail(
        &[&delta("a"), &completed("int-1", "completed")],
        after,
    )])
    .await;

    let start = tokio::time::Instant::now();
    let events: Vec<_> = stub
        .paused_clock_client()
        .interaction()
        .with_model("test-model")
        .with_text("Hi")
        .with_timeout(Duration::from_millis(100))
        .create_stream()
        .collect()
        .await;

    assert!(
        matches!(events.as_slice(), [Ok(_), Ok(last)] if last.is_complete()),
        "{events:?}"
    );
    assert!(start.elapsed() >= after, "{:?}", start.elapsed());
}

/// A round that asks for `echo`.
const CALL_ECHO: &str = "data: {\"event_type\":\"step.start\",\"index\":0,\"step\":{\"type\":\"function_call\",\"id\":\"call-1\",\"name\":\"echo\",\"arguments\":{}}}\n\n";

struct Echo;

impl CallableFunction for Echo {
    fn declaration(&self) -> FunctionDeclaration {
        FunctionDeclaration::builder("echo")
            .with_description("returns a fixed value")
            .build()
    }

    fn call(&self, _args: Value) -> FunctionFuture<'_> {
        Box::pin(async move { Ok(json!({"echoed": true})) })
    }
}

struct EchoService;

impl ToolService for EchoService {
    fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
        vec![Arc::new(Echo)]
    }
}

/// Each round's body fails after `interaction.completed`. Before errors
/// after the terminal event were swallowed, that late error failed the
/// finished round and so the whole run.
#[tokio::test]
async fn auto_function_stream_survives_late_errors_after_each_round() {
    let stub = Stub::replying(vec![
        sse_then_transport_failure(&[CALL_ECHO, &completed("int-1", "requires_action")]),
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

/// The next round chains on the previous round's id, which the server
/// stores only once that round's stream is done, so its request waits for
/// the tail. The per-chunk timeout, shorter than the tail, doesn't bound
/// that read. Each round holds the clock until its `Completed` is in.
#[tokio::test(start_paused = true)]
async fn auto_function_stream_reads_each_round_to_its_end_before_the_next() {
    let after = Duration::from_millis(300);
    let arrivals = Arc::new(Mutex::new(Vec::new()));
    let seen = arrivals.clone();
    let replies = Mutex::new(vec![
        Some(held_sse_with_tail(
            &[CALL_ECHO, &completed("int-1", "requires_action")],
            after,
        )),
        Some(held_sse_with_tail(
            &[&delta("done"), &completed("int-2", "completed")],
            Duration::ZERO,
        )),
    ]);
    let stub = Stub::start(move |_, n| {
        seen.lock().unwrap().push(tokio::time::Instant::now());
        replies.lock().unwrap()[n].take().unwrap()
    })
    .await;

    let events: Vec<_> = stub
        .paused_clock_client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .with_tool_service(Arc::new(EchoService))
        .with_timeout(Duration::from_millis(100))
        .create_stream_with_auto_functions()
        .collect()
        .await;

    let chunks: Vec<_> = events
        .into_iter()
        .map(|event| event.expect("no error reaches the caller").chunk)
        .collect();
    assert!(
        matches!(chunks.last(), Some(AutoFunctionStreamChunk::Complete(r)) if r.id.as_deref() == Some("int-2")),
        "{chunks:?}"
    );
    let arrivals = arrivals.lock().unwrap();
    assert_eq!(arrivals.len(), 2);
    let gap = arrivals[1] - arrivals[0];
    assert!(gap >= after, "the next round was sent after {gap:?}");
    assert_eq!(
        stub.requests()[1].json()["previous_interaction_id"],
        json!("int-1")
    );
}

/// After the last round's `Complete`, the stream reads that round's tail
/// before it ends, so a caller that gets or chains on the interaction once
/// the stream is done finds it stored.
#[tokio::test(start_paused = true)]
async fn auto_function_stream_reads_the_final_round_to_its_end() {
    let after = Duration::from_millis(300);
    let stub = Stub::replying(vec![
        held_sse_with_tail(
            &[CALL_ECHO, &completed("int-1", "requires_action")],
            Duration::ZERO,
        ),
        held_sse_with_tail(&[&delta("done"), &completed("int-2", "completed")], after),
    ])
    .await;

    let start = tokio::time::Instant::now();
    let mut stream = stub
        .paused_clock_client()
        .interaction()
        .with_model("test-model")
        .with_text("hi")
        .with_tool_service(Arc::new(EchoService))
        .create_stream_with_auto_functions();
    let mut completed_at = None;
    while let Some(event) = stream.next().await {
        if let AutoFunctionStreamChunk::Complete(response) = event.unwrap().chunk {
            assert_eq!(response.id.as_deref(), Some("int-2"));
            completed_at = Some(start.elapsed());
        }
    }
    let ended_at = start.elapsed();

    let completed_at = completed_at.expect("a Complete event");
    assert!(
        completed_at < after,
        "Complete waited for the tail: {completed_at:?}"
    );
    assert!(ended_at >= after, "ended before the tail: {ended_at:?}");
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
/// it, not the 10 ms interval. Each request holds the paused clock while in
/// flight, so the gap is exactly the one second the poll slept.
#[tokio::test(start_paused = true)]
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
        seen.lock().unwrap().push(tokio::time::Instant::now());
        replies.lock().unwrap()[n].take().unwrap()
    })
    .await;

    let poll = PollOptions::new()
        .with_poll_interval(Duration::from_millis(10))
        .with_timeout(Duration::from_secs(30));
    let start = tokio::time::Instant::now();
    let ready = stub
        .paused_clock_client()
        .files()
        .wait_until_active("files/abc", poll)
        .await
        .unwrap();

    assert!(ready.is_active());
    let arrivals = arrivals.lock().unwrap();
    assert_eq!(arrivals.len(), 2);
    assert_eq!(arrivals[0], start);
    let gap = arrivals[1] - arrivals[0];
    assert_eq!(gap, Duration::from_secs(1), "polled again after {gap:?}");
}

// =============================================================================
// Which failed status reads a poll goes on through
// =============================================================================

/// Waits for `files/abc` against `stub`, polling every 10 ms for up to 30 s.
async fn wait_for_file(stub: &Stub) -> Result<genai_rs::FileMetadata, GenaiError> {
    let poll = PollOptions::new()
        .with_poll_interval(Duration::from_millis(10))
        .with_timeout(Duration::from_secs(30));
    stub.paused_clock_client()
        .files()
        .wait_until_active("files/abc", poll)
        .await
}

/// A 408 from a front end that gave up waiting for the status request is
/// retryable, so the wait goes on through it.
#[tokio::test(start_paused = true)]
async fn wait_until_active_polls_through_a_408() {
    let stub = Stub::replying(vec![
        Reply::text(408, "<html><title>408 Request Timeout</title></html>"),
        Reply::json(
            200,
            json!({"name": "files/abc", "mimeType": "video/mp4", "uri": "u", "state": "ACTIVE"}),
        ),
    ])
    .await;

    let ready = wait_for_file(&stub).await.unwrap();

    assert!(ready.is_active());
    assert_eq!(stub.requests().len(), 2);
}

/// A 501 is not retryable: the wait ends with it at once, rather than
/// asking again until the deadline for an answer that will not change.
#[tokio::test(start_paused = true)]
async fn wait_until_active_ends_at_a_501() {
    let stub = Stub::replying(vec![Reply::json(
        501,
        json!({"error": {"code": 501, "message": "Method not implemented.", "status": "UNIMPLEMENTED"}}),
    )])
    .await;

    let err = wait_for_file(&stub).await.unwrap_err();

    assert!(
        matches!(
            &err,
            GenaiError::Api {
                status_code: 501,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(stub.requests().len(), 1);
}

// =============================================================================
// The backon recipe in docs/RELIABILITY.md and its runnable example
// =============================================================================

/// The lines of the backon recipe in `source`, trimmed: `MAX_DELAY`,
/// `next_delay`, the backoff's builder chain and the retry's `retry`,
/// `when` and `adjust`. Comments and doctest-hidden lines (`# ...`) are not
/// part of it.
fn backon_recipe(source: &str) -> Vec<&str> {
    let lines: Vec<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//") && !line.starts_with('#'))
        .collect();
    let mut recipe = Vec::new();
    for (start, end) in [
        ("const MAX_DELAY", ";"),
        ("fn next_delay(", "}"),
        ("let backoff = ExponentialBuilder", ";"),
    ] {
        let from = lines
            .iter()
            .position(|line| line.starts_with(start))
            .expect("the recipe has each of its parts");
        let to = from
            + lines[from..]
                .iter()
                .position(|line| line.ends_with(end))
                .expect("each part of the recipe ends");
        recipe.extend_from_slice(&lines[from..=to]);
    }
    recipe.extend(lines.iter().filter(|line| {
        [".retry(", ".when(", ".adjust("]
            .iter()
            .any(|call| line.starts_with(call))
    }));
    recipe
}

/// `docs/RELIABILITY.md`'s backon snippet (doctested) and
/// `examples/retry_with_backoff.rs` (built) are the same recipe, jitter
/// included: the snippet once lacked the example's `.with_jitter()`, and
/// nothing compiled either one against the other.
#[test]
fn the_backon_recipe_in_reliability_md_matches_its_example() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = std::fs::read_to_string(root.join("docs/RELIABILITY.md")).unwrap();
    let example = std::fs::read_to_string(root.join("examples/retry_with_backoff.rs")).unwrap();
    let start = doc
        .find("use backon::")
        .expect("RELIABILITY.md has the backon snippet");
    let end = start + doc[start..].find("```").expect("the snippet ends");
    let snippet = &doc[start..end];

    let recipe = backon_recipe(snippet);

    assert_eq!(recipe, backon_recipe(&example));
    assert!(
        recipe.iter().any(|line| line.starts_with(".with_jitter()")),
        "{recipe:#?}"
    );
    assert!(
        recipe.contains(&".when(GenaiError::is_retryable)"),
        "{recipe:#?}"
    );
}
