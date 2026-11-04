#!/usr/bin/env bash
# Fixture tests for snapshot_openapi.py.
#
# Run directly: .github/scripts/tests/test_snapshot_openapi.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SNAPSHOT="$script_dir/../snapshot_openapi.py"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

cat >"$work/spec.json" <<'EOF'
{
  "openapi": "3.0.3",
  "info": {"title": "t", "description": "long prose", "version": "v1beta"},
  "paths": {
    "/{api_version}/things": {
      "parameters": [],
      "post": {"description": "x", "x-codeSamples": [{"source": "..."}]},
      "get": {}
    }
  },
  "components": {"schemas": {
    "Thing": {
      "type": "object",
      "description": "A thing.",
      "required": ["id"],
      "properties": {
        "id": {"type": "string", "readOnly": true, "description": "The id."},
        "size": {"type": "string", "format": "int64", "example": "7"},
        "kind": {"type": "string", "enum": ["a", "b"], "x-google-enum-descriptions": ["A", "B"]}
      }
    }
  }}
}
EOF

out=$(python3 "$SNAPSHOT" "$work/spec.json" https://example.com/spec.json 2026-11-01)
checks=$(python3 -c '
import json, sys
d = json.loads(sys.stdin.read())
thing = d["schemas"]["Thing"]
ok = [
    d["source"] == "https://example.com/spec.json",
    d["fetched"] == "2026-11-01",
    d["version"] == "v1beta",
    d["endpoints"] == ["GET /{api_version}/things", "POST /{api_version}/things"],
    "description" not in thing and "description" not in thing["properties"]["id"],
    "example" not in thing["properties"]["size"],
    "x-google-enum-descriptions" not in thing["properties"]["kind"],
    thing["properties"]["id"]["readOnly"] is True,
    thing["properties"]["size"]["format"] == "int64",
    thing["required"] == ["id"],
]
print(" ".join("ok" if x else f"fail{i}" for i, x in enumerate(ok)))
' <<<"$out")
case "$checks" in
    *fail*) fail "snapshot output wrong: $checks" ;;
esac

# Not an OpenAPI 3 document: fail rather than write an empty snapshot.
echo '{"swagger": "2.0"}' >"$work/bad.json"
if python3 "$SNAPSHOT" "$work/bad.json" x 2026-11-01 >/dev/null 2>&1; then
    fail "a spec without components.schemas exited 0"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "snapshot_openapi.py: all checks passed"
