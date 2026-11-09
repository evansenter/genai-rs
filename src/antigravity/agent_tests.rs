// The stand-in harness is a shell script.
#![cfg(unix)]

use super::*;

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

#[tokio::test]
async fn test_a_harness_that_stops_reading_fails_each_turn_at_its_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("localhost:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release, released) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(answer_init_then_stop_reading(listener, released));
    let turn_timeout = Duration::from_secs(10);
    let mut agent = AntigravityAgent::builder()
        .with_harness_path(fake_harness(dir.path(), port))
        .with_turn_timeout(turn_timeout)
        .spawn()
        .await
        .expect("spawn");
    // Paused once spawned: a stuck send then waits on the clock alone.
    tokio::time::pause();
    let guard = Duration::from_secs(600);

    // A prompt larger than loopback's socket buffers (inline media can be),
    // so its send cannot complete. The turn deadline bounds it.
    let started = tokio::time::Instant::now();
    let first = tokio::time::timeout(guard, agent.chat("x".repeat(32 << 20)))
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

    // The turn never ended, so the next one sends a halt first. That send
    // is stuck behind the prompt's, and the halt-and-drain budget bounds
    // it; then the turn deadline bounds the new message's.
    let started = tokio::time::Instant::now();
    let second = tokio::time::timeout(guard, agent.chat("again"))
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
    let streamed = tokio::time::timeout(guard, agent.send_streaming("and again"))
        .await
        .expect("send_streaming gives up rather than hanging")
        .map(|_| ());
    let elapsed = started.elapsed();
    assert!(timed_out_at(&streamed, turn_timeout), "{streamed:?}");
    assert!(
        elapsed < HALT_DRAIN_BUDGET + turn_timeout + Duration::from_secs(1),
        "{elapsed:?}"
    );

    drop(agent);
    let _ = release.send(());
    peer.await.unwrap();
}
