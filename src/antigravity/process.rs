//! Harness process management: binary discovery, spawn + stdio handshake,
//! stderr draining, and ordered shutdown.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

use super::handshake::{InputConfig, OutputConfig};
use super::session::WireContext;
use super::{AntigravityError, SUPPORTED_HARNESS_VERSION};
use crate::wire::WireEvent;

/// Environment variable naming the harness binary (same variable the
/// reference Python SDK honors).
pub(crate) const HARNESS_PATH_ENV: &str = "ANTIGRAVITY_HARNESS_PATH";

/// Relative location of the harness binary inside a Python site-packages
/// directory (from the `google-antigravity` wheel).
const SITE_PACKAGES_SUFFIX: &str = "google/antigravity/bin/localharness";

/// Maximum stderr lines retained for diagnostics.
const STDERR_RING_CAPACITY: usize = 200;

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
///    site-packages directories.
/// 4. `localharness` on `PATH`.
pub(crate) fn discover_harness(explicit: Option<&Path>) -> Result<PathBuf, AntigravityError> {
    let env_path = std::env::var_os(HARNESS_PATH_ENV).map(PathBuf::from);
    let site_dirs = python_site_dirs();
    let path_var = std::env::var_os("PATH");
    discover_in(
        explicit,
        env_path.as_deref(),
        &site_dirs,
        path_var.as_deref(),
    )
}

/// Pure discovery core, separated for unit testing.
fn discover_in(
    explicit: Option<&Path>,
    env_path: Option<&Path>,
    site_dirs: &[PathBuf],
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

    for dir in site_dirs {
        let candidate = dir.join(SITE_PACKAGES_SUFFIX);
        if candidate.is_file() {
            return Ok(candidate);
        }
        searched.push(candidate.display().to_string());
    }
    if site_dirs.is_empty() {
        searched.push("python3 site-packages (python3 not found or no site dirs)".to_string());
    }

    if let Some(path_var) = path_var {
        for dir in std::env::split_paths(path_var) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join("localharness");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        searched.push("localharness on PATH".to_string());
    }

    Err(AntigravityError::HarnessNotFound { searched })
}

/// Asks `python3` for its site-packages directories. Returns an empty list
/// when `python3` is unavailable — discovery then falls through to `PATH`.
fn python_site_dirs() -> Vec<PathBuf> {
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(
            "import site, sys\n\
             paths = list(getattr(site, 'getsitepackages', lambda: [])())\n\
             usersite = getattr(site, 'getusersitepackages', lambda: None)()\n\
             if usersite: paths.append(usersite)\n\
             print('\\n'.join(paths))",
        )
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect(),
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
    stderr_lines: Arc<Mutex<VecDeque<String>>>,
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

/// Drains a stderr stream line by line into the diagnostic ring (bounded
/// to [`STDERR_RING_CAPACITY`]), forwarding each line to `tracing` and the
/// wire-inspection layer.
///
/// Reads bytes rather than UTF-8 lines: this sits at the wrong-binary
/// trust boundary, and a single non-UTF-8 line must not stop the drain —
/// a stopped drain lets the OS pipe buffer fill and deadlocks the child.
/// Invalid UTF-8 is replaced lossily; only EOF or a genuine I/O error ends
/// the loop.
async fn drain_stderr<R>(stderr: R, ring: Arc<Mutex<VecDeque<String>>>, wire: WireContext)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stderr);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => return, // EOF: the child closed its stderr.
            Ok(_) => {
                // Strip the `\n` (and a preceding `\r`), matching what
                // `AsyncBufReadExt::lines` used to do here.
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                tracing::debug!("harness stderr: {line}");
                wire.emit(|| WireEvent::HarnessStderr {
                    id: wire.id(),
                    line: line.clone(),
                });
                let mut ring = ring.lock().expect("stderr ring lock");
                if ring.len() == STDERR_RING_CAPACITY {
                    ring.pop_front();
                }
                ring.push_back(line);
            }
            Err(e) => {
                tracing::debug!("harness stderr drain ended on read error: {e}");
                return;
            }
        }
    }
}

/// The environment variables the harness reads a Gemini API key from when
/// its model config carries none (the Go genai SDK's two names; both appear
/// in the 0.1.23 binary).
const API_KEY_ENV_VARS: [&str; 2] = ["GEMINI_API_KEY", "GOOGLE_API_KEY"];

/// The harness command line: piped stdio, its own process group on Unix,
/// and, with `scrub_key_env`, none of [`API_KEY_ENV_VARS`] in its
/// environment.
fn harness_command(binary: &Path, scrub_key_env: bool) -> Command {
    let mut command = Command::new(binary);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
        let mut child = harness_command(binary, scrub_key_env)
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

        let mut stdin = child.stdin.take().expect("stdin was piped");
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        // Drain stderr on a background task from the very start: the OS pipe
        // buffer (typically 64 KiB) would otherwise fill and deadlock the
        // harness. Lines are retained in a bounded ring for diagnostics and
        // surfaced through the wire-inspection layer.
        let stderr_lines = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING_CAPACITY)));
        tokio::spawn(drain_stderr(
            stderr,
            Arc::clone(&stderr_lines),
            wire.clone(),
        ));

        let version = harness_version(binary);
        check_version(binary, version.as_deref());
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
        let ring = self.stderr_lines.lock().expect("stderr ring lock");
        ring.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    /// The `google-antigravity` version the binary was installed from, when
    /// known (see [`harness_version`]).
    pub(crate) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Kills the harness and every process it started, then reaps it
    /// (init-failure path).
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
    pub(crate) async fn shutdown(mut self) -> Result<(), AntigravityError> {
        drop(self.stdin.take());
        if self.wait_for_exit(SHUTDOWN_GRACE).await {
            return Ok(());
        }

        tracing::warn!("Harness did not exit after stdin EOF; sending SIGTERM.");
        self.signal(Signal::Term);
        if self.wait_for_exit(KILL_GRACE).await {
            return Ok(());
        }

        tracing::warn!("Harness ignored SIGTERM; sending SIGKILL.");
        self.signal(Signal::Kill);
        self.wait_for_exit(KILL_GRACE).await;
        Ok(())
    }

    /// Waits up to `grace` for the harness to exit, reaping it if it does.
    /// The child stays in `self` while waiting, so a cancelled shutdown
    /// still leaves `Drop` something to kill.
    async fn wait_for_exit(&mut self, grace: Duration) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            self.child = None;
            true
        } else {
            false
        }
    }

    /// Signals the harness's process tree. Nothing is sent once the child
    /// has been reaped: its group id may no longer be ours.
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

/// Dropping a harness that was never shut down kills it and every process
/// it started (stdio MCP servers, `run_command` commands), without the
/// graceful exit's trajectory persistence. Signal only: `Drop` cannot wait,
/// so the reap is left to tokio's orphan reaper.
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
/// `<site-packages>/google/antigravity/bin/localharness` beside
/// `<site-packages>/google_antigravity-<version>.dist-info/METADATA`, so
/// the version is read from there. Symlinks (a pipx or `uv tool` shim on
/// `PATH`) are resolved first.
///
/// `None` when the binary is not inside a wheel install (a hand-placed
/// copy), or the metadata is missing or names more than one version.
pub(crate) fn harness_version(binary: &Path) -> Option<String> {
    let binary = binary
        .canonicalize()
        .unwrap_or_else(|_| binary.to_path_buf());
    let site_packages = binary.ancestors().nth(4)?;
    if !site_packages.join(SITE_PACKAGES_SUFFIX).is_file() {
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
mod tests {
    use super::*;

    fn touch_executable(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn test_discovery_prefers_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = dir.path().join("explicit/localharness");
        let env = dir.path().join("env/localharness");
        touch_executable(&explicit);
        touch_executable(&env);
        let found = discover_in(Some(&explicit), Some(&env), &[], None).unwrap();
        assert_eq!(found, explicit);
    }

    #[test]
    fn test_discovery_env_var_beats_site_packages() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join("env/localharness");
        let site = dir.path().join("site");
        touch_executable(&env);
        touch_executable(&site.join(SITE_PACKAGES_SUFFIX));
        let found = discover_in(None, Some(&env), std::slice::from_ref(&site), None).unwrap();
        assert_eq!(found, env);
    }

    #[test]
    fn test_discovery_site_packages_beats_path() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        let path_dir = dir.path().join("bin");
        touch_executable(&site.join(SITE_PACKAGES_SUFFIX));
        touch_executable(&path_dir.join("localharness"));
        let found = discover_in(
            None,
            None,
            std::slice::from_ref(&site),
            Some(path_dir.as_os_str()),
        )
        .unwrap();
        assert_eq!(found, site.join(SITE_PACKAGES_SUFFIX));
    }

    #[test]
    fn test_discovery_falls_back_to_path() {
        let dir = tempfile::tempdir().unwrap();
        let path_dir = dir.path().join("bin");
        touch_executable(&path_dir.join("localharness"));
        let found = discover_in(None, None, &[], Some(path_dir.as_os_str())).unwrap();
        assert_eq!(found, path_dir.join("localharness"));
    }

    #[test]
    fn test_discovery_missing_explicit_falls_through() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope/localharness");
        let path_dir = dir.path().join("bin");
        touch_executable(&path_dir.join("localharness"));
        let found = discover_in(Some(&missing), None, &[], Some(path_dir.as_os_str())).unwrap();
        assert_eq!(found, path_dir.join("localharness"));
    }

    #[tokio::test]
    async fn test_handshake_frame_rejects_oversized_length_before_allocating() {
        // Length prefix declares ~4 GiB; the payload never follows. The cap
        // must reject it from the prefix alone (no allocation, no read).
        let data = u32::MAX.to_le_bytes();
        let err = read_handshake_frame(&mut &data[..]).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("exceeding"), "got: {err}");
    }

    #[tokio::test]
    async fn test_handshake_frame_rejects_just_over_cap() {
        let data = ((MAX_HANDSHAKE_FRAME_LEN as u32) + 1).to_le_bytes();
        let err = read_handshake_frame(&mut &data[..]).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn test_handshake_frame_reads_valid_frame() {
        let mut data = 5u32.to_le_bytes().to_vec();
        data.extend_from_slice(b"hello");
        let payload = read_handshake_frame(&mut &data[..]).await.unwrap();
        assert_eq!(payload, b"hello");
    }

    #[tokio::test]
    async fn test_handshake_frame_truncated_payload_is_io_error() {
        // Declared length exceeds the available bytes (but is under the cap).
        let mut data = 10u32.to_le_bytes().to_vec();
        data.extend_from_slice(b"short");
        let err = read_handshake_frame(&mut &data[..]).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn test_stderr_drain_survives_invalid_utf8() {
        // The drain sits at the wrong-binary trust boundary: a non-UTF-8
        // line must be replaced lossily, not end the drain (a stopped
        // drain lets the pipe fill and deadlocks the child).
        let data: &[u8] = b"first line\n\xff\xfe broken\nafter\r\nlast without newline";
        let ring = Arc::new(Mutex::new(VecDeque::new()));
        drain_stderr(data, Arc::clone(&ring), WireContext::new(Vec::new())).await;

        let lines: Vec<String> = ring.lock().unwrap().iter().cloned().collect();
        assert_eq!(lines.len(), 4, "got: {lines:?}");
        assert_eq!(lines[0], "first line");
        assert!(
            lines[1].contains('\u{FFFD}') && lines[1].ends_with(" broken"),
            "invalid bytes must be replaced lossily, got: {:?}",
            lines[1]
        );
        assert_eq!(lines[2], "after", "CRLF must be stripped");
        assert_eq!(lines[3], "last without newline");
    }

    #[tokio::test]
    async fn test_stderr_drain_ring_is_bounded() {
        let mut data = Vec::new();
        for i in 0..(STDERR_RING_CAPACITY + 10) {
            data.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        let ring = Arc::new(Mutex::new(VecDeque::new()));
        drain_stderr(&data[..], Arc::clone(&ring), WireContext::new(Vec::new())).await;

        let ring = ring.lock().unwrap();
        assert_eq!(ring.len(), STDERR_RING_CAPACITY);
        assert_eq!(
            ring.back().unwrap(),
            &format!("line {}", STDERR_RING_CAPACITY + 9)
        );
    }

    /// Lays out a wheel install: the binary under `google/antigravity/bin`
    /// and a dist-info directory per version beside it.
    fn wheel_install(site: &Path, versions: &[&str]) -> PathBuf {
        let binary = site.join(SITE_PACKAGES_SUFFIX);
        touch_executable(&binary);
        for version in versions {
            let dist_info = site.join(format!("google_antigravity-{version}.dist-info"));
            std::fs::create_dir_all(&dist_info).unwrap();
            std::fs::write(
                dist_info.join("METADATA"),
                format!(
                    "Metadata-Version: 2.4\nName: google-antigravity\nVersion: {version}\n\n\
                     Version: 9.9.9 (description text, not a header)\n"
                ),
            )
            .unwrap();
        }
        binary
    }

    #[test]
    fn test_harness_version_reads_the_wheel_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let binary = wheel_install(&dir.path().join("site-packages"), &["0.1.23"]);
        assert_eq!(harness_version(&binary).as_deref(), Some("0.1.23"));
    }

    #[cfg(unix)]
    #[test]
    fn test_harness_version_follows_a_path_symlink_into_the_wheel() {
        // pipx / `uv tool` put a symlink on PATH; the layout is read from
        // where it points.
        let dir = tempfile::tempdir().unwrap();
        let binary = wheel_install(&dir.path().join("venv/site-packages"), &["0.1.18"]);
        let link = dir.path().join("bin/localharness");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&binary, &link).unwrap();
        assert_eq!(harness_version(&link).as_deref(), Some("0.1.18"));
    }

    #[test]
    fn test_harness_version_is_none_outside_a_wheel_or_when_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        // A hand-placed binary: no wheel layout around it.
        let bare = dir.path().join("bin/localharness");
        touch_executable(&bare);
        assert_eq!(harness_version(&bare), None);
        // The layout, but no metadata.
        let unlabelled = wheel_install(&dir.path().join("a"), &[]);
        assert_eq!(harness_version(&unlabelled), None);
        // Two installed versions: there is no telling which one this is.
        let ambiguous = wheel_install(&dir.path().join("b"), &["0.1.18", "0.1.23"]);
        assert_eq!(harness_version(&ambiguous), None);
    }

    #[test]
    fn test_version_mismatch_notes_only_a_known_different_version() {
        assert_eq!(version_mismatch(None), None);
        assert_eq!(version_mismatch(Some(SUPPORTED_HARNESS_VERSION)), None);
        let note = version_mismatch(Some("0.1.18")).unwrap();
        assert!(note.contains("0.1.18") && note.contains(SUPPORTED_HARNESS_VERSION));

        // An init failure carries the note; other errors are untouched.
        let init = AntigravityError::InitFailed {
            message: "unknown field \"runWorkflow\"".to_string(),
            stderr: String::new(),
        }
        .with_version_note(Some("0.1.18"));
        let AntigravityError::InitFailed { message, .. } = init else {
            panic!("variant must be kept");
        };
        assert!(message.starts_with("unknown field") && message.contains("0.1.18"));
        let other = AntigravityError::Turn("x".to_string()).with_version_note(Some("0.1.18"));
        assert!(matches!(other, AntigravityError::Turn(m) if m == "x"));
    }

    #[test]
    fn test_metadata_version_needs_the_package_name() {
        assert_eq!(
            metadata_version("Name: google_antigravity\nVersion: 0.1.23\n").as_deref(),
            Some("0.1.23")
        );
        assert_eq!(
            metadata_version("Name: google-genai\nVersion: 2.34.0\n"),
            None
        );
        assert_eq!(metadata_version("Version: 0.1.23\n"), None);
    }

    #[test]
    fn test_harness_command_scrubs_key_env_only_when_asked() {
        let removed = |command: &Command| -> Vec<String> {
            command
                .as_std()
                .get_envs()
                .filter(|(_, value)| value.is_none())
                .map(|(key, _)| key.to_string_lossy().into_owned())
                .collect()
        };
        let scrubbed = harness_command(Path::new("/nonexistent/localharness"), true);
        let mut names = removed(&scrubbed);
        names.sort();
        assert_eq!(names, ["GEMINI_API_KEY", "GOOGLE_API_KEY"]);

        let inherited = harness_command(Path::new("/nonexistent/localharness"), false);
        assert!(
            removed(&inherited).is_empty(),
            "without a custom host the harness keeps its environment"
        );
    }

    #[test]
    fn test_discovery_error_lists_searched_locations() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope/localharness");
        let site = dir.path().join("site");
        std::fs::create_dir_all(&site).unwrap();
        let err = discover_in(
            Some(&missing),
            None,
            std::slice::from_ref(&site),
            Some(std::ffi::OsStr::new("/definitely/not/a/dir")),
        )
        .unwrap_err();
        let AntigravityError::HarnessNotFound { searched } = &err else {
            panic!("expected HarnessNotFound, got {err:?}");
        };
        assert!(searched.iter().any(|s| s.contains("explicit path")));
        assert!(searched.iter().any(|s| s.contains(SITE_PACKAGES_SUFFIX)));
        assert!(searched.iter().any(|s| s.contains("PATH")));
        // The rendered error must point users at the fix.
        let message = err.to_string();
        assert!(message.contains("pip install google-antigravity"));
        assert!(message.contains(HARNESS_PATH_ENV));
    }
}
