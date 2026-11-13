//! The flakiness report (`ci-flakiness-report.yml`), tied to what it reads
//! (#504): the jobs whose logs it fetches by name, the retry lines the
//! example loops and the live tests' own retries print, the steps it counts
//! them in, which leave out the retry helper's offline tests, the order in
//! which it publishes and then fails, and the offline tests that would
//! otherwise flake on a loaded runner.

use super::{command_lines, is_live_nextest_step, jobs, read, root, steps};
use crate::source_scan;
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
pub(super) fn display_names(id: &str, job: &str) -> Vec<String> {
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

/// Problems with the in-test retry marker, empty when it holds:
/// `retry_on_transient` (`tests/common/retry.rs`) prints it before every
/// retry, which is each `sleep(` in its body, and `count_retried_flakes.sh`
/// reads the same word and treats the same codes as transport.
fn retry_marker_problems(retry: &str, script: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(marker) = retry
        .lines()
        .find_map(|line| line.strip_prefix("pub const TRANSIENT_RETRY_MARKER: &str = \""))
        .and_then(|rest| rest.strip_suffix("\";"))
    else {
        return vec!["retry.rs has no `pub const TRANSIENT_RETRY_MARKER: &str = \"...\";`".into()];
    };
    let body = retry
        .split("pub async fn retry_on_transient")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .unwrap_or_default();
    let before_each_sleep: Vec<&str> = body.split("sleep(").collect();
    if before_each_sleep.len() < 3 {
        problems.push(format!(
            "retry_on_transient sleeps before {} retries, not the two kinds (a failed \
             interaction and an error): is the scan broken?",
            before_each_sleep.len().saturating_sub(1)
        ));
    }
    for (i, before) in before_each_sleep
        .iter()
        .take(before_each_sleep.len().saturating_sub(1))
        .enumerate()
    {
        if !before.contains("println!(\"{}\", transient_retry_marker(") {
            problems.push(format!(
                "retry_on_transient's retry {} sleeps without printing the marker first",
                i + 1
            ));
        }
    }
    if !retry.contains("format!(\"{TRANSIENT_RETRY_MARKER} {test} {code}\")") {
        problems.push("the marker line is not `<marker> <test> <code>`".into());
    }
    if !script.contains(&format!("w[1] == \"{marker}\"")) {
        problems.push(format!(
            "count_retried_flakes.sh does not read `{marker}` lines"
        ));
    }
    // The codes retry_code gives a failure before any HTTP status, which
    // the script must count as transport, as it does a status, in awk's
    // `transport()` alone: a second list (jq's totals once had two) could
    // drift from it unseen.
    let transport_lines: Vec<&str> = script
        .lines()
        .skip_while(|line| !line.contains("function transport(code)"))
        .take(4)
        .collect();
    let transport = transport_lines.concat();
    let elsewhere: Vec<&str> = script
        .lines()
        .filter(|line| !line.trim_start().starts_with('#') && !transport_lines.contains(line))
        .collect();
    let codes = retry
        .split("fn retry_code(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .unwrap_or_default();
    for code in codes
        .split("=> \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|code| *code != "other")
    {
        if !transport.contains(&format!("code == \"{code}\"")) {
            problems.push(format!(
                "retry_code's transport code `{code}` is not transport in count_retried_flakes.sh"
            ));
        }
        if let Some(line) = elsewhere.iter().find(|line| line.contains(code)) {
            problems.push(format!(
                "count_retried_flakes.sh names the transport code `{code}` outside \
                 transport(), which alone should classify codes: `{}`",
                line.trim()
            ));
        }
    }
    problems
}

/// A live test's `retry_request!` retries transport errors and the
/// model-side flakes D-052 allows inside the test, where nextest never sees
/// them, so the report could not count them: H4's `malformed_tool_call`
/// retry moved a flake nextest used to retry out of its sight. Each retry
/// now prints `TRANSIENT_RETRIED <test> <code>`, which
/// `count_retried_flakes.sh` counts per test.
#[test]
fn in_test_retries_print_the_marker_the_report_counts() {
    let retry = read(&root().join("tests/common/retry.rs"));
    let script = read(&root().join(".github/scripts/count_retried_flakes.sh"));
    let problems = retry_marker_problems(&retry, &script);
    assert!(
        problems.is_empty(),
        "in-test retries must print what the flakiness report counts:\n  {}",
        problems.join("\n  ")
    );
    let broken = [
        // The error retry's marker gone, or the failed interaction's.
        (
            retry.replacen(
                "println!(\"{}\", transient_retry_marker(&retry_code(&err)));",
                "",
                1,
            ),
            script.clone(),
        ),
        (
            retry.replacen(
                "println!(\"{}\", transient_retry_marker(\"malformed_tool_call\"));",
                "",
                1,
            ),
            script.clone(),
        ),
        // The script reading another word, or counting a timeout as a flake.
        (
            retry.clone(),
            script.replace("w[1] == \"TRANSIENT_RETRIED\"", "w[1] == \"RETRIED\""),
        ),
        (retry.clone(), script.replace("code == \"timeout\" || ", "")),
        // jq classifying the codes again, by its own list.
        (
            retry.clone(),
            script.replace(
                "select(.[5] == \"0\")",
                "select(.[3] | test(\"^([0-9]+|timeout|network|stream_interrupted)$\") | not)",
            ),
        ),
    ];
    for (i, (retry_text, script_text)) in broken.iter().enumerate() {
        assert!(
            retry_text != &retry || script_text != &script,
            "mutation {i} did not apply"
        );
        assert!(
            !retry_marker_problems(retry_text, script_text).is_empty(),
            "mutation {i} went unnoticed"
        );
    }
}

/// The source of test binary `binary`: `tests/<binary>.rs` and, when it
/// has one, every file under `tests/<binary>/`, walked by the shared
/// walker. Not its module tree: nearly every binary compiles
/// `tests/common/`, the retry helper's home, so each would read as calling
/// it.
fn binary_source(binary: &str) -> String {
    let tests = root().join("tests");
    let mut text = read(&tests.join(format!("{binary}.rs")));
    let dir = tests.join(binary);
    if dir.is_dir() {
        for path in source_scan::rust_files(&dir) {
            text.push_str(&read(&path));
        }
    }
    text
}

/// Problems with the steps of `rust` that the flakiness report counts (the
/// live nextest steps, which retry), and how many there are. The retry
/// helper's offline tests (`error_handling_tests`) print
/// `TRANSIENT_RETRIED` on every run, on purpose, and the counter cannot
/// tell their markers from a live test's. So each counted `nextest run`
/// runs the live tests alone (`--run-ignored only`), or binaries
/// (`binary(x)`) whose source, read by `source`, never calls the helper.
fn counted_step_problems(rust: &str, source: impl Fn(&str) -> String) -> (usize, Vec<String>) {
    let binary = Regex::new(r"binary\(([A-Za-z0-9_]+)\)").expect("a regex");
    let mut counted = 0;
    let mut problems = Vec::new();
    for (id, body) in jobs(rust) {
        for step in steps(&body).iter().filter(|step| {
            is_live_nextest_step(step)
                && command_lines(step).any(|line| line.starts_with("NEXTEST_RETRIES:"))
        }) {
            counted += 1;
            let title = step.lines().next().unwrap_or("").trim().to_string();
            for line in command_lines(step).filter(|line| line.contains("nextest run")) {
                if line.contains("--run-ignored only") {
                    continue;
                }
                let filtered: Vec<&str> = binary
                    .captures_iter(line)
                    .filter_map(|found| found.get(1).map(|name| name.as_str()))
                    .collect();
                if filtered.is_empty() {
                    problems.push(format!(
                        "job {id}, step `{title}`: `{line}` runs offline tests too, and the \
                         retry helper's own tests print TRANSIENT_RETRIED on every run; \
                         run `--run-ignored only`"
                    ));
                }
                for name in filtered {
                    let text = source(name);
                    if text.contains("retry_on_transient(") || text.contains("retry_request!") {
                        problems.push(format!(
                            "job {id}, step `{title}`: `{line}` runs {name}'s offline tests \
                             too, and it calls the retry helper; run `--run-ignored only`"
                        ));
                    }
                }
            }
        }
    }
    (counted, problems)
}

/// A live test's in-test retry is counted from its marker, but the retry
/// helper's offline tests print it too, on purpose, on every run: run in
/// the integration loop, which once ran each binary's offline tests as
/// well (`--run-ignored all`), five of them read as chronic flakes in
/// every run, and the report failed every day.
#[test]
fn counted_steps_leave_the_offline_retry_tests_out() {
    let rust = read(&root().join(".github/workflows/rust.yml"));
    let (counted, problems) = counted_step_problems(&rust, binary_source);
    assert!(counted >= 2, "found only {counted} counted steps");
    assert!(
        problems.is_empty(),
        "a step the flakiness report counts runs the retry helper's offline tests:\n  {}",
        problems.join("\n  ")
    );
    let integration = "cargo nextest run --test $test --run-ignored only";
    let harness = "-E 'binary(antigravity_harness)'";
    assert!(rust.contains(integration) && rust.contains(harness));
    let broken = [
        // The integration loop running every test again.
        (
            rust.replace(
                integration,
                "cargo nextest run --test $test --run-ignored all",
            ),
            false,
        ),
        // The harness step unfiltered.
        (rust.replace(harness, ""), false),
        // The harness binary calling the helper.
        (rust.clone(), true),
    ];
    for (i, (text, calls)) in broken.iter().enumerate() {
        let source = |name: &str| {
            if *calls && name == "antigravity_harness" {
                "retry_on_transient(3, || async { .. })".to_string()
            } else {
                binary_source(name)
            }
        };
        assert!(
            !counted_step_problems(text, source).1.is_empty(),
            "mutation {i} went unnoticed"
        );
    }
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
    files.extend(source_scan::rust_files(&tests.join("http_mock_resources")));

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
