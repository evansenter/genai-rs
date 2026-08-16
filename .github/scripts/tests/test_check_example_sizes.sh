#!/usr/bin/env bash
# Exercises `check_example_sizes.sh` over fixture size maps.
#
# The script decides whether a PR goes red, and every one of its interesting
# behaviours is a comparison it either makes or silently declines to make.
# The PR description listed a manual matrix; a matrix nobody re-runs is a
# matrix that stops holding, and the failure mode is the worst kind — a gate
# reporting "all examples within threshold" while comparing nothing.
#
# Depends only on bash and jq, which the script under test already needs.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/check_example_sizes.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# Runs `compare` and captures output *and* exit status. The status is the
# product here — it is what turns a check red — so it is asserted, not
# discarded by a trailing `|| true`.
run_compare() {
  rc=0
  out=$(SIZE_GROWTH_OK="${SIZE_GROWTH_OK:-}" bash "$SCRIPT" compare "$@" 2>&1) || rc=$?
}

expect_rc() {
  local label="$1" want="$2"
  if [ "$rc" -eq "$want" ]; then
    echo "ok   - $label (exit $rc)"
  else
    echo "FAIL - $label: expected exit $want, got $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

expect_output() {
  local label="$1" needle="$2"
  if grep -q -- "$needle" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected to find '$needle'"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

refute_output() {
  local label="$1" needle="$2"
  if grep -q -- "$needle" <<<"$out"; then
    echo "FAIL - $label: found '$needle'"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  else
    echo "ok   - $label"
  fi
}

echo '{"alpha": 1000000, "beta": 2000000}' > "$WORK/base.json"

# --- Growth under the threshold passes.
echo '{"alpha": 1050000, "beta": 2000000}' > "$WORK/small.json"
run_compare "$WORK/base.json" "$WORK/small.json" 15
expect_rc     "under threshold: passes" 0
expect_output "under threshold: reports how many it compared" "2 compared"

# --- Growth over the threshold fails, and names the example.
echo '{"alpha": 1500000, "beta": 2000000}' > "$WORK/big.json"
run_compare "$WORK/base.json" "$WORK/big.json" 15
expect_rc     "over threshold: fails" 1
expect_output "over threshold: names the example" "::error::alpha grew"

# --- The boundary case the rounding bug produced: raw 15.04% compared while
#     the rounded 15% was printed, so the error read "grew +15% ... over the
#     +15% threshold" — a checker that looks broken rather than a regression.
echo '{"alpha": 1150400, "beta": 2000000}' > "$WORK/edge.json"
run_compare "$WORK/base.json" "$WORK/edge.json" 15
expect_rc      "boundary: +15.04% rounds to +15% and passes" 0
refute_output  "boundary: does not report +15% as over +15%" "grew +15% "

# --- Shrinking is never a failure.
echo '{"alpha": 500000, "beta": 2000000}' > "$WORK/small2.json"
run_compare "$WORK/base.json" "$WORK/small2.json" 15
expect_rc "shrinkage: passes" 0

# --- A new example has nothing to compare against; reported, never fatal.
echo '{"alpha": 1000000, "beta": 2000000, "gamma": 9000000}' > "$WORK/added.json"
run_compare "$WORK/base.json" "$WORK/added.json" 15
expect_rc     "new example: passes" 0
expect_output "new example: is reported" "New examples"

# --- No baseline: the check cannot run. It must say so as a warning, because
#     a gate that is not running must not look like a gate that ran clean.
: > "$WORK/empty.json"
run_compare "$WORK/empty.json" "$WORK/small.json" 15
expect_rc     "no baseline: does not fail the job" 0
expect_output "no baseline: warns rather than notices" "::warning::"
refute_output "no baseline: does not claim a pass" "within +15%"

# --- Disjoint key sets: the dangerous mode. Both files are non-empty and
#     nothing is comparable, which previously printed "All examples within
#     +15% of baseline" — a silently disabled gate reading as a passing one.
echo '{"delta-abc123": 1000000}' > "$WORK/disjoint.json"
run_compare "$WORK/base.json" "$WORK/disjoint.json" 15
expect_rc     "disjoint keys: fails rather than passing vacuously" 1
expect_output "disjoint keys: says the check did not run" "not running"
refute_output "disjoint keys: does not claim a pass" "All examples within"

# --- The label override the failure message promises.
run_compare "$WORK/base.json" "$WORK/big.json" 15
expect_rc "override off: still fails" 1
SIZE_GROWTH_OK=true run_compare "$WORK/base.json" "$WORK/big.json" 15
expect_rc     "override on: waves the growth through" 0
expect_output "override on: says why" "size-growth-ok"

# --- `measure` over an empty directory must not report success: a run that
#     measured nothing feeds an empty map into the comparison above.
mkdir -p "$WORK/nothing"
rc=0
out=$(bash "$SCRIPT" measure "$WORK/nothing" "$WORK/measured.json" 2>&1) || rc=$?
expect_rc     "measure: fails on an empty directory" 1
expect_output "measure: says nothing was found" "Measured 0"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
