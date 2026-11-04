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

# --json resolves references through imports (a sibling module, a module
# alias from another package, a union alias), drops the SDK's Unknown*
# fallback arms, and reads the hand-written classes in ../types.py with
# camelCase wire names.
genai="$work/genai"
mkdir -p "$genai/_gaos/types/interactions" "$genai/_gaos/types/agents"
cat >"$genai/version.py" <<'EOF'
__version__ = '9.9.9'
EOF
cat >"$genai/_gaos/types/__init__.py" <<'EOF'
from .basemodel import BaseModel
EOF
cat >"$genai/_gaos/types/interactions/usage.py" <<'EOF'
from .. import BaseModel
from typing import Optional


class Usage(BaseModel):
    total_tokens: Optional[int] = None
EOF
cat >"$genai/_gaos/types/interactions/tool.py" <<'EOF'
from .. import BaseModel, UnrecognizedStr
from typing import Any, Literal, Optional, Union


class CodeExecution(BaseModel):
    type: Literal["code_execution"] = "code_execution"


class UnknownTool(BaseModel):
    type: Literal["UNKNOWN"] = "UNKNOWN"
    raw: Any
    is_unknown: Literal[True] = True


Mode = Union[Literal["a", "b"], UnrecognizedStr]

Tool = Union[CodeExecution, UnknownTool]
EOF
cat >"$genai/_gaos/types/agents/agent.py" <<'EOF'
from .. import BaseModel
from ..interactions import tool as interactions_tool, usage as interactions_usage
from typing import List, Optional


class Agent(BaseModel):
    tools: Optional[List[interactions_tool.Tool]] = None
    usage: Optional[interactions_usage.Usage] = None
    mode: interactions_tool.Mode
EOF
cat >"$genai/types.py" <<'EOF'
import datetime
from typing import Optional
from . import _common


class FileState(_common.CaseInSensitiveEnum):
    ACTIVE = 'ACTIVE'


class File(_common.BaseModel):
    display_name: Optional[str] = None
    create_time: Optional[datetime.datetime] = None
    state: Optional[FileState] = None
EOF
cat >"$genai/_gaos/agents.py" <<'PY'
def get():
    return dict(method="GET", path="/{api_version}/agents/{agentsId}")
PY
for name in ListFilesResponse FileSearchStore ListFileSearchStoresResponse \
    CreateFileSearchStoreConfig Document ListDocumentsResponse \
    UploadToFileSearchStoreConfig ImportFileConfig; do
    printf '\n\nclass %s(_common.BaseModel):\n    http_options: Optional[str] = None\n' "$name" >>"$genai/types.py"
done
if ! json_dump=$(python3 "$DUMP" --json "$genai/_gaos" 2>&1); then
    fail "--json failed: $json_dump"
else
    checks=$(python3 -c '
import json, sys
d = json.loads(sys.stdin.read())
c, a = d["classes"], d["aliases"]
agent = c["agents.Agent"]["fields"]
ok = [
    d["sdk_version"] == "9.9.9",
    d["endpoints"] == ["GET /{}/agents/{}"],
    agent["tools"]["type"] == {"kind": "list", "item": {"kind": "ref", "name": "interactions.Tool"}},
    a["interactions.Tool"] == {"kind": "ref", "name": "interactions.CodeExecution"},
    agent["usage"]["type"] == {"kind": "ref", "name": "interactions.Usage"},
    agent["mode"]["required"] is True,
    a["interactions.Mode"] == {"kind": "literal", "values": ["a", "b"], "open": True},
    "interactions.UnknownTool" not in c,
    set(c["legacy.File"]["fields"]) == {"displayName", "createTime", "state"},
    c["legacy.File"]["fields"]["createTime"]["type"] == {"kind": "datetime"},
    c["legacy.File"]["fields"]["state"]["type"]["values"] == ["ACTIVE"],
    c["legacy.CreateFileSearchStoreConfig"]["fields"] == {},
]
print(" ".join("ok" if x else f"fail{i}" for i, x in enumerate(ok)))
' <<<"$json_dump")
    case "$checks" in
        *fail*) fail "--json output wrong: $checks" ;;
    esac
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "dump_bindings_schema.py: all checks passed"
