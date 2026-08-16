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

# Preflight, because without jq the failures are not merely unhelpful, they
# are wrong: `measure` dies at `jq length` with 127 and reports as
# "expected exit 0, got 127", while `! jq ...` *succeeds* when jq is missing,
# so every `compare` case falls into the no-usable-baseline branch and the
# rc-1 cases report a confident wrong verdict. About a dozen red assertions,
# none of them naming jq. The harness must not be able to do the thing it
# exists to catch the script doing.
command -v jq >/dev/null 2>&1 || {
    echo "jq is required to run this harness (brew install jq / apt install jq)" >&2
    exit 1
}

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
    printf '%s\n' "${out//$'\n'/$'\n'         }"
    failures=$((failures + 1))
  fi
}

expect_output() {
  local label="$1" needle="$2"
  if grep -q -- "$needle" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected to find '$needle'"
    printf '%s\n' "${out//$'\n'/$'\n'         }"
    failures=$((failures + 1))
  fi
}

refute_output() {
  local label="$1" needle="$2"
  if grep -q -- "$needle" <<<"$out"; then
    echo "FAIL - $label: found '$needle'"
    printf '%s\n' "${out//$'\n'/$'\n'         }"
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
expect_output "new example: is reported" "No usable baseline.*gamma"

# --- No baseline: the check cannot run. It must say so as a warning, because
#     a gate that is not running must not look like a gate that ran clean.
: > "$WORK/empty.json"
run_compare "$WORK/empty.json" "$WORK/small.json" 15
expect_rc     "no baseline: does not fail the job" 0
expect_output "no baseline: warns rather than notices" "::warning::"
refute_output "no baseline: does not claim a pass" "within +15%"

# --- A truncated download is non-empty and non-JSON. It must land in the
#     did-not-run branch, not abort jq and exit non-zero on a parse error —
#     which reads as a size regression.
printf 'not json' > "$WORK/corrupt.json"
run_compare "$WORK/corrupt.json" "$WORK/small.json" 15
expect_rc     "corrupt baseline: does not fail the job" 0
expect_output "corrupt baseline: warns rather than aborting" "::warning::"
refute_output "corrupt baseline: does not claim a pass" "within +15%"

# --- Well-formed JSON the filters cannot use. An array, a string or null is
#     rejected by `$base[$k]`; an object of *string* sizes gets further —
#     jq orders numbers before strings, so it survives `> 0` — and then aborts
#     at `(.value - $was)`. Both are the same abort, the same raw-jq-error
#     exit and the same misreading as a truncated file, so both belong in the
#     same branch. Not reachable while `measure` writes every baseline through
#     `tonumber`; insurance against a format change or a foreign artifact.
for shape in '[]' '"a string"' 'null' '42' '{"alpha": "1000000"}'; do
    printf '%s' "$shape" > "$WORK/nonobject.json"
    run_compare "$WORK/nonobject.json" "$WORK/small.json" 15
    expect_rc     "unusable baseline ($shape): does not fail the job" 0
    expect_output "unusable baseline ($shape): warns rather than aborting" "::warning::"
    refute_output "unusable baseline ($shape): does not claim a pass" "within +15%"
done

# --- Names that a hand-built JSON emitter gets wrong. This is the property
#     the `jq -Rn` fold in `measure` was adopted for: `printf '"%s": %s'` on a
#     name containing a quote or a backslash emits a malformed map, and
#     `compare` then reports it as "the build under test is unmeasurable" — a
#     confusing diagnosis for what is really a naming problem.
rm -rf "$WORK/escbins" && mkdir -p "$WORK/escbins"
esc_names=('has"quote' 'has\backslash' $'tab\tname' 'ünïcødé' 'plain')
for n in "${esc_names[@]}"; do
  : > "$WORK/escbins/$n"
  chmod +x "$WORK/escbins/$n"
done
if bash "$SCRIPT" measure "$WORK/escbins" "$WORK/escbins.json" >/dev/null 2>&1; then
  echo "ok   - measure: survives names needing JSON escaping (exit 0)"
else
  echo "FAIL - measure: exited non-zero on names needing JSON escaping"
  failures=$((failures + 1))
fi
for n in "${esc_names[@]}"; do
  if [ "$(jq --arg k "$n" 'has($k)' < "$WORK/escbins.json" 2>/dev/null)" = "true" ]; then
    echo "ok   - measure: [$n] survives escaping intact"
  else
    echo "FAIL - measure: [$n] missing from the map: $(jq -c 'keys' < "$WORK/escbins.json" 2>&1)"
    failures=$((failures + 1))
  fi
done

# --- A non-numeric threshold. The quoted form is the dangerous one: jq binds
#     a string, orders every number before every string, and so reports every
#     example as within threshold — a green gate that compared nothing.
#
#     Deliberately compared against the *under*-threshold fixture: with a
#     valid threshold this pair exits 0, so a non-zero exit can only be the
#     guard firing. Using the over-threshold pair would pass for the wrong
#     reason, since it fails on size regardless of what the threshold says.
#
#     An empty third argument is not in the list: `${3:-15}` defaults it, so
#     it is a legitimate call, not a malformed one.
for bad in '"15"' '15%' '-5' 'abc' '1.5'; do
    run_compare "$WORK/base.json" "$WORK/small.json" "$bad"
    expect_rc     "bad threshold ([$bad]): rejected rather than silently applied" 1
    expect_output "bad threshold ([$bad]): says what it wanted" "non-negative integer"
    refute_output "bad threshold ([$bad]): does not claim a pass" "All examples within"
done

# --- And a valid threshold on the same pair still passes, so the guard above
#     is rejecting the argument rather than the comparison.
run_compare "$WORK/base.json" "$WORK/small.json" 15
expect_rc "valid threshold: still passes on the same pair" 0

# --- A missing, malformed or unusable *current* file is a hard error, not
#     the warn-and-skip a bad baseline gets: the build under test is
#     unmeasurable, and skipping that is the vacuous pass the disjoint guard
#     rejects. Same value-type case as the baseline loop above.
for shape in '[]' 'not json' '' '{"alpha": "1000000"}'; do
    printf '%s' "$shape" > "$WORK/badcurrent.json"
    run_compare "$WORK/base.json" "$WORK/badcurrent.json" 15
    expect_rc     "unusable current ([$shape]): fails rather than skipping" 1
    expect_output "unusable current ([$shape]): says the build is unmeasurable" "unmeasurable"
    refute_output "unusable current ([$shape]): does not claim a pass" "All examples within"
done

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

# --- The hash filter must require the *whole* tail after the final hyphen to
#     be hex, not merely to start with 8 hex characters. A false positive here
#     drops the example from both sides, so it appears in neither the table nor
#     the added/removed notices — the compared count is its only trace.
rm -rf "$WORK/hashbins" && mkdir -p "$WORK/hashbins"
for n in plain_name real-deadbeef_suffix short-abc keep-0123456g \
         drop-0123456789abcdef drop-0123456789; do
  : > "$WORK/hashbins/$n"
  chmod +x "$WORK/hashbins/$n"
done
bash "$SCRIPT" measure "$WORK/hashbins" "$WORK/hashbins.json" >/dev/null 2>&1
kept=$(jq -r 'keys | sort | join(",")' < "$WORK/hashbins.json")
expected="keep-0123456g,plain_name,real-deadbeef_suffix,short-abc"
if [ "$kept" = "$expected" ]; then
  echo "ok   - measure: drops only all-hex tails of 8+ characters"
else
  echo "FAIL - measure: hash filter kept [$kept], expected [$expected]"
  failures=$((failures + 1))
fi

# --- The success line must carry both denominators. A *partial* key-set
#     divergence is fatal to coverage but invisible in a bare compared count:
#     rename 19 of 20 and "1 compared" still reads like a clean run.
echo '{"alpha": 1000000, "beta": 2000000, "gamma": 3000000}' > "$WORK/three.json"
echo '{"alpha": 1000000}' > "$WORK/one.json"
run_compare "$WORK/three.json" "$WORK/one.json" 15
expect_rc     "partial divergence: still passes (nothing regressed)" 0
expect_output "partial divergence: reports how much was compared" "1 compared of 1 built"
expect_output "partial divergence: reports the baseline size too" "3 in baseline"

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
expect_output "zero baseline: the entry is named, not silently dropped" "No usable baseline.*alpha"

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
#     shape `compare` can read. Two changes have moved those bytes since —
#     `stat -c%s` -> `wc -c`, where BSD `wc` pads its count with leading
#     spaces, and the switch from a hand-built `printf` map to the `jq -Rn`
#     fold — and neither would have been caught by the cases above.
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

# --- The summary table. It was inline jq in the workflow, where no fixture
#     could reach it — the last size logic in the job without a test, which
#     undercut the point of consolidating the definitions.
echo '{"big": 14050000, "round": 13631488, "tiny": 40000}' > "$WORK/render.json"
rc=0
out=$(bash "$SCRIPT" render "$WORK/render.json" 2>&1) || rc=$?
expect_rc     "render: exits 0" 0
# Order, not just presence: asserting the row exists passes whichever way
# `sort_by` points, which is how the first version of this check went inert.
if [ "$(grep -o '^| \(big\|round\|tiny\) ' <<<"$out" | tr -d '| ' | paste -sd,)" = "big,round,tiny" ]; then
  echo "ok   - render: largest first"
else
  echo "FAIL - render: expected big,round,tiny; got $(grep -o '^| \(big\|round\|tiny\) ' <<<"$out" | tr -d '| ' | paste -sd,)"
  failures=$((failures + 1))
fi
# One decimal always: 13.0 must not render as `13` beside a `13.4` neighbour.
expect_output "render: keeps a trailing zero" "| round | 13.0 MB |"
# ...and a small binary reads as small rather than as `0 MB`.
expect_output "render: floors instead of rounding to zero" "| tiny | <0.1 MB |"
if [ "$(grep -c '^| ' <<<"$out")" -eq 4 ]; then
  echo "ok   - render: one header row and one row per example"
else
  echo "FAIL - render: expected 4 table rows, got $(grep -c '^| ' <<<"$out")"
  failures=$((failures + 1))
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
