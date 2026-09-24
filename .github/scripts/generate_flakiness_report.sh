#!/usr/bin/env bash
# Generates markdown flakiness report from JSON data.
# Usage: generate_flakiness_report.sh --trends <trends.json> --tests <tests.json> --start-date YYYY-MM-DD --end-date YYYY-MM-DD
# Output: Markdown to stdout

set -euo pipefail

TRENDS_FILE=""
TESTS_FILE=""
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

TOTAL_RUNS=$(echo "$TRENDS" | jq -r '.current.total_runs')
FAILED_RUNS=$(echo "$TRENDS" | jq -r '.current.failed_runs')
UNIQUE_FLAKY=$(echo "$TRENDS" | jq -r '.current.unique_flaky')
API_ERRORS=$(echo "$TRENDS" | jq -r '.current.api_errors')
RATE_LIMIT=$(echo "$TRENDS" | jq -r '.current.rate_limit')
ASSERTION_FAILURES=$(echo "$TRENDS" | jq -r '.current.assertion_failures')
PANIC=$(echo "$TRENDS" | jq -r '.current.panic')
UNKNOWN=$(echo "$TRENDS" | jq -r '.current.unknown')

if [ "$TOTAL_RUNS" -gt 0 ]; then
  FAILURE_RATE=$(awk "BEGIN {printf \"%.1f\", ($FAILED_RUNS / $TOTAL_RUNS) * 100}")
else
  FAILURE_RATE="0.0"
fi

HAS_24H=$(echo "$TRENDS" | jq -r '.trends_24h.available')
HAS_7D=$(echo "$TRENDS" | jq -r '.trends_7d.available')

format_trend() {
  local value="$1"
  local trend="$2"
  local delta="$3"

  if [ "$trend" = "new" ]; then
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

if [ "$HAS_24H" = "true" ]; then
  COL_24H_FAILED=$(format_trend "" "$TREND_24H_FAILED" "$DELTA_24H_FAILED")
  COL_24H_FLAKY=$(format_trend "" "$TREND_24H_FLAKY" "$DELTA_24H_FLAKY")
else
  COL_24H_FAILED="—"
  COL_24H_FLAKY="—"
fi

if [ "$HAS_7D" = "true" ]; then
  COL_7D_FAILED=$(format_trend "" "$TREND_7D_FAILED" "$DELTA_7D_FAILED")
  COL_7D_FLAKY=$(format_trend "" "$TREND_7D_FLAKY" "$DELTA_7D_FLAKY")
else
  COL_7D_FAILED="—"
  COL_7D_FLAKY="—"
fi

cat <<EOF
| Total runs | $TOTAL_RUNS | — | — |
| Failed runs | $FAILED_RUNS ($FAILURE_RATE%) | $COL_24H_FAILED | $COL_7D_FAILED |
| Unique flaky tests | $UNIQUE_FLAKY | $COL_24H_FLAKY | $COL_7D_FLAKY |
EOF

TOTAL_CATEGORIZED=$((API_ERRORS + RATE_LIMIT + ASSERTION_FAILURES + PANIC + UNKNOWN))
if [ "$TOTAL_CATEGORIZED" -gt 0 ]; then
  cat <<EOF

### Failure Breakdown

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

  format_category_row "API errors (503/timeout)" "$API_ERRORS" "api_errors"
  format_category_row "Rate limit (429)" "$RATE_LIMIT" "rate_limit"
  format_category_row "Assertion failures" "$ASSERTION_FAILURES" "assertion_failures"
  format_category_row "Panics" "$PANIC" "panic"
  format_category_row "Unknown" "$UNKNOWN" "unknown"
fi

TEST_COUNT=$(echo "$TESTS" | jq 'length')
if [ "$TEST_COUNT" -gt 0 ]; then
  cat <<EOF

### Top Flaky Tests

| Test | Failures | Category | File |
|------|----------|----------|------|
EOF

  echo "$TESTS" | jq -r '
    sort_by(-.failures) |
    .[:10][] |
    "| `\(.name)` | \(.failures) | \(.category) | \(.file) |"
  '
else
  cat <<EOF

### Top Flaky Tests

No test failures detected in the reporting period.
EOF
fi

echo ""
echo "### Recommendations"
echo ""

RECOMMENDATIONS=0

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

if [ "$RECOMMENDATIONS" -eq 0 ]; then
  echo "No specific recommendations — CI looks healthy!"
fi

cat <<EOF

---
*Generated automatically by [CI Flakiness Report](${REPO_URL:-https://github.com}/actions/workflows/ci-flakiness-report.yml)*
EOF
