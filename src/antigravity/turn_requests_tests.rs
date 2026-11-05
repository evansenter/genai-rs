use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[tokio::test]
async fn test_a_hook_runs_off_the_turn_thread_and_returns_its_value() {
    let caller = std::thread::current().id();
    let (value, thread) = run_hook(|| (41 + 1, std::thread::current().id()))
        .await
        .unwrap();
    assert_eq!(value, 42);
    assert_ne!(
        thread, caller,
        "a blocking hook must not hold the turn's thread"
    );
}

#[tokio::test]
async fn test_a_hook_can_reach_the_callers_runtime() {
    // As it could on tokio's blocking pool: a sync hook may need
    // `Handle::current()` to hand work to the runtime.
    let reached = run_hook(|| tokio::runtime::Handle::try_current().is_ok())
        .await
        .unwrap();
    assert!(reached);
}

#[tokio::test]
#[should_panic(expected = "hook panic 7203")]
async fn test_a_panicking_hook_panics_the_caller() {
    let _: () = run_hook(|| panic!("hook panic 7203")).await.unwrap();
}

#[test]
fn test_a_runtime_dropped_while_a_hook_blocks_does_not_wait_for_it() {
    // Regression: hooks ran on tokio's blocking pool, and dropping a runtime
    // waits for its blocking tasks. A hook abandoned at the turn deadline
    // and still blocked then hung the end of `#[tokio::main]`.
    let (release, blocked) = mpsc::channel::<()>();
    let (started, has_started) = tokio::sync::oneshot::channel::<()>();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let hook = run_hook(move || {
            let _ = started.send(());
            let _ = blocked.recv();
        });
        // Abandon the hook once it is blocked, as the turn deadline does.
        tokio::select! {
            _ = hook => panic!("the hook cannot return before it is released"),
            started = has_started => started.expect("the hook started"),
        }
    });

    let (dropped, has_dropped) = mpsc::channel();
    std::thread::spawn(move || {
        drop(runtime);
        let _ = dropped.send(());
    });
    let returned = has_dropped.recv_timeout(Duration::from_secs(10)).is_ok();
    // Let the hook finish either way, so a failure doesn't leak a thread.
    let _ = release.send(());
    assert!(
        returned,
        "dropping the runtime waited for a hook it had abandoned"
    );
}
