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

# Writes a `gh` stub that answers `issue list` from $1, `issue view` from $2
# (defaulting to an issue with no comments), and logs every other call to
# stderr as `GH-CALL: ...`. `issue edit --body-file` is echoed with its
# contents so the body can be asserted on.
make_gh_stub() {
  local list_json="$1" view_json="${2:-{\"createdAt\":\"2026-05-01T00:00:00Z\",\"comments\":[]\}}"
  cat > "$WORK/gh" <<STUB
#!/usr/bin/env bash
if [ "\$1 \$2" = "issue list" ]; then
  prev=""
  filter=""
  for a in "\$@"; do
    [ "\$prev" = "--jq" ] && filter="\$a"
    prev="\$a"
  done
  echo '$list_json' | jq "\$filter"
  exit 0
fi
if [ "\$1 \$2" = "issue view" ]; then
  # Honour --jq here too: the script reads \`--json body --jq .body\`, and a
  # stub that ignored the filter would hand back raw JSON, making the
  # annotation assertions fail for a reason that has nothing to do with the
  # script.
  prev=""
  filter=""
  for a in "\$@"; do
    [ "\$prev" = "--jq" ] && filter="\$a"
    prev="\$a"
  done
  if [ -n "\$filter" ]; then
    echo '$view_json' | jq -r "\$filter"
  else
    echo '$view_json'
  fi
  exit 0
fi
echo "GH-CALL: \$*" >&2
prev=""
for a in "\$@"; do
  if [ "\$prev" = "--body-file" ]; then
    echo "GH-BODY: \$(cat "\$a")" >&2
  fi
  prev="\$a"
done
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

# Runs a script under the stubbed `gh`, keeping its output *and* its status.
#
# The tolerance has to live off the command substitution. `out=$(... || true)`
# discards the exit status, so a script that died before reaching its
# create-or-update branch produces output containing no "issue create" — and
# the negative assertions below, the two written specifically to catch
# duplicate filing, would report ok for the wrong reason. Keeping the
# tolerance is still right: a failing script should print its output through
# `check` rather than killing the harness under `set -e`.
run_script() {
  local label="$1"
  shift
  local rc=0
  out=$(PATH="$WORK:$PATH" bash "$@" 2>&1) || rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "FAIL - $label: the script exited $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

TITLE_FLAKY="Scheduled workflow failing: CI Flakiness Report"

# --- Reporting: an existing issue must be updated and commented on, not
#     duplicated. This is the assertion the $ENV.TITLE bug would fail.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77},{\"title\":\"unrelated\",\"number\":2}]"
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
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
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "reporting: files a new issue when none matches" "issue create" "$out"
# Anchored to the create call: the script also runs `gh label create
# ci-health-escalation` unconditionally, whose log line contains the same
# string — so an unanchored grep passed even with the label dropped from
# `issue create`. An issue filed without it is invisible to the resolver,
# which then no-ops forever.
check "reporting: labels it for the scoped lookup" "issue create.*ci-health-escalation" "$out"

# --- Resolving: an open issue is commented on and closed.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]"
run_script "resolve_scheduled_failure.sh" "$SCRIPTS/resolve_scheduled_failure.sh" \
  "CI Flakiness Report"
check "resolving: closes the issue"  "issue close 77" "$out"
check "resolving: says why"          "Recovered"      "$out"

# --- Resolving: nothing open is a clean no-op, not an error.
make_gh_stub '[]'
run_script "resolve_scheduled_failure.sh" "$SCRIPTS/resolve_scheduled_failure.sh" \
  "CI Flakiness Report"
check "resolving: no-op when nothing is open" "No open failure issue" "$out"

# --- Reporting: the body carries streak length, which is the thing #431 is
#     about. "It failed again" is what the email channel already gave.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" \
  '{"createdAt":"2026-05-01T00:00:00Z","comments":[
      {"body":"Still failing: x","createdAt":"2026-05-02T00:00:00Z"},
      {"body":"Still failing: x","createdAt":"2026-05-03T00:00:00Z"}]}'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
# Two prior "Still failing" comments + the run that opened it + this run.
check "streak: counts the run that opened the issue" \
  "Failing since 2026-05-01 — 4 consecutive scheduled runs" "$out"

# --- Streak: a recovery resets it. Comments before the "Recovered" belong to
#     a closed-out episode, and folding them in would report a streak that
#     never happened.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" \
  '{"createdAt":"2026-05-01T00:00:00Z","comments":[
      {"body":"Still failing: x","createdAt":"2026-05-02T00:00:00Z"},
      {"body":"Recovered — the workflow succeeded. Closing.","createdAt":"2026-05-03T00:00:00Z"},
      {"body":"Still failing: x","createdAt":"2026-06-10T00:00:00Z"}]}'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "streak: a recovery resets the count" \
  "Failing since 2026-06-10 — 2 consecutive scheduled runs" "$out"

# --- Streak: the first failure of a new episode, where the comment list
#     *ends* at the Recovered. Both fixtures above leave a failure comment
#     after the reset, so `$sf` is non-empty in each and the empty-timestamp
#     branch — the one that falls back to today's date — went unexercised.
#     That is the hole an IFS-collapse bug went through, rendering
#     "Failing since false" while the harness stayed green.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" \
  '{"createdAt":"2026-05-01T00:00:00Z","comments":[
      {"body":"Still failing: x","createdAt":"2026-05-02T00:00:00Z"},
      {"body":"Recovered — the workflow succeeded. Closing.","createdAt":"2026-05-03T00:00:00Z"}]}'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "streak: a re-failure right after recovery dates itself today" \
  "Failing since $(date -u +%Y-%m-%d) — 1 scheduled run" "$out"

# --- ...and the cap must also fire when the returned page happens to
#     contain a Recovered. Deriving it from the post-reset tail made it
#     unreachable there — that expression tops out at total-1, i.e. 99 on
#     a full page — so the branch printed a confident streak over a
#     history it could not see.
python3 - > "$WORK/capped_recovery.json" <<'FIXTURE'
import json
comments = [{"body": "Recovered — the workflow succeeded. Closing.",
             "createdAt": "2026-05-02T00:00:00Z"}]
comments += [{"body": "Still failing: x", "createdAt": "2026-05-03T00:00:00Z"}] * 99
print(json.dumps({"createdAt": "2026-05-01T00:00:00Z", "comments": comments}))
FIXTURE
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" "$(cat "$WORK/capped_recovery.json")"
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "streak: caps a full page that contains a recovery" "Still failing.**" "$out"

# --- Annotations survive a body that has been through the web UI, which
#     normalises to CRLF. That is the sequence the marker exists for: a human
#     opens the issue and types a note. An exact-equality match against an
#     LF marker would drop it, and a discarded annotation is indistinguishable
#     from one nobody wrote.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" \
  '{"createdAt":"2026-05-01T00:00:00Z","comments":[],
    "body":"old text\r\n<!-- Generated above; anything you add below this line is preserved. -->\r\nTyped in the browser, tracked in #500.\r\n"}'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "annotations: survive a CRLF body from the web UI" "tracked in #500" "$out"

# --- Streak: past the API's one-page comment limit the count is computed
#     over a window, not the history, so the claim is capped rather than
#     stated as fact. A confident wrong number is the failure this line was
#     added to avoid.
python3 - > "$WORK/capped.json" <<'PY'
import json
print(json.dumps({
    "createdAt": "2026-05-01T00:00:00Z",
    "comments": [{"body": "Still failing: x", "createdAt": "2026-05-02T00:00:00Z"}] * 100,
}))
PY
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" "$(cat "$WORK/capped.json")"
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "streak: caps the claim at the page boundary" "Still failing.**" "$out"
# No number at all past the boundary, not even a floor: the missing page is
# the *newest* comments, so a recovery outside the window would make "at
# least N" overstate a streak that in fact restarted.
if grep -qE "Failing since|at least [0-9]" <<<"$out"; then
  echo "FAIL - streak: stated a figure it cannot support past the page boundary"
  failures=$((failures + 1))
else
  echo "ok   - streak: states no figure past the page boundary"
fi

# --- The body edit overwrites, so anything a maintainer added must be
#     carried across. Losing it silently is worse than not offering the
#     affordance at all.
make_gh_stub "[{\"title\":\"$TITLE_FLAKY\",\"number\":77}]" \
  '{"createdAt":"2026-05-01T00:00:00Z","comments":[],
    "body":"old generated text\n<!-- Generated above; anything you add below this line is preserved. -->\nRoot cause is the retention change, tracked in #500."}'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "annotations: carried across the body rewrite" "tracked in #500" "$out"
if grep -q "old generated text" <<<"$out"; then
  echo "FAIL - annotations: kept stale generated text from above the marker"
  failures=$((failures + 1))
else
  echo "ok   - annotations: text above the marker is regenerated, not preserved"
fi

# --- The marker must exist on a fresh issue too, or the first annotator has
#     nowhere to write that survives.
make_gh_stub '[]'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "CI Flakiness Report" "https://example/run/1"
check "annotations: a new issue ships the marker" "anything you add below this line is preserved" "$out"

# --- The third argument replaces the default body paragraph. `audit.yml`
#     relies on this to avoid pointing a reader at CI health.
make_gh_stub '[]'
run_script "report_scheduled_failure.sh" "$SCRIPTS/report_scheduled_failure.sh" \
  "Security Audit" "https://example/run/1" "CUSTOM CONTEXT PARAGRAPH"
check "context: the override reaches the body" "CUSTOM CONTEXT PARAGRAPH" "$out"
if grep -q "Scheduled workflows fail quietly" <<<"$out"; then
  echo "FAIL - context: the default paragraph survived the override"
  failures=$((failures + 1))
else
  echo "ok   - context: the override replaces the default rather than adding to it"
fi

# --- The lookups must agree. Drift lands updates on one issue and closes on
#     another.
#
#     `|| true` on every grep is not decoration: under `set -euo pipefail` a
#     non-matching `grep` exits 1, `pipefail` carries that past the
#     successful `head`, and the assignment takes `set -e` — so the script
#     would die here and the FAIL branch below could never print. The one
#     scenario these checks exist for is the one where they would have gone
#     silent about themselves.
report_title=$(grep -o 'Scheduled workflow failing: \$WORKFLOW' "$SCRIPTS/report_scheduled_failure.sh" | head -1 || true)
resolve_title=$(grep -o 'Scheduled workflow failing: \$WORKFLOW' "$SCRIPTS/resolve_scheduled_failure.sh" | head -1 || true)
if [ -n "$report_title" ] && [ "$report_title" = "$resolve_title" ]; then
  echo "ok   - both scripts build the same issue title"
else
  echo "FAIL - the two scripts' issue titles have drifted"
  failures=$((failures + 1))
fi

# The label is the half that actually differs when it drifts: both title
# greps echo the same literal back, so that check only proves "the string is
# present in both files". A typo in one ESCALATION_LABEL passes every
# stubbed assertion above — the stub answers `issue list` from its fixture
# regardless of `--label` — and in production the resolver would search a
# label nothing is filed under, find nothing, print its clean no-op, and
# leave the issue open forever. That is the always-on-signal failure
# `resolve_scheduled_failure.sh` exists to prevent.
# `sed`, not `grep -oP`: PCRE mode is a GNU extension and this harness should
# not silently become Linux-only over a capture group.
report_label=$(sed -n 's/^ESCALATION_LABEL="\([^"]*\)".*/\1/p' "$SCRIPTS/report_scheduled_failure.sh" | head -1 || true)
resolve_label=$(sed -n 's/^ESCALATION_LABEL="\([^"]*\)".*/\1/p' "$SCRIPTS/resolve_scheduled_failure.sh" | head -1 || true)
if [ -n "$report_label" ] && [ "$report_label" = "$resolve_label" ]; then
  echo "ok   - both scripts use the same escalation label ($report_label)"
else
  echo "FAIL - escalation labels have drifted: report='$report_label' resolve='$resolve_label'"
  failures=$((failures + 1))
fi

# The recovery prefix is the same shape of coupling, one layer less obvious:
# the resolver *writes* the comment and the reporter *matches* on it to reset
# the streak. Reword the resolver and the reset silently stops firing — the
# streak then spans a recovery that did happen, reporting a number that is
# simply wrong. Every stubbed assertion above still passes, which is exactly
# why this is checked here rather than left to a fixture.
#
# Reachable, not theoretical: the reporter's lookup uses `--state all`, so a
# recovered-then-refailed workflow takes the reset path every time.
report_recovery=$(sed -n 's/^RECOVERY_PREFIX="\([^"]*\)".*/\1/p' "$SCRIPTS/report_scheduled_failure.sh" | head -1 || true)
resolve_recovery=$(sed -n 's/^RECOVERY_PREFIX="\([^"]*\)".*/\1/p' "$SCRIPTS/resolve_scheduled_failure.sh" | head -1 || true)
if [ -n "$report_recovery" ] && [ "$report_recovery" = "$resolve_recovery" ]; then
  echo "ok   - both scripts agree on the recovery prefix ($report_recovery)"
else
  echo "FAIL - recovery prefixes have drifted: report='$report_recovery' resolve='$resolve_recovery'"
  failures=$((failures + 1))
fi

# ...and the resolver's comment must actually start with the prefix it
# declares, or the constant agrees with itself while the wire format does not.
resolve_body=$(sed -n 's/^gh issue comment "\$EXISTING" --body "\(.*\)/\1/p' "$SCRIPTS/resolve_scheduled_failure.sh" | head -1 || true)
case "$resolve_body" in
  "\$RECOVERY_PREFIX"*)
    echo "ok   - the resolver's comment is built from the declared prefix" ;;
  *)
    echo "FAIL - the resolver's comment does not begin with \$RECOVERY_PREFIX: '$resolve_body'"
    failures=$((failures + 1)) ;;
esac

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
