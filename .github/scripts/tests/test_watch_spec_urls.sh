#!/usr/bin/env bash
# Exercises watch_spec_urls.sh against a local HTTP server (the published
# spec URLs, alive or dead) and a stubbed `gh` (the tracking issue), so the
# branches that decide whether to comment are asserted here rather than on
# the first morning a URL comes back.
#
# Run directly: .github/scripts/tests/test_watch_spec_urls.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/watch_spec_urls.sh"
WORKFLOW="$(dirname "$(dirname "$HERE")")/workflows/api-surface-sweep.yml"
SERVER="$HERE/fake_api_server.py"
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

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# The spec URLs: one serves a spec, one redirects to it, one is a 404, one
# serves JSON that is not a spec.
cat >"$WORK/routes.json" <<'EOF'
{
  "GET /live/spec.json": {"body": {"openapi": "3.0.3", "info": {"version": "v1beta"},
                                   "paths": {"/a": {}, "/b": {}}}},
  "GET /old/spec-v1.json": {"status": 302, "headers": {"Location": "/live/spec.json"}},
  "GET /not-a-spec.json": {"body": {"error": {"code": 404}}}
}
EOF
python3 "$SERVER" "$WORK/routes.json" "$WORK" "$WORK/port" "$WORK/requests" &
SERVER_PID=$!
for _ in $(seq 1 100); do
    [ -s "$WORK/port" ] && break
    sleep 0.05
done
[ -s "$WORK/port" ] || {
    echo "the fake server did not start" >&2
    exit 1
}
BASE="http://127.0.0.1:$(cat "$WORK/port")"

# A spec snapshot recording where it came from.
snapshot() {
    printf '{"source": "%s", "version": "%s", "fetched": "2026-11-01"}\n' "$2" "$3" >"$WORK/$1"
}

# A `gh` that answers `issue view` from $GH_VIEW (or fails when it is
# `fail`), and logs every call, with a comment's body, to $WORK/gh.log.
mkdir -p "$WORK/bin"
cat >"$WORK/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >>"$GH_LOG"
if [ "$1 $2" = "issue view" ]; then
    [ "$GH_VIEW" = fail ] && { echo "HTTP 502" >&2; exit 1; }
    printf '%s\n' "$GH_VIEW"
    exit 0
fi
prev=""
for arg in "$@"; do
    if [ "$prev" = "--body-file" ]; then
        cat "$arg" >>"$GH_LOG"
    fi
    prev=$arg
done
EOF
chmod +x "$WORK/bin/gh"

OPEN='{"state": "OPEN", "comments": []}'

run() {
    : >"$WORK/gh.log"
    rc=0
    out=$(PATH="$WORK/bin:$PATH" GH_LOG="$WORK/gh.log" GH_VIEW="${GH_VIEW:-$OPEN}" \
        bash "$SCRIPT" "$@" 2>&1) || rc=$?
}

expect_rc() {
    if [ "$rc" -ne "$2" ]; then
        fail "$1: expected exit $2, got $rc: $out"
    fi
}

comments() { grep -c '^gh issue comment' "$WORK/gh.log" || true; }

# Every URL dead: exit 0, and the issue is not even read.
snapshot dead.json "$BASE/dead/spec.json" v1beta
snapshot notspec.json "$BASE/not-a-spec.json" v1
run 511 "$WORK/dead.json" "$WORK/notspec.json"
expect_rc "all dead" 0
grep -q "HTTP 404" <<<"$out" || fail "a 404 was not reported: $out"
grep -q "not an OpenAPI document" <<<"$out" || fail "JSON that is not a spec was not reported: $out"
[ -s "$WORK/gh.log" ] && fail "all dead, but gh was called: $(cat "$WORK/gh.log")"

# One serves through a redirect: one comment, naming both URLs, with the
# marker and the refresh command for its API version.
snapshot v1.json "$BASE/old/spec-v1.json" v1
run 511 "$WORK/dead.json" "$WORK/v1.json"
expect_rc "one serves" 0
[ "$(comments)" = 1 ] || fail "expected one comment, got: $(cat "$WORK/gh.log")"
grep -q "^gh issue comment 511 --body-file" "$WORK/gh.log" || fail "not a comment on #511: $(cat "$WORK/gh.log")"
for want in "$BASE/old/spec-v1.json, served from $BASE/live/spec.json" \
    "<!-- spec-url-serves: $BASE/old/spec-v1.json -->" \
    "refresh_api_snapshot.sh --spec-v1 spec-v1.json --spec-source '$BASE/old/spec-v1.json'" \
    '`info.version` v1beta, 2 paths'; do
    grep -qF -- "$want" "$WORK/gh.log" || fail "the comment lacks: $want"$'\n'"$(cat "$WORK/gh.log")"
done
grep -qF "dead/spec.json" "$WORK/gh.log" && fail "the comment names the dead URL"

# Already announced: the marker is in a comment, so no second comment.
GH_VIEW=$(jq -n --arg m "<!-- spec-url-serves: $BASE/old/spec-v1.json -->" \
    '{state: "OPEN", comments: [{body: ("served again\n" + $m)}]}') \
    run 511 "$WORK/v1.json"
expect_rc "already announced" 0
[ "$(comments)" = 0 ] || fail "commented twice on one URL: $(cat "$WORK/gh.log")"

# A closed issue gets no comment.
GH_VIEW='{"state": "CLOSED", "comments": []}' run 511 "$WORK/v1.json"
expect_rc "closed issue" 0
[ "$(comments)" = 0 ] || fail "commented on a closed issue: $(cat "$WORK/gh.log")"

# A snapshot with no URL for a source is skipped, not probed.
: >"$WORK/requests"
snapshot manual.json manual v1beta
run 511 "$WORK/manual.json"
expect_rc "manual source" 0
[ -s "$WORK/requests" ] && fail "a manual source was probed: $(cat "$WORK/requests")"

# A failed gh call fails the run, rather than reading as "no issue".
GH_VIEW=fail run 511 "$WORK/v1.json"
expect_rc "gh view fails" 1

# Bad usage and unreadable snapshots.
run
expect_rc "no arguments" 2
run 511
expect_rc "no snapshot" 2
run "#511" "$WORK/v1.json"
expect_rc "not an issue number" 2
run 511 "$WORK/missing.json"
expect_rc "missing snapshot" 2

# The sweep runs it on both committed snapshots, for #511.
grep -q 'watch_spec_urls.sh "$SPEC_URL_ISSUE"' "$WORKFLOW" ||
    fail "api-surface-sweep.yml does not run watch_spec_urls.sh"
for spec in openapi-v1beta.json openapi-v1.json; do
    grep -q "tests/fixtures/api_surface/$spec" "$WORKFLOW" ||
        fail "api-surface-sweep.yml does not watch $spec's URL"
done

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "watch_spec_urls.sh: all checks passed"
