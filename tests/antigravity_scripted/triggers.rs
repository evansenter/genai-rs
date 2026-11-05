//! Triggers (#493 item 7): custom and file-change triggers, the trigger-turn
//! consumer, and the turn-timeout and trigger-discard wire discipline
//! (moved here from `antigravity_harness.rs`, where they called the real
//! endpoint with a placeholder key).

use super::*;
use genai_rs::antigravity::triggers;

/// Records every message this client sends the harness.
#[derive(Default)]
struct WsSendCapture(Mutex<Vec<Value>>);

impl genai_rs::wire::WireInspector for WsSendCapture {
    fn on_event(&self, event: &genai_rs::wire::WireEvent) {
        if let genai_rs::wire::WireEvent::WsSend { payload, .. } = event {
            self.0.lock().unwrap().push(payload.clone());
        }
    }
}

impl WsSendCapture {
    /// The index of the first send carrying `key`.
    fn first(&self, key: &str) -> Option<usize> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .position(|p| p.get(key).is_some())
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
    let inspector = Arc::new(WsSendCapture::default());
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
    let inspector = Arc::new(WsSendCapture::default());
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
