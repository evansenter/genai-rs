//! Lifecycle hooks (#493 item 5): each fires at its point in a real
//! harness session, and its answer changes what the harness does.

use super::*;
use genai_rs::antigravity::{StopDecision, TurnDecision};

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_lifecycle_hooks_fire_in_order_and_steer_the_turn() {
    let workspace = scratch_dir("agy-scripted-life");
    let missing = workspace.path().canonicalize().unwrap().join("missing.txt");
    let fake = FakeGemini::start(vec![
        view_file(&missing.to_string_lossy()),
        ModelTurn::text("first-answer"),
        ModelTurn::text("continued-answer"),
    ])
    .await;
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let record = |log: &Arc<Mutex<Vec<String>>>| {
        let log = Arc::clone(log);
        move |entry: String| log.lock().unwrap().push(entry)
    };
    let (start, end, pre, post, error, stop) = (
        record(&log),
        record(&log),
        record(&log),
        record(&log),
        record(&log),
        record(&log),
    );
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .on_session_start(move || start("session_start".into()))
        .on_session_end(move || end("session_end".into()))
        .on_pre_turn(move |turn| {
            pre(format!("pre_turn:{}", turn.text));
            TurnDecision::Allow
        })
        .on_post_turn(move |turn| post(format!("post_turn:{}", turn.response_text)))
        .on_tool_error(move |failure| {
            error(format!("tool_error:{}", failure.name));
            Some("CUSTOM-ERROR-1177: create the file first".to_string())
        })
        .on_stop(move |request| {
            stop(format!("stop:{}", request.continuation_count));
            if request.continuation_count == 0 {
                StopDecision::continue_with("CONTINUE-REASON-1178")
            } else {
                StopDecision::Stop
            }
        })
        .spawn()
        .await
        .expect("spawn");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["session_start"],
        "the session-start hook runs before spawn() returns"
    );

    let response = agent.chat("read the missing file").await.expect("turn");
    // The stop hook kept the turn going for one more model call.
    assert_eq!(response.text(), "continued-answer");
    assert_eq!(fake.requests().len(), 3);
    assert!(
        fake.request_json(2)
            .unwrap()
            .to_string()
            .contains("CONTINUE-REASON-1178"),
        "the continue reason reaches the model"
    );
    // The tool-error hook's text replaced the harness's.
    let told = function_response_text(&fake, 1);
    assert!(told.contains("CUSTOM-ERROR-1177"), "{told}");

    agent.shutdown().await.expect("shutdown");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "session_start",
            "pre_turn:read the missing file",
            "tool_error:view_file",
            "stop:0",
            "stop:1",
            "post_turn:continued-answer",
            "session_end",
        ]
    );
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_pre_turn_hook_can_refuse_a_turn() {
    let fake = FakeGemini::start(vec![ModelTurn::text("allowed-turn-9921")]).await;
    let mut agent = scripted_agent(&fake)
        .on_pre_turn(|turn| {
            if turn.text.contains("forbidden") {
                TurnDecision::deny("PRE-TURN-DENIED-9920")
            } else {
                TurnDecision::Allow
            }
        })
        .spawn()
        .await
        .expect("spawn");
    let err = agent.chat("a forbidden request").await.unwrap_err();
    let AntigravityError::Turn { kind, message } = &err else {
        panic!("a refused turn fails with Turn: {err:?}");
    };
    assert_eq!(*kind, TurnFailure::Denied);
    // The hook's reason is carried.
    assert!(message.contains("PRE-TURN-DENIED-9920"), "{message}");
    assert!(fake.requests().is_empty(), "a refused turn calls no model");
    // The agent stays usable.
    let response = agent.chat("an allowed request").await.expect("turn");
    assert_eq!(response.text(), "allowed-turn-9921");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_pre_tool_hook_can_modify_arguments() {
    let workspace = scratch_dir("agy-scripted-modify");
    let asked = write_file(&workspace, "asked.txt", "asked-content-4401");
    let swapped = write_file(&workspace, "swapped.txt", "swapped-content-4402");
    let fake = FakeGemini::start(vec![view_file(&asked), ModelTurn::text("read")]).await;
    let target = swapped.clone();
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .on_pre_tool(move |call| {
            if call.name == "view_file" {
                PreToolDecision::modify(json!({"AbsolutePath": target}))
            } else {
                PreToolDecision::Allow
            }
        })
        .spawn()
        .await
        .expect("spawn");
    agent.chat("read it").await.expect("turn");
    let told = function_response_text(&fake, 1);
    assert!(told.contains("swapped-content-4402"), "{told}");
    assert!(!told.contains("asked-content-4401"), "{told}");
    agent.shutdown().await.expect("shutdown");
}

static HOOKED_TOOL_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Echoes its key, for the hook-count and modify tests.
#[tool(key(description = "Any key"))]
fn scripted_echo(key: String) -> String {
    HOOKED_TOOL_CALLS.fetch_add(1, Ordering::SeqCst);
    format!("echo-{key}")
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_custom_tool_consults_the_pre_tool_hook_once_and_runs_its_args() {
    // The harness asks the pre-tool callback before it dispatches a custom
    // tool; the bridge used to judge the call a second time on dispatch,
    // running the hook twice.
    let fake = FakeGemini::start(vec![
        ModelTurn::call("scripted_echo", json!({"key": "original"})),
        ModelTurn::text("echoed"),
    ])
    .await;
    let consulted = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&consulted);
    let mut agent = scripted_agent(&fake)
        .add_tool(ScriptedEchoCallable.declaration())
        .on_pre_tool(move |call| {
            if call.name == "scripted_echo" {
                count.fetch_add(1, Ordering::SeqCst);
                PreToolDecision::modify(json!({"key": "rewritten"}))
            } else {
                PreToolDecision::Allow
            }
        })
        .spawn()
        .await
        .expect("spawn");
    agent.chat("echo").await.expect("turn");
    assert_eq!(
        consulted.load(Ordering::SeqCst),
        1,
        "one hook call per tool call"
    );
    // The tool ran on the hook's arguments.
    assert!(
        function_response_text(&fake, 1).contains("echo-rewritten"),
        "{}",
        function_response_text(&fake, 1)
    );
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_hook_can_block_on_async_work() {
    // An "async hook" is a sync closure that blocks on the runtime, which
    // its thread has entered.
    let workspace = scratch_dir("agy-scripted-async-hook");
    let path = write_file(&workspace, "f.txt", "async-hook-content-5501");
    let fake = FakeGemini::start(vec![view_file(&path), ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .add_workspace(workspace.path().to_string_lossy())
        .on_pre_tool(|_| {
            tokio::runtime::Handle::current().block_on(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                PreToolDecision::Allow
            })
        })
        .spawn()
        .await
        .expect("spawn");
    agent.chat("read").await.expect("turn");
    assert!(function_response_text(&fake, 1).contains("async-hook-content-5501"));
    agent.shutdown().await.expect("shutdown");
}

/// A turn that hits its deadline is halted and drained with no one to
/// consume it, so its per-turn hooks don't run for it: the drain answers
/// them as if no hook were set (a stop hook's `Continue` would otherwise
/// keep the halted turn going). The next turn runs them as usual.
#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_lifecycle_hooks_skip_a_drained_turn() {
    for budget in DEADLINE_BUDGETS {
        let started = Started::default();
        let fake = FakeGemini::start(vec![
            ModelTurn::call("hanging_tool", json!({})),
            ModelTurn::text("after-drain-7731"),
        ])
        .await;
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let (post, stop) = (Arc::clone(&log), Arc::clone(&log));
        let mut agent = scripted_agent(&fake)
            .add_tool_service(Arc::new(HangingService(started.clone())))
            .with_turn_timeout(budget)
            .on_post_turn(move |turn| {
                post.lock()
                    .unwrap()
                    .push(format!("post_turn:{}", turn.response_text))
            })
            .on_stop(move |request| {
                stop.lock()
                    .unwrap()
                    .push(format!("stop:{}", request.continuation_count));
                StopDecision::Stop
            })
            .spawn()
            .await
            .expect("spawn");

        let turn_start = Instant::now();
        let err = agent.chat("call the hanging tool").await.unwrap_err();
        assert!(
            matches!(err, AntigravityError::Timeout { .. }),
            "expected Timeout, got {err:?}"
        );
        if !started_in_time(started.at(), turn_start, budget) {
            println!("hanging_tool did not start early in a {budget:?} turn; retrying");
            agent.shutdown().await.expect("shutdown");
            continue;
        }
        assert!(
            log.lock().unwrap().is_empty(),
            "the drained turn ran a lifecycle hook: {:?}",
            log.lock().unwrap()
        );

        let next = agent.chat("are you still there?").await.expect("next turn");
        assert_eq!(next.text(), "after-drain-7731");
        assert_eq!(
            log.lock().unwrap().as_slice(),
            ["stop:0", "post_turn:after-drain-7731"]
        );
        agent.shutdown().await.expect("shutdown");
        return;
    }
    panic!("hanging_tool never started early enough in any of {DEADLINE_BUDGETS:?}");
}
