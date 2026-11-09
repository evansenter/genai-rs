//! Cancel safety (G1, #504 WP12): a turn abandoned while this client's own
//! work is in flight — an outer timeout on `chat`, or a `send_streaming`
//! stream dropped mid-turn, while a custom tool hangs or the pre-tool hook
//! the harness waits on is still running.
//!
//! The harness is still inside that turn, waiting for a result that will
//! never come. Before the fix, the next `chat` hung until its turn timeout
//! and the one after it returned the abandoned turn's reply. Each test here
//! abandons the first turn one way and asserts that the next two chats get
//! their own replies, each within seconds. A turn abandoned while it waits
//! on the model, which recovered before the fix too, is held to the same
//! bar.
//!
//! The halt is not free (half a second of draining at least), so these
//! tests also count halts: only the turn after an unfinished one sends
//! one. The turn after one that finished, one the harness itself failed,
//! or a turn timeout whose own halt drained the harness sends none.

use super::*;
use genai_rs::antigravity::TurnDecision;

/// The start of the debug line each halt-and-drain logs.
const HALT_LINE: &str = "Halting and draining the harness turn";

/// Records the halts the agent sends, by the debug line each halt-and-drain
/// logs, so a test can tell a turn that halted the one before it from one
/// that did not.
#[derive(Clone, Default)]
struct Halts(Arc<Mutex<Vec<String>>>);

impl Halts {
    /// Records until the guard drops. `#[tokio::test]` drives the agent on
    /// the test's thread, so a thread-scoped subscriber sees every halt.
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let guard = tracing::subscriber::set_default(self.clone());
        // A callsite first hit with no subscriber can cache "never" for the
        // process; rebuild so this subscriber is asked.
        tracing::callsite::rebuild_interest_cache();
        guard
    }

    /// The halts logged since the last call, each as its log line.
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl tracing::Subscriber for Halts {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target().starts_with("genai_rs::antigravity")
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        if message.0.starts_with(HALT_LINE) {
            self.0.lock().unwrap().push(message.0);
        }
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

const FIRST: &str = "first-prompt-6101";
const SECOND: &str = "second-prompt-6102";
const THIRD: &str = "third-prompt-6103";
const SECOND_REPLY: &str = "second-reply-6202";
const THIRD_REPLY: &str = "third-reply-6203";
/// The abandoned turn's own reply: the slow model's, or the model's answer
/// should the abandoned tool's result ever reach it.
const ABANDONED_REPLY: &str = "abandoned-reply-6201";
/// What the model answers a request carrying none of the prompts.
const UNEXPECTED_REPLY: &str = "unexpected-request-6299";

/// How long the work the first turn blocks on may take to start.
const START_LIMIT: Duration = Duration::from_secs(30);
/// How long each turn after the abandoned one may take: halting and
/// draining the abandoned turn (at most 5s) plus one model round trip. A
/// desynced turn instead waits for the agent's 60s turn timeout.
const NEXT_TURN_LIMIT: Duration = Duration::from_secs(15);
/// How often the first turn is polled while waiting for its work to start.
const POLL: Duration = Duration::from_millis(50);

/// What the first turn is blocked on when it is abandoned.
#[derive(Clone, Copy, Debug)]
enum Blocker {
    /// A custom tool that never returns.
    HangingTool,
    /// A pre-tool hook (for a `view_file` call) that blocks until released.
    PendingHook,
    /// A model reply that arrives only after the turn is abandoned.
    SlowModel,
}

/// How the first turn is abandoned.
#[derive(Clone, Copy, Debug)]
enum Abandon {
    /// `tokio::time::timeout` around `chat` fires.
    OuterTimeout,
    /// The `send_streaming` stream is dropped mid-turn.
    DroppedStream,
}

/// The scripted model's reply to `request`, chosen by the latest prompt it
/// carries, so a turn that gets another turn's reply is caught by name.
fn reply_to(request: &Value, blocking_call: &ModelTurn) -> ModelTurn {
    let body = request.to_string();
    if body.contains(THIRD) {
        ModelTurn::text(THIRD_REPLY)
    } else if body.contains(SECOND) {
        ModelTurn::text(SECOND_REPLY)
    } else if body.contains(FIRST) && body.contains("functionResponse") {
        ModelTurn::text(ABANDONED_REPLY)
    } else if body.contains(FIRST) {
        blocking_call.clone()
    } else {
        ModelTurn::text(UNEXPECTED_REPLY)
    }
}

/// Polls `turn` until the work it blocks on has started, failing if the turn
/// ends first.
async fn poll_until_started<F>(mut turn: std::pin::Pin<&mut F>, started: &Started)
where
    F: std::future::Future,
    F::Output: std::fmt::Debug,
{
    let deadline = Instant::now() + START_LIMIT;
    while started.at().is_none() {
        assert!(
            Instant::now() < deadline,
            "the work the first turn blocks on never started within {START_LIMIT:?}"
        );
        if let Ok(output) = tokio::time::timeout(POLL, turn.as_mut()).await {
            panic!("the first turn ended before it was abandoned: {output:?}");
        }
    }
}

/// Starts the first turn, abandons it `abandon`'s way once it is blocked on
/// `blocker`, then checks that the next two chats each get their own reply
/// within [`NEXT_TURN_LIMIT`], and that only the first of them halts.
async fn assert_next_turns_survive(blocker: Blocker, abandon: Abandon) {
    let halts = Halts::default();
    let _recording = halts.install();
    let started = Started::default();
    let workspace = scratch_dir("agy-scripted-cancel");
    let path = write_file(&workspace, "f.txt", "x");
    let blocking_call = match blocker {
        Blocker::HangingTool => ModelTurn::call("hanging_tool", json!({})),
        Blocker::PendingHook => view_file(&path),
        Blocker::SlowModel => ModelTurn::text(ABANDONED_REPLY).delayed(Duration::from_secs(10)),
    };
    let model_started = started.clone();
    let fake = FakeGemini::routed(move |request, _| {
        let reply = reply_to(&request.json(), &blocking_call);
        if matches!(blocker, Blocker::SlowModel) && matches!(reply, ModelTurn::Delayed(..)) {
            model_started.mark();
        }
        reply
    })
    .await;
    // Dropping `release` lets every call of the hook return.
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    let builder = scripted_agent(&fake).add_workspace(workspace.path().to_string_lossy());
    let builder = match blocker {
        Blocker::HangingTool => {
            builder.with_tool_service(Arc::new(HangingService(started.clone())))
        }
        Blocker::PendingHook => {
            let blocked = Mutex::new(blocked);
            let hook_started = started.clone();
            builder.on_pre_tool(move |_| {
                hook_started.mark();
                let _ = blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(120));
                PreToolDecision::Allow
            })
        }
        Blocker::SlowModel => builder,
    };
    let mut agent = builder.spawn().await.expect("spawn");

    match abandon {
        Abandon::OuterTimeout => {
            let mut chat = std::pin::pin!(agent.chat(FIRST));
            poll_until_started(chat.as_mut(), &started).await;
            // The caller's timeout fires while the turn is blocked and
            // drops it.
            let outcome = tokio::time::timeout(Duration::from_millis(200), chat).await;
            assert!(
                outcome.is_err(),
                "the first turn must still be blocked: {outcome:?}"
            );
        }
        Abandon::DroppedStream => {
            let mut stream = agent.send_streaming(FIRST).await.expect("stream starts");
            {
                let consume = async {
                    while let Some(event) = stream.next().await {
                        if let AgentEvent::Finished(response) = event? {
                            return Ok(*response);
                        }
                    }
                    Err(AntigravityError::Protocol {
                        message: "no Finished event".into(),
                        source: None,
                    })
                };
                let mut consume = std::pin::pin!(consume);
                poll_until_started(consume.as_mut(), &started).await;
            }
            // The consumer gives up on the turn mid-way.
            drop(stream);
        }
    }
    assert_eq!(
        halts.take(),
        Vec::<String>::new(),
        "{blocker:?}/{abandon:?}: the first turn has nothing to halt"
    );

    // The turn after the abandoned one halts it; the one after that
    // follows a finished turn and sends no halt.
    for (prompt, reply, expected_halts) in [(SECOND, SECOND_REPLY, 1), (THIRD, THIRD_REPLY, 0)] {
        let turn_start = Instant::now();
        let response = tokio::time::timeout(NEXT_TURN_LIMIT, agent.chat(prompt))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "{blocker:?}/{abandon:?}: chat({prompt:?}) after the abandoned turn hung \
                     past {NEXT_TURN_LIMIT:?}"
                )
            })
            .unwrap_or_else(|e| {
                panic!(
                    "{blocker:?}/{abandon:?}: chat({prompt:?}) after the abandoned turn failed: {e}"
                )
            });
        assert_eq!(
            response.text(),
            reply,
            "{blocker:?}/{abandon:?}: chat({prompt:?}) got another turn's reply (after {:?})",
            turn_start.elapsed()
        );
        let sent = halts.take();
        assert_eq!(
            sent.len(),
            expected_halts,
            "{blocker:?}/{abandon:?}: chat({prompt:?}) sent the wrong number of halts: {sent:?}"
        );
    }

    drop(release);
    agent.shutdown().await.expect("shutdown");
}

/// A turn the harness itself ended with an error is over. A pre-turn hook's
/// denial cancels the trajectory; the next chat gets its reply without a
/// halt.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_turn_the_harness_failed_is_not_halted_again() {
    let halts = Halts::default();
    let _recording = halts.install();
    let fake = FakeGemini::start(vec![ModelTurn::text(SECOND_REPLY)]).await;
    let mut agent = scripted_agent(&fake)
        .on_pre_turn(|turn| {
            if turn.text.contains(FIRST) {
                TurnDecision::deny("pre-turn-denied-6301")
            } else {
                TurnDecision::Allow
            }
        })
        .spawn()
        .await
        .expect("spawn");

    let err = agent.chat(FIRST).await.unwrap_err();
    assert!(
        matches!(&err, AntigravityError::Turn(m) if m.contains("pre-turn-denied-6301")),
        "{err:?}"
    );
    let response = agent.chat(SECOND).await.expect("the turn after the denial");
    assert_eq!(response.text(), SECOND_REPLY);
    assert_eq!(
        halts.take(),
        Vec::<String>::new(),
        "a turn the harness failed needs no halt"
    );
    agent.shutdown().await.expect("shutdown");
}

/// A turn timeout halts and drains its own turn. Once that drain goes
/// quiet the turn is over, and the next chat sends no second halt.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_a_drained_turn_timeout_is_not_halted_again() {
    let halts = Halts::default();
    let _recording = halts.install();
    let started = Started::default();
    let hanging_call = ModelTurn::call("hanging_tool", json!({}));
    let fake = FakeGemini::routed(move |request, _| reply_to(&request.json(), &hanging_call)).await;
    // Room for one model round trip on a slow runner, so the turn after
    // the timeout completes under the same budget.
    let mut agent = scripted_agent(&fake)
        .with_tool_service(Arc::new(HangingService(started)))
        .with_turn_timeout(Duration::from_secs(5))
        .spawn()
        .await
        .expect("spawn");

    let err = agent.chat(FIRST).await.unwrap_err();
    assert!(matches!(err, AntigravityError::Timeout { .. }), "{err:?}");
    let sent = halts.take();
    assert!(
        sent.len() == 1 && sent[0].contains("turn timeout"),
        "the timeout halts its own turn once: {sent:?}"
    );
    let response = agent
        .chat(SECOND)
        .await
        .expect("the turn after the timeout");
    assert_eq!(response.text(), SECOND_REPLY);
    assert_eq!(
        halts.take(),
        Vec::<String>::new(),
        "a turn the timeout drained needs no second halt"
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_outer_timeout_during_a_hanging_tool_keeps_later_turns_in_sync() {
    assert_next_turns_survive(Blocker::HangingTool, Abandon::OuterTimeout).await;
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_outer_timeout_during_a_pending_pre_tool_hook_keeps_later_turns_in_sync() {
    assert_next_turns_survive(Blocker::PendingHook, Abandon::OuterTimeout).await;
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_outer_timeout_while_the_model_is_slow_keeps_later_turns_in_sync() {
    assert_next_turns_survive(Blocker::SlowModel, Abandon::OuterTimeout).await;
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_stream_dropped_during_a_hanging_tool_keeps_later_turns_in_sync() {
    assert_next_turns_survive(Blocker::HangingTool, Abandon::DroppedStream).await;
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_stream_dropped_during_a_pending_pre_tool_hook_keeps_later_turns_in_sync() {
    assert_next_turns_survive(Blocker::PendingHook, Abandon::DroppedStream).await;
}
