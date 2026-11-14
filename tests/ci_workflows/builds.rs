//! What CI builds beyond Linux and all features (#504).
//!
//! Windows broke twice without a PR noticing (#486, then eleven red pushes to
//! main): its leg ran only on main pushes, and its checkout had CRLF line
//! endings that no other leg sees. So a PR runs the Windows stable leg, the
//! full matrix also runs on the schedule, every text file checks out with LF,
//! the Windows doctests run even after a failing unit test, the antigravity
//! feature compiles off Linux, its process-tree tests run on macOS (whose
//! libproc path no other job reaches) on every macOS leg and on a PR that
//! changes the process code or what it builds with, clippy lints the
//! Windows-only code on the leg
//! a PR runs, and clippy lints every target without default features, in
//! CI and in `make clippy`.

use yaml_rust2::Yaml;

use super::{jobs, read, root, steps, yaml};

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

/// The step's own `if:`, as YAML reads it (a quoted key, or one with a
/// space before its colon, too), if it has one. A step YAML cannot read
/// gives a condition no term list allows, so it never reads as
/// unconditional.
fn step_condition(step: &str) -> Option<String> {
    yaml::step_value(step, "if").unwrap_or_else(|e| Some(format!("<a step YAML cannot read: {e}>")))
}

/// The Windows clippy step's command.
const WINDOWS_CLIPPY: &str = "cargo clippy --features antigravity --all-targets -- -D warnings";

/// The `&&` terms the Windows clippy step's `if:` may have. Each keeps the
/// step on the Windows stable leg, the PR's included.
const WINDOWS_CLIPPY_CONDITION_TERMS: [&str; 7] = [
    "!cancelled()",
    "always()",
    "success()",
    "runner.os != 'Linux'",
    "runner.os == 'Windows'",
    "matrix.os == 'windows-latest'",
    "matrix.rust == 'stable'",
];

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

/// The `&&` terms of a step's `if:` that are not in `allowed`, the
/// `${{ }}` taken off; empty when it has no `if:`.
fn unknown_condition_terms(step: &str, allowed: &[&str]) -> Vec<String> {
    let Some(condition) = step_condition(step) else {
        return Vec::new();
    };
    let expression = condition
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
        .unwrap_or(&condition);
    expression
        .split("&&")
        .map(str::trim)
        .filter(|term| !allowed.contains(term))
        .map(String::from)
        .collect()
}

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

/// A matrix value written `${{ github.event_name == 'pull_request' &&
/// fromJSON('[..]') || fromJSON('[..]') }}`, as `(the PR's values, every
/// other trigger's values)`; `None` for any other shape.
pub(super) fn event_matrix(value: &str) -> Option<(Vec<String>, Vec<String>)> {
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
/// `if:`, read as YAML reads the job's keys, as each `if:` here is); a PR's
/// matrix is the Windows stable leg and every other trigger's is the full
/// matrix, none of it excluded (no `exclude:`, read the same way); the doctests run after a failed
/// unit-test step; the antigravity feature is checked on every Windows
/// leg; clippy lints it, every target, on the Windows stable leg, with
/// the toolchain's clippy component installed; and the antigravity
/// process-tree tests run on every macOS leg.
fn cross_platform_problems(workflow: &str) -> Vec<String> {
    let body = job(workflow, "cross-platform");
    let mut problems = Vec::new();
    let read = yaml::job_mapping(&body);
    match &read {
        Ok(read) => {
            if let Some(condition) = yaml::get(read, "if") {
                problems.push(format!(
                    "a job-level `if: {}`: the job must run on PRs and the schedule too",
                    yaml::text(condition)
                ));
            }
            let excludes = yaml::get(read, "strategy")
                .and_then(Yaml::as_hash)
                .and_then(|strategy| yaml::get(strategy, "matrix"))
                .and_then(Yaml::as_hash)
                .is_some_and(|matrix| yaml::get(matrix, "exclude").is_some());
            if excludes {
                problems.push("the matrix excludes legs: the full matrix must run whole".into());
            }
        }
        Err(e) => problems.push(format!(
            "a job YAML cannot read ({e}), so neither its `if:` nor its matrix can be read"
        )),
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
            let unknown = unknown_condition_terms(&step, &ANTIGRAVITY_CONDITION_TERMS);
            if !unknown.is_empty() {
                problems.push(format!(
                    "`cargo check --features antigravity` must run on every Windows leg, \
                     the PR's included: its `if:` has {unknown:?}, not only terms from \
                     {ANTIGRAVITY_CONDITION_TERMS:?}"
                ));
            }
        }
        None => problems.push("no `cargo check --features antigravity` step".into()),
    }
    match step_running(&body, WINDOWS_CLIPPY) {
        Some(step) => {
            let unknown = unknown_condition_terms(&step, &WINDOWS_CLIPPY_CONDITION_TERMS);
            if !unknown.is_empty() {
                problems.push(format!(
                    "`{WINDOWS_CLIPPY}` must run on the Windows stable leg, the PR's \
                     included: its `if:` has {unknown:?}, not only terms from \
                     {WINDOWS_CLIPPY_CONDITION_TERMS:?}"
                ));
            }
        }
        None => problems.push(format!(
            "no `{WINDOWS_CLIPPY}` step: nothing lints the Windows-only code"
        )),
    }
    let components = body
        .lines()
        .find_map(|line| line.trim().strip_prefix("components: "));
    if !components.is_some_and(|list| list.split(',').any(|c| c.trim() == "clippy")) {
        problems.push(format!(
            "the toolchain step does not install clippy (`components: clippy`): found \
             {components:?}"
        ));
    }
    match step_running(&body, PROCESS_TREE_TESTS) {
        Some(step) => {
            let unknown = unknown_condition_terms(&step, &PROCESS_TREE_CONDITION_TERMS);
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
        // The Windows clippy step gone, or narrowed to the library.
        workflow.replace(
            "run: cargo clippy --features antigravity --all-targets -- -D warnings\n",
            "run: cargo clippy --features antigravity -- -D warnings\n",
        ),
        // ... on macOS instead, or off the PR's leg.
        workflow.replace(
            "runner.os == 'Windows' && matrix.rust == 'stable' }}\n        run: cargo clippy",
            "runner.os == 'macOS' && matrix.rust == 'stable' }}\n        run: cargo clippy",
        ),
        workflow.replace(
            "matrix.rust == 'stable' }}\n        run: cargo clippy",
            "matrix.rust == 'stable' && github.event_name != 'pull_request' }}\n        run: cargo clippy",
        ),
        // ... or with no clippy to run.
        workflow.replace("          components: clippy\n      - uses: taiki-e/install-action@cargo-nextest", "      - uses: taiki-e/install-action@cargo-nextest"),
        // The macOS process-tree tests gone, widened to every test, or
        // moved off macOS.
        workflow.replace(&format!("        run: {PROCESS_TREE_TESTS}\n"), ""),
        workflow.replace("-E 'test(/process_tree/)'", "-E 'all()'"),
        workflow.replace(
            &format!("runner.os == 'macOS' }}}}\n        run: {PROCESS_TREE_TESTS}"),
            &format!("runner.os == 'Linux' }}}}\n        run: {PROCESS_TREE_TESTS}"),
        ),
        // Each of these with its key quoted or spaced, which YAML reads as
        // the key and a text match once missed (#504, L-6): a job-level
        // `if:`, a step's `if:` that moves the antigravity check or the
        // process-tree tests off their legs, and an `exclude:`.
        workflow.replace(
            "    runs-on: ${{ matrix.os }}\n",
            "    \"if\": github.event_name == 'push'\n    runs-on: ${{ matrix.os }}\n",
        ),
        workflow.replace(
            "    runs-on: ${{ matrix.os }}\n",
            "    if : github.event_name == 'push'\n    runs-on: ${{ matrix.os }}\n",
        ),
        workflow.replace(
            "        if: ${{ !cancelled() && runner.os != 'Linux' }}\n        run: cargo check --features antigravity",
            "        \"if\": ${{ runner.os == 'macOS' }}\n        run: cargo check --features antigravity",
        ),
        workflow.replace(
            &format!("        if: ${{{{ !cancelled() && runner.os == 'macOS' }}}}\n        run: {PROCESS_TREE_TESTS}"),
            &format!("        'if': ${{{{ runner.os == 'Linux' }}}}\n        run: {PROCESS_TREE_TESTS}"),
        ),
        workflow.replace(
            "fromJSON('[\"stable\", \"beta\"]') }}\n",
            "fromJSON('[\"stable\", \"beta\"]') }}\n        \"exclude\":\n          - os: windows-latest\n",
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

/// The process-tree tests a PR runs on macOS: the library's unit tests
/// alone, in the default profile.
const PR_PROCESS_TREE_TESTS: &str =
    "cargo nextest run --features antigravity --lib -E 'test(/process_tree/)'";

/// The `case` arms the PR job's change check must have: every
/// `src/antigravity/process*.rs` file, `process_tree.rs` and the tests
/// beside them included; what the libproc path builds with, the
/// dependencies (`libc`, `tokio`) and the toolchain, which can break it as
/// surely as the code can; and `rust.yml`, so that a change to the job runs
/// it.
const PROCESS_TREE_INPUTS: [&str; 5] = [
    "src/antigravity/process*.rs",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain*",
    ".github/workflows/rust.yml",
];

/// Problems with the job that runs the process-tree tests on macOS for a
/// PR, empty when it holds. The Cross Platform macOS legs run on main
/// pushes and the schedule only, so a PR that broke the libproc lookup
/// would first fail on main. One job runs on every PR, on macos-latest; a
/// step with `id: paths` sets `changed` to `true` when the PR touches one
/// of [`PROCESS_TREE_INPUTS`] (and when it cannot tell, rather than skip),
/// and the tests run when it is `true`. The job's keys, the step's `id:`
/// and each `if:` are read as YAML reads them.
fn process_tree_pr_problems(workflow: &str) -> Vec<String> {
    let found: Vec<(String, String)> = jobs(workflow)
        .into_iter()
        .filter(|(_, body)| step_running(body, PR_PROCESS_TREE_TESTS).is_some())
        .collect();
    let [(id, body)] = found.as_slice() else {
        return vec![format!(
            "want exactly one job running `{PR_PROCESS_TREE_TESTS}`; found {:?}",
            found.iter().map(|(id, _)| id).collect::<Vec<_>>()
        )];
    };
    let mut problems = Vec::new();
    let field = |key: &str| yaml::job_value(body, key).unwrap_or_else(|e| Some(format!("<{e}>")));
    if field("runs-on").as_deref() != Some("macos-latest") {
        problems.push(format!(
            "{id} runs on {:?}, not macos-latest",
            field("runs-on")
        ));
    }
    if field("if").as_deref() != Some("github.event_name == 'pull_request'") {
        problems.push(format!(
            "{id} must run on every pull request (`if: github.event_name == \
             'pull_request'`): found {:?}",
            field("if")
        ));
    }
    let job_steps = steps(body);
    match job_steps
        .iter()
        .find(|step| yaml::step_value(step, "id").ok().flatten().as_deref() == Some("paths"))
    {
        Some(check) => {
            let script: Vec<&str> = check.lines().map(str::trim).collect();
            for input in PROCESS_TREE_INPUTS {
                if !script.iter().any(|line| {
                    line.split('|')
                        .any(|arm| arm.trim_end_matches(')') == input)
                }) {
                    problems.push(format!(
                        "{id}'s change check has no `case` arm for `{input}`"
                    ));
                }
            }
            for (needle, why) in [
                (
                    "echo \"changed=$changed\" >> \"$GITHUB_OUTPUT\"",
                    "records what it found",
                ),
                (
                    "echo \"changed=true\" >> \"$GITHUB_OUTPUT\"",
                    "tests when it cannot tell",
                ),
            ] {
                if !script.contains(&needle) {
                    problems.push(format!("{id}'s change check never {why} (`{needle}`)"));
                }
            }
        }
        None => problems.push(format!(
            "{id} has no change check (a step with `id: paths`)"
        )),
    }
    if let Some(step) = step_running(body, PR_PROCESS_TREE_TESTS) {
        match step_condition(&step).as_deref() {
            None | Some("steps.paths.outputs.changed == 'true'") => {}
            Some(other) => problems.push(format!(
                "{id}: the tests run `if: {other}`, not when the process code changed"
            )),
        }
    }
    problems
}

#[test]
fn a_pull_request_that_changes_the_process_code_runs_the_macos_tests() {
    let problems = process_tree_pr_problems(&rust_yml());
    assert!(
        problems.is_empty(),
        "rust.yml's PR process-tree job:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_process_tree_pr_check_catches_what_it_claims_to() {
    let workflow = rust_yml();
    assert!(process_tree_pr_problems(&workflow).is_empty());
    let broken = [
        // The job gone, or widened to every test.
        workflow.replace(&format!("        run: {PR_PROCESS_TREE_TESTS}\n"), ""),
        workflow.replace(
            &format!("        run: {PR_PROCESS_TREE_TESTS}\n"),
            "        run: cargo nextest run --features antigravity --lib\n",
        ),
        // Off macOS, or off pull requests.
        workflow.replace(
            "    name: Process Tree (macOS)\n    if: github.event_name == 'pull_request'\n    runs-on: macos-latest\n",
            "    name: Process Tree (macOS)\n    if: github.event_name == 'pull_request'\n    runs-on: ubuntu-latest\n",
        ),
        workflow.replace(
            "    name: Process Tree (macOS)\n    if: github.event_name == 'pull_request'\n",
            "    name: Process Tree (macOS)\n    if: github.event_name == 'push'\n",
        ),
        // A filter that misses process_tree.rs, the dependencies, the
        // toolchain or the job itself, or one that never tests.
        workflow.replace(
            "              src/antigravity/process*.rs|",
            "              src/antigravity/process.rs|",
        ),
        workflow.replace("|Cargo.toml|Cargo.lock|", "|Cargo.lock|"),
        workflow.replace("|Cargo.toml|Cargo.lock|", "|Cargo.toml|"),
        workflow.replace("|rust-toolchain*|", "|rust-toolchain.toml|"),
        workflow.replace("|.github/workflows/rust.yml)\n", ")\n"),
        workflow.replace(
            "            echo \"changed=true\" >> \"$GITHUB_OUTPUT\"\n            exit 0\n",
            "            echo \"changed=false\" >> \"$GITHUB_OUTPUT\"\n            exit 0\n",
        ),
        workflow.replace(
            &format!(
                "        if: steps.paths.outputs.changed == 'true'\n        run: {PR_PROCESS_TREE_TESTS}"
            ),
            &format!(
                "        if: steps.paths.outputs.changed == 'false'\n        run: {PR_PROCESS_TREE_TESTS}"
            ),
        ),
        // ... the same with the key quoted or spaced, which a text match
        // once read as no `if:` at all (#504, L-6).
        workflow.replace(
            &format!(
                "        if: steps.paths.outputs.changed == 'true'\n        run: {PR_PROCESS_TREE_TESTS}"
            ),
            &format!(
                "        \"if\": steps.paths.outputs.changed == 'false'\n        run: {PR_PROCESS_TREE_TESTS}"
            ),
        ),
        workflow.replace(
            &format!(
                "        if: steps.paths.outputs.changed == 'true'\n        run: {PR_PROCESS_TREE_TESTS}"
            ),
            &format!(
                "        if : steps.paths.outputs.changed == 'false'\n        run: {PR_PROCESS_TREE_TESTS}"
            ),
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !process_tree_pr_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// What a consumer builds with `default-features = false` compiles code the
/// all-features run never does (`src/wire/printer.rs` without `wire-color`),
/// so every target is linted that way too, in CI and in `make clippy`.
#[test]
fn clippy_lints_every_target_without_default_features() {
    const COMMAND: &str =
        "cargo clippy --workspace --all-targets --no-default-features -- -D warnings";
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
