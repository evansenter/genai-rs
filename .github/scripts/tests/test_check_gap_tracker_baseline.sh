#!/usr/bin/env bash
# Exercises `check_gap_tracker_baseline.sh` against fixture trackers (#457).
#
# The interesting part of the script is its `sed` capture, and the guard on
# an empty capture: without it, a reflowed tracker row would make the gate
# pass vacuously. So the fixtures cover agreement, disagreement, a reworded
# row, and a pre-release baseline (`2.19.0rc1`), which the capture's version
# class was widened for.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/check_gap_tracker_baseline.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# A tracker whose header row names `version`, shaped like the real one.
tracker() {
  cat <<MD
# Interactions API gap tracker

> | Field | Value |
> |-------|-------|
> | **Last swept against** | \`google-genai\` **$1** |
> | **Swept on** | 2026-11-01 |
MD
}

run_check() {
  local baseline="$1" tracker_file="$2"
  printf '%s\n' "$baseline" > "$WORK/baseline"
  rc=0
  out=$(BASELINE_FILE="$WORK/baseline" TRACKER="$tracker_file" bash "$SCRIPT" 2>&1) || rc=$?
}

expect() {
  local label="$1" want_rc="$2" needle="$3"
  if [ "$rc" -eq "$want_rc" ] && grep -qF -- "$needle" <<<"$out"; then
    echo "ok   - $label (exit $rc)"
  else
    echo "FAIL - $label: expected exit $want_rc and '$needle', got exit $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

tracker 2.34.0 > "$WORK/agree.md"
run_check 2.34.0 "$WORK/agree.md"
expect "agreeing versions pass" 0 "agree: google-genai 2.34.0"

run_check 2.35.0 "$WORK/agree.md"
expect "disagreeing versions fail, naming both" 1 "Gap tracker says 2.34.0"

# The row reworded: the capture finds nothing, and that must fail loudly
# rather than compare an empty string.
tracker 2.34.0 | sed 's/Last swept against/Last checked against/' > "$WORK/reworded.md"
run_check 2.34.0 "$WORK/reworded.md"
expect "a reworded row fails as moved or changed shape" 1 "moved or changed shape"

# A pre-release baseline, as a maintainer following the sweep can produce.
tracker 2.19.0rc1 > "$WORK/rc.md"
run_check 2.19.0rc1 "$WORK/rc.md"
expect "a pre-release version parses and agrees" 0 "agree: google-genai 2.19.0rc1"
run_check 2.19.0 "$WORK/rc.md"
expect "a pre-release header against the release fails as a mismatch" 1 "Gap tracker says 2.19.0rc1"

# Whitespace around the baseline is ignored; an empty baseline is an error.
run_check "  2.34.0  " "$WORK/agree.md"
expect "a padded baseline still agrees" 0 "agree: google-genai 2.34.0"
run_check "" "$WORK/agree.md"
expect "an empty baseline fails" 1 "is empty"

# A missing file is named rather than read as empty.
rc=0
out=$(BASELINE_FILE="$WORK/baseline" TRACKER="$WORK/missing.md" bash "$SCRIPT" 2>&1) || rc=$?
expect "a missing tracker is named" 1 "$WORK/missing.md not found"

# The real tree, from the repo root: the defaults still point at it.
rc=0
out=$(cd "$HERE/../../.." && bash "$SCRIPT" 2>&1) || rc=$?
expect "the defaults read the real tracker and baseline" 0 "Gap tracker and CI baseline agree"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
