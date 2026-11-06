#!/usr/bin/env bash
# Exercises `cleanup_test_resources.py` against recorded API responses
# (#492).
#
# The janitor deletes resources permanently, so what it chooses is the
# whole product: it must take every test-made resource past the age floor,
# and nothing else. `fake_api_server.py` replays the listings in
# fixtures/janitor/ (recorded from the test key 2026-11-05 and trimmed,
# plus a few made-up neighbours that are not a test's: see the README there)
# and logs every request, so each case asserts the exact set of DELETEs.

set -euo pipefail

command -v python3 >/dev/null 2>&1 || {
  echo "python3 is required to run this harness (brew install python3 / apt install python3)" >&2
  exit 1
}

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/cleanup_test_resources.py"
WORKFLOW="$(dirname "$(dirname "$HERE")")/workflows/test-resource-janitor.yml"
SERVER="$HERE/fake_api_server.py"
FIXTURES="$HERE/fixtures/janitor"
WORK="$(mktemp -d)"
SERVER_PID=""
stop_server() {
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
  fi
}
trap 'stop_server; rm -rf "$WORK"' EXIT

# The recorded listings are dated, so "now" is fixed: six hours before it
# is 2026-11-05T06:00:00Z.
NOW="2026-11-05T12:00:00Z"
failures=0
# The opt-in a scheduled run reads; a developer's own must not leak in.
unset JANITOR_DELETE

# Starts the fake API with `routes` (a file) and waits for its port.
start_server() {
  local routes="$1"
  stop_server
  rm -f "$WORK/port" "$WORK/requests"
  : > "$WORK/requests"
  python3 "$SERVER" "$routes" "$FIXTURES" "$WORK/port" "$WORK/requests" &
  SERVER_PID=$!
  for _ in $(seq 1 100); do
    [ -s "$WORK/port" ] && break
    sleep 0.05
  done
  [ -s "$WORK/port" ] || { echo "the fake API did not start" >&2; exit 1; }
  BASE_URL="http://127.0.0.1:$(cat "$WORK/port")"
}

run_janitor() {
  rc=0
  out=$(GEMINI_API_KEY=fixture-key JANITOR_BACKOFF_SCALE=0 \
    python3 "$SCRIPT" --base-url "$BASE_URL" --now "$NOW" "$@" 2>&1) || rc=$?
}

deletes() { grep '^DELETE ' "$WORK/requests" | sort || true; }

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
  if grep -qF -- "$needle" <<<"$out"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected to find '$needle'"
    sed 's/^/         /' <<<"$out"
    failures=$((failures + 1))
  fi
}

expect_in_file() {
  local label="$1" file="$2" needle="$3"
  if grep -qF -- "$needle" "$file"; then
    echo "ok   - $label"
  else
    echo "FAIL - $label: expected to find '$needle' in $(basename "$file")"
    sed 's/^/         /' "$file" 2>/dev/null || true
    failures=$((failures + 1))
  fi
}

expect_deletes() {
  local label="$1" want="$2" got
  got=$(deletes)
  if [ "$got" = "$want" ]; then
    echo "ok   - $label"
  else
    echo "FAIL - $label"
    diff <(echo "$want") <(echo "$got") | sed 's/^/         /' || true
    failures=$((failures + 1))
  fi
}

# Every test-made resource older than six hours, and nothing else: no
# resource without a marker, none under six hours old, and none whose age
# cannot be told. Sorted like `deletes`.
EXPECTED=$(sort <<'EOF'
DELETE /v1beta/environments/52ab487220d5f6d72523f3c8f87a86e1
DELETE /v1beta/environments/fd4100e45dcbb2c909905f47ef297734
DELETE /v1beta/environments/8587694d6bd36adc7f96164dcc5caffe
DELETE /v1beta/environments/d05d5a0c69999a77911f9f4b641f2668
DELETE /v1beta/environments/0b1d0e5c0de0c0ffee00000000prefix
DELETE /v1beta/credentials/genai-rs-test-bearer-287836888
DELETE /v1beta/credentials/genai-rs-test-dup-287793240
DELETE /v1beta/webhooks/wh-test-endpoint
DELETE /v1beta/webhooks/wh-test-name
DELETE /v1beta/agents/genai-rs-test-agent-1793800000-123456789
DELETE /v1beta/triggers/t-test
DELETE /v1beta/triggers/t-legacy
DELETE /v1beta/fileSearchStores/genairstestbasics-a1b2c3d4e5f6?force=true
DELETE /v1beta/voices/voice_p5gdvkkj5mvw
DELETE /v1beta/voices/voice_legacytestname
EOF
)

# --- Dry run: the same choices, and no DELETE at all.
start_server "$FIXTURES/routes.json"
run_janitor --dry-run
expect_rc     "dry run: exits 0" 0
expect_deletes "dry run: sends no DELETE" ""
expect_output "dry run: names an old empty environment" \
  "would delete environments/52ab487220d5f6d72523f3c8f87a86e1: no sources and no files, last active 2026-07-19T23:20:03"
expect_output "dry run: counts what it would delete" "| environments | 12 | 8 | 5 | 3 | 0 |"
expect_output "dry run: says nothing was deleted" "Dry run: nothing was deleted"

# --- Delete: exactly the expected set, pages followed (an environment and a
#     voice sit on second pages).
start_server "$FIXTURES/routes.json"
run_janitor --delete
expect_rc      "delete: exits 0" 0
expect_deletes "delete: deletes exactly the old test resources" "$EXPECTED"
expect_output  "delete: keeps a young test environment" \
  "kept     environments/0b1d0e5c0de0c0ffee000000000young"
expect_output  "delete: keeps an agent whose age it cannot tell" \
  "kept     agents/genai-rs-test-agent-287338089: a genai-rs-test id, but its age is unknown"
expect_output  "delete: keeps a young prompted voice from page two" \
  "kept     voices/voice_youngtestvoice"
# Environments carry no field a test can mark, so a sourceless one is taken
# only when it is empty and unused for the floor; one that holds files (a
# leaked fork, or a person's) is kept and listed for a person to check.
expect_output  "delete: keeps an empty sourceless environment used within the floor" \
  "kept     environments/0b1d0e5c0de0c0ffee000000000inuse: no sources and no files, last active 2026-11-05T09:30:00+00:00, under 6h ago"
expect_output  "delete: lists a sourceless environment holding files for review" \
  "review   environments/0b1d0e5c0de0c0ffee00000000holder: no sources, but it holds 3 file(s)"
# Named outright too, so a widened EXPECTED cannot quietly take one.
touched=0
for id in 0b1d0e5c0de0c0ffee0000000000user 0b1d0e5c0de0c0ffee000000000mixed \
          0b1d0e5c0de0c0ffee0000000gitrepo 0b1d0e5c0de0c0ffee00000000holder \
          0b1d0e5c0de0c0ffee000000000inuse 8e8112f1-f062-43dd-a9e2-5cf1650c2957 \
          github-deploy-token wh-production support-bot t-nightly genairswp2bprobe \
          voice_brandnarrator; do
  if grep -q -- "$id" <(deletes); then
    echo "FAIL - delete: removed $id, which carries no test marker"
    touched=1
    failures=$((failures + 1))
  fi
done
[ "$touched" -eq 1 ] || echo "ok   - delete: no resource without a marker was touched"

# --- A scheduled run is a dry run unless JANITOR_DELETE is true: deleting
#     is the key owner's opt-in (its first run would take about 112
#     environments and 3 credentials). The summary lists what it would take.
for value in "" false 1 yes "true-ish"; do
  start_server "$FIXTURES/routes.json"
  rm -f "$WORK/summary.md"
  if [ -z "$value" ]; then
    run_janitor --scheduled --summary "$WORK/summary.md"
    shown="unset"
  else
    JANITOR_DELETE="$value" run_janitor --scheduled --summary "$WORK/summary.md"
    shown="'$value'"
  fi
  expect_rc      "scheduled, JANITOR_DELETE $shown: exits 0" 0
  expect_deletes "scheduled, JANITOR_DELETE $shown: sends no DELETE" ""
  expect_output  "scheduled, JANITOR_DELETE $shown: says why it is a dry run" \
    "Scheduled run: JANITOR_DELETE is $shown, not true, so this is a dry run."
done
expect_output  "scheduled dry run: names the opt-in in a notice" \
  "::notice::15 test resource(s) would be deleted. Set the repository variable JANITOR_DELETE to true"
expect_in_file "scheduled dry run: the summary is headed as a dry run" "$WORK/summary.md" \
  "## Test resource janitor (scheduled, dry run)"
expect_in_file "scheduled dry run: the summary says how to opt in" "$WORK/summary.md" \
  "Set the repository variable JANITOR_DELETE to true to let scheduled runs delete."
expect_in_file "scheduled dry run: the summary counts what it would delete" "$WORK/summary.md" \
  "### Would delete (15)"
expect_in_file "scheduled dry run: the summary lists each resource" "$WORK/summary.md" \
  '- `environments/52ab487220d5f6d72523f3c8f87a86e1`: no sources'
expect_in_file "scheduled dry run: the summary lists a store" "$WORK/summary.md" \
  '- `fileSearchStores/fileSearchStores/genairstestbasics-a1b2c3d4e5f6`: a genai-rs-test display name'
expect_in_file "scheduled dry run: the summary names a marked resource of unknown age" "$WORK/summary.md" \
  '- `agents/genai-rs-test-agent-287338089`: a genai-rs-test id'
expect_in_file "scheduled dry run: the summary lists the environments to check by hand" "$WORK/summary.md" \
  "### Unmarked, kept: check by hand (1)"
expect_in_file "scheduled dry run: the summary names the sourceless environment holding files" "$WORK/summary.md" \
  '- `environments/0b1d0e5c0de0c0ffee00000000holder`: no sources, but it holds 3 file(s)'
expect_in_file "scheduled dry run: the summary has the table" "$WORK/summary.md" \
  "| environments | 12 | 8 | 5 | 3 | 0 |"
expect_in_file "scheduled dry run: the summary says nothing was deleted" "$WORK/summary.md" \
  "Dry run: nothing was deleted"

for value in true TRUE " true "; do
  start_server "$FIXTURES/routes.json"
  rm -f "$WORK/summary.md"
  JANITOR_DELETE="$value" run_janitor --scheduled --summary "$WORK/summary.md"
  expect_rc      "scheduled, JANITOR_DELETE '$value': exits 0" 0
  expect_deletes "scheduled, JANITOR_DELETE '$value': deletes exactly the old test resources" "$EXPECTED"
done
expect_output  "scheduled delete: says it deletes" "Scheduled run: JANITOR_DELETE is true, so it deletes."
expect_in_file "scheduled delete: the summary is headed as a delete" "$WORK/summary.md" \
  "## Test resource janitor (scheduled, delete)"
expect_in_file "scheduled delete: the summary lists what it deleted" "$WORK/summary.md" \
  "### Deleted (15)"
if grep -q '::notice::' <<<"$out"; then
  echo "FAIL - scheduled delete: printed the dry-run notice"
  failures=$((failures + 1))
else
  echo "ok   - scheduled delete: no dry-run notice"
fi

# --- The explicit modes ignore the opt-in: --dry-run never deletes.
start_server "$FIXTURES/routes.json"
JANITOR_DELETE=true run_janitor --dry-run
expect_rc      "dry run with JANITOR_DELETE=true: exits 0" 0
expect_deletes "dry run with JANITOR_DELETE=true: sends no DELETE" ""

# --- The workflow passes --scheduled on a schedule, never --delete, and
#     hands it the repository variable; a manual run keeps its input.
if grep -qE "event_name == 'schedule' && 'delete'" "$WORKFLOW"; then
  echo "FAIL - workflow: a scheduled run is mapped straight to delete"
  failures=$((failures + 1))
else
  echo "ok   - workflow: a scheduled run is not mapped straight to delete"
fi
expect_in_file "workflow: a scheduled run passes --scheduled" "$WORKFLOW" \
  'if [ "$EVENT_NAME" = schedule ]; then'
expect_in_file "workflow: --scheduled is the scheduled mode" "$WORKFLOW" "mode=scheduled"
expect_in_file "workflow: the opt-in comes from the repository variable" "$WORKFLOW" \
  'JANITOR_DELETE: ${{ vars.JANITOR_DELETE }}'
expect_in_file "workflow: a manual run follows its mode input" "$WORKFLOW" \
  'dry-run|delete) mode="$INPUT_MODE" ;;'
expect_in_file "workflow: the summary goes to the job summary" "$WORKFLOW" \
  '--summary "$GITHUB_STEP_SUMMARY"'

# --- A raised floor keeps more: 100 hours before "now" is
#     2026-11-01T08:00:00Z, so the credentials from 07:59:48 and the webhook
#     from 01:00 that day go, and the webhook from 2026-11-02 stays.
start_server "$FIXTURES/routes.json"
run_janitor --delete --min-age-hours 100 --kinds credentials,webhooks
expect_rc      "100h floor: exits 0" 0
expect_deletes "100h floor: only resources older than 100 hours" "$(printf '%s\n' \
  'DELETE /v1beta/credentials/genai-rs-test-bearer-287836888' \
  'DELETE /v1beta/credentials/genai-rs-test-dup-287793240' \
  'DELETE /v1beta/webhooks/wh-test-endpoint' | sort)"

# --- --kinds limits what is listed, not only what is deleted.
start_server "$FIXTURES/routes.json"
run_janitor --dry-run --kinds agents
if grep -v '^GET /v1beta/agents' "$WORK/requests" | grep -q .; then
  echo "FAIL - --kinds: requested other kinds: $(grep -v '^GET /v1beta/agents' "$WORK/requests" | head -3)"
  failures=$((failures + 1))
else
  echo "ok   - --kinds: lists only the named kind"
fi

# --- A failed DELETE fails the run, naming it, after the others went
#     through (5xx is retried first).
jq '. + {"DELETE /v1beta/webhooks/wh-test-endpoint": {"status": 500, "body": {"error": {"message": "backend"}}}}' \
  "$FIXTURES/routes.json" > "$WORK/routes-delete-fails.json"
start_server "$WORK/routes-delete-fails.json"
run_janitor --delete --kinds webhooks,credentials
expect_rc     "failed delete: exits 1" 1
expect_output "failed delete: names the resource" "::error::webhooks/wh-test-endpoint: delete failed"
expect_output "failed delete: the rest still go" "deleted  credentials/genai-rs-test-dup-287793240"
if [ "$(grep -c '^DELETE /v1beta/webhooks/wh-test-endpoint' "$WORK/requests")" -eq 4 ]; then
  echo "ok   - failed delete: a 5xx is retried before giving up"
else
  echo "FAIL - failed delete: expected 4 attempts, got $(grep -c '^DELETE /v1beta/webhooks/wh-test-endpoint' "$WORK/requests")"
  failures=$((failures + 1))
fi

# --- A delete that went through but answered 500 is retried into a 404:
#     the resource is gone, so it counts as deleted and the run stays green
#     (a red scheduled run would open an escalation issue for nothing).
jq '. + {"DELETE /v1beta/webhooks/wh-test-endpoint": [
      {"status": 500, "body": {"error": {"message": "backend"}}},
      {"status": 404, "body": {"error": {"message": "not found"}}}]}' \
  "$FIXTURES/routes.json" > "$WORK/routes-delete-404.json"
start_server "$WORK/routes-delete-404.json"
run_janitor --delete --kinds webhooks
expect_rc     "retried delete into a 404: exits 0" 0
expect_output "retried delete into a 404: reported as already gone" \
  "gone     webhooks/wh-test-endpoint: the test webhook endpoint, created 2026-11-01T01:00:00+00:00; already deleted (404)"
expect_output "retried delete into a 404: counted as deleted" "| webhooks | 4 | 3 | 2 | 1 | 0 |"

# --- A 429 honours Retry-After and then succeeds.
jq '. + {"GET /v1beta/credentials?page_size=100": [
      {"status": 429, "headers": {"Retry-After": "0"}, "body": {"error": {"message": "slow down"}}},
      "credentials.json"]}' "$FIXTURES/routes.json" > "$WORK/routes-429.json"
start_server "$WORK/routes-429.json"
run_janitor --delete --kinds credentials
expect_rc      "429: retried, exits 0" 0
expect_deletes "429: then deletes as usual" "$(printf '%s\n' \
  'DELETE /v1beta/credentials/genai-rs-test-bearer-287836888' \
  'DELETE /v1beta/credentials/genai-rs-test-dup-287793240' | sort)"

# --- A listing that fails fails the run, and the other kinds still run.
jq '. + {"GET /v1beta/agents?page_size=100": {"status": 403, "body": {"error": {"message": "denied"}}}}' \
  "$FIXTURES/routes.json" > "$WORK/routes-list-fails.json"
start_server "$WORK/routes-list-fails.json"
run_janitor --dry-run --kinds agents,triggers
expect_rc     "failed list: exits 1" 1
expect_output "failed list: names the kind" "::error::agents: listing failed"
expect_output "failed list: the next kind still runs" "would delete triggers/t-test"

# --- Usage errors: a mode is required, the floor cannot drop under an
#     hour, and an empty key is refused before any request.
start_server "$FIXTURES/routes.json"
run_janitor
expect_rc "no mode: usage error" 2
run_janitor --dry-run --delete
expect_rc "both modes: usage error" 2
run_janitor --scheduled --delete
expect_rc "scheduled and delete: usage error" 2
run_janitor --dry-run --min-age-hours 0.5
expect_rc "floor under an hour: usage error" 2
run_janitor --dry-run --kinds environments,files
expect_rc "unknown kind: usage error" 2
rc=0
out=$(GEMINI_API_KEY="  " python3 "$SCRIPT" --base-url "$BASE_URL" --dry-run 2>&1) || rc=$?
expect_rc "blank key: refused" 2
if [ -s "$WORK/requests" ]; then
  echo "FAIL - usage errors: sent requests: $(head -3 "$WORK/requests")"
  failures=$((failures + 1))
else
  echo "ok   - usage errors: no request was sent"
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "$failures check(s) failed."
  exit 1
fi
echo "All checks passed."
