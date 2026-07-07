//! Harness process management: binary discovery, spawn + stdio handshake,
//! stderr draining, and ordered shutdown.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

use super::AntigravityError;
use super::handshake::{InputConfig, OutputConfig};
use super::session::WireContext;
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
    child: Child,
    /// Held open; dropped first during shutdown to signal EOF.
    stdin: Option<ChildStdin>,
    stderr_lines: Arc<Mutex<VecDeque<String>>>,
}

impl std::fmt::Debug for HarnessProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HarnessProcess")
            .field("pid", &self.child.id())
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

impl HarnessProcess {
    /// Spawns the harness, performs the stdio handshake, and starts the
    /// stderr drain task.
    pub(crate) async fn spawn(
        binary: &Path,
        input_config: &InputConfig,
        wire: &WireContext,
    ) -> Result<(Self, OutputConfig), AntigravityError> {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Last-resort zombie hygiene: if this struct is dropped without
            // an explicit shutdown, the OS still reaps the harness.
            .kill_on_drop(true)
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

        let mut process = Self {
            child,
            stdin: None,
            stderr_lines,
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
                let _ = process.child.start_kill();
                return Err(AntigravityError::HandshakeFailed {
                    message: format!("stdio handshake I/O failed: {e}"),
                    stderr,
                });
            }
            Err(_) => {
                let stderr = process.stderr_tail().await;
                let _ = process.child.start_kill();
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
                let _ = process.child.start_kill();
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

    /// Kills the harness immediately (init-failure path).
    pub(crate) async fn kill(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }

    /// Graceful, escalating shutdown:
    ///
    /// 1. Close stdin — the harness's main loop sees EOF, cleans up
    ///    (serializes trajectories) and exits on its own.
    /// 2. After [`SHUTDOWN_GRACE`], escalate to SIGTERM.
    /// 3. After [`KILL_GRACE`], escalate to SIGKILL.
    pub(crate) async fn shutdown(mut self) -> Result<(), AntigravityError> {
        drop(self.stdin.take());

        if tokio::time::timeout(SHUTDOWN_GRACE, self.child.wait())
            .await
            .is_ok()
        {
            return Ok(());
        }

        tracing::warn!("Harness did not exit after stdin EOF; sending SIGTERM.");
        self.terminate();
        if tokio::time::timeout(KILL_GRACE, self.child.wait())
            .await
            .is_ok()
        {
            return Ok(());
        }

        tracing::warn!("Harness ignored SIGTERM; sending SIGKILL.");
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(KILL_GRACE, self.child.wait()).await;
        Ok(())
    }

    #[cfg(unix)]
    fn terminate(&self) {
        if let Some(pid) = self.child.id() {
            // SAFETY: plain signal dispatch to a pid we own; no memory is
            // shared with the callee.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    #[cfg(not(unix))]
    fn terminate(&self) {
        // No SIGTERM equivalent: go straight to the kill escalation.
    }
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
