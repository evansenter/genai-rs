#!/usr/bin/env bash
# Generates markdown flakiness report from JSON data.
# Usage: generate_flakiness_report.sh --trends <trends.json> --tests <tests.json>
#          [--summary <summary.json>] --start-date YYYY-MM-DD --end-date YYYY-MM-DD
#          [--repo-url URL]
# Output: Markdown to stdout
#
# The current window's numbers come from --summary (the collect step's
# summary.json) when given, else from the trends file's `current`, which
# compare_trends.sh copies for the fields it trends.
#
# Exit status: 0 for a healthy week; 3 when the report is complete but a
# target is breached, so the workflow can publish it and then fail (a
# failed scheduled run escalates, #431):
#   - the week's flake rate, tests that passed only on a retry over live
#     test runs, is 1% or more (#504);
#   - no live test runs were read, so the rate is unknown;
#   - a chronic flake: a test or example that passed only on a retry in
#     more than 5% of the runs it ran in, and in at least two.
# Anything else non-zero is an error.

set -euo pipefail

TRENDS_FILE=""
TESTS_FILE=""
SUMMARY_FILE=""
START_DATE=""
END_DATE=""
REPO_URL=""

while [[ $# -gt 0 ]]; do
  case $1 in
    --trends)
      TRENDS_FILE="$2"
      shift 2
      ;;
    --tests)
      TESTS_FILE="$2"
      shift 2
      ;;
    --summary)
      SUMMARY_FILE="$2"
      shift 2
      ;;
    --start-date)
      START_DATE="$2"
      shift 2
      ;;
    --end-date)
      END_DATE="$2"
      shift 2
      ;;
    --repo-url)
      REPO_URL="$2"
      shift 2
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

: "${TRENDS_FILE:?--trends required}"
: "${TESTS_FILE:?--tests required}"
: "${START_DATE:?--start-date required}"
: "${END_DATE:?--end-date required}"

TRENDS=$(cat "$TRENDS_FILE")
TESTS=$(cat "$TESTS_FILE")
if [ -n "$SUMMARY_FILE" ]; then
  SUMMARY=$(cat "$SUMMARY_FILE")
else
  SUMMARY=$(jq '.current' <<<"$TRENDS")
fi

# A current-window value: the summary's, else the trends file's, else the
# default (absent from summaries written before the field existed).
current() {
  jq -r --arg key "$1" --arg default "$2" --argjson trends "$TRENDS" \
    '.[$key] // $trends.current[$key] // $default' <<<"$SUMMARY"
}

TOTAL_RUNS=$(current total_runs 0)
FAILED_RUNS=$(current failed_runs 0)
UNIQUE_FLAKY=$(current unique_flaky 0)
API_ERRORS=$(current api_errors 0)
RATE_LIMIT=$(current rate_limit 0)
ASSERTION_FAILURES=$(current assertion_failures 0)
PANIC=$(current panic 0)
DETERMINISTIC=$(current deterministic 0)
UNKNOWN=$(current unknown 0)
# Failed runs by event: main (push, schedule, dispatch) and the rest
# (pull requests, branches). Empty when the summary predates the split.
MAIN_RUNS=$(current main_runs "")
FAILED_MAIN=$(current failed_runs_main "")
BRANCH_RUNS=$(current branch_runs "")
FAILED_BRANCH=$(current failed_runs_branch "")
# Tests nextest retried to a pass, from every run's live jobs, green ones
# included (#492), and the test runs they are a share of.
RETRIED=$(current retried_flakes 0)
LIVE_RUNS=$(current live_test_runs 0)
# Examples the smoke loops retried to a pass, and the example runs.
EXAMPLES_RUN=$(current examples_run 0)
EXAMPLES_RETRIED=$(current examples_retried 0)
# The weekly flake rate the Tests/CI bar holds under 1% (#504).
FLAKE_TARGET_PCT=1
# A test's own share of runs that needed a retry, past which it is a
# chronic flake, and the fewest such runs that count as chronic.
CHRONIC_PCT=5
CHRONIC_MIN=2

percent() {
  awk -v n="$1" -v d="$2" -v p="$3" 'BEGIN { printf "%.*f", p, (n / d) * 100 }'
}

if [ "$TOTAL_RUNS" -gt 0 ]; then
  FAILURE_RATE=$(percent "$FAILED_RUNS" "$TOTAL_RUNS" 1)
else
  FAILURE_RATE="0.0"
fi

# No live test runs is no measurement, not a clean week.
NO_LIVE_RUNS=0
if [ "$LIVE_RUNS" -gt 0 ]; then
  FLAKE_RATE=$(percent "$RETRIED" "$LIVE_RUNS" 2)
  FLAKE_RATE_CELL="$FLAKE_RATE%"
  OVER_TARGET=$(awk -v r="$FLAKE_RATE" -v t="$FLAKE_TARGET_PCT" 'BEGIN { print (r >= t) ? 1 : 0 }')
else
  FLAKE_RATE="n/a"
  FLAKE_RATE_CELL="n/a: no live test runs were read"
  OVER_TARGET=0
  NO_LIVE_RUNS=1
fi

# Chronic flakes, worst share first: `retried` runs that passed only on a
# retry, of the `runs` the test ran in.
CHRONIC=$(jq -c --argjson pct "$CHRONIC_PCT" --argjson min "$CHRONIC_MIN" '
  [.[] | select((.retried // 0) >= $min and (.runs // 0) > 0
                and ((.retried / .runs) * 100) > $pct)]
  | sort_by(-(.retried / .runs), .name)' <<<"$TESTS")
CHRONIC_COUNT=$(jq 'length' <<<"$CHRONIC")

HAS_24H=$(echo "$TRENDS" | jq -r '.trends_24h.available')
HAS_7D=$(echo "$TRENDS" | jq -r '.trends_7d.available')

format_trend() {
  local value="$1"
  local trend="$2"
  local delta="$3"

  if [ "$trend" = "null" ] || [ -z "$trend" ]; then
    # Not trended (a field compare_trends.sh does not carry).
    echo "—"
  elif [ "$trend" = "new" ]; then
    echo "$value (new)"
  elif [ "$delta" = "null" ]; then
    echo "$value"
  elif [ "$delta" -gt 0 ] 2>/dev/null; then
    echo "$value $trend +$delta"
  elif [ "$delta" -lt 0 ] 2>/dev/null; then
    echo "$value $trend $delta"
  else
    echo "$value $trend"
  fi
}

cat <<EOF
## CI Flakiness Report ($START_DATE to $END_DATE)

### Summary

| Metric | Current | 24hr | 7-day |
|--------|---------|------|-------|
EOF

TREND_24H_FAILED=$(echo "$TRENDS" | jq -r '.trends_24h.failed_runs')
TREND_7D_FAILED=$(echo "$TRENDS" | jq -r '.trends_7d.failed_runs')
DELTA_24H_FAILED=$(echo "$TRENDS" | jq -r '.deltas_24h.failed_runs')
DELTA_7D_FAILED=$(echo "$TRENDS" | jq -r '.deltas_7d.failed_runs')

TREND_24H_FLAKY=$(echo "$TRENDS" | jq -r '.trends_24h.unique_flaky')
TREND_7D_FLAKY=$(echo "$TRENDS" | jq -r '.trends_7d.unique_flaky')
DELTA_24H_FLAKY=$(echo "$TRENDS" | jq -r '.deltas_24h.unique_flaky')
DELTA_7D_FLAKY=$(echo "$TRENDS" | jq -r '.deltas_7d.unique_flaky')

TREND_24H_RETRIED=$(echo "$TRENDS" | jq -r '.trends_24h.retried_flakes // "new"')
TREND_7D_RETRIED=$(echo "$TRENDS" | jq -r '.trends_7d.retried_flakes // "new"')
DELTA_24H_RETRIED=$(echo "$TRENDS" | jq -r '.deltas_24h.retried_flakes // null')
DELTA_7D_RETRIED=$(echo "$TRENDS" | jq -r '.deltas_7d.retried_flakes // null')

if [ "$HAS_24H" = "true" ]; then
  COL_24H_FAILED=$(format_trend "" "$TREND_24H_FAILED" "$DELTA_24H_FAILED")
  COL_24H_FLAKY=$(format_trend "" "$TREND_24H_FLAKY" "$DELTA_24H_FLAKY")
  COL_24H_RETRIED=$(format_trend "" "$TREND_24H_RETRIED" "$DELTA_24H_RETRIED")
else
  COL_24H_FAILED="—"
  COL_24H_FLAKY="—"
  COL_24H_RETRIED="—"
fi

if [ "$HAS_7D" = "true" ]; then
  COL_7D_FAILED=$(format_trend "" "$TREND_7D_FAILED" "$DELTA_7D_FAILED")
  COL_7D_FLAKY=$(format_trend "" "$TREND_7D_FLAKY" "$DELTA_7D_FLAKY")
  COL_7D_RETRIED=$(format_trend "" "$TREND_7D_RETRIED" "$DELTA_7D_RETRIED")
else
  COL_7D_FAILED="—"
  COL_7D_FLAKY="—"
  COL_7D_RETRIED="—"
fi

# `x of y <unit> (z%)`, with no share when y is 0.
share_cell() {
  if [ "$2" -gt 0 ]; then
    echo "$1 of $2$4 ($(percent "$1" "$2" "$3")%)"
  else
    echo "$1 of 0$4"
  fi
}

echo "| Total runs | $TOTAL_RUNS | — | — |"
echo "| Failed runs | $FAILED_RUNS ($FAILURE_RATE%) | $COL_24H_FAILED | $COL_7D_FAILED |"
if [ -n "$MAIN_RUNS" ] && [ -n "$FAILED_MAIN" ] && [ -n "$BRANCH_RUNS" ] && [ -n "$FAILED_BRANCH" ]; then
  echo "| Failed on main (push, schedule, dispatch) | $(share_cell "$FAILED_MAIN" "$MAIN_RUNS" 1 "") | — | — |"
  echo "| Failed on pull requests and branches (not categorized) | $(share_cell "$FAILED_BRANCH" "$BRANCH_RUNS" 1 "") | — | — |"
fi
echo "| Unique flaky tests | $UNIQUE_FLAKY | $COL_24H_FLAKY | $COL_7D_FLAKY |"
echo "| Passed on retry | $RETRIED of $LIVE_RUNS live test runs ($FLAKE_RATE_CELL) | $COL_24H_RETRIED | $COL_7D_RETRIED |"
if [ "$EXAMPLES_RUN" -gt 0 ] || [ "$EXAMPLES_RETRIED" -gt 0 ]; then
  echo "| Examples passed on retry | $(share_cell "$EXAMPLES_RETRIED" "$EXAMPLES_RUN" 2 " example runs") | — | — |"
fi

BREACHES=0
if [ "$OVER_TARGET" -eq 1 ]; then
  BREACHES=$((BREACHES + 1))
  echo
  echo "**The flake rate is $FLAKE_RATE%, at or over the ${FLAKE_TARGET_PCT}% weekly target.**"
fi
if [ "$NO_LIVE_RUNS" -eq 1 ]; then
  BREACHES=$((BREACHES + 1))
  echo
  echo "**No live test runs were read, so the flake rate is unknown.** The retry jobs' logs are missing or no longer match \`RETRY_JOBS\` in ci-flakiness-report.yml."
fi
if [ "$CHRONIC_COUNT" -gt 0 ]; then
  BREACHES=$((BREACHES + 1))
  echo
  echo "**$CHRONIC_COUNT chronic flake(s): passed only on a retry in more than ${CHRONIC_PCT}% of their runs.**"
fi

TOTAL_CATEGORIZED=$((API_ERRORS + RATE_LIMIT + ASSERTION_FAILURES + PANIC + DETERMINISTIC + UNKNOWN))
if [ "$TOTAL_CATEGORIZED" -gt 0 ]; then
  cat <<EOF

### Failure Breakdown

Failed tests on main's runs; a pull request's red run is usually the change under review.

| Category | Count | 24hr | 7-day |
|----------|-------|------|-------|
EOF

  format_category_row() {
    local name="$1"
    local count="$2"
    local key="$3"

    if [ "$count" -eq 0 ]; then
      return
    fi

    local trend_24h delta_24h col_24h
    local trend_7d delta_7d col_7d

    trend_24h=$(echo "$TRENDS" | jq -r ".trends_24h.$key")
    delta_24h=$(echo "$TRENDS" | jq -r ".deltas_24h.$key")
    trend_7d=$(echo "$TRENDS" | jq -r ".trends_7d.$key")
    delta_7d=$(echo "$TRENDS" | jq -r ".deltas_7d.$key")

    if [ "$HAS_24H" = "true" ]; then
      col_24h=$(format_trend "" "$trend_24h" "$delta_24h")
    else
      col_24h="—"
    fi

    if [ "$HAS_7D" = "true" ]; then
      col_7d=$(format_trend "" "$trend_7d" "$delta_7d")
    else
      col_7d="—"
    fi

    echo "| $name | $count | $col_24h | $col_7d |"
  }

  format_category_row "Deterministic (every try failed, on one OS only)" "$DETERMINISTIC" "deterministic"
  format_category_row "API errors (503/timeout)" "$API_ERRORS" "api_errors"
  format_category_row "Rate limit (429)" "$RATE_LIMIT" "rate_limit"
  format_category_row "Assertion failures" "$ASSERTION_FAILURES" "assertion_failures"
  format_category_row "Panics" "$PANIC" "panic"
  format_category_row "Unknown" "$UNKNOWN" "unknown"
fi

if [ "$CHRONIC_COUNT" -gt 0 ]; then
  cat <<EOF

### Chronic Flakes

Passed only on a retry in more than ${CHRONIC_PCT}% of the runs they ran in (and in at least ${CHRONIC_MIN}). Each one fails this report until it is fixed.

| Test | Passed on retry | Runs | Share | File |
|------|-----------------|------|-------|------|
EOF
  jq -r '.[] | "| `\(.name)` | \(.retried) | \(.runs) | \((.retried / .runs * 1000 | round) / 10)% | \(.file) |"' <<<"$CHRONIC"
fi

TEST_COUNT=$(echo "$TESTS" | jq 'length')
if [ "$TEST_COUNT" -gt 0 ]; then
  cat <<EOF

### Top Flaky Tests

| Test | Failures | Passed on retry | Category | File |
|------|----------|-----------------|----------|------|
EOF

  # `// 0`: tests.json from before #492 has no `retried`.
  echo "$TESTS" | jq -r '
    sort_by(-(.failures + (.retried // 0)), .name) |
    .[:10][] |
    "| `\(.name)` | \(.failures) | \(.retried // 0) | \(.category) | \(.file) |"
  '
else
  cat <<EOF

### Top Flaky Tests

No test failures or retried flakes detected in the reporting period.
EOF
fi

echo ""
echo "### Recommendations"
echo ""

RECOMMENDATIONS=0

if [ "$NO_LIVE_RUNS" -eq 1 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Find the retry-job logs** — no live test runs were read this week, so nothing here measures the flake rate"
fi

if [ "$CHRONIC_COUNT" -gt 0 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Fix the chronic flakes** — $CHRONIC_COUNT test(s) or example(s) passed only on a retry in more than ${CHRONIC_PCT}% of their runs"
fi

if [ "$DETERMINISTIC" -gt 0 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Fix the deterministic failures** — $DETERMINISTIC failed every try on one OS only: a break on that platform, not a flake"
fi

if [ "$API_ERRORS" -gt 0 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Investigate API reliability** — $API_ERRORS failures due to backend/network issues (503, timeout, connection errors)"
fi

if [ "$RATE_LIMIT" -gt 0 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Review rate limits** — $RATE_LIMIT failures due to quota issues (429, rate limit)"
fi

if [ "$ASSERTION_FAILURES" -gt 0 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Fix assertion failures** — $ASSERTION_FAILURES tests with logic errors that need investigation"
fi

if [ "$PANIC" -gt 0 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Investigate panics** — $PANIC tests crashed unexpectedly"
fi

if [ "$OVER_TARGET" -eq 1 ]; then
  RECOMMENDATIONS=$((RECOMMENDATIONS + 1))
  echo "$RECOMMENDATIONS. **Fix the retried flakes** — $RETRIED of $LIVE_RUNS live test runs ($FLAKE_RATE%) passed only on a retry, over the ${FLAKE_TARGET_PCT}% target"
fi

if [ "$RECOMMENDATIONS" -eq 0 ]; then
  echo "No specific recommendations — CI looks healthy!"
fi

cat <<EOF

---
*Generated automatically by [CI Flakiness Report](${REPO_URL:-https://github.com}/actions/workflows/ci-flakiness-report.yml)*
EOF

if [ "$BREACHES" -gt 0 ]; then
  exit 3
fi
