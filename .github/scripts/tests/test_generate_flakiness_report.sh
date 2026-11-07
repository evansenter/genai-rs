#!/usr/bin/env bash
# Exercises `generate_flakiness_report.sh`: what the rolling report says,
# and when it exits 3 so the workflow fails and escalates (#504).
#
# The 1% weekly target used to be printed and nothing more, a week with no
# live test runs read as "0.00%", and a test retried in one run of eight
# hid under a healthy aggregate. Each of those now breaches; these cases pin
# the edges of each.

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
    echo "jq is required to run this harness (brew install jq / apt install jq)" >&2
    exit 1
}

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPORT="$(dirname "$HERE")/generate_flakiness_report.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# The trends file compare_trends.sh writes, with no earlier artifacts.
NO_TRENDS='{"current": {}, "trends_24h": {"available": false}, "trends_7d": {"available": false},
            "deltas_24h": {}, "deltas_7d": {}}'

# render <summary overrides> <tests.json>: a healthy summary.json (500
# live test runs, none retried) with the overrides merged in.
render() {
    jq -n --argjson o "$1" '{
        total_runs: 30, failed_runs: 4, main_runs: 10, failed_runs_main: 1,
        branch_runs: 20, failed_runs_branch: 3, unique_flaky: 0,
        retried_flakes: 0, live_test_runs: 500, examples_run: 0,
        examples_retried: 0, api_errors: 0, rate_limit: 0,
        assertion_failures: 0, panic: 0, deterministic: 0, unknown: 0} + $o' \
        > "$WORK/summary.json"
    printf '%s\n' "$2" > "$WORK/tests.json"
    printf '%s\n' "$NO_TRENDS" > "$WORK/trends.json"
    rc=0
    out=$(bash "$REPORT" --trends "$WORK/trends.json" --summary "$WORK/summary.json" \
        --tests "$WORK/tests.json" --start-date 2026-11-01 --end-date 2026-11-08 \
        --repo-url https://example.invalid 2>&1) || rc=$?
}

expect_rc() {
    local label="$1" want="$2"
    if [ "$rc" = "$want" ]; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: exited $rc, expected $want"
        printf '%s\n' "$out" | sed 's/^/         /'
        failures=$((failures + 1))
    fi
}
expect() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" <<<"$out"; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: expected to find '$needle'"
        printf '%s\n' "$out" | sed 's/^/         /'
        failures=$((failures + 1))
    fi
}
refute() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" <<<"$out"; then
        echo "FAIL - $label: found '$needle'"
        failures=$((failures + 1))
    else
        echo "ok   - $label"
    fi
}

flaky() { # name retried runs
    printf '{"name": "%s", "failures": 0, "retried": %s, "runs": %s, "category": "passed on retry", "file": "tests/x.rs"}' "$1" "$2" "$3"
}

# --- A healthy week passes.
render '{}' '[]'
expect_rc "healthy: exits 0" 0
expect "healthy: says so" 'CI looks healthy'
expect "healthy: the rate is shown" '| Passed on retry | 0 of 500 live test runs (0.00%) |'

# --- The rate target: under it passes, at it fails.
render '{"retried_flakes": 4}' "[$(flaky test_a 4 400)]"
expect_rc "rate 0.80%: under the target, exits 0" 0
render '{"retried_flakes": 5}' "[$(flaky test_a 5 400)]"
expect_rc "rate 1.00%: at the target, exits 3" 3
expect "rate 1.00%: called out" 'The flake rate is 1.00%, at or over the 1% weekly target.'

# --- No live test runs: the rate is unknown, not 0.00%, and that fails.
render '{"live_test_runs": 0}' '[]'
expect_rc "no live runs: exits 3" 3
expect "no live runs: no rate" '| Passed on retry | 0 of 0 live test runs (n/a: no live test runs were read) |'
refute "no live runs: not a clean 0.00%" '0.00%'
expect "no live runs: called out" 'No live test runs were read, so the flake rate is unknown.'
refute "no live runs: not healthy" 'CI looks healthy'

# --- Chronic flakes: over 5% of a test's own runs, in at least two runs.
render '{"retried_flakes": 2}' "[$(flaky test_chronic 13 98), $(flaky test_exactly 5 100), $(flaky test_once 1 10)]"
expect_rc "chronic: exits 3 though the rate is low" 3
expect "chronic: listed with its share" '| `test_chronic` | 13 | 98 | 13.3% | tests/x.rs |'
refute "chronic: exactly 5% is not over it" '| `test_exactly` | 5 | 100 |'
refute "chronic: one retry is not chronic" '| `test_once` | 1 | 10 |'
expect "chronic: recommended" 'Fix the chronic flakes'
render '{"examples_run": 30, "examples_retried": 2}' \
    "[$(flaky 'example streaming' 2 30)]"
expect_rc "chronic example: exits 3" 3
expect "chronic example: listed" '| `example streaming` | 2 | 30 | 6.7% | tests/x.rs |'
expect "examples: their own row" '| Examples passed on retry | 2 of 30 example runs (6.67%) |'

# --- Failed runs by event, and a deterministic break as its own row.
render '{"deterministic": 2}' \
    '[{"name": "crlf_test", "failures": 2, "retried": 0, "runs": 0, "category": "deterministic on windows", "file": "mod.rs"}]'
expect_rc "deterministic alone: no flake target breached" 0
expect "event split: main" '| Failed on main (push, schedule, dispatch) | 1 of 10 (10.0%) |'
expect "event split: pull requests" '| Failed on pull requests and branches (not categorized) | 3 of 20 (15.0%) |'
expect "deterministic: its row" '| Deterministic (every try failed, on every leg of one OS only) | 2 | — | — |'
expect "deterministic: labelled in the table" '| `crlf_test` | 2 | 0 | deterministic on windows | mod.rs |'
expect "deterministic: recommended" 'Fix the deterministic failures'
refute "no examples ran: no example row" 'Examples passed on retry'

# --- A trends file alone (no --summary), as before the split: its
#     `current` is read, and the split rows are left out.
jq -n '{current: {total_runs: 4, failed_runs: 0, unique_flaky: 0, retried_flakes: 0,
                  live_test_runs: 50, api_errors: 0, rate_limit: 0,
                  assertion_failures: 0, panic: 0, unknown: 0},
        trends_24h: {available: true, failed_runs: "→", unique_flaky: "→", retried_flakes: "new"},
        trends_7d: {available: false}, deltas_24h: {failed_runs: 0, unique_flaky: 0}, deltas_7d: {}}' \
    > "$WORK/trends.json"
echo '[]' > "$WORK/tests.json"
rc=0
out=$(bash "$REPORT" --trends "$WORK/trends.json" --tests "$WORK/tests.json" \
    --start-date 2026-11-01 --end-date 2026-11-08 2>&1) || rc=$?
expect_rc "trends only: exits 0" 0
expect "trends only: the rate from current" '| Passed on retry | 0 of 50 live test runs (0.00%) |'
refute "trends only: no event split" 'Failed on main'

# --- A usage error is an error, not a breach.
rc=0
out=$(bash "$REPORT" --bogus 2>&1) || rc=$?
expect_rc "usage error: exits 1" 1

echo
if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed."
    exit 1
fi
echo "All checks passed."
