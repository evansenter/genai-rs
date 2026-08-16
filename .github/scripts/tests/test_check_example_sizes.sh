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
# The override is threaded as `SIZE_GROWTH_OK=... run_compare ...`, and an
# assignment prefix on a *function* does not leak past the call in bash — I
# checked. But whether it does is not something a reader of this harness
# should have to resolve, and the cost of being wrong is a later case that
# expects exit 1 passing vacuously, in a file whose whole subject is gates
# that pass vacuously. So it is read from the environment here and reset
# explicitly at the one call site that sets it.
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

# --- The override has two branches now; this is the second, which turns off
#     the disjoint guard. A branch reachable only by adding a label to a PR
#     is one nobody exercises by accident, and this is the guard the header
#     calls out as protecting against "a gate reporting all examples within
#     threshold while comparing nothing".
SIZE_GROWTH_OK=true run_compare "$WORK/base.json" "$WORK/disjoint.json" 15
expect_rc     "disjoint + override: waves the guard through" 0
expect_output "disjoint + override: says why" "Nothing was comparable"
unset SIZE_GROWTH_OK

# --- The label override the failure message promises.
run_compare "$WORK/base.json" "$WORK/big.json" 15
expect_rc "override off: still fails" 1
SIZE_GROWTH_OK=true run_compare "$WORK/base.json" "$WORK/big.json" 15
expect_rc     "override on: waves the growth through" 0
expect_output "override on: says why" "size-growth-ok"
unset SIZE_GROWTH_OK

# --- The hash-suffix filter, which is the behaviour that matters most in
#     `measure`: without it the key set changes every rebuild, every entry
#     reads as new, and the delta check is silently disabled. The disjoint
#     guard above catches the symptom; this catches it at the edit.
mkdir -p "$WORK/bins"
: > "$WORK/bins/simple_interaction"
: > "$WORK/bins/simple_interaction-0123456789abcdef"
: > "$WORK/bins/simple_interaction.d"
chmod +x "$WORK/bins/simple_interaction" "$WORK/bins/simple_interaction-0123456789abcdef"
rc=0
out=$(bash "$SCRIPT" measure "$WORK/bins" "$WORK/bins.json" 2>&1) || rc=$?
expect_rc "measure: succeeds over a populated directory" 0
if [ "$(jq -r 'keys | join(",")' < "$WORK/bins.json")" = "simple_interaction" ]; then
  echo "ok   - measure: keeps the unsuffixed name and drops the hashed rebuild"
else
  echo "FAIL - measure: expected exactly {simple_interaction}, got $(jq -c 'keys' < "$WORK/bins.json")"
  failures=$((failures + 1))
fi

# --- A zero-byte baseline entry must not abort the filter. jq dies on a zero
#     divisor, and `measure` records 0 for any executable-but-empty file —
#     which is exactly what these fixtures create, so piping `measure` into
#     `compare` is one step away.
echo '{"alpha": 0, "beta": 2000000}' > "$WORK/zerobase.json"
run_compare "$WORK/zerobase.json" "$WORK/small.json" 15
expect_rc     "zero baseline: does not abort the filter" 0
expect_output "zero baseline: treated as uncomparable, like a new example" "1 compared"
# ...and it must actually be *reported*, not silently dropped from every
# list — which is what `select($base[.] == null)` did to it.
expect_output "zero baseline: the entry is named, not silently dropped" "New examples.*alpha"

# --- A removed example should be reported, not silently dropped.
echo '{"alpha": 1000000}' > "$WORK/removed.json"
run_compare "$WORK/base.json" "$WORK/removed.json" 15
expect_rc     "removed example: passes" 0
expect_output "removed example: is reported" "not in this build: beta"

# --- A missing directory is the case that fires if the workflow path to
#     target/release/examples ever drifts.
rc=0
out=$(bash "$SCRIPT" measure "$WORK/no-such-dir" "$WORK/x.json" 2>&1) || rc=$?
expect_rc     "measure: fails on a missing directory" 1
expect_output "measure: names the missing directory" "directory not found"

# --- Non-executable artifacts are excluded only by the exec-bit check; the
#     `.d` filter above it does not cover them.
mkdir -p "$WORK/mixed"
: > "$WORK/mixed/real_example"
chmod +x "$WORK/mixed/real_example"
echo "not a binary" > "$WORK/mixed/leftover.txt"
rc=0
out=$(bash "$SCRIPT" measure "$WORK/mixed" "$WORK/mixed.json" 2>&1) || rc=$?
expect_rc "measure: succeeds over a mixed directory" 0
if [ "$(jq -r 'keys | join(",")' < "$WORK/mixed.json")" = "real_example" ]; then
  echo "ok   - measure: skips non-executable artifacts"
else
  echo "FAIL - measure: expected {real_example}, got $(jq -c 'keys' < "$WORK/mixed.json")"
  failures=$((failures + 1))
fi

# --- `measure` over an empty directory must not report success: a run that
#     measured nothing feeds an empty map into the comparison above.
mkdir -p "$WORK/nothing"
rc=0
out=$(bash "$SCRIPT" measure "$WORK/nothing" "$WORK/measured.json" 2>&1) || rc=$?
expect_rc     "measure: fails on an empty directory" 1
expect_output "measure: says nothing was found" "Measured 0"

# --- The `measure` -> `compare` seam. Every case above stops short of it:
#     the compare cases use hand-written JSON, and the measure cases only
#     assert on `jq keys`. Nothing pinned that the JSON `measure` emits is a
#     shape `compare` can read — and `measure` builds it by hand with
#     `printf` rather than through `jq`, so it is the half most likely to
#     drift. The round-2 `stat -c%s` -> `wc -c` swap changed the emitted
#     bytes (BSD `wc` pads its count with leading spaces) and nothing here
#     would have noticed.
mkdir -p "$WORK/seam-a" "$WORK/seam-b"
# 1000 -> 1050 bytes, i.e. +5%, comfortably under the threshold: the point
# here is that the two halves agree on a format, not that the delta trips.
head -c 1000 /dev/zero > "$WORK/seam-a/demo"
head -c 1050 /dev/zero > "$WORK/seam-b/demo"
chmod +x "$WORK/seam-a/demo" "$WORK/seam-b/demo"
bash "$SCRIPT" measure "$WORK/seam-a" "$WORK/seam-a.json" >/dev/null
bash "$SCRIPT" measure "$WORK/seam-b" "$WORK/seam-b.json" >/dev/null
run_compare "$WORK/seam-a.json" "$WORK/seam-b.json" 15
expect_rc     "seam: compare reads what measure wrote" 0
expect_output "seam: the entry is compared, not skipped" "1 compared"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
