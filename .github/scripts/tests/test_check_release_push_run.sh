#!/usr/bin/env bash
# Exercises `check_release_push_run.sh` against a stubbed `gh` (#504).
#
# release.yml publishes only a commit whose main push run of rust.yml is
# green on every job: the Cross Platform legs included, which release
# validation, on Ubuntu alone, never runs. What is pinned: a green run
# passes; a failed, cancelled or skipped leg, or a run with no Cross
# Platform legs, fails and is named; a run still going is waited for, and
# one that outlasts the wait fails; a commit with no push run fails.

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
    echo "jq is required to run this harness (brew install jq / apt install jq)" >&2
    exit 1
}

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/check_release_push_run.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
SHA=0123456789abcdef0123456789abcdef01234567

# `gh` answers from files in $STUB: `list-<n>` for the n-th `run list`
# (the last one once they run out, `[]` with none), `view-<n>` likewise for
# `run view --json databaseId,...`, and `jobs.json` for the jobs. Each call
# is logged.
STUB="$WORK/stub"
mkdir -p "$STUB/bin"
cat > "$STUB/bin/gh" <<STUB
#!/usr/bin/env bash
dir="$STUB"
echo "gh \$*" >> "\$dir/calls.log"
answer() { # kind
    local n
    n=\$(( \$(cat "\$dir/\$1.count" 2>/dev/null || echo 0) + 1 ))
    echo "\$n" > "\$dir/\$1.count"
    while [ "\$n" -gt 0 ] && [ ! -f "\$dir/\$1-\$n" ]; do n=\$((n - 1)); done
    if [ "\$n" -gt 0 ]; then cat "\$dir/\$1-\$n"; else echo '[]'; fi
}
case "\$1 \$2" in
  "run list") answer list ;;
  "run view")
    case "\$*" in
      *"--json jobs"*) cat "\$dir/jobs.json" ;;
      *) answer view ;;
    esac
    ;;
esac
STUB
chmod +x "$STUB/bin/gh"

# A fresh stub state: `run list` answers in order, then the jobs.
setup() { # jobs.json, then each `run list` answer
    rm -f "$STUB"/list-* "$STUB"/view-* "$STUB"/*.count "$STUB/calls.log"
    printf '%s\n' "$1" > "$STUB/jobs.json"
    shift
    local n=0
    for answer in "$@"; do
        n=$((n + 1))
        printf '%s\n' "$answer" > "$STUB/list-$n"
    done
}

run_check() {
    rc=0
    out=$(PATH="$STUB/bin:$PATH" POLL_SECONDS=0 RUN_APPEAR_SECONDS="${APPEAR:-0}" \
        RUN_FINISH_SECONDS="${FINISH:-0}" "$SCRIPT" "$SHA" 2>&1) || rc=$?
}
expect_rc() {
    local label="$1" want="$2"
    if [ "$rc" = "$want" ]; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: exited $rc, expected $want"
        printf '%s\n' "$out" | sed 's/^/         /'
        failures=$((failures + 1))
    fi
}
expect() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" <<<"$out"; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: expected to find '$needle'"
        printf '%s\n' "$out" | sed 's/^/         /'
        failures=$((failures + 1))
    fi
}

completed='[{"databaseId": 7, "status": "completed", "conclusion": "success", "url": "https://example.invalid/runs/7"}]'
# jobs <conclusion of the Windows stable leg> [extra job JSON]
jobs() {
    jq -nc --arg windows "$1" --argjson extra "${2:-[]}" '{jobs: ([
      {name: "Check", conclusion: "success"},
      {name: "Test Suite", conclusion: "success"},
      {name: "Cross Platform (ubuntu-latest, stable)", conclusion: "success"},
      {name: "Cross Platform (windows-latest, stable)", conclusion: $windows},
      {name: "Cross Platform (macos-latest, stable)", conclusion: "success"},
      {name: "Scheduled Run Status", conclusion: "skipped"}] + $extra)}'
}

# --- A green run passes; skipped jobs are fine.
setup "$(jobs success)" "$completed"
run_check
expect_rc "green: exits 0" 0
expect "green: says so" "Every job of the push run passed or was skipped"
if grep -q -- "--branch main --event push --commit $SHA" "$STUB/calls.log"; then
    echo "ok   - green: looks for main's push run of the commit"
else
    echo "FAIL - green: run list was not filtered to main's push run of the commit"
    sed 's/^/         /' "$STUB/calls.log"
    failures=$((failures + 1))
fi

# --- A red leg, a cancelled job, a skipped leg: each named, each fails.
setup "$(jobs failure)" "$completed"
run_check
expect_rc "red Windows leg: exits 1" 1
expect "red Windows leg: named" "::error::Cross Platform (windows-latest, stable): failure"
setup "$(jobs success '[{"name": "Integration Tests (core)", "conclusion": "cancelled"}]')" "$completed"
run_check
expect_rc "cancelled job: exits 1" 1
expect "cancelled job: named" "::error::Integration Tests (core): cancelled"
setup "$(jobs skipped)" "$completed"
run_check
expect_rc "skipped leg: exits 1" 1
expect "skipped leg: named" "::error::Cross Platform (windows-latest, stable): skipped"

# --- A run with no Cross Platform legs is not a main push run's.
setup '{"jobs": [{"name": "Check", "conclusion": "success"}]}' "$completed"
run_check
expect_rc "no legs: exits 1" 1
expect "no legs: said" "no Cross Platform legs"

# --- A run still going is waited for, then read.
setup "$(jobs success)" \
    '[{"databaseId": 7, "status": "in_progress", "conclusion": "", "url": "u"}]'
printf '%s\n' '{"databaseId": 7, "status": "in_progress", "conclusion": "", "url": "u"}' > "$STUB/view-1"
printf '%s\n' '{"databaseId": 7, "status": "completed", "conclusion": "success", "url": "u"}' > "$STUB/view-2"
FINISH=60 run_check
expect_rc "in progress: waited for, then green" 0
expect "in progress: the wait is shown" "is in_progress; looking again"
setup "$(jobs success)" \
    '[{"databaseId": 7, "status": "in_progress", "conclusion": "", "url": "u"}]'
printf '%s\n' '{"databaseId": 7, "status": "in_progress", "conclusion": "", "url": "u"}' > "$STUB/view-1"
FINISH=0 run_check
expect_rc "in progress past the wait: exits 1" 1
expect "in progress past the wait: said" "is still in_progress after 0s"

# --- No push run for the commit: looked for again, then fails.
setup "$(jobs success)" '[]' '[]' "$completed"
APPEAR=60 run_check
expect_rc "listed late: found on a later look" 0
setup "$(jobs success)" '[]'
APPEAR=0 run_check
expect_rc "never listed: exits 1" 1
expect "never listed: said" "No push run of rust.yml on main for $SHA"

# --- The newest run, when the commit has several.
setup "$(jobs success)" \
    '[{"databaseId": 5, "status": "completed", "conclusion": "failure", "url": "old"},
      {"databaseId": 9, "status": "completed", "conclusion": "success", "url": "new"}]'
run_check
expect "several runs: the newest is read" "Push run new for"

echo
if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed."
    exit 1
fi
echo "All checks passed."
