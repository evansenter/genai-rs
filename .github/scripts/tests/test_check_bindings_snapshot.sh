#!/usr/bin/env bash
# Fixture tests for check_bindings_snapshot.sh, plus the committed snapshot
# against the committed baseline.
#
# Run directly: .github/scripts/tests/test_check_bindings_snapshot.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../../.." && pwd)
CHECK="$script_dir/../check_bindings_snapshot.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

expect_rc() {
    local label=$1 want=$2
    shift 2
    local rc=0
    "$CHECK" "$@" >/dev/null 2>&1 || rc=$?
    if [ "$rc" -ne "$want" ]; then
        fail "$label: expected exit $want, got $rc"
    fi
}

echo "2.34.0" > "$work/version"
printf '%s\n' '# google-genai 2.34.0' '# Generated; do not edit.' \
    'model a.py:A.x: str' 'endpoint GET /{}/interactions/{}' > "$work/snapshot"
printf '%s\n' 'model a.py:A.x: str' 'endpoint GET /{}/interactions/{}' > "$work/fresh"

expect_rc "matching header" 0 "$work/snapshot" "$work/version"
expect_rc "matching header and body" 0 "$work/snapshot" "$work/version" "$work/fresh"

# Baseline bumped without regenerating: the case this check exists for.
echo "2.35.0" > "$work/version-bumped"
expect_rc "stale header" 1 "$work/snapshot" "$work/version-bumped"

# Prefix versions must not pass (2.34.0 vs 2.34.01).
echo "2.34.01" > "$work/version-prefix"
expect_rc "prefix version" 1 "$work/snapshot" "$work/version-prefix"

# Hand-edited body.
printf '%s\n' 'model a.py:A.x: int' 'endpoint GET /{}/interactions/{}' > "$work/fresh-diff"
expect_rc "edited body" 1 "$work/snapshot" "$work/version" "$work/fresh-diff"

# Header but no schema lines.
printf '%s\n' '# google-genai 2.34.0' > "$work/empty"
expect_rc "empty body" 1 "$work/empty" "$work/version"

# Missing files and bad usage are errors, not passes.
expect_rc "missing snapshot" 2 "$work/nope" "$work/version"
expect_rc "missing fresh dump" 2 "$work/snapshot" "$work/version" "$work/nope"
expect_rc "no arguments" 2

# The committed snapshot agrees with the committed baseline.
expect_rc "committed snapshot" 0 \
    "$repo_root/.github/bindings-schema.txt" "$repo_root/.github/last-swept-sdk-version"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "check_bindings_snapshot: all cases pass"
