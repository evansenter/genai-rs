//! Budgets and retries (#493 item 4, #504): each budget cap and its scope
//! shown ending a turn with its own stop reason, the backoff settings
//! spacing the model retries, and the output retries deciding whether a
//! malformed tool call ends the turn.

use super::*;
use genai_rs::antigravity::protocol::StopReason;
use genai_rs::antigravity::{Budget, BudgetScope, RetryConfig, SessionContinuation};

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_each_budget_cap_and_its_scope_stop_the_turn() {
    // Each scripted model call reports 1 input and 1 output token (2 in
    // all), and asks for another tool call, so only a cap ends the turn.
    let workspace = scratch_dir("agy-scripted-caps");
    let file = write_file(&workspace, "a.txt", "a");
    let caps = [
        (
            Budget::new().with_max_model_calls(1),
            StopReason::MaxModelCallsExceeded,
        ),
        (
            Budget::new().with_max_input_tokens(1),
            StopReason::MaxInputTokensExceeded,
        ),
        (
            Budget::new().with_max_output_tokens(1),
            StopReason::MaxOutputTokensExceeded,
        ),
        (
            Budget::new().with_max_total_tokens(2),
            StopReason::MaxTotalTokensExceeded,
        ),
    ];
    for (budget, reason) in caps {
        let fake = FakeGemini::start(vec![
            view_file(&file),
            view_file(&file),
            ModelTurn::text("uncapped-6101"),
        ])
        .await;
        let mut agent = scripted_agent(&fake)
            .add_workspace(workspace.path().to_string_lossy())
            .with_budget(budget)
            .spawn()
            .await
            .expect("spawn");
        let response = agent.chat("read it again and again").await.expect("turn");
        assert_eq!(response.stop_reason(), Some(&reason), "{budget:?}");
        assert_eq!(fake.requests().len(), 1, "{budget:?} ends the turn");
        agent.shutdown().await.expect("shutdown");
    }

    // The scope decides whether a resumed conversation's earlier calls
    // count: two calls before the resume, and a cap of two after it.
    for (scope, reason) in [
        (
            BudgetScope::Lifetime,
            Some(StopReason::MaxModelCallsExceeded),
        ),
        (BudgetScope::ForwardLooking, None),
    ] {
        let save_dir = scratch_dir("agy-scripted-scope");
        let fake = FakeGemini::start(vec![ModelTurn::text("scoped-6102")]).await;
        let mut agent = scripted_agent(&fake)
            .with_save_dir(save_dir.path().to_string_lossy())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("one").await.expect("turn");
        agent.chat("two").await.expect("turn");
        let conversation = agent.conversation_id().expect("an id").to_string();
        agent.shutdown().await.expect("shutdown");

        let mut agent = scripted_agent(&fake)
            .with_save_dir(save_dir.path().to_string_lossy())
            .with_conversation_id(&conversation)
            .with_session_continuation(SessionContinuation::Resume)
            .with_budget(Budget::new().with_max_model_calls(2).with_scope(scope))
            .spawn()
            .await
            .expect("resume");
        let response = agent.chat("three").await.expect("turn");
        assert_eq!(response.stop_reason(), reason.as_ref(), "{scope:?}");
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_backoff_settings_space_the_model_retries() {
    // Two 503s, then an answer. With no jitter the harness waits exactly
    // the initial backoff, then that times the multiplier (0.1.23,
    // 2026-11-09: 500 ms and 4 gave 507 ms and 2007 ms). Its own
    // defaults, 4 s and 2, would wait 4 s and then 8 s.
    let arrivals: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let seen = Arc::clone(&arrivals);
    let fake = FakeGemini::routed(move |_, n| {
        seen.lock().unwrap().push(Instant::now());
        if n < 2 {
            ModelTurn::error(503, "scripted-unavailable")
        } else {
            ModelTurn::text("recovered-6103")
        }
    })
    .await;
    let initial = Duration::from_millis(300);
    let mut agent = scripted_agent(&fake)
        .with_retry(
            RetryConfig::new()
                .with_max_api_retries(3)
                .with_initial_backoff(initial)
                .with_backoff_multiplier(3.0)
                .with_jitter(0.0),
        )
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat("hi").await.expect("turn");
    assert_eq!(response.text(), "recovered-6103");
    agent.shutdown().await.expect("shutdown");

    let arrivals = arrivals.lock().unwrap().clone();
    assert_eq!(arrivals.len(), 3, "two retries");
    let (first, second) = (arrivals[1] - arrivals[0], arrivals[2] - arrivals[1]);
    // Lower bounds hold on any machine; the upper one is half the
    // harness's default first wait.
    assert!(
        first >= initial.mul_f64(0.95) && first < Duration::from_secs(2),
        "the first retry waited {first:?}, not the initial backoff"
    );
    assert!(
        second >= initial.mul_f64(3.0 * 0.95),
        "the second retry waited {second:?}, not three times the first"
    );
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_output_retries_decide_whether_a_malformed_call_ends_the_turn() {
    // A call to a tool nobody declared is output the harness cannot use
    // ("invalid tool call error (unknown_tool)" on 0.1.23); the next
    // answer is valid.
    let script = || {
        vec![
            ModelTurn::call("no_such_tool_6104", json!({"a": 1})),
            ModelTurn::text("valid-6105"),
        ]
    };

    // No output retries: the malformed call ends the turn.
    let fake = FakeGemini::start(script()).await;
    let mut agent = scripted_agent(&fake)
        .with_retry(RetryConfig::new().with_max_output_retries(0))
        .spawn()
        .await
        .expect("spawn");
    let err = agent.chat("go").await.unwrap_err();
    assert!(
        matches!(err, AntigravityError::Turn { .. }),
        "a malformed call with no output retries fails the turn, got {err:?}"
    );
    assert_eq!(fake.requests().len(), 1, "no retry");
    let _ = agent.shutdown().await;

    // One retry: the harness tells the model and takes the valid answer.
    let fake = FakeGemini::start(script()).await;
    let mut agent = scripted_agent(&fake)
        .with_retry(RetryConfig::new().with_max_output_retries(1))
        .spawn()
        .await
        .expect("spawn");
    let response = agent.chat("go").await.expect("turn");
    assert_eq!(response.text(), "valid-6105");
    assert_eq!(fake.requests().len(), 2, "one retry");
    agent.shutdown().await.expect("shutdown");
}
