#!/usr/bin/env bash
# Opens or updates a `ci-health` issue when a scheduled workflow fails.
#
# Scheduled workflows fail quietly. The flakiness report failed 22 days
# straight, then stopped firing for ~10 weeks, and nobody learned of any of
# the three transitions (#431). GitHub emails the workflow file's last
# committer on a scheduled-run failure — the 22 failures presumably produced
# 22 such emails and no action, which is evidence about that channel rather
# than about the people.
#
# So the report's own escalation mechanism gets pointed at the report's own
# health: one rolling issue per workflow, updated in place, closed by
# `resolve_scheduled_failure.sh` on the next success. A monitoring job that
# can fail without saying so is worth less than its green runs suggest.
#
#
# Boundary worth naming: this runs inside the job it reports on and reads its
# own script from the checkout, so a scheduled run that dies *before or
# during* `actions/checkout` fails as silently as it did before #431. That is
# the one sub-case an in-job mechanism cannot cover; the general
# not-running-at-all case is the deferred liveness work.
# Usage: report_scheduled_failure.sh <workflow-name> <run-url> [context]
# Requires: GH_TOKEN with issues:write.

set -euo pipefail

WORKFLOW="${1:?usage: report_scheduled_failure.sh <workflow-name> <run-url> [context]}"
RUN_URL="${2:?usage: report_scheduled_failure.sh <workflow-name> <run-url> [context]}"

# Optional third argument: the explanatory paragraph in the issue body.
#
# The default below is written for a monitoring workflow, where a failure
# means the monitoring broke. That framing is wrong for `Security Audit`,
# whose overwhelmingly likely reason to fail on a schedule is a genuine new
# RUSTSEC advisory — the workflow working, not the workflow being broken. An
# issue that points such a reader at CI health instead of at the
# vulnerability, and then says "Still failing" weekly until the advisory is
# resolved, misleads about which thing needs attention.
CONTEXT="${3:-Scheduled workflows fail quietly — the only other signal is an email
to whoever last committed the workflow file, which does not survive a
change of maintainer. See #431 for the history that motivated this.}"

# Exported before the query below, not after: the `--jq` filter reads
# `$ENV.TITLE`, which resolves in the environment of the `gh` child process.
# As a plain shell variable it would be absent there, `$ENV.TITLE` would be
# null, `select(.title == null)` would match nothing, and every failing run
# would file a fresh duplicate while the update path below never ran.
export TITLE="Scheduled workflow failing: $WORKFLOW"

# Looked up by a dedicated label, not by `ci-health`.
#
# `ci-health` is shared with the flakiness report, which currently files a
# *new* issue every day (#425 fixes that, but this must not depend on it).
# `gh issue list` is newest-first and capped, and this rolling issue is
# created once and thereafter only edited — edits do not bump created_at, so
# it sinks steadily. At roughly 100 newer issues it falls off the page, the
# lookup returns empty, and every failure files a duplicate: exactly the
# flood this script exists to avoid.
#
# A label only these two scripts apply keeps the population at one per
# watched workflow. `ci-health` is applied as well, so the issues still show
# up in the existing view.
ESCALATION_LABEL="ci-health-escalation"

gh label create "$ESCALATION_LABEL" \
  --description "A scheduled workflow is failing (opened by the workflow itself)" \
  --color "b60205" \
  2>/dev/null || true
gh label create "ci-health" \
  --description "CI health and flakiness tracking" \
  --color "fbca04" \
  2>/dev/null || true

# `--state all` for the same reason the API sweep uses it: an issue closed
# without the underlying failure being fixed would otherwise get a fresh
# duplicate filed every morning.
EXISTING=$(gh issue list \
  --label "$ESCALATION_LABEL" \
  --state all \
  --limit 100 \
  --json number,title \
  --jq '[.[] | select(.title == $ENV.TITLE) | .number] | first // empty')

# mktemp rather than the working directory: this runs inside a checkout, and
# a stray report file in the repo root is the kind of thing a later step
# picks up by accident.
# Streak, for the update path.
#
# Without this the body edit below rewrites the issue with text that is
# identical every time bar the run URL — which the "Still failing" comment
# already carries — so the edit is close to a no-op. Streak length is the
# thing #431 is actually about: "it failed again", repeated 22 times, is
# what the email channel already provided and what nobody acted on.
# "Failing since 2026-05-01, 22 consecutive scheduled runs" is not.
#
# Derived from the issue's own comments, since there is nowhere else to keep
# state. Every failure after the one that opened-or-reopened the issue posts
# one FAILURE_PREFIX comment; every recovery posts RECOVERY_PREFIX and
# closes. So the current streak is the run of failure comments since the last
# recovery, plus this run (which has not commented yet), plus the opening run
# when there has been no recovery to reopen from.
#
# Both prefixes are named constants because one of them is written by a
# *different file*. `resolve_scheduled_failure.sh` posts the recovery
# comment; reword it there and `rindex` here finds nothing, the whole history
# counts as one episode, and the issue reports a streak spanning a recovery
# that did happen — a wrong number in the one field this line was added to
# carry, with nothing failing to say so. The harness asserts the two agree,
# for the same reason it asserts the label.
FAILURE_PREFIX="Still failing"
RECOVERY_PREFIX="Recovered"

streak_line() {
  local issue="$1" view parsed
  view=$(gh issue view "$issue" --json createdAt,comments 2>/dev/null) || return 0
  parsed=$(jq -r \
    --arg recovered "$RECOVERY_PREFIX" \
    --arg failing "$FAILURE_PREFIX" '
    ((.comments // []) | map(.body | startswith($recovered)) | rindex(true)) as $r
    | (if $r == null
       then {opened: .createdAt, rest: (.comments // [])}
       else {opened: null,       rest: ((.comments // [])[($r + 1):])}
       end)
    | (.rest | map(select(.body | startswith($failing)))) as $sf
    | "\(($sf | length) + (if .opened == null then 1 else 2 end))\t\(.opened // $sf[0].createdAt // "")"
  ' <<<"$view" 2>/dev/null) || return 0

  local count since
  count=${parsed%%$'\t'*}
  since=${parsed#*$'\t'}
  [ -n "$count" ] || return 0
  # No timestamp means this run is itself the start of the streak.
  since=${since:0:10}
  [ -n "$since" ] || since=$(date -u +%Y-%m-%d)

  local runs="consecutive scheduled runs"
  [ "$count" = "1" ] && runs="scheduled run"
  echo "- **Failing since $since — $count $runs.**"
}

BODY=$(mktemp)
trap 'rm -f "$BODY"' EXIT

{
  echo "\`$WORKFLOW\` failed on its scheduled run."
  echo
  echo "- Most recent failing run: $RUN_URL"
  [ -n "$EXISTING" ] && streak_line "$EXISTING"
  echo "- This issue is opened by the workflow itself and **closes**"
  echo "  automatically on the next successful run."
  echo
  echo "$CONTEXT"
  echo
  echo "---"
  echo "_Generated by [Claude Code](https://claude.ai/code)_"
} > "$BODY"

if [ -n "$EXISTING" ]; then
  gh issue reopen "$EXISTING" 2>/dev/null || true
  gh issue edit "$EXISTING" --body-file "$BODY"
  # A body edit notifies nobody and does not bump the issue in an inbox, so
  # a second consecutive failure would otherwise be silent.
  gh issue comment "$EXISTING" --body "$FAILURE_PREFIX: $RUN_URL

---
_Generated by [Claude Code](https://claude.ai/code)_"
  echo "Updated issue #$EXISTING"
else
  gh issue create \
    --title "$TITLE" \
    --body-file "$BODY" \
    --label "$ESCALATION_LABEL" \
    --label "ci-health"
fi
