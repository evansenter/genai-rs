#!/usr/bin/env bash
# Exercises `count_retried_flakes.sh` over real nextest output (#492).
#
# The fixtures in fixtures/nextest_retries/ are nextest 0.9.120's own
# `--retries 2 --success-output=final --color=always` output, renamed and
# wrapped as GitHub job logs hold it: a green integration job (raw job log,
# timestamped) where three tests passed on a retry, and a red run as
# `gh run view --log` prints it, where one test passed on a retry and one
# failed every try. A green run's flakes are the case the report used to
# miss, so it is the one pinned hardest here.
#
# Each case runs under every awk on the machine: CI's Ubuntu runner has
# mawk, a Mac has BSD awk. The script is run directly, as the workflow
# runs it, so a commit that drops its executable bit fails here too.

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
    echo "jq is required to run this harness (brew install jq / apt install jq)" >&2
    exit 1
}

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/count_retried_flakes.sh"
FIXTURES="$HERE/fixtures/nextest_retries"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

expect_json() {
    local label="$1" filter="$2" want="$3" got
    got=$(jq -c "$filter" <<<"$out")
    if [ "$got" = "$want" ]; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: $filter is $got, expected $want"
        failures=$((failures + 1))
    fi
}

awks=()
for candidate in awk mawk gawk; do
    command -v "$candidate" >/dev/null 2>&1 && awks+=("$candidate")
done

for awk_impl in "${awks[@]}"; do
    echo "# awk: $awk_impl"
    export AWK="$awk_impl"

    # --- A green job: nothing failed, three tests passed on a retry.
    out=$("$SCRIPT" "$FIXTURES/green_integration_job.txt")
    expect_json "green job: sums both runs' Summary lines" .tests_run 5
    expect_json "green job: counts every retried flake" .flaky 3
    expect_json "green job: one entry per test, by binary" \
        '[.tests[] | "\(.binary) \(.name)"] | sort' \
        '["files_api_tests test_upload_and_get","temp_file_tests test_markdown_document_from_temp_file","temp_file_tests test_pdf_from_temp_file"]'
    # FLAKY 2/3 and TRY 2 PASS describe one event; counted once.
    expect_json "green job: FLAKY and TRY n PASS count once" \
        '[.tests[] | select(.name == "test_markdown_document_from_temp_file") | .flaky]' '[1]'
    expect_json "green job: failed tries come from FLAKY n/m" \
        '[.tests[] | select(.name == "test_pdf_from_temp_file") | .failed_tries]' '[2]'

    # --- A red run from `gh run view --log`, read on stdin: job and step
    #     before the timestamp. A test that failed every try is a failure,
    #     not a flake.
    out=$("$SCRIPT" < "$FIXTURES/red_run_view.txt")
    expect_json "red run: tests run" .tests_run 3
    expect_json "red run: only the test that recovered is a flake" \
        '[.tests[] | .name]' '["test_stateful_conversation"]'

    # --- Two logs at once, as the workflow passes a run's jobs.
    out=$("$SCRIPT" "$FIXTURES/green_integration_job.txt" "$FIXTURES/red_run_view.txt")
    expect_json "two logs: totals add up" '[.tests_run, .flaky]' '[8,4]'

    # --- The antigravity job, trimmed from a real `gh run view --log`
    #     (2026-11-05) with one retry added: the offline suite (1676 tests)
    #     and the scripted suite (15) run without retries in the same log,
    #     and only the harness step's 24 tests count toward the rate.
    out=$("$SCRIPT" "$FIXTURES/antigravity_job_run_view.txt")
    expect_json "antigravity job: only the retried step's tests count" .tests_run 24
    expect_json "antigravity job: its flake counts" \
        '[.tests[] | "\(.binary) \(.name) \(.flaky)"]' \
        '["antigravity_harness test_antigravity_chat_basic 1"]'

    # --- A raw job log: a retried step (by NEXTEST_RETRIES) then one
    #     without retries. The step state resets per log, so the plain log
    #     after it counts whole.
    cat > "$WORK/steps.log" <<'LOG'
2026-11-05T10:00:00.0000000Z ##[group]Run cargo nextest run --test voices_tests
2026-11-05T10:00:00.1000000Z env:
2026-11-05T10:00:00.2000000Z   NEXTEST_RETRIES: 2
2026-11-05T10:00:00.3000000Z ##[endgroup]
2026-11-05T10:00:01.0000000Z     Starting 3 tests across 1 binary
2026-11-05T10:00:02.0000000Z    FLAKY 2/3 [   0.010s] genai-rs::voices_tests test_voice_list
2026-11-05T10:00:03.0000000Z      Summary [   0.020s] 3 tests run: 3 passed (1 flaky), 0 skipped
2026-11-05T10:00:04.0000000Z ##[group]Run cargo nextest run --workspace
2026-11-05T10:00:04.1000000Z cargo nextest run --workspace   # no --retries here
2026-11-05T10:00:04.2000000Z ##[endgroup]
2026-11-05T10:00:05.0000000Z     Starting 900 tests across 30 binaries
2026-11-05T10:00:06.0000000Z      Summary [   9.000s] 900 tests run: 900 passed, 0 skipped
LOG
    printf '     Summary [   1.000s] 7 tests run: 7 passed, 0 skipped\n' > "$WORK/clean.log"
    out=$("$SCRIPT" "$WORK/steps.log" "$WORK/clean.log")
    expect_json "steps: a step without retries is left out, a later plain log is not" \
        '[.tests_run, .flaky]' '[10,1]'

    # --- A command continued over lines with backslashes, as
    #     `gh run view --log` prints a step header: `--retries` on a later
    #     line than `nextest run` still makes the step a retried one. The
    #     second step splits `nextest run` and `--retries` across two
    #     separate commands, so it is not; neither is the third, a continued
    #     command with no retries.
    cat > "$WORK/continued.log" <<'LOG'
Integration	Run tests	2026-11-05T10:00:00.0000000Z ##[group]Run cargo nextest run --test voices_tests \
Integration	Run tests	2026-11-05T10:00:00.0000001Z ^[[36;1mcargo nextest run --test voices_tests \^[[0m
Integration	Run tests	2026-11-05T10:00:00.0000002Z ^[[36;1m  --run-ignored all \^[[0m
Integration	Run tests	2026-11-05T10:00:00.0000003Z ^[[36;1m  --retries 2 --no-fail-fast^[[0m
Integration	Run tests	2026-11-05T10:00:00.0000004Z shell: /usr/bin/bash -e {0}
Integration	Run tests	2026-11-05T10:00:00.0000005Z ##[endgroup]
Integration	Run tests	2026-11-05T10:00:01.0000000Z     Starting 3 tests across 1 binary
Integration	Run tests	2026-11-05T10:00:02.0000000Z    FLAKY 2/3 [   0.010s] genai-rs::voices_tests test_voice_list
Integration	Run tests	2026-11-05T10:00:03.0000000Z      Summary [   0.020s] 3 tests run: 3 passed (1 flaky), 0 skipped
Integration	Split	2026-11-05T10:00:04.0000000Z ##[group]Run cargo nextest run --workspace
Integration	Split	2026-11-05T10:00:04.0000001Z ^[[36;1mcargo nextest run --workspace^[[0m
Integration	Split	2026-11-05T10:00:04.0000002Z ^[[36;1mecho "--retries 2"^[[0m
Integration	Split	2026-11-05T10:00:04.0000003Z ##[endgroup]
Integration	Split	2026-11-05T10:00:05.0000000Z     Starting 900 tests across 30 binaries
Integration	Split	2026-11-05T10:00:05.5000000Z    FLAKY 2/3 [   0.010s] genai-rs::voices_tests test_split_step
Integration	Split	2026-11-05T10:00:06.0000000Z      Summary [   9.000s] 900 tests run: 900 passed, 0 skipped
Integration	No retries	2026-11-05T10:00:07.0000000Z ##[group]Run cargo nextest run --workspace \
Integration	No retries	2026-11-05T10:00:07.0000001Z ^[[36;1mcargo nextest run --workspace \^[[0m
Integration	No retries	2026-11-05T10:00:07.0000002Z ^[[36;1m  --no-fail-fast^[[0m
Integration	No retries	2026-11-05T10:00:07.0000003Z ^[[36;1mecho retried 2^[[0m
Integration	No retries	2026-11-05T10:00:07.0000004Z ##[endgroup]
Integration	No retries	2026-11-05T10:00:08.0000000Z     Starting 50 tests across 2 binaries
Integration	No retries	2026-11-05T10:00:09.0000000Z      Summary [   1.000s] 50 tests run: 50 passed, 0 skipped
LOG
    out=$("$SCRIPT" "$WORK/continued.log")
    expect_json "continued command: --retries on a later line counts the step" \
        '[.tests_run, .flaky]' '[3,1]'
    expect_json "continued command: only the retried step's flake is listed" \
        '[.tests[] | .name]' '["test_voice_list"]'

    # --- One test flaky in two runs counts twice; a library unit test is
    #     reported under `lib`; a `TRY n PASS` with no FLAKY line still
    #     counts (a run with the final status list turned off).
    cat > "$WORK/twice.log" <<'LOG'
    Starting 2 tests across 1 binary
  TRY 1 FAIL [   0.010s] genai-rs::voices_tests test_voice_list
  TRY 2 PASS [   0.010s] genai-rs::voices_tests test_voice_list
     Summary [   0.020s] 2 tests run: 2 passed (1 flaky), 0 skipped
    Starting 2 tests across 1 binary
  TRY 1 FAIL [   0.010s] genai-rs::voices_tests test_voice_list
  TRY 2 PASS [   0.010s] genai-rs::voices_tests test_voice_list
   FLAKY 2/3 [   0.010s] genai-rs::voices_tests test_voice_list
   FLAKY 2/3 [   0.010s] genai-rs request::tests::builds_the_body
     Summary [   0.020s] 2 tests run: 2 passed (2 flaky), 0 skipped
LOG
    out=$("$SCRIPT" "$WORK/twice.log")
    expect_json "two runs: the same test counts once per run" \
        '[.tests[] | select(.name == "test_voice_list") | .flaky]' '[2]'
    expect_json "two runs: a lib unit test is under lib" \
        '[.tests[] | select(.binary == "lib") | .name]' '["request::tests::builds_the_body"]'
    expect_json "two runs: tests run" .tests_run 4

    # --- A flaky test's own runs: the counted runs it ran in, under any
    #     status, which the report divides its flakes by. Its run in a step
    #     without retries does not count.
    cat > "$WORK/runs.log" <<'LOG'
2026-11-05T10:00:00.0000000Z ##[group]Run cargo nextest run --test voices_tests
2026-11-05T10:00:00.1000000Z   NEXTEST_RETRIES: 2
2026-11-05T10:00:00.2000000Z ##[endgroup]
2026-11-05T10:00:01.0000000Z     Starting 2 tests across 1 binary
2026-11-05T10:00:01.1000000Z         PASS [   0.010s] (1/2) genai-rs::voices_tests test_voice_list
2026-11-05T10:00:01.2000000Z         PASS [   0.010s] (2/2) genai-rs::voices_tests test_voice_get
2026-11-05T10:00:01.3000000Z      Summary [   0.020s] 2 tests run: 2 passed, 0 skipped
2026-11-05T10:00:02.0000000Z     Starting 2 tests across 1 binary
2026-11-05T10:00:02.1000000Z   TRY 1 FAIL [   0.010s] (─────) genai-rs::voices_tests test_voice_list
2026-11-05T10:00:02.2000000Z   TRY 2 PASS [   0.010s] (1/2) genai-rs::voices_tests test_voice_list
2026-11-05T10:00:02.3000000Z         PASS [   0.010s] (2/2) genai-rs::voices_tests test_voice_get
2026-11-05T10:00:02.4000000Z      Summary [   0.020s] 2 tests run: 2 passed (1 flaky), 0 skipped
2026-11-05T10:00:02.5000000Z    FLAKY 2/3 [   0.010s] (1/2) genai-rs::voices_tests test_voice_list
2026-11-05T10:00:03.0000000Z     Starting 1 test across 1 binary
2026-11-05T10:00:03.1000000Z    SLOW [> 60.000s] genai-rs::voices_tests test_voice_list
2026-11-05T10:00:03.2000000Z         FAIL [  61.000s] (1/1) genai-rs::voices_tests test_voice_list
2026-11-05T10:00:03.3000000Z      Summary [  61.000s] 1 test run: 0 passed, 1 failed, 0 skipped
2026-11-05T10:00:04.0000000Z ##[group]Run cargo nextest run --workspace
2026-11-05T10:00:04.1000000Z ##[endgroup]
2026-11-05T10:00:05.0000000Z     Starting 1 test across 1 binary
2026-11-05T10:00:05.1000000Z         PASS [   0.010s] (1/1) genai-rs::voices_tests test_voice_list
2026-11-05T10:00:05.2000000Z      Summary [   0.010s] 1 test run: 1 passed, 0 skipped
LOG
    out=$("$SCRIPT" "$WORK/runs.log")
    expect_json "runs: a flaky test's runs count every retried run it ran in" \
        '[.tests[] | [.name, .flaky, .runs]]' '[["test_voice_list",1,3]]'

    # --- The example loops: a group per example run, and an EXAMPLE_RETRIED
    #     line for one that passed only on a retry, in steps with no
    #     NEXTEST_RETRIES. The step header holds the loop's own script, whose
    #     echo lines are not runs or retries.
    cat > "$WORK/examples.log" <<'LOG'
Example Smoke Run	Run examples	2026-11-05T10:00:00.0000000Z ##[group]Run set -eo pipefail
Example Smoke Run	Run examples	2026-11-05T10:00:00.0000001Z ^[[36;1m  echo "::group::cargo run --example $ex"^[[0m
Example Smoke Run	Run examples	2026-11-05T10:00:00.0000002Z ^[[36;1m    echo "EXAMPLE_RETRIED $ex $retried"^[[0m
Example Smoke Run	Run examples	2026-11-05T10:00:00.0000003Z ##[endgroup]
Example Smoke Run	Run examples	2026-11-05T10:00:01.0000000Z ##[group]cargo run --example simple_interaction
Example Smoke Run	Run examples	2026-11-05T10:00:02.0000000Z ##[endgroup]
Example Smoke Run	Run examples	2026-11-05T10:00:03.0000000Z ##[group]cargo run --example streaming
Example Smoke Run	Run examples	2026-11-05T10:00:04.0000000Z ##[warning]streaming failed, retrying once
Example Smoke Run	Run examples	2026-11-05T10:00:05.0000000Z ##[endgroup]
Example Smoke Run	Run examples	2026-11-05T10:00:05.0000001Z EXAMPLE_RETRIED streaming 1
Test Suite (antigravity)	Smoke-run antigravity examples	2026-11-05T10:00:06.0000000Z ##[group]cargo run --example repo_auditor (budget 660s)
Test Suite (antigravity)	Smoke-run antigravity examples	2026-11-05T10:00:07.0000000Z ##[endgroup]
Test Suite (antigravity)	Smoke-run antigravity examples	2026-11-05T10:00:07.0000001Z EXAMPLE_RETRIED repo_auditor 2
LOG
    out=$("$SCRIPT" "$WORK/examples.log")
    expect_json "examples: a group per run, either loop" .examples_run 3
    expect_json "examples: retried ones counted" .examples_retried 2
    expect_json "examples: by name, with failed tries and runs" \
        '[.examples[] | [.name, .flaky, .failed_tries, .runs]]' \
        '[["repo_auditor",1,2,1],["streaming",1,1,1]]'
    expect_json "examples: not live test runs" '[.tests_run, .flaky]' '[0,0]'

    # --- Skip markers: counted in the retried step's test output, each on
    #     the test whose status line precedes it (nextest prints a passing
    #     test's output after its line with --success-output=final). The
    #     step header's script and the loop's own warning name the markers
    #     too, and are not markers; neither is one in a step without
    #     retries. A marker before any status line has no test.
    cat > "$WORK/markers.log" <<'LOG'
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:00.0Z ##[group]Run set -eo pipefail
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:00.1Z ^[[36;1m  skips=$(grep -cE 'SEMANTIC_VALIDATION_SKIPPED|LIVE_TOOL_EVIDENCE_SKIPPED' "/tmp/$test.log" || true)^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:00.2Z   NEXTEST_RETRIES: 2
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:00.3Z ##[endgroup]
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.0Z     Starting 2 tests across 1 binary
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.1Z ^[[32;1m        PASS^[[0m [   1.000s] (1/2) ^[[35;1mgenai-rs::tools_and_config_tests^[[0m ^[[36mmcp^[[0m^[[36m::^[[0m^[[34;1mtest_mcp^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.2Z ^[[32;1m        PASS^[[0m [   1.000s] (2/2) ^[[35;1mgenai-rs::tools_and_config_tests^[[0m ^[[34;1mtest_other^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.3Z ^[[32;1m     Summary^[[0m [   2.000s] 2 tests run: 2 passed, 0 skipped
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.4Z ^[[32;1m        PASS^[[0m [   1.000s] (1/2) ^[[35;1mgenai-rs::tools_and_config_tests^[[0m ^[[36mmcp^[[0m^[[36m::^[[0m^[[34;1mtest_mcp^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.5Z ^[[32;1m ^[[0m ^[[32;1mstdout^[[0m ^[[32;1m───^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.6Z     running 1 test
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.7Z     LIVE_TOOL_EVIDENCE_SKIPPED: the MCP server could not be reached
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.8Z     SEMANTIC_VALIDATION_SKIPPED (transient validator error): Api 503
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:01.9Z ^[[32;1m        PASS^[[0m [   1.000s] (2/2) ^[[35;1mgenai-rs::tools_and_config_tests^[[0m ^[[34;1mtest_other^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:02.0Z ##[warning]tools_and_config_tests: 2 unverified assertion(s) or tolerated failure(s) (grep SEMANTIC_VALIDATION_SKIPPED or LIVE_TOOL_EVIDENCE_SKIPPED in the log for causes)
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:03.0Z     Starting 1 test across 1 binary
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:03.1Z     SEMANTIC_VALIDATION_SKIPPED (missing-verdict): printed before any test
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:03.2Z ^[[32;1m        PASS^[[0m [   1.000s] (1/1) ^[[35;1mgenai-rs::tools_and_config_tests^[[0m ^[[36mmcp^[[0m^[[36m::^[[0m^[[34;1mtest_mcp^[[0m
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:03.3Z     LIVE_TOOL_EVIDENCE_SKIPPED: again
Integration Tests (tools)	Run integration tests (tools)	2026-11-05T10:00:03.4Z ^[[32;1m     Summary^[[0m [   1.000s] 1 test run: 1 passed, 0 skipped
Test Suite	Run unit tests	2026-11-05T10:00:04.0Z ##[group]Run cargo nextest run --workspace
Test Suite	Run unit tests	2026-11-05T10:00:04.1Z ##[endgroup]
Test Suite	Run unit tests	2026-11-05T10:00:05.0Z     Starting 1 test across 1 binary
Test Suite	Run unit tests	2026-11-05T10:00:05.1Z         PASS [   0.010s] (1/1) genai-rs::voices_tests test_voice_list
Test Suite	Run unit tests	2026-11-05T10:00:05.2Z     LIVE_TOOL_EVIDENCE_SKIPPED: not a retried step
LOG
    out=$("$SCRIPT" "$WORK/markers.log")
    expect_json "markers: every marker line in the retried step, none from its script or warning" \
        .skip_markers 4
    expect_json "markers: by the test that printed them, with its runs" \
        '[.skips[] | [.binary, .name, .markers, .runs]]' \
        '[["tools_and_config_tests","mcp::test_mcp",3,2],["","",1,0]]'
    expect_json "markers: not flakes" '[.tests_run, .flaky]' '[3,0]'

    # --- Nothing to count: a clean run, and no input at all.
    printf '     Summary [   1.000s] 7 tests run: 7 passed, 0 skipped\n' > "$WORK/clean.log"
    out=$("$SCRIPT" "$WORK/clean.log")
    expect_json "clean run: no flakes" '[.tests_run, .flaky, (.tests | length)]' '[7,0,0]'
    out=$("$SCRIPT" < /dev/null)
    expect_json "empty input: zeros, not an error" '[.tests_run, .flaky, (.tests | length)]' '[0,0,0]'
    expect_json "empty input: no examples" '[.examples_run, .examples_retried, (.examples | length)]' '[0,0,0]'
    expect_json "empty input: no skip markers" '[.skip_markers, (.skips | length)]' '[0,0]'
done

unset AWK

# --- Into the report: a green week's flakes reach the rendered issue body,
#     with the rate against the 1% target. tests.json is shaped the way the
#     workflow's collect step writes it from the counter's output. The
#     report exits 3 when a target is breached (test_generate_flakiness_report.sh
#     covers each one).
REPORT="$(dirname "$HERE")/generate_flakiness_report.sh"
render_report() {
    local counted="$1"
    jq '[.tests[] | {name, failures: 0, retried: .flaky, runs: .runs, category: "passed on retry",
                     file: "tests/\(.binary).rs"}]' <<<"$counted" > "$WORK/tests.json"
    jq --argjson c "$counted" -n '{
        current: {total_runs: 4, failed_runs: 0, unique_flaky: ($c.tests | length),
                  retried_flakes: $c.flaky, live_test_runs: $c.tests_run,
                  api_errors: 0, rate_limit: 0, assertion_failures: 0, panic: 0, unknown: 0},
        trends_24h: {available: false}, trends_7d: {available: false},
        deltas_24h: {}, deltas_7d: {}}' > "$WORK/trends.json"
    report_rc=0
    out=$(bash "$REPORT" --trends "$WORK/trends.json" --tests "$WORK/tests.json" \
        --start-date 2026-11-01 --end-date 2026-11-08 --repo-url https://example.invalid 2>&1) \
        || report_rc=$?
}
expect_rc() {
    local label="$1" want="$2"
    if [ "$report_rc" = "$want" ]; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: exited $report_rc, expected $want"
        failures=$((failures + 1))
    fi
}
expect_report() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" <<<"$out"; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: expected to find '$needle'"
        printf '%s\n' "$out" | sed 's/^/         /'
        failures=$((failures + 1))
    fi
}
refute_report() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" <<<"$out"; then
        echo "FAIL - $label: found '$needle'"
        failures=$((failures + 1))
    else
        echo "ok   - $label"
    fi
}

render_report "$("$SCRIPT" "$FIXTURES/green_integration_job.txt")"
expect_report "report: a green week's retried flake is listed" \
    '| `test_pdf_from_temp_file` | 0 | 1 | passed on retry | tests/temp_file_tests.rs |'
expect_report "report: the rate row counts flakes over live test runs" \
    '| Passed on retry | 3 of 5 live test runs (60.00%) |'
expect_report "report: a rate over the target is called out" \
    'at or over the 1% weekly target'
refute_report "report: a flaky week does not read as healthy" 'CI looks healthy'
expect_rc "report: a rate over the target fails the report" 3

# 1 flake in 200 test runs is 0.5%, under the target.
render_report '{"tests_run": 200, "flaky": 1, "tests": [{"binary": "voices_tests", "name": "test_voice_list", "flaky": 1, "failed_tries": 1}]}'
expect_report "report: a rate under the target is shown" '1 of 200 live test runs (0.50%)'
refute_report "report: a rate under the target is not called out" 'weekly target'
expect_rc "report: a rate under the target passes" 0

echo
if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed."
    exit 1
fi
echo "All checks passed (awk: ${awks[*]})."
