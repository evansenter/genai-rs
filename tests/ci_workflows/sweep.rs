//! The daily API surface sweep's own checks: the spec watch and the issue
//! pointer check run on every scheduled run, and a failure in either is
//! seen (#504).

use super::{command_lines, escapes, is_scheduled, jobs, read, root, steps};

/// The sweep workflow, its sweep job and the job's steps, from a scheduled
/// `api-surface-sweep.yml` that can write issues.
fn sweep_job() -> (String, String, Vec<String>) {
    let workflow = read(&root().join(".github/workflows/api-surface-sweep.yml"));
    assert!(
        is_scheduled(&workflow),
        "api-surface-sweep.yml runs on a schedule"
    );
    assert!(
        workflow.contains("\n  issues: write\n"),
        "the sweep's token can open and update its issues"
    );
    let job = jobs(&workflow)
        .into_iter()
        .find(|(id, _)| id == "sweep")
        .map(|(_, body)| body)
        .expect("api-surface-sweep.yml has a sweep job");
    let job_steps = steps(&job);
    (workflow, job, job_steps)
}

/// The step that runs `script`, checked to run whatever the steps before
/// it did (`!cancelled()`), to fail the run when the script does (nothing
/// but that `if:` from `escapes`: no `continue-on-error`, no shell-level
/// mask such as `|| true`), and to have the job's token for its `gh` calls.
/// Panics naming what is missing.
fn step_running((workflow, job, steps): &(String, String, Vec<String>), script: &str) -> String {
    let step = steps
        .iter()
        .find(|step| {
            command_lines(step).any(|line| line.starts_with(&format!(".github/scripts/{script}")))
        })
        .unwrap_or_else(|| panic!("the sweep job has no step that runs {script}"))
        .clone();
    let lines: Vec<&str> = command_lines(&step).collect();
    assert!(
        lines.contains(&"if: ${{ !cancelled() }}"),
        "{script} runs whatever the steps before it did:\n{step}"
    );
    let found = escapes(workflow, job, &step, &["if: ${{ !cancelled() }}"]);
    assert!(
        found.is_empty(),
        "a failing {script} fails the run, which escalates, but its step has \
         {found:?}:\n{step}"
    );
    assert!(
        lines.contains(&"GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}"),
        "{script}'s gh calls need the job's token:\n{step}"
    );
    assert!(
        root()
            .join(".github/scripts/tests")
            .join(format!("test_{script}"))
            .exists(),
        "{script} has its harness"
    );
    step
}

/// The published OpenAPI specs drift or move without a release of the
/// bindings, and only the sweep's watch sees it (`watch_spec_urls.sh`):
/// deleting the step, or dropping a committed snapshot from it, would fail
/// nothing else.
#[test]
fn the_sweep_watches_both_published_specs() {
    let step = step_running(&sweep_job(), "watch_spec_urls.sh");
    for snapshot in [
        "tests/fixtures/api_surface/openapi-v1beta.json",
        "tests/fixtures/api_surface/openapi-v1.json",
    ] {
        assert!(
            command_lines(&step).any(|line| line.starts_with(snapshot)),
            "the spec watch reads {snapshot}:\n{step}"
        );
        assert!(root().join(snapshot).exists(), "{snapshot} is committed");
    }
}

/// D-051: a pointer names an open issue only, and an issue's state needs
/// the network, so the sweep's `check_issue_pointers.sh` is the only check
/// that a pointer still does. It reads the repository's issues, so it
/// names the repository.
#[test]
fn the_sweep_flags_pointers_to_closed_issues() {
    let step = step_running(&sweep_job(), "check_issue_pointers.sh");
    assert!(
        command_lines(&step).any(|line| line == "GH_REPO: ${{ github.repository }}"),
        "the pointer check looks issues up in this repository:\n{step}"
    );
    assert!(
        !command_lines(&step).any(|line| line.contains("--print")),
        "the sweep's run updates the rolling issue rather than only printing:\n{step}"
    );
}
