use super::*;
use tokio::sync::mpsc;

#[test]
fn test_trigger_config_construction() {
    let trigger = TriggerConfig::new("check the queue", Duration::from_secs(60));
    assert_eq!(trigger.message, "check the queue");
    assert_eq!(trigger.interval, Duration::from_secs(60));
}

/// Spawns a trigger task whose sends are observable through an mpsc
/// channel.
fn observable_trigger_with_sync(
    config: TriggerConfig,
    idle: watch::Receiver<bool>,
    turn_sync: Arc<tokio::sync::Mutex<()>>,
) -> (JoinHandle<()>, mpsc::UnboundedReceiver<String>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = spawn_trigger_task(config, idle, turn_sync, move |message| {
        let tx = tx.clone();
        async move {
            tx.send(message)
                .map_err(|e| AntigravityError::WebSocket(e.to_string()))
        }
    });
    (handle, rx)
}

fn observable_trigger(
    config: TriggerConfig,
    idle: watch::Receiver<bool>,
) -> (JoinHandle<()>, mpsc::UnboundedReceiver<String>) {
    observable_trigger_with_sync(config, idle, Arc::new(tokio::sync::Mutex::new(())))
}

#[tokio::test(start_paused = true)]
async fn test_trigger_fires_repeatedly_when_idle() {
    let (_idle_tx, idle_rx) = watch::channel(true);
    let (handle, mut rx) =
        observable_trigger(TriggerConfig::new("ping", Duration::from_secs(60)), idle_rx);

    // Paused-clock auto-advance: each recv drives one interval.
    assert_eq!(rx.recv().await.as_deref(), Some("ping"));
    assert_eq!(rx.recv().await.as_deref(), Some("ping"));
    assert_eq!(rx.recv().await.as_deref(), Some("ping"));
    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn test_trigger_defers_while_busy_and_collapses_missed_intervals() {
    let (idle_tx, idle_rx) = watch::channel(false); // busy
    let (handle, mut rx) = observable_trigger(
        TriggerConfig::new("check", Duration::from_millis(10)),
        idle_rx,
    );

    // Ten intervals elapse while the agent is busy: nothing delivered.
    tokio::time::sleep(Duration::from_millis(105)).await;
    assert!(rx.try_recv().is_err(), "must not deliver while busy");

    // Turn ends: exactly one deferred delivery for all missed intervals.
    idle_tx.send_replace(true);
    assert_eq!(rx.recv().await.as_deref(), Some("check"));
    // Before the next interval elapses, nothing else is queued.
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert!(
        rx.try_recv().is_err(),
        "missed intervals must collapse into one delivery"
    );
    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn test_trigger_defers_when_turn_begins_during_delivery_window() {
    // TOCTOU regression test: the trigger passes its idle check, but a
    // turn begins (takes the lock, marks busy) before the trigger can
    // send. The trigger must re-check idleness under the lock and
    // defer, not deliver into the freshly started turn.
    let (idle_tx, idle_rx) = watch::channel(true);
    let turn_sync = Arc::new(tokio::sync::Mutex::new(()));
    let (handle, mut rx) = observable_trigger_with_sync(
        TriggerConfig::new("check", Duration::from_millis(10)),
        idle_rx,
        Arc::clone(&turn_sync),
    );

    // Simulate begin_turn acquiring the lock first: the trigger's
    // interval elapses and its idle wait passes (idle is true), but it
    // blocks on the lock before sending.
    let sync = turn_sync.lock().await;
    tokio::time::sleep(Duration::from_millis(15)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(
        rx.try_recv().is_err(),
        "must not deliver while the turn lock is held"
    );

    // The turn marks the agent busy and releases the lock (as
    // begin_turn does). The trigger acquires the lock, re-checks
    // idleness, sees the turn, and defers.
    idle_tx.send_replace(false);
    drop(sync);
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(
        rx.try_recv().is_err(),
        "must re-check idleness under the lock and defer"
    );

    // Turn ends: the deferred delivery lands.
    idle_tx.send_replace(true);
    assert_eq!(rx.recv().await.as_deref(), Some("check"));
    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn test_trigger_task_exits_when_idle_channel_closes() {
    let (idle_tx, idle_rx) = watch::channel(false);
    let (handle, _rx) =
        observable_trigger(TriggerConfig::new("x", Duration::from_millis(1)), idle_rx);
    // Dropping the sender (the agent) ends the task without an abort.
    drop(idle_tx);
    handle.await.expect("task exits cleanly, not aborted");
}

#[tokio::test(start_paused = true)]
async fn test_trigger_task_exits_when_send_fails() {
    let (_idle_tx, idle_rx) = watch::channel(true);
    let handle = spawn_trigger_task(
        TriggerConfig::new("x", Duration::from_millis(1)),
        idle_rx,
        Arc::new(tokio::sync::Mutex::new(())),
        |_message| async { Err(AntigravityError::WebSocket("closed".to_string())) },
    );
    handle.await.expect("task exits cleanly after send failure");
}

#[tokio::test(start_paused = true)]
async fn test_trigger_tasks_abort_on_drop() {
    let (_idle_tx, idle_rx) = watch::channel(false); // never idle: task runs forever
    let (handle, _rx) =
        observable_trigger(TriggerConfig::new("x", Duration::from_secs(1)), idle_rx);
    let abort_handle = handle.abort_handle();

    let mut tasks = TriggerTasks::default();
    tasks.push(handle);
    drop(tasks);

    while !abort_handle.is_finished() {
        tokio::task::yield_now().await;
    }
}

/// A context whose deliveries land in a channel.
fn observable_context(
    idle: watch::Receiver<bool>,
) -> (TriggerContext, mpsc::UnboundedReceiver<String>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let context = TriggerContext::new(
        idle,
        Arc::new(tokio::sync::Mutex::new(())),
        Arc::new(move |message| {
            let tx = tx.clone();
            Box::pin(async move {
                tx.send(message)
                    .map_err(|e| AntigravityError::WebSocket(e.to_string()))
            })
        }),
    );
    (context, rx)
}

#[tokio::test(start_paused = true)]
async fn test_custom_trigger_sends_through_its_context_when_idle() {
    let (idle_tx, idle_rx) = watch::channel(false);
    let (context, mut rx) = observable_context(idle_rx);
    let trigger: CustomTrigger = Box::new(|context: TriggerContext| {
        Box::pin(async move {
            context.send("custom-1").await?;
            context.send("custom-2").await
        })
    });
    let handle = spawn_custom_trigger(trigger, context);
    // Busy: nothing yet.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(rx.try_recv().is_err(), "must wait while a turn runs");
    idle_tx.send_replace(true);
    assert_eq!(rx.recv().await.as_deref(), Some("custom-1"));
    assert_eq!(rx.recv().await.as_deref(), Some("custom-2"));
    handle.await.expect("the trigger returns on its own");
}

#[tokio::test]
async fn test_trigger_context_send_fails_once_the_agent_is_gone() {
    let (idle_tx, idle_rx) = watch::channel(false);
    let (context, _rx) = observable_context(idle_rx);
    drop(idle_tx);
    let err = context.send("late").await.unwrap_err();
    assert!(
        matches!(err, AntigravityError::ConnectionClosed { .. }),
        "{err:?}"
    );
}

#[test]
fn test_snapshot_diff_reports_added_modified_and_deleted_files() {
    let dir = tempfile::tempdir().unwrap();
    let keep = dir.path().join("keep.txt");
    let edit = dir.path().join("sub/edit.txt");
    let gone = dir.path().join("gone.txt");
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(&keep, "same").unwrap();
    std::fs::write(&edit, "v1").unwrap();
    std::fs::write(&gone, "bye").unwrap();
    // Ignored, like watchfiles' default filter.
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".git/HEAD"), "ref").unwrap();
    let before = snapshot(dir.path());
    assert_eq!(before.len(), 3, "{before:?}");

    std::fs::write(&edit, "version two").unwrap();
    std::fs::remove_file(&gone).unwrap();
    let added = dir.path().join("new.txt");
    std::fs::write(&added, "hi").unwrap();
    std::fs::write(dir.path().join(".git/HEAD"), "changed").unwrap();
    let after = snapshot(dir.path());

    let changes = diff_snapshots(&before, &after);
    let shape: Vec<(FileChangeKind, &Path)> =
        changes.iter().map(|c| (c.kind, c.path.as_path())).collect();
    assert_eq!(
        shape,
        vec![
            (FileChangeKind::Deleted, gone.as_path()),
            (FileChangeKind::Added, added.as_path()),
            (FileChangeKind::Modified, edit.as_path()),
        ]
    );
    assert!(diff_snapshots(&after, &after).is_empty());
    // A single file can be watched too; a missing path is empty.
    assert_eq!(snapshot(&keep).len(), 1);
    assert!(snapshot(&dir.path().join("missing")).is_empty());
}

#[tokio::test]
async fn test_on_file_change_sends_the_callbacks_message() {
    let dir = tempfile::tempdir().unwrap();
    let (_idle_tx, idle_rx) = watch::channel(true);
    let (context, mut rx) = observable_context(idle_rx);
    let trigger = on_file_change(dir.path(), Duration::from_millis(20), |changes| {
        Some(format!(
            "{} change(s): {:?}",
            changes.len(),
            changes[0].kind
        ))
    });
    let handle = tokio::spawn(trigger(context));
    // Let the watch take its first snapshot before the change.
    tokio::time::sleep(Duration::from_millis(60)).await;
    std::fs::write(dir.path().join("a.txt"), "x").unwrap();
    let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a message within 5s");
    assert_eq!(message.as_deref(), Some("1 change(s): Added"));
    handle.abort();
}
