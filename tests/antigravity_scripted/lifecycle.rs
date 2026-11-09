//! The harness dying under a call (#504): a killed harness fails the call
//! waiting on it at once, with `ConnectionClosed` and the harness's stderr,
//! rather than at its turn timeout or with a bare transport error. Unix
//! only: the tests kill the harness by its pid.

use super::*;
use genai_rs::wire::{WireEvent, WireInspector};

/// Records the harness's pid from its spawn event.
#[derive(Default)]
struct HarnessPid(Mutex<Option<u32>>);

impl WireInspector for HarnessPid {
    fn on_event(&self, event: &WireEvent) {
        if let WireEvent::HarnessSpawn { pid, .. } = event {
            *self.0.lock().unwrap() = *pid;
        }
    }
}

impl HarnessPid {
    fn get(&self) -> u32 {
        self.0
            .lock()
            .unwrap()
            .expect("the spawn event names the pid")
    }
}

/// Kills `pid` with SIGKILL, as a crash would end it: no close frame, no
/// last words on the socket.
fn kill_harness(pid: u32) {
    let status = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("run kill");
    assert!(status.success(), "kill -9 {pid}: {status}");
}

/// An agent on `fake` whose harness pid `pid` records. A healthy harness
/// writes nothing to stderr (0.1.23, 2026-11-09), so the Go runtime's init
/// trace (`GODEBUG=inittrace=1`, a line per package it initializes) gives
/// it a stderr for the error to carry.
async fn agent_with_pid(fake: &FakeGemini, pid: &Arc<HarnessPid>) -> AntigravityAgent {
    let inspector: Arc<dyn WireInspector> = pid.clone();
    scripted_agent(fake)
        .add_wire_inspector(inspector)
        .add_harness_env("GODEBUG", "inittrace=1")
        .spawn()
        .await
        .expect("spawn")
}

/// Asserts `err` is the harness's connection closing, with the tail of its
/// stderr, and returns the message.
fn connection_closed(err: &AntigravityError) -> &str {
    let AntigravityError::ConnectionClosed { message, stderr } = err else {
        panic!("a killed harness must fail the call with ConnectionClosed, got {err:?}");
    };
    assert!(
        stderr.contains("init "),
        "the error carries the harness's stderr: {err:?}"
    );
    message
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_harness_killed_mid_turn_fails_the_turn_at_once() {
    // The model holds the turn for a minute; the harness dies inside it.
    let fake = Arc::new(
        FakeGemini::start(vec![
            ModelTurn::text("never-delivered-5501").delayed(Duration::from_secs(60)),
        ])
        .await,
    );
    let pid = Arc::new(HarnessPid::default());
    let mut agent = agent_with_pid(&fake, &pid).await;
    let harness = pid.get();
    let watcher = Arc::clone(&fake);
    let killer = tokio::spawn(async move {
        assert!(
            watcher.wait_for_requests(1, Duration::from_secs(20)).await,
            "the turn never reached the model"
        );
        kill_harness(harness);
    });

    let start = Instant::now();
    let err = agent
        .chat("this turn loses its harness")
        .await
        .expect_err("the harness died mid-turn");
    killer.await.expect("killer");
    let elapsed = start.elapsed();
    let message = connection_closed(&err);
    assert!(message.contains("mid-turn"), "{message}");
    assert!(
        elapsed < Duration::from_secs(20),
        "the turn failed after {elapsed:?}, waiting on a dead harness"
    );
    drop(agent);
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_harness_killed_while_next_trigger_turn_waits_fails_it_at_once() {
    let fake = FakeGemini::start(vec![ModelTurn::text("no-trigger-turn-5502")]).await;
    let pid = Arc::new(HarnessPid::default());
    let mut agent = agent_with_pid(&fake, &pid).await;
    let harness = pid.get();
    let killer = tokio::spawn(async move {
        // Long enough for next_trigger_turn to be waiting.
        tokio::time::sleep(Duration::from_millis(500)).await;
        kill_harness(harness);
    });

    let start = Instant::now();
    let err = agent
        .next_trigger_turn(Duration::from_secs(60))
        .await
        .expect_err("the harness died while the agent waited");
    killer.await.expect("killer");
    let elapsed = start.elapsed();
    let message = connection_closed(&err);
    assert!(message.contains("waiting for a trigger turn"), "{message}");
    assert!(
        elapsed < Duration::from_secs(20),
        "next_trigger_turn failed after {elapsed:?}, waiting on a dead harness"
    );
    assert!(fake.requests().is_empty(), "no turn started");
    drop(agent);
}
