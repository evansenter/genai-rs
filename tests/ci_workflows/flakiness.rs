//! The flakiness report (`ci-flakiness-report.yml`), tied to what it reads
//! (#504): the jobs whose logs it fetches by name, the retry lines the
//! example loops print, the order in which it publishes and then fails, and
//! the offline tests that would otherwise flake on a loaded runner.

use super::{command_lines, is_live_nextest_step, jobs, read, root, steps};
use regex::Regex;
use std::collections::BTreeMap;

/// The values each `matrix:` key of a job takes: a `key: [a, b]` list, or
/// one `key: value` per `include:` entry.
fn matrix_values(job: &str) -> BTreeMap<String, Vec<String>> {
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut matrix_indent = None;
    for line in job.lines() {
        let indent = line.len() - line.trim_start().len();
        let text = line.trim();
        if text == "matrix:" {
            matrix_indent = Some(indent);
            continue;
        }
        let Some(outer) = matrix_indent else {
            continue;
        };
        if !text.is_empty() && indent <= outer {
            break;
        }
        if text.starts_with('#') {
            continue;
        }
        let entry = text.trim_start_matches("- ");
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let found = values.entry(key.trim().to_string()).or_default();
        let items: Vec<&str> = match value.strip_prefix('[') {
            Some(list) => list.trim_end_matches(']').split(',').collect(),
            None => vec![value],
        };
        for item in items {
            let item = item
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
            if !found.contains(&item) {
                found.push(item);
            }
        }
    }
    values
}

/// The names a job shows in a run: its `name:`, with each
/// `${{ matrix.key }}` expanded over the key's values, or its id.
fn display_names(id: &str, job: &str) -> Vec<String> {
    let Some(name) = job
        .lines()
        .find_map(|line| line.strip_prefix("    name: "))
        .map(str::trim)
    else {
        return vec![id.to_string()];
    };
    let placeholder = Regex::new(r"\$\{\{\s*matrix\.(\w+)\s*\}\}").expect("regex");
    let values = matrix_values(job);
    let mut names = vec![name.to_string()];
    while let Some(found) = names.iter().find_map(|n| placeholder.captures(n)) {
        let (whole, key) = (found[0].to_string(), found[1].to_string());
        let options = values
            .get(&key)
            .unwrap_or_else(|| panic!("job {id}: no matrix values for `{key}`"));
        names = names
            .iter()
            .flat_map(|n| options.iter().map(|v| n.replacen(&whole, v, 1)))
            .collect();
    }
    names
}

/// A step that retries examples by hand: it runs `cargo run --example`
/// against the live API.
fn is_live_example_step(step: &str) -> bool {
    command_lines(step).any(|line| line.contains("cargo run") && line.contains("--example"))
        && step.contains("GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}")
}

/// The report fetches retry jobs' logs by display name (`RETRY_JOBS`), so a
/// renamed job, or a new one that retries, would drop out of the flake rate
/// unseen; and the report would read zero live runs as a clean week, had
/// it not escalated on them. The regex matches exactly the `rust.yml` jobs
/// with a live nextest step or a live example loop.
#[test]
fn retry_jobs_match_the_jobs_that_retry() {
    let report = read(&root().join(".github/workflows/ci-flakiness-report.yml"));
    let pattern = report
        .lines()
        .find_map(|line| line.trim().strip_prefix("RETRY_JOBS='"))
        .and_then(|rest| rest.strip_suffix('\''))
        .expect("ci-flakiness-report.yml sets RETRY_JOBS='...'");
    let retry_jobs = Regex::new(pattern).expect("RETRY_JOBS is a regex");
    let workflow_name = report
        .lines()
        .find_map(|line| line.trim().strip_prefix("WORKFLOW_NAME=\""))
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("ci-flakiness-report.yml sets WORKFLOW_NAME=\"...\"");

    let rust = read(&root().join(".github/workflows/rust.yml"));
    assert_eq!(
        rust.lines()
            .next()
            .and_then(|line| line.strip_prefix("name: ")),
        Some(workflow_name),
        "the report lists runs of the workflow named {workflow_name:?}, which must be rust.yml"
    );

    let mut retrying = Vec::new();
    let mut problems = Vec::new();
    for (id, body) in jobs(&rust) {
        let retries = steps(&body).iter().any(|step| {
            (is_live_nextest_step(step)
                && command_lines(step).any(|line| line.starts_with("NEXTEST_RETRIES:")))
                || is_live_example_step(step)
        });
        for name in display_names(&id, &body) {
            let matched = retry_jobs.is_match(&name);
            if retries && !matched {
                problems.push(format!(
                    "job {id} ({name}) retries, but RETRY_JOBS does not match it"
                ));
            } else if !retries && matched {
                problems.push(format!(
                    "RETRY_JOBS matches job {id} ({name}), which does not retry"
                ));
            }
        }
        if retries {
            retrying.push(id);
        }
    }
    for expected in ["test-antigravity", "test-integration", "example-smoke"] {
        assert!(
            retrying.iter().any(|id| id == expected),
            "found no retries in rust.yml job {expected}; found {retrying:?}"
        );
    }
    assert!(
        problems.is_empty(),
        "RETRY_JOBS ({pattern}) must match exactly the rust.yml jobs that retry:\n  {}",
        problems.join("\n  ")
    );
}

/// `cargo run` has no retries, so the example loops retry by hand and a
/// retry leaves the job green. Each opens a `::group::cargo run --example`
/// per example (the report's count of example runs) and prints
/// `EXAMPLE_RETRIED` when one passed only on a retry, which
/// `count_retried_flakes.sh` counts.
#[test]
fn example_loops_report_their_retries() {
    let rust = read(&root().join(".github/workflows/rust.yml"));
    let mut loops = 0;
    let mut problems = Vec::new();
    for (id, body) in jobs(&rust) {
        for step in steps(&body)
            .iter()
            .filter(|step| is_live_example_step(step))
        {
            loops += 1;
            let title = step.lines().next().unwrap_or("").trim().to_string();
            for (needle, why) in [
                (
                    "echo \"::group::cargo run --example $ex",
                    "group per example",
                ),
                ("echo \"EXAMPLE_RETRIED $ex ", "EXAMPLE_RETRIED line"),
            ] {
                if !command_lines(step).any(|line| line.starts_with(needle)) {
                    problems.push(format!("job {id}, step `{title}`: no {why} (`{needle}`)"));
                }
            }
        }
    }
    assert_eq!(loops, 2, "expected the two example loops in rust.yml");
    assert!(
        problems.is_empty(),
        "example loops must print what the flakiness report counts:\n  {}",
        problems.join("\n  ")
    );
}

/// A breached target (a flake rate of 1% or more, no live runs read, a
/// chronic flake) fails the report, which escalates. The issue is updated
/// first, so the report that explains the failure is published; and the
/// failing step comes before the escalation, which runs on `failure()`.
#[test]
fn the_report_publishes_then_fails_then_escalates() {
    let report = read(&root().join(".github/workflows/ci-flakiness-report.yml"));
    let (_, job) = jobs(&report)
        .into_iter()
        .next()
        .expect("ci-flakiness-report.yml has a job");
    let all = steps(&job);
    let position = |name: &str| {
        all.iter()
            .position(|step| step.contains(&format!("- name: {name}\n")))
            .unwrap_or_else(|| panic!("no step named `{name}`"))
    };
    let generate = position("Generate report");
    let publish = position("Create or update the rolling report issue");
    let fail = position("Fail on a breached flake target");
    let escalate = position("Escalate a failing scheduled run");
    assert!(
        generate < publish && publish < fail && fail < escalate,
        "steps out of order: generate {generate}, publish {publish}, fail {fail}, escalate {escalate}"
    );
    assert!(
        all[generate].contains("--summary summary.json")
            && all[generate].contains("echo \"breach=true\" >> \"$GITHUB_OUTPUT\""),
        "the report step must pass the summary and record a breach (exit 3):\n{}",
        all[generate]
    );
    assert!(
        all[fail].contains("if: steps.report.outputs.breach == 'true'")
            && command_lines(&all[fail]).any(|line| line == "exit 1"),
        "the failing step must run on a breach and fail:\n{}",
        all[fail]
    );
}

/// A sub-second timeout against the HTTP stub races the real clock, so on a
/// loaded runner it fires early or late (#501; CLAUDE.md, Tests). Every
/// such test runs on the paused clock.
#[test]
fn stub_timeouts_run_on_the_paused_clock() {
    let tests = root().join("tests");
    let mut files = vec![
        tests.join("http_mock_tests.rs"),
        tests.join("reliability_tests.rs"),
    ];
    let dir = tests.join("http_mock_resources");
    let mut resources: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    resources.sort();
    files.extend(resources);

    let mut timed = 0;
    let mut problems = Vec::new();
    for path in files {
        let text = read(&path);
        // Each `#[tokio::test...]` attribute and the code up to the next.
        for chunk in text.split("#[tokio::test").skip(1) {
            let (attribute, body) = chunk.split_once(']').unwrap_or((chunk, ""));
            let compact: String = body.split_whitespace().collect();
            if !compact.contains("timeout(Duration::from_millis(") {
                continue;
            }
            timed += 1;
            if !attribute.contains("start_paused = true") {
                let name = body
                    .split("fn ")
                    .nth(1)
                    .and_then(|rest| rest.split('(').next())
                    .unwrap_or("?");
                let file = path.strip_prefix(root()).unwrap_or(&path).display();
                problems.push(format!("{file}: {name}"));
            }
        }
    }
    assert!(
        timed >= 5,
        "found only {timed} stub tests with a millisecond timeout; is the scan broken?"
    );
    assert!(
        problems.is_empty(),
        "tests with a millisecond timeout must use #[tokio::test(start_paused = true)] \
         and a paused-clock client (docs/TESTING.md):\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn display_names_expand_the_matrix() {
    let job = "    name: Cross (${{ matrix.os }}, ${{ matrix.rust }})\n    strategy:\n      matrix:\n        os: [ubuntu-latest, windows-latest]\n        rust: [stable]\n    steps:\n      - run: a\n";
    assert_eq!(
        display_names("cross", job),
        [
            "Cross (ubuntu-latest, stable)",
            "Cross (windows-latest, stable)"
        ]
    );
    let job = "    name: Group (${{ matrix.group }})\n    strategy:\n      matrix:\n        include:\n          - group: core\n            tests: \"a b\"\n          - group: tools\n            tests: \"c\"\n    steps:\n";
    assert_eq!(display_names("g", job), ["Group (core)", "Group (tools)"]);
    assert_eq!(display_names("plain", "    runs-on: x\n"), ["plain"]);
}
