//! The Code Coverage job reports somewhere every run, measures the
//! `antigravity` feature with its scripted suite, counts library files
//! alone, and fails under a floor that no `if:`, `continue-on-error`,
//! `needs:` or `defaults:`, as YAML reads the workflow's keys, nor a
//! shell-level mask the guards know in the `run:` value YAML reads, skips
//! or ignores, from a step that holds its name and its one command and
//! nothing else (#504, D-058).

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use yaml_rust2::Yaml;

use super::{command_lines, failure_escapes, jobs, read, root, run_script, steps, yaml};
use crate::source_scan;

/// The Code Coverage job of `workflow`, and its steps.
fn coverage_job(workflow: &str) -> (String, Vec<String>) {
    let (_, job) = jobs(workflow)
        .into_iter()
        .find(|(id, _)| id == "coverage")
        .expect("rust.yml has a coverage job");
    let job_steps = steps(&job);
    (job, job_steps)
}

fn rust_yml() -> String {
    read(&root().join(".github/workflows/rust.yml"))
}

/// The lowest floor the guard accepts: well under the measured value, so
/// only a floor dropped by mistake (or removed by setting it to 0) trips
/// it, not the floor's own small moves.
const MIN_FLOOR: f64 = 90.0;

/// Problems with the Code Coverage job's summary and upload, empty when
/// they hold. With no `CODECOV_TOKEN` the Codecov upload used to fail
/// quietly under `fail_ci_if_error: false`, so the job passed with coverage
/// reported nowhere. The job writes llvm-cov's summary, over the packages
/// it measured, to the job summary on every run (no `if:` on its step), and
/// the upload, when it runs, fails the job on an error. The keys are read
/// as YAML reads them, so a quoted or spaced `if:` counts.
fn summary_problems(workflow: &str) -> Vec<String> {
    let (_, all) = coverage_job(workflow);
    let mut problems = Vec::new();

    let report_line = |step: &String| {
        command_lines(step)
            .find(|line| line.contains("cargo llvm-cov report") && line.contains("--summary-only"))
            .map(str::to_owned)
    };
    let Some((summary, report)) = all
        .iter()
        .find_map(|step| report_line(step).map(|line| (step, line)))
    else {
        return vec!["no step runs `cargo llvm-cov report --summary-only`".into()];
    };
    if !summary.contains(">> \"$GITHUB_STEP_SUMMARY\"") {
        problems.push(format!(
            "the coverage summary goes to the job summary:\n{summary}"
        ));
    }
    match yaml::step_value(summary, "if") {
        Ok(None) => {}
        Ok(Some(condition)) => problems.push(format!(
            "the coverage summary is written on every run, but its step has \
             `if: {condition}`"
        )),
        Err(e) => problems.push(format!(
            "a summary step YAML cannot read ({e}), so its `if:` cannot be read"
        )),
    }

    // `cargo llvm-cov report` selects packages afresh: without the run's
    // `--workspace` it reports the root package only, so the summary would
    // leave out genai-rs-macros, which the uploaded lcov holds.
    match all
        .iter()
        .flat_map(|step| command_lines(step).collect::<Vec<_>>())
        .find(|line| line.contains("cargo llvm-cov nextest"))
    {
        Some(run) if run.contains("--workspace") != report.contains("--workspace") => {
            problems.push(format!(
                "the summary's `cargo llvm-cov report` must select the packages the run \
                 measured (`--workspace`):\n  run:    {run}\n  report: {report}"
            ));
        }
        Some(_) => {}
        None => problems.push("the coverage job runs `cargo llvm-cov nextest`".into()),
    }

    let uploads: Vec<&String> = all
        .iter()
        .filter(|step| step.contains("codecov/codecov-action"))
        .collect();
    if uploads.is_empty() {
        problems.push("no Codecov upload step in the coverage job".into());
    }
    for upload in uploads {
        let read = yaml::step_mapping(upload);
        let fails = read.as_ref().ok().is_some_and(|step| {
            yaml::get(step, "with")
                .and_then(Yaml::as_hash)
                .and_then(|with| yaml::get(with, "fail_ci_if_error"))
                == Some(&Yaml::Boolean(true))
        });
        if !fails {
            problems.push(format!(
                "a Codecov upload must fail the job on an error (`fail_ci_if_error: true`):\n{upload}"
            ));
        }
        if yaml::step_value(upload, "if").ok().flatten().as_deref()
            != Some("steps.codecov.outputs.upload == 'true'")
        {
            problems.push(format!(
                "the upload runs only when the token is set:\n{upload}"
            ));
        }
    }
    problems
}

#[test]
fn code_coverage_reports_a_summary_and_fails_a_failed_upload() {
    let problems = summary_problems(&rust_yml());
    assert!(
        problems.is_empty(),
        "rust.yml's Code Coverage summary and upload:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_summary_check_catches_what_it_claims_to() {
    let workflow = rust_yml();
    assert!(summary_problems(&workflow).is_empty());
    let summary = "      - name: Coverage summary\n";
    let broken = [
        // The summary skipped on some runs, its `if:` written plain,
        // quoted or spaced (#504, L-6: the quoted key once passed).
        workflow.replace(
            summary,
            &format!("{summary}        if: github.event_name == 'schedule'\n"),
        ),
        workflow.replace(
            summary,
            &format!("{summary}        \"if\": github.event_name == 'schedule'\n"),
        ),
        workflow.replace(
            summary,
            &format!("{summary}        if : github.event_name == 'schedule'\n"),
        ),
        // ... written somewhere else, or over the root package alone.
        workflow.replace("} >> \"$GITHUB_STEP_SUMMARY\"", "} >> summary.md"),
        workflow.replace(
            "cargo llvm-cov report --workspace --ignore-filename-regex \"$COVERAGE_IGNORE\" --summary-only",
            "cargo llvm-cov report --ignore-filename-regex \"$COVERAGE_IGNORE\" --summary-only",
        ),
        // A failed upload that passes, or an upload that runs with no
        // token.
        workflow.replace("fail_ci_if_error: true", "fail_ci_if_error: false"),
        workflow.replace(
            "        if: steps.codecov.outputs.upload == 'true'\n",
            "        if: always()\n",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !summary_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// Whether a `#[cfg(..)]` predicate (`module_files` keeps each as its
/// tokens) holds only in a test or fuzzing build: `test`, `all(test, ..)`,
/// `any(fuzzing, test)` (the fuzzing entry points are compiled for
/// cargo-fuzz too, which a coverage run is not), through the guards' one
/// `cfg` reading (`source_scan::cfg_needs`). One it cannot parse can hold.
fn test_build_only(predicate: &str) -> bool {
    syn::parse_str::<syn::Meta>(predicate)
        .is_ok_and(|meta| source_scan::cfg_needs(&meta, &["test", "fuzzing"]))
}

/// Every file module of the crate rooted at `lib`, as `(path relative to
/// the repository, whether only a test build compiles it)`, from the shared
/// module walk (`source_scan::module_files`). A module is test-only when a
/// `cfg` on the way to it, on its declaration, an inline module around it
/// or a module that declares it, holds in a test build alone.
fn crate_files(lib: &Path) -> Vec<(String, bool)> {
    source_scan::module_files(lib)
        .into_iter()
        .map(|module| {
            let test_only = module
                .cfgs
                .iter()
                .any(|predicate| test_build_only(predicate));
            (source_scan::repo_relative(&module.path), test_only)
        })
        .collect()
}

/// The library crates' source files, and which only a test build compiles.
fn library_files() -> &'static [(String, bool)] {
    static FILES: OnceLock<Vec<(String, bool)>> = OnceLock::new();
    FILES.get_or_init(|| {
        let mut files = crate_files(&root().join("src/lib.rs"));
        files.extend(crate_files(&root().join("genai-rs-macros/src/lib.rs")));
        files
    })
}

/// Problems with the Code Coverage job in `workflow`, empty when it holds.
///
/// Coverage once left out the `antigravity` feature, about a fifth of the
/// source, and had no floor, so a drop passed (#504). Then it counted the
/// test-only code a test build compiles into the library, nearly all of it
/// covered, which diluted a drop in the library's own coverage. The job
/// measures the feature in both of its runs, the unit tests and the
/// key-free scripted harness suite (which tests most of it); every report
/// leaves out the test-only files by one `COVERAGE_IGNORE` pattern, which
/// matches every module file only a test build compiles and no other (an
/// inline `#[cfg(test)] mod tests { .. }` in a library file is beyond a
/// filename pattern, and still counted); and a step after the summary,
/// with no key but its name and its `run:`, runs one command, its `run:`
/// value as YAML reads it, that fails under a floor near the measured
/// value, with nothing the guards know to skip it or pass its failure
/// ([`failure_escapes`]: an `if:`, `continue-on-error` or `needs:`, on the
/// step or its job, a `defaults:`, each key as YAML reads it, or a
/// shell-level mask such as `|| true`, `exit "0"` or one an expression
/// splices in), as the runs before it have.
fn coverage_problems(workflow: &str, files: &[(String, bool)]) -> Vec<String> {
    let (job, all) = coverage_job(workflow);
    let mut problems = Vec::new();
    let lines: Vec<String> = all
        .iter()
        .flat_map(|step| command_lines(step).map(str::to_owned).collect::<Vec<_>>())
        .collect();

    let runs: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("cargo llvm-cov nextest"))
        .collect();
    if runs.len() < 2 {
        problems.push(format!(
            "the coverage job runs the unit tests and the scripted suite: {runs:?}"
        ));
    }
    for run in &runs {
        if !(run.contains("--workspace") && run.contains("--features antigravity")) {
            problems.push(format!(
                "every coverage run measures the workspace with the antigravity feature: {run}"
            ));
        }
        if !run.contains("--no-report") {
            problems.push(format!(
                "the runs feed one report, so each is --no-report: {run}"
            ));
        }
    }
    for step in all
        .iter()
        .filter(|step| command_lines(step).any(|line| line.contains("cargo llvm-cov nextest")))
    {
        for escape in failure_escapes(workflow, &job, step) {
            problems.push(format!(
                "a coverage run's step has {escape}: a failing test must fail the job, not \
                 leave a figure from a partial run"
            ));
        }
    }
    if !runs.iter().any(|run| {
        run.contains("--run-ignored all") && run.contains("binary(antigravity_scripted)")
    }) {
        problems.push(format!(
            "the scripted harness suite runs under coverage: {runs:?}"
        ));
    }
    match all
        .iter()
        .find(|step| step.contains("binary(antigravity_scripted)"))
    {
        Some(scripted) if yaml::step_env(scripted, "GEMINI_API_KEY").as_deref() == Some("") => {}
        Some(scripted) => problems.push(format!(
            "the scripted suite runs with the key blanked:\n{scripted}"
        )),
        None => problems.push("no step runs the scripted suite".into()),
    }
    if !lines
        .iter()
        .any(|line| line.contains("pip install google-antigravity=="))
    {
        problems
            .push("the coverage job installs the pinned harness the scripted suite drives".into());
    }
    if !lines
        .iter()
        .any(|line| line.contains("cargo llvm-cov clean --workspace"))
    {
        problems.push(
            "the job starts from a clean profile, so a cached run cannot merge into this one"
                .into(),
        );
    }

    // Library code only: one pattern, the job's, as YAML reads its env, on
    // every report.
    let pattern = yaml::job_mapping(&job).ok().and_then(|job| {
        let env = yaml::get(&job, "env")?.as_hash()?;
        yaml::get(env, "COVERAGE_IGNORE").map(yaml::text)
    });
    match pattern.as_deref().map(Regex::new) {
        None => problems.push("the job's env sets no COVERAGE_IGNORE pattern".into()),
        Some(Err(e)) => problems.push(format!("COVERAGE_IGNORE is not a regex: {e}")),
        Some(Ok(ignore)) => {
            // llvm-cov matches the pattern against absolute paths.
            let matches = |file: &str| ignore.is_match(&format!("/checkout/genai-rs/{file}"));
            let kept: Vec<&str> = files
                .iter()
                .filter(|(file, test_only)| *test_only && !matches(file))
                .map(|(file, _)| file.as_str())
                .collect();
            if !kept.is_empty() {
                problems.push(format!(
                    "COVERAGE_IGNORE leaves test-only sources in the coverage figure: {kept:?}"
                ));
            }
            let dropped: Vec<&str> = files
                .iter()
                .filter(|(file, test_only)| !*test_only && matches(file))
                .map(|(file, _)| file.as_str())
                .collect();
            if !dropped.is_empty() {
                problems.push(format!(
                    "COVERAGE_IGNORE leaves library code out of the coverage figure: {dropped:?}"
                ));
            }
        }
    }
    let reports: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("cargo llvm-cov report"))
        .collect();
    if reports.len() < 3 {
        problems.push(format!(
            "the lcov, the summary and the floor each run `cargo llvm-cov report`: {reports:?}"
        ));
    }
    for report in reports {
        if !report.contains("--ignore-filename-regex \"$COVERAGE_IGNORE\"") {
            problems.push(format!(
                "a report counts test-only code (no `--ignore-filename-regex \
                 \"$COVERAGE_IGNORE\"`): {report}"
            ));
        }
    }

    let Some(summary_at) = all.iter().position(|step| step.contains("--summary-only")) else {
        problems.push("no summary step".into());
        return problems;
    };
    let Some((floor_at, floor)) = all.iter().enumerate().find_map(|(i, step)| {
        command_lines(step)
            .find_map(|line| line.split("--fail-under-lines ").nth(1))
            .map(|rest| (i, rest.split_whitespace().next().unwrap_or("").to_owned()))
    }) else {
        problems.push("no step runs `cargo llvm-cov report --fail-under-lines <N>`".into());
        return problems;
    };
    match floor.parse::<f64>() {
        Ok(floor) if floor >= MIN_FLOOR => {}
        Ok(floor) => problems.push(format!(
            "the coverage floor is {floor}%, under {MIN_FLOOR}%: was it lowered by mistake?"
        )),
        Err(e) => problems.push(format!("the floor `{floor}` is not a number: {e}")),
    }
    if floor_at < summary_at {
        problems.push(
            "the floor is checked after the summary, so a drop is shown before it fails".into(),
        );
    }
    for escape in failure_escapes(workflow, &job, &all[floor_at]) {
        problems.push(format!(
            "the floor step has {escape}: a drop must fail the job on every run"
        ));
    }
    // Its name and its one command, and nothing else in the step: a step
    // `env:` that redefines `COVERAGE_IGNORE`, or any other key, can change
    // the figure the command reads.
    if let Ok(step) = yaml::step_mapping(&all[floor_at]) {
        let others: Vec<String> = step
            .keys()
            .map(yaml::text)
            .filter(|key| key != "name" && key != "run")
            .collect();
        if !others.is_empty() {
            problems.push(format!(
                "the floor step holds its name and its command and nothing else, but has \
                 {others:?}: a step `env:` can redefine COVERAGE_IGNORE, and with it the \
                 figure the floor reads"
            ));
        }
    }
    // One command, which reads the packages the runs measured, so its
    // status is the floor's verdict and nothing else's.
    let expected = format!(
        "cargo llvm-cov report --workspace --ignore-filename-regex \"$COVERAGE_IGNORE\" \
         --fail-under-lines {floor}"
    );
    let script = run_script(&all[floor_at]);
    if script != [expected.as_str()] {
        problems.push(format!(
            "the floor step runs `{expected}` and nothing else: found {script:?}"
        ));
    }
    problems
}

#[test]
fn code_coverage_measures_library_code_and_holds_a_floor() {
    let problems = coverage_problems(&rust_yml(), library_files());
    assert!(
        problems.is_empty(),
        "rust.yml's Code Coverage job:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_coverage_check_catches_what_it_claims_to() {
    let workflow = rust_yml();
    let files = library_files();
    assert!(coverage_problems(&workflow, files).is_empty());
    let floor = "        run: cargo llvm-cov report --workspace --ignore-filename-regex \
                 \"$COVERAGE_IGNORE\" --fail-under-lines ";
    let broken = [
        // The floor's failure ignored, by the step or by the job.
        workflow.replace(floor, &format!("        continue-on-error: true\n{floor}")),
        workflow.replace(
            "    name: Code Coverage\n",
            "    name: Code Coverage\n    continue-on-error: true\n",
        ),
        // ... or the step skipped, or its job.
        workflow.replace(
            floor,
            &format!("        if: github.event_name == 'push'\n{floor}"),
        ),
        workflow.replace(
            "    name: Code Coverage\n",
            "    name: Code Coverage\n    if: github.event_name != 'pull_request'\n",
        ),
        // The floor dropped to nothing.
        workflow.replace("--fail-under-lines 93", "--fail-under-lines 0"),
        // The floor, or the summary, counting test code again.
        workflow.replace(
            "--ignore-filename-regex \"$COVERAGE_IGNORE\" --fail-under-lines",
            "--fail-under-lines",
        ),
        workflow.replace(
            "--ignore-filename-regex \"$COVERAGE_IGNORE\" --summary-only",
            "--summary-only",
        ),
        // A pattern that keeps the property tests, or the fuzzing entry
        // points, in the figure.
        workflow.replace("/src/(proptest_|", "/src/("),
        workflow.replace("|fuzzing)'", ")'"),
        // A pattern that leaves library code out.
        workflow.replace("|fuzzing)'", "|fuzzing|antigravity/)'"),
        // No pattern at all.
        workflow.replace("      COVERAGE_IGNORE: '", "      COVERAGE_SKIP: '"),
        // M3: the floor's failure masked in the shell, in each spelling.
        workflow.replace(
            "--fail-under-lines 93\n",
            "--fail-under-lines 93 || true\n",
        ),
        workflow.replace("--fail-under-lines 93\n", "--fail-under-lines 93; true\n"),
        workflow.replace("--fail-under-lines 93\n", "--fail-under-lines 93 || :\n"),
        workflow.replace(
            "--fail-under-lines 93\n",
            "--fail-under-lines 93 || echo \"below the floor\"\n",
        ),
        workflow.replace(
            &format!("{floor}93\n"),
            &format!(
                "        run: |\n          set +e\n          {}93\n",
                floor.trim_start().trim_start_matches("run: ")
            ),
        ),
        // ... or skipped with a job it needs, or run under another shell.
        workflow.replace(
            "    name: Code Coverage\n",
            "    name: Code Coverage\n    needs: captured-fixtures\n",
        ),
        workflow.replace(floor, &format!("        shell: bash {{0}}\n{floor}")),
        // ... or a flag beside it that reads another figure.
        workflow.replace(
            "--fail-under-lines 93\n",
            "--fail-under-lines 93 --ignore-filename-regex '.*'\n",
        ),
        // P1: an `|| true` YAML joins to the floor's plain `run:` from a
        // deeper continuation line, or a folded block that runs the floor
        // as one line with it.
        workflow.replace(
            "--fail-under-lines 93\n",
            "--fail-under-lines 93\n          || true\n",
        ),
        workflow.replace(
            &format!("{floor}93\n"),
            &format!(
                "        run: >\n          {}93\n          || true\n",
                floor.trim_start().trim_start_matches("run: ")
            ),
        ),
        // P4, P5: `continue-on-error` written so the text never matched
        // it, on the floor step or on the job.
        workflow.replace(floor, &format!("        \"continue-on-error\": true\n{floor}")),
        workflow.replace(floor, &format!("        continue-on-error : true\n{floor}")),
        workflow.replace(
            "    name: Code Coverage\n",
            "    name: Code Coverage\n    \"continue-on-error\": true\n",
        ),
        workflow.replace(
            "    name: Code Coverage\n",
            "    name: Code Coverage\n    continue-on-error : true\n",
        ),
        // An env on the floor step that redefines the pattern the floor
        // reads, so it counts another figure; or the job's pattern moved
        // out of its env, where the text still found it.
        workflow.replace(
            "      - name: Enforce the coverage floor\n",
            "      - name: Enforce the coverage floor\n        env:\n          COVERAGE_IGNORE: '(src/antigravity|src/interactions)/'\n",
        ),
        workflow.replace(
            "    runs-on: ubuntu-latest\n    env:\n      # Library files only",
            "    runs-on: ubuntu-latest\n    outputs:\n      # Library files only",
        ),
        // A mask in an expression, which the runner splices in.
        workflow.replace(
            "--fail-under-lines 93\n",
            "--fail-under-lines 93 ${{ '|| true' }}\n",
        ),
        // A failing test in a coverage run passed over.
        workflow.replace(
            "cargo llvm-cov nextest --workspace --features antigravity --no-report\n",
            "cargo llvm-cov nextest --workspace --features antigravity --no-report || true\n",
        ),
        workflow.replace(
            "        run: |\n          set -euo pipefail\n          cargo llvm-cov clean --workspace\n",
            "        run: |\n          set -eo pipefail\n          cargo llvm-cov clean --workspace\n",
        ),
        workflow.replace(
            "cargo llvm-cov nextest --workspace --features antigravity --no-report\n",
            "cargo llvm-cov nextest --workspace --features antigravity --no-report || exit \"0\"\n",
        ),
        // A shell for the whole workflow that never runs a script.
        workflow.replace(
            "\njobs:\n",
            "\ndefaults:\n  run:\n    shell: true {0}\n\njobs:\n",
        ),
        // The antigravity feature, or the scripted suite, gone.
        workflow.replace(
            "cargo llvm-cov nextest --workspace --features antigravity --no-report\n",
            "cargo llvm-cov nextest --workspace --no-report\n",
        ),
        workflow.replace(
            " --run-ignored all -E 'binary(antigravity_scripted)'\n          cargo llvm-cov report",
            "\n          cargo llvm-cov report",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !coverage_problems(text, files).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// The module walk finds the test-only files the pattern must match, and
/// keeps the library's own.
#[test]
fn the_module_walk_tells_test_only_files_from_library_code() {
    let files = library_files();
    let test_only = |file: &str| {
        files
            .iter()
            .find(|(found, _)| found == file)
            .unwrap_or_else(|| panic!("the walk did not reach {file}"))
            .1
    };
    for file in [
        "src/proptest_tests.rs",
        "src/proptest_stream.rs",
        "src/coverage_oracle/mod.rs",
        "src/coverage_oracle/request/bodies.rs",
        "src/test_subscriber.rs",
        "src/fuzzing.rs",
        "src/fuzzing_tests.rs",
        "src/fuzzing_wire.rs",
        "src/request/tests.rs",
        "src/antigravity/process_tree_tests.rs",
    ] {
        assert!(test_only(file), "{file} is compiled only in a test build");
    }
    for file in [
        "src/lib.rs",
        "src/client.rs",
        "src/antigravity/process_tree.rs",
        "src/request_builder/auto_functions/round.rs",
        "genai-rs-macros/src/schema.rs",
    ] {
        assert!(!test_only(file), "{file} is library code");
    }
    for (predicate, only) in [
        ("test", true),
        ("any(fuzzing, test)", true),
        ("all(test, unix)", true),
        ("any(target_os = \"linux\", test)", false),
        ("unix", false),
        ("not(test)", false),
        ("not(not(test))", true),
        (
            "all(unix, not(any(target_os = \"linux\", target_os = \"macos\")))",
            false,
        ),
        ("any (fuzzing , test)", true),
    ] {
        assert_eq!(test_build_only(predicate), only, "{predicate}");
    }
}
