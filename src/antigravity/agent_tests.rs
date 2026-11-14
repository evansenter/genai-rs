// The stand-in harness is a shell script.
#![cfg(unix)]

use super::session_lifecycle::{INIT_TIMEOUT, SESSION_END_TIMEOUT};
use super::*;
use crate::antigravity::DEFAULT_TURN_TIMEOUT;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

/// The line the stand-in harness writes to its stderr as it starts, which
/// an error carrying the stderr tail shows.
pub(super) const STAND_IN_STDERR: &str = "stand-in harness: listening";

/// A stand-in harness in `dir`: a script that writes [`STAND_IN_STDERR`]
/// to its stderr, answers the stdio handshake with `port`, where the test's
/// own WebSocket peer listens, then reads stdin until its EOF, as the real
/// harness does.
pub(super) fn fake_harness(dir: &std::path::Path, port: u16) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    // The OutputConfig: field 1, the port (a varint); field 2, the token.
    let mut payload = vec![0x08];
    let mut port = u32::from(port);
    while port >= 0x80 {
        payload.push(u8::try_from(port & 0x7f).unwrap() | 0x80);
        port >>= 7;
    }
    payload.push(u8::try_from(port).unwrap());
    payload.extend_from_slice(&[0x12, 5]);
    payload.extend_from_slice(b"token");
    let mut frame = u32::try_from(payload.len()).unwrap().to_le_bytes().to_vec();
    frame.extend_from_slice(&payload);
    let frame_path = dir.join("output_config.bin");
    std::fs::write(&frame_path, frame).unwrap();

    let script = dir.join("localharness");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho '{STAND_IN_STDERR}' >&2\ncat '{}'\nexec cat >/dev/null\n",
            frame_path.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// The stand-in harness's WebSocket: answers the conversation's
/// initialization and sends `then`, then never reads again, as a wedged
/// harness does. It says so on `sent`, and holds the connection until
/// `release` resolves.
async fn answer_init_then_stop_reading(
    listener: tokio::net::TcpListener,
    then: Vec<Message>,
    sent: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
) {
    let (socket, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
    ws.next().await.unwrap().unwrap();
    let answer = serde_json::json!({"initializeConversationResponse": {"cascadeId": "stalled"}});
    ws.send(Message::Text(answer.to_string().into()))
        .await
        .unwrap();
    for message in then {
        ws.send(message).await.unwrap();
    }
    let _ = sent.send(());
    let _ = release.await;
}

/// A harness message, as the stand-in's peer sends it.
fn text(event: &serde_json::Value) -> Message {
    Message::Text(event.to_string().into())
}

/// Whether `result` failed with the turn timeout.
fn timed_out_at<T>(result: &Result<T, AntigravityError>, turn_timeout: Duration) -> bool {
    matches!(result, Err(AntigravityError::Timeout { timeout, .. }) if *timeout == turn_timeout)
}

/// An agent spawned against the stand-in harness, whose WebSocket peer
/// stops reading once it has answered the initialization.
struct Stalled {
    agent: AntigravityAgent,
    release: Option<tokio::sync::oneshot::Sender<()>>,
    peer: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Stalled {
    /// Spawns `builder`'s agent, then pauses the clock: a stuck send then
    /// waits on the clock alone.
    async fn spawn(builder: AgentBuilder) -> Self {
        Self::spawn_then(builder, Vec::new()).await
    }

    /// As [`spawn`](Self::spawn), with the peer sending `then` after the
    /// initialization's answer. The agent reads none of it until a call
    /// does, by when it has been waiting in the socket on the real clock
    /// (the stderr wait below), so a paused clock cannot outrun it.
    async fn spawn_then(builder: AgentBuilder, then: Vec<Message>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("localhost:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, released) = tokio::sync::oneshot::channel();
        let (sent, all_sent) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(answer_init_then_stop_reading(
            listener, then, sent, released,
        ));
        let agent = builder
            .with_harness_path(fake_harness(dir.path(), port))
            .spawn()
            .await
            .expect("spawn");
        all_sent.await.unwrap();
        // The stand-in's stderr line is in, so an error's tail shows it.
        let mut stderr = String::new();
        for _ in 0..100 {
            stderr = agent.harness.stderr_tail().await;
            if stderr.contains(STAND_IN_STDERR) {
                break;
            }
        }
        assert!(stderr.contains(STAND_IN_STDERR), "{stderr}");
        tokio::time::pause();
        Self {
            agent,
            release: Some(release),
            peer,
            _dir: dir,
        }
    }

    /// Lets the peer go mid-test: it drops the connection, as a harness
    /// that ended its run does once it has sent its last events.
    async fn hang_up(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        while !self.peer.is_finished() {
            tokio::task::yield_now().await;
        }
    }

    /// Lets the peer go and resumes the clock, so a next spawn runs on
    /// real time.
    async fn finish(mut self) {
        tokio::time::resume();
        drop(self.agent);
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        self.peer.await.unwrap();
    }
}

/// Longer than any bound under test: a call still pending after it hangs.
const GUARD: Duration = Duration::from_secs(900);

/// A prompt larger than loopback's socket buffers (inline media can be),
/// so its send to a peer that does not read cannot complete.
fn oversized_prompt() -> String {
    "x".repeat(32 << 20)
}

#[tokio::test]
async fn test_a_harness_that_stops_reading_fails_each_turn_at_its_deadline() {
    // A turn timeout shorter than a send's own 30 s bound, and the default
    // one, which is longer: each turn fails at its own deadline.
    let short = Duration::from_secs(10);
    for (builder, turn_timeout) in [
        (AntigravityAgent::builder().with_turn_timeout(short), short),
        (AntigravityAgent::builder(), DEFAULT_TURN_TIMEOUT),
    ] {
        let mut stalled = Stalled::spawn(builder).await;
        let agent = &mut stalled.agent;

        let started = tokio::time::Instant::now();
        let first = tokio::time::timeout(GUARD, agent.chat(oversized_prompt()))
            .await
            .expect("chat gives up rather than hanging");
        let elapsed = started.elapsed();
        assert!(timed_out_at(&first, turn_timeout), "{first:?}");
        let Err(AntigravityError::Timeout { operation, .. }) = &first else {
            unreachable!()
        };
        assert!(
            operation.contains("did not take the message"),
            "{operation}"
        );
        assert!(
            elapsed >= turn_timeout && elapsed < turn_timeout + Duration::from_secs(1),
            "{elapsed:?}"
        );

        // The turn never ended, so the next one sends a halt first. That
        // send is stuck behind the prompt's, and the halt-and-drain budget
        // bounds it; then the turn deadline bounds the new message's.
        let started = tokio::time::Instant::now();
        let second = tokio::time::timeout(GUARD, agent.chat("again"))
            .await
            .expect("chat gives up rather than hanging");
        let elapsed = started.elapsed();
        assert!(timed_out_at(&second, turn_timeout), "{second:?}");
        assert!(
            elapsed < HALT_DRAIN_BUDGET + turn_timeout + Duration::from_secs(1),
            "{elapsed:?}"
        );

        // send_streaming fails the same way, before it returns a stream.
        let started = tokio::time::Instant::now();
        let streamed = tokio::time::timeout(GUARD, agent.send_streaming("and again"))
            .await
            .expect("send_streaming gives up rather than hanging")
            .map(|_| ());
        let elapsed = started.elapsed();
        assert!(timed_out_at(&streamed, turn_timeout), "{streamed:?}");
        assert!(
            elapsed < HALT_DRAIN_BUDGET + turn_timeout + Duration::from_secs(1),
            "{elapsed:?}"
        );

        stalled.finish().await;
    }
}

/// Holds `agent`'s sink, as a send stuck on the harness does, for `hold`.
pub(super) async fn hold_the_sink(
    agent: &AntigravityAgent,
    hold: Duration,
) -> tokio::task::JoinHandle<()> {
    let held = agent.session.hold_sink().await;
    tokio::spawn(async move {
        tokio::time::sleep(hold).await;
        drop(held);
    })
}

#[tokio::test]
async fn test_initialization_and_session_end_count_their_send_against_their_budget() {
    let mut stalled = Stalled::spawn(AntigravityAgent::builder()).await;
    let agent = &mut stalled.agent;

    // The session-end request goes out late, two thirds into
    // SESSION_END_TIMEOUT, and the wait for the hooks gets what is left
    // rather than a full budget of its own.
    let releaser = hold_the_sink(agent, SESSION_END_TIMEOUT * 2 / 3).await;
    let started = tokio::time::Instant::now();
    tokio::time::timeout(GUARD, agent.end_session())
        .await
        .expect("end_session gives up rather than hanging");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= SESSION_END_TIMEOUT && elapsed < SESSION_END_TIMEOUT + Duration::from_secs(1),
        "{elapsed:?}"
    );
    releaser.await.unwrap();

    // An initialization message the harness never takes fails within
    // INIT_TIMEOUT, as the initialization failing (`spawn()` reports
    // `InitFailed`, with stderr) rather than as a bare send timeout.
    let releaser = hold_the_sink(agent, INIT_TIMEOUT * 2).await;
    let started = tokio::time::Instant::now();
    let init = tokio::time::timeout(
        GUARD,
        agent.initialize(protocol::InitializeConversationEvent::default()),
    )
    .await
    .expect("initialize gives up rather than hanging");
    let elapsed = started.elapsed();
    let Err(AntigravityError::InitFailed { message, .. }) = &init else {
        panic!("expected InitFailed: {init:?}");
    };
    assert!(
        message.contains("did not take the initialization message"),
        "{message}"
    );
    assert!(
        elapsed >= INIT_TIMEOUT && elapsed < INIT_TIMEOUT + Duration::from_secs(1),
        "{elapsed:?}"
    );
    releaser.await.unwrap();

    stalled.finish().await;
}

/// A harness request whose answer goes out mid-turn: a post-tool callback,
/// answered with an empty result (after the post-tool hook, when one is
/// set).
fn post_tool_request() -> serde_json::Value {
    serde_json::json!({
        "callHookRequest": {"requestId": "r-3470", "postToolArgs": {"toolName": "view_file"}}
    })
}

/// [`post_tool_request`] as the agent reads it.
fn hook_request() -> OutputEvent {
    serde_json::from_value(post_tool_request()).unwrap()
}

#[tokio::test]
async fn test_a_reply_mid_turn_is_bounded_by_the_turn_deadline_alone() {
    // #552: replies went through the session's 30 s bound on a send, so a
    // turn whose deadline was further off failed at 30 s.
    let mut stalled = Stalled::spawn(AntigravityAgent::builder()).await;
    let agent = &mut stalled.agent;

    // Held past 30 s, inside a two-minute turn: the reply goes out.
    let mut turn = TurnState::new(Some(Duration::from_secs(120)), None);
    let releaser = hold_the_sink(agent, Duration::from_secs(45)).await;
    let started = tokio::time::Instant::now();
    let replied = tokio::time::timeout(GUARD, agent.process_event(hook_request(), &mut turn))
        .await
        .expect("the reply gives up rather than hanging");
    assert!(replied.is_ok(), "{replied:?}");
    assert!(started.elapsed() >= Duration::from_secs(45));
    releaser.await.unwrap();

    // Held past the turn deadline: the turn fails there, naming the reply.
    let turn_timeout = Duration::from_secs(60);
    let mut turn = TurnState::new(Some(turn_timeout), None);
    let releaser = hold_the_sink(agent, Duration::from_secs(200)).await;
    let started = tokio::time::Instant::now();
    let replied = tokio::time::timeout(GUARD, agent.process_event(hook_request(), &mut turn))
        .await
        .expect("the reply gives up rather than hanging");
    let elapsed = started.elapsed();
    assert!(timed_out_at(&replied, turn_timeout), "{replied:?}");
    let Err(AntigravityError::Timeout { operation, .. }) = &replied else {
        unreachable!()
    };
    assert!(operation.contains("a hook's answer"), "{operation}");
    assert!(
        elapsed >= turn_timeout && elapsed < turn_timeout + Duration::from_secs(1),
        "{elapsed:?}"
    );
    releaser.await.unwrap();

    stalled.finish().await;
}

/// How long [`TurnCall::NextTriggerTurn`] waits for a turn to start.
const TRIGGER_WAIT: Duration = Duration::from_secs(5);

/// A call that starts a turn or waits for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnCall {
    Chat,
    SendStreaming,
    NextTriggerTurn,
}

impl TurnCall {
    const ALL: [Self; 3] = [Self::Chat, Self::SendStreaming, Self::NextTriggerTurn];

    /// Makes the call on `agent`, failing the test when it hangs past
    /// [`GUARD`]. `next_trigger_turn` waits up to [`TRIGGER_WAIT`].
    async fn make(self, agent: &mut AntigravityAgent) -> Result<(), AntigravityError> {
        let call = async {
            match self {
                Self::Chat => agent.chat("hi").await.map(|_| ()),
                Self::SendStreaming => agent.send_streaming("hi").await.map(|_| ()),
                Self::NextTriggerTurn => agent.next_trigger_turn(TRIGGER_WAIT).await.map(|_| ()),
            }
        };
        tokio::time::timeout(GUARD, call)
            .await
            .unwrap_or_else(|_| panic!("{self:?} gives up rather than hanging"))
    }
}

/// The main trajectory's first update of a turn, as a trigger's turn
/// starts.
fn running_update() -> serde_json::Value {
    serde_json::json!({"trajectoryStateUpdate": {"trajectoryId": "stalled", "state": "STATE_RUNNING"}})
}

#[tokio::test]
async fn test_the_turn_deadline_starts_before_the_wait_for_a_trigger_delivery() {
    // J-12: a trigger's delivery holds `turn_sync` across its send, up to
    // 30 s on a harness that stopped reading, and the deadline started
    // only after chat() had taken it. K-2 (#504): next_trigger_turn's wait,
    // once its turn's first event is in, had no test.
    let turn_timeout = Duration::from_secs(10);
    for call in TurnCall::ALL {
        let trigger_turn = call == TurnCall::NextTriggerTurn;
        // A trigger's turn has started, for next_trigger_turn to take up.
        let then = if trigger_turn {
            vec![text(&running_update())]
        } else {
            Vec::new()
        };
        let builder = AntigravityAgent::builder().with_turn_timeout(turn_timeout);
        let mut stalled = Stalled::spawn_then(builder, then).await;
        let agent = &mut stalled.agent;
        let delivery = Arc::clone(&agent.turn_sync).lock_owned().await;
        let releaser = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            drop(delivery);
        });
        let started = tokio::time::Instant::now();
        let result = call.make(agent).await;
        let elapsed = started.elapsed();
        assert!(timed_out_at(&result, turn_timeout), "{call:?}: {result:?}");
        let Err(AntigravityError::Timeout { operation, .. }) = &result else {
            unreachable!()
        };
        assert!(operation.contains("trigger"), "{call:?}: {operation}");
        // next_trigger_turn's deadline starts with the first event. Read on
        // the paused clock, it can come as the clock jumps to the wait's
        // end; either way the delivery still holds the lock.
        let read = if trigger_turn {
            TRIGGER_WAIT
        } else {
            Duration::ZERO
        };
        assert!(
            elapsed >= turn_timeout && elapsed < turn_timeout + read + Duration::from_secs(1),
            "{call:?}: {elapsed:?}"
        );
        // Nothing was started: the agent is idle. A chat leaves no turn to
        // drain; a trigger's turn is left for the next chat to halt.
        assert!(*agent.idle.borrow());
        assert_eq!(agent.turn_unfinished, trigger_turn, "{call:?}");
        releaser.await.unwrap();
        stalled.finish().await;
    }
}

/// Records the kind of each message the agent sends the harness: its
/// payload's one key (`userInput`, `haltRequest`, ...).
#[derive(Default)]
struct Sends(std::sync::Mutex<Vec<String>>);

impl crate::wire::WireInspector for Sends {
    fn on_event(&self, event: &crate::wire::WireEvent) {
        if let crate::wire::WireEvent::WsSend { payload, .. } = event {
            let keys = payload.as_object().into_iter().flat_map(|o| o.keys());
            self.0.lock().unwrap().extend(keys.cloned());
        }
    }
}

impl Sends {
    /// The kinds sent since the last call.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// Asserts that `result` is a refusal to start or wait for a turn after
/// the session ended: `ConnectionClosed`, its message containing `cause`,
/// with the harness's stderr.
fn assert_refused(result: &Result<(), AntigravityError>, cause: &str) {
    let Err(AntigravityError::ConnectionClosed { message, stderr }) = result else {
        panic!("expected ConnectionClosed: {result:?}");
    };
    assert!(message.contains(cause), "{message}");
    assert!(stderr.contains(STAND_IN_STDERR), "{stderr}");
}

#[tokio::test]
async fn test_once_the_session_ended_each_turn_call_refuses_without_sending() {
    // K-2 (#504): only chat's refusal had a test. Each call fails with
    // ConnectionClosed at once: a send would go to a harness that ended
    // its run, and a wait would wait on nothing.
    let sends = Arc::new(Sends::default());
    let builder = AntigravityAgent::builder().add_wire_inspector(sends.clone());
    let mut stalled = Stalled::spawn(builder).await;
    let agent = &mut stalled.agent;
    agent.session_ended = Some(SessionEnd::Failed(TurnFailure::Trajectory));
    sends.take();
    for call in TurnCall::ALL {
        let started = tokio::time::Instant::now();
        let result = call.make(agent).await;
        assert_refused(&result, "ended the session");
        // At once: the stderr tail's brief yield, not TRIGGER_WAIT.
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{call:?} waited"
        );
        assert_eq!(sends.take(), Vec::<String>::new(), "{call:?} sent");
    }
    stalled.finish().await;
}

/// A system error step of the main trajectory with HTTP status `status`,
/// the step harness 0.1.23 sends when the model backend fails a call.
fn backend_error_step(status: u32) -> serde_json::Value {
    serde_json::json!({"stepUpdate": {
        "trajectoryId": "stalled", "stepIndex": 1, "source": "SOURCE_SYSTEM",
        "target": "TARGET_USER", "state": "STATE_ERROR",
        "error": {"errorMessage": "Agent execution terminated due to error.", "httpCode": status}
    }})
}

#[tokio::test]
async fn test_a_turn_that_ends_the_session_while_drained_fails_the_next_turn_unsent() {
    // K-3 (#504): a trigger's turn that the model backend failed, left for
    // the next chat to discard, ended the session unseen: the chat sent its
    // message into the closed connection and failed with a WebSocket error.
    // Harness 0.1.23 sends the error step, then the trajectory's idle state
    // with the error, then closes; or the harness just closes.
    let idle = serde_json::json!({"trajectoryStateUpdate": {
        "trajectoryId": "stalled", "state": "STATE_FULLY_IDLE", "error": "Error 400"
    }});
    let cases = [
        (
            vec![
                text(&backend_error_step(400)),
                text(&idle),
                Message::Close(None),
            ],
            "an earlier turn ended the session (model backend error, HTTP 400)",
        ),
        (vec![Message::Close(None)], "the harness ended the session"),
    ];
    for (then, cause) in cases {
        let sends = Arc::new(Sends::default());
        let builder = AntigravityAgent::builder().add_wire_inspector(sends.clone());
        let mut stalled = Stalled::spawn_then(builder, then).await;
        let agent = &mut stalled.agent;
        // A trigger delivered, and its turn ran as above.
        agent
            .trigger_fired
            .store(true, std::sync::atomic::Ordering::SeqCst);
        sends.take();
        // The chat halts the trigger's turn and drains it, then refuses.
        let first = TurnCall::Chat.make(agent).await;
        assert_refused(&first, cause);
        assert_eq!(sends.take(), ["haltRequest"], "the message was not sent");
        // The next call refuses at once.
        let second = TurnCall::Chat.make(agent).await;
        assert_refused(&second, cause);
        assert_eq!(sends.take(), Vec::<String>::new());
        stalled.finish().await;
    }
}

/// The main trajectory's idle state with the error of a failed run, as
/// harness 0.1.23 sends it after the error step.
fn failed_idle() -> serde_json::Value {
    serde_json::json!({"trajectoryStateUpdate": {
        "trajectoryId": "stalled", "state": "STATE_FULLY_IDLE", "error": "Error 400"
    }})
}

#[tokio::test]
async fn test_a_halt_that_cannot_be_sent_still_reads_that_the_session_ended() {
    // #504: an interval trigger keeps delivering while the agent is idle,
    // into the connection of a harness that ended its run, until a write
    // draws the reset. The chat's halt then failed, and the drain gave up
    // without reading the failure: the chat sent its message and failed
    // with a WebSocket error, as did every chat after it.
    let then = vec![
        text(&backend_error_step(400)),
        text(&failed_idle()),
        Message::Close(None),
    ];
    let sends = Arc::new(Sends::default());
    let builder = AntigravityAgent::builder().add_wire_inspector(sends.clone());
    let mut stalled = Stalled::spawn_then(builder, then).await;
    stalled.hang_up().await;
    let agent = &mut stalled.agent;
    let deliver = agent.trigger_delivery();
    let mut refused = false;
    for _ in 0..200 {
        if deliver("tick".to_string()).await.is_err() {
            refused = true;
            break;
        }
        // Real time, for the peer's reset to come back.
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(refused, "no delivery into the closed connection failed");
    sends.take();
    // The halt fails, and the chat reads on: the failure, or, where the
    // reset discarded it, the connection's end.
    let first = TurnCall::Chat.make(agent).await;
    assert_refused(&first, "ended the session");
    assert_eq!(sends.take(), ["haltRequest"], "the message was not sent");
    let second = TurnCall::Chat.make(agent).await;
    assert_refused(&second, "ended the session");
    assert_eq!(sends.take(), Vec::<String>::new());
    stalled.finish().await;
}

#[tokio::test]
async fn test_a_turn_timeout_whose_drain_sees_the_session_end_fails_the_next_turn_unsent() {
    // #504: the drain after a turn timeout records what ended the session
    // as begin_turn's does: the turn's own failure, which came after its
    // deadline, or the harness closing the connection. The next call
    // refuses at once, where it would halt again and send its message.
    let turn_timeout = Duration::from_secs(10);
    let cases = [
        (
            vec![
                text(&backend_error_step(400)),
                text(&failed_idle()),
                Message::Close(None),
            ],
            "an earlier turn ended the session (model backend error, HTTP 400)",
        ),
        (vec![Message::Close(None)], "the harness ended the session"),
    ];
    for (rest, cause) in cases {
        // The turn runs a post-tool hook that outlasts its deadline (it
        // returns once `release` drops); what the harness sends after that
        // request is left to the drain.
        let mut then = vec![text(&running_update()), text(&post_tool_request())];
        then.extend(rest);
        let (release, released) = std::sync::mpsc::channel::<()>();
        let released = std::sync::Mutex::new(released);
        let sends = Arc::new(Sends::default());
        let builder = AntigravityAgent::builder()
            .with_turn_timeout(turn_timeout)
            .on_post_tool(move |_| {
                let _ = released.lock().unwrap().recv();
            })
            .add_wire_inspector(sends.clone());
        let mut stalled = Stalled::spawn_then(builder, then).await;
        let agent = &mut stalled.agent;
        sends.take();
        let first = TurnCall::Chat.make(agent).await;
        assert!(timed_out_at(&first, turn_timeout), "{first:?}");
        assert_eq!(sends.take(), ["userInput", "haltRequest"]);
        for call in TurnCall::ALL {
            let result = call.make(agent).await;
            assert_refused(&result, cause);
            assert_eq!(sends.take(), Vec::<String>::new(), "{call:?} sent");
        }
        drop(release);
        stalled.finish().await;
    }
}

#[tokio::test]
async fn test_a_turn_timeout_too_large_for_the_clock_runs_without_a_deadline() {
    // J-12: `with_turn_timeout(Duration::MAX)` panicked at the first turn.
    let mut stalled =
        Stalled::spawn(AntigravityAgent::builder().with_turn_timeout(Duration::MAX)).await;
    let waited = tokio::time::timeout(GUARD, stalled.agent.chat("hi")).await;
    assert!(waited.is_err(), "no deadline ends the turn: {waited:?}");
    stalled.finish().await;
}

#[test]
fn test_halting_an_earlier_turn_counts_against_the_turn_deadline() {
    // J-12: the halt-and-drain before a turn ran on its own five-second
    // budget ahead of the deadline. Now a deadline that comes first cuts it
    // short, and the turn's message is not sent past it.
    let turn_timeout = Duration::from_secs(3);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let logged = crate::test_subscriber::capture_messages(|| {
        runtime.block_on(async {
            let mut stalled =
                Stalled::spawn(AntigravityAgent::builder().with_turn_timeout(turn_timeout)).await;
            let agent = &mut stalled.agent;
            // An earlier turn never ended, and the harness does not take
            // the halt.
            agent.turn_unfinished = true;
            let releaser = hold_the_sink(agent, Duration::from_secs(20)).await;
            let started = tokio::time::Instant::now();
            let result = tokio::time::timeout(GUARD, agent.chat("hi"))
                .await
                .expect("chat gives up rather than hanging");
            let elapsed = started.elapsed();
            assert!(timed_out_at(&result, turn_timeout), "{result:?}");
            let Err(AntigravityError::Timeout { operation, .. }) = &result else {
                unreachable!()
            };
            assert!(
                operation.contains("before the message was sent"),
                "{operation}"
            );
            assert!(
                elapsed >= turn_timeout && elapsed < turn_timeout + Duration::from_secs(1),
                "{elapsed:?}"
            );
            releaser.await.unwrap();
            stalled.finish().await;
        });
    });
    // The warning names the bound that ran out: the turn's deadline, not
    // the drain's own five seconds.
    let halt = logged
        .iter()
        .find(|m| m.contains("did not take the halt request"))
        .unwrap_or_else(|| panic!("no halt warning: {logged:?}"));
    assert!(halt.contains("the turn's deadline"), "{halt}");
}
