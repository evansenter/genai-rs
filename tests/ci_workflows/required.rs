//! The required checks fail on a failure: every `run:` step of every job
//! whose check branch protection requires on `main` is held to `escapes`,
//! as the coverage floor and the live steps are (#504, D-058). Those jobs
//! are listed in [`REQUIRED_JOBS`], tied to CLAUDE.md's list.

use std::collections::BTreeSet;

use super::flakiness::display_names;
use super::{
    LIVE_ALLOWED, LIVE_GATE, escapes, jobs, read, reads_the_key, root, step_name, steps, yaml,
};

/// A `rust.yml` job whose check branch protection requires on `main`, and
/// the `if:` and `needs:` lines (`key: value`) it may carry.
struct RequiredJob {
    id: &'static str,
    allowed: &'static [&'static str],
}

/// The jobs whose checks are required on `main` (CLAUDE.md, "CI notes":
/// "Required on `main`"; `the_required_jobs_are_the_ones_claude_md_lists`
/// holds the two together). The live groups run only where the key is,
/// after Check; a step in any of them that reads the key may carry the
/// same owner gate ([`LIVE_GATE`]).
const REQUIRED_JOBS: [RequiredJob; 8] = [
    RequiredJob {
        id: "check",
        allowed: &[],
    },
    RequiredJob {
        id: "test",
        allowed: &[],
    },
    RequiredJob {
        id: "fmt",
        allowed: &[],
    },
    RequiredJob {
        id: "clippy",
        allowed: &[],
    },
    RequiredJob {
        id: "doc",
        allowed: &[],
    },
    RequiredJob {
        id: "test-antigravity",
        allowed: &[],
    },
    RequiredJob {
        id: "test-strict-unknown",
        allowed: &[],
    },
    RequiredJob {
        id: "test-integration",
        allowed: &LIVE_ALLOWED,
    },
];

fn rust_yml() -> String {
    read(&root().join(".github/workflows/rust.yml"))
}

/// Problems with the `run:` steps of [`REQUIRED_JOBS`] in `workflow`, and
/// how many there are: each is held to [`escapes`], with nothing allowed
/// but its job's lines and, on a step that reads the key, the owner gate.
/// The jobs and steps are the ones YAML reads (`yaml::structure_problems`),
/// so none is left out of the count.
fn required_step_problems(workflow: &str) -> (usize, Vec<String>) {
    let mut checked = 0;
    let mut problems: Vec<String> = yaml::structure_problems(workflow)
        .into_iter()
        .map(|problem| format!("rust.yml: {problem}"))
        .collect();
    let all = jobs(workflow);
    for required in &REQUIRED_JOBS {
        let Some((_, body)) = all.iter().find(|(id, _)| id == required.id) else {
            problems.push(format!("rust.yml has no required job {}", required.id));
            continue;
        };
        for step in steps(body)
            .iter()
            .filter(|step| !matches!(yaml::run_value(step), Ok(None)))
        {
            checked += 1;
            let mut allowed = required.allowed.to_vec();
            if reads_the_key(step) {
                allowed.push(LIVE_GATE);
            }
            for escape in escapes(workflow, body, step, &allowed) {
                problems.push(format!(
                    "rust.yml job {}, step `{}`: {escape}",
                    required.id,
                    step_name(step)
                ));
            }
        }
    }
    (checked, problems)
}

/// A required check's own test, lint and build steps once took any of the
/// masks the guarded steps could not: `|| true` on Test Suite's nextest
/// run, the antigravity unit run or Clippy's `-D warnings`, and
/// `continue-on-error` on strict-unknown's run each passed every guard (the
/// #504 grade of 2026-11-13, K-8). Each `run:` step of each required job
/// is now held to them.
#[test]
fn every_step_of_a_required_check_fails_its_job() {
    let (checked, problems) = required_step_problems(&rust_yml());
    assert!(checked >= 20, "found only {checked} required run steps");
    assert!(
        problems.is_empty(),
        "a required check's step must fail its job on a failure:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_required_step_check_catches_what_it_claims_to() {
    let workflow = rust_yml();
    assert!(required_step_problems(&workflow).1.is_empty());
    let broken = [
        // K-8: Test Suite's run, the antigravity unit run and Clippy's run
        // masked, and strict-unknown's run ignored.
        workflow.replacen(
            "        run: cargo nextest run --workspace\n      - name: Run doctests (default features)\n",
            "        run: cargo nextest run --workspace || true\n      - name: Run doctests (default features)\n",
            1,
        ),
        workflow.replacen(
            "        run: cargo nextest run --workspace --features antigravity\n",
            "        run: cargo nextest run --workspace --features antigravity || true\n",
            1,
        ),
        workflow.replacen(
            "        run: cargo clippy --workspace --all-targets --all-features -- -D warnings\n",
            "        run: cargo clippy --workspace --all-targets --all-features -- -D warnings || true\n",
            1,
        ),
        workflow.replacen(
            "      - name: Run unit tests in strict mode\n",
            "      - name: Run unit tests in strict mode\n        continue-on-error: true\n",
            1,
        ),
        // ... the same, written as YAML reads it but the text did not.
        workflow.replacen(
            "        run: cargo fmt --all -- --check\n",
            "        run: cargo fmt --all -- --check\n          || true\n",
            1,
        ),
        workflow.replacen(
            "    name: Documentation\n",
            "    name: Documentation\n    \"continue-on-error\": true\n",
            1,
        ),
        // ... the owner gate on a step that reads no key, so a fork's PR
        // skips it, or on the job.
        workflow.replacen(
            "      - name: Run unit tests with antigravity feature\n",
            &format!("      - name: Run unit tests with antigravity feature\n        {LIVE_GATE}\n"),
            1,
        ),
        workflow.replacen(
            "    name: Test Suite (antigravity)\n",
            &format!("    name: Test Suite (antigravity)\n    {LIVE_GATE}\n"),
            1,
        ),
        // ... or the smoke run's retry loop with a status the guard does
        // not read.
        workflow.replacen(
            "          exit \"$rc\"\n        env:\n          GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}\n\n  test-integration:",
            "          exit 0\n        env:\n          GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}\n\n  test-integration:",
            1,
        ),
        // A step the text split cannot follow: its `-` alone on its line.
        workflow.replacen(
            "      - name: Check formatting\n",
            "      -\n        name: Check formatting\n",
            1,
        ),
    ];
    for (i, text) in broken.iter().enumerate() {
        assert_ne!(text, &workflow, "mutation {i} did not apply");
        assert!(
            !required_step_problems(text).1.is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// The checks CLAUDE.md's "Required on `main`" bullet lists, as written
/// in `claude`: the names it lists one by one, and its last item, `the
/// <count> <name> groups`, as `(count, name)`.
fn claude_md_required(claude: &str) -> (BTreeSet<String>, (usize, String)) {
    let start = claude
        .find("\n- Required on `main`")
        .expect("CLAUDE.md lists the checks required on main");
    let bullet: Vec<&str> = claude[start + 1..]
        .lines()
        .enumerate()
        .take_while(|(i, line)| *i == 0 || line.starts_with("  "))
        .map(|(_, line)| line.trim())
        .collect();
    let bullet = bullet.join(" ");
    let list = bullet
        .split_once("): ")
        .and_then(|(_, rest)| rest.split_once(". "))
        .map(|(list, _)| list)
        .expect("the bullet reads `...): <checks>. ...`");
    let (names, groups) = list
        .rsplit_once(" and ")
        .expect("the list ends `and the <count> <name> groups`");
    let groups = groups
        .strip_prefix("the ")
        .and_then(|rest| rest.strip_suffix(" groups"))
        .and_then(|rest| rest.split_once(' '))
        .expect("the list ends `the <count> <name> groups`");
    let count = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ]
    .iter()
    .position(|word| *word == groups.0)
    .map(|at| at + 1)
    .unwrap_or_else(|| panic!("`{}` is not a count from one to ten", groups.0));
    let names = names.split(", ").map(str::to_owned).collect();
    (names, (count, groups.1.to_owned()))
}

/// How [`REQUIRED_JOBS`], named as `workflow` shows them, differs from the
/// checks `claude` (CLAUDE.md) lists as required; empty when they agree.
fn required_list_problems(workflow: &str, claude: &str) -> Vec<String> {
    let all = jobs(workflow);
    let mut names = BTreeSet::new();
    let mut problems = Vec::new();
    for required in &REQUIRED_JOBS {
        match all.iter().find(|(id, _)| id == required.id) {
            Some((_, body)) => names.extend(display_names(required.id, body)),
            None => problems.push(format!("rust.yml has no job {}", required.id)),
        }
    }
    let (listed, (count, group)) = claude_md_required(claude);
    let prefix = format!("{group} (");
    let (grouped, single): (BTreeSet<String>, BTreeSet<String>) = names
        .into_iter()
        .partition(|name| name.starts_with(&prefix));
    if single != listed {
        problems.push(format!(
            "REQUIRED_JOBS holds {single:?}, but CLAUDE.md requires {listed:?}"
        ));
    }
    if grouped.len() != count {
        problems.push(format!(
            "CLAUDE.md requires the {count} {group} groups; REQUIRED_JOBS holds {grouped:?}"
        ));
    }
    problems
}

/// [`REQUIRED_JOBS`] is the list CLAUDE.md gives (and branch protection
/// holds, which only the owner can read): a check required there but not
/// held here, or held here but no longer required, fails.
#[test]
fn the_required_jobs_are_the_ones_claude_md_lists() {
    let workflow = rust_yml();
    let claude = read(&root().join("CLAUDE.md"));
    let problems = required_list_problems(&workflow, &claude);
    assert!(
        problems.is_empty(),
        "REQUIRED_JOBS and CLAUDE.md's required list differ:\n  {}",
        problems.join("\n  ")
    );
    // A check added to the list, one dropped from it, a group count that
    // moved, or a required job renamed in rust.yml: each fails.
    let broken = [
        (
            workflow.clone(),
            claude.replace("Format, Clippy,", "Format, Clippy, Shell Scripts,"),
        ),
        (
            workflow.clone(),
            claude.replace("Format, Clippy,", "Format,"),
        ),
        (
            workflow.clone(),
            claude.replace(
                "five Integration Tests groups",
                "six Integration Tests groups",
            ),
        ),
        (
            workflow.replace("    name: Clippy\n", "    name: Lint\n"),
            claude.clone(),
        ),
    ];
    for (i, (mutated_workflow, mutated_claude)) in broken.iter().enumerate() {
        assert!(
            *mutated_workflow != workflow || *mutated_claude != claude,
            "mutation {i} did not apply"
        );
        assert!(
            !required_list_problems(mutated_workflow, mutated_claude).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}
