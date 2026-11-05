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
# mawk, a Mac has BSD awk.

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
    out=$(bash "$SCRIPT" "$FIXTURES/green_integration_job.txt")
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
    out=$(bash "$SCRIPT" < "$FIXTURES/red_run_view.txt")
    expect_json "red run: tests run" .tests_run 3
    expect_json "red run: only the test that recovered is a flake" \
        '[.tests[] | .name]' '["test_stateful_conversation"]'

    # --- Two logs at once, as the workflow passes a run's jobs.
    out=$(bash "$SCRIPT" "$FIXTURES/green_integration_job.txt" "$FIXTURES/red_run_view.txt")
    expect_json "two logs: totals add up" '[.tests_run, .flaky]' '[8,4]'

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
    out=$(bash "$SCRIPT" "$WORK/twice.log")
    expect_json "two runs: the same test counts once per run" \
        '[.tests[] | select(.name == "test_voice_list") | .flaky]' '[2]'
    expect_json "two runs: a lib unit test is under lib" \
        '[.tests[] | select(.binary == "lib") | .name]' '["request::tests::builds_the_body"]'
    expect_json "two runs: tests run" .tests_run 4

    # --- Nothing to count: a clean run, and no input at all.
    printf '     Summary [   1.000s] 7 tests run: 7 passed, 0 skipped\n' > "$WORK/clean.log"
    out=$(bash "$SCRIPT" "$WORK/clean.log")
    expect_json "clean run: no flakes" '[.tests_run, .flaky, (.tests | length)]' '[7,0,0]'
    out=$(bash "$SCRIPT" < /dev/null)
    expect_json "empty input: zeros, not an error" '[.tests_run, .flaky, (.tests | length)]' '[0,0,0]'
done

unset AWK

# --- Into the report: a green week's flakes reach the rendered issue body,
#     with the rate against the 1% target. tests.json is shaped the way the
#     workflow's collect step writes it from the counter's output.
REPORT="$(dirname "$HERE")/generate_flakiness_report.sh"
render_report() {
    local counted="$1"
    jq '[.tests[] | {name, failures: 0, retried: .flaky, category: "passed on retry",
                     file: "tests/\(.binary).rs"}]' <<<"$counted" > "$WORK/tests.json"
    jq --argjson c "$counted" -n '{
        current: {total_runs: 4, failed_runs: 0, unique_flaky: ($c.tests | length),
                  retried_flakes: $c.flaky, live_test_runs: $c.tests_run,
                  api_errors: 0, rate_limit: 0, assertion_failures: 0, panic: 0, unknown: 0},
        trends_24h: {available: false}, trends_7d: {available: false},
        deltas_24h: {}, deltas_7d: {}}' > "$WORK/trends.json"
    out=$(bash "$REPORT" --trends "$WORK/trends.json" --tests "$WORK/tests.json" \
        --start-date 2026-11-01 --end-date 2026-11-08 --repo-url https://example.invalid 2>&1)
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

render_report "$(bash "$SCRIPT" "$FIXTURES/green_integration_job.txt")"
expect_report "report: a green week's retried flake is listed" \
    '| `test_pdf_from_temp_file` | 0 | 1 | passed on retry | tests/temp_file_tests.rs |'
expect_report "report: the rate row counts flakes over live test runs" \
    '| Passed on retry | 3 of 5 live test runs (60.00%) |'
expect_report "report: a rate over the target is called out" \
    'at or over the 1% weekly target'
refute_report "report: a flaky week does not read as healthy" 'CI looks healthy'

# 1 flake in 200 test runs is 0.5%, under the target.
render_report '{"tests_run": 200, "flaky": 1, "tests": [{"binary": "voices_tests", "name": "test_voice_list", "flaky": 1, "failed_tries": 1}]}'
expect_report "report: a rate under the target is shown" '1 of 200 live test runs (0.50%)'
refute_report "report: a rate under the target is not called out" 'weekly target'

echo
if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed."
    exit 1
fi
echo "All checks passed (awk: ${awks[*]})."
