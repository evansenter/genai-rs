//! The Code Coverage job reports somewhere every run, measures the
//! `antigravity` feature with its scripted suite, and fails under a floor
//! (#504).

use super::{command_lines, jobs, read, root, steps};

/// The Code Coverage job's steps.
fn coverage_steps() -> Vec<String> {
    let rust = read(&root().join(".github/workflows/rust.yml"));
    let (_, job) = jobs(&rust)
        .into_iter()
        .find(|(id, _)| id == "coverage")
        .expect("rust.yml has a coverage job");
    steps(&job)
}

/// The lowest floor the guard accepts: well under the measured value, so
/// only a floor dropped by mistake (or removed by setting it to 0) trips
/// it, not the floor's own small moves.
const MIN_FLOOR: f64 = 90.0;

/// With no `CODECOV_TOKEN` the Codecov upload used to fail quietly under
/// `fail_ci_if_error: false`, so the job passed with coverage reported
/// nowhere. The job writes llvm-cov's summary to the job summary on every
/// run, and the upload, when it runs, fails the job on an error.
#[test]
fn code_coverage_reports_a_summary_and_fails_a_failed_upload() {
    let all = coverage_steps();

    let report_line = |step: &String| {
        command_lines(step)
            .find(|line| line.contains("cargo llvm-cov report") && line.contains("--summary-only"))
            .map(str::to_owned)
    };
    let (summary, report) = all
        .iter()
        .find_map(|step| report_line(step).map(|line| (step, line)))
        .expect("a step runs `cargo llvm-cov report --summary-only`");
    assert!(
        summary.contains(">> \"$GITHUB_STEP_SUMMARY\"") && !summary.contains("        if:"),
        "the coverage summary goes to the job summary on every run:\n{summary}"
    );

    // `cargo llvm-cov report` selects packages afresh: without the run's
    // `--workspace` it reports the root package alone, so the summary would
    // leave out genai-rs-macros, which the uploaded lcov holds.
    let run = all
        .iter()
        .flat_map(|step| command_lines(step).collect::<Vec<_>>())
        .find(|line| line.contains("cargo llvm-cov nextest"))
        .expect("the coverage job runs `cargo llvm-cov nextest`");
    assert_eq!(
        run.contains("--workspace"),
        report.contains("--workspace"),
        "the summary's `cargo llvm-cov report` must select the packages the run \
         measured (`--workspace`):\n  run:    {run}\n  report: {report}"
    );

    let uploads: Vec<&String> = all
        .iter()
        .filter(|step| step.contains("codecov/codecov-action"))
        .collect();
    assert!(
        !uploads.is_empty(),
        "no Codecov upload step in the coverage job"
    );
    for upload in uploads {
        assert!(
            upload.contains("fail_ci_if_error: true"),
            "a Codecov upload must fail the job on an error:\n{upload}"
        );
        assert!(
            upload.contains("if: steps.codecov.outputs.upload == 'true'"),
            "the upload runs only when the token is set:\n{upload}"
        );
    }
}

/// Coverage once left out the `antigravity` feature, about a fifth of the
/// source, and had no floor, so a drop passed (#504). The job measures the
/// feature in both of its runs, the unit tests and the key-free scripted
/// harness suite (which tests most of it), and then fails under a floor
/// near the measured value, in a step after the summary and with no `if:`.
#[test]
fn code_coverage_measures_antigravity_and_holds_a_floor() {
    let all = coverage_steps();
    let lines: Vec<String> = all
        .iter()
        .flat_map(|step| command_lines(step).map(str::to_owned).collect::<Vec<_>>())
        .collect();

    let runs: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("cargo llvm-cov nextest"))
        .collect();
    assert!(
        runs.len() >= 2,
        "the coverage job runs the unit tests and the scripted suite: {runs:?}"
    );
    for run in &runs {
        assert!(
            run.contains("--workspace") && run.contains("--features antigravity"),
            "every coverage run measures the workspace with the antigravity feature: {run}"
        );
        assert!(
            run.contains("--no-report"),
            "the runs feed one report, so each is --no-report: {run}"
        );
    }
    assert!(
        runs.iter()
            .any(|run| run.contains("--run-ignored all")
                && run.contains("binary(antigravity_scripted)")),
        "the scripted harness suite runs under coverage: {runs:?}"
    );
    let scripted = all
        .iter()
        .find(|step| step.contains("binary(antigravity_scripted)"))
        .expect("a step runs the scripted suite");
    assert!(
        scripted.contains("GEMINI_API_KEY: \"\""),
        "the scripted suite runs with the key blanked:\n{scripted}"
    );
    assert!(
        all.iter().any(|step| command_lines(step)
            .any(|line| line.contains("pip install google-antigravity=="))),
        "the coverage job installs the pinned harness the scripted suite drives"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("cargo llvm-cov clean --workspace")),
        "the job starts from a clean profile, so a cached run cannot merge into this one"
    );

    let summary_at = all
        .iter()
        .position(|step| step.contains("--summary-only"))
        .expect("a summary step");
    let (floor_at, floor) = all
        .iter()
        .enumerate()
        .find_map(|(i, step)| {
            command_lines(step)
                .find_map(|line| line.split("--fail-under-lines ").nth(1))
                .map(|rest| (i, rest.split_whitespace().next().unwrap_or("").to_owned()))
        })
        .expect("a step runs `cargo llvm-cov report --fail-under-lines <N>`");
    let floor: f64 = floor
        .parse()
        .unwrap_or_else(|e| panic!("the floor `{floor}` is not a number: {e}"));
    assert!(
        floor >= MIN_FLOOR,
        "the coverage floor is {floor}%, under {MIN_FLOOR}%: was it lowered by mistake?"
    );
    assert!(
        floor_at > summary_at,
        "the floor is checked after the summary, so a drop is shown before it fails"
    );
    assert!(
        !all[floor_at].contains("        if:"),
        "the floor runs on every run:\n{}",
        all[floor_at]
    );
    let floor_line = command_lines(&all[floor_at])
        .find(|line| line.contains("--fail-under-lines"))
        .unwrap_or_default();
    assert!(
        floor_line.contains("--workspace"),
        "the floor reads the packages the runs measured: {floor_line}"
    );
}
