//! Triggers (#493 item 7): custom and file-change triggers, the trigger-turn
//! consumer, and the turn-timeout and trigger-discard wire discipline
//! (moved here from `antigravity_harness.rs`, where they called the real
//! endpoint with a placeholder key).

use super::*;
use genai_rs::antigravity::{RetryConfig, StopDecision, TurnDecision, triggers};

/// Records every message this client sends the harness, and the lines the
/// harness writes to its stderr.
#[derive(Default)]
struct WireCapture {
    sends: Mutex<Vec<Value>>,
    stderr: Mutex<Vec<String>>,
}

impl genai_rs::wire::WireInspector for WireCapture {
    fn on_event(&self, event: &genai_rs::wire::WireEvent) {
        match event {
            genai_rs::wire::WireEvent::WsSend { payload, .. } => {
                self.sends.lock().unwrap().push(payload.clone());
            }
            genai_rs::wire::WireEvent::HarnessStderr { line, .. } => {
                self.stderr.lock().unwrap().push(line.clone());
            }
            _ => {}
        }
    }
}

impl WireCapture {
    /// The index of the first send carrying `key`.
    fn first(&self, key: &str) -> Option<usize> {
        self.sends
            .lock()
            .unwrap()
            .iter()
            .position(|p| p.get(key).is_some())
    }

    /// How many messages this client has sent.
    fn sent(&self) -> usize {
        self.sends.lock().unwrap().len()
    }

    /// How many of the messages this client has sent carry `key`.
    fn count(&self, key: &str) -> usize {
        self.sends
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.get(key).is_some())
            .count()
    }

    /// Waits up to `timeout` for a stderr line containing `needle`.
    async fn wait_for_stderr(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        while !self
            .stderr
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains(needle))
        {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        true
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_custom_trigger_turn_is_consumed_by_next_trigger_turn() {
    let fake = FakeGemini::start(vec![
        ModelTurn::text("trigger-reply-3411"),
        ModelTurn::text("user-reply-3412"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_custom_trigger(|ctx| async move { ctx.send("custom-trigger-3410").await })
        .spawn()
        .await
        .expect("spawn");

    let response = agent
        .next_trigger_turn(Duration::from_secs(20))
        .await
        .expect("trigger turn")
        .expect("the trigger started a turn");
    assert_eq!(response.text(), "trigger-reply-3411");
    assert!(
        fake.request_json(0)
            .unwrap()
            .to_string()
            .contains("custom-trigger-3410")
    );
    // Nothing else is coming.
    assert!(
        agent
            .next_trigger_turn(Duration::from_millis(500))
            .await
            .expect("no error")
            .is_none()
    );
    // A user turn still works after a consumed trigger turn.
    let next = agent.chat("hello").await.expect("turn");
    assert_eq!(next.text(), "user-reply-3412");
    agent.shutdown().await.expect("shutdown");
}

/// Harness 0.1.23 never asks the pre-turn hook about a trigger turn: a
/// gate there does not see it, though the turn's other hooks run. `spawn()`
/// warns about the pair for that reason. This fails the day the harness
/// starts asking, and the docs saying it does not must change with it.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_trigger_turn_skips_the_pre_turn_hook() {
    let fake = FakeGemini::start(vec![
        ModelTurn::text("trigger-reply-3451"),
        ModelTurn::text("user-reply-3452"),
    ])
    .await;
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let (pre, post, stop) = (Arc::clone(&log), Arc::clone(&log), Arc::clone(&log));
    let mut agent = scripted_agent(&fake)
        .add_custom_trigger(|ctx| async move { ctx.send("custom-trigger-3450").await })
        .on_pre_turn(move |turn| {
            pre.lock().unwrap().push(format!("pre_turn:{}", turn.text));
            // A gate that refuses everything, so a call would show twice.
            TurnDecision::deny("PRE-TURN-GATE-3453")
        })
        .on_post_turn(move |turn| {
            post.lock()
                .unwrap()
                .push(format!("post_turn:{}", turn.response_text));
        })
        .on_stop(move |_| {
            stop.lock().unwrap().push("stop".to_string());
            StopDecision::Stop
        })
        .spawn()
        .await
        .expect("spawn");

    let response = agent
        .next_trigger_turn(Duration::from_secs(20))
        .await
        .expect("the gate is not consulted, so the trigger turn runs")
        .expect("the trigger started a turn");
    assert_eq!(response.text(), "trigger-reply-3451");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["stop", "post_turn:trigger-reply-3451"],
        "a trigger turn runs the stop and post-turn hooks but not the pre-turn hook"
    );

    // A turn of the caller's own is asked about, and refused.
    let err = agent.chat("a user turn").await.unwrap_err();
    let AntigravityError::Turn { kind, message } = &err else {
        panic!("the pre-turn hook refuses a user turn: {err:?}");
    };
    assert_eq!(*kind, TurnFailure::Denied);
    assert!(message.contains("PRE-TURN-GATE-3453"), "{message}");
    assert_eq!(
        log.lock().unwrap().last().map(String::as_str),
        Some("pre_turn:a user turn")
    );
    assert_eq!(fake.requests().len(), 1, "the refused turn called no model");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_file_change_trigger_delivers_the_callbacks_message() {
    let watched = scratch_dir("agy-scripted-watch");
    let fake = FakeGemini::start(vec![ModelTurn::text("saw-it-3421")]).await;
    let mut agent = scripted_agent(&fake)
        .add_custom_trigger(triggers::on_file_change(
            watched.path(),
            Duration::from_millis(100),
            |changes| {
                Some(format!(
                    "file-change-3420: {}",
                    changes
                        .iter()
                        .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join(",")
                ))
            },
        ))
        .spawn()
        .await
        .expect("spawn");
    // No wait: the watch's baseline was taken when it was declared.
    write_file(&watched, "notes.md", "new");
    let response = agent
        .next_trigger_turn(Duration::from_secs(20))
        .await
        .expect("trigger turn")
        .expect("the change started a turn");
    assert_eq!(response.text(), "saw-it-3421");
    assert!(
        fake.request_json(0)
            .unwrap()
            .to_string()
            .contains("file-change-3420: notes.md")
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_turn_timeout_halts_the_harness_turn() {
    // The model answers after a minute; the 1s budget fires first, and the
    // recovery must halt the harness's still-running turn so the next turn
    // cannot consume its output.
    let fake = FakeGemini::start(vec![
        ModelTurn::text("too-late-3431").delayed(Duration::from_secs(60)),
        ModelTurn::text("next-turn-3432"),
    ])
    .await;
    let inspector = Arc::new(WireCapture::default());
    let mut agent = scripted_agent(&fake)
        .with_turn_timeout(Duration::from_secs(1))
        .add_wire_inspector(inspector.clone())
        .spawn()
        .await
        .expect("spawn");
    let err = agent.chat("this turn cannot finish").await.unwrap_err();
    assert!(
        matches!(err, AntigravityError::Timeout { .. }),
        "expected Timeout, got {err:?}"
    );
    let user_input = inspector.first("userInput").expect("userInput was sent");
    let halt = inspector
        .first("haltRequest")
        .expect("the timed-out turn must be halted");
    assert!(halt > user_input);

    let next = agent.chat("again").await.expect("next turn");
    assert_eq!(next.text(), "next-turn-3432");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_chat_after_a_trigger_discards_the_trigger_turn() {
    // The trigger's turn is still waiting on the model when chat() starts:
    // chat must halt and drain it, then run its own turn, whose answer is
    // the user's, not the trigger's.
    let fake = Arc::new(
        FakeGemini::start(vec![
            ModelTurn::text("stale-trigger-3441").delayed(Duration::from_secs(60)),
            ModelTurn::text("user-turn-3442"),
        ])
        .await,
    );
    let inspector = Arc::new(WireCapture::default());
    let mut agent = scripted_agent(&fake)
        .add_trigger(TriggerConfig::new("tick-3440", Duration::from_secs(1)))
        .add_wire_inspector(inspector.clone())
        .spawn()
        .await
        .expect("spawn");
    assert!(
        fake.wait_for_requests(1, Duration::from_secs(20)).await,
        "the trigger's turn never reached the model"
    );

    let response = agent.chat("hello after the trigger").await.expect("turn");
    assert_eq!(response.text(), "user-turn-3442");
    let trigger = inspector.first("automatedTrigger").expect("delivered");
    let halt = inspector
        .first("haltRequest")
        .expect("chat() must halt the unconsumed trigger turn");
    let user_input = inspector.first("userInput").expect("userInput was sent");
    assert!(
        trigger < halt && halt < user_input,
        "expected automatedTrigger < haltRequest < userInput"
    );
    agent.shutdown().await.expect("shutdown");
}

/// A trigger's turn that ends the session, left for the next chat to
/// discard (K-3, #504): a 400, and a 503 with no retries left. The chat
/// halts it, then fails with `ConnectionClosed` and the harness's stderr
/// without sending its message, and so does the chat after it. Both used
/// to send theirs into the closed connection and fail with a `WebSocket`
/// error.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_trigger_turn_that_ends_the_session_fails_the_next_chat_unsent() {
    for status in [400, 503] {
        let fake = FakeGemini::start(vec![ModelTurn::error(status, "scripted-3460")]).await;
        let inspector = Arc::new(WireCapture::default());
        let mut agent = scripted_agent(&fake)
            .add_custom_trigger(|ctx| async move { ctx.send("custom-trigger-3461").await })
            // No retries, which a 400 never gets: the first 503 ends it.
            .with_retry(RetryConfig::new().with_max_api_retries(0))
            .add_wire_inspector(inspector.clone())
            .spawn()
            .await
            .expect("spawn");
        // The harness's run has failed before the chat halts the turn.
        assert!(
            inspector
                .wait_for_stderr("executor run failed", Duration::from_secs(20))
                .await,
            "the trigger's turn never failed (HTTP {status})"
        );
        let delivered = inspector.sent();
        for (n, prompt) in ["first-chat-3462", "second-chat-3463"]
            .into_iter()
            .enumerate()
        {
            let err = agent.chat(prompt).await.unwrap_err();
            let AntigravityError::ConnectionClosed { message, stderr } = &err else {
                panic!("chat {n} after a trigger turn that ended the session: {err:?}");
            };
            let cause = format!("ended the session (model backend error, HTTP {status})");
            assert!(message.contains(&cause), "{message}");
            assert!(stderr.contains("executor run failed"), "{stderr}");
        }
        // The first chat sent only the halt that discarded the turn.
        assert_eq!(inspector.sent(), delivered + 1);
        assert_eq!(inspector.first("haltRequest"), Some(delivered));
        assert_eq!(inspector.first("userInput"), None);
        assert_eq!(
            fake.requests().len(),
            1,
            "the trigger's turn made the one call"
        );
        drop(agent);
    }
}

/// As above, with an interval trigger (#504): idle, the agent keeps
/// delivering after the session ended, into the closed connection, until a
/// delivery fails on the harness's reset. The chat's halt then fails too;
/// the chat still reads why the session ended, rather than sending its
/// message and failing with a `WebSocket` error. Where the reset leaves the
/// failure unread, the refusal says the harness ended the session.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_an_interval_trigger_into_the_ended_session_fails_the_next_chat_unsent() {
    let fake = FakeGemini::start(vec![ModelTurn::error(400, "scripted-3470")]).await;
    let inspector = Arc::new(WireCapture::default());
    let mut agent = scripted_agent(&fake)
        .add_trigger(TriggerConfig::new("tick-3471", Duration::from_millis(500)))
        .add_wire_inspector(inspector.clone())
        .spawn()
        .await
        .expect("spawn");
    assert!(
        inspector
            .wait_for_stderr("executor run failed", Duration::from_secs(20))
            .await,
        "the trigger's turn never failed"
    );
    // The trigger delivers into the closed connection until a send fails,
    // then stops.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut seen = inspector.count("automatedTrigger");
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let now = inspector.count("automatedTrigger");
        if now == seen && now >= 2 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the trigger kept delivering: {now}"
        );
        seen = now;
    }
    let delivered = inspector.sent();
    for (n, prompt) in ["first-chat-3472", "second-chat-3473"]
        .into_iter()
        .enumerate()
    {
        let err = agent.chat(prompt).await.unwrap_err();
        let AntigravityError::ConnectionClosed { message, stderr } = &err else {
            panic!("chat {n} after deliveries into the ended session: {err:?}");
        };
        assert!(message.contains("ended the session"), "{message}");
        assert!(stderr.contains("executor run failed"), "{stderr}");
    }
    // The first chat's halt was all it tried to send.
    assert_eq!(inspector.sent(), delivered + 1);
    assert_eq!(inspector.first("haltRequest"), Some(delivered));
    assert_eq!(inspector.first("userInput"), None);
    assert_eq!(
        fake.requests().len(),
        1,
        "the trigger's turn made the one call"
    );
    drop(agent);
}
