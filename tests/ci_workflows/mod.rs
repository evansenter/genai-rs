//! Guards on the CI workflows themselves, for `tests/ci_coverage.rs` (#501).
//!
//! The workflows are YAML that only runs in CI, so a regression in how they
//! run tests surfaces late: a fail-fast run hides how far a break reaches, a
//! beta toolchain rewords the diagnostics trybuild pins, and a scheduled
//! workflow that fails without escalating is noticed only by whoever last
//! committed it. These tests read the files as text (the crate has no YAML
//! dependency); the jobs are the two-space-indented keys under `jobs:`.
//!
//! Not a test target itself: Cargo only builds `tests/*.rs` and
//! `tests/*/main.rs`.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Line endings normalized: a Windows checkout has CRLF (autocrlf).
fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// `(file name, text)` for every workflow in `.github/workflows/`.
fn workflows() -> Vec<(String, String)> {
    let dir = root().join(".github/workflows");
    let mut found: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, read(&path))
        })
        .collect();
    found.sort();
    assert!(found.len() > 5, "found too few workflows: {found:?}");
    found
}

/// `(job id, job text)` for every job: the two-space-indented keys under
/// `jobs:`, each up to the next one.
fn jobs(workflow: &str) -> Vec<(String, String)> {
    let Some(body) = workflow.split("\njobs:\n").nth(1) else {
        return Vec::new();
    };
    let mut jobs: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        let is_key = line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':');
        if is_key {
            let id = line.trim().trim_end_matches(':').to_string();
            jobs.push((id, String::new()));
        } else if let Some((_, text)) = jobs.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    jobs
}

/// The shell lines of a job that run a command, comments dropped.
fn command_lines(job: &str) -> impl Iterator<Item = &str> {
    job.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
}

/// Whether the workflow's `on:` block has a `schedule:` trigger.
fn is_scheduled(workflow: &str) -> bool {
    let on = workflow
        .split("\non:\n")
        .nth(1)
        .map(|rest| rest.split("\njobs:\n").next().unwrap_or(rest))
        .unwrap_or("");
    on.lines().any(|line| line.trim() == "schedule:")
}

/// A red scheduled run otherwise only emails whoever last committed the
/// workflow file (#431), so every scheduled workflow opens a
/// `ci-health-escalation` issue on failure and closes it on recovery,
/// through the shared scripts.
#[test]
fn every_scheduled_workflow_escalates_failures() {
    let mut scheduled = 0;
    let mut missing = Vec::new();
    for (name, text) in workflows() {
        if !is_scheduled(&text) {
            continue;
        }
        scheduled += 1;
        for script in [
            "report_scheduled_failure.sh",
            "resolve_scheduled_failure.sh",
        ] {
            let calls = command_lines(&text).any(|line| line.contains(script));
            if !calls {
                missing.push(format!("{name}: never calls {script}"));
            }
        }
    }
    assert!(
        scheduled >= 6,
        "found only {scheduled} scheduled workflows; is the `on:` parsing broken?"
    );
    assert!(
        missing.is_empty(),
        "Scheduled workflows must escalate a failure and resolve on recovery \
         (see audit.yml):\n  {}",
        missing.join("\n  ")
    );
}

/// Fail-fast stops a run at the first failure and hides how far one break
/// reaches (the Windows CRLF break, #486). `.config/nextest.toml` turns it
/// off for the default profile, which every CI run uses: no workflow picks
/// another profile or turns it back on.
#[test]
fn nextest_runs_do_not_fail_fast() {
    let config = read(&root().join(".config/nextest.toml"));
    let mut section = "";
    let mut default_off = false;
    let mut problems = Vec::new();
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "fail-fast" {
            continue;
        }
        match (section, value.trim()) {
            ("[profile.default]", "false") => default_off = true,
            (section, value) => problems.push(format!(
                ".config/nextest.toml: fail-fast = {value} in {section}"
            )),
        }
    }
    if !default_off {
        problems.push(".config/nextest.toml: no `fail-fast = false` in [profile.default]".into());
    }

    let mut runs = 0;
    for (name, text) in workflows() {
        for line in command_lines(&text).filter(|line| line.contains("cargo nextest run")) {
            runs += 1;
            let words: Vec<&str> = line.split_whitespace().collect();
            let other_profile = words
                .windows(2)
                .any(|w| (w[0] == "--profile" || w[0] == "-P") && w[1] != "default")
                || words
                    .iter()
                    .any(|w| w.starts_with("--profile=") && *w != "--profile=default");
            if words.contains(&"--fail-fast") || other_profile {
                problems.push(format!("{name}: {line}"));
            }
        }
    }
    assert!(runs > 5, "found only {runs} nextest runs in the workflows");
    assert!(
        problems.is_empty(),
        "CI nextest runs must not fail fast:\n  {}",
        problems.join("\n  ")
    );
}

/// trybuild's `ui_tests` compare rustc's exact diagnostics with `.stderr`
/// snapshots, and the beta toolchain rewords them. A job that runs a beta
/// toolchain therefore filters every nextest run through a
/// `NEXTEST_FILTER` that drops `ui_tests` on beta, and runs `cargo test`
/// only for doctests.
#[test]
fn ui_tests_do_not_run_on_beta() {
    let mut beta_jobs = 0;
    let mut problems = Vec::new();
    for (name, text) in workflows() {
        for (job, body) in jobs(&text) {
            let on_beta = body.lines().any(|line| {
                let line = line.trim();
                (line.starts_with("rust:") || line.starts_with("toolchain:"))
                    && line.contains("beta")
            }) || body.contains("rust-toolchain@beta");
            if !on_beta {
                continue;
            }
            beta_jobs += 1;
            let drops_ui_tests = body.lines().any(|line| {
                let line = line.trim();
                line.starts_with("NEXTEST_FILTER:")
                    && line.contains("== 'beta'")
                    && line.contains("'not binary(ui_tests)'")
            });
            if !drops_ui_tests {
                problems.push(format!(
                    "{name} {job}: no NEXTEST_FILTER that drops binary(ui_tests) on beta"
                ));
            }
            for line in command_lines(&body) {
                if line.contains("cargo nextest run") && !line.contains("-E \"$NEXTEST_FILTER\"") {
                    problems.push(format!("{name} {job}: unfiltered `{line}`"));
                }
                if line.contains("cargo test") && !line.contains("--doc") {
                    problems.push(format!("{name} {job}: `{line}` runs more than doctests"));
                }
            }
        }
    }
    assert!(
        beta_jobs > 0,
        "no job runs a beta toolchain; is the matrix parsing broken?"
    );
    assert!(
        problems.is_empty(),
        "ui_tests must not run on beta:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_job_parser_splits_jobs_and_finds_schedules() {
    let workflow = "name: X\n\non:\n  push:\n  schedule:\n    - cron: '1 2 3 4 5'\n\njobs:\n  first:\n    runs-on: x\n    steps:\n      - run: a\n  second:\n    steps:\n      - run: b\n";
    let found = jobs(workflow);
    let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["first", "second"]);
    assert!(found[0].1.contains("run: a") && !found[0].1.contains("run: b"));
    assert!(is_scheduled(workflow));
    assert!(!is_scheduled(&workflow.replace("  schedule:\n", "")));
}
