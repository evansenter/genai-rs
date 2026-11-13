//! A release publishes only what CI passed on every platform (#504).

use regex::Regex;

use super::builds::event_matrix;
use super::flakiness::display_names;
use super::{command_lines, failure_escapes, jobs, read, root, steps};

/// Problems with release.yml's push-run gate, empty when it holds.
/// release.yml validates on Ubuntu only, so the tagged commit's push run of
/// rust.yml on main, which runs the Cross Platform legs and the rest, must
/// be green on every job before anything publishes: `validate` runs
/// `check_release_push_run.sh` on the tagged commit, with the token able
/// to read Actions, with nothing the guards know to skip the step or pass
/// its failure ([`failure_escapes`]), and `publish` needs `validate`.
fn push_run_gate_problems(release: &str) -> Vec<String> {
    let all = jobs(release);
    let job = |id: &str| {
        all.iter()
            .find(|(job, _)| job == id)
            .map(|(_, body)| body.clone())
    };
    let (Some(validate), Some(publish)) = (job("validate"), job("publish")) else {
        return vec!["release.yml has no validate job, or no publish job".into()];
    };
    let Some(check) = steps(&validate)
        .into_iter()
        .find(|step| command_lines(step).any(|line| line.contains("check_release_push_run.sh")))
    else {
        return vec!["validate never runs .github/scripts/check_release_push_run.sh".into()];
    };
    let mut problems = Vec::new();
    let call = command_lines(&check)
        .find(|line| line.contains("check_release_push_run.sh"))
        .unwrap_or_default()
        .to_owned();
    if !(call.contains("\"$GITHUB_SHA\"") || call.contains("${{ github.sha }}")) {
        problems.push(format!("the check reads the tagged commit's run: {call}"));
    }
    if !(check.contains("GH_TOKEN: ${{ github.token }}")
        || check.contains("GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}"))
    {
        problems.push(format!(
            "the check's gh calls need the job's token:\n{check}"
        ));
    }
    for escape in failure_escapes(release, &validate, &check) {
        problems.push(format!(
            "the check runs on every release and its failure stops it, but it has {escape}"
        ));
    }
    if !validate.contains("      actions: read") {
        problems.push("validate's token must read Actions for `gh run`".into());
    }
    if !command_lines(&publish).any(|line| line == "needs: validate") {
        problems.push("publish waits for validate (`needs: validate`)".into());
    }
    problems
}

#[test]
fn a_release_publishes_only_a_commit_green_on_its_main_push_run() {
    let release = read(&root().join(".github/workflows/release.yml"));
    let problems = push_run_gate_problems(&release);
    assert!(
        problems.is_empty(),
        "release.yml's push-run gate:\n  {}",
        problems.join("\n  ")
    );
    assert!(
        root()
            .join(".github/scripts/tests/test_check_release_push_run.sh")
            .exists(),
        "the check has its harness"
    );
}

#[test]
fn the_push_run_gate_check_catches_what_it_claims_to() {
    let release = read(&root().join(".github/workflows/release.yml"));
    let gate = "        run: .github/scripts/check_release_push_run.sh \"$GITHUB_SHA\"\n";
    let broken = [
        // R1: the gate's failure masked in the shell, in each spelling.
        release.replace(gate, &gate.replace("\"\n", "\" || true\n")),
        release.replace(gate, &gate.replace("\"\n", "\"; true\n")),
        release.replace(gate, &gate.replace("\"\n", "\" || exit 0\n")),
        release.replace(gate, &gate.replace("\"\n", "\" || exit \"0\"\n")),
        release.replace(gate, &gate.replace("\"\n", "\" || [ 1 ]\n")),
        release.replace(
            gate,
            "        run: |\n          set +e\n          .github/scripts/check_release_push_run.sh \"$GITHUB_SHA\"\n",
        ),
        // ... ignored, or skipped, by the step or by its job.
        release.replace(gate, &format!("        continue-on-error: true\n{gate}")),
        release.replace(
            gate,
            &format!("        if: github.event_name == 'workflow_dispatch'\n{gate}"),
        ),
        release.replace(
            "    name: Validate Release\n",
            "    name: Validate Release\n    continue-on-error: true\n",
        ),
        // ... run on another commit, without a token, or not waited for.
        release.replace(
            "check_release_push_run.sh \"$GITHUB_SHA\"",
            "check_release_push_run.sh main",
        ),
        release.replace("          GH_TOKEN: ${{ github.token }}\n", ""),
        release.replace("    needs: validate\n", ""),
        // ... or run, with every step, by a shell that never runs a script.
        release.replace(
            "\njobs:\n",
            "\ndefaults:\n  run:\n    shell: true {0}\n\njobs:\n",
        ),
        release.replace(
            "  CARGO_TERM_COLOR: always\n",
            "  CARGO_TERM_COLOR: always\n  BASH_ENV: .github/mask.sh\n",
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &release, "mutation {i} did not apply");
        assert!(
            !push_run_gate_problems(text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// A `NAME='...'` or `NAME=...` assignment's value in a shell script.
fn assigned<'a>(script: &'a str, name: &str) -> Option<&'a str> {
    script.lines().find_map(|line| {
        let value = line.strip_prefix(name)?.strip_prefix('=')?;
        Some(value.trim_matches('\''))
    })
}

/// Whether a job-level `if:` leaves out every push: it tests the event,
/// only for equality, and never with `push`.
fn skips_every_push(condition: &str) -> bool {
    let equal = Regex::new(r"github\.event_name\s*==\s*'([a-z_]+)'").expect("regex");
    let events: Vec<&str> = equal
        .captures_iter(condition)
        .map(|found| found.get(1).map_or("", |event| event.as_str()))
        .collect();
    !events.is_empty()
        && !condition.contains("github.event_name !=")
        && events.iter().all(|event| *event != "push")
}

/// Problems with `check_release_push_run.sh`'s two values against
/// `rust.yml`, empty when they agree. `SKIPPED_ON_PUSH` names exactly the
/// jobs whose `if:` leaves pushes out, so the gate lets those skip and no
/// other; `CROSS_PLATFORM_LEGS` is the number of Cross Platform legs off a
/// PR, every OS times every toolchain.
fn release_gate_problems(rust: &str, script: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let listed: Option<Vec<String>> =
        assigned(script, "SKIPPED_ON_PUSH").and_then(|value| serde_json::from_str(value).ok());
    let mut expected: Vec<String> = jobs(rust)
        .into_iter()
        .filter(|(_, body)| {
            body.lines()
                .find_map(|line| line.strip_prefix("    if: "))
                .is_some_and(skips_every_push)
        })
        .flat_map(|(id, body)| display_names(&id, &body))
        .collect();
    expected.sort();
    match listed {
        Some(mut listed) => {
            listed.sort();
            if listed != expected {
                problems.push(format!(
                    "SKIPPED_ON_PUSH lists {listed:?}, but the rust.yml jobs a push skips \
                     by design are {expected:?}"
                ));
            }
        }
        None => problems.push("no SKIPPED_ON_PUSH='[\"...\"]' JSON list in the script".into()),
    }

    let (_, cross) = jobs(rust)
        .into_iter()
        .find(|(id, _)| id == "cross-platform")
        .expect("rust.yml has a cross-platform job");
    let legs: Option<usize> = ["os", "rust"].iter().try_fold(1, |product, key| {
        let value = cross
            .lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{key}: ")))?;
        let (_, off_pr) = event_matrix(value)?;
        Some(product * off_pr.len())
    });
    let counted: Option<usize> =
        assigned(script, "CROSS_PLATFORM_LEGS").and_then(|value| value.parse().ok());
    match (legs, counted) {
        (Some(legs), Some(counted)) if legs == counted => {}
        (legs, counted) => problems.push(format!(
            "CROSS_PLATFORM_LEGS is {counted:?}, but the matrix off a PR has {legs:?} legs"
        )),
    }
    problems
}

/// The push run counts as green only when every job succeeded but the ones
/// a push skips by design, with every Cross Platform leg present: a gate
/// that let any job be skipped passed a run whose live groups never ran,
/// and one that wanted a leg or more passed a shrunken matrix.
#[test]
fn the_release_gate_knows_which_jobs_skip_a_push_and_how_many_legs_run() {
    let rust = read(&root().join(".github/workflows/rust.yml"));
    let script = read(&root().join(".github/scripts/check_release_push_run.sh"));
    let problems = release_gate_problems(&rust, &script);
    assert!(
        problems.is_empty(),
        "check_release_push_run.sh disagrees with rust.yml:\n  {}",
        problems.join("\n  ")
    );

    let broken = [
        // A new job that skips pushes, not listed.
        (
            rust.replace(
                "\n  fmt:\n",
                "\n  nightly-only:\n    name: Nightly Only\n    if: github.event_name == 'schedule'\n    runs-on: x\n\n  fmt:\n",
            ),
            script.clone(),
        ),
        // A listed job that now runs on pushes.
        (
            rust.replace(
                "if: (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && github.repository_owner",
                "if: github.repository_owner",
            ),
            script.clone(),
        ),
        // A job dropped from the list, so the gate fails every release.
        (
            rust.clone(),
            script.replace("\"Captured Fixtures Refresh\", ", ""),
        ),
        // Every job allowed to skip.
        (rust.clone(), script.replace("SKIPPED_ON_PUSH=", "SKIPPABLE=")),
        // Beta dropped from the matrix, the count left at six.
        (
            rust.replace(
                "|| fromJSON('[\"stable\", \"beta\"]')",
                "|| fromJSON('[\"stable\"]')",
            ),
            script.clone(),
        ),
        // The count lowered on its own.
        (rust.clone(), script.replace("CROSS_PLATFORM_LEGS=6", "CROSS_PLATFORM_LEGS=1")),
    ];
    for (i, (workflow, gate)) in broken.iter().enumerate() {
        assert!(
            workflow != &rust || gate != &script,
            "mutation {i} did not apply"
        );
        assert!(
            !release_gate_problems(workflow, gate).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}
