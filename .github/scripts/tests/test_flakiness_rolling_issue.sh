#!/usr/bin/env bash
# Exercises the rolling-issue step of `ci-flakiness-report.yml` against a
# stubbed `gh`.
#
# This block has now broken silently twice. Round 4 diffed the newly-flaky set
# against the body's top-10 table, so every test ranked 11+ read as new on
# every run. Round 5 applied a "comment before edit" reorder and dropped the
# `gh issue edit` altogether — the step still echoed "Updated issue: ..." and
# the job still went green while the issue was never touched.
#
# Neither is visible to `bash -n`, and neither is visible to review that reads
# the diff rather than the resulting whole. So the step is run here, with the
# calls it makes asserted.
#
# The block is extracted from the workflow rather than duplicated: a copy is
# what goes stale, and a stale copy would keep passing while the workflow it
# claims to cover breaks.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/ci-flakiness-report.yml"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0

# Pull the step's `run:` body out and dedent it. Deliberately not via a YAML
# parser: this must run on a bare runner, and PyYAML is not guaranteed there.
extract_step() {
  awk -v want="$1" '
    $0 ~ ("^      - name: " want "$") { in_step = 1; next }
    in_step && /^        run: \|/ { in_run = 1; next }
    in_run {
      # A line at step indentation or shallower ends the block; blank lines
      # inside it do not.
      if ($0 !~ /^          / && $0 !~ /^[[:space:]]*$/) { exit }
      sub(/^          /, "")
      print
    }
  ' "$WORKFLOW"
}

# `${{ }}` is substituted by Actions *before* bash parses the script, so bash
# never sees it — and would choke on it ("bad substitution") if it did. The
# harness has to model that same textual substitution or it tests a script the
# runner never runs.
substitute_expressions() {
  sed 's/\${{[^}]*}}/EXPR/g'
}

extract_step "Create or update the rolling report issue" \
  | substitute_expressions > "$WORK/step.sh"
if [ ! -s "$WORK/step.sh" ]; then
  echo "FAIL - could not extract the step from $WORKFLOW (was it renamed?)"
  exit 1
fi
# The extraction is the load-bearing part: if it silently produced a stub,
# every assertion below would pass against nothing.
if ! grep -q "EXISTING_ISSUE=" "$WORK/step.sh"; then
  echo "FAIL - the extracted block does not look like the rolling-issue step"
  exit 1
fi

# `gh` stub: `issue list` answers from a fixture through the step's own --jq
# filter, `issue view` returns a fixture body, everything else is logged.
make_gh_stub() {
  cat > "$WORK/gh" <<STUB
#!/usr/bin/env bash
prev=""
filter=""
for a in "\$@"; do
  [ "\$prev" = "--jq" ] && filter="\$a"
  prev="\$a"
done
if [ "\$1 \$2" = "issue list" ]; then
  echo '$1' | jq -r "\$filter"
  exit 0
fi
if [ "\$1 \$2" = "issue view" ]; then
  if [ -n "\$filter" ]; then echo '$2' | jq -r "\$filter"; else echo '$2'; fi
  exit 0
fi
echo "GH-CALL: \$*" >&2
STUB
  chmod +x "$WORK/gh"
}

run_step() {
  local rc=0
  cd "$WORK"
  out=$(PATH="$WORK:$PATH" bash "$WORK/step.sh" 2>&1) || rc=$?
  cd "$ROOT"
  if [ "$rc" -ne 0 ]; then
    echo "FAIL - the step exited $rc"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

check() {
  local label="$1" expected="$2"
  if grep -q -- "$expected" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label"
    echo "       expected to find: $expected"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

# Asserts one call precedes another. "Both happen" is not the property the
# workflow argues for: the edit commits the new baseline, so editing first
# means a failed comment silently advances it past tests nobody was told
# about. Swapping the two back leaves every other check in this file green —
# and that swap is exactly the edit that produced the round-6 regression.
expect_order() {
  local label="$1" first="$2" second="$3"
  local a b
  # `|| true` on both: a non-matching `grep -n` exits 1, `pipefail` carries
  # that past the successful `head` and `cut`, and `set -e` then kills the
  # harness — on precisely the missing-`gh issue comment` regression this
  # helper exists to catch. Without it the run ends on a bare non-zero exit
  # with no `FAIL -` line and no output dump, and the cases below never run,
  # which makes the `${a:-none}` fallback in the message below unreachable by
  # construction. Same trap the sibling harness documents.
  a=$(grep -n -- "$first" <<<"$out" | head -1 | cut -d: -f1 || true)
  b=$(grep -n -- "$second" <<<"$out" | head -1 | cut -d: -f1 || true)
  if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ]; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: '$first' at ${a:-none}, '$second' at ${b:-none}"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

refute() {
  local label="$1" unexpected="$2"
  if grep -q -- "$unexpected" <<<"$out"; then
    echo "FAIL - $label (found: $unexpected)"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  else
    echo "ok   - $label"
  fi
}

TITLE="CI Flakiness Report (rolling 7-day window)"
LEGACY="CI Flakiness Report - Week of 2026-08-16"

# The step reads flakiness_report.md and tests.json from the working dir.
printf 'report body\n\n<!-- flaky-baseline\nt01\nt02\n-->\n' > "$WORK/flakiness_report.md"
echo '[{"name":"t01"},{"name":"t02"}]' > "$WORK/tests.json"

# --- The rolling issue must be edited, not merely commented on. Dropping the
#     edit is the round-5 regression: the run reports success, the issue keeps
#     its old title and body, no baseline is ever committed, and the newly
#     flaky comment re-fires with the full set every day thereafter.
make_gh_stub "[{\"title\":\"$TITLE\",\"number\":77}]" \
  '{"body":"old\n<!-- flaky-baseline\nt01\nt02\n-->"}'
run_step
check  "update: edits the issue"            "issue edit 77"
check  "update: retitles it"                "\-\-title"
check  "update: writes the new body"        "flakiness_report.md"
refute "update: files no duplicate"         "issue create"

# --- A legacy date-stamped issue is adopted and retitled in place. This is
#     the single-use migration path, and the retitle is the whole point of it.
make_gh_stub "[{\"title\":\"$LEGACY\",\"number\":417}]" '{"body":"no marker here"}'
run_step
check "adoption: edits the legacy issue"    "issue edit 417"
check "adoption: announces the full set"    "Newly flaky"
expect_order "adoption: comments before committing the new baseline" \
  "issue comment 417" "issue edit 417"

# --- Nothing matching means a fresh issue, and it must carry the label the
#     lookup filters on.
make_gh_stub '[{"title":"unrelated","number":2}]' '{"body":""}'
run_step
check  "create: files a new issue"          "issue create"
# Anchored to the create call. Unanchored, this happens to pass today only
# because the `gh label create "ci-health"` above it carries `2>/dev/null`,
# and the stub logs to stderr — so that call's line never reaches the captured
# output. That is an accident of the workflow's redirection, not a property of
# the assertion: drop the `2>/dev/null` in a cleanup and the check silently
# goes vacuous while still passing. The sibling harness hit the same trap for
# the real reason (its label call is not redirected) and anchors for it.
#
# What is being pinned: an issue filed without the label is invisible to every
# subsequent lookup, so the job files a fresh one daily — the accumulation
# this PR exists to stop.
check  "create: labels it ci-health"        "issue create.*ci-health"
refute "create: does not edit"              "issue edit"

# --- An unchanged set must not comment. This is the churn the PR removes;
#     re-announcing daily would put it straight back.
make_gh_stub "[{\"title\":\"$TITLE\",\"number\":77}]" \
  '{"body":"old\n<!-- flaky-baseline\nt01\nt02\n-->"}'
run_step
refute "steady state: posts no comment when nothing is new" "issue comment"

# --- A body that has been through the web UI comes back CRLF-normalized.
#     Every other fixture here is LF, so the carriage-return strip added in
#     round 5 was never exercised — removing it would have left all of them
#     green while a maintainer's typo fix silently re-announced the full set.
make_gh_stub "[{\"title\":\"$TITLE\",\"number\":77}]" \
  '{"body":"old\r\n<!-- flaky-baseline\r\nt01\r\nt02\r\n-->\r\n"}'
echo '[{"name":"t01"},{"name":"t02"}]' > "$WORK/tests.json"
run_step
refute "CRLF body: still recognises the baseline, posts no comment" "issue comment"

# --- A genuinely new test does comment, and names only that one.
echo '[{"name":"t01"},{"name":"t02"},{"name":"t03_new"}]' > "$WORK/tests.json"
run_step
check "new test: comments" "t03_new"

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
