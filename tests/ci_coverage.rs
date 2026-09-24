//! Guards the link between live tests and the CI integration matrix.
//!
//! `rust.yml`'s `test-integration` job runs `--run-ignored all` per binary
//! listed in its matrix groups, and `release.yml` is the only other place
//! live tests run — after the tag exists. A live test binary left out of the
//! matrix therefore goes unexercised on every PR, which is how
//! `streaming_resume_tests` and `tool_service_tests` sat unrun.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Runs in the `test-antigravity` job's own step, not the matrix.
const OUTSIDE_MATRIX: &[&str] = &["antigravity_harness"];

/// The one ignore reason for a live test (CLAUDE.md, "Test conventions").
const LIVE_REASON: &str = "Requires API key";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `(binary name, source)` for every top-level test file.
fn test_sources() -> Vec<(String, String)> {
    let dir = root().join("tests");
    let mut sources: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).expect("read test source");
            (name, text)
        })
        .collect();
    sources.sort();
    assert!(
        !sources.is_empty(),
        "found no test sources in {}",
        dir.display()
    );
    sources
}

/// Binaries named in the matrix's `tests: "…"` lines.
fn matrix_binaries() -> Vec<String> {
    let workflow =
        std::fs::read_to_string(root().join(".github/workflows/rust.yml")).expect("read rust.yml");
    let job = workflow
        .split("\n  test-integration:")
        .nth(1)
        .expect("rust.yml has no test-integration job");
    let job = job.split("\n  fmt:").next().unwrap();
    let binaries: Vec<String> = job
        .lines()
        .filter_map(|line| line.trim().strip_prefix("tests: \""))
        .flat_map(|list| list.trim_end_matches('"').split_whitespace())
        .map(str::to_string)
        .collect();
    assert!(
        !binaries.is_empty(),
        "no matrix groups found in test-integration"
    );
    binaries
}

fn ignore_reasons(source: &str) -> impl Iterator<Item = &str> {
    source.lines().filter_map(|line| {
        line.trim()
            .strip_prefix("#[ignore = \"")
            .and_then(|rest| rest.strip_suffix("\"]"))
    })
}

#[test]
fn every_live_test_binary_is_in_the_ci_matrix() {
    let matrix = matrix_binaries();
    let unique: BTreeSet<&String> = matrix.iter().collect();
    assert_eq!(
        unique.len(),
        matrix.len(),
        "a binary is listed twice: {matrix:?}"
    );

    let sources = test_sources();
    let names: BTreeSet<&str> = sources.iter().map(|(name, _)| name.as_str()).collect();
    for binary in &matrix {
        assert!(
            names.contains(binary.as_str()),
            "the matrix lists {binary}, but tests/{binary}.rs does not exist"
        );
    }

    let missing: Vec<&str> = sources
        .iter()
        .filter(|(name, text)| {
            !OUTSIDE_MATRIX.contains(&name.as_str())
                && ignore_reasons(text).next().is_some()
                && !matrix.contains(name)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "these binaries have live tests but no test-integration matrix group: {missing:?}"
    );
}

#[test]
fn live_tests_use_the_standard_ignore_reason() {
    let mut offenders = Vec::new();
    for (name, text) in test_sources() {
        if OUTSIDE_MATRIX.contains(&name.as_str()) {
            continue;
        }
        for reason in ignore_reasons(&text) {
            if reason != LIVE_REASON {
                offenders.push(format!("tests/{name}.rs: {reason:?}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "use #[ignore = \"{LIVE_REASON}\"] for live tests:\n  {}",
        offenders.join("\n  ")
    );
}
