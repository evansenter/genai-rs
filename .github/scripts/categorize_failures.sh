#!/usr/bin/env bash
# Categorizes the test failures of one GitHub Actions run by cause.
# Usage: categorize_failures.sh <run_id>
# Output: JSON to stdout with categorized failures
#
# Reads `gh run view <run_id> --log-failed`, whose lines carry the job
# name before a tab. A test counts as failed when its last try failed: one
# nextest retried to a pass in the same job (`FLAKY`, `TRY n PASS`) is a
# flake, which count_retried_flakes.sh counts, even though its failed tries
# printed `test ... FAILED` into the log. Output with no nextest status
# lines (`cargo test`) counts libtest's `test ... FAILED` lines.
#
# A test that failed every try, only in jobs named for one OS, and in
# every leg of that OS in the same matrix (`Cross Platform (windows-latest,
# stable)` and `(windows-latest, beta)`), is `deterministic`: a break on
# that OS, not a flake. The legs come from `gh run view --json jobs`, since
# the failed log holds only the jobs that failed: a test that fails on one
# leg while its twin passes it is a flake, and so is one whose legs cannot
# be listed. The rest are categorized by the error text around the test's
# name. `os` maps each deterministic test to its OS.

set -euo pipefail

RUN_ID="${1:?Usage: categorize_failures.sh <run_id>}"

LOGS=$(gh run view "$RUN_ID" --log-failed 2>/dev/null || echo "")
# Every job of the run by name, the green ones included.
JOBS=$(gh run view "$RUN_ID" --json jobs 2>/dev/null | jq -r '.jobs[].name' 2>/dev/null || true)

# `job<TAB>test` for every test whose last try failed, in each job.
FAILED=$(printf '%s\n' "$LOGS" | "${AWK:-awk}" -v esc=$'\033' '
  {
    line = $0
    sub(/\r$/, "", line)
    job = ""
    n = split(line, cols, "\t")
    if (n > 1) { job = cols[1]; line = cols[n] }
    sub(/^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9:.]+Z /, "", line)
    gsub(esc "\\[[0-9;]*m", "", line)
    gsub(/\^\[\[[0-9;]*m/, "", line)

    # libtest: `test name ... FAILED`, and its `---- name stdout ----`.
    if (match(line, /test [a-zA-Z0-9_:]+ \.\.\. FAILED/)) {
      name = substr(line, RSTART + 5, RLENGTH - 16)
      libtest[job SUBSEP name] = 1
      next
    }
    if (match(line, /---- [a-zA-Z0-9_:]+ stdout ----/)) {
      name = substr(line, RSTART + 5, RLENGTH - 17)
      libtest[job SUBSEP name] = 1
      next
    }

    # nextest: `STATUS [ 0.1s] (n/m) crate::binary test`, with retries
    # `TRY 2 FAIL [ ...` and `FLAKY 2/3 [ ...`. SLOW, START and RETRY
    # lines say nothing about the outcome.
    words = split(line, w, " ")
    if (words < 4) next
    timed = 0
    for (i = 2; i <= 4; i++) if (w[i] ~ /^\[/) timed = 1
    if (!timed) next
    status = (w[1] == "TRY") ? w[3] : w[1]
    key = job SUBSEP w[words]
    if (status == "PASS" || status == "FLAKY" || status == "LEAK") {
      passed[key] = 1
    } else if (status == "FAIL" || status == "TIMEOUT" || status == "TMT" \
               || status == "ABORT" || status ~ /^SIG/) {
      failed[key] = 1
    }
  }
  END {
    for (key in libtest) failed[key] = 1
    for (key in failed) {
      if (key in passed) continue
      split(key, k, SUBSEP)
      printf "%s\t%s\n", k[1], k[2]
    }
  }
' | sort -u)

declare -a API_ERRORS=()
declare -a RATE_LIMIT=()
declare -a ASSERTION_FAILURES=()
declare -a PANIC=()
declare -a DETERMINISTIC=()
declare -a UNKNOWN=()
OS_JSON='{}'

# The OS a job is named for, or nothing.
job_os() {
  local name
  name=$(tr '[:upper:]' '[:lower:]' <<<"$1")
  case "$name" in
    *windows*) echo windows ;;
    *macos*) echo macos ;;
    *ubuntu*) echo ubuntu ;;
  esac
}

# Whether <test> failed in every job of the run that is named for <os> and
# shares its matrix (the name before ` (`) with a job it failed in. False
# when the run's jobs could not be listed.
every_leg_failed() {
  local test_name="$1" os="$2" job name families="" legs=0
  while IFS=$'\t' read -r job name; do
    if [ "$name" = "$test_name" ]; then
      families+="${job%% (*}"$'\n'
    fi
  done <<<"$FAILED"
  while IFS= read -r job; do
    [ -n "$job" ] || continue
    [ "$(job_os "$job")" = "$os" ] || continue
    grep -qxF -- "${job%% (*}" <<<"$families" || continue
    legs=$((legs + 1))
    grep -qxF -- "$job"$'\t'"$test_name" <<<"$FAILED" || return 1
  done <<<"$JOBS"
  [ "$legs" -gt 0 ]
}

ALL_TESTS=$(cut -f2 <<<"$FAILED" | sort -u | grep -v '^$' || true)

while IFS= read -r test_name; do
  [ -z "$test_name" ] && continue

  # Every job the test failed in names the same OS, and it failed in every
  # leg of that OS.
  one_os=""
  while IFS=$'\t' read -r job name; do
    [ "$name" = "$test_name" ] || continue
    os=$(job_os "$job")
    if [ -z "$os" ] || { [ -n "$one_os" ] && [ "$os" != "$one_os" ]; }; then
      one_os="-"
      break
    fi
    one_os="$os"
  done <<<"$FAILED"
  if [ -n "$one_os" ] && [ "$one_os" != "-" ] && every_leg_failed "$test_name" "$one_os"; then
    DETERMINISTIC+=("$test_name")
    OS_JSON=$(jq -c --arg t "$test_name" --arg os "$one_os" '. + {($t): $os}' <<<"$OS_JSON")
    continue
  fi

  CONTEXT=$(grep -F -A 100 -B 20 -- "$test_name" <<<"$LOGS" 2>/dev/null || echo "")

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
    "deterministic": $(to_json_array "${DETERMINISTIC[@]+"${DETERMINISTIC[@]}"}"),
    "unknown": $(to_json_array "${UNKNOWN[@]+"${UNKNOWN[@]}"}")
  },
  "totals": {
    "api_errors": ${#API_ERRORS[@]},
    "rate_limit": ${#RATE_LIMIT[@]},
    "assertion_failures": ${#ASSERTION_FAILURES[@]},
    "panic": ${#PANIC[@]},
    "deterministic": ${#DETERMINISTIC[@]},
    "unknown": ${#UNKNOWN[@]}
  },
  "os": $OS_JSON
}
EOF
