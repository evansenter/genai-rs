use super::*;
use std::collections::BTreeMap;

/// Where the wheel puts the binary off Windows, and where discovery still
/// looks on Windows after `localharness.exe`.
const SITE_PACKAGES_SUFFIX: &str = "google/antigravity/bin/localharness";

/// A `site_dirs` source that must never be consulted.
fn no_python() -> impl Iterator<Item = Vec<PathBuf>> {
    std::iter::from_fn(|| {
        panic!("discovery asked an interpreter for site-packages it did not need")
    })
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
    let found = discover_in(
        HARNESS_FILE_NAMES,
        Some(&explicit),
        Some(&env),
        no_python(),
        None,
    )
    .unwrap();
    assert_eq!(found, explicit);
}

#[test]
fn test_discovery_env_var_beats_site_packages() {
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("env/localharness");
    let site = dir.path().join("site");
    touch_executable(&env);
    touch_executable(&site.join(SITE_PACKAGES_SUFFIX));
    let found = discover_in(HARNESS_FILE_NAMES, None, Some(&env), no_python(), None).unwrap();
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
        HARNESS_FILE_NAMES,
        None,
        None,
        [vec![site.clone()]],
        Some(path_dir.as_os_str()),
    )
    .unwrap();
    assert_eq!(found, site.join(SITE_PACKAGES_SUFFIX));
}

#[test]
fn test_harness_file_names_add_the_exe_on_windows() {
    assert_eq!(
        harness_file_names(true),
        ["localharness.exe", "localharness"]
    );
    assert_eq!(harness_file_names(false), ["localharness"]);
    assert_eq!(HARNESS_FILE_NAMES, harness_file_names(cfg!(windows)));
}

#[test]
fn test_python_interpreters_add_python_then_the_py_launcher_on_windows() {
    // python.org's Windows installer has no `python3`, and its `py`
    // launcher can be the only one on PATH; it takes `-3` before `-c`.
    let windows: &[&[&str]] = &[&["python3"], &["python"], &["py", "-3"]];
    assert_eq!(python_interpreters(true), windows);
    assert_eq!(python_interpreters(false), [["python3"]]);
    assert_eq!(PYTHONS, python_interpreters(cfg!(windows)));
}

#[test]
fn test_discovery_asks_the_next_interpreter_only_while_the_harness_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let empty_site = dir.path().join("empty-site");
    let site = dir.path().join("site");
    touch_executable(&site.join(SITE_PACKAGES_SUFFIX));
    // The first interpreter's site-packages lack the harness: the second's
    // are searched.
    let found = discover_in(
        HARNESS_FILE_NAMES,
        None,
        None,
        [vec![empty_site.clone()], vec![site.clone()]],
        None,
    )
    .unwrap();
    assert_eq!(found, site.join(SITE_PACKAGES_SUFFIX));
    // The first interpreter's hold it: the second is never started.
    let found = discover_in(
        HARNESS_FILE_NAMES,
        None,
        None,
        std::iter::once(vec![site.clone()]).chain(no_python()),
        None,
    )
    .unwrap();
    assert_eq!(found, site.join(SITE_PACKAGES_SUFFIX));
    // Neither answers: the error names every interpreter tried.
    let err = discover_in(HARNESS_FILE_NAMES, None, None, [vec![], vec![]], None).unwrap_err();
    let AntigravityError::HarnessNotFound { searched } = &err else {
        panic!("{err:?}");
    };
    let pythons: Vec<_> = PYTHONS.iter().map(|command| command.join(" ")).collect();
    assert!(
        searched
            .iter()
            .any(|entry| entry.starts_with(&format!("{} site-packages", pythons.join(" or ")))),
        "{searched:?}"
    );
}

#[test]
fn test_discovery_finds_the_windows_exe_in_site_packages_and_on_path() {
    let windows = harness_file_names(true);
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    let exe = site.join("google/antigravity/bin/localharness.exe");
    touch_executable(&exe);
    let found = discover_in(windows, None, None, [vec![site.clone()]], None).unwrap();
    assert_eq!(found, exe);

    let path_dir = dir.path().join("bin");
    touch_executable(&path_dir.join("localharness.exe"));
    let found = discover_in(windows, None, None, [], Some(path_dir.as_os_str())).unwrap();
    assert_eq!(found, path_dir.join("localharness.exe"));

    // Off Windows only the bare name counts.
    let err = discover_in(
        harness_file_names(false),
        None,
        None,
        [vec![site.clone()]],
        Some(path_dir.as_os_str()),
    )
    .unwrap_err();
    assert!(
        matches!(err, AntigravityError::HarnessNotFound { .. }),
        "{err:?}"
    );
}

#[test]
fn test_discovery_falls_back_to_path() {
    let dir = tempfile::tempdir().unwrap();
    let path_dir = dir.path().join("bin");
    touch_executable(&path_dir.join("localharness"));
    let found = discover_in(
        HARNESS_FILE_NAMES,
        None,
        None,
        [],
        Some(path_dir.as_os_str()),
    )
    .unwrap();
    assert_eq!(found, path_dir.join("localharness"));
}

#[test]
fn test_discovery_missing_named_path_is_not_found() {
    // A path named by the builder or the environment variable is the
    // harness: when it is missing, discovery fails naming it, and never
    // runs the harness the rest of the search would find instead.
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope/localharness");
    let found_elsewhere = dir.path().join("env/localharness");
    let path_dir = dir.path().join("bin");
    touch_executable(&found_elsewhere);
    touch_executable(&path_dir.join("localharness"));
    // (explicit, env, what the location names, the fix the message gives)
    let env_fix = format!("Point {HARNESS_PATH_ENV} at");
    let cases = [
        (
            Some(&missing),
            None,
            "explicit path",
            "Point `with_harness_path` at",
        ),
        // The explicit path wins over the variable, even a valid one.
        (
            Some(&missing),
            Some(&found_elsewhere),
            "explicit path",
            "Point `with_harness_path` at",
        ),
        (None, Some(&missing), HARNESS_PATH_ENV, env_fix.as_str()),
    ];
    for (explicit, env, named, fix) in cases {
        let err = discover_in(
            HARNESS_FILE_NAMES,
            explicit.map(PathBuf::as_path),
            env.map(PathBuf::as_path),
            no_python(),
            Some(path_dir.as_os_str()),
        )
        .unwrap_err();
        let AntigravityError::HarnessNotFound { searched } = &err else {
            panic!("expected HarnessNotFound, got {err:?}");
        };
        assert_eq!(searched.len(), 1, "{searched:?}");
        assert!(
            searched[0].contains(named) && searched[0].contains(&missing.display().to_string()),
            "{searched:?}"
        );
        // The message points at the path that was named, not at an
        // install or a variable that would not be read.
        let message = err.to_string();
        assert!(
            message.contains(fix) && !message.contains("pip install"),
            "{message}"
        );
    }
}

#[test]
fn test_discovery_treats_an_empty_variable_as_unset() {
    let dir = tempfile::tempdir().unwrap();
    let path_dir = dir.path().join("bin");
    touch_executable(&path_dir.join("localharness"));
    let found = discover_in(
        HARNESS_FILE_NAMES,
        None,
        Some(Path::new("")),
        [],
        Some(path_dir.as_os_str()),
    )
    .unwrap();
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
    stub_named_interpreter(dir, "python3", body)
}

/// A stand-in interpreter called `name`: a shell script running `body`.
#[cfg(unix)]
fn stub_named_interpreter(dir: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
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
        python_site_dirs(&[python.into_os_string()], Duration::from_secs(10)),
        [PathBuf::from("/a/site-packages"), PathBuf::from("/b")]
    );
    // A failing or missing interpreter gives nothing, and discovery moves on.
    let failing = stub_interpreter(&dir.path().join("fail"), "echo /a; exit 1");
    assert!(python_site_dirs(&[failing.into_os_string()], Duration::from_secs(10)).is_empty());
    let missing = dir.path().join("no-such-python");
    assert!(python_site_dirs(&[missing.into_os_string()], Duration::from_secs(10)).is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn test_discovery_finds_and_versions_a_wheel_only_python_has() {
    // Windows' order, where `python3` is the Microsoft Store's stub and
    // python.org's interpreter is `python`.
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    let binary = wheel_install(&site, &["0.1.23"]);
    let python3 = stub_named_interpreter(
        &dir.path().join("store"),
        "python3",
        "echo 'Python was not found' >&2; exit 9009",
    );
    let python = stub_named_interpreter(
        &dir.path().join("python-org"),
        "python",
        &format!("echo '{}'", site.display()),
    );
    let found = discover(
        None,
        None,
        None,
        vec![
            vec![python3.into_os_string()],
            vec![python.into_os_string()],
        ],
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert_eq!(found, binary);
    // The version check reads the wheel it was found in, as through python3.
    assert_eq!(
        harness_version(HARNESS_FILE_NAMES, &found).as_deref(),
        Some("0.1.23")
    );
}

#[cfg(unix)]
#[test]
fn test_python_site_dirs_passes_the_launchers_arguments_before_the_script() {
    let dir = tempfile::tempdir().unwrap();
    // A stand-in `py` that answers only when `-3` comes first and the
    // script after it, as the launcher reads them.
    let py = stub_named_interpreter(
        dir.path(),
        "py",
        r#"[ "$1" = -3 ] && [ "$2" = -c ] && [ -n "$3" ] && echo /launcher/site || exit 2"#,
    );
    let launcher = [py.into_os_string(), OsString::from("-3")];
    assert_eq!(
        python_site_dirs(&launcher, Duration::from_secs(10)),
        [PathBuf::from("/launcher/site")]
    );
    // Without its arguments it answers nothing: the stand-in checks them.
    assert!(python_site_dirs(&launcher[..1], Duration::from_secs(10)).is_empty());
    assert!(python_site_dirs(&[], Duration::from_secs(10)).is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn test_discovery_finds_a_wheel_only_the_py_launcher_has() {
    // Windows' order where neither `python3` nor `python` answers: the
    // Microsoft Store's stub, no `python` on PATH, and python.org's `py`.
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    let binary = wheel_install(&site, &["0.1.23"]);
    let python3 = stub_named_interpreter(
        &dir.path().join("store"),
        "python3",
        "echo 'Python was not found' >&2; exit 9009",
    );
    let py = stub_named_interpreter(
        &dir.path().join("launcher"),
        "py",
        &format!(
            r#"[ "$1" = -3 ] && [ "$2" = -c ] && echo '{}'"#,
            site.display()
        ),
    );
    let interpreters: Vec<Vec<OsString>> = vec![
        vec![python3.into_os_string()],
        vec![dir.path().join("no-python").into_os_string()],
        vec![py.into_os_string(), OsString::from("-3")],
    ];
    let found = discover(None, None, None, interpreters, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(found, binary);
    assert_eq!(
        harness_version(HARNESS_FILE_NAMES, &found).as_deref(),
        Some("0.1.23")
    );
}

#[cfg(unix)]
#[test]
fn test_python_site_dirs_gives_up_on_a_hung_interpreter() {
    let dir = tempfile::tempdir().unwrap();
    let python = stub_interpreter(dir.path(), "exec sleep 60");
    let started = std::time::Instant::now();
    assert!(python_site_dirs(&[python.into_os_string()], Duration::from_millis(200)).is_empty());
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
        vec![vec![python.into_os_string()]],
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
    let dir = tempfile::tempdir().unwrap();
    let (binary, fifo) = stalled_wheel_install(dir.path());

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
    release(&fifo);
}

#[cfg(unix)]
#[test]
fn test_an_abandoned_version_read_does_not_hold_runtime_shutdown() {
    // Dropping a runtime waits for the tasks still running on its blocking
    // pool, so a read abandoned there would move the hang from `spawn()` to
    // the end of `#[tokio::main]`.
    let dir = tempfile::tempdir().unwrap();
    let (binary, fifo) = stalled_wheel_install(dir.path());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let version = runtime.block_on(harness_version_bounded(&binary, Duration::from_millis(300)));
    assert_eq!(version, None);

    let (dropped, has_dropped) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(runtime);
        let _ = dropped.send(());
    });
    let shut_down = has_dropped.recv_timeout(Duration::from_secs(5));
    // Release the read whatever happened, so a failure here does not hang.
    release(&fifo);
    assert!(
        shut_down.is_ok(),
        "dropping the runtime waited on the abandoned version read"
    );
}

/// A wheel install whose METADATA is a FIFO with no writer: reading the
/// version blocks, as a stalled network mount would. Returns the binary
/// and the FIFO.
#[cfg(unix)]
fn stalled_wheel_install(dir: &Path) -> (PathBuf, PathBuf) {
    let site = dir.join("site-packages");
    let binary = wheel_install(&site, &[]);
    let dist_info = site.join("google_antigravity-0.1.23.dist-info");
    std::fs::create_dir_all(&dist_info).unwrap();
    let fifo = dist_info.join("METADATA");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo failed");
    (binary, fifo)
}

/// Lets a read blocked on [`stalled_wheel_install`]'s FIFO finish (it
/// reads end of file), so the thread doing it ends.
#[cfg(unix)]
fn release(fifo: &Path) {
    drop(std::fs::OpenOptions::new().write(true).open(fifo).unwrap());
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
    assert_eq!(
        harness_version(HARNESS_FILE_NAMES, &binary).as_deref(),
        Some("0.1.23")
    );
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
    assert_eq!(
        harness_version(HARNESS_FILE_NAMES, &link).as_deref(),
        Some("0.1.18")
    );
}

#[test]
fn test_harness_version_is_none_outside_a_wheel_or_when_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    // A hand-placed binary: no wheel layout around it.
    let bare = dir.path().join("bin/localharness");
    touch_executable(&bare);
    assert_eq!(harness_version(HARNESS_FILE_NAMES, &bare), None);
    // The layout, but no metadata.
    let unlabelled = wheel_install(&dir.path().join("a"), &[]);
    assert_eq!(harness_version(HARNESS_FILE_NAMES, &unlabelled), None);
    // Two installed versions: there is no telling which one this is.
    let ambiguous = wheel_install(&dir.path().join("b"), &["0.1.18", "0.1.23"]);
    assert_eq!(harness_version(HARNESS_FILE_NAMES, &ambiguous), None);
}

#[test]
fn test_harness_version_reads_the_windows_wheel_with_the_exe() {
    // The win32 wheel ships `localharness.exe` and no bare name.
    let dir = tempfile::tempdir().unwrap();
    let bare = wheel_install(&dir.path().join("site-packages"), &["0.1.23"]);
    let exe = bare.with_file_name("localharness.exe");
    std::fs::rename(&bare, &exe).unwrap();
    assert_eq!(
        harness_version(harness_file_names(true), &exe).as_deref(),
        Some("0.1.23")
    );
    // Off Windows the `.exe` is not the harness, so this is no wheel install.
    assert_eq!(harness_version(harness_file_names(false), &exe), None);
}

#[test]
fn test_version_mismatch_notes_only_a_known_different_version() {
    assert_eq!(version_mismatch(None), None);
    assert_eq!(version_mismatch(Some(SUPPORTED_HARNESS_VERSION)), None);
    let note = version_mismatch(Some("0.1.18")).unwrap();
    assert!(note.contains("0.1.18") && note.contains(SUPPORTED_HARNESS_VERSION));

    // An init failure carries the note; other errors are untouched.
    let init = AntigravityError::init_failed("unknown field \"runWorkflow\"", String::new())
        .with_version_note(Some("0.1.18"));
    let AntigravityError::InitFailed { message, .. } = init else {
        panic!("variant must be kept");
    };
    assert!(message.starts_with("unknown field") && message.contains("0.1.18"));
    let other = AntigravityError::Config("x".to_string()).with_version_note(Some("0.1.18"));
    assert!(matches!(other, AntigravityError::Config(m) if m == "x"));
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
    let site = dir.path().join("site");
    std::fs::create_dir_all(&site).unwrap();
    let err = discover_in(
        HARNESS_FILE_NAMES,
        None,
        None,
        [vec![site.clone()]],
        Some(std::ffi::OsStr::new("/definitely/not/a/dir")),
    )
    .unwrap_err();
    let AntigravityError::HarnessNotFound { searched } = &err else {
        panic!("expected HarnessNotFound, got {err:?}");
    };
    assert!(searched.iter().any(|s| s.contains(HARNESS_PATH_ENV)));
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

/// Runs `HarnessProcess::spawn` on `binary`, which must fail.
#[cfg(unix)]
async fn failed_handshake(binary: &Path) -> AntigravityError {
    let wire = WireContext::new(Vec::new());
    match HarnessProcess::spawn(binary, &InputConfig::default(), false, &wire).await {
        Ok(_) => panic!("{} completed the handshake", binary.display()),
        Err(e) => e,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_a_failed_handshake_keeps_its_cause_as_the_source() {
    use std::error::Error as _;
    let dir = tempfile::tempdir().unwrap();
    // A file that cannot run, a harness that exits before replying, and one
    // whose reply is a frame that does not decode (field 2 claims 16 bytes
    // of the 0 that follow).
    let unrunnable = dir.path().join("unrunnable/localharness");
    std::fs::create_dir_all(unrunnable.parent().unwrap()).unwrap();
    std::fs::write(&unrunnable, b"not a binary").unwrap();
    let exits = stub_named_interpreter(
        &dir.path().join("exits"),
        "localharness",
        "echo 'stand-in: unknown flag' >&2; exit 2",
    );
    let garbled = stub_named_interpreter(
        &dir.path().join("garbled"),
        "localharness",
        r"echo 'stand-in: garbled' >&2; printf '\002\000\000\000\022\020'; exec cat >/dev/null",
    );
    for (binary, step, stderr_line) in [
        (&unrunnable, "failed to spawn harness at", None),
        (
            &exits,
            "stdio handshake I/O failed",
            Some("stand-in: unknown flag"),
        ),
        (
            &garbled,
            "decoding the handshake reply failed",
            Some("stand-in: garbled"),
        ),
    ] {
        let error = failed_handshake(binary).await;
        let AntigravityError::HandshakeFailed {
            message, stderr, ..
        } = &error
        else {
            panic!("expected HandshakeFailed: {error:?}");
        };
        assert!(message.starts_with(step), "{message}");
        if let Some(line) = stderr_line {
            assert!(stderr.contains(line), "{step}: {stderr}");
        }
        let source = error.source().expect("the cause is the source");
        assert!(
            !error.to_string().contains(&source.to_string()),
            "Display repeats the source: {error}"
        );
        // An I/O step's source is the I/O error itself.
        let io = source.downcast_ref::<std::io::Error>();
        assert_eq!(io.is_some(), !step.starts_with("decoding"), "{source:?}");
        if step.starts_with("failed to spawn") {
            assert_eq!(
                io.map(std::io::Error::kind),
                Some(std::io::ErrorKind::PermissionDenied)
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn test_a_handshake_with_no_reply_fails_at_its_bound_without_a_source() {
    use std::error::Error as _;
    let dir = tempfile::tempdir().unwrap();
    // No `exec`: the shell must keep holding the stdout pipe. `exec cat
    // >/dev/null` gives the pipe's one write end over to /dev/null, so the
    // read ends at once (an I/O error, not a missing reply), and on this
    // paused clock which comes first was a race with the real process.
    let silent = stub_named_interpreter(dir.path(), "localharness", "cat >/dev/null");
    let started = tokio::time::Instant::now();
    let error = failed_handshake(&silent).await;
    let elapsed = started.elapsed();
    let AntigravityError::HandshakeFailed { message, .. } = &error else {
        panic!("expected HandshakeFailed: {error:?}");
    };
    assert!(message.contains("no handshake reply"), "{message}");
    assert!(error.source().is_none(), "{error:?}");
    // The version read and the handshake each get 10 s; the kill waits 1 s.
    assert!(
        elapsed >= HANDSHAKE_TIMEOUT
            && elapsed < DISCOVERY_TIMEOUT + HANDSHAKE_TIMEOUT + KILL_GRACE * 2,
        "{elapsed:?}"
    );
}
