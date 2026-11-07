//! A red push to main escalates, as a red scheduled run does.
//!
//! Nothing waits on a push to main, and most of the cross-platform matrix
//! runs only there and on the schedule, so main's Windows legs failed on
//! eleven pushes in a row before anyone looked (#504). `rust.yml` therefore has
//! a job that opens a `ci-health-escalation` issue when a push to main goes
//! red and closes it on the next green one, through the scripts the
//! scheduled runs use, with `ESCALATION_TRIGGER: push` so the push keeps an
//! issue of its own (D-NEXT-WP14).

use std::collections::BTreeSet;

use super::{command_lines, jobs, read, root, steps};

/// The job's one-line `needs: [...]`, if it has one.
fn needs(job: &str) -> Option<BTreeSet<String>> {
    let list = job
        .lines()
        .find_map(|line| line.strip_prefix("    needs: ["))?;
    Some(
        list.trim_end()
            .trim_end_matches(']')
            .split(',')
            .map(|job| job.trim().to_string())
            .collect(),
    )
}

/// The job's own `if:` (four-space indent; a step's is deeper).
fn job_condition(job: &str) -> Option<&str> {
    job.lines().find_map(|line| line.strip_prefix("    if: "))
}

/// Whether a job runs only for a push to main, whatever its other jobs did.
fn runs_after_every_push_to_main(condition: &str) -> bool {
    condition.contains("always()")
        && condition.contains("github.event_name == 'push'")
        && condition.contains("github.ref == 'refs/heads/main'")
}

/// The step of `job` that calls `script`, if there is one.
fn step_calling<'a>(job_steps: &'a [String], script: &str) -> Option<&'a String> {
    job_steps
        .iter()
        .find(|step| command_lines(step).any(|line| line.contains(script)))
}

/// Problems with `rust.yml`'s main-push escalation, empty when it is whole:
/// one job runs after every push to main, needs every job but itself and
/// `scheduled-run-status`, reports when one of them failed or was cancelled
/// and resolves otherwise, both under `ESCALATION_TRIGGER: push`.
fn main_push_escalation_problems(workflow: &str) -> Vec<String> {
    let all = jobs(workflow);
    let escalating: Vec<&(String, String)> = all
        .iter()
        .filter(|(_, body)| job_condition(body).is_some_and(runs_after_every_push_to_main))
        .filter(|(_, body)| {
            command_lines(body).any(|line| line.contains("report_scheduled_failure.sh"))
        })
        .collect();
    let [(id, body)] = escalating.as_slice() else {
        return vec![format!(
            "want exactly one job with `if: always() && github.event_name == 'push' && \
             github.ref == 'refs/heads/main' ...` that calls report_scheduled_failure.sh; \
             found {:?}",
            escalating.iter().map(|(id, _)| id).collect::<Vec<_>>()
        )];
    };

    let mut problems = Vec::new();
    let mut expected: BTreeSet<String> = all.iter().map(|(id, _)| id.clone()).collect();
    expected.remove(id.as_str());
    // It needs this job, so this job cannot need it back.
    expected.remove("scheduled-run-status");
    match needs(body) {
        Some(found) if found == expected => {}
        Some(found) => problems.push(format!(
            "{id} needs {found:?}, not every job but itself and scheduled-run-status: {expected:?}"
        )),
        None => problems.push(format!("{id} has no one-line `needs: [...]`")),
    }

    let job_steps = steps(body);
    for (script, condition) in [
        (
            "report_scheduled_failure.sh",
            "if: contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled')",
        ),
        (
            "resolve_scheduled_failure.sh",
            "if: ${{ !contains(needs.*.result, 'failure') && !contains(needs.*.result, 'cancelled') }}",
        ),
    ] {
        let Some(step) = step_calling(&job_steps, script) else {
            problems.push(format!("{id} never calls {script}"));
            continue;
        };
        if !step.lines().any(|line| line.trim() == condition) {
            problems.push(format!(
                "{id}: the {script} step is not gated by `{condition}`"
            ));
        }
        if !step
            .lines()
            .any(|line| line.trim() == "ESCALATION_TRIGGER: push")
        {
            problems.push(format!(
                "{id}: the {script} step has no `ESCALATION_TRIGGER: push`, so it would \
                 file under, or close, the scheduled run's issue"
            ));
        }
    }
    problems
}

#[test]
fn a_red_push_to_main_escalates() {
    let workflow = read(&root().join(".github/workflows/rust.yml"));
    let problems = main_push_escalation_problems(&workflow);
    assert!(
        problems.is_empty(),
        "rust.yml must escalate a red push to main (see main-push-status):\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_escalation_check_catches_what_it_claims_to() {
    let workflow = read(&root().join(".github/workflows/rust.yml"));
    assert!(main_push_escalation_problems(&workflow).is_empty());

    let broken = [
        // The job gone, or gated so that it never runs.
        workflow.replace("github.event_name == 'push' && github.ref", "false && github.ref"),
        // A job left out of its needs.
        workflow.replace(
            "cross-platform, coverage, build-metrics]\n    if: always() && github.event_name == 'push'",
            "coverage, build-metrics]\n    if: always() && github.event_name == 'push'",
        ),
        // Filing under the scheduled run's issue.
        workflow.replacen("          ESCALATION_TRIGGER: push\n", "", 1),
        // Resolving on a red run.
        workflow.replace(
            "if: ${{ !contains(needs.*.result, 'failure') && !contains(needs.*.result, 'cancelled') }}\n        env:\n          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n          SELF_WORKFLOW_NAME: ${{ github.workflow }}\n          ESCALATION_TRIGGER: push",
            "if: always()\n        env:\n          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n          SELF_WORKFLOW_NAME: ${{ github.workflow }}\n          ESCALATION_TRIGGER: push",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !main_push_escalation_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}
