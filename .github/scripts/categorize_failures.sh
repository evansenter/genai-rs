#!/usr/bin/env bash
# Categorizes test failures from a GitHub Actions run by error type.
# Usage: categorize_failures.sh <run_id>
# Output: JSON to stdout with categorized failures

set -euo pipefail

RUN_ID="${1:?Usage: categorize_failures.sh <run_id>}"

LOGS=$(gh run view "$RUN_ID" --log-failed 2>/dev/null || echo "")

if [ -z "$LOGS" ]; then
  cat <<EOF
{
  "run_id": $RUN_ID,
  "categories": {
    "api_errors": [],
    "rate_limit": [],
    "assertion_failures": [],
    "panic": [],
    "unknown": []
  },
  "totals": {
    "api_errors": 0,
    "rate_limit": 0,
    "assertion_failures": 0,
    "panic": 0,
    "unknown": 0
  }
}
EOF
  exit 0
fi

declare -a API_ERRORS=()
declare -a RATE_LIMIT=()
declare -a ASSERTION_FAILURES=()
declare -a PANIC=()
declare -a UNKNOWN=()

FAILED_TESTS=$(echo "$LOGS" | grep -oE "test [a-zA-Z0-9_:]+ \.\.\. FAILED" | sed 's/.*test //' | sed 's/ \.\.\..*$//' || true)

PANIC_TESTS=$(echo "$LOGS" | grep -oE "---- [a-zA-Z0-9_:]+ stdout ----" | sed 's/---- //' | sed 's/ stdout ----//' || true)

ALL_TESTS=$(echo -e "$FAILED_TESTS\n$PANIC_TESTS" | sort -u | grep -v '^$' || true)

while IFS= read -r test_name; do
  [ -z "$test_name" ] && continue

  CONTEXT=$(echo "$LOGS" | grep -A 100 -B 20 "$test_name" 2>/dev/null || echo "")

  if echo "$CONTEXT" | grep -qiE '(503|service unavailable|timeout|timed out|deadline exceeded|connection reset|connection refused|spanner.*utf-8|internal server error|gateway)'; then
    API_ERRORS+=("$test_name")
  elif echo "$CONTEXT" | grep -qiE '(429|rate.?limit|quota.*exceed|too many requests|resource exhausted)'; then
    RATE_LIMIT+=("$test_name")
  elif echo "$CONTEXT" | grep -qE '(assertion.*failed|left:.*right:|panicked at.*assert)'; then
    ASSERTION_FAILURES+=("$test_name")
  elif echo "$CONTEXT" | grep -q 'panicked'; then
    PANIC+=("$test_name")
  else
    UNKNOWN+=("$test_name")
  fi
done <<< "$ALL_TESTS"

to_json_array() {
  local arr=("$@")
  if [ ${#arr[@]} -eq 0 ]; then
    echo "[]"
  else
    printf '%s\n' "${arr[@]}" | jq -R . | jq -s .
  fi
}

cat <<EOF
{
  "run_id": $RUN_ID,
  "categories": {
    "api_errors": $(to_json_array "${API_ERRORS[@]+"${API_ERRORS[@]}"}"),
    "rate_limit": $(to_json_array "${RATE_LIMIT[@]+"${RATE_LIMIT[@]}"}"),
    "assertion_failures": $(to_json_array "${ASSERTION_FAILURES[@]+"${ASSERTION_FAILURES[@]}"}"),
    "panic": $(to_json_array "${PANIC[@]+"${PANIC[@]}"}"),
    "unknown": $(to_json_array "${UNKNOWN[@]+"${UNKNOWN[@]}"}")
  },
  "totals": {
    "api_errors": ${#API_ERRORS[@]},
    "rate_limit": ${#RATE_LIMIT[@]},
    "assertion_failures": ${#ASSERTION_FAILURES[@]},
    "panic": ${#PANIC[@]},
    "unknown": ${#UNKNOWN[@]}
  }
}
EOF
