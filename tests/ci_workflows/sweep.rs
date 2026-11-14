//! The daily API surface sweep's own checks: the spec watch and the issue
//! pointer check run on every scheduled run, and a failure in either is
//! seen (#504). The pointer check runs in the one mode that writes to its
//! rolling issue, which a local run does not.

use super::{command_lines, escapes, is_scheduled, jobs, read, root, run_script, steps, yaml};

/// The `if:` each of the sweep's checks carries, so it runs whatever the
/// steps before it did.
const NOT_CANCELLED: &str = "if: ${{ !cancelled() }}";

/// The pointer check's command in the sweep: `--update-issue` is the one
/// mode that opens, updates or closes the rolling issue; without it the
/// script only lists what it finds (D-051).
const POINTER_CHECK: &str = ".github/scripts/check_issue_pointers.sh --update-issue";

fn sweep_yml() -> String {
    read(&root().join(".github/workflows/api-surface-sweep.yml"))
}

/// The step of `workflow`'s sweep job that runs `script`, and the problems
/// with it, empty when it holds: it runs whatever the steps before it did
/// (`!cancelled()`, its `if:` as YAML reads it), fails the run when the
/// script does (nothing but that `if:` from `escapes`: no `continue-on-error`, on the step or its job, as
/// YAML reads their keys, no shell-level mask such as `|| true` in the
/// `run:` value YAML reads), and has the job's token for its `gh` calls,
/// in a scheduled workflow whose token can write issues.
fn sweep_step(workflow: &str, script: &str) -> (Option<String>, Vec<String>) {
    let mut problems = Vec::new();
    if !is_scheduled(workflow) {
        problems.push("api-surface-sweep.yml runs on a schedule".to_owned());
    }
    if !workflow.contains("\n  issues: write\n") {
        problems.push("the sweep's token can open and update its issues".to_owned());
    }
    let Some((_, job)) = jobs(workflow).into_iter().find(|(id, _)| id == "sweep") else {
        problems.push("api-surface-sweep.yml has a sweep job".to_owned());
        return (None, problems);
    };
    let Some(step) = steps(&job).into_iter().find(|step| {
        command_lines(step).any(|line| line.starts_with(&format!(".github/scripts/{script}")))
    }) else {
        problems.push(format!("the sweep job has no step that runs {script}"));
        return (None, problems);
    };
    let gate = yaml::step_value(&step, "if").ok().flatten();
    if gate.map(|gate| format!("if: {gate}")).as_deref() != Some(NOT_CANCELLED) {
        problems.push(format!(
            "{script} runs whatever the steps before it did (`{NOT_CANCELLED}`)"
        ));
    }
    for escape in escapes(workflow, &job, &step, &[NOT_CANCELLED]) {
        problems.push(format!(
            "a failing {script} fails the run, which escalates, but its step has {escape}"
        ));
    }
    if yaml::step_env(&step, "GH_TOKEN").as_deref() != Some("${{ secrets.GITHUB_TOKEN }}") {
        problems.push(format!("{script}'s gh calls need the job's token"));
    }
    (Some(step), problems)
}

/// The step that runs `script`, held to [`sweep_step`], with its harness.
/// Panics naming what is missing.
fn step_running(script: &str) -> String {
    let (step, problems) = sweep_step(&sweep_yml(), script);
    assert!(
        problems.is_empty(),
        "the sweep's {script} step:\n  {}\n{step:?}",
        problems.join("\n  ")
    );
    assert!(
        root()
            .join(".github/scripts/tests")
            .join(format!("test_{script}"))
            .exists(),
        "{script} has its harness"
    );
    step.expect("a step with no problems")
}

/// The published OpenAPI specs drift or move without a release of the
/// bindings, and only the sweep's watch sees it (`watch_spec_urls.sh`):
/// deleting the step, or dropping a committed snapshot from it, would fail
/// nothing else.
#[test]
fn the_sweep_watches_both_published_specs() {
    let step = step_running("watch_spec_urls.sh");
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

/// Problems with the sweep's pointer check, empty when it holds: the step
/// [`sweep_step`] holds, which looks issues up in this repository and runs
/// [`POINTER_CHECK`] and nothing else, its `run:` value read as YAML reads
/// it. A run without `--update-issue` lists its findings and leaves the
/// rolling issue alone, so the sweep would report nothing.
fn pointer_step_problems(workflow: &str) -> Vec<String> {
    let (step, mut problems) = sweep_step(workflow, "check_issue_pointers.sh");
    let Some(step) = step else {
        return problems;
    };
    if yaml::step_env(&step, "GH_REPO").as_deref() != Some("${{ github.repository }}") {
        problems.push(format!(
            "the pointer check looks issues up in this repository \
             (`GH_REPO: ${{{{ github.repository }}}}`):\n{step}"
        ));
    }
    let script = run_script(&step);
    if script != ["set -euo pipefail", POINTER_CHECK] {
        problems.push(format!(
            "the sweep runs `{POINTER_CHECK}`, the mode that keeps the rolling issue, and \
             nothing else: found {script:?}"
        ));
    }
    problems
}

/// D-051: a pointer names an open issue only, and an issue's state needs
/// the network, so the sweep's `check_issue_pointers.sh` is the only check
/// that a pointer still does. It reads the repository's issues, so it
/// names the repository, and it alone passes `--update-issue`: a bare run,
/// a local one, writes nothing to GitHub (#504, L-11).
#[test]
fn the_sweep_flags_pointers_to_closed_issues() {
    step_running("check_issue_pointers.sh");
    let problems = pointer_step_problems(&sweep_yml());
    assert!(
        problems.is_empty(),
        "the sweep's pointer check:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_sweep_step_check_catches_what_it_claims_to() {
    let workflow = sweep_yml();
    let watch = "      - name: Compare the published OpenAPI specs with their snapshots\n";
    let pointers = "          .github/scripts/check_issue_pointers.sh --update-issue\n";
    let broken = [
        // P5b: `continue-on-error` on a sweep step or its job, written so
        // the text never matched it.
        (
            "watch_spec_urls.sh",
            workflow.replace(
                watch,
                &format!("{watch}        \"continue-on-error\": true\n"),
            ),
        ),
        (
            "watch_spec_urls.sh",
            workflow.replace(watch, &format!("{watch}        continue-on-error : true\n")),
        ),
        (
            "check_issue_pointers.sh",
            workflow.replace(
                "    name: Diff google-genai generated bindings\n",
                "    name: Diff google-genai generated bindings\n    \"continue-on-error\": true\n",
            ),
        ),
        // ... the failure masked, or the step skipped beyond `!cancelled()`.
        (
            "check_issue_pointers.sh",
            workflow.replace(
                pointers,
                "          .github/scripts/check_issue_pointers.sh --update-issue || true\n",
            ),
        ),
        (
            "watch_spec_urls.sh",
            workflow.replace(
                &format!("{watch}        if: ${{{{ !cancelled() }}}}\n"),
                &format!("{watch}        if: github.event_name == 'schedule'\n"),
            ),
        ),
        // ... or without the job's token.
        (
            "check_issue_pointers.sh",
            workflow.replacen(
                "          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n          GH_REPO:",
                "          GH_REPO:",
                1,
            ),
        ),
    ];
    assert!(sweep_step(&workflow, "watch_spec_urls.sh").1.is_empty());
    for (i, (script, text)) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !sweep_step(text, script).1.is_empty(),
            "mutation {i} went unnoticed"
        );
    }

    // The pointer check run without writing to its issue, by dropping the
    // flag or by a read-only one, or with another command beside it; or
    // looking issues up in another repository.
    assert!(pointer_step_problems(&workflow).is_empty());
    for (i, broken) in [
        workflow.replace(
            pointers,
            "          .github/scripts/check_issue_pointers.sh\n",
        ),
        workflow.replace(
            pointers,
            "          .github/scripts/check_issue_pointers.sh --print\n",
        ),
        workflow.replace(
            pointers,
            &format!("{pointers}          .github/scripts/check_issue_pointers.sh\n"),
        ),
        workflow.replace(
            "          GH_REPO: ${{ github.repository }}\n",
            "          GH_REPO: evansenter/other\n",
        ),
    ]
    .iter()
    .enumerate()
    {
        assert_ne!(broken, &workflow, "pointer mutation {i} did not apply");
        assert!(
            !pointer_step_problems(broken).is_empty(),
            "pointer mutation {i} went unnoticed"
        );
    }
}
