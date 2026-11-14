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
  "info": {"title": "t", "description": "long prose", "version": "v1beta", "x-google-revision": "0"},
  "servers": [{"url": "https://api.example.com", "description": "Prose."}],
  "security": [{"apiKey": []}],
  "tags": [{"name": "things", "description": "Prose."}],
  "x-speakeasy-retries": {"strategy": "backoff", "statusCodes": [408, 429, "5XX"]},
  "x-speakeasy-globals": {"parameters": [{"$ref": "#/components/parameters/UserProject"}]},
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
          "schema": {"$ref": "#/components/schemas/Thing"}}}},
        "responses": {"default": {"description": "Created.", "content": {"application/json": {
          "schema": {"$ref": "#/components/schemas/Thing"}}}}}
      },
      "get": {
        "operationId": "listThings", "tags": ["things"], "summary": "Listing things",
        "x-speakeasy-group": "things", "x-speakeasy-name-override": "list",
        "x-speakeasy-sse-overload": true, "deprecated": true,
        "parameters": [
          {"name": "page_size", "in": "query", "required": true,
           "schema": {"type": "integer", "format": "int32", "description": "Prose."}},
          {"$ref": "#/components/parameters/Filter"},
          {"name": "kind", "in": "query", "explode": true,
           "schema": {"type": "array", "items": {"type": "string"}}},
          {"name": "session", "in": "cookie"}
        ],
        "responses": {
          "200": {"description": "OK.", "content": {
            "application/json": {"schema": {"$ref": "#/components/schemas/Thing"},
                                 "examples": {"one": {"value": {"id": "x"}}}},
            "text/event-stream": {"schema": {"$ref": "#/components/schemas/title"},
                                  "x-speakeasy-sse-sentinel": "[DONE]"}}},
          "4XX": {"$ref": "#/components/responses/Error"}
        }
      }
    },
    "/{api_version}/things/{id}": {
      "servers": [{"url": "https://upload.example.com", "description": "Prose."}],
      "parameters": [
        {"$ref": "#/components/parameters/ApiVersion"},
        {"name": "id", "in": "path", "required": true, "description": "Prose.",
         "schema": {"type": "string"}}
      ],
      "patch": {"requestBody": {"$ref": "#/components/requestBodies/Patch"}},
      "put": {
        "servers": [{"url": "https://put.example.com"}],
        "parameters": [{"$ref": "#/components/parameters/Command"}],
        "responses": {"200": {"description": "Started.",
          "headers": {"X-Upload-URL": {"$ref": "#/components/headers/UploadUrl"}}}}
      }
    }
  },
  "components": {
  "parameters": {
    "ApiVersion": {"name": "api_version", "in": "path", "required": true, "description": "Prose.",
                   "schema": {"type": "string"}},
    "Filter": {"name": "filter", "in": "query", "description": "Prose.", "schema": {"type": "string"}},
    "Command": {"name": "X-Upload-Command", "in": "header", "required": true, "description": "Prose.",
                "schema": {"type": "string", "const": "start", "default": "start"}},
    "UserProject": {"name": "x-goog-user-project", "in": "header", "description": "Prose.",
                    "schema": {"type": "string"}}
  },
  "securitySchemes": {"apiKey": {"type": "apiKey", "in": "header", "name": "x-goog-api-key",
                                 "description": "Prose."}},
  "headers": {"UploadUrl": {"description": "Prose.", "schema": {"type": "string", "format": "uri"}}},
  "responses": {"Error": {"description": "Prose.", "content": {"application/json": {"schema": {
    "type": "object", "properties": {"error": {"type": "string", "description": "E."}}}}}}},
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
    "Other": {"type": "object"},
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
        "PUT /{api_version}/things/{id}",
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
    # with whether each is required and its schema and other keys without
    # their prose.
    d["operations"]["POST /{api_version}/things"]["query"]
    == {"page_size": {"required": False}, "thing_id": {"required": False}},
    d["operations"]["GET /{api_version}/things"]["query"]
    == {
        "filter": {"required": False, "schema": {"type": "string"}},
        "kind": {
            "explode": True,
            "required": False,
            "schema": {"items": {"type": "string"}, "type": "array"},
        },
        "page_size": {"required": True, "schema": {"format": "int32", "type": "integer"}},
    },
    # Path and header parameters the same way, those of the path item
    # included; one in another location gets a map of its own.
    d["operations"]["GET /{api_version}/things"]["path"]
    == {"api_version": {"required": True}},
    d["operations"]["PUT /{api_version}/things/{id}"]["path"]
    == {
        "api_version": {"required": True, "schema": {"type": "string"}},
        "id": {"required": True, "schema": {"type": "string"}},
    },
    d["operations"]["POST /{api_version}/things"]["header"] == {"trace": {"required": False}},
    d["operations"]["PUT /{api_version}/things/{id}"]["header"]
    == {"X-Upload-Command": {
        "required": True, "schema": {"const": "start", "default": "start", "type": "string"}}},
    d["operations"]["GET /{api_version}/things"]["cookie"] == {"session": {"required": False}},
    "cookie" not in d["operations"]["POST /{api_version}/things"],
    # And its request body: whether it is required, and its schema by
    # content type, a `$ref` as it is, an inline schema (or one behind a
    # `requestBodies` reference) without its prose.
    d["operations"]["POST /{api_version}/things"]["body"]
    == {"required": True, "content": {"application/json": {"$ref": "#/components/schemas/Thing"}}},
    "body" not in d["operations"]["GET /{api_version}/things"],
    d["operations"]["PATCH /{api_version}/things/{id}"]
    == {
        "path": {
            "api_version": {"required": True, "schema": {"type": "string"}},
            "id": {"required": True, "schema": {"type": "string"}},
        },
        "query": {},
        "header": {},
        "body": {"required": False, "content": {"application/json": {
            "type": "object",
            "properties": {"thing": {"$ref": "#/components/schemas/Thing"}},
        }}},
        "responses": {},
        "servers": [{"url": "https://upload.example.com"}],
    },
    # And its responses by status: the schema of each content type without its
    # prose or examples (a `responses` reference followed), the other keys
    # of a content type, and the headers of the response when it has any.
    d["operations"]["GET /{api_version}/things"]["responses"]
    == {
        "200": {
            "content": {
                "application/json": {"$ref": "#/components/schemas/Thing"},
                "text/event-stream": {"$ref": "#/components/schemas/title"},
            },
            "media": {"text/event-stream": {"x-speakeasy-sse-sentinel": "[DONE]"}},
        },
        "4XX": {"content": {"application/json": {
            "type": "object", "properties": {"error": {"type": "string"}}}}},
    },
    d["operations"]["POST /{api_version}/things"]["responses"]
    == {"default": {"content": {"application/json": {"$ref": "#/components/schemas/Thing"}}}},
    d["operations"]["PUT /{api_version}/things/{id}"]["responses"]
    == {"200": {"content": {}, "headers": {
        "X-Upload-URL": {"schema": {"format": "uri", "type": "string"}}}}},
    # And every other key of the operation or its path item (the one on
    # the operation wins), but the prose and the names generated code
    # takes (operationId, tags, summary, the Speakeasy names and its SSE
    # overload).
    {k: v for k, v in d["operations"]["GET /{api_version}/things"].items()
     if k not in ("path", "query", "header", "cookie", "responses")}
    == {"deprecated": True},
    "description" not in d["operations"]["POST /{api_version}/things"],
    "x-codeSamples" not in d["operations"]["POST /{api_version}/things"],
    d["operations"]["PUT /{api_version}/things/{id}"].get("servers") == [{"url": "https://put.example.com"}],
    # The top level of the spec but its paths and components, without
    # prose or tags, a reference inlined; and the security schemes.
    d.get("top_level")
    == {
        "info": {"version": "v1beta", "x-google-revision": "0"},
        "openapi": "3.0.3",
        "security": [{"apiKey": []}],
        "servers": [{"url": "https://api.example.com"}],
        "x-speakeasy-globals": {"parameters": [
            {"in": "header", "name": "x-goog-user-project", "schema": {"type": "string"}}]},
        "x-speakeasy-retries": {"statusCodes": [408, 429, "5XX"], "strategy": "backoff"},
    },
    d.get("security_schemes") == {"apiKey": {"in": "header", "name": "x-goog-api-key", "type": "apiKey"}},
]
print(" ".join("ok" if x else f"fail{i}" for i, x in enumerate(ok)))
' <<<"$out")
case "$checks" in
    *fail*) fail "snapshot output wrong: $checks" ;;
esac

# A drift in what the snapshot keeps changes the reduced output (#504 J-1):
# the first three reduced to the same bytes before header parameters,
# parameter schemas and responses were kept, and the last three before path
# parameters and a request body's required flag were, so the daily watch
# stayed green. Each mutation is a Python statement on `spec`.
mutate() {
    python3 -c '
import json, sys
spec = json.load(open(sys.argv[1]))
exec(sys.argv[3])
json.dump(spec, open(sys.argv[2], "w"))
' "$work/spec.json" "$work/mutated.json" "$1"
    python3 "$SNAPSHOT" "$work/mutated.json" https://example.com/spec.json 2026-11-01
}
expect_drift() {
    local what=$1 statement=$2 check=$3 mutated
    mutated=$(mutate "$statement")
    if [ "$mutated" = "$out" ]; then
        fail "$what: the reduced snapshot did not change"
    elif ! python3 -c "import json, sys; d = json.loads(sys.stdin.read()); sys.exit(0 if ($check) else 1)" \
        <<<"$mutated"; then
        fail "$what: the reduced snapshot changed, but not as expected"
    fi
}
expect_drift "a required header added to an operation" \
    'spec["paths"]["/{api_version}/things"]["post"]["parameters"].append({"name": "X-Goog-Api-Revision", "in": "header", "required": True, "schema": {"type": "string"}})' \
    'd["operations"]["POST /{api_version}/things"]["header"]["X-Goog-Api-Revision"] == {"required": True, "schema": {"type": "string"}}'
expect_drift "a query parameter retyped from an array of strings to a string enum" \
    'next(p for p in spec["paths"]["/{api_version}/things"]["get"]["parameters"] if p.get("name") == "kind")["schema"] = {"type": "string", "enum": ["a", "b"]}' \
    'd["operations"]["GET /{api_version}/things"]["query"]["kind"]["schema"] == {"enum": ["a", "b"], "type": "string"}'
expect_drift "a response pointed at another schema" \
    'spec["paths"]["/{api_version}/things"]["get"]["responses"]["200"]["content"]["application/json"]["schema"] = {"$ref": "#/components/schemas/Other"}' \
    'd["operations"]["GET /{api_version}/things"]["responses"]["200"]["content"]["application/json"] == {"$ref": "#/components/schemas/Other"}'
expect_drift "a header parameter made optional" \
    'spec["components"]["parameters"]["Command"]["required"] = False' \
    'd["operations"]["PUT /{api_version}/things/{id}"]["header"]["X-Upload-Command"]["required"] is False'
expect_drift "a response header removed" \
    'spec["paths"]["/{api_version}/things/{id}"]["put"]["responses"]["200"].pop("headers")' \
    'd["operations"]["PUT /{api_version}/things/{id}"]["responses"]["200"] == {"content": {}}'
expect_drift "a path parameter retyped and made optional" \
    'spec["paths"]["/{api_version}/things/{id}"]["parameters"][1].update(required=False, schema={"type": "integer", "format": "int64"})' \
    'd["operations"]["PATCH /{api_version}/things/{id}"]["path"]["id"] == {"required": False, "schema": {"format": "int64", "type": "integer"}}'
expect_drift "a path parameter given an enum behind its reference" \
    'spec["components"]["parameters"]["ApiVersion"]["schema"] = {"type": "string", "enum": ["v1alpha"]}' \
    'd["operations"]["PUT /{api_version}/things/{id}"]["path"]["api_version"]["schema"] == {"enum": ["v1alpha"], "type": "string"}'
expect_drift "a request body made optional" \
    'spec["paths"]["/{api_version}/things"]["post"]["requestBody"]["required"] = False' \
    'd["operations"]["POST /{api_version}/things"]["body"]["required"] is False'

# The rest reduced to the same bytes until #504's K-12: an operation under
# a method outside the five the crate calls, an operation's `deprecated`,
# servers, security and callbacks, a body's encoding, and the spec's
# servers, security schemes, global parameters and retry policy.
expect_drift "a HEAD operation added" \
    'spec["paths"]["/{api_version}/things/{id}"]["head"] = {"responses": {"200": {"description": "Exists."}}}' \
    '"HEAD /{api_version}/things/{id}" in d["endpoints"] and d["operations"]["HEAD /{api_version}/things/{id}"]["responses"] == {"200": {"content": {}}}'
expect_drift "an OPTIONS and a TRACE operation added" \
    'spec["paths"]["/{api_version}/things"].update(options={"responses": {}}, trace={"responses": {}})' \
    '{"OPTIONS /{api_version}/things", "TRACE /{api_version}/things"} <= set(d["endpoints"])'
expect_drift "an operation deprecated" \
    'spec["paths"]["/{api_version}/things"]["post"]["deprecated"] = True' \
    'd["operations"]["POST /{api_version}/things"]["deprecated"] is True'
expect_drift "an operation given its own servers" \
    'spec["paths"]["/{api_version}/things"]["post"]["servers"] = [{"url": "https://regional.example.com"}]' \
    'd["operations"]["POST /{api_version}/things"]["servers"] == [{"url": "https://regional.example.com"}]'
expect_drift "an operation that needs no credentials" \
    'spec["paths"]["/{api_version}/things"]["post"]["security"] = []' \
    'd["operations"]["POST /{api_version}/things"]["security"] == []'
expect_drift "a callback added" \
    'spec["paths"]["/{api_version}/things"]["post"]["callbacks"] = {"done": {"{$request.body#/uri}": {"post": {"description": "Prose.", "responses": {}}}}}' \
    'd["operations"]["POST /{api_version}/things"]["callbacks"] == {"done": {"{$request.body#/uri}": {"post": {"responses": {}}}}}'
expect_drift "a request body given an encoding" \
    'spec["paths"]["/{api_version}/things"]["post"]["requestBody"]["content"]["application/json"]["encoding"] = {"file": {"contentType": "image/png"}}' \
    'd["operations"]["POST /{api_version}/things"]["body"]["media"] == {"application/json": {"encoding": {"file": {"contentType": "image/png"}}}}'
expect_drift "the server moved" \
    'spec["servers"][0]["url"] = "https://eu.example.com"' \
    'd["top_level"]["servers"] == [{"url": "https://eu.example.com"}]'
expect_drift "a security scheme changed" \
    'spec["components"]["securitySchemes"]["apiKey"]["in"] = "query"' \
    'd["security_schemes"]["apiKey"]["in"] == "query"'
expect_drift "a global parameter made required" \
    'spec["components"]["parameters"]["UserProject"]["required"] = True' \
    'd["top_level"]["x-speakeasy-globals"]["parameters"][0]["required"] is True'
expect_drift "the reference SDK's retry policy changed" \
    'spec["x-speakeasy-retries"]["statusCodes"].append(501)' \
    'd["top_level"]["x-speakeasy-retries"]["statusCodes"] == [408, 429, "5XX", 501]'

# What names generated code or is prose does not reach the snapshot, so
# a rename churns nothing.
for statement in \
    'spec["paths"]["/{api_version}/things"]["get"].update(operationId="getThings", tags=["other"], summary="Other prose")' \
    'spec["paths"]["/{api_version}/things"]["get"].update({"x-speakeasy-group": "other", "x-speakeasy-name-override": "fetch", "x-speakeasy-sse-overload": False})' \
    'spec["paths"]["/{api_version}/things/{id}"]["servers"][0]["description"] = "Other prose."' \
    'spec["tags"].append({"name": "more"})'; do
    if [ "$(mutate "$statement")" != "$out" ]; then
        fail "a change to prose or generated names changed the snapshot: $statement"
    fi
done

# A reference the spec does not resolve fails, rather than dropping what it
# stood for: a parameter, a response or a response header.
for statement in \
    'spec["paths"]["/{api_version}/things"]["get"]["parameters"].append({"$ref": "#/components/parameters/Missing"})' \
    'spec["paths"]["/{api_version}/things"]["get"]["responses"]["4XX"] = {"$ref": "#/components/responses/Missing"}' \
    'spec["paths"]["/{api_version}/things/{id}"]["put"]["responses"]["200"]["headers"]["X-Upload-URL"] = {"$ref": "#/components/headers/Missing"}'; do
    if mutate "$statement" >/dev/null 2>&1; then
        fail "a dangling reference exited 0: $statement"
    fi
done

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
