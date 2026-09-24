#!/usr/bin/env bash
# Fixture tests for dump_bindings_schema.py.
#
# Run directly: .github/scripts/tests/test_dump_bindings_schema.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
DUMP="$script_dir/../dump_bindings_schema.py"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# Writes a minimal `_gaos` tree. $2 is the body of VideoContent, $3 the
# endpoint path.
make_tree() {
    local root=$1 fields=$2 path=$3
    mkdir -p "$root/types/interactions"
    cat >"$root/types/interactions/videocontent.py" <<EOF
from .. import BaseModel
from typing import Literal, Optional, Union

Mime = Union[Literal["video/webm", "video/mp4"], str]


class VideoContent(BaseModel):
$fields
EOF
    cat >"$root/interactions.py" <<EOF
def get():
    return dict(
        method="GET",
        path="$path",
    )
EOF
}

# Base, and a release that only reorders a union, rewraps a docstring and
# renames a path parameter: none of that is API surface.
make_tree "$work/a" '    r"""A video."""
    uri: Optional[str] = None' '/{api_version}/interactions/{id}'
make_tree "$work/b" '    r"""A video,
    rewrapped."""
    uri: Union[None, str] = None' '/{api_version}/interactions/{interactionsId}'
sed -i.bak 's/"video\/webm", "video\/mp4"/"video\/mp4", "video\/webm"/' "$work/b/types/interactions/videocontent.py"

if ! diff <(python3 "$DUMP" "$work/a") <(python3 "$DUMP" "$work/b") >/dev/null; then
    fail "formatting-only changes produced a schema diff"
fi

# A new field is surface.
make_tree "$work/c" '    uri: Optional[str] = None
    name: Optional[str] = None' '/{api_version}/interactions/{id}'
added=$(diff <(python3 "$DUMP" "$work/a") <(python3 "$DUMP" "$work/c") | grep -c '^>' || true)
if [ "$added" != "1" ]; then
    fail "expected exactly one added line for a new field, got $added"
fi
# Captured first: `grep -q` exits early, and under pipefail the writer's
# broken pipe would read as a failure.
dump_c=$(python3 "$DUMP" "$work/c")
dump_a=$(python3 "$DUMP" "$work/a")
if ! grep -qx 'model types/interactions/videocontent.py:VideoContent.name: Union\[None, str\]' <<<"$dump_c"; then
    fail "new field not rendered as expected"
fi
if ! grep -qx 'endpoint GET /{}/interactions/{}' <<<"$dump_a"; then
    fail "endpoint line missing"
fi

# A missing types/ directory must fail, not print an empty (clean-looking) dump.
mkdir -p "$work/empty"
if python3 "$DUMP" "$work/empty" >/dev/null 2>&1; then
    fail "a tree without types/ exited 0"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "dump_bindings_schema.py: all checks passed"
