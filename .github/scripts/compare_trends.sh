#!/usr/bin/env bash
# Compares current flakiness data with 24hr and 7-day old artifacts.
# Usage: compare_trends.sh <artifact_prefix> <current_summary_file>
# Output: JSON with trend indicators to stdout
#
# Requires: GH_TOKEN environment variable

set -euo pipefail

ARTIFACT_PREFIX="${1:?Usage: compare_trends.sh <artifact_prefix> <current_summary_file>}"
CURRENT_FILE="${2:?Usage: compare_trends.sh <artifact_prefix> <current_summary_file>}"

# Read current data
CURRENT_JSON=$(cat "$CURRENT_FILE")

# Calculate dates for comparison
ONE_DAY_AGO=$(date -u -d "1 day ago" +"%Y-%m-%d")
SEVEN_DAYS_AGO=$(date -u -d "7 days ago" +"%Y-%m-%d")

# Function to download artifact by date
download_artifact() {
  local target_date="$1"
  local output_file="$2"
  local artifact_name="${ARTIFACT_PREFIX}-${target_date}"

  # Search for artifact by name
  local artifact_info
  artifact_info=$(gh api \
    "/repos/{owner}/{repo}/actions/artifacts" \
    --jq ".artifacts[] | select(.name == \"$artifact_name\") | {id: .id, expired: .expired}" \
    2>/dev/null || echo "")

  if [ -z "$artifact_info" ]; then
    return 1
  fi

  local artifact_id
  artifact_id=$(echo "$artifact_info" | jq -r '.id')
  local expired
  expired=$(echo "$artifact_info" | jq -r '.expired')

  if [ "$expired" = "true" ]; then
    return 1
  fi

  # Download artifact (uses global ARTIFACT_ZIP for trap cleanup)
  if gh api \
    "/repos/{owner}/{repo}/actions/artifacts/${artifact_id}/zip" \
    > "$ARTIFACT_ZIP" 2>/dev/null; then
    if unzip -p "$ARTIFACT_ZIP" > "$output_file" 2>/dev/null; then
      rm -f "$ARTIFACT_ZIP"
      return 0
    fi
    rm -f "$ARTIFACT_ZIP"
  fi

  return 1
}

# Helper to calculate trend indicator
# Returns: "↑" (>20% increase), "↓" (>20% decrease), "→" (stable), "new" (no previous)
calculate_trend() {
  local current="$1"
  local previous="$2"

  if [ -z "$previous" ] || [ "$previous" = "null" ]; then
    echo "new"
    return
  fi

  # Handle zero previous (avoid division by zero)
  if [ "$previous" -eq 0 ] 2>/dev/null; then
    if [ "$current" -gt 0 ] 2>/dev/null; then
      echo "↑"
    else
      echo "→"
    fi
    return
  fi

  # Calculate percentage change
  local change
  change=$(awk "BEGIN {printf \"%.1f\", (($current - $previous) / $previous) * 100}")

  # Determine trend
  if awk "BEGIN {exit !($change > 20)}"; then
    echo "↑"
  elif awk "BEGIN {exit !($change < -20)}"; then
    echo "↓"
  else
    echo "→"
  fi
}

# Helper to calculate delta
calculate_delta() {
  local current="$1"
  local previous="$2"

  if [ -z "$previous" ] || [ "$previous" = "null" ]; then
    echo "null"
    return
  fi

  echo $((current - previous))
}

# Try to download previous artifacts
PREV_24H_FILE=$(mktemp)
PREV_7D_FILE=$(mktemp)
ARTIFACT_ZIP="/tmp/artifact_$$.zip"
# Single-quoted so the paths are read when the trap fires rather than baked in
# now (SC2064). No behavioural difference today — all three are set above and
# never reassigned — but a later edit that reassigns one would otherwise leave
# the original temp file behind.
# Insurance for a bare `shellcheck` run, not for this repo's gate: SC2317 is
# info-severity and `make test-scripts` runs `-S warning`, so it is filtered
# out before this directive is consulted. Kept so a contributor linting this
# file directly does not see a false positive on the trap body.
# shellcheck disable=SC2317
trap 'rm -f "$PREV_24H_FILE" "$PREV_7D_FILE" "$ARTIFACT_ZIP"' EXIT

HAS_24H=false
HAS_7D=false

if download_artifact "$ONE_DAY_AGO" "$PREV_24H_FILE"; then
  HAS_24H=true
fi

if download_artifact "$SEVEN_DAYS_AGO" "$PREV_7D_FILE"; then
  HAS_7D=true
fi

# Extract current metrics
CURRENT_TOTAL=$(echo "$CURRENT_JSON" | jq -r '.total_runs // 0')
CURRENT_FAILED=$(echo "$CURRENT_JSON" | jq -r '.failed_runs // 0')
CURRENT_FLAKY=$(echo "$CURRENT_JSON" | jq -r '.unique_flaky // 0')
CURRENT_API=$(echo "$CURRENT_JSON" | jq -r '.api_errors // 0')
CURRENT_RATE=$(echo "$CURRENT_JSON" | jq -r '.rate_limit // 0')
CURRENT_ASSERT=$(echo "$CURRENT_JSON" | jq -r '.assertion_failures // 0')
CURRENT_PANIC=$(echo "$CURRENT_JSON" | jq -r '.panic // 0')
CURRENT_UNKNOWN=$(echo "$CURRENT_JSON" | jq -r '.unknown // 0')

# Extract 24hr metrics (if available)
if [ "$HAS_24H" = true ]; then
  PREV_24H_TOTAL=$(jq -r '.total_runs // 0' "$PREV_24H_FILE")
  PREV_24H_FAILED=$(jq -r '.failed_runs // 0' "$PREV_24H_FILE")
  PREV_24H_FLAKY=$(jq -r '.unique_flaky // 0' "$PREV_24H_FILE")
  PREV_24H_API=$(jq -r '.api_errors // 0' "$PREV_24H_FILE")
  PREV_24H_RATE=$(jq -r '.rate_limit // 0' "$PREV_24H_FILE")
  PREV_24H_ASSERT=$(jq -r '.assertion_failures // 0' "$PREV_24H_FILE")
  PREV_24H_PANIC=$(jq -r '.panic // 0' "$PREV_24H_FILE")
  PREV_24H_UNKNOWN=$(jq -r '.unknown // 0' "$PREV_24H_FILE")
else
  PREV_24H_TOTAL="" PREV_24H_FAILED="" PREV_24H_FLAKY=""
  PREV_24H_API="" PREV_24H_RATE="" PREV_24H_ASSERT="" PREV_24H_PANIC="" PREV_24H_UNKNOWN=""
fi

# Extract 7-day metrics (if available)
if [ "$HAS_7D" = true ]; then
  PREV_7D_TOTAL=$(jq -r '.total_runs // 0' "$PREV_7D_FILE")
  PREV_7D_FAILED=$(jq -r '.failed_runs // 0' "$PREV_7D_FILE")
  PREV_7D_FLAKY=$(jq -r '.unique_flaky // 0' "$PREV_7D_FILE")
  PREV_7D_API=$(jq -r '.api_errors // 0' "$PREV_7D_FILE")
  PREV_7D_RATE=$(jq -r '.rate_limit // 0' "$PREV_7D_FILE")
  PREV_7D_ASSERT=$(jq -r '.assertion_failures // 0' "$PREV_7D_FILE")
  PREV_7D_PANIC=$(jq -r '.panic // 0' "$PREV_7D_FILE")
  PREV_7D_UNKNOWN=$(jq -r '.unknown // 0' "$PREV_7D_FILE")
else
  PREV_7D_TOTAL="" PREV_7D_FAILED="" PREV_7D_FLAKY=""
  PREV_7D_API="" PREV_7D_RATE="" PREV_7D_ASSERT="" PREV_7D_PANIC="" PREV_7D_UNKNOWN=""
fi

# Build trend output
cat <<EOF
{
  "current": {
    "total_runs": $CURRENT_TOTAL,
    "failed_runs": $CURRENT_FAILED,
    "unique_flaky": $CURRENT_FLAKY,
    "api_errors": $CURRENT_API,
    "rate_limit": $CURRENT_RATE,
    "assertion_failures": $CURRENT_ASSERT,
    "panic": $CURRENT_PANIC,
    "unknown": $CURRENT_UNKNOWN
  },
  "trends_24h": {
    "available": $HAS_24H,
    "total_runs": "$(calculate_trend "$CURRENT_TOTAL" "$PREV_24H_TOTAL")",
    "failed_runs": "$(calculate_trend "$CURRENT_FAILED" "$PREV_24H_FAILED")",
    "unique_flaky": "$(calculate_trend "$CURRENT_FLAKY" "$PREV_24H_FLAKY")",
    "api_errors": "$(calculate_trend "$CURRENT_API" "$PREV_24H_API")",
    "rate_limit": "$(calculate_trend "$CURRENT_RATE" "$PREV_24H_RATE")",
    "assertion_failures": "$(calculate_trend "$CURRENT_ASSERT" "$PREV_24H_ASSERT")",
    "panic": "$(calculate_trend "$CURRENT_PANIC" "$PREV_24H_PANIC")",
    "unknown": "$(calculate_trend "$CURRENT_UNKNOWN" "$PREV_24H_UNKNOWN")"
  },
  "deltas_24h": {
    "failed_runs": $(calculate_delta "$CURRENT_FAILED" "$PREV_24H_FAILED"),
    "unique_flaky": $(calculate_delta "$CURRENT_FLAKY" "$PREV_24H_FLAKY"),
    "api_errors": $(calculate_delta "$CURRENT_API" "$PREV_24H_API"),
    "rate_limit": $(calculate_delta "$CURRENT_RATE" "$PREV_24H_RATE"),
    "assertion_failures": $(calculate_delta "$CURRENT_ASSERT" "$PREV_24H_ASSERT"),
    "panic": $(calculate_delta "$CURRENT_PANIC" "$PREV_24H_PANIC"),
    "unknown": $(calculate_delta "$CURRENT_UNKNOWN" "$PREV_24H_UNKNOWN")
  },
  "trends_7d": {
    "available": $HAS_7D,
    "total_runs": "$(calculate_trend "$CURRENT_TOTAL" "$PREV_7D_TOTAL")",
    "failed_runs": "$(calculate_trend "$CURRENT_FAILED" "$PREV_7D_FAILED")",
    "unique_flaky": "$(calculate_trend "$CURRENT_FLAKY" "$PREV_7D_FLAKY")",
    "api_errors": "$(calculate_trend "$CURRENT_API" "$PREV_7D_API")",
    "rate_limit": "$(calculate_trend "$CURRENT_RATE" "$PREV_7D_RATE")",
    "assertion_failures": "$(calculate_trend "$CURRENT_ASSERT" "$PREV_7D_ASSERT")",
    "panic": "$(calculate_trend "$CURRENT_PANIC" "$PREV_7D_PANIC")",
    "unknown": "$(calculate_trend "$CURRENT_UNKNOWN" "$PREV_7D_UNKNOWN")"
  },
  "deltas_7d": {
    "failed_runs": $(calculate_delta "$CURRENT_FAILED" "$PREV_7D_FAILED"),
    "unique_flaky": $(calculate_delta "$CURRENT_FLAKY" "$PREV_7D_FLAKY"),
    "api_errors": $(calculate_delta "$CURRENT_API" "$PREV_7D_API"),
    "rate_limit": $(calculate_delta "$CURRENT_RATE" "$PREV_7D_RATE"),
    "assertion_failures": $(calculate_delta "$CURRENT_ASSERT" "$PREV_7D_ASSERT"),
    "panic": $(calculate_delta "$CURRENT_PANIC" "$PREV_7D_PANIC"),
    "unknown": $(calculate_delta "$CURRENT_UNKNOWN" "$PREV_7D_UNKNOWN")
  }
}
EOF
