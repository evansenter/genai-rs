//! The Code Coverage job reports somewhere every run (#504).

use super::{command_lines, jobs, read, root, steps};

/// With no `CODECOV_TOKEN` the Codecov upload used to fail quietly under
/// `fail_ci_if_error: false`, so the job passed with coverage reported
/// nowhere. The job writes llvm-cov's summary to the job summary on every
/// run, and the upload, when it runs, fails the job on an error.
#[test]
fn code_coverage_reports_a_summary_and_fails_a_failed_upload() {
    let rust = read(&root().join(".github/workflows/rust.yml"));
    let (_, job) = jobs(&rust)
        .into_iter()
        .find(|(id, _)| id == "coverage")
        .expect("rust.yml has a coverage job");
    let all = steps(&job);

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
