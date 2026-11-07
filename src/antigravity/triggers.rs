//! Client-side triggers: messages injected into the conversation outside a
//! user turn, as [`InputEvent::AutomatedTrigger`](super::protocol::InputEvent).
//!
//! Three kinds, mirroring the Python SDK's `triggers` module:
//!
//! - **Interval** ([`TriggerConfig`], the Python SDK's `every`): a fixed
//!   message every interval, added with
//!   [`AgentBuilder::add_trigger`](super::AgentBuilder::add_trigger).
//! - **File change** ([`on_file_change`]): a message built from the changes
//!   under a path.
//! - **Custom**: any async function handed a [`TriggerContext`], added with
//!   [`AgentBuilder::add_custom_trigger`](super::AgentBuilder::add_custom_trigger),
//!   that calls [`TriggerContext::send`] when it wants the agent to act — on
//!   a queue message, a webhook, a cron schedule.
//!
//! Each runs as a task of its own after a successful `spawn()`, as in the
//! Python SDK's `TriggerRunner`: no ordering between them, and one that
//! fails or returns never takes the session down.
//!
//! # Delivery semantics
//!
//! - The interval trigger's first firing happens after the first `interval`
//!   elapses (not immediately) — same as the reference SDK's `every()`.
//! - A message is **delivered only while the agent is idle** (no turn is
//!   being driven by `chat`/`send_streaming`). [`TriggerContext::send`]
//!   waits for the current turn to end. The reference SDK's
//!   `TriggerDelivery` names this mode `WAIT_IDLE` (its default
//!   `send_immediately` mode does not suit this crate's sequential turn
//!   loop, where nothing reads the WebSocket between turns of a busy
//!   agent). Delivery is serialized with `chat`/`send_streaming`'s turn
//!   begin through a shared lock, so a trigger can never slip its message
//!   into a turn that begins concurrently with the idle check.
//! - **Overlap suppression**: interval firings missed while busy collapse
//!   into a single delivery — the timer restarts only after the deferred
//!   delivery lands, so a long turn produces one trigger message, not a
//!   backlog. The file watcher does the same: changes made during a turn
//!   arrive as one batch.
//! - A delivered message starts a harness-side turn. Its output is
//!   surfaced only if you consume it with
//!   [`AntigravityAgent::next_trigger_turn`](super::AntigravityAgent::next_trigger_turn).
//!   Otherwise the next `chat`/`send_streaming` call halts that turn if it
//!   is still running and discards its events before sending the new input,
//!   so trigger turns can never desync or leak into a user turn's response.
//!   The trigger's effects on conversation history (and any tool calls the
//!   harness completed before the halt) persist.
//!
//! # Lifecycle
//!
//! Trigger tasks stop cleanly — no zombie timers:
//!
//! - [`AntigravityAgent::shutdown`](super::AntigravityAgent::shutdown)
//!   aborts them before closing the WebSocket;
//! - dropping the agent aborts them (the task handles abort on drop);
//! - a failed send (session gone) or a closed idle channel (agent dropped)
//!   ends the task on its own.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::AntigravityError;

/// A boxed, sendable future.
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Configuration for a recurring client-side trigger (the Python SDK's
/// `every`).
///
/// Register with [`AgentBuilder::add_trigger`](super::AgentBuilder::add_trigger).
/// See the [module docs](self) for delivery semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TriggerConfig {
    /// Message injected into the conversation on each firing.
    pub message: String,
    /// Interval between firings. Must be non-zero (validated at
    /// `spawn()` time).
    pub interval: Duration,
}

impl TriggerConfig {
    /// Creates a trigger that injects `message` every `interval`.
    #[must_use]
    pub fn new(message: impl Into<String>, interval: Duration) -> Self {
        Self {
            message: message.into(),
            interval,
        }
    }
}

/// Delivers one trigger message to the harness.
pub(crate) type Deliver =
    Arc<dyn Fn(String) -> BoxFuture<Result<(), AntigravityError>> + Send + Sync>;

/// What a trigger is handed: the way to send the agent a message (the
/// Python SDK's `TriggerContext`). Cheap to clone.
#[derive(Clone)]
pub struct TriggerContext {
    idle: watch::Receiver<bool>,
    turn_sync: Arc<tokio::sync::Mutex<()>>,
    deliver: Deliver,
}

impl std::fmt::Debug for TriggerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriggerContext")
            .field("idle", &*self.idle.borrow())
            .finish_non_exhaustive()
    }
}

impl TriggerContext {
    /// `idle` reports whether the agent is between turns (`true` = idle);
    /// the sender side lives on the agent. `deliver` sends the message — in
    /// production it wraps the session's shared sink handle (and sets the
    /// agent's `trigger_fired` flag first), in tests a channel.
    ///
    /// `turn_sync` makes the delivery decision and the agent's turn begin
    /// mutually exclusive: [`send`](Self::send) holds it across [idle
    /// re-check → deliver], and `AntigravityAgent::begin_turn` holds the
    /// same lock across [mark busy → consume `trigger_fired`]. Without it, a
    /// delivery could race a starting turn — pass the idle check, lose the
    /// race to the turn's flag swap, and land its message inside the user's
    /// turn with nobody left to drain the trigger-initiated harness turn.
    pub(crate) fn new(
        idle: watch::Receiver<bool>,
        turn_sync: Arc<tokio::sync::Mutex<()>>,
        deliver: Deliver,
    ) -> Self {
        Self {
            idle,
            turn_sync,
            deliver,
        }
    }

    /// Sends `message` to the agent as an automated trigger, which starts a
    /// turn on it. Waits while a turn is in flight (see the
    /// [module docs](self)).
    ///
    /// # Errors
    ///
    /// [`AntigravityError::ConnectionClosed`] once the agent is shut down
    /// or dropped, or the transport error when the send fails. A trigger
    /// should stop on either.
    pub async fn send(&self, message: impl Into<String>) -> Result<(), AntigravityError> {
        let message = message.into();
        let mut idle = self.idle.clone();
        loop {
            if idle.wait_for(|is_idle| *is_idle).await.is_err() {
                return Err(AntigravityError::ConnectionClosed {
                    message: "the agent was shut down or dropped".to_string(),
                    stderr: String::new(),
                });
            }
            // Re-check idleness under the turn lock: a turn may have begun
            // between the wait above and the lock acquisition — if so,
            // defer again instead of delivering into that turn.
            let sync = self.turn_sync.lock().await;
            if !*idle.borrow() {
                drop(sync);
                continue;
            }
            let delivered = (self.deliver)(message.clone()).await;
            drop(sync);
            return delivered;
        }
    }

    /// Whether the agent is between turns right now.
    fn is_idle(&self) -> bool {
        *self.idle.borrow()
    }
}

/// A custom trigger, as added with
/// [`AgentBuilder::add_custom_trigger`](super::AgentBuilder::add_custom_trigger).
pub(crate) type CustomTrigger =
    Box<dyn FnOnce(TriggerContext) -> BoxFuture<Result<(), AntigravityError>> + Send>;

/// Owns the spawned trigger tasks; aborts them all on drop so a dropped or
/// shut-down agent can never leak timer tasks.
#[derive(Debug, Default)]
pub(crate) struct TriggerTasks {
    handles: Vec<JoinHandle<()>>,
}

impl TriggerTasks {
    pub(crate) fn push(&mut self, handle: JoinHandle<()>) {
        self.handles.push(handle);
    }

    pub(crate) fn abort_all(&mut self) {
        for handle in self.handles.drain(..) {
            handle.abort();
        }
    }
}

impl Drop for TriggerTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

/// Spawns the timer task for one interval trigger.
///
/// The task ends on its own when the idle channel closes (agent dropped)
/// or a send fails (session closed); otherwise it runs until aborted.
pub(crate) fn spawn_trigger_task<F, Fut>(
    config: TriggerConfig,
    idle: watch::Receiver<bool>,
    turn_sync: Arc<tokio::sync::Mutex<()>>,
    send: F,
) -> JoinHandle<()>
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), AntigravityError>> + Send + 'static,
{
    let context = TriggerContext::new(idle, turn_sync, Arc::new(move |m| Box::pin(send(m))));
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(config.interval).await;
            // Deliver only while idle; a firing that comes due mid-turn is
            // deferred until the turn ends. Missed intervals collapse into
            // this single deferred delivery (overlap suppression).
            if let Err(e) = context.send(config.message.clone()).await {
                tracing::debug!("Interval trigger stopping: {e}");
                return;
            }
            tracing::debug!(
                interval = ?config.interval,
                "Delivered automated trigger message"
            );
        }
    })
}

/// Spawns a custom trigger's task. An error it returns is logged; it ends
/// that trigger only.
pub(crate) fn spawn_custom_trigger(
    trigger: CustomTrigger,
    context: TriggerContext,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = trigger(context).await {
            tracing::warn!("A custom trigger ended with an error: {e}");
        }
    })
}

// =============================================================================
// File-change trigger
// =============================================================================

/// What happened to a path under an [`on_file_change`] watch (the Python
/// SDK's `FileChangeKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FileChangeKind {
    /// The file appeared.
    Added,
    /// Its contents or modification time changed.
    Modified,
    /// It disappeared.
    Deleted,
}

/// One change under an [`on_file_change`] watch (the Python SDK's
/// `FileChange`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FileChange {
    /// What happened.
    pub kind: FileChangeKind,
    /// The file's path.
    pub path: PathBuf,
}

/// Directories a watch skips, as the Python SDK's default `watchfiles`
/// filter does.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".tox",
    ".venv",
    ".idea",
    ".mypy_cache",
    ".pytest_cache",
    ".hypothesis",
    "__pycache__",
    "node_modules",
];

/// A trigger that watches `path` (a file, or a directory and everything
/// under it) and hands each batch of changes to `on_change`; a message it
/// returns is sent to the agent (the Python SDK's `on_file_change`). Add it
/// with
/// [`AgentBuilder::add_custom_trigger`](super::AgentBuilder::add_custom_trigger):
///
/// ```rust,no_run
/// use genai_rs::antigravity::{AntigravityAgent, triggers};
/// use std::time::Duration;
///
/// let builder = AntigravityAgent::builder()
///     .add_custom_trigger(triggers::on_file_change(
///         "/repo/docs",
///         Duration::from_secs(2),
///         |changes| Some(format!("{} doc file(s) changed; check the links.", changes.len())),
///     ))
///     // ...
///     ;
/// # let _ = builder;
/// ```
///
/// The watch polls every `poll_interval`, comparing each file's size and
/// modification time (no OS notification API, so it works the same on every
/// platform and adds no dependency). It skips the directories the Python
/// SDK's `watchfiles` filter skips (`.git`, `node_modules`, `__pycache__`,
/// virtualenvs and tool caches) and does not follow symbolic links.
///
/// Changes that happen while a turn runs are held until the agent is idle,
/// then handed to `on_change` as one batch: the net change since the last
/// batch, so a file added and deleted within the turn is not reported.
/// `on_change` runs when the batch is about to be delivered, on the
/// trigger's task: keep it quick.
///
/// Edits the agent itself makes under `path` are reported too, so a trigger
/// turn that writes there starts the next one: filter such changes in
/// `on_change`, or watch a path the agent does not write. (The Python SDK's
/// `watchfiles` behaves the same way.)
///
/// The baseline is the state of `path` when `on_file_change` is called, so
/// a change made during `spawn()` or just after it is reported, with no
/// wait for the watch to start. Taking it reads the tree once, on the
/// calling thread. If `path` exists but cannot be read then, the first
/// snapshot that reads it becomes the baseline instead, and is not
/// reported.
pub fn on_file_change(
    path: impl Into<PathBuf>,
    poll_interval: Duration,
    on_change: impl Fn(&[FileChange]) -> Option<String> + Send + Sync + 'static,
) -> impl FnOnce(TriggerContext) -> BoxFuture<Result<(), AntigravityError>> + Send + 'static {
    watch_files(path.into(), poll_interval, on_change, snapshot)
}

/// Takes the snapshots of a watch: [`snapshot`], or a stand-in in tests.
type Snapshotter = fn(&Path) -> Option<BTreeMap<PathBuf, Stamp>>;

/// [`on_file_change`] with the snapshot function given.
fn watch_files(
    path: PathBuf,
    poll_interval: Duration,
    on_change: impl Fn(&[FileChange]) -> Option<String> + Send + Sync + 'static,
    take_snapshot: Snapshotter,
) -> impl FnOnce(TriggerContext) -> BoxFuture<Result<(), AntigravityError>> + Send + 'static {
    // `None` when the path exists but could not be read: the first
    // snapshot that reads it becomes the baseline, unreported, rather than
    // every file in it reading as added. (A missing path is an empty
    // baseline, so files created in it are reported.)
    let baseline = take_snapshot(&path);
    move |context: TriggerContext| {
        Box::pin(async move {
            // The state the last batch was reported against.
            let mut reported = baseline;
            loop {
                tokio::time::sleep(poll_interval).await;
                // Hold changes while a turn runs: the next idle poll diffs
                // against `reported`, so they arrive merged, in one batch.
                if !context.is_idle() {
                    continue;
                }
                // A snapshot that failed says nothing about the files:
                // skip the tick rather than diff against an empty one,
                // which would report every file deleted, then added back.
                let Some(now) = snapshot_of(&path, take_snapshot).await else {
                    continue;
                };
                // With no baseline yet, this snapshot becomes it.
                let changes = match &reported {
                    Some(before) => diff_snapshots(before, &now),
                    None => Vec::new(),
                };
                reported = Some(now);
                if changes.is_empty() {
                    continue;
                }
                // A turn that begins between the idle check and here makes
                // `send` wait for it; changes made meanwhile go in the next
                // batch, against the snapshot just taken.
                if let Some(message) = on_change(&changes)
                    && let Err(e) = context.send(message).await
                {
                    break Err::<(), _>(e);
                }
            }
        }) as BoxFuture<Result<(), AntigravityError>>
    }
}

/// A file's modification time and size.
type Stamp = (Option<SystemTime>, u64);

/// Takes a snapshot of `path` off the runtime's worker threads. `None`,
/// with a warning, when `path` itself could not be read or the snapshot
/// task failed (it panicked, or the runtime is shutting down): the caller
/// skips that tick.
async fn snapshot_of(path: &Path, take_snapshot: Snapshotter) -> Option<BTreeMap<PathBuf, Stamp>> {
    let owned = path.to_path_buf();
    match tokio::task::spawn_blocking(move || take_snapshot(&owned)).await {
        Ok(Some(snapshot)) => Some(snapshot),
        Ok(None) => {
            tracing::warn!(
                "The file watch could not read {}; skipping this poll",
                path.display()
            );
            None
        }
        Err(e) => {
            tracing::warn!(
                "The file watch on {} could not take a snapshot ({e}); skipping this poll",
                path.display()
            );
            None
        }
    }
}

/// Every file under `path` (or `path` itself, when it is a file) with its
/// stamp. Unreadable entries below `path` are skipped, and a missing path is
/// empty. `None` when `path` itself exists but could not be read (an I/O or
/// permission error on its metadata or listing): an empty snapshot then
/// would report every file deleted, and the next one every file added.
pub(crate) fn snapshot(path: &Path) -> Option<BTreeMap<PathBuf, Stamp>> {
    let unreadable = |e: &std::io::Error| e.kind() != std::io::ErrorKind::NotFound;
    let mut files = BTreeMap::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(next) = pending.pop() {
        let metadata = match std::fs::symlink_metadata(&next) {
            Ok(metadata) => metadata,
            Err(e) if next == path && unreadable(&e) => return None,
            Err(_) => continue,
        };
        if metadata.is_file() {
            files.insert(next, (metadata.modified().ok(), metadata.len()));
        } else if metadata.is_dir() {
            let skipped = next != path
                && next
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| IGNORED_DIRS.contains(&name));
            if skipped {
                continue;
            }
            match std::fs::read_dir(&next) {
                Ok(entries) => pending.extend(entries.flatten().map(|entry| entry.path())),
                Err(e) if next == path && unreadable(&e) => return None,
                Err(_) => {}
            }
        }
    }
    Some(files)
}

/// The changes from `before` to `after`, ordered by path.
pub(crate) fn diff_snapshots(
    before: &BTreeMap<PathBuf, Stamp>,
    after: &BTreeMap<PathBuf, Stamp>,
) -> Vec<FileChange> {
    let mut changes: Vec<FileChange> = after
        .iter()
        .filter_map(|(path, stamp)| {
            let kind = match before.get(path) {
                None => FileChangeKind::Added,
                Some(old) if old != stamp => FileChangeKind::Modified,
                Some(_) => return None,
            };
            Some(FileChange {
                kind,
                path: path.clone(),
            })
        })
        .chain(
            before
                .keys()
                .filter(|path| !after.contains_key(*path))
                .map(|path| FileChange {
                    kind: FileChangeKind::Deleted,
                    path: path.clone(),
                }),
        )
        .collect();
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

#[cfg(test)]
#[path = "triggers_tests.rs"]
mod tests;
