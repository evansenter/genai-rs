use super::*;
use std::collections::BTreeMap;

/// A `site_dirs` source that must never be consulted.
fn no_python() -> Vec<PathBuf> {
    panic!("discovery asked python3 for site-packages it did not need")
}

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
    let found = discover_in(Some(&explicit), Some(&env), no_python, None).unwrap();
    assert_eq!(found, explicit);
}

#[test]
fn test_discovery_env_var_beats_site_packages() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("env/localharness");
    let site = dir.path().join("site");
    touch_executable(&env);
    touch_executable(&site.join(SITE_PACKAGES_SUFFIX));
    let found = discover_in(None, Some(&env), no_python, None).unwrap();
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
        || vec![site.clone()],
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
    let found = discover_in(None, None, Vec::new, Some(path_dir.as_os_str())).unwrap();
    assert_eq!(found, path_dir.join("localharness"));
}

#[test]
fn test_discovery_missing_explicit_falls_through() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope/localharness");
    let path_dir = dir.path().join("bin");
    touch_executable(&path_dir.join("localharness"));
    let found = discover_in(Some(&missing), None, Vec::new, Some(path_dir.as_os_str())).unwrap();
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

/// A stand-in `python3`: a shell script running `body`.
#[cfg(unix)]
fn stub_interpreter(dir: &Path, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("python3");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn test_python_site_dirs_reads_the_interpreters_answer() {
    let dir = tempfile::tempdir().unwrap();
    let python = stub_interpreter(dir.path(), "echo /a/site-packages; echo; echo '  /b  '");
    assert_eq!(
        python_site_dirs(python.as_os_str(), Duration::from_secs(10)),
        [PathBuf::from("/a/site-packages"), PathBuf::from("/b")]
    );
    // A failing or missing interpreter gives nothing, and discovery moves on.
    let failing = stub_interpreter(&dir.path().join("fail"), "echo /a; exit 1");
    assert!(python_site_dirs(failing.as_os_str(), Duration::from_secs(10)).is_empty());
    let missing = dir.path().join("no-such-python");
    assert!(python_site_dirs(missing.as_os_str(), Duration::from_secs(10)).is_empty());
}

#[cfg(unix)]
#[test]
fn test_python_site_dirs_gives_up_on_a_hung_interpreter() {
    let dir = tempfile::tempdir().unwrap();
    let python = stub_interpreter(dir.path(), "exec sleep 60");
    let started = std::time::Instant::now();
    assert!(python_site_dirs(python.as_os_str(), Duration::from_millis(200)).is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a hung interpreter held discovery for {:?}",
        started.elapsed()
    );
}

/// Counts 10 ms ticks on the current runtime until dropped: a call that
/// blocks a current-thread runtime's only thread stops the count.
#[cfg(unix)]
fn ticker() -> (
    Arc<std::sync::atomic::AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&ticks);
    let task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
    (ticks, task)
}

#[cfg(unix)]
#[tokio::test] // current-thread: blocking here would stop the ticker
async fn test_discovery_runs_off_the_runtime_and_bounds_the_interpreter() {
    let dir = tempfile::tempdir().unwrap();
    let python = stub_interpreter(dir.path(), "exec sleep 60");
    let path_dir = dir.path().join("bin");
    touch_executable(&path_dir.join("localharness"));

    let (ticks, ticking) = ticker();
    let started = std::time::Instant::now();
    let found = discover(
        None,
        None,
        Some(path_dir.clone().into_os_string()),
        python.into_os_string(),
        Duration::from_millis(500),
    )
    .await
    .unwrap();
    let elapsed = started.elapsed();
    ticking.abort();

    assert_eq!(
        found,
        path_dir.join("localharness"),
        "PATH after the hung python3"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "discovery took {elapsed:?}"
    );
    let ticks = ticks.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        ticks >= 5,
        "the runtime's only thread was blocked during discovery: {ticks} ticks in {elapsed:?}"
    );
}

#[cfg(unix)]
#[tokio::test] // current-thread: blocking here would stop the ticker
async fn test_version_read_runs_off_the_runtime_under_a_timeout() {
    // A METADATA that is a FIFO with no writer blocks its reader, as a
    // stalled network mount would.
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site-packages");
    let binary = wheel_install(&site, &[]);
    let dist_info = site.join("google_antigravity-0.1.23.dist-info");
    std::fs::create_dir_all(&dist_info).unwrap();
    let fifo = dist_info.join("METADATA");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo failed");

    let (ticks, ticking) = ticker();
    let started = std::time::Instant::now();
    let version = harness_version_bounded(&binary, Duration::from_millis(300)).await;
    let elapsed = started.elapsed();
    ticking.abort();

    assert_eq!(
        version, None,
        "a read that times out leaves the version unknown"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "the read took {elapsed:?}"
    );
    let ticks = ticks.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        ticks >= 5,
        "the runtime's only thread was blocked by the version read: {ticks} ticks in {elapsed:?}"
    );
    // Release the abandoned read, so the runtime's blocking pool can wind
    // down when the test ends.
    drop(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
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
    let scrubbed = harness_command(
        Path::new("/nonexistent/localharness"),
        &BTreeMap::new(),
        true,
    );
    let mut names = removed(&scrubbed);
    names.sort();
    assert_eq!(names, ["GEMINI_API_KEY", "GOOGLE_API_KEY"]);

    let inherited = harness_command(
        Path::new("/nonexistent/localharness"),
        &BTreeMap::new(),
        false,
    );
    assert!(
        removed(&inherited).is_empty(),
        "without a custom host the harness keeps its environment"
    );
}

#[test]
fn test_harness_command_adds_the_configured_env() {
    let env = BTreeMap::from([("AGY_PROBE".to_string(), "on".to_string())]);
    let command = harness_command(Path::new("/nonexistent/localharness"), &env, false);
    let set: Vec<_> = command
        .as_std()
        .get_envs()
        .filter_map(|(k, v)| Some((k.to_str()?.to_string(), v?.to_str()?.to_string())))
        .collect();
    assert_eq!(set, [("AGY_PROBE".to_string(), "on".to_string())]);
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
        || vec![site.clone()],
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

/// A stand-in harness process: `script` runs under `sh`, with stdin piped
/// (EOF is the shutdown signal) and stderr drained like the real one's.
#[cfg(unix)]
fn fake_harness(script: &str) -> HarnessProcess {
    let mut child = Command::new("sh")
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sh");
    let stdin = child.stdin.take();
    let stderr = child.stderr.take().expect("stderr was piped");
    let stderr_lines = Arc::new(Mutex::new(StderrRing::default()));
    tokio::spawn(drain_stderr(
        stderr,
        Arc::clone(&stderr_lines),
        WireContext::new(Vec::new()),
    ));
    HarnessProcess {
        child: Some(child),
        stdin,
        stderr_lines,
        version: None,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_shutdown_reports_a_failed_graceful_exit_with_the_stderr_tail() {
    let failing = fake_harness("cat >/dev/null; echo 'could not save trajectory' >&2; exit 3");
    let err = failing.shutdown().await.unwrap_err();
    let AntigravityError::HarnessExited { code, stderr } = &err else {
        panic!("expected HarnessExited, got {err:?}");
    };
    assert_eq!(*code, Some(3));
    assert!(stderr.contains("could not save trajectory"), "{stderr}");
    assert!(err.to_string().contains("code 3"), "{err}");

    let clean = fake_harness("cat >/dev/null; exit 0");
    clean.shutdown().await.expect("a clean exit is Ok");
}
