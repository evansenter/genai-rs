//! The fuzz run fails its job, and reads the committed seeds (#504, K-8).
//! `Fuzz Gate`, the check `fuzz.yml` names for branch protection, passes
//! when `needs.fuzz.result` is `success`, so a fuzz run whose failure
//! passes its job passes the gate too.

use super::{escapes, jobs, read, root, run_script, step_name, steps, yaml};

/// What the fuzz job may carry beyond running on every run: it needs the
/// path check, and runs when that found a fuzzed path (or on schedule and
/// on demand, when it always does). The gate reads the path check's
/// verdict too, so a skipped matrix passes it only when no target had to
/// run.
const FUZZ_ALLOWED: [&str; 2] = ["needs: changes", "if: needs.changes.outputs.fuzz == 'true'"];

/// What the fuzz run reads besides the corpus earlier runs grew: the
/// committed seeds, which it never writes.
const SEEDS: &str = "\"seeds/$TARGET\"";

fn fuzz_yml() -> String {
    read(&root().join(".github/workflows/fuzz.yml"))
}

/// Problems with `fuzz.yml`'s fuzz job, empty when it holds: each `run:`
/// step is held to [`escapes`], with nothing allowed but
/// [`FUZZ_ALLOWED`], and the one `cargo +nightly fuzz run` passes the
/// seeds directory among its corpus directories, before `--`.
fn fuzz_job_problems(workflow: &str) -> Vec<String> {
    let mut problems: Vec<String> = yaml::structure_problems(workflow)
        .into_iter()
        .map(|problem| format!("fuzz.yml: {problem}"))
        .collect();
    let Some((_, job)) = jobs(workflow).into_iter().find(|(id, _)| id == "fuzz") else {
        problems.push("fuzz.yml has no fuzz job".into());
        return problems;
    };
    let mut runs = Vec::new();
    for step in steps(&job)
        .iter()
        .filter(|step| !matches!(yaml::run_value(step), Ok(None)))
    {
        for escape in escapes(workflow, &job, step, &FUZZ_ALLOWED) {
            problems.push(format!(
                "fuzz.yml job fuzz, step `{}`: {escape}",
                step_name(step)
            ));
        }
        runs.extend(
            run_script(step)
                .into_iter()
                .filter(|line| line.contains("cargo +nightly fuzz run")),
        );
    }
    match runs.as_slice() {
        [run] => {
            let corpora = run.split(" -- ").next().unwrap_or_default();
            if !corpora.split_whitespace().any(|word| word == SEEDS) {
                problems.push(format!(
                    "the fuzz run reads the committed seeds ({SEEDS} before `--`): {run}"
                ));
            }
        }
        _ => problems.push(format!(
            "the fuzz job runs `cargo +nightly fuzz run` once: {runs:?}"
        )),
    }
    problems
}

/// A failing target once could pass the fuzz job, and with it Fuzz Gate:
/// an `|| true` after the run, or `continue-on-error` on the job, passed
/// every guard, as did a run that dropped the committed seeds (the #504
/// grade of 2026-11-13, K-8).
#[test]
fn the_fuzz_run_fails_its_job_and_reads_the_seeds() {
    let problems = fuzz_job_problems(&fuzz_yml());
    assert!(
        problems.is_empty(),
        "fuzz.yml's fuzz job:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_fuzz_run_check_catches_what_it_claims_to() {
    let workflow = fuzz_yml();
    let run_end = "-max_total_time=\"$SECONDS_PER_TARGET\" -timeout=10\n";
    let broken = [
        // K-8: the run's failure masked, or the job's ignored.
        workflow.replace(run_end, &run_end.replace("10\n", "10 || true\n")),
        workflow.replace(
            "    name: Fuzz ${{ matrix.target }}\n",
            "    name: Fuzz ${{ matrix.target }}\n    continue-on-error: true\n",
        ),
        workflow.replace(
            "    name: Fuzz ${{ matrix.target }}\n",
            "    name: Fuzz ${{ matrix.target }}\n    \"continue-on-error\": true\n",
        ),
        // ... the seeds dropped, so the run starts from the grown corpus
        // alone.
        workflow.replace(
            "\"$TARGET\" \"corpus/$TARGET\" \"seeds/$TARGET\" --",
            "\"$TARGET\" \"corpus/$TARGET\" --",
        ),
        // ... the script no longer checked, or the run skipped beyond the
        // path check, or the step skipped.
        workflow.replace(
            "        run: |\n          set -euo pipefail\n          case \"$SECONDS_PER_TARGET\" in",
            "        run: |\n          case \"$SECONDS_PER_TARGET\" in",
        ),
        workflow.replace(
            "    if: needs.changes.outputs.fuzz == 'true'\n",
            "    if: needs.changes.outputs.fuzz == 'true' && github.event_name == 'schedule'\n",
        ),
        workflow.replace(
            "      - name: Fuzz for a bounded time\n",
            "      - name: Fuzz for a bounded time\n        if: github.event_name == 'schedule'\n",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !fuzz_job_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}
