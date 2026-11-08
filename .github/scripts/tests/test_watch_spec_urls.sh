#!/usr/bin/env bash
# Exercises watch_spec_urls.sh against a local HTTP server (the published
# spec URLs: matching, drifted, redirected or dead) and a stubbed `gh` (the
# rolling issue), so the branches that decide whether to open, update,
# comment on or close the issue are asserted here rather than on the first
# morning a spec drifts.
#
# Run directly: .github/scripts/tests/test_watch_spec_urls.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(dirname "$(dirname "$(dirname "$HERE")")")"
SCRIPT="$(dirname "$HERE")/watch_spec_urls.sh"
SNAPSHOT_SCRIPT="$(dirname "$HERE")/snapshot_openapi.py"
WORKFLOW="$(dirname "$(dirname "$HERE")")/workflows/api-surface-sweep.yml"
SERVER="$HERE/fake_api_server.py"
TITLE='API surface sweep: a published OpenAPI spec drifted or stopped serving'
WORK="$(mktemp -d)"
SERVER_PID=""
TRUNCATING_PID=""
stop_servers() {
    for pid in "$SERVER_PID" "$TRUNCATING_PID"; do
        if [ -n "$pid" ]; then
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
    SERVER_PID=""
    TRUNCATING_PID=""
}
trap 'stop_servers; rm -rf "$WORK"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# The spec as committed, and as served after a field was added and a
# description reworded (prose the snapshot drops, so it alone is no drift).
cat >"$WORK/served-spec.json" <<'EOF'
{"openapi": "3.0.3", "info": {"version": "v1beta"},
 "paths": {"/v1beta/things": {"get": {}}},
 "components": {"schemas": {"Thing": {"type": "object", "description": "A thing.",
   "properties": {"id": {"type": "string"}}}}}}
EOF
cat >"$WORK/served-reworded.json" <<'EOF'
{"openapi": "3.0.3", "info": {"version": "v1beta"},
 "paths": {"/v1beta/things": {"get": {}}},
 "components": {"schemas": {"Thing": {"type": "object", "description": "Some thing.",
   "properties": {"id": {"type": "string"}}}}}}
EOF
cat >"$WORK/served-drifted.json" <<'EOF'
{"openapi": "3.0.3", "info": {"version": "v1beta"},
 "paths": {"/v1beta/things": {"get": {}}},
 "components": {"schemas": {"Thing": {"type": "object",
   "properties": {"id": {"type": "string"}, "size_bytes": {"type": "string"}}}}}}
EOF
cat >"$WORK/served-no-schemas.json" <<'EOF'
{"openapi": "3.0.3", "info": {"version": "v1"}, "paths": {"/a": {}}}
EOF
cat >"$WORK/routes.json" <<'EOF'
{
  "GET /same/spec.json": {"body": "served-spec.json"},
  "GET /reworded/spec.json": {"body": "served-reworded.json"},
  "GET /drifted/spec.json": {"body": "served-drifted.json"},
  "GET /old/spec.json": {"status": 302, "headers": {"Location": "/drifted/spec.json"}},
  "GET /no-schemas.json": {"body": "served-no-schemas.json"},
  "GET /not-a-spec.json": {"body": {"error": {"code": 404}}},
  "GET /flaky/spec.json": [{"status": 503, "body": {}}, {"body": "served-spec.json"}]
}
EOF
# A server that promises a body and hangs up partway through it, so the
# probe's read fails mid-response.
cat >"$WORK/truncating_server.py" <<'EOF'
import os, socket, sys

port_file = sys.argv[1]
listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen()
with open(port_file + ".tmp", "w", encoding="utf-8") as f:
    f.write(str(listener.getsockname()[1]))
os.replace(port_file + ".tmp", port_file)
while True:
    conn, _ = listener.accept()
    with conn:
        conn.recv(65536)
        conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n"
                     b"Content-Length: 4096\r\n\r\n{\"openapi\": ")
EOF
python3 "$SERVER" "$WORK/routes.json" "$WORK" "$WORK/port" "$WORK/requests" &
SERVER_PID=$!
python3 "$WORK/truncating_server.py" "$WORK/truncating-port" &
TRUNCATING_PID=$!
for _ in $(seq 1 100); do
    [ -s "$WORK/port" ] && [ -s "$WORK/truncating-port" ] && break
    sleep 0.05
done
[ -s "$WORK/port" ] && [ -s "$WORK/truncating-port" ] || {
    echo "the fake servers did not start" >&2
    exit 1
}
BASE="http://127.0.0.1:$(cat "$WORK/port")"
TRUNCATING="http://127.0.0.1:$(cat "$WORK/truncating-port")/spec.json"

# A committed snapshot of spec.json, recording `$2` as where it came from.
snapshot() {
    python3 "$SNAPSHOT_SCRIPT" "$WORK/served-spec.json" "$2" 2026-11-01 >"$WORK/$1"
}

# A `gh` that answers `issue list` from $GH_LIST and `issue view` from
# $GH_VIEW (either fails when it is `fail`), and logs every call, with a
# body file's text, to $WORK/gh.log.
mkdir -p "$WORK/bin"
cat >"$WORK/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >>"$GH_LOG"
case "$1 $2" in
    "issue list")
        [ "$GH_LIST" = fail ] && { echo "HTTP 502" >&2; exit 1; }
        printf '%s\n' "$GH_LIST"
        exit 0
        ;;
    "issue view")
        [ "$GH_VIEW" = fail ] && { echo "HTTP 502" >&2; exit 1; }
        printf '%s\n' "$GH_VIEW"
        exit 0
        ;;
esac
prev=""
for arg in "$@"; do
    if [ "$prev" = "--body-file" ]; then
        cat "$arg" >>"$GH_LOG"
    fi
    prev=$arg
done
EOF
chmod +x "$WORK/bin/gh"

NO_BODY='{"body": ""}'
run() {
    : >"$WORK/gh.log"
    rc=0
    out=$(PATH="$WORK/bin:$PATH" GH_LOG="$WORK/gh.log" GH_LIST="${GH_LIST:-[]}" \
        GH_VIEW="${GH_VIEW:-$NO_BODY}" SPEC_WATCH_RETRY_DELAY=0 \
        bash "$SCRIPT" "$@" 2>&1) || rc=$?
}

expect_rc() {
    if [ "$rc" -ne "$2" ]; then
        fail "$1: expected exit $2, got $rc: $out"
    fi
}

calls() { grep -c "^gh issue $1" "$WORK/gh.log" || true; }
logged() { grep -qF -- "$1" "$WORK/gh.log"; }

ours() { jq -n --arg t "$TITLE" --arg s "$1" '[{number: 544, title: $t, state: $s}]'; }
OTHER=$(jq -n '[{number: 7, title: "API surface sweep: google-genai has new Interactions bindings", state: "OPEN"}]')

snapshot same.json "$BASE/same/spec.json"
snapshot reworded.json "$BASE/reworded/spec.json"
snapshot drifted.json "$BASE/drifted/spec.json"
snapshot redirected.json "$BASE/old/spec.json"
snapshot dead.json "$BASE/dead/spec.json"
snapshot notspec.json "$BASE/not-a-spec.json"
snapshot unreadable.json "$BASE/no-schemas.json"
snapshot flaky.json "$BASE/flaky/spec.json"
snapshot truncated.json "$TRUNCATING"
snapshot malformed.json "http://[malformed/spec.json"

# Every spec matches (prose changes included): no issue is opened, and the
# bindings sweep's issue under the same label is left alone.
GH_LIST=$OTHER run "$WORK/same.json" "$WORK/reworded.json"
expect_rc "all match" 0
grep -q "Every published spec matches" <<<"$out" ||
    fail "a clean run did not say so: $out"
for verb in create edit comment close reopen; do
    [ "$(calls "$verb")" = 0 ] || fail "a clean run called gh issue $verb: $(cat "$WORK/gh.log")"
done

# A clean run closes our open issue, with a comment.
GH_LIST=$(ours OPEN) run "$WORK/same.json"
expect_rc "clean, issue open" 0
[ "$(calls "comment 544")" = 1 ] || fail "no closing comment: $(cat "$WORK/gh.log")"
[ "$(calls "close 544")" = 1 ] || fail "the issue was not closed: $(cat "$WORK/gh.log")"

# A closed issue stays closed on a clean run.
GH_LIST=$(ours CLOSED) run "$WORK/same.json"
expect_rc "clean, issue closed" 0
[ "$(calls close)" = 0 ] || fail "closed a closed issue: $(cat "$WORK/gh.log")"

# A drifted spec, reached through a redirect, with no issue yet: one issue,
# with the diff, the URLs, the refresh command and the marker.
GH_LIST=$OTHER run "$WORK/same.json" "$WORK/redirected.json"
expect_rc "drift, no issue" 0
[ "$(calls create)" = 1 ] || fail "expected one new issue: $(cat "$WORK/gh.log")"
logged "--title $TITLE --label api-sweep" || fail "the issue lacks its title or label: $(cat "$WORK/gh.log")"
for want in "$BASE/old/spec.json (served from $BASE/drifted/spec.json) has drifted" \
    '+    "size_bytes": {' \
    "refresh_api_snapshot.sh --spec-v1beta spec-2.json --spec-source '$BASE/old/spec.json'" \
    "<!-- spec-watch: "; do
    logged "$want" || fail "the issue lacks: $want"$'\n'"$(cat "$WORK/gh.log")"
done
logged "$BASE/same/spec.json" && fail "the issue names the matching URL"
[ "$(calls "edit 7")" = 0 ] || fail "edited the bindings sweep's issue"

# The same findings on an open issue whose body carries their marker: the
# body is refreshed, with no comment.
marker=$(grep -o '<!-- spec-watch: [0-9]* -->' "$WORK/gh.log")
GH_LIST=$(ours OPEN) GH_VIEW=$(jq -n --arg b "old body $marker" '{body: $b}') \
    run "$WORK/same.json" "$WORK/redirected.json"
expect_rc "drift, already reported" 0
[ "$(calls "edit 544")" = 1 ] || fail "the body was not refreshed: $(cat "$WORK/gh.log")"
[ "$(calls comment)" = 0 ] || fail "commented on findings already reported: $(cat "$WORK/gh.log")"
[ "$(calls create)" = 0 ] || fail "filed a duplicate: $(cat "$WORK/gh.log")"

# New findings on a closed issue: reopened, refreshed and commented on.
GH_LIST=$(ours CLOSED) GH_VIEW=$(jq -n --arg b "old body $marker" '{body: $b}') \
    run "$WORK/drifted.json" "$WORK/dead.json"
expect_rc "new findings, issue closed" 0
[ "$(calls "reopen 544")" = 1 ] || fail "the closed issue was not reopened: $(cat "$WORK/gh.log")"
[ "$(calls "edit 544")" = 1 ] || fail "the body was not refreshed: $(cat "$WORK/gh.log")"
[ "$(calls "comment 544")" = 1 ] || fail "new findings were not announced: $(cat "$WORK/gh.log")"

# Dead URLs: a 404, JSON that is not a spec, and a spec the snapshot script
# cannot reduce are each reported, after a second try.
: >"$WORK/requests"
run "$WORK/dead.json" "$WORK/notspec.json" "$WORK/unreadable.json"
expect_rc "dead URLs" 0
for want in "$BASE/dead/spec.json stopped serving the spec" "HTTP 404" \
    "not an OpenAPI document" "$BASE/no-schemas.json serves a spec the snapshot script cannot read" \
    "no components.schemas"; do
    logged "$want" || fail "the issue lacks: $want"$'\n'"$(cat "$WORK/gh.log")"
done
[ "$(grep -c "^GET /dead/spec.json" "$WORK/requests")" = 2 ] ||
    fail "a dead URL was not tried twice: $(cat "$WORK/requests")"
logged "refresh_api_snapshot.sh" && fail "a dead URL got a refresh command"

# A response cut off partway through its body, and a URL the HTTP client
# rejects, are dead URLs too: reported on the issue, not a failed run.
run "$WORK/truncated.json" "$WORK/malformed.json"
expect_rc "truncated response, malformed URL" 0
for want in "$TRUNCATING stopped serving the spec" "IncompleteRead" \
    "http://[malformed/spec.json stopped serving the spec" "ValueError"; do
    logged "$want" || fail "the issue lacks: $want"$'\n'"$out"
done

# A URL that fails once and then serves is not dead.
run "$WORK/flaky.json"
expect_rc "flaky URL" 0
[ "$(calls create)" = 0 ] || fail "a URL that served on the retry was reported: $(cat "$WORK/gh.log")"

# A snapshot with no URL for a source is skipped, not probed.
: >"$WORK/requests"
python3 "$SNAPSHOT_SCRIPT" "$WORK/served-spec.json" manual 2026-11-01 >"$WORK/manual.json"
run "$WORK/manual.json"
expect_rc "manual source" 0
[ -s "$WORK/requests" ] && fail "a manual source was probed: $(cat "$WORK/requests")"

# A failed gh call fails the run, rather than reading as "no issue".
GH_LIST=fail run "$WORK/drifted.json"
expect_rc "gh list fails" 1
GH_LIST=$(ours OPEN) GH_VIEW=fail run "$WORK/drifted.json"
expect_rc "gh view fails" 1

# Bad usage and unreadable snapshots.
run
expect_rc "no arguments" 2
run "$WORK/missing.json"
expect_rc "missing snapshot" 2
printf '{"source": "%s"}\n' "$BASE/same/spec.json" >"$WORK/undated.json"
run "$WORK/undated.json"
expect_rc "no fetch date" 2

# The sweep runs it on both committed snapshots, and the committed ones are
# what the script compares (each records its source and fetch date).
grep -q '\.github/scripts/watch_spec_urls\.sh \\$' "$WORKFLOW" ||
    fail "api-surface-sweep.yml does not run watch_spec_urls.sh"
for spec in openapi-v1beta.json openapi-v1.json; do
    grep -q "tests/fixtures/api_surface/$spec" "$WORKFLOW" ||
        fail "api-surface-sweep.yml does not watch $spec"
    committed="$REPO/tests/fixtures/api_surface/$spec"
    jq -e '(.source | startswith("https://")) and (.fetched | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}$"))' \
        "$committed" >/dev/null || fail "$spec records no source URL and fetch date"
done

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "watch_spec_urls.sh: all checks passed"
