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
      "parameters": [
        {"name": "api_version", "in": "path", "required": true},
        {"name": "page_size", "in": "query", "description": "From the path item."},
        {"name": "trace", "in": "header"}
      ],
      "post": {
        "description": "x", "x-codeSamples": [{"source": "..."}],
        "parameters": [{"name": "thing_id", "in": "query", "description": "An id."}],
        "requestBody": {"required": true, "content": {"application/json": {
          "schema": {"$ref": "#/components/schemas/Thing"}}}}
      },
      "get": {
        "parameters": [
          {"name": "page_size", "in": "query", "required": true},
          {"$ref": "#/components/parameters/Filter"}
        ]
      }
    },
    "/{api_version}/things/{id}": {
      "patch": {"requestBody": {"$ref": "#/components/requestBodies/Patch"}}
    }
  },
  "components": {
  "parameters": {"Filter": {"name": "filter", "in": "query", "description": "Prose."}},
  "requestBodies": {"Patch": {"content": {"application/json": {"schema": {
    "type": "object", "description": "Inline.",
    "properties": {"thing": {"$ref": "#/components/schemas/Thing", "description": "T."}}}}}}},
  "schemas": {
    "Thing": {
      "type": "object",
      "description": "A thing.",
      "required": ["id"],
      "properties": {
        "id": {"type": "string", "readOnly": true, "description": "The id."},
        "size": {"type": "string", "format": "int64", "example": "7"},
        "kind": {"type": "string", "enum": ["a", "b"], "x-google-enum-descriptions": ["A", "B"]},
        "description": {"type": "string", "description": "A field named like a prose keyword."},
        "title": {"type": "object", "properties": {"title": {"type": "string", "title": "T"}}}
      }
    },
    "title": {"type": "string", "description": "A schema named like a prose keyword."}
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
    d["endpoints"] == [
        "GET /{api_version}/things",
        "PATCH /{api_version}/things/{id}",
        "POST /{api_version}/things",
    ],
    "description" not in thing and "description" not in thing["properties"]["id"],
    "example" not in thing["properties"]["size"],
    "x-google-enum-descriptions" not in thing["properties"]["kind"],
    thing["properties"]["id"]["readOnly"] is True,
    thing["properties"]["size"]["format"] == "int64",
    thing["required"] == ["id"],
    # A field or schema whose *name* is a prose keyword is data: kept, with
    # only its own prose stripped, at any depth.
    thing["properties"].get("description") == {"type": "string"},
    thing["properties"].get("title", {}).get("properties", {}).get("title") == {"type": "string"},
    d["schemas"].get("title") == {"type": "string"},
    # Each operation keeps its query parameters, those of the path item and
    # the operation overriding one of the same name, a `$ref` followed,
    # with whether each is required; path and header parameters are left out.
    d["operations"]["POST /{api_version}/things"]["query"]
    == {"page_size": {"required": False}, "thing_id": {"required": False}},
    d["operations"]["GET /{api_version}/things"]["query"]
    == {"filter": {"required": False}, "page_size": {"required": True}},
    # And its request body by content type: a `$ref` as it is, an inline
    # schema (or one behind a `requestBodies` reference) without its prose.
    d["operations"]["POST /{api_version}/things"]["body"]
    == {"application/json": {"$ref": "#/components/schemas/Thing"}},
    "body" not in d["operations"]["GET /{api_version}/things"],
    d["operations"]["PATCH /{api_version}/things/{id}"]
    == {
        "query": {},
        "body": {"application/json": {
            "type": "object",
            "properties": {"thing": {"$ref": "#/components/schemas/Thing"}},
        }},
    },
]
print(" ".join("ok" if x else f"fail{i}" for i, x in enumerate(ok)))
' <<<"$out")
case "$checks" in
    *fail*) fail "snapshot output wrong: $checks" ;;
esac

# A reference the spec does not resolve fails, rather than dropping the
# parameter it stood for.
python3 -c '
import json, sys
spec = json.load(open(sys.argv[1]))
spec["paths"]["/{api_version}/things"]["get"]["parameters"].append(
    {"$ref": "#/components/parameters/Missing"})
json.dump(spec, open(sys.argv[2], "w"))
' "$work/spec.json" "$work/dangling.json"
if python3 "$SNAPSHOT" "$work/dangling.json" x 2026-11-01 >/dev/null 2>&1; then
    fail "a dangling parameter reference exited 0"
fi

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
