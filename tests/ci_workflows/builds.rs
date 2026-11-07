//! What CI builds beyond Linux and all features (#504).
//!
//! Windows broke twice without a PR noticing (#486, then ten red pushes to
//! main): its leg ran only on main pushes, and its checkout had CRLF line
//! endings that no other leg sees. So a PR runs the Windows stable leg, the
//! full matrix also runs on the schedule, every text file checks out with LF,
//! the Windows doctests run even after a failing unit test, the antigravity
//! feature compiles off Linux, and clippy lints the library without default
//! features, in CI and in `make clippy`.

use super::{jobs, read, root, steps};

fn rust_yml() -> String {
    read(&root().join(".github/workflows/rust.yml"))
}

/// The text of `rust.yml`'s job `id`.
fn job(workflow: &str, id: &str) -> String {
    jobs(workflow)
        .into_iter()
        .find_map(|(found, body)| (found == id).then_some(body))
        .unwrap_or_else(|| panic!("rust.yml has no {id} job"))
}

/// The step of `job` whose `run:` is `command`, if there is one.
fn step_running(job: &str, command: &str) -> Option<String> {
    steps(job).into_iter().find(|step| {
        step.lines()
            .any(|line| line.trim().strip_prefix("run: ") == Some(command))
    })
}

/// The step's own `if:`, if it has one.
fn step_condition(step: &str) -> Option<&str> {
    step.lines()
        .find_map(|line| line.trim().strip_prefix("if: "))
}

/// Problems with the cross-platform job, empty when it holds: it runs on
/// every `rust.yml` trigger, the weekly schedule included (no job-level
/// `if:`); a PR's matrix is the Windows stable leg; the doctests run after a
/// failed unit-test step; and the antigravity feature is checked on Windows.
fn cross_platform_problems(workflow: &str) -> Vec<String> {
    let body = job(workflow, "cross-platform");
    let mut problems = Vec::new();
    if let Some(condition) = body.lines().find_map(|line| line.strip_prefix("    if: ")) {
        problems.push(format!(
            "a job-level `if: {condition}`: the job must run on PRs and the schedule too"
        ));
    }
    for (key, on_pr) in [("os", "windows-latest"), ("rust", "stable")] {
        let pr_only =
            format!("github.event_name == 'pull_request' && fromJSON('[\"{on_pr}\"]') ||");
        let matrix_line = body
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with(&format!("{key}: ")));
        if !matrix_line.is_some_and(|line| line.contains(&pr_only)) {
            problems.push(format!(
                "the matrix's `{key}` is not `{on_pr}` alone on a PR (`{pr_only} ...`): \
                 found {matrix_line:?}"
            ));
        }
    }
    match step_running(&body, "cargo test --workspace --release --doc") {
        Some(step) if step_condition(&step).is_some_and(|c| c.contains("!cancelled()")) => {}
        _ => problems.push(
            "the doctest step must run after a failed unit-test step (`if: ${{ !cancelled() \
             && ... }}`)"
                .into(),
        ),
    }
    match step_running(&body, "cargo check --features antigravity") {
        Some(step) => {
            let skips_windows = step_condition(&step).is_some_and(|condition| {
                condition.contains("runner.os == 'Linux'")
                    || condition.contains("runner.os == 'macOS'")
                    || condition.contains("runner.os != 'Windows'")
            });
            if skips_windows {
                problems.push("`cargo check --features antigravity` skips Windows".into());
            }
        }
        None => problems.push("no `cargo check --features antigravity` step".into()),
    }
    problems
}

#[test]
fn windows_runs_on_pull_requests_and_checks_antigravity() {
    let workflow = rust_yml();
    let problems = cross_platform_problems(&workflow);
    assert!(
        problems.is_empty(),
        "rust.yml cross-platform:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_cross_platform_check_catches_what_it_claims_to() {
    let workflow = rust_yml();
    assert!(cross_platform_problems(&workflow).is_empty());
    let broken = [
        // Back to main pushes only.
        workflow.replace(
            "    runs-on: ${{ matrix.os }}\n",
            "    if: github.event_name == 'push'\n    runs-on: ${{ matrix.os }}\n",
        ),
        // The PR leg dropped.
        workflow.replace(
            "github.event_name == 'pull_request' && fromJSON('[\"windows-latest\"]') || ",
            "",
        ),
        // Doctests skipped again after a red unit test.
        workflow.replace(
            "if: ${{ !cancelled() && steps.build.outcome == 'success' }}\n        run: cargo test --workspace --release --doc",
            "run: cargo test --workspace --release --doc",
        ),
        // The antigravity check gone.
        workflow.replace(
            "run: cargo check --features antigravity\n",
            "run: cargo check\n",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !cross_platform_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// The library a consumer gets with `default-features = false` compiles code
/// the all-features run never does (`src/wire/printer.rs` without
/// `wire-color`), so it is linted on its own, in CI and in `make clippy`.
#[test]
fn clippy_lints_the_library_without_default_features() {
    const COMMAND: &str = "cargo clippy --lib --no-default-features -- -D warnings";
    let workflow = rust_yml();
    assert!(
        step_running(&job(&workflow, "clippy"), COMMAND).is_some(),
        "rust.yml's clippy job has no step running `{COMMAND}`"
    );
    let makefile = read(&root().join("Makefile"));
    let target = makefile
        .split("\nclippy:\n")
        .nth(1)
        .expect("the Makefile has a clippy target");
    let recipe: Vec<&str> = target
        .lines()
        .take_while(|line| line.starts_with('\t'))
        .map(str::trim)
        .collect();
    assert!(
        recipe.contains(&COMMAND),
        "`make clippy` does not run `{COMMAND}`: {recipe:?}"
    );
}

/// A Windows checkout follows `core.autocrlf` unless `.gitattributes` says
/// otherwise, and CRLF broke tests that read repository files there twice.
#[test]
fn text_files_check_out_with_lf_line_endings() {
    let attributes = read(&root().join(".gitattributes"));
    assert!(
        attributes
            .lines()
            .any(|line| line.split_whitespace().collect::<Vec<_>>() == ["*", "text=auto", "eol=lf"]),
        ".gitattributes must have `* text=auto eol=lf`"
    );
}
