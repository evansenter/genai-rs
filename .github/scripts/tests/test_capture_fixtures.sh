#!/usr/bin/env bash
# Exercises `capture_fixtures.py --check-age` (#492).
#
# The weekly `captured-fixtures` job re-captures the fixtures but cannot
# commit them, so it checks the committed copies' capture dates and goes red
# once they are older than its limit. Capturing needs the live API and is
# not covered here; the age check reads files only.

set -euo pipefail

command -v python3 >/dev/null 2>&1 || {
  echo "python3 is required to run this harness (brew install python3 / apt install python3)" >&2
  exit 1
}

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/capture_fixtures.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

check() {
  rc=0
  out=$(python3 "$SCRIPT" --check-age "$1" --today "$2" --out "$WORK/f" 2>&1) || rc=$?
}

expect() {
  local label="$1" want_rc="$2" needle="$3"
  if [ "$rc" -eq "$want_rc" ] && grep -qF -- "$needle" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected exit $want_rc and '$needle', got exit $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

fixture() { printf '{"captured": %s, "note": "", "request": {}, "response": {}}\n' "$2" > "$WORK/f/$1.json"; }

mkdir -p "$WORK/f"
fixture create_stream '"2026-11-05"'
fixture list_voices '"2026-10-01"'

check 90 2026-12-01
expect "within the limit: passes" 0 "All 2 fixtures were captured within 90 days."

# 2027-01-01 is 92 days after 2026-10-01 and 57 after 2026-11-05.
check 90 2027-01-01
expect "one past the limit: fails, naming it" 1 "list_voices.json: captured 2026-10-01, 92 days ago (limit 90)"
if grep -q "create_stream" <<<"$out"; then
  echo "FAIL - one past the limit: also named the fresh fixture"
  failures=$((failures + 1))
else
  echo "ok   - one past the limit: the fresh fixture is not named"
fi

fixture get_environment null
check 90 2026-12-01
expect "an undated fixture fails" 1 "get_environment.json: no capture date (captured: None)"

rm -f "$WORK/f"/*.json
check 90 2026-12-01
expect "no fixtures at all fails" 1 "no captured fixtures in"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
