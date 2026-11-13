// The stand-in harness is a shell script.
#![cfg(unix)]

use super::session_lifecycle::{INIT_TIMEOUT, SESSION_END_TIMEOUT};
use super::*;
use crate::antigravity::DEFAULT_TURN_TIMEOUT;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

/// A stand-in harness in `dir`: a script that answers the stdio handshake
/// with `port`, where the test's own WebSocket peer listens, then reads
/// stdin until its EOF, as the real harness does.
fn fake_harness(dir: &std::path::Path, port: u16) -> std::path::PathBuf {
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
            "#!/bin/sh\ncat '{}'\nexec cat >/dev/null\n",
            frame_path.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// The stand-in harness's WebSocket: answers the conversation's
/// initialization, then never reads again, as a wedged harness does. It
/// holds the connection until `release` resolves.
async fn answer_init_then_stop_reading(
    listener: tokio::net::TcpListener,
    release: tokio::sync::oneshot::Receiver<()>,
) {
    let (socket, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
    ws.next().await.unwrap().unwrap();
    let answer = serde_json::json!({"initializeConversationResponse": {"cascadeId": "stalled"}});
    ws.send(Message::Text(answer.to_string().into()))
        .await
        .unwrap();
    let _ = release.await;
}

/// Whether `result` failed with the turn timeout.
fn timed_out_at<T>(result: &Result<T, AntigravityError>, turn_timeout: Duration) -> bool {
    matches!(result, Err(AntigravityError::Timeout { timeout, .. }) if *timeout == turn_timeout)
}

/// An agent spawned against the stand-in harness, whose WebSocket peer
/// stops reading once it has answered the initialization.
struct Stalled {
    agent: AntigravityAgent,
    release: tokio::sync::oneshot::Sender<()>,
    peer: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Stalled {
    /// Spawns `builder`'s agent, then pauses the clock: a stuck send then
    /// waits on the clock alone.
    async fn spawn(builder: AgentBuilder) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("localhost:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, released) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(answer_init_then_stop_reading(listener, released));
        let agent = builder
            .with_harness_path(fake_harness(dir.path(), port))
            .spawn()
            .await
            .expect("spawn");
        tokio::time::pause();
        Self {
            agent,
            release,
            peer,
            _dir: dir,
        }
    }

    /// Lets the peer go and resumes the clock, so a next spawn runs on
    /// real time.
    async fn finish(self) {
        tokio::time::resume();
        drop(self.agent);
        let _ = self.release.send(());
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
async fn hold_the_sink(agent: &AntigravityAgent, hold: Duration) -> tokio::task::JoinHandle<()> {
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

/// A harness request whose answer goes out mid-turn: a post-tool callback
/// with no post-tool hook set, answered with an empty result.
fn hook_request() -> OutputEvent {
    serde_json::from_value(serde_json::json!({
        "callHookRequest": {"requestId": "r-3470", "postToolArgs": {"toolName": "view_file"}}
    }))
    .unwrap()
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

#[tokio::test]
async fn test_the_turn_deadline_starts_before_the_wait_for_a_trigger_delivery() {
    // J-12: a trigger's delivery holds `turn_sync` across its send, up to
    // 30 s on a harness that stopped reading, and the deadline started
    // only after chat() had taken it.
    let turn_timeout = Duration::from_secs(10);
    for streaming in [false, true] {
        let mut stalled =
            Stalled::spawn(AntigravityAgent::builder().with_turn_timeout(turn_timeout)).await;
        let agent = &mut stalled.agent;
        let delivery = Arc::clone(&agent.turn_sync).lock_owned().await;
        let releaser = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            drop(delivery);
        });
        let started = tokio::time::Instant::now();
        let result = if streaming {
            tokio::time::timeout(GUARD, agent.send_streaming("hi"))
                .await
                .expect("send_streaming gives up rather than hanging")
                .map(|_| ())
        } else {
            tokio::time::timeout(GUARD, agent.chat("hi"))
                .await
                .expect("chat gives up rather than hanging")
                .map(|_| ())
        };
        let elapsed = started.elapsed();
        assert!(timed_out_at(&result, turn_timeout), "{result:?}");
        let Err(AntigravityError::Timeout { operation, .. }) = &result else {
            unreachable!()
        };
        assert!(operation.contains("trigger"), "{operation}");
        assert!(
            elapsed >= turn_timeout && elapsed < turn_timeout + Duration::from_secs(1),
            "{elapsed:?}"
        );
        // Nothing was started: the agent is idle, with no turn to drain.
        assert!(*agent.idle.borrow());
        assert!(!agent.turn_unfinished);
        releaser.await.unwrap();
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
