#!/usr/bin/env bash
# Checks that the commit a release tag names passed every job of its main
# push run of rust.yml, before release.yml publishes it (#504).
#
# release.yml's own validation runs on Ubuntu only. The push run is where
# the rest of CI ran on that commit: the Cross Platform legs on macOS and
# Windows, MSRV, the fuzz gate's inputs, the live groups. A tag on a commit
# whose push run went red, or on one main never ran, would publish what CI
# never passed.
#
# Usage: check_release_push_run.sh <commit sha>
#
# It finds the `push` run of rust.yml on `main` for the commit, waits for a
# run still going to finish, then reads its jobs (the latest attempt's, so a
# leg re-run to green counts as green). It passes when every job succeeded
# or was skipped, and the run has Cross Platform legs, none of them
# skipped. Otherwise it names what is wrong and exits 1.
#
# Environment (all optional):
#   RUST_WORKFLOW         the workflow file (default rust.yml)
#   RUN_APPEAR_SECONDS    how long to wait for the run to be listed (300)
#   RUN_FINISH_SECONDS    how long to wait for it to finish (2700)
#   POLL_SECONDS          the wait between looks (30)
# gh needs a token that can read Actions (GH_TOKEN in a workflow).

set -euo pipefail

sha="${1:?usage: check_release_push_run.sh <commit sha>}"
workflow="${RUST_WORKFLOW:-rust.yml}"
appear="${RUN_APPEAR_SECONDS:-300}"
finish="${RUN_FINISH_SECONDS:-2700}"
poll="${POLL_SECONDS:-30}"

command -v jq >/dev/null 2>&1 || {
  echo "check_release_push_run.sh: jq is required" >&2
  exit 2
}

# The newest main push run of the workflow for the commit, as JSON, or
# nothing when there is none.
find_run() {
  gh run list --workflow "$workflow" --branch main --event push --commit "$sha" \
    --limit 5 --json databaseId,status,conclusion,url \
    | jq -c 'sort_by(-.databaseId) | first // empty'
}

waited=0
run=$(find_run)
while [ -z "$run" ]; do
  if [ "$waited" -ge "$appear" ]; then
    echo "::error::No push run of $workflow on main for $sha after ${waited}s. Tag a commit on main once its push run has started; the tag names a commit CI never ran."
    exit 1
  fi
  echo "No push run of $workflow on main for $sha yet; looking again in ${poll}s"
  sleep "$poll"
  waited=$((waited + poll))
  run=$(find_run)
done

id=$(jq -r '.databaseId' <<<"$run")
url=$(jq -r '.url // ""' <<<"$run")
waited=0
while [ "$(jq -r '.status' <<<"$run")" != "completed" ]; do
  if [ "$waited" -ge "$finish" ]; then
    echo "::error::The push run of $workflow for $sha ($url) is still $(jq -r '.status' <<<"$run") after ${waited}s; re-run this job once it finishes."
    exit 1
  fi
  echo "The push run ($url) is $(jq -r '.status' <<<"$run"); looking again in ${poll}s"
  sleep "$poll"
  waited=$((waited + poll))
  run=$(gh run view "$id" --json databaseId,status,conclusion,url)
done

jobs=$(gh run view "$id" --json jobs | jq -c '[.jobs[] | {name, conclusion}]')
echo "Push run $url for $sha:"
jq -r '.[] | "  \(.conclusion)\t\(.name)"' <<<"$jobs"

problems=$(jq -r '
  ([.[] | select(.conclusion != "success" and .conclusion != "skipped")
        | "\(.name): \(.conclusion)"])
  + (if any(.[]; .name | startswith("Cross Platform (")) then [] else
       ["no Cross Platform legs: not a full main push run"] end)
  + [.[] | select((.name | startswith("Cross Platform (")) and .conclusion == "skipped")
         | "\(.name): skipped"]
  | .[]' <<<"$jobs")
if [ -n "$problems" ]; then
  while IFS= read -r problem; do
    echo "::error::$problem"
  done <<<"$problems"
  echo "::error::The push run of $workflow for $sha ($url) is not green on every job, so this release stops before publishing. Fix main and tag the fix, or re-run the failed jobs and then this one."
  exit 1
fi
echo "Every job of the push run passed or was skipped, the Cross Platform legs included."
