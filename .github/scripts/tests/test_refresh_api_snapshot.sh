#!/usr/bin/env bash
# Fixture tests for refresh_api_snapshot.sh, offline: pip and the dump
# script are stubbed, so no wheel is downloaded.
#
# Run directly: .github/scripts/tests/test_refresh_api_snapshot.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
scripts="$script_dir/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

real_python=$(command -v python3)

# A python3 that answers `-m pip download` itself: FAKE_PIP=fail exits 1 (a
# version that does not exist, no network), FAKE_PIP=ok writes a wheel
# holding an empty `google/genai/_gaos`. Anything else runs the real one.
mkdir -p "$work/bin"
cat >"$work/bin/python3" <<EOF
#!/usr/bin/env bash
if [ "\${1:-}" = -m ] && [ "\${2:-}" = pip ] && [ "\${3:-}" = download ]; then
    if [ "\${FAKE_PIP:-ok}" = fail ]; then
        echo "ERROR: No matching distribution found" >&2
        exit 1
    fi
    dest=""
    while [ \$# -gt 0 ]; do
        if [ "\$1" = -d ]; then dest=\$2; fi
        shift
    done
    mkdir -p "\$dest"
    exec "$real_python" -c 'import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr("google/genai/_gaos/__init__.py", "")' "\$dest/google_genai-0-py3-none-any.whl"
fi
exec "$real_python" "\$@"
EOF
chmod +x "$work/bin/python3"

# A checkout: the script under test, a stub dump script (FAKE_DUMP=fail
# exits 1; otherwise it prints FAKE_DUMP_OUTPUT), the real spec snapshotter,
# a baseline and a committed snapshot.
new_repo() {
    local repo="$work/repo"
    rm -rf "$repo"
    mkdir -p "$repo/.github/scripts" "$repo/tests/fixtures/api_surface"
    cp "$scripts/refresh_api_snapshot.sh" "$scripts/snapshot_openapi.py" "$repo/.github/scripts/"
    cat >"$repo/.github/scripts/dump_bindings_schema.py" <<'EOF'
import os, sys
if os.environ.get("FAKE_DUMP") == "fail":
    sys.exit("dump failed")
print(os.environ.get("FAKE_DUMP_OUTPUT", '{"sdk_version": "new"}'))
EOF
    echo "1.0.0" >"$repo/.github/last-swept-sdk-version"
    echo '{"sdk_version": "1.0.0"}' >"$repo/tests/fixtures/api_surface/bindings.json"
    echo '{"source": "old"}' >"$repo/tests/fixtures/api_surface/openapi-v1beta.json"
}

run() {
    PATH="$work/bin:$PATH" bash "$work/repo/.github/scripts/refresh_api_snapshot.sh" "$@" \
        >"$work/out.log" 2>&1
}

baseline() { tr -d '[:space:]' <"$work/repo/.github/last-swept-sdk-version"; }
snapshot() { cat "$work/repo/tests/fixtures/api_surface/$1"; }

unchanged() {
    local what=$1
    [ "$(baseline)" = 1.0.0 ] || fail "$what: the baseline was bumped to $(baseline)"
    [ "$(snapshot bindings.json)" = '{"sdk_version": "1.0.0"}' ] ||
        fail "$what: bindings.json was rewritten"
    [ "$(snapshot openapi-v1beta.json)" = '{"source": "old"}' ] ||
        fail "$what: openapi-v1beta.json was rewritten"
}

cat >"$work/spec.json" <<'EOF'
{"openapi": "3.0.3", "info": {"version": "v1beta"}, "paths": {},
 "components": {"schemas": {"A": {"type": "object", "properties": {"x": {"type": "string"}}}}}}
EOF
echo '{"swagger": "2.0"}' >"$work/bad-spec.json"

# A failed download (a version that does not exist): exit non-zero, and
# leave the baseline and the snapshot as they were.
new_repo
if FAKE_PIP=fail run --version 99.99.99; then
    fail "a failed download exited 0"
fi
unchanged "failed download"

# A failed dump: the same.
new_repo
if FAKE_DUMP=fail run --version 2.0.0; then
    fail "a failed dump exited 0"
fi
unchanged "failed dump"

# A spec that does not snapshot: nothing is written, the bindings included.
new_repo
if run --version 2.0.0 --spec-v1beta "$work/bad-spec.json"; then
    fail "a bad spec exited 0"
fi
unchanged "bad spec"

# Success: the snapshot is written, then the baseline bumped. A supplied
# spec records "manual" as its source unless --spec-source says otherwise,
# never the dead ai.google.dev URL.
new_repo
if ! run --version 2.0.0 --spec-v1beta "$work/spec.json" --spec-fetched 2026-11-04; then
    fail "a good refresh exited non-zero: $(cat "$work/out.log")"
fi
[ "$(baseline)" = 2.0.0 ] || fail "the baseline is $(baseline), not 2.0.0"
[ "$(snapshot bindings.json)" = '{"sdk_version": "new"}' ] || fail "bindings.json not written"
source=$("$real_python" -c 'import json, sys; d = json.load(open(sys.argv[1])); print(d["source"], d["fetched"])' \
    "$work/repo/tests/fixtures/api_surface/openapi-v1beta.json")
[ "$source" = "manual 2026-11-04" ] || fail "spec source/fetched recorded as: $source"

new_repo
run --version 2.0.0 --spec-v1beta "$work/spec.json" --spec-source https://example.com/spec.json ||
    fail "a refresh with --spec-source exited non-zero: $(cat "$work/out.log")"
grep -q '"source": "https://example.com/spec.json"' "$work/repo/tests/fixtures/api_surface/openapi-v1beta.json" ||
    fail "--spec-source was not recorded"

# --out leaves the baseline alone.
new_repo
run --version 2.0.0 --out "$work/artifact" || fail "--out exited non-zero: $(cat "$work/out.log")"
[ -s "$work/artifact/bindings.json" ] || fail "--out wrote no bindings.json"
unchanged "--out"

# --check compares a fresh dump of the baseline with the committed one.
new_repo
FAKE_DUMP_OUTPUT='{"sdk_version": "1.0.0"}' run --check ||
    fail "--check on a matching snapshot exited non-zero: $(cat "$work/out.log")"
new_repo
if FAKE_DUMP_OUTPUT='{"sdk_version": "drifted"}' run --check; then
    fail "--check on a drifted snapshot exited 0"
fi
unchanged "--check"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "refresh_api_snapshot.sh: all checks passed"
