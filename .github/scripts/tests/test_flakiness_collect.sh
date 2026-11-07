#!/usr/bin/env bash
# Runs the collect and report steps of `ci-flakiness-report.yml` against a
# stubbed `gh` (#504).
#
# The steps are extracted from the workflow, not copied, as
# test_flakiness_rolling_issue.sh does for the issue step. What is pinned:
#   - failed runs are split by event: only main's are categorized, so a
#     pull request's own breakage is not reported as a flake;
#   - a one-OS break is labelled deterministic, with its OS;
#   - green runs' retry-job logs are read, the example loop's included,
#     and each flake carries the runs it ran in;
#   - a breached target finishes the report and records `breach=true`
#     instead of stopping the step before the baseline marker is written.

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
    echo "jq is required to run this harness (brew install jq / apt install jq)" >&2
    exit 1
}
# The collect step uses associative arrays, as the runner's bash allows.
if [ "${BASH_VERSINFO[0]}" -lt 4 ]; then
    if [ -n "${CI:-}" ]; then
        echo "FAIL - bash ${BASH_VERSION} has no associative arrays" >&2
        exit 1
    fi
    echo "skip - bash ${BASH_VERSION} is too old for the collect step (needs 4+)"
    exit 0
fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/ci-flakiness-report.yml"
FIXTURES="$HERE/fixtures"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# A step's `run:` body, dedented, with `${{ }}` replaced as Actions does
# before bash sees it.
extract_step() {
    awk -v want="$1" '
      $0 ~ ("^      - name: " want "$") { in_step = 1; next }
      in_step && /^        run: \|/ { in_run = 1; next }
      in_run {
        if ($0 !~ /^          / && $0 !~ /^[[:space:]]*$/) { exit }
        sub(/^          /, "")
        print
      }
    ' "$WORKFLOW" | sed 's/\${{[^}]*}}/EXPR/g'
}

extract_step "Collect and categorize failures" > "$WORK/collect.sh"
extract_step "Generate report" > "$WORK/report.sh"
for step in collect report; do
    if ! grep -q "set -euo pipefail" "$WORK/$step.sh"; then
        echo "FAIL - could not extract the $step step from $WORKFLOW (was it renamed?)"
        exit 1
    fi
done

# The steps run from the checkout's root: scripts by relative path, and
# tests/ and src/ for the file lookup.
RUN_DIR="$WORK/checkout"
mkdir -p "$RUN_DIR"
for dir in .github tests src; do
    ln -s "$ROOT/$dir" "$RUN_DIR/$dir"
done

# --- The week: runs relative to now, so the 7-day window holds them.
STUB="$WORK/stub"
mkdir -p "$STUB/bin"
ago() { jq -nr --argjson d "$1" 'now - $d * 86400 | floor | todate'; }
jq -n --arg d1 "$(ago 1)" --arg d2 "$(ago 2)" --arg d3 "$(ago 3)" --arg old "$(ago 10)" '[
  {databaseId: 101, conclusion: "failure", createdAt: $d1, headBranch: "main", event: "push"},
  {databaseId: 102, conclusion: "failure", createdAt: $d1, headBranch: "feature", event: "pull_request"},
  {databaseId: 103, conclusion: "success", createdAt: $d2, headBranch: "main", event: "schedule"},
  {databaseId: 104, conclusion: "success", createdAt: $old, headBranch: "main", event: "push"},
  {databaseId: 105, conclusion: "failure", createdAt: $d3, headBranch: "main", event: "pull_request"}
]' > "$STUB/runs.json"

# Run 101 (main push): both Windows legs break one test, every try. Its
# jobs list both legs, which categorizing checks before calling the break
# deterministic.
cp "$FIXTURES/flakiness/windows_legs_run_view.txt" "$STUB/failed-101.log"
jq -n '{jobs: ([
  "Cross Platform (ubuntu-latest, stable)", "Cross Platform (ubuntu-latest, beta)",
  "Cross Platform (windows-latest, stable)", "Cross Platform (windows-latest, beta)",
  "Test Suite"] | to_entries
  | map({name: .value, databaseId: (2000 + .key),
         conclusion: (if (.value | test("windows")) then "failure" else "success" end)}))}' \
    > "$STUB/jobs-101.json"
# Run 102 (a pull request): its own test fails. Not a flake.
printf 'Test Suite\tRun unit tests\t2026-11-05T10:00:00.0Z         FAIL [   0.010s] (1/1) genai-rs::ci_coverage test_the_pr_broke\n' \
    > "$STUB/failed-102.log"
# Run 105: a fork's pull request from its own main is still a pull request.
cp "$STUB/failed-102.log" "$STUB/failed-105.log"

# Retry jobs: the integration group and the example loop, on run 103.
jq -n '{jobs: [
  {name: "Integration Tests (core)", conclusion: "success", databaseId: 1001},
  {name: "Example Smoke Run", conclusion: "success", databaseId: 1002},
  {name: "Test Suite", conclusion: "success", databaseId: 1003}
]}' > "$STUB/jobs-103.json"
cat > "$STUB/job-1001.log" <<'LOG'
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:00.0Z ##[group]Run set -eo pipefail
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:00.1Z   NEXTEST_RETRIES: 2
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:00.2Z ##[endgroup]
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:01.0Z     Starting 4 tests across 1 binary
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:01.1Z   TRY 1 FAIL [   1.000s] (─────) genai-rs::interactions_api_tests generation_config::test_continuation
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:01.2Z   TRY 2 PASS [   1.000s] (1/4) genai-rs::interactions_api_tests generation_config::test_continuation
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:01.3Z      Summary [   2.000s] 4 tests run: 4 passed (1 flaky), 0 skipped
Integration Tests (core)	Run integration tests (core)	2026-11-05T10:00:01.4Z    FLAKY 2/3 [   1.000s] (1/4) genai-rs::interactions_api_tests generation_config::test_continuation
LOG
cat > "$STUB/job-1002.log" <<'LOG'
Example Smoke Run	Run examples	2026-11-05T10:00:00.0Z ##[group]Run set -eo pipefail
Example Smoke Run	Run examples	2026-11-05T10:00:00.1Z ##[endgroup]
Example Smoke Run	Run examples	2026-11-05T10:00:01.0Z ##[group]cargo run --example streaming
Example Smoke Run	Run examples	2026-11-05T10:00:02.0Z ##[endgroup]
Example Smoke Run	Run examples	2026-11-05T10:00:02.1Z EXAMPLE_RETRIED streaming 1
Example Smoke Run	Run examples	2026-11-05T10:00:03.0Z ##[group]cargo run --example simple_interaction
Example Smoke Run	Run examples	2026-11-05T10:00:04.0Z ##[endgroup]
LOG

# `gh` answers from the files above; a call for anything else is logged
# and answered with nothing, as a run without that data would be.
cat > "$STUB/bin/gh" <<STUB
#!/usr/bin/env bash
dir="$STUB"
echo "gh \$*" >> "\$dir/calls.log"
case "\$1 \$2" in
  "run list") cat "\$dir/runs.json" ;;
  "run view")
    if [ "\$3" = "--job" ]; then
      [ -f "\$dir/job-\$4.log" ] || exit 1
      cat "\$dir/job-\$4.log"
    elif [ "\$4" = "--log-failed" ]; then
      cat "\$dir/failed-\$3.log" 2>/dev/null || true
    elif [ "\$4" = "--json" ]; then
      cat "\$dir/jobs-\$3.json" 2>/dev/null || echo '{"jobs": []}'
    fi
    ;;
esac
STUB
chmod +x "$STUB/bin/gh"

check() {
    local label="$1" file="$2" filter="$3" want="$4" got
    got=$(jq -c "$filter" "$RUN_DIR/$file")
    if [ "$got" = "$want" ]; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: $filter in $file is $got, expected $want"
        failures=$((failures + 1))
    fi
}
check_text() {
    local label="$1" file="$2" needle="$3"
    if grep -qF -- "$needle" "$file"; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: '$needle' not in $file"
        sed 's/^/         /' "$file"
        failures=$((failures + 1))
    fi
}

run_step() {
    local step="$1" rc=0
    : > "$WORK/github_output"
    (cd "$RUN_DIR" && PATH="$STUB/bin:$PATH" GITHUB_OUTPUT="$WORK/github_output" \
        bash "$WORK/$step.sh" > "$WORK/$step.out" 2>&1) || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "FAIL - the $step step exited $rc"
        sed 's/^/         /' "$WORK/$step.out"
        failures=$((failures + 1))
    fi
}

CRLF_TEST="wire_streaming::tests::test_enum_wire_formats_doc_lists_the_serializers_names"

run_step collect
check "collect: the week's runs, the 10-day-old one left out" summary.json \
    '[.total_runs, .failed_runs]' '[4,3]'
check "collect: main and branch runs apart" summary.json \
    '[.main_runs, .failed_runs_main, .branch_runs, .failed_runs_branch]' '[2,1,2,2]'
check "collect: only main's failed run is categorized" summary.json \
    '[.deterministic, .api_errors, .rate_limit, .assertion_failures, .panic, .unknown]' '[1,0,0,0,0,0]'
check "collect: a pull request's failure is not a flake" tests.json \
    '[.[] | select(.name == "test_the_pr_broke")] | length' 0
check "collect: the one-OS break is deterministic, by OS" tests.json \
    "[.[] | select(.name == \"$CRLF_TEST\") | [.failures, .category]]" '[[1,"deterministic on windows"]]'
check "collect: a green run's flake, with its runs" tests.json \
    '[.[] | select(.name == "generation_config::test_continuation") | [.retried, .runs, .file]]' \
    '[[1,1,"tests/interactions_api_tests.rs"]]'
check "collect: the live rate's counts" summary.json '[.retried_flakes, .live_test_runs]' '[1,4]'
check "collect: example runs and retries" summary.json '[.examples_run, .examples_retried]' '[2,1]'
check "collect: a retried example is listed" tests.json \
    '[.[] | select(.name == "example streaming") | [.retried, .runs, .file]]' \
    '[[1,1,"cargo run --example streaming"]]'
if grep -q -- "--log-failed" "$STUB/calls.log" && ! grep -qE "run view (102|105) --log-failed" "$STUB/calls.log"; then
    echo "ok   - collect: pull request runs' logs are not fetched for categorizing"
else
    echo "FAIL - collect: expected --log-failed for run 101 only"
    sed 's/^/         /' "$STUB/calls.log"
    failures=$((failures + 1))
fi

# --- The report step, on the collected data: 1 flake in 4 live test runs
#     is 25%, a breach. The step still finishes the report.
jq '{current: ., trends_24h: {available: false}, trends_7d: {available: false},
     deltas_24h: {}, deltas_7d: {}}' "$RUN_DIR/summary.json" > "$RUN_DIR/trends.json"
run_step report
check_text "report: a breach is recorded" "$WORK/github_output" "breach=true"
check_text "report: the report is complete" "$RUN_DIR/flakiness_report.md" "### Recommendations"
check_text "report: the baseline marker follows it" "$RUN_DIR/flakiness_report.md" "<!-- flaky-baseline"
check_text "report: the deterministic break is in it" "$RUN_DIR/flakiness_report.md" "deterministic on windows"

# --- A healthy week records no breach.
jq '.retried_flakes = 0 | .live_test_runs = 400' "$RUN_DIR/summary.json" > "$WORK/s.json"
mv "$WORK/s.json" "$RUN_DIR/summary.json"
echo '[]' > "$RUN_DIR/tests.json"
run_step report
check_text "report: a healthy week records no breach" "$WORK/github_output" "breach=false"

echo
if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed."
    exit 1
fi
echo "All checks passed."
