//! What CI builds beyond Linux and all features (#504).
//!
//! Windows broke twice without a PR noticing (#486, then eleven red pushes to
//! main): its leg ran only on main pushes, and its checkout had CRLF line
//! endings that no other leg sees. So a PR runs the Windows stable leg, the
//! full matrix also runs on the schedule, every text file checks out with LF,
//! the Windows doctests run even after a failing unit test, the antigravity
//! feature compiles off Linux, its process-tree tests run on macOS (whose
//! libproc path no other job reaches), and clippy lints the library without
//! default features, in CI and in `make clippy`.

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

/// Each matrix key: the one value a PR runs, and the values every other
/// trigger (main pushes, the schedule, manual runs) must keep.
const MATRIX: [(&str, &str, &[&str]); 2] = [
    (
        "os",
        "windows-latest",
        &["ubuntu-latest", "macos-latest", "windows-latest"],
    ),
    ("rust", "stable", &["stable", "beta"]),
];

/// The `&&` terms the antigravity check's `if:` may have. Each keeps the step
/// on every Windows leg, the PR's included; anything else (another OS test,
/// an event test, an `||`) is refused rather than read.
const ANTIGRAVITY_CONDITION_TERMS: [&str; 5] = [
    "!cancelled()",
    "always()",
    "success()",
    "runner.os != 'Linux'",
    "runner.os == 'Windows'",
];

/// The step that runs the antigravity process-tree tests on macOS.
const PROCESS_TREE_TESTS: &str = "cargo nextest run --features antigravity --cargo-profile release \
                                  --lib -E 'test(/process_tree/)'";

/// The `&&` terms the process-tree step's `if:` may have. Each keeps the
/// step on the macOS legs; anything else is refused rather than read.
const PROCESS_TREE_CONDITION_TERMS: [&str; 5] = [
    "!cancelled()",
    "always()",
    "success()",
    "runner.os == 'macOS'",
    "runner.os != 'Linux'",
];

/// The `&&` terms of a step's `if:` that are not in `allowed`.
fn unknown_terms<'a>(condition: &'a str, allowed: &[&str]) -> Vec<&'a str> {
    let expression = condition
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(condition);
    expression
        .split("&&")
        .map(str::trim)
        .filter(|term| !allowed.contains(term))
        .collect()
}

/// A matrix value written `${{ github.event_name == 'pull_request' &&
/// fromJSON('[..]') || fromJSON('[..]') }}`, as `(the PR's values, every
/// other trigger's values)`; `None` for any other shape.
fn event_matrix(value: &str) -> Option<(Vec<String>, Vec<String>)> {
    let rest = value
        .strip_prefix("${{ github.event_name == 'pull_request' && fromJSON('")?
        .strip_suffix("') }}")?;
    let (on_pr, otherwise) = rest.split_once("') || fromJSON('")?;
    Some((
        serde_json::from_str(on_pr).ok()?,
        serde_json::from_str(otherwise).ok()?,
    ))
}

/// Problems with the cross-platform job, empty when it holds: it runs on
/// every `rust.yml` trigger, the weekly schedule included (no job-level
/// `if:`); a PR's matrix is the Windows stable leg and every other trigger's
/// is the full matrix, none of it excluded; the doctests run after a failed
/// unit-test step; the antigravity feature is checked on every Windows
/// leg; and the antigravity process-tree tests run on every macOS leg.
fn cross_platform_problems(workflow: &str) -> Vec<String> {
    let body = job(workflow, "cross-platform");
    let mut problems = Vec::new();
    if let Some(condition) = body.lines().find_map(|line| line.strip_prefix("    if: ")) {
        problems.push(format!(
            "a job-level `if: {condition}`: the job must run on PRs and the schedule too"
        ));
    }
    for (key, on_pr, full) in MATRIX {
        let value = body
            .lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{key}: ")));
        let Some((pr_legs, other_legs)) = value.and_then(event_matrix) else {
            problems.push(format!(
                "the matrix's `{key}` is not `${{{{ github.event_name == 'pull_request' && \
                 fromJSON('[..]') || fromJSON('[..]') }}}}`: found {value:?}"
            ));
            continue;
        };
        if pr_legs != [on_pr] {
            problems.push(format!(
                "the matrix's `{key}` on a PR is {pr_legs:?}, not [{on_pr:?}] alone"
            ));
        }
        let missing: Vec<&str> = full
            .iter()
            .copied()
            .filter(|leg| !other_legs.iter().any(|found| found == leg))
            .collect();
        if !missing.is_empty() {
            problems.push(format!(
                "the matrix's `{key}` off a PR (main pushes, the schedule) drops {missing:?}: \
                 found {other_legs:?}"
            ));
        }
    }
    if body.lines().any(|line| line.trim() == "exclude:") {
        problems.push("the matrix excludes legs: the full matrix must run whole".into());
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
            if let Some(condition) = step_condition(&step) {
                let unknown = unknown_terms(condition, &ANTIGRAVITY_CONDITION_TERMS);
                if !unknown.is_empty() {
                    problems.push(format!(
                        "`cargo check --features antigravity` must run on every Windows leg, \
                         the PR's included: its `if:` has {unknown:?}, not only terms from \
                         {ANTIGRAVITY_CONDITION_TERMS:?}"
                    ));
                }
            }
        }
        None => problems.push("no `cargo check --features antigravity` step".into()),
    }
    match step_running(&body, PROCESS_TREE_TESTS) {
        Some(step) => {
            let unknown = step_condition(&step).map_or_else(Vec::new, |condition| {
                unknown_terms(condition, &PROCESS_TREE_CONDITION_TERMS)
            });
            if !unknown.is_empty() {
                problems.push(format!(
                    "the process-tree tests must run on every macOS leg: their `if:` has \
                     {unknown:?}, not only terms from {PROCESS_TREE_CONDITION_TERMS:?}"
                ));
            }
        }
        None => problems.push(format!("no step running `{PROCESS_TREE_TESTS}`")),
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
        // Every OS on a PR.
        workflow.replace(
            "fromJSON('[\"windows-latest\"]') || fromJSON('[\"ubuntu-latest\", \"macos-latest\", \"windows-latest\"]')",
            "fromJSON('[\"ubuntu-latest\", \"macos-latest\", \"windows-latest\"]') || fromJSON('[\"ubuntu-latest\", \"macos-latest\", \"windows-latest\"]')",
        ),
        // Main pushes and the schedule down to Windows: Linux and macOS gone.
        workflow.replace(
            "|| fromJSON('[\"ubuntu-latest\", \"macos-latest\", \"windows-latest\"]')",
            "|| fromJSON('[\"windows-latest\"]')",
        ),
        // Main pushes and the schedule down to Linux: Windows gone.
        workflow.replace(
            "|| fromJSON('[\"ubuntu-latest\", \"macos-latest\", \"windows-latest\"]')",
            "|| fromJSON('[\"ubuntu-latest\"]')",
        ),
        // Beta gone from main pushes and the schedule.
        workflow.replace(
            "|| fromJSON('[\"stable\", \"beta\"]')",
            "|| fromJSON('[\"stable\"]')",
        ),
        // The Windows legs excluded from the full matrix.
        workflow.replace(
            "fromJSON('[\"stable\", \"beta\"]') }}\n",
            "fromJSON('[\"stable\", \"beta\"]') }}\n        exclude:\n          - os: windows-latest\n",
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
        // The antigravity check on macOS only, spelt three ways.
        workflow.replace(
            "runner.os != 'Linux' }}\n        run: cargo check --features antigravity",
            "runner.os == 'macOS' }}\n        run: cargo check --features antigravity",
        ),
        workflow.replace(
            "runner.os != 'Linux' }}\n        run: cargo check --features antigravity",
            "matrix.os == 'macos-latest' }}\n        run: cargo check --features antigravity",
        ),
        workflow.replace(
            "runner.os != 'Linux' }}\n        run: cargo check --features antigravity",
            "runner.os != 'Linux' && runner.os != 'Windows' }}\n        run: cargo check --features antigravity",
        ),
        // The antigravity check off the PR's Windows leg.
        workflow.replace(
            "runner.os != 'Linux' }}\n        run: cargo check --features antigravity",
            "runner.os != 'Linux' && github.event_name != 'pull_request' }}\n        run: cargo check --features antigravity",
        ),
        // ... or behind an `||` the terms check cannot read.
        workflow.replace(
            "!cancelled() && runner.os != 'Linux' }}\n        run: cargo check --features antigravity",
            "runner.os == 'macOS' || runner.os == 'Linux' }}\n        run: cargo check --features antigravity",
        ),
        // The macOS process-tree tests gone, widened to every test, or
        // moved off macOS.
        workflow.replace(&format!("        run: {PROCESS_TREE_TESTS}\n"), ""),
        workflow.replace("-E 'test(/process_tree/)'", "-E 'all()'"),
        workflow.replace(
            &format!("runner.os == 'macOS' }}}}\n        run: {PROCESS_TREE_TESTS}"),
            &format!("runner.os == 'Linux' }}}}\n        run: {PROCESS_TREE_TESTS}"),
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
