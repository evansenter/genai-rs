//! Guards on the CI workflows themselves, for `tests/ci_coverage.rs` (#501).
//!
//! `flakiness` ties the flakiness report to what it reads (#504):
//! `RETRY_JOBS` names exactly the `rust.yml` jobs that retry, both example
//! loops print `EXAMPLE_RETRIED`, every in-test retry prints
//! `TRANSIENT_RETRIED`, the report publishes before it fails and
//! escalates, and a stub test with a millisecond timeout runs on the paused
//! clock. Every live nextest step keeps its skip markers in the log and
//! fails its step over a budget of them, each budget pinned with its reason
//! (`SKIP_BUDGETS`). `coverage` checks that the Code Coverage job writes its
//! summary, over the packages it measured, on every run, measures the
//! `antigravity` feature with its scripted suite and library files alone,
//! fails under a floor, and that a Codecov upload fails the job on an
//! error. `release` checks that a release publishes only a commit whose
//! main push run of `rust.yml` is green on every job but the ones a push
//! skips by design, with every Cross Platform leg. `sweep` requires
//! `api-surface-sweep`'s spec-watch and issue-pointer steps.
//!
//! The steps whose failure must fail their job (the coverage floor and the
//! runs before it, the release gate, every live nextest step, the scripted
//! harness step, the sweep's two) are read by `escapes`: no `if:` or
//! `needs:` but the ones a caller allows, no `continue-on-error`, on the
//! step or its job, no `defaults:` on the job or the workflow, no other
//! shell, and no shell-level mask that `shell::shell_masks` knows (`|| true`,
//! `; true`, `set +e`, an `exit` whose status may be 0 and the others it
//! lists; a script of more than one line starts with `set -euo pipefail`,
//! and every step that pipes nextest does). It reads the script as text,
//! so a status computed out of its sight could still pass it (D-058).
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

use regex::Regex;

mod coverage;
mod flakiness;
mod release;
mod shell;
mod sweep;

use shell::{
    SKIP_COUNT, blank_quotes, exits_with_rc, failed_list_exits, fails_the_step, if_branch,
    lists_failed_rc, pipes_nextest, run_script, shell_masks, turns_checks_off,
};

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

/// The step's own key on line `i` of its text, if the line holds one: the
/// first line's after `- `, the others' at eight spaces.
fn step_key(i: usize, line: &str) -> Option<&str> {
    if i == 0 {
        line.strip_prefix("      - ")
    } else {
        line.strip_prefix("        ")
            .filter(|rest| !rest.starts_with(' '))
    }
}

/// The step's `name:`, or its first line when it has none.
fn step_name(step: &str) -> &str {
    let first = step.lines().next().unwrap_or("").trim();
    first.strip_prefix("- name: ").unwrap_or(first)
}

/// What in `workflow` itself can change how every one of its bash steps
/// runs, each named: a workflow-level `defaults:`, whose shell template
/// (`true {0}`) can run no script at all, and `BASH_ENV`, `SHELLOPTS` or
/// `BASHOPTS`, which bash reads before a step's script.
fn workflow_escapes(workflow: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in workflow.lines() {
        if line.starts_with("defaults:") {
            found.push(
                "a workflow-level `defaults:`, which can change the shell every step runs \
                 under"
                    .into(),
            );
        }
        let names_shell_env = ["BASH_ENV", "SHELLOPTS", "BASHOPTS"]
            .iter()
            .any(|name| line.contains(name));
        if names_shell_env && !line.trim_start().starts_with('#') {
            found.push(format!(
                "`{}` in the workflow, which can change how every bash step runs",
                line.trim()
            ));
        }
    }
    found
}

/// What would let `step` fail without failing its job, each named, empty
/// when the step runs on every run and its failure fails the job. On the
/// step or its job: an `if:`, which can skip it, or a `continue-on-error`
/// other than `false` (the default), which ignores its failure. On the
/// job: a `needs:`, which skips the job with a job it needs, or a
/// `defaults:`; on the step, a `shell:` other than `bash`; on the workflow,
/// what [`workflow_escapes`] names: each can run its script under other
/// rules. In its script: a shell-level mask ([`shell_masks`]). `allowed`
/// lists the `if:` and `needs:` lines (`key: value`, as written) a caller
/// accepts, such as the live steps' owner gate.
fn escapes(workflow: &str, job: &str, step: &str, allowed: &[&str]) -> Vec<String> {
    let mut found = workflow_escapes(workflow);
    for (i, line) in step.lines().enumerate() {
        let Some(key) = step_key(i, line).map(str::trim_end) else {
            continue;
        };
        if key.starts_with("if:") && !allowed.contains(&key) {
            found.push(format!("a step-level `{key}`, which can skip it"));
        }
        if let Some(value) = key.strip_prefix("continue-on-error:")
            && value.trim() != "false"
        {
            found.push(format!("`{key}` on the step, which ignores its failure"));
        }
        if let Some(shell) = key.strip_prefix("shell:")
            && shell.trim() != "bash"
        {
            found.push(format!(
                "`{key}` on the step, a shell other than bash, which can run its script \
                 under other rules"
            ));
        }
    }
    for line in job.lines() {
        // The job's own keys sit at 4 spaces.
        let Some(key) = line
            .strip_prefix("    ")
            .filter(|rest| !rest.starts_with(' '))
            .map(str::trim_end)
        else {
            continue;
        };
        if key.starts_with("if:") && !allowed.contains(&key) {
            found.push(format!(
                "a job-level `{key}`, which can skip the job and the step with it"
            ));
        }
        if let Some(value) = key.strip_prefix("continue-on-error:")
            && value.trim() != "false"
        {
            found.push(format!("`{key}` on its job, which ignores its failure"));
        }
        if key.starts_with("needs:") && !allowed.contains(&key) {
            found.push(format!(
                "a job-level `{key}`, which skips the job when a job it needs fails or is \
                 skipped"
            ));
        }
        if key.starts_with("defaults:") {
            found.push(
                "a job-level `defaults:`, which can change the shell its steps run under".into(),
            );
        }
    }
    found.extend(shell_masks(step));
    found
}

/// [`escapes`] with nothing allowed: what would let a step that must run on
/// every run fail without failing its job.
fn failure_escapes(workflow: &str, job: &str, step: &str) -> Vec<String> {
    escapes(workflow, job, step, &[])
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

/// A live nextest step's skip budget: the number of skip markers past
/// which it fails.
struct SkipBudget {
    workflow: &'static str,
    job: &'static str,
    step: &'static str,
    budget: u32,
    reason: &'static str,
}

/// Every live nextest step's skip budget, pinned with its reason, so a
/// budget raised to quiet a noisy run fails here first, where the reason it
/// was set can be weighed against the new one (D-058).
const SKIP_BUDGETS: [SkipBudget; 4] = [
    SkipBudget {
        workflow: "rust.yml",
        job: "test-integration",
        step: "Run integration tests (${{ matrix.group }})",
        budget: 3,
        reason: "per binary: past 3 unverified assertions a green run of one binary \
                 cannot be trusted, both marker kinds counted together",
    },
    SkipBudget {
        workflow: "rust.yml",
        job: "test-antigravity",
        step: "Run harness integration tests",
        budget: 3,
        reason: "one binary, as each integration binary is budgeted",
    },
    SkipBudget {
        workflow: "release.yml",
        job: "validate",
        step: "Run harness integration tests",
        budget: 3,
        reason: "the same binary and budget as rust.yml's harness step",
    },
    SkipBudget {
        workflow: "release.yml",
        job: "validate",
        step: "Run integration tests",
        budget: 10,
        reason: "one whole-workspace run of about 50 markers, so 10 tolerates the \
                 roughly 20% rust.yml's 3 per binary does",
    },
];

/// The line that opens a step's over-budget branch, before its budget.
const BUDGET_LINE: &str = "if [ \"${skips:-0}\" -gt ";

/// Problems with the skip markers of every live nextest step in
/// `workflows`, and how many such steps there are. Each step keeps passing
/// tests' output (`--success-output=final`), where the markers are; counts
/// both kinds once ([`SKIP_COUNT`]), in the log its nextest run tees, with
/// no other value given `skips`; and over its budget, a number written as
/// [`SKIP_BUDGETS`] pins it (`if [ "${skips:-0}" -gt <N> ]; then`), fails:
/// its over-budget branch exits 1 or sets `rc=1`, and a step that sets
/// `rc` exits with it as its last line, or lists its binary as failed in a
/// list that, when not empty, exits 1 at the script's end.
fn skip_budget_problems(workflows: &[(String, String)]) -> (usize, Vec<String>) {
    let mut live = 0;
    let mut problems = Vec::new();
    let mut pinned_found = [false; SKIP_BUDGETS.len()];
    let skip_count = Regex::new(SKIP_COUNT).expect("the skip-count pattern");
    for (name, text) in workflows {
        for (job, body) in jobs(text) {
            for step in steps(&body)
                .iter()
                .filter(|step| is_live_nextest_step(step))
            {
                live += 1;
                let title = step.lines().next().unwrap_or("").trim().to_string();
                let script = run_script(step);
                let lines: Vec<&str> = script.iter().map(String::as_str).collect();
                let budget = lines.iter().position(|line| line.starts_with(BUDGET_LINE));
                // A bare number, or the guard cannot hold it to its pin.
                let set_to: Option<u32> = budget.and_then(|at| {
                    let number = lines[at]
                        .strip_prefix(BUDGET_LINE)?
                        .strip_suffix(" ]; then")?;
                    number
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                        .then(|| number.parse().ok())
                        .flatten()
                });
                if let Some(at) = budget
                    && set_to.is_none()
                {
                    problems.push(format!(
                        "{name} job {job}, step `{title}`: a budget the guard cannot read \
                         as a number (`{}`, not `{BUDGET_LINE}<N> ]; then`)",
                        lines[at]
                    ));
                }
                let pinned = SKIP_BUDGETS.iter().position(|pin| {
                    pin.workflow == name.as_str()
                        && pin.job == job.as_str()
                        && pin.step == step_name(step)
                });
                match pinned {
                    None => problems.push(format!(
                        "{name} job {job}, step `{title}`: no budget pinned in SKIP_BUDGETS"
                    )),
                    Some(at) => {
                        pinned_found[at] = true;
                        let pin = &SKIP_BUDGETS[at];
                        if let Some(set_to) = set_to
                            && set_to != pin.budget
                        {
                            problems.push(format!(
                                "{name} job {job}, step `{title}`: a skip budget of {set_to}, \
                                 but SKIP_BUDGETS pins {} ({}); change both, with the reason",
                                pin.budget, pin.reason
                            ));
                        }
                    }
                }
                let keeps_output = lines.iter().any(|line| {
                    line.contains("nextest run") && line.contains("--success-output=final")
                });
                let teed = lines
                    .iter()
                    .find(|line| line.contains("nextest run"))
                    .and_then(|line| line.split("| tee ").nth(1))
                    .and_then(|rest| rest.split_whitespace().next());
                let counted: Vec<(&str, &str)> = lines
                    .iter()
                    .filter_map(|line| skip_count.captures(line))
                    .filter_map(|found| Some((found.get(1)?.as_str(), found.get(2)?.as_str())))
                    .collect();
                // The count is the only value `skips` is given.
                let set_once = lines
                    .iter()
                    .filter(|line| line.starts_with("skips="))
                    .count()
                    == 1;
                let counts = set_once
                    && matches!(counted.as_slice(), [(pattern, log)]
                        if pattern.contains("SEMANTIC_VALIDATION_SKIPPED")
                            && pattern.contains("LIVE_TOOL_EVIDENCE_SKIPPED")
                            && Some(*log) == teed);
                let over = budget.map(|at| if_branch(&lines, at)).unwrap_or_default();
                let fails = over.iter().any(|line| fails_the_step(line));
                let sets_rc = over.iter().any(|line| {
                    line.split(';')
                        .any(|part| part.trim().trim_start_matches("then ") == "rc=1")
                });
                let exits_rc = exits_with_rc(&lines);
                let lists_failed = lists_failed_rc(&lines);
                // A list of failed binaries fails the step only if a
                // non-empty one exits 1.
                let failed_exits = failed_list_exits(&lines);
                for (ok, what) in [
                    (keeps_output, "no --success-output=final on its nextest run"),
                    (
                        counts,
                        "does not count both skip markers, once, in the log its nextest run tees",
                    ),
                    (
                        budget.is_some(),
                        "has no budget over which the markers fail it",
                    ),
                    (
                        budget.is_none() || fails,
                        "its over-budget branch neither exits 1 nor sets rc=1, so the step passes",
                    ),
                    (
                        !sets_rc || exits_rc || lists_failed,
                        "sets rc=1 over budget but never exits with it (`exit \"$rc\"` last)",
                    ),
                    (
                        !lists_failed || failed_exits,
                        "lists a failed binary, but a non-empty list does not exit 1, so the step passes",
                    ),
                ] {
                    if !ok {
                        problems.push(format!("{name} job {job}, step `{title}`: {what}"));
                    }
                }
            }
        }
    }
    for (pin, found) in SKIP_BUDGETS.iter().zip(pinned_found) {
        if !found {
            problems.push(format!(
                "SKIP_BUDGETS pins {} job {}, step `{}`, which is not a live nextest step",
                pin.workflow, pin.job, pin.step
            ));
        }
    }
    (live, problems)
}

/// A live test that passes without verifying prints a skip marker
/// (`docs/TESTING.md`, "Skip markers"). Every live nextest step keeps
/// passing tests' output (`--success-output=final`), where the markers
/// are, and fails over a budget of them: the Antigravity harness step once
/// had neither, so its skips were invisible (#504), and a budget whose
/// branch printed the error but left the step's exit status alone passed
/// every check here.
#[test]
fn live_nextest_steps_keep_and_budget_skip_markers() {
    let (live, problems) = skip_budget_problems(&workflows());
    assert!(live >= 4, "found only {live} live nextest steps");
    assert!(
        problems.is_empty(),
        "live nextest steps must keep and budget their skip markers:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_skip_budget_check_catches_what_it_claims_to() {
    let all = workflows();
    assert!(skip_budget_problems(&all).1.is_empty());
    let mutate = |file: &str, from: &str, to: &str| {
        let mutated: Vec<(String, String)> = all
            .iter()
            .map(|(name, text)| {
                let text = if name == file {
                    text.replacen(from, to, 1)
                } else {
                    text.clone()
                };
                (name.clone(), text)
            })
            .collect();
        let original = &all
            .iter()
            .find(|(name, _)| name == file)
            .expect("workflow")
            .1;
        let changed = &mutated
            .iter()
            .find(|(name, _)| name == file)
            .expect("workflow")
            .1;
        assert_ne!(original, changed, "{file}: `{from}` did not apply");
        mutated
    };
    // The harness step's and each integration group's over-budget branch.
    let harness_branch = "            if [ \"$rc\" -eq 0 ]; then rc=1; fi\n";
    let group_branch = "              if [ \"$rc\" -eq 0 ]; then rc=1; fi\n";
    let broken = [
        // The over-budget branch prints its error and nothing more.
        mutate("rust.yml", harness_branch, "            true\n"),
        mutate("rust.yml", group_branch, "              true\n"),
        mutate("release.yml", harness_branch, "            true\n"),
        mutate(
            "release.yml",
            "            if [ \"$rc\" -eq 0 ]; then rc=1; fi\n          elif [ \"${skips:-0}\" -gt 0 ]; then\n            echo \"::warning::$skips",
            "            true\n          elif [ \"${skips:-0}\" -gt 0 ]; then\n            echo \"::warning::$skips",
        ),
        // ... sets rc, but the step exits 0 regardless.
        mutate("rust.yml", "          exit \"$rc\"\n", "          exit 0\n"),
        mutate(
            "rust.yml",
            "            [ \"$rc\" -eq 0 ] || failed+=(\"$test\")\n",
            "            true\n",
        ),
        // ... or lists the binary as failed, but a failed list exits 0.
        mutate(
            "rust.yml",
            "            echo \"::error::failed: ${failed[*]}\"\n            exit 1\n",
            "            echo \"::error::failed: ${failed[*]}\"\n            exit 0\n",
        ),
        // No budget, or the markers' output dropped.
        mutate(
            "rust.yml",
            "if [ \"${skips:-0}\" -gt 3 ]; then",
            "if false; then",
        ),
        mutate(
            "rust.yml",
            "--no-fail-fast --success-output=final",
            "--no-fail-fast",
        ),
        // A budget raised to quiet a noisy run (M5), in either workflow.
        mutate(
            "rust.yml",
            "if [ \"${skips:-0}\" -gt 3 ]; then",
            "if [ \"${skips:-0}\" -gt 300 ]; then",
        ),
        mutate(
            "release.yml",
            "if [ \"${skips:-0}\" -gt 10 ]; then",
            "if [ \"${skips:-0}\" -gt 11 ]; then",
        ),
        // ... or written so it reads as no number, or never holds.
        mutate(
            "rust.yml",
            "if [ \"${skips:-0}\" -gt 3 ]; then",
            "if [ \"${skips:-0}\" -gt \"300\" ]; then",
        ),
        mutate(
            "release.yml",
            "if [ \"${skips:-0}\" -gt 10 ]; then",
            "if [ \"${skips:-0}\" -gt \"$((10 * 30))\" ]; then",
        ),
        mutate(
            "rust.yml",
            "if [ \"${skips:-0}\" -gt 3 ]; then",
            "if [ \"${skips:-0}\" -gt 3 -a 1 = 0 ]; then",
        ),
        // The count reset, or read from a file the run never wrote.
        mutate(
            "release.yml",
            "/tmp/integration.log || true)\n",
            "/tmp/integration.log || true)\n          skips=0\n",
        ),
        mutate(
            "release.yml",
            "LIVE_TOOL_EVIDENCE_SKIPPED' /tmp/integration.log || true)",
            "LIVE_TOOL_EVIDENCE_SKIPPED' /tmp/other.log || true)",
        ),
        // A live step renamed away from its pin.
        mutate(
            "release.yml",
            "      - name: Run integration tests\n",
            "      - name: Run the integration tests\n",
        ),
    ];
    for (i, mutated) in broken.iter().enumerate() {
        assert!(
            !skip_budget_problems(mutated).1.is_empty(),
            "mutation {i} went unnoticed"
        );
    }
    for pin in &SKIP_BUDGETS {
        assert!(
            pin.reason.split_whitespace().count() > 3,
            "{} {}: the budget's reason is the point of pinning it",
            pin.workflow,
            pin.step
        );
    }
}

/// The `if:` a live nextest step, or its job, may carry: the fork and
/// owner gate. A fork's pull request has no key, and a fork's own main no
/// secret, so the live steps run only where the key is.
const LIVE_GATE: &str = "if: (github.event_name != 'pull_request' || \
                         github.event.pull_request.head.repo.full_name == github.repository) \
                         && github.repository_owner == 'evansenter'";

/// What a live nextest step and its job may carry beyond running on every
/// run: [`LIVE_GATE`], and `needs: check`, a job with no `if:`.
const LIVE_ALLOWED: [&str; 2] = [LIVE_GATE, "needs: check"];

/// Problems with every live nextest step in `workflows`, and how many there
/// are: nothing but [`LIVE_ALLOWED`] may skip one, and nothing may let its
/// failure pass ([`escapes`]): a red live run must fail its job, since the
/// skip budget's verdict and nextest's both reach it only through the
/// step's exit status.
fn live_step_problems(workflows: &[(String, String)]) -> (usize, Vec<String>) {
    let mut live = 0;
    let mut problems = Vec::new();
    for (name, text) in workflows {
        for (job, body) in jobs(text) {
            for step in steps(&body)
                .iter()
                .filter(|step| is_live_nextest_step(step))
            {
                live += 1;
                for escape in escapes(text, &body, step, &LIVE_ALLOWED) {
                    problems.push(format!(
                        "{name} job {job}, step `{}`: {escape}",
                        step_name(step)
                    ));
                }
            }
        }
    }
    (live, problems)
}

/// The live integration and harness steps once took `set -e` alone, where
/// `nextest | tee` has tee's status, and nothing stopped a
/// `continue-on-error` or an `|| true` on one: each passed every guard (the
/// #504 grade of 2026-11-09, M4b, M4c, M8).
#[test]
fn live_nextest_steps_fail_their_job() {
    let (live, problems) = live_step_problems(&workflows());
    assert!(live >= 4, "found only {live} live nextest steps");
    assert!(
        problems.is_empty(),
        "a live nextest step's failure must fail its job:\n  {}",
        problems.join("\n  ")
    );
}

/// `all` with the first `from` in workflow `file` replaced by `to`, which
/// must change it.
fn mutated(all: &[(String, String)], file: &str, from: &str, to: &str) -> Vec<(String, String)> {
    all.iter()
        .map(|(name, text)| {
            let changed = if name == file {
                let changed = text.replacen(from, to, 1);
                assert_ne!(&changed, text, "{file}: `{from}` did not apply");
                changed
            } else {
                text.clone()
            };
            (name.clone(), changed)
        })
        .collect()
}

#[test]
fn the_live_step_check_catches_what_it_claims_to() {
    let all = workflows();
    assert!(live_step_problems(&all).1.is_empty());
    let integration = "      - name: Run integration tests (${{ matrix.group }})\n        run: |\n          set -euo pipefail\n";
    let harness_run = "        run: |\n          set -euo pipefail\n          # Fail on an empty key: these tests self-skip";
    let broken = [
        // M4b, M4c: `set -e` alone, where `nextest | tee` has tee's status.
        mutated(
            &all,
            "rust.yml",
            integration,
            &integration.replace("set -euo pipefail", "set -e"),
        ),
        mutated(
            &all,
            "rust.yml",
            harness_run,
            &harness_run.replace("set -euo pipefail", "set -e"),
        ),
        mutated(
            &all,
            "release.yml",
            "          set -euo pipefail\n          if [ -z",
            "          set -eo pipefail\n          if [ -z",
        ),
        // M8: the failure ignored, by the step or by its job.
        mutated(
            &all,
            "rust.yml",
            "      - name: Run integration tests (${{ matrix.group }})\n",
            "      - name: Run integration tests (${{ matrix.group }})\n        continue-on-error: true\n",
        ),
        mutated(
            &all,
            "rust.yml",
            "    name: Integration Tests (${{ matrix.group }})\n",
            "    name: Integration Tests (${{ matrix.group }})\n    continue-on-error: true\n",
        ),
        mutated(
            &all,
            "release.yml",
            "      - name: Run harness integration tests\n",
            "      - name: Run harness integration tests\n        continue-on-error: ${{ github.event_name == 'push' }}\n",
        ),
        // ... or masked in the shell.
        mutated(
            &all,
            "rust.yml",
            "--success-output=final 2>&1 | tee \"/tmp/$test.log\" || rc=$?",
            "--success-output=final 2>&1 | tee \"/tmp/$test.log\" || true",
        ),
        mutated(
            &all,
            "release.yml",
            "          exit \"$rc\"\n",
            "          exit \"$rc\" || :\n",
        ),
        mutated(
            &all,
            "rust.yml",
            "          exit \"$rc\"\n",
            "          set +e\n          exit \"$rc\"\n",
        ),
        // ... or skipped beyond the owner gate.
        mutated(
            &all,
            "rust.yml",
            "        if: (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && github.repository_owner == 'evansenter'\n        run: |\n          set -euo pipefail\n          # Fail on an empty key: these tests self-skip",
            "        if: github.event_name == 'schedule'\n        run: |\n          set -euo pipefail\n          # Fail on an empty key: these tests self-skip",
        ),
        mutated(
            &all,
            "rust.yml",
            "    name: Integration Tests (${{ matrix.group }})\n    needs: check\n",
            "    name: Integration Tests (${{ matrix.group }})\n    needs: captured-fixtures\n",
        ),
        // ... or masked in a spelling the guard once let through: an
        // `exit` whose status is a quoted or computed 0, a test as the
        // handler, ...
        mutated(
            &all,
            "rust.yml",
            "| tee /tmp/antigravity_harness.log || rc=$?",
            "| tee /tmp/antigravity_harness.log || exit \"0\"",
        ),
        mutated(
            &all,
            "rust.yml",
            "| tee /tmp/antigravity_harness.log || rc=$?",
            "| tee /tmp/antigravity_harness.log || exit '0'",
        ),
        mutated(
            &all,
            "rust.yml",
            "| tee \"/tmp/$test.log\" || rc=$?",
            "| tee \"/tmp/$test.log\" || exit 00",
        ),
        mutated(
            &all,
            "rust.yml",
            "| tee \"/tmp/$test.log\" || rc=$?",
            "| tee \"/tmp/$test.log\" || exit $((0))",
        ),
        mutated(
            &all,
            "release.yml",
            "| tee /tmp/integration.log || rc=$?",
            "| tee /tmp/integration.log || [ -s /tmp/integration.log ]",
        ),
        mutated(
            &all,
            "release.yml",
            "| tee /tmp/integration.log || rc=$?",
            "| tee /tmp/integration.log || [[ 1 ]]",
        ),
        // ... an early exit, `rc` or the failure list reset before it is
        // read, or `exit "$rc"` in a branch never taken, ...
        mutated(
            &all,
            "rust.yml",
            "          done\n          if [ \"${#failed[@]}\" -gt 0 ]; then\n",
            "          done\n          exit \"0\"\n          if [ \"${#failed[@]}\" -gt 0 ]; then\n",
        ),
        mutated(
            &all,
            "rust.yml",
            "          done\n          if [ \"${#failed[@]}\" -gt 0 ]; then\n",
            "          done\n          failed=()\n          if [ \"${#failed[@]}\" -gt 0 ]; then\n",
        ),
        mutated(
            &all,
            "release.yml",
            "          exit \"$rc\"\n",
            "          rc=0\n          exit \"$rc\"\n",
        ),
        mutated(
            &all,
            "rust.yml",
            "          exit \"$rc\"\n",
            "          if [ 1 = 0 ]; then\n            exit \"$rc\"\n          fi\n",
        ),
        // ... pipefail turned off by `shopt`, a script the guard cannot
        // read, a command substitution's status lost, ...
        mutated(
            &all,
            "rust.yml",
            "          set -euo pipefail\n          # Fail on an empty key: these tests self-skip",
            "          set -euo pipefail\n          shopt -u -o pipefail\n          # Fail on an empty key: these tests self-skip",
        ),
        mutated(
            &all,
            "release.yml",
            "          cargo nextest run --workspace --run-ignored all --no-fail-fast --success-output=final 2>&1",
            "          bash -c 'cargo nextest run --workspace --run-ignored all --no-fail-fast --success-output=final || true' 2>&1",
        ),
        mutated(
            &all,
            "release.yml",
            "          cargo nextest run --workspace --run-ignored all --no-fail-fast --success-output=final 2>&1 |",
            "          echo \"$(cargo nextest run --workspace --run-ignored all --no-fail-fast --success-output=final 2>&1)\" |",
        ),
        // ... or a shell for the whole workflow that never runs a script.
        mutated(
            &all,
            "release.yml",
            "\njobs:\n",
            "\ndefaults:\n  run:\n    shell: true {0}\n\njobs:\n",
        ),
    ];
    for (i, workflows) in broken.iter().enumerate() {
        assert!(
            !live_step_problems(workflows).1.is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// Problems with every step in `workflows` that pipes nextest, and how many
/// there are: each starts with `set -euo pipefail` and never turns it off,
/// or the pipeline has its last command's status (`tee`'s), and a failed
/// run passes.
fn piped_nextest_problems(workflows: &[(String, String)]) -> (usize, Vec<String>) {
    let mut piped = 0;
    let mut problems = Vec::new();
    for (name, text) in workflows {
        for (job, body) in jobs(text) {
            for step in steps(&body).iter().filter(|step| pipes_nextest(step)) {
                piped += 1;
                let script = run_script(step);
                let turned_off = script
                    .iter()
                    .any(|line| turns_checks_off(&blank_quotes(line)));
                if script.first().map(String::as_str) != Some("set -euo pipefail") || turned_off {
                    problems.push(format!("{name} job {job}, step `{}`", step_name(step)));
                }
            }
        }
    }
    (piped, problems)
}

#[test]
fn a_step_that_pipes_nextest_sets_pipefail() {
    let all = workflows();
    let (piped, problems) = piped_nextest_problems(&all);
    assert!(piped >= 4, "found only {piped} steps that pipe nextest");
    assert!(
        problems.is_empty(),
        "a step that pipes nextest must start with `set -euo pipefail`:\n  {}",
        problems.join("\n  ")
    );
    for (file, from, to) in [
        (
            "rust.yml",
            "set -euo pipefail\n          # Fail on an empty key: every test here",
            "set -e\n          # Fail on an empty key: every test here",
        ),
        (
            "release.yml",
            "set -euo pipefail\n          if [ -z",
            "set -eu\n          if [ -z",
        ),
        (
            "release.yml",
            "          exit \"$rc\"\n",
            "          set +o pipefail\n          exit \"$rc\"\n",
        ),
    ] {
        assert!(
            !piped_nextest_problems(&mutated(&all, file, from, to))
                .1
                .is_empty(),
            "{file}: `{from}` to `{to}` went unnoticed"
        );
    }
}

/// The scripted harness suite runs, key blanked, in `test-antigravity`,
/// which is the required Test Suite (antigravity) check: a failure there
/// must fail it, on every run. An `|| true` on the step passed every guard
/// (the #504 grade of 2026-11-09, M9).
fn scripted_step_problems(workflow: &str) -> Vec<String> {
    let Some((_, job)) = jobs(workflow)
        .into_iter()
        .find(|(id, _)| id == "test-antigravity")
    else {
        return vec!["rust.yml has no test-antigravity job".into()];
    };
    let Some(step) = steps(&job).into_iter().find(|step| {
        run_script(step).iter().any(|line| {
            line.contains("--run-ignored all") && line.contains("binary(antigravity_scripted)")
        })
    }) else {
        return vec![
            "no test-antigravity step runs `--run-ignored all -E 'binary(antigravity_scripted)'`"
                .into(),
        ];
    };
    let mut problems = Vec::new();
    if !command_lines(&step).any(|line| line == "GEMINI_API_KEY: \"\"") {
        problems.push(format!(
            "the scripted suite runs with the key blanked (`GEMINI_API_KEY: \"\"`):\n{step}"
        ));
    }
    for escape in failure_escapes(workflow, &job, &step) {
        problems.push(format!(
            "the scripted step has {escape}: its failure must fail the required check"
        ));
    }
    problems
}

#[test]
fn the_scripted_harness_suite_fails_its_required_check() {
    let workflow = read(&root().join(".github/workflows/rust.yml"));
    let problems = scripted_step_problems(&workflow);
    assert!(
        problems.is_empty(),
        "rust.yml's scripted harness step:\n  {}",
        problems.join("\n  ")
    );
    let run = "        run: cargo nextest run --features antigravity --run-ignored all -E 'binary(antigravity_scripted)'\n";
    let broken = [
        // M9, and the other spellings of it.
        workflow.replace(
            run,
            &run.replace(
                "antigravity_scripted)'\n",
                "antigravity_scripted)' || true\n",
            ),
        ),
        workflow.replace(
            run,
            &run.replace("antigravity_scripted)'\n", "antigravity_scripted)'; true\n"),
        ),
        workflow.replace(
            run,
            &run.replace(
                "antigravity_scripted)'\n",
                "antigravity_scripted)' || echo flaky\n",
            ),
        ),
        workflow.replace(
            run,
            &run.replace("antigravity_scripted)'\n", "antigravity_scripted)' &\n"),
        ),
        workflow.replace(
            run,
            &format!(
                "        run: |\n          set +e\n          {}",
                run.trim_start().trim_start_matches("run: ")
            ),
        ),
        // Ignored, skipped, or skipped with a job it needs.
        workflow.replace(run, &format!("        continue-on-error: true\n{run}")),
        workflow.replace(
            run,
            &format!("        if: github.event_name == 'push'\n{run}"),
        ),
        workflow.replace(
            "    name: Test Suite (antigravity)\n",
            "    name: Test Suite (antigravity)\n    needs: captured-fixtures\n",
        ),
        // A shell for the whole workflow that never runs a script.
        workflow.replace(
            "\njobs:\n",
            "\ndefaults:\n  run:\n    shell: true {0}\n\njobs:\n",
        ),
        // The key no longer blanked.
        workflow.replace(
            &format!("{run}        env:\n          GEMINI_API_KEY: \"\"\n"),
            run,
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !scripted_step_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

#[test]
fn the_failure_escape_check_reads_the_step_and_its_job() {
    let workflow = "name: X\n\non:\n  push:\n\nenv:\n  A: b\n\njobs:\n";
    let job = "    runs-on: x\n    steps:\n";
    let step = "      - name: Floor\n        run: check\n        env:\n          if: not-a-key\n";
    assert!(failure_escapes(workflow, job, step).is_empty());
    for broken in [
        "      - name: Floor\n        if: always()\n        run: check\n",
        "      - if: github.event_name == 'push'\n        run: check\n",
        "      - name: Floor\n        continue-on-error: true\n        run: check\n",
        "      - name: Floor\n        continue-on-error: ${{ matrix.ok }}\n        run: check\n",
    ] {
        assert!(
            !failure_escapes(workflow, job, broken).is_empty(),
            "{broken}"
        );
    }
    assert!(
        failure_escapes(
            workflow,
            job,
            "      - name: Floor\n        continue-on-error: false\n        run: check\n"
        )
        .is_empty()
    );
    let lenient = "    runs-on: x\n    continue-on-error: true\n    steps:\n";
    assert!(!failure_escapes(workflow, lenient, step).is_empty());
    let skipped = "    runs-on: x\n    if: github.event_name != 'pull_request'\n    steps:\n";
    assert!(!failure_escapes(workflow, skipped, step).is_empty());
    // A step's `if:` is the step's, not the job's.
    let job_with_step_if =
        "    runs-on: x\n    steps:\n      - name: Other\n        if: always()\n";
    assert!(failure_escapes(workflow, job_with_step_if, step).is_empty());
    // A job that needs another is skipped with it; a job's `defaults:` or a
    // step's shell can run the script under other rules; and the script
    // itself can mask the failure.
    for job in [
        "    runs-on: x\n    needs: check\n    steps:\n",
        "    runs-on: x\n    needs: [check, test]\n    steps:\n",
        "    runs-on: x\n    defaults:\n      run:\n        shell: bash {0}\n    steps:\n",
    ] {
        assert!(!failure_escapes(workflow, job, step).is_empty(), "{job}");
    }
    for broken in [
        "      - name: Floor\n        shell: sh -c 'exit 0' {0}\n        run: check\n",
        "      - name: Floor\n        run: check || true\n",
        "      - name: Floor\n        run: |\n          set +e\n          check\n",
    ] {
        assert!(
            !failure_escapes(workflow, job, broken).is_empty(),
            "{broken}"
        );
    }
    assert!(
        failure_escapes(
            workflow,
            job,
            "      - name: Floor\n        shell: bash\n        run: check\n"
        )
        .is_empty()
    );
    // The workflow's own `defaults:`, whose shell template can run no
    // script at all, or a variable bash reads before every script, wherever
    // the workflow sets it.
    for broken in [
        workflow.replace(
            "\njobs:\n",
            "\ndefaults:\n  run:\n    shell: true {0}\n\njobs:\n",
        ),
        workflow.replace("  A: b\n", "  BASH_ENV: ./mask.sh\n"),
        format!(
            "{workflow}  x:\n    steps:\n      - run: echo \"SHELLOPTS=x\" >> \"$GITHUB_ENV\"\n"
        ),
    ] {
        assert!(!failure_escapes(&broken, job, step).is_empty(), "{broken}");
    }
    assert!(
        failure_escapes(
            &format!("# BASH_ENV is not set here.\n{workflow}"),
            job,
            step
        )
        .is_empty()
    );
    // What a caller allows is allowed, exactly as written, and no more.
    let gated = "    runs-on: x\n    needs: check\n    if: github.repository_owner == 'me'\n";
    let allowed = ["needs: check", "if: github.repository_owner == 'me'"];
    assert!(escapes(workflow, gated, step, &allowed).is_empty());
    assert!(!escapes(workflow, gated, step, &allowed[..1]).is_empty());
    assert!(!escapes(workflow, gated, step, &["needs: check", "if: always()"]).is_empty());
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
