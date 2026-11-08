//! Harness process management: binary discovery, spawn + stdio handshake,
//! and ordered shutdown. The stderr drain is in `harness_stderr`.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};

use super::handshake::{InputConfig, OutputConfig};
use super::harness_stderr::{StderrRing, drain_stderr, lock_ring};
use super::session::WireContext;
use super::{AntigravityError, SUPPORTED_HARNESS_VERSION};
use crate::wire::WireEvent;

/// Environment variable naming the harness binary (same variable the
/// reference Python SDK honors).
pub(crate) const HARNESS_PATH_ENV: &str = "ANTIGRAVITY_HARNESS_PATH";

/// The directory of the harness binary inside a Python site-packages
/// directory (from the `google-antigravity` wheel).
const SITE_PACKAGES_BIN_DIR: &str = "google/antigravity/bin";

/// The harness binary's file names on this platform, in the order discovery
/// tries them in each directory.
const HARNESS_FILE_NAMES: &[&str] = harness_file_names(cfg!(windows));

/// The harness binary's file names, on Windows or elsewhere. The Windows
/// wheel ships `localharness.exe`, which the Python SDK looks for on win32;
/// the bare name is still tried after it there, as discovery always has.
const fn harness_file_names(windows: bool) -> &'static [&'static str] {
    if windows {
        &["localharness.exe", "localharness"]
    } else {
        &["localharness"]
    }
}

/// The interpreters discovery asks for their site-packages directories, in
/// order, on this platform.
const PYTHONS: &[&str] = python_interpreters(cfg!(windows));

/// The interpreters, on Windows or elsewhere. python.org's Windows
/// installer provides `python.exe` and no `python3`, which there is often
/// only the Microsoft Store's stub (it fails and prints how to install
/// Python), so Windows asks `python` after `python3`.
const fn python_interpreters(windows: bool) -> &'static [&'static str] {
    if windows {
        &["python3", "python"]
    } else {
        &["python3"]
    }
}

/// How long discovery waits for each of [`PYTHONS`] to list its
/// site-packages, and
/// `spawn()` for the wheel's version metadata, before going on without
/// them: a hung interpreter or a stalled read of the metadata must not hold
/// `spawn()` forever. Discovery's own file checks and the exec of the
/// binary are not bounded: on a mount that stalls them, the harness could
/// not start anyway.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for the handshake reply on stdout.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum accepted length of the harness's handshake reply frame. The
/// `OutputConfig` is a few dozen bytes in practice; anything near this cap
/// means the child is not a harness (or is misbehaving), so we reject the
/// declared length *before* allocating a buffer for it.
const MAX_HANDSHAKE_FRAME_LEN: usize = 4 * 1024 * 1024;

/// How long to wait for a clean exit after closing stdin.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// How long to wait after each kill escalation.
const KILL_GRACE: Duration = Duration::from_secs(1);

// =============================================================================
// Binary discovery
// =============================================================================

/// Discovers the harness binary. Order (mirrors the reference SDK,
/// extended):
///
/// 1. Explicit path from the builder.
/// 2. `ANTIGRAVITY_HARNESS_PATH` environment variable.
/// 3. `google/antigravity/bin/localharness` inside `python3`'s
///    site-packages directories, then (on Windows) `python`'s
///    ([`PYTHONS`]).
/// 4. `localharness` on `PATH`.
///
/// On Windows steps 3 and 4 look for `localharness.exe` first
/// ([`HARNESS_FILE_NAMES`]).
///
/// An interpreter is asked for its site-packages only when the steps
/// before it find nothing, so an explicit path costs no interpreter
/// start-up, and `python` none when `python3`'s site-packages hold the
/// harness.
///
/// The search runs on tokio's blocking pool, not on the caller's runtime
/// thread: it checks files and may start the interpreters, each of which
/// gets [`DISCOVERY_TIMEOUT`] to answer before the search moves on.
pub(crate) async fn discover_harness(explicit: Option<&Path>) -> Result<PathBuf, AntigravityError> {
    discover(
        explicit.map(Path::to_path_buf),
        std::env::var_os(HARNESS_PATH_ENV).map(PathBuf::from),
        std::env::var_os("PATH"),
        PYTHONS.iter().map(OsString::from).collect(),
        DISCOVERY_TIMEOUT,
    )
    .await
}

/// [`discover_harness`] with its inputs and the interpreters' bound
/// given, for tests.
async fn discover(
    explicit: Option<PathBuf>,
    env_path: Option<PathBuf>,
    path_var: Option<OsString>,
    interpreters: Vec<OsString>,
    interpreter_timeout: Duration,
) -> Result<PathBuf, AntigravityError> {
    off_runtime(move || {
        discover_in(
            HARNESS_FILE_NAMES,
            explicit.as_deref(),
            env_path.as_deref(),
            interpreters
                .iter()
                .map(|interpreter| python_site_dirs(interpreter, interpreter_timeout)),
            path_var.as_deref(),
        )
    })
    .await
    .map_err(|source| AntigravityError::Io {
        message: "harness discovery was cancelled".to_string(),
        source,
    })?
}

/// Runs blocking work (a filesystem walk, an interpreter's start-up) on
/// tokio's blocking pool, so it never stalls the runtime's worker threads.
/// A panic in `work` is resumed here. A cancellation, which only a runtime
/// shutting down first can cause, is an I/O error.
async fn off_runtime<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| match e.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(cancelled) => std::io::Error::other(cancelled),
        })
}

/// Pure discovery core, separated for unit testing. `names` are the binary's
/// file names to look for in site-packages and on `PATH`. `site_dirs` yields
/// each interpreter's site-packages directories in turn; it is advanced only
/// while the explicit path, the environment variable and the interpreters
/// before miss.
fn discover_in(
    names: &[&str],
    explicit: Option<&Path>,
    env_path: Option<&Path>,
    site_dirs: impl IntoIterator<Item = Vec<PathBuf>>,
    path_var: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, AntigravityError> {
    let mut searched = Vec::new();

    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        searched.push(format!("explicit path {}", path.display()));
    }

    if let Some(path) = env_path {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        searched.push(format!("{HARNESS_PATH_ENV}={}", path.display()));
    } else if explicit.is_none() {
        searched.push(format!("{HARNESS_PATH_ENV} (unset)"));
    }

    let mut any_site_dirs = false;
    for site_dirs in site_dirs {
        for dir in &site_dirs {
            any_site_dirs = true;
            for name in names {
                let candidate = dir.join(format!("{SITE_PACKAGES_BIN_DIR}/{name}"));
                if candidate.is_file() {
                    return Ok(candidate);
                }
                searched.push(candidate.display().to_string());
            }
        }
    }
    if !any_site_dirs {
        searched.push(format!(
            "{} site-packages (not found, or no site dirs)",
            PYTHONS.join(" or ")
        ));
    }

    if let Some(path_var) = path_var {
        for dir in std::env::split_paths(path_var) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
        searched.push(format!("{} on PATH", names.join(" or ")));
    }

    Err(AntigravityError::HarnessNotFound { searched })
}

/// Asks `interpreter` for its site-packages directories, giving it
/// `timeout` to answer. Returns an empty list when it is unavailable,
/// fails, or does not answer in time (it is then killed): discovery moves
/// on to the next interpreter, then to `PATH`.
///
/// Blocking: call it off the runtime ([`discover`] does). Its stdout is
/// read on a thread of its own, so the bound holds even if something the
/// interpreter started keeps the pipe open; that thread ends when the pipe
/// closes.
fn python_site_dirs(interpreter: &OsStr, timeout: Duration) -> Vec<PathBuf> {
    let deadline = Instant::now() + timeout;
    let Ok(mut child) = std::process::Command::new(interpreter)
        .arg("-c")
        .arg(
            "import site, sys\n\
             paths = list(getattr(site, 'getsitepackages', lambda: [])())\n\
             usersite = getattr(site, 'getusersitepackages', lambda: None)()\n\
             if usersite: paths.append(usersite)\n\
             print('\\n'.join(paths))",
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let read = stdout.read_to_end(&mut output);
            let _ = sender.send(read.map(|_| output));
        });
    }
    let output = receiver.recv_timeout(timeout);
    // Reap it, or kill it once the deadline passes: it must not outlive
    // discovery either way.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    match (output, status) {
        (Ok(Ok(output)), Some(status)) if status.success() => String::from_utf8_lossy(&output)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect(),
        (_, None) => {
            tracing::warn!(
                "{} did not list its site-packages within {timeout:?}; looking for \
                 localharness on PATH instead",
                interpreter.to_string_lossy()
            );
            Vec::new()
        }
        _ => Vec::new(),
    }
}

// =============================================================================
// Process lifecycle
// =============================================================================

/// A running harness process with a drained stderr and an open stdin.
///
/// Stdin must stay open for the harness's lifetime — EOF on stdin is the
/// graceful shutdown signal.
pub(crate) struct HarnessProcess {
    /// `None` once the process has been reaped, so `Drop` knows there is
    /// nothing left to kill.
    child: Option<Child>,
    /// Held open; dropped first during shutdown to signal EOF.
    stdin: Option<ChildStdin>,
    stderr_lines: Arc<Mutex<StderrRing>>,
    /// The `google-antigravity` wheel version the binary came from, when
    /// its install layout says (see [`harness_version`]).
    version: Option<String>,
}

impl std::fmt::Debug for HarnessProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HarnessProcess")
            .field("pid", &self.child.as_ref().and_then(Child::id))
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// Reads one length-prefixed (u32 LE) handshake frame from `reader`,
/// rejecting frames whose declared length exceeds
/// [`MAX_HANDSHAKE_FRAME_LEN`] *before* allocating the payload buffer.
async fn read_handshake_frame<R>(reader: &mut R) -> std::io::Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes).await?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > MAX_HANDSHAKE_FRAME_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "handshake frame declares {len} bytes, exceeding the \
                 {MAX_HANDSHAKE_FRAME_LEN}-byte cap; the binary is likely not a localharness"
            ),
        ));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

/// The environment variables the harness reads a Gemini API key from when
/// its model config carries none (the Go genai SDK's two names; both appear
/// in the 0.1.23 binary).
pub(crate) const API_KEY_ENV_VARS: [&str; 2] = ["GEMINI_API_KEY", "GOOGLE_API_KEY"];

/// The harness command line: piped stdio, its own process group on Unix,
/// `env` added to its environment and, with `scrub_key_env`, none of
/// [`API_KEY_ENV_VARS`] in it (`spawn()` refuses an `env` that sets one
/// while scrubbing).
fn harness_command(
    binary: &Path,
    env: &std::collections::BTreeMap<String, String>,
    scrub_key_env: bool,
) -> Command {
    let mut command = Command::new(binary);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(env);
    // A harness pointed at a custom Gemini host falls back to these when
    // its configured key is missing or empty, and would send that key to
    // the custom host (verified on 0.1.23, 2026-11-04). The builder
    // refuses a blank key; this makes the fallback impossible as well.
    if scrub_key_env {
        for var in API_KEY_ENV_VARS {
            command.env_remove(var);
        }
    }
    // Its own process group, so a forced kill reaches what it started
    // (see `process_tree`). This also keeps a terminal's Ctrl-C from
    // killing the harness outright: the signal reaches this process,
    // and when it exits the harness sees stdin EOF and shuts down
    // gracefully, stopping its commands itself. No `kill_on_drop` on
    // unix: `Drop` kills the whole tree instead.
    #[cfg(unix)]
    command.process_group(0);
    // Elsewhere (Windows) there are no process groups to signal: a forced
    // kill ends the harness process only, and the commands it ran keep
    // running. A Job Object would reach them; none is set up here.
    #[cfg(not(unix))]
    command.kill_on_drop(true);
    command
}

impl HarnessProcess {
    /// Spawns the harness, performs the stdio handshake, and starts the
    /// stderr drain task. With `scrub_key_env`, the harness starts without
    /// [`API_KEY_ENV_VARS`] in its environment (see [`harness_command`]).
    pub(crate) async fn spawn(
        binary: &Path,
        input_config: &InputConfig,
        scrub_key_env: bool,
        wire: &WireContext,
    ) -> Result<(Self, OutputConfig), AntigravityError> {
        let version = harness_version_bounded(binary, DISCOVERY_TIMEOUT).await;
        check_version(binary, version.as_deref());

        let mut child = harness_command(binary, &input_config.env, scrub_key_env)
            .spawn()
            .map_err(|e| AntigravityError::HandshakeFailed {
                message: format!("failed to spawn harness at {}: {e}", binary.display()),
                stderr: String::new(),
            })?;

        wire.emit(|| WireEvent::HarnessSpawn {
            id: wire.id(),
            path: binary.display().to_string(),
            pid: child.id(),
        });

        #[expect(
            clippy::expect_used,
            reason = "harness_command pipes all three, and nothing has taken them yet"
        )]
        let (mut stdin, mut stdout, stderr) = (
            child.stdin.take().expect("stdin was piped"),
            child.stdout.take().expect("stdout was piped"),
            child.stderr.take().expect("stderr was piped"),
        );

        // Drain stderr on a background task from the very start: the OS pipe
        // buffer (typically 64 KiB) would otherwise fill and deadlock the
        // harness. Lines are retained in a bounded ring for diagnostics and
        // surfaced through the wire-inspection layer.
        let stderr_lines = Arc::new(Mutex::new(StderrRing::default()));
        tokio::spawn(drain_stderr(
            stderr,
            Arc::clone(&stderr_lines),
            wire.clone(),
        ));

        let mut process = Self {
            child: Some(child),
            stdin: None,
            stderr_lines,
            version,
        };

        // Handshake: write the length-prefixed InputConfig, read the
        // length-prefixed OutputConfig.
        let handshake = async {
            stdin.write_all(&input_config.encode_frame()).await?;
            stdin.flush().await?;
            read_handshake_frame(&mut stdout).await
        };
        let payload = match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await {
            Ok(Ok(payload)) => payload,
            Ok(Err(e)) => {
                let stderr = process.stderr_tail().await;
                process.kill().await;
                return Err(AntigravityError::HandshakeFailed {
                    message: format!("stdio handshake I/O failed: {e}"),
                    stderr,
                });
            }
            Err(_) => {
                let stderr = process.stderr_tail().await;
                process.kill().await;
                return Err(AntigravityError::HandshakeFailed {
                    message: format!("no handshake reply within {HANDSHAKE_TIMEOUT:?}"),
                    stderr,
                });
            }
        };
        let output_config = match OutputConfig::decode(&payload) {
            Ok(config) => config,
            Err(e) => {
                let stderr = process.stderr_tail().await;
                process.kill().await;
                return Err(AntigravityError::HandshakeFailed {
                    message: format!("invalid OutputConfig: {e}"),
                    stderr,
                });
            }
        };

        // Keep stdin open: EOF is the shutdown signal. Stdout is done after
        // the handshake and may be dropped.
        process.stdin = Some(stdin);
        Ok((process, output_config))
    }

    /// Returns the retained tail of the harness's stderr for diagnostics.
    ///
    /// Yields briefly first so the drain task can catch up with output the
    /// harness wrote just before failing.
    pub(crate) async fn stderr_tail(&self) -> String {
        tokio::time::sleep(Duration::from_millis(50)).await;
        lock_ring(&self.stderr_lines).tail()
    }

    /// The `google-antigravity` version the binary was installed from, when
    /// known (see [`harness_version`]).
    pub(crate) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Kills the harness and, on Linux and macOS, every process it started,
    /// then reaps it (init-failure path). See [`Self::signal`] for the
    /// reach.
    pub(crate) async fn kill(&mut self) {
        self.signal(Signal::Kill);
        self.wait_for_exit(KILL_GRACE).await;
    }

    /// Graceful, escalating shutdown:
    ///
    /// 1. Close stdin — the harness's main loop sees EOF, cleans up
    ///    (serializes trajectories, stops the commands it started) and
    ///    exits on its own.
    /// 2. After [`SHUTDOWN_GRACE`], escalate to SIGTERM for the harness and
    ///    everything it started (`process_tree`). The harness takes
    ///    SIGTERM's default action (0.1.23), so this step no longer gives it
    ///    a chance to clean up; it is for the processes it started.
    /// 3. After [`KILL_GRACE`], escalate to SIGKILL.
    ///
    /// Steps 2 and 3 are Unix's. Elsewhere both terminate the harness
    /// process alone (see [`Self::signal`]), so only step 1 stops the
    /// commands it started.
    ///
    /// A harness that exits on its own with a failure status (step 1) is
    /// reported as [`AntigravityError::HarnessExited`] with its stderr
    /// tail, as the Python SDK raises: a failed exit there usually means
    /// the trajectory was not saved. The escalated paths return `Ok`; the
    /// harness was killed on purpose.
    pub(crate) async fn shutdown(mut self) -> Result<(), AntigravityError> {
        drop(self.stdin.take());
        if let Some(status) = self.wait_for_exit(SHUTDOWN_GRACE).await {
            if status.success() {
                return Ok(());
            }
            return Err(AntigravityError::HarnessExited {
                code: status.code(),
                stderr: self.stderr_tail().await,
            });
        }

        tracing::warn!("Harness did not exit after stdin EOF; sending SIGTERM.");
        self.signal(Signal::Term);
        if self.wait_for_exit(KILL_GRACE).await.is_some() {
            return Ok(());
        }

        tracing::warn!("Harness ignored SIGTERM; sending SIGKILL.");
        self.signal(Signal::Kill);
        self.wait_for_exit(KILL_GRACE).await;
        Ok(())
    }

    /// Waits up to `grace` for the harness to exit, reaping it if it does,
    /// and returns its exit status (a success when it was already reaped).
    /// The child stays in `self` while waiting, so a cancelled shutdown
    /// still leaves `Drop` something to kill.
    async fn wait_for_exit(&mut self, grace: Duration) -> Option<std::process::ExitStatus> {
        let Some(child) = self.child.as_mut() else {
            return Some(std::process::ExitStatus::default());
        };
        match tokio::time::timeout(grace, child.wait()).await {
            Ok(status) => {
                self.child = None;
                // A failed wait means the child is gone and unreapable;
                // there is no status to report.
                Some(status.unwrap_or_default())
            }
            Err(_) => None,
        }
    }

    /// Signals the harness's process tree. Nothing is sent once the child
    /// has been reaped: its group id may no longer be ours.
    ///
    /// On Unix that is the harness's process group and, on Linux and macOS,
    /// the group of each of its children (`process_tree`). Elsewhere
    /// (Windows) there are no groups and no SIGTERM: either signal
    /// terminates the harness process alone, and the `run_command` commands
    /// it started keep running.
    /// (Stdio MCP servers exit either way, at the end of their stdin.)
    fn signal(&mut self, signal: Signal) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            let signal = match signal {
                Signal::Term => libc::SIGTERM,
                Signal::Kill => libc::SIGKILL,
            };
            super::process_tree::signal_tree(pid, signal);
        }
        // No process groups or SIGTERM equivalent: kill the harness itself.
        #[cfg(not(unix))]
        {
            let _ = signal;
            let _ = child.start_kill();
        }
    }
}

/// Dropping a harness that was never shut down kills it, and on Linux and
/// macOS every process it started (stdio MCP servers, `run_command`
/// commands), without the graceful exit's trajectory persistence. On other
/// Unixes and on Windows its `run_command` commands keep running (see
/// [`HarnessProcess::signal`]). Signal only: `Drop` cannot wait, so the
/// reap is left to tokio's orphan reaper.
impl Drop for HarnessProcess {
    fn drop(&mut self) {
        self.signal(Signal::Kill);
    }
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

// =============================================================================
// Harness version
// =============================================================================

/// Reads the `google-antigravity` version a harness binary was installed
/// from.
///
/// The binary carries no version of its own (`localharness --version` just
/// reads stdin for a handshake, 0.1.23), and the handshake reply carries
/// none either. A wheel install lays the binary out as
/// `<site-packages>/google/antigravity/bin/localharness[.exe]` (one of
/// `names`, which is [`HARNESS_FILE_NAMES`] outside tests) beside
/// `<site-packages>/google_antigravity-<version>.dist-info/METADATA`, so
/// the version is read from there. Symlinks (a pipx or `uv tool` shim on
/// `PATH`) are resolved first.
///
/// `None` when the binary is not inside a wheel install (a hand-placed
/// copy), or the metadata is missing or names more than one version.
pub(crate) fn harness_version(names: &[&str], binary: &Path) -> Option<String> {
    let binary = binary
        .canonicalize()
        .unwrap_or_else(|_| binary.to_path_buf());
    let site_packages = binary.ancestors().nth(4)?;
    let in_wheel = names.iter().any(|name| {
        site_packages
            .join(format!("{SITE_PACKAGES_BIN_DIR}/{name}"))
            .is_file()
    });
    if !in_wheel {
        return None;
    }
    let mut versions: Vec<String> = std::fs::read_dir(site_packages)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("google_antigravity-") && name.ends_with(".dist-info")
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("METADATA")).ok())
        .filter_map(|metadata| metadata_version(&metadata))
        .collect();
    versions.sort();
    versions.dedup();
    match versions.as_slice() {
        [version] => Some(version.clone()),
        _ => None,
    }
}

/// [`harness_version`] on a thread of its own, given `timeout`: the read
/// resolves symlinks and reads files beside the binary, which a stalled
/// network mount could hold forever. A read that times out is left to
/// finish on its own and the version counts as unknown.
///
/// The thread is detached rather than taken from tokio's blocking pool
/// because dropping a runtime waits for the blocking tasks still running
/// on it: an abandoned read stuck there would hang the end of
/// `#[tokio::main]` instead of `spawn()`. A panic in the read is resumed
/// here.
async fn harness_version_bounded(binary: &Path, timeout: Duration) -> Option<String> {
    let path = binary.to_path_buf();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let started = std::thread::Builder::new()
        .name("antigravity-version-read".to_string())
        .spawn(move || {
            let outcome = std::panic::catch_unwind(|| harness_version(HARNESS_FILE_NAMES, &path));
            // Fails only when `spawn()` stopped waiting (the timeout fired).
            let _ = sender.send(outcome);
        });
    if let Err(e) = started {
        tracing::debug!("could not start a thread to read the harness version: {e}");
        return None;
    }
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(Ok(version))) => version,
        Ok(Ok(Err(panic))) => std::panic::resume_unwind(panic),
        Ok(Err(_)) => {
            tracing::debug!("the harness version read ended without a result");
            None
        }
        Err(_) => {
            tracing::warn!(
                "reading the google-antigravity version beside {} took over {timeout:?}; \
                 going on without it",
                binary.display()
            );
            None
        }
    }
}

/// Warns when the harness is not the version this build was written
/// against.
///
/// A warning, not an error. The failures a mismatch causes are already
/// loud: the harness rejects a config field it does not know at init (with
/// its reason on stderr), and a turn that never ends names the version in
/// its timeout error ([`version_mismatch`]). An error would also block
/// running a newer wheel deliberately, which is how a harness bump is
/// tested. Callers who want to refuse a mismatch can compare
/// [`AntigravityAgent::harness_version`](super::AntigravityAgent::harness_version)
/// with [`SUPPORTED_HARNESS_VERSION`] themselves.
fn check_version(binary: &Path, version: Option<&str>) {
    match version {
        Some(version) if version == SUPPORTED_HARNESS_VERSION => {
            tracing::debug!("localharness {version} at {}", binary.display());
        }
        Some(version) => tracing::warn!(
            "localharness at {} is google-antigravity {version}, but this build of genai-rs \
             supports {SUPPORTED_HARNESS_VERSION}; protocol changes may fail turns or go \
             unnoticed. Install the supported version with \
             `pip install google-antigravity=={SUPPORTED_HARNESS_VERSION}`.",
            binary.display()
        ),
        None => tracing::debug!(
            "localharness at {} is not inside a google-antigravity wheel install; its version \
             is unknown",
            binary.display()
        ),
    }
}

/// Describes a known version mismatch for an error message: `Some` only
/// when the harness version is known and differs from
/// [`SUPPORTED_HARNESS_VERSION`].
pub(crate) fn version_mismatch(version: Option<&str>) -> Option<String> {
    version
        .filter(|version| *version != SUPPORTED_HARNESS_VERSION)
        .map(|version| {
            format!(
                "the harness is google-antigravity {version}; this build supports \
                 {SUPPORTED_HARNESS_VERSION}"
            )
        })
}

/// The `Version:` header of a wheel's `METADATA`, if its `Name:` is
/// `google-antigravity`.
fn metadata_version(metadata: &str) -> Option<String> {
    let mut name = None;
    let mut version = None;
    // The headers end at the first blank line; the description follows.
    for line in metadata.lines().take_while(|line| !line.trim().is_empty()) {
        if let Some(value) = line.strip_prefix("Name:") {
            name = Some(value.trim().to_ascii_lowercase().replace('_', "-"));
        } else if let Some(value) = line.strip_prefix("Version:") {
            version = Some(value.trim().to_string());
        }
    }
    (name.as_deref() == Some("google-antigravity"))
        .then_some(version)
        .flatten()
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
