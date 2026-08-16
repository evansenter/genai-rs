#!/usr/bin/env bash
# Exercises the scheduled-failure escalation scripts against a stubbed `gh`.
#
# `bash -n` cannot catch what these scripts get wrong. The bug review found
# in the first draft — `export TITLE` placed *after* the `gh issue list` whose
# `--jq` reads `$ENV.TITLE` — is valid syntax that means the wrong thing: the
# filter resolved to null, the lookup always came back empty, every failing
# run would have filed a duplicate, and the entire update path was dead code.
#
# So the branches are asserted here, in the tree, rather than in a one-off
# local run that the next edit does not inherit.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS="$(dirname "$HERE")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# Writes a `gh` stub that answers `issue list` from $1 and logs every other
# call to stderr as `GH-CALL: ...`.
make_gh_stub() {
  cat > "$WORK/gh" <<STUB
#!/usr/bin/env bash
if [ "\$1 \$2" = "issue list" ]; then
  prev=""
  filter=""
  for a in "\$@"; do
    [ "\$prev" = "--jq" ] && filter="\$a"
    prev="\$a"
  done
  echo '$1' | jq "\$filter"
  exit 0
fi
echo "GH-CALL: \$*" >&2
STUB
  chmod +x "$WORK/gh"
}

check() {
  local label="$1" expected="$2" actual="$3"
  if grep -q -- "$expected" <<<"$actual"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label"
    echo "       expected to find: $expected"
    echo "       in:"
    sed 's/^/         /' <<<"$actual"
    failures=$((failures + 1))
  fi
}

TITLE_FLAKY="Scheduled workflow failing: CI Flakiness Report"

# --- Reporting: an existing issue must be updated and commented on, not
#     duplicated. This is the assertion the $ENV.TITLE bug would fail.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77},{\"title\":\"unrelated\",\"number\":2}]"
out=$(PATH="$WORK:$PATH" bash "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1" 2>&1 || true)
check "reporting: finds the existing issue" "Updated issue #77" "$out"
check "reporting: edits its body"           "issue edit 77"     "$out"
check "reporting: comments on repeat"       "Still failing"     "$out"
# The assertion the $ENV.TITLE bug would have failed outright: with the
# lookup returning empty, this path filed a *new* issue every time.
if grep -q "issue create" <<<"$out"; then
  echo "FAIL - reporting: filed a duplicate despite an existing issue"
  failures=$((failures + 1))
else
  echo "ok   - reporting: files no duplicate when one already exists"
fi

# --- Reporting: nothing matching means a fresh issue.
make_gh_stub '[{"title":"unrelated","number":2}]'
out=$(PATH="$WORK:$PATH" bash "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1" 2>&1 || true)
check "reporting: files a new issue when none matches" "issue create" "$out"
check "reporting: labels it for the scoped lookup" "ci-health-escalation" "$out"

# --- Resolving: an open issue is commented on and closed.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]"
out=$(PATH="$WORK:$PATH" bash "$SCRIPTS/resolve_scheduled_failure.sh" \
  "CI Flakiness Report" 2>&1 || true)
check "resolving: closes the issue"  "issue close 77" "$out"
check "resolving: says why"          "Recovered"      "$out"

# --- Resolving: nothing open is a clean no-op, not an error.
make_gh_stub '[]'
out=$(PATH="$WORK:$PATH" bash "$SCRIPTS/resolve_scheduled_failure.sh" \
  "CI Flakiness Report" 2>&1 || true)
check "resolving: no-op when nothing is open" "No open failure issue" "$out"

# --- The lookups must agree: a title the reporter files under is a title the
#     resolver finds. Drift here lands updates on one issue and closes on
#     another.
report_title=$(grep -o 'Scheduled workflow failing: \$WORKFLOW' "$SCRIPTS/report_scheduled_failure.sh" | head -1)
resolve_title=$(grep -o 'Scheduled workflow failing: \$WORKFLOW' "$SCRIPTS/resolve_scheduled_failure.sh" | head -1)
if [ -n "$report_title" ] && [ "$report_title" = "$resolve_title" ]; then
  echo "ok   - both scripts build the same issue title"
else
  echo "FAIL - the two scripts' issue titles have drifted"
  failures=$((failures + 1))
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
