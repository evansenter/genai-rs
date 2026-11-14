//! The fuzz run fails its job, and reads the committed seeds (#504, K-8).
//! `Fuzz Gate`, the check `fuzz.yml` names for branch protection, passes
//! when `needs.fuzz.result` is `success`, so a fuzz run whose failure
//! passes its job passes the gate too. The gate's verdict and the path
//! check that decides whether the targets run are pinned whole (#504,
//! L-4): each could pass a crashing matrix, or stop fuzzing, with the fuzz
//! run itself untouched.

use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use super::shell::script_lines;
use super::{escapes, jobs, read, root, run_script, step_name, steps, workflow_escapes, yaml};

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
        // ... the same from an expression the runner splices in, or a
        // `cargo` that runs `true`: an alias in POSIX mode.
        workflow.replace(run_end, &run_end.replace("10\n", "10 ${{ '||' }} true\n")),
        workflow.replace(run_end, &run_end.replace("10\n", "10 ${{ '|| true' }}\n")),
        workflow.replace(
            "          set -euo pipefail\n          case \"$SECONDS_PER_TARGET\" in",
            "          set -euo pipefail\n          set -o posix\n          BASH_ALIASES[cargo]=true\n          case \"$SECONDS_PER_TARGET\" in",
        ),
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

/// `fuzz.yml`'s path check, the `changes` job, as it must read: on a pull
/// request, `fuzz` is `true` when the merge changes what a target builds
/// (the crate's sources, of `src/antigravity/` only the harness protocol
/// `harness_output_event` reads, the macros crate, `Cargo.toml`, the fuzz
/// crate with its lockfile, and this workflow), and `false` otherwise; on
/// any other event, or when it cannot tell, `true`. The job's output is
/// what the fuzz job's `if:` and Fuzz Gate read. A `run:` value is compared
/// by its command lines ([`script_lines`]: comments dropped), every other
/// key and value as YAML reads it.
const PATH_CHECK_JOB: &str = r##"
name: Fuzzed Paths
runs-on: ubuntu-latest
outputs:
  fuzz: ${{ steps.paths.outputs.fuzz }}
steps:
  - uses: actions/checkout@v4
    if: github.event_name == 'pull_request'
    with:
      fetch-depth: 2
  - name: Check whether a fuzzed path changed
    id: paths
    env:
      EVENT: ${{ github.event_name }}
    run: |
      set -euo pipefail
      if [ "$EVENT" != pull_request ] || ! git rev-parse -q --verify 'HEAD^2' >/dev/null; then
        echo "fuzz=true" >> "$GITHUB_OUTPUT"
        exit 0
      fi
      changed=$(git diff --name-only 'HEAD^1' HEAD)
      fuzz=false
      while IFS= read -r path; do
        case "$path" in
          src/antigravity/protocol/*)
            echo "Fuzzed path changed: $path"
            fuzz=true
            ;;
          src/antigravity/*) ;;
          src/*|genai-rs-macros/*|Cargo.toml|fuzz/*|.github/workflows/fuzz.yml)
            echo "Fuzzed path changed: $path"
            fuzz=true
            ;;
        esac
      done <<< "$changed"
      echo "fuzz=$fuzz" >> "$GITHUB_OUTPUT"
"##;

/// `fuzz.yml`'s `fuzz-gate` job, the `Fuzz Gate` check, as it must read:
/// it runs after both jobs whatever they did (`always()`), and fails when
/// the path check did not succeed, passes when no target had to run, and
/// otherwise passes only when the matrix succeeded. Compared as
/// [`PATH_CHECK_JOB`] is.
const FUZZ_GATE_JOB: &str = r##"
name: Fuzz Gate
needs: [changes, fuzz]
if: always()
runs-on: ubuntu-latest
steps:
  - name: Pass when every target ran clean, or none had to run
    env:
      CHANGES_RESULT: ${{ needs.changes.result }}
      FUZZ_NEEDED: ${{ needs.changes.outputs.fuzz }}
      FUZZ_RESULT: ${{ needs.fuzz.result }}
    run: |
      set -euo pipefail
      if [ "$CHANGES_RESULT" != success ]; then
        echo "::error::The path check ended '$CHANGES_RESULT', so whether to fuzz is unknown."
        exit 1
      fi
      if [ "$FUZZ_NEEDED" != true ]; then
        echo "No fuzzed path changed; the targets did not run."
        exit 0
      fi
      if [ "$FUZZ_RESULT" != success ]; then
        echo "::error::The fuzz matrix ended '$FUZZ_RESULT'. A failed target uploads its input as the fuzz-<target>-crash artifact."
        exit 1
      fi
      echo "Every fuzz target ran clean."
"##;

/// `job` as YAML reads it, each step's `run:` value replaced by its command
/// lines, so a comment in a script changes nothing and every other change
/// does.
fn pinned_form(job: &str) -> Result<Hash, String> {
    let mut job = yaml::job_mapping(job)?;
    if let Some(Yaml::Array(steps)) = job.get_mut(&Yaml::String("steps".into())) {
        for step in steps.iter_mut() {
            if let Some(run) = step
                .as_mut_hash()
                .and_then(|step| step.get_mut(&Yaml::String("run".into())))
            {
                let lines = script_lines(&yaml::text(run));
                *run = Yaml::Array(lines.into_iter().map(Yaml::String).collect());
            }
        }
    }
    Ok(job)
}

/// Problems with `fuzz.yml`'s path check and Fuzz Gate, empty when each
/// job is its pin ([`PATH_CHECK_JOB`], [`FUZZ_GATE_JOB`]) and nothing in
/// the workflow changes how their scripts run ([`workflow_escapes`]: a
/// workflow-level `defaults:`, or a variable bash reads at start). A pinned
/// job holds every key and step it may have, so nothing in it can skip a
/// step, ignore a failure (`continue-on-error`) or set a variable a step
/// reads; a change to either job is a change to its pin, with the reason.
fn fuzz_gate_problems(workflow: &str) -> Vec<String> {
    let mut problems: Vec<String> = yaml::structure_problems(workflow)
        .into_iter()
        .map(|problem| format!("fuzz.yml: {problem}"))
        .collect();
    problems.extend(
        workflow_escapes(workflow)
            .into_iter()
            .map(|escape| format!("fuzz.yml has {escape}")),
    );
    let found = jobs(workflow);
    for (id, pin) in [("changes", PATH_CHECK_JOB), ("fuzz-gate", FUZZ_GATE_JOB)] {
        let Some((_, job)) = found.iter().find(|(found, _)| found == id) else {
            problems.push(format!("fuzz.yml has no {id} job"));
            continue;
        };
        let want = pinned_form(pin).expect("the pin is a job");
        match pinned_form(job) {
            Ok(read) if read == want => {}
            Ok(read) => problems.push(format!(
                "fuzz.yml's {id} job decides whether the fuzz matrix counts, so it must read \
                 as its pin in tests/ci_workflows/fuzz.rs; change both, with the reason:\n  \
                 found {}\n  want  {}",
                yaml::text(&Yaml::Hash(read)),
                yaml::text(&Yaml::Hash(want))
            )),
            Err(e) => problems.push(format!("fuzz.yml's {id} job: {e}")),
        }
    }
    problems
}

/// Fuzz Gate's verdict and the path check once held nothing: a gate that
/// exits 0 on a failed matrix, an `|| true` on its last line, a path check
/// that writes `fuzz=false` in doubt or drops an input, and
/// `continue-on-error` on either job each passed every guard (the #504
/// grade of 2026-11-14, L-4).
#[test]
fn fuzz_gate_and_the_path_check_read_as_pinned() {
    let problems = fuzz_gate_problems(&fuzz_yml());
    assert!(
        problems.is_empty(),
        "fuzz.yml's path check and gate:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_fuzz_gate_check_catches_what_it_claims_to() {
    let workflow = fuzz_yml();
    assert!(fuzz_gate_problems(&workflow).is_empty());
    let gate = "    name: Fuzz Gate\n";
    let changes = "    name: Fuzzed Paths\n";
    let broken = [
        // The gate passes a failed matrix: its exit, its test, a mask on
        // its last line, or `continue-on-error` on its job, quoted too.
        workflow.replace(
            "fuzz-<target>-crash artifact.\"\n            exit 1\n",
            "fuzz-<target>-crash artifact.\"\n            exit 0\n",
        ),
        workflow.replace(
            "if [ \"$FUZZ_RESULT\" != success ]; then",
            "if [ \"$FUZZ_RESULT\" = failure-never ]; then",
        ),
        workflow.replace(
            "echo \"Every fuzz target ran clean.\"\n",
            "echo \"Every fuzz target ran clean.\" || true\n",
        ),
        workflow.replace(gate, &format!("{gate}    continue-on-error: true\n")),
        workflow.replace(gate, &format!("{gate}    \"continue-on-error\": true\n")),
        // ... reads a verdict it was not given, runs only when both jobs
        // passed, or a step before it sets what it reads.
        workflow.replace(
            "FUZZ_RESULT: ${{ needs.fuzz.result }}",
            "FUZZ_RESULT: success",
        ),
        workflow.replace(
            "    needs: [changes, fuzz]\n    if: always()\n",
            "    needs: [changes, fuzz]\n    if: success()\n",
        ),
        workflow.replace(
            "      - name: Pass when every target ran clean, or none had to run\n",
            "      - run: echo \"FUZZ_RESULT=success\" >> \"$GITHUB_ENV\"\n      - name: Pass when every target ran clean, or none had to run\n",
        ),
        // The path check stops fuzzing: `fuzz=false` in doubt (scheduled
        // and manual runs), an input dropped, the verdict forced before it
        // is recorded, or a job output or a step that never reports it.
        workflow.replace(
            "            echo \"fuzz=true\" >> \"$GITHUB_OUTPUT\"\n            exit 0\n",
            "            echo \"fuzz=false\" >> \"$GITHUB_OUTPUT\"\n            exit 0\n",
        ),
        workflow.replace(
            "              src/*|genai-rs-macros/*|",
            "              genai-rs-macros/*|",
        ),
        workflow.replace("|genai-rs-macros/*|Cargo.toml|", "|genai-rs-macros/*|"),
        workflow.replace(
            "          echo \"fuzz=$fuzz\" >> \"$GITHUB_OUTPUT\"\n",
            "          fuzz=false\n          echo \"fuzz=$fuzz\" >> \"$GITHUB_OUTPUT\"\n",
        ),
        workflow.replace(changes, &format!("{changes}    continue-on-error: true\n")),
        workflow.replace(
            "      fuzz: ${{ steps.paths.outputs.fuzz }}\n",
            "      fuzz: 'false'\n",
        ),
        workflow.replace(
            "        id: paths\n",
            "        id: paths\n        \"if\": github.event_name == 'schedule'\n",
        ),
        // A shell for the whole workflow that never runs a script.
        workflow.replace(
            "\njobs:\n",
            "\ndefaults:\n  run:\n    shell: true {0}\n\njobs:\n",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !fuzz_gate_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
    // A comment in a pinned script, or a key quoted, is no change.
    for same in [
        workflow.replace(
            "          set -euo pipefail\n          if [ \"$CHANGES_RESULT\"",
            "          set -euo pipefail\n          # A comment.\n          if [ \"$CHANGES_RESULT\"",
        ),
        workflow.replace(
            "    needs: [changes, fuzz]\n    if: always()\n",
            "    needs: [changes, fuzz]\n    \"if\": always()\n",
        ),
    ] {
        assert_ne!(same, workflow);
        assert!(fuzz_gate_problems(&same).is_empty(), "{same}");
    }
}
