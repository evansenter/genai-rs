// The stand-in harness is a shell script.
#![cfg(unix)]

use std::error::Error as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::protocol::Message;

use super::super::tests::{STAND_IN_STDERR, fake_harness, hold_the_sink};
use super::{INIT_TIMEOUT, SESSION_END_TIMEOUT};
use crate::antigravity::protocol::InitializeConversationEvent;
use crate::antigravity::{AgentBuilder, AntigravityAgent, AntigravityError, TurnFailure};

/// The stand-in harness's end of the WebSocket.
type Peer = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// Longer than any bound under test: a call still pending after it hangs.
const GUARD: Duration = Duration::from_secs(900);

/// What the stand-in harness's WebSocket does once it has read the
/// initialization message (and answered it, when the script says so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    /// Nothing more: it holds the connection without reading.
    Hold,
    /// Closes the connection.
    Close,
    /// Sends a frame with reserved opcode 3, which no WebSocket peer may
    /// send: the client's read fails with a protocol error, not as a
    /// closed connection.
    ReservedOpcode,
    /// Sends a text frame that is not JSON.
    Garbage,
    /// Reports the main trajectory cancelled, which fails the event's
    /// handling.
    CancelledTrajectory,
    /// Sends the session-start hook's request, then holds.
    StartHook,
    /// Sends the session-end hook's request, then holds.
    EndHook,
    /// Skips two events that are not the answer, answers, then holds.
    NoiseThenAnswer,
    /// Waits for the session-end request, then closes the connection.
    CloseOnSessionEnd,
    /// Waits for the session-end request, then calls the session-end hook,
    /// reports a cancelled trajectory, and confirms the end.
    EndSessionWithHook,
}

async fn send_json(ws: &mut Peer, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

fn init_response() -> serde_json::Value {
    json!({"initializeConversationResponse": {"cascadeId": "stand-in"}})
}

fn hook_request(hook_type: &str) -> serde_json::Value {
    json!({"callHookRequest": {"requestId": "hook-1", "type": hook_type}})
}

fn cancelled_trajectory() -> serde_json::Value {
    json!({"trajectoryStateUpdate": {
        "trajectoryId": "main", "state": "STATE_CANCELLED", "error": "stand-in cancel"
    }})
}

/// Reads messages until one is a session-end request.
async fn await_session_end_request(ws: &mut Peer) {
    while let Some(Ok(message)) = ws.next().await {
        if message
            .to_text()
            .is_ok_and(|t| t.contains("sessionEndRequest"))
        {
            return;
        }
    }
    panic!("the client never asked to end the session");
}

/// The stand-in harness's WebSocket peer on `listener`.
async fn peer(listener: tokio::net::TcpListener, answer_init: bool, then: Then) {
    let (socket, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
    ws.next().await.unwrap().unwrap();
    if answer_init {
        send_json(&mut ws, init_response()).await;
    }
    match then {
        Then::Hold => {}
        Then::Close => {
            ws.close(None).await.unwrap();
            return;
        }
        Then::ReservedOpcode => {
            ws.get_mut().write_all(&[0x83, 0x00]).await.unwrap();
            ws.get_mut().flush().await.unwrap();
        }
        Then::Garbage => {
            ws.send(Message::Text("not json".into())).await.unwrap();
        }
        Then::CancelledTrajectory => send_json(&mut ws, cancelled_trajectory()).await,
        Then::StartHook => {
            send_json(&mut ws, hook_request("LIFECYCLE_HOOK_ON_SESSION_START")).await;
        }
        Then::EndHook => send_json(&mut ws, hook_request("LIFECYCLE_HOOK_ON_SESSION_END")).await,
        Then::NoiseThenAnswer => {
            send_json(&mut ws, json!({})).await;
            send_json(
                &mut ws,
                json!({"trajectoryStateUpdate": {"trajectoryId": "main", "state": "STATE_RUNNING"}}),
            )
            .await;
            send_json(&mut ws, init_response()).await;
        }
        Then::CloseOnSessionEnd => {
            await_session_end_request(&mut ws).await;
            ws.close(None).await.unwrap();
            return;
        }
        Then::EndSessionWithHook => {
            await_session_end_request(&mut ws).await;
            send_json(&mut ws, hook_request("LIFECYCLE_HOOK_ON_SESSION_END")).await;
            // The hook's answer.
            ws.next().await.unwrap().unwrap();
            send_json(&mut ws, cancelled_trajectory()).await;
            send_json(&mut ws, json!({"sessionEndResponse": true})).await;
        }
    }
    // Hold the connection until the test aborts this task.
    std::future::pending::<()>().await;
}

/// A `spawn()` against the stand-in harness, and its WebSocket peer.
struct Spawned {
    result: Result<AntigravityAgent, AntigravityError>,
    peer: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Spawned {
    /// Spawns `builder`'s agent against a stand-in whose peer answers the
    /// initialization when `answer_init` is set, then does `then`.
    async fn new(builder: AgentBuilder, answer_init: bool, then: Then) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("localhost:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = tokio::spawn(peer(listener, answer_init, then));
        let result = builder
            .with_harness_path(fake_harness(dir.path(), port))
            .spawn()
            .await;
        Self {
            result,
            peer,
            _dir: dir,
        }
    }

    /// The agent, once its stderr tail holds the stand-in's line, so a
    /// paused clock cannot outrun the drain that reads it.
    async fn agent(&mut self) -> &mut AntigravityAgent {
        let agent = self.result.as_mut().expect("spawn");
        for _ in 0..100 {
            if agent.harness.stderr_tail().await.contains(STAND_IN_STDERR) {
                return agent;
            }
        }
        panic!("the stand-in's stderr never arrived");
    }

    /// `spawn()`'s error.
    fn error(&self) -> &AntigravityError {
        match &self.result {
            Ok(_) => panic!("spawn succeeded"),
            Err(e) => e,
        }
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        self.peer.abort();
    }
}

/// The parts of an `InitFailed`, asserting it carries the stand-in's
/// stderr.
fn init_failed(error: &AntigravityError) -> (&str, Option<&(dyn std::error::Error + 'static)>) {
    let AntigravityError::InitFailed {
        message, stderr, ..
    } = error
    else {
        panic!("expected InitFailed: {error:?}");
    };
    assert!(stderr.contains(STAND_IN_STDERR), "{stderr}");
    (message, error.source())
}

/// The `WebSocket` error `source` is, with its message.
fn websocket_message<'a>(source: Option<&'a (dyn std::error::Error + 'static)>) -> &'a str {
    let source = source.expect("the cause is the source");
    let Some(AntigravityError::WebSocket { message, source }) = source.downcast_ref() else {
        panic!("the source is a WebSocket error: {source:?}");
    };
    assert!(
        source
            .downcast_ref::<tokio_tungstenite::tungstenite::Error>()
            .is_some(),
        "the transport error is one link further: {source:?}"
    );
    message
}

/// Whether `error`'s `Display` leaves its source's text to `source()`.
fn display_leaves_the_source(error: &AntigravityError) -> bool {
    error
        .source()
        .is_none_or(|source| !error.to_string().contains(&source.to_string()))
}

#[tokio::test]
async fn test_a_failed_initialization_exchange_is_init_failed_with_its_cause() {
    // The harness closes the connection instead of answering.
    let closed = Spawned::new(AntigravityAgent::builder(), false, Then::Close).await;
    let (message, source) = init_failed(closed.error());
    assert!(
        message.contains("closed the connection during initialization"),
        "{message}"
    );
    assert!(source.is_none(), "{source:?}");

    // A frame the client cannot read: the read error is the source.
    let unreadable = Spawned::new(AntigravityAgent::builder(), false, Then::ReservedOpcode).await;
    let (message, source) = init_failed(unreadable.error());
    assert!(
        message.contains("reading the initialization response"),
        "{message}"
    );
    assert_eq!(websocket_message(source), "read failed");
    assert!(display_leaves_the_source(unreadable.error()));

    // A message that does not parse: the parse failure is the source.
    let garbled = Spawned::new(AntigravityAgent::builder(), false, Then::Garbage).await;
    let (_, source) = init_failed(garbled.error());
    let source = source.expect("the parse failure is the source");
    assert!(
        matches!(
            source.downcast_ref(),
            Some(AntigravityError::Protocol {
                source: Some(_),
                ..
            })
        ),
        "{source:?}"
    );
}

#[tokio::test]
async fn test_events_before_the_initialization_response_are_skipped() {
    let mut spawned = Spawned::new(AntigravityAgent::builder(), false, Then::NoiseThenAnswer).await;
    let agent = spawned.agent().await;
    assert_eq!(agent.conversation_id(), Some("stand-in"));
}

#[tokio::test]
async fn test_a_failed_session_start_is_init_failed_with_its_cause() {
    let ran = Arc::new(AtomicBool::new(false));
    let builder = || {
        let ran = Arc::clone(&ran);
        AntigravityAgent::builder().on_session_start(move || ran.store(true, Ordering::SeqCst))
    };

    // The harness closes the connection before it calls the hook.
    let closed = Spawned::new(builder(), true, Then::Close).await;
    let (message, source) = init_failed(closed.error());
    assert!(
        message.contains("before the session-start hook"),
        "{message}"
    );
    assert!(source.is_none(), "{source:?}");

    // A frame the client cannot read, where spawn() returned the bare
    // WebSocket error, with no stderr.
    let unreadable = Spawned::new(builder(), true, Then::ReservedOpcode).await;
    let (message, source) = init_failed(unreadable.error());
    assert!(
        message.contains("before the session-start hook"),
        "{message}"
    );
    assert_eq!(websocket_message(source), "read failed");
    assert!(display_leaves_the_source(unreadable.error()));

    // A message whose handling fails: that failure is the source.
    let cancelled = Spawned::new(builder(), true, Then::CancelledTrajectory).await;
    let (message, source) = init_failed(cancelled.error());
    assert!(
        message.contains("before the session-start hook"),
        "{message}"
    );
    let source = source.expect("the handling failure is the source");
    assert!(
        matches!(
            source.downcast_ref(),
            Some(AntigravityError::Turn {
                kind: TurnFailure::Cancelled,
                ..
            })
        ),
        "{source:?}"
    );

    assert!(!ran.load(Ordering::SeqCst), "no harness called the hook");
}

#[tokio::test]
async fn test_a_failed_initialization_send_keeps_the_websocket_error_as_its_source() {
    let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::Hold).await;
    let agent = spawned.agent().await;
    // After its close frame, the client's next send fails at once.
    agent.session.close().await;
    let error = tokio::time::timeout(
        GUARD,
        agent.initialize(InitializeConversationEvent::default()),
    )
    .await
    .expect("initialize gives up rather than hanging")
    .expect_err("the send fails");
    let (message, source) = init_failed(&error);
    assert!(
        message.contains("sending the initialization message"),
        "{message}"
    );
    // D-062: the send's error is the source, not text in the message.
    assert_eq!(websocket_message(source), "send failed");
    assert!(display_leaves_the_source(&error));
}

/// A session-start or session-end hook that blocks until `release` drops.
fn blocking_hook() -> (
    std::sync::mpsc::Sender<()>,
    Arc<dyn Fn() + Send + Sync + 'static>,
) {
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = std::sync::Mutex::new(released);
    let hook = move || {
        let _ = released.lock().unwrap().recv_timeout(GUARD);
    };
    (release, Arc::new(hook))
}

/// Runs `call` on a paused clock and returns its result and how long it
/// took in virtual time.
async fn timed<T>(call: impl std::future::Future<Output = T>) -> (T, Duration) {
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(GUARD, call)
        .await
        .expect("gives up rather than hanging");
    let elapsed = started.elapsed();
    tokio::time::resume();
    (result, elapsed)
}

/// Runs `call`, which waits on the peer, on real time and returns how
/// long it took.
async fn real_time(call: impl std::future::Future<Output = ()>) -> Duration {
    let started = std::time::Instant::now();
    tokio::time::timeout(GUARD, call)
        .await
        .expect("gives up rather than hanging");
    started.elapsed()
}

/// Whether `elapsed` is `bound`, give or take the stderr read.
fn at(elapsed: Duration, bound: Duration) -> bool {
    elapsed >= bound && elapsed < bound + Duration::from_secs(1)
}

#[tokio::test]
async fn test_a_harness_that_stops_answering_fails_initialization_at_init_timeout() {
    let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::Hold).await;
    let agent = spawned.agent().await;

    // No answer to the initialization.
    let (result, elapsed) = timed(agent.initialize(InitializeConversationEvent::default())).await;
    let error = result.expect_err("no answer");
    let (message, source) = init_failed(&error);
    assert!(message.contains("no initialization response"), "{message}");
    assert!(source.is_none(), "{source:?}");
    assert!(at(elapsed, INIT_TIMEOUT), "{elapsed:?}");

    // No call of the session-start hook.
    let (release, hook) = blocking_hook();
    agent.lifecycle.session_start = Some(hook);
    let (result, elapsed) = timed(agent.start_session()).await;
    let error = result.expect_err("no hook call");
    let (message, source) = init_failed(&error);
    assert!(
        message.contains("did not call the session-start hook"),
        "{message}"
    );
    assert!(source.is_none(), "{source:?}");
    assert!(at(elapsed, INIT_TIMEOUT), "{elapsed:?}");
    drop(release);
}

#[tokio::test]
async fn test_a_session_start_hook_that_overruns_fails_at_init_timeout() {
    // Spawned without the hook, so the hook's request waits unread.
    let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::StartHook).await;
    let agent = spawned.agent().await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (release, hook) = blocking_hook();
    agent.lifecycle.session_start = Some(hook);
    let (result, elapsed) = timed(agent.start_session()).await;
    drop(release);
    let error = result.expect_err("the hook overran");
    let (message, source) = init_failed(&error);
    assert!(
        message.contains("session-start hook did not finish"),
        "{message}"
    );
    assert!(source.is_none(), "{source:?}");
    assert!(at(elapsed, INIT_TIMEOUT), "{elapsed:?}");
}

#[tokio::test]
async fn test_a_session_start_hook_whose_answer_fails_is_init_failed_with_the_send_error() {
    let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::StartHook).await;
    let agent = spawned.agent().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    // After its close frame the client still reads the hook's request, and
    // its answer fails to send.
    agent.session.close().await;
    let ran = Arc::new(AtomicBool::new(false));
    let hook_ran = Arc::clone(&ran);
    agent.lifecycle.session_start = Some(Arc::new(move || hook_ran.store(true, Ordering::SeqCst)));
    let error = tokio::time::timeout(GUARD, agent.start_session())
        .await
        .expect("start_session gives up rather than hanging")
        .expect_err("the answer cannot be sent");
    assert!(ran.load(Ordering::SeqCst), "the hook ran before its answer");
    let (message, source) = init_failed(&error);
    assert!(
        message.contains("answering the session-start hook"),
        "{message}"
    );
    assert_eq!(websocket_message(source), "send failed");
}

/// Runs `test` on a runtime of its own under a recording subscriber, and
/// returns its output and the messages it logged.
fn logged<T>(test: impl std::future::Future<Output = T>) -> (T, Vec<String>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut output = None;
    let messages = crate::test_subscriber::capture_messages(|| {
        output = Some(runtime.block_on(test));
    });
    (output.expect("the test ran"), messages)
}

/// Whether one of `messages` contains `text`.
fn logs(messages: &[String], text: &str) -> bool {
    messages.iter().any(|m| m.contains(text))
}

#[test]
fn test_a_session_end_request_that_fails_ends_session_end_at_once() {
    let (elapsed, messages) = logged(async {
        let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::Hold).await;
        let agent = spawned.agent().await;
        agent.session.close().await;
        timed(agent.end_session()).await.1
    });
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    assert!(
        logs(&messages, "Could not ask the harness to end the session"),
        "{messages:?}"
    );
}

#[test]
fn test_a_session_end_request_the_harness_never_takes_ends_at_its_bound() {
    let (elapsed, messages) = logged(async {
        let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::Hold).await;
        let agent = spawned.agent().await;
        let releaser = hold_the_sink(agent, GUARD).await;
        let (_, elapsed) = timed(agent.end_session()).await;
        // Dropping the task drops the sink's guard.
        releaser.abort();
        elapsed
    });
    assert!(at(elapsed, SESSION_END_TIMEOUT), "{elapsed:?}");
    assert!(
        logs(&messages, "did not take the session-end request"),
        "{messages:?}"
    );
}

#[test]
fn test_session_end_runs_the_hook_and_returns_on_the_harness_confirmation() {
    let ran = Arc::new(AtomicBool::new(false));
    let hook_ran = Arc::clone(&ran);
    let builder = AntigravityAgent::builder().on_session_end(move || {
        hook_ran.store(true, Ordering::SeqCst);
    });
    let (elapsed, messages) = logged(async {
        let mut spawned = Spawned::new(builder, true, Then::EndSessionWithHook).await;
        let agent = spawned.agent().await;
        real_time(agent.end_session()).await
    });
    assert!(ran.load(Ordering::SeqCst), "the session-end hook ran");
    assert!(elapsed < SESSION_END_TIMEOUT / 2, "{elapsed:?}");
    // The cancelled trajectory between the hook and the confirmation is
    // skipped, not taken for the end.
    assert!(logs(&messages, "Ignoring a late event"), "{messages:?}");
    assert!(!logs(&messages, "did not finish"), "{messages:?}");
}

#[test]
fn test_session_end_returns_when_the_harness_closes_the_connection() {
    let (elapsed, messages) = logged(async {
        let mut spawned =
            Spawned::new(AntigravityAgent::builder(), true, Then::CloseOnSessionEnd).await;
        let agent = spawned.agent().await;
        real_time(agent.end_session()).await
    });
    assert!(elapsed < SESSION_END_TIMEOUT / 2, "{elapsed:?}");
    assert!(!logs(&messages, "did not finish"), "{messages:?}");
}

#[test]
fn test_a_session_end_hook_that_overruns_ends_session_end_at_its_bound() {
    let (elapsed, messages) = logged(async {
        // The hook's request is sent unasked, so it waits unread.
        let mut spawned = Spawned::new(AntigravityAgent::builder(), true, Then::EndHook).await;
        let agent = spawned.agent().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let (release, hook) = blocking_hook();
        agent.lifecycle.session_end = Some(hook);
        let (_, elapsed) = timed(agent.end_session()).await;
        drop(release);
        elapsed
    });
    assert!(at(elapsed, SESSION_END_TIMEOUT), "{elapsed:?}");
    assert!(
        logs(&messages, "The session-end hook did not finish"),
        "{messages:?}"
    );
}
