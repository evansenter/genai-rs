//! Guards on the CI workflows themselves, for `tests/ci_coverage.rs` (#501).
//!
//! `flakiness` ties the flakiness report to what it reads (#504):
//! `RETRY_JOBS` names exactly the `rust.yml` jobs that retry, both example
//! loops print `EXAMPLE_RETRIED`, the report publishes before it fails and
//! escalates, and a stub test with a millisecond timeout runs on the paused
//! clock. Every live nextest step keeps its skip markers in the log and
//! fails over a budget of them. `coverage` checks that the Code Coverage
//! job writes its summary, over the packages it measured, on every run,
//! measures the `antigravity` feature with its scripted suite, fails under
//! a floor, and that a Codecov upload fails the job on an error. `release`
//! checks that a release publishes only a commit whose main push run of
//! `rust.yml` is green on every job.
//!
//! The workflows are YAML that only runs in CI, so a regression in how they
//! run tests surfaces late: a fail-fast run hides how far a break reaches, a
//! beta toolchain rewords the diagnostics trybuild pins, and a scheduled
//! workflow or a push to main that fails without escalating is noticed only
//! by whoever last committed it (`escalation`). What CI builds beyond Linux
//! and all features is in `builds`. These tests read the files as text (the
//! crate has no YAML dependency); the jobs are the two-space-indented keys
//! under `jobs:`.
//!
//! Not a test target itself: Cargo only builds `tests/*.rs` and
//! `tests/*/main.rs`.

mod builds;
mod escalation;

use std::path::{Path, PathBuf};

mod coverage;
mod flakiness;
mod release;

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
/// `NEXTEST_FILTER` that drops `ui_tests` on beta, unless the run is the
/// library's own unit tests (`--lib`), which cannot reach `ui_tests`, and
/// runs `cargo test` only for doctests.
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
                if line.contains("cargo nextest run")
                    && !line.contains("-E \"$NEXTEST_FILTER\"")
                    && !line.split_whitespace().any(|word| word == "--lib")
                {
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

/// The steps of a job: each `- ` item under its `steps:` key, with the
/// lines indented below it (its `run:` script and `env:` block).
fn steps(job: &str) -> Vec<String> {
    let Some(body) = job.split("\n    steps:\n").nth(1) else {
        return Vec::new();
    };
    let mut steps: Vec<String> = Vec::new();
    for line in body.lines() {
        let indent = line.len() - line.trim_start().len();
        if line.starts_with("      - ") {
            steps.push(String::new());
        } else if !line.trim().is_empty() && indent < 6 {
            // A key back at the job's own level ends its steps.
            break;
        } else if indent == 6 && line.trim_start().starts_with('#') {
            // A comment between steps belongs to neither.
            continue;
        }
        if let Some(step) = steps.last_mut() {
            step.push_str(line);
            step.push('\n');
        }
    }
    steps
}

/// What would let `step` fail without failing its job, each named: a
/// step-level `if:`, which can skip it, or `continue-on-error` on the step
/// or on `job`, which ignores its failure (`false` is the default, and
/// allowed). Empty when the step runs on every run and its failure fails
/// the job.
fn failure_escapes(job: &str, step: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (i, line) in step.lines().enumerate() {
        // The step's own keys sit at 8 spaces, the first one after `- `.
        let key = if i == 0 {
            line.strip_prefix("      - ")
        } else {
            line.strip_prefix("        ")
                .filter(|rest| !rest.starts_with(' '))
        };
        let Some(key) = key else {
            continue;
        };
        if key.starts_with("if:") {
            found.push(format!("a step-level `{key}`, which can skip it"));
        }
        if let Some(value) = key.strip_prefix("continue-on-error:")
            && value.trim() != "false"
        {
            found.push(format!("`{key}` on the step, which ignores its failure"));
        }
    }
    for line in job.lines() {
        if let Some(value) = line.strip_prefix("    continue-on-error:")
            && value.trim() != "false"
        {
            found.push(format!(
                "`continue-on-error:{value}` on its job, which ignores its failure"
            ));
        }
    }
    found
}

/// A step that runs nextest against the live API: it runs `nextest run`
/// with the repository's key in its env (the scripted harness step blanks
/// the key, and the unit-test steps have none).
fn is_live_nextest_step(step: &str) -> bool {
    command_lines(step).any(|line| line.contains("nextest run"))
        && step.contains("GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}")
}

/// The flakiness report counts a step's tests only when the step retries,
/// which `count_retried_flakes.sh` reads from the step header GitHub
/// echoes into the log. Retries are set as `NEXTEST_RETRIES` in the step's
/// env, which the header always lists, never as `--retries`, so the
/// command's layout cannot hide them; and every live nextest step sets
/// them, so the jobs the report reads keep being counted (#504).
#[test]
fn nextest_retries_are_set_in_the_step_env() {
    let mut live = Vec::new();
    let mut problems = Vec::new();
    for (name, text) in workflows() {
        for line in command_lines(&text) {
            if line.contains("--retries") && !line.starts_with("NEXTEST_RETRIES") {
                problems.push(format!("{name}: `{line}` (set NEXTEST_RETRIES instead)"));
            }
        }
        for (job, body) in jobs(&text) {
            for step in steps(&body)
                .iter()
                .filter(|step| is_live_nextest_step(step))
            {
                let title = step.lines().next().unwrap_or("").trim().to_string();
                if !command_lines(step).any(|line| line.starts_with("NEXTEST_RETRIES:")) {
                    problems.push(format!(
                        "{name} job {job}, step `{title}`: a live nextest step with no \
                         NEXTEST_RETRIES in its env"
                    ));
                }
                live.push(format!("{name}:{job}"));
            }
        }
    }
    // rust.yml's integration groups and harness step, and release.yml's two
    // live steps: the parser must still find them.
    for expected in [
        "rust.yml:test-integration",
        "rust.yml:test-antigravity",
        "release.yml:validate",
    ] {
        assert!(
            live.iter().any(|found| found == expected),
            "no live nextest step found in {expected}; found {live:?}"
        );
    }
    assert!(
        live.len() >= 4,
        "found only {} live nextest steps: {live:?}",
        live.len()
    );
    assert!(
        problems.is_empty(),
        "nextest retries must be set through NEXTEST_RETRIES in each live step's env:\n  {}",
        problems.join("\n  ")
    );
}

/// A live test that passes without verifying prints a skip marker
/// (`docs/TESTING.md`, "Skip markers"). Every live nextest step keeps
/// passing tests' output (`--success-output=final`), where the markers
/// are, and fails over a budget of them: the Antigravity harness step once
/// had neither, so its skips were invisible (#504).
#[test]
fn live_nextest_steps_keep_and_budget_skip_markers() {
    let mut live = 0;
    let mut problems = Vec::new();
    for (name, text) in workflows() {
        for (job, body) in jobs(&text) {
            for step in steps(&body)
                .iter()
                .filter(|step| is_live_nextest_step(step))
            {
                live += 1;
                let title = step.lines().next().unwrap_or("").trim().to_string();
                let lines: Vec<&str> = command_lines(step).collect();
                let keeps_output = lines.iter().any(|line| {
                    line.contains("nextest run") && line.contains("--success-output=final")
                });
                let counts = lines.iter().any(|line| {
                    line.contains("grep -cE")
                        && line.contains("SEMANTIC_VALIDATION_SKIPPED")
                        && line.contains("LIVE_TOOL_EVIDENCE_SKIPPED")
                });
                let budgets = lines
                    .iter()
                    .any(|line| line.contains("\"${skips:-0}\" -gt") && !line.contains("-gt 0"));
                for (ok, what) in [
                    (keeps_output, "no --success-output=final on its nextest run"),
                    (counts, "does not count both skip markers"),
                    (budgets, "has no budget over which the markers fail it"),
                ] {
                    if !ok {
                        problems.push(format!("{name} job {job}, step `{title}`: {what}"));
                    }
                }
            }
        }
    }
    assert!(live >= 4, "found only {live} live nextest steps");
    assert!(
        problems.is_empty(),
        "live nextest steps must keep and budget their skip markers:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_failure_escape_check_reads_the_step_and_its_job() {
    let job = "    runs-on: x\n    steps:\n";
    let step = "      - name: Floor\n        run: check\n        env:\n          if: not-a-key\n";
    assert!(failure_escapes(job, step).is_empty());
    for broken in [
        "      - name: Floor\n        if: always()\n        run: check\n",
        "      - if: github.event_name == 'push'\n        run: check\n",
        "      - name: Floor\n        continue-on-error: true\n        run: check\n",
        "      - name: Floor\n        continue-on-error: ${{ matrix.ok }}\n        run: check\n",
    ] {
        assert!(!failure_escapes(job, broken).is_empty(), "{broken}");
    }
    assert!(
        failure_escapes(
            job,
            "      - name: Floor\n        continue-on-error: false\n        run: check\n"
        )
        .is_empty()
    );
    let lenient = "    runs-on: x\n    continue-on-error: true\n    steps:\n";
    assert!(!failure_escapes(lenient, step).is_empty());
}

#[test]
fn the_step_parser_finds_live_steps_and_their_env() {
    let job = "    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n      # The live step.\n      - name: Unit\n        run: cargo nextest run --workspace\n      - name: Live\n        run: |\n          cargo nextest run \\\n            --run-ignored all\n        env:\n          GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}\n          NEXTEST_RETRIES: 2\n      - name: Scripted\n        run: cargo nextest run -E 'binary(x)'\n        env:\n          GEMINI_API_KEY: \"\"\n";
    let found = steps(job);
    assert_eq!(found.len(), 4, "{found:?}");
    assert!(found[2].contains("NEXTEST_RETRIES: 2") && !found[2].contains("Scripted"));
    let live: Vec<bool> = found.iter().map(|s| is_live_nextest_step(s)).collect();
    assert_eq!(live, [false, false, true, false]);
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
