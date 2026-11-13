#!/usr/bin/env python3
"""Reduce an Interactions OpenAPI spec to the snapshot the coverage oracle reads.

Usage: snapshot_openapi.py <spec.json> <source URL> <fetched YYYY-MM-DD>

Keeps every schema's structure (properties, types, formats, enums, `$ref`,
`oneOf`, `required`, `deprecated`, `readOnly`), the endpoint list, and each
endpoint's operation:
- its query and header parameters (`query`, `header`, and `cookie` if it
  has one), each with whether it is required, its schema (type, array
  items, format, default, enum, const) and any other non-prose key it has
  (`style`, `explode`, ...); a path parameter is in the endpoint's path, so
  it is not kept again;
- its request body's schema by content type (`body`, a `$ref`, or the
  schema without its prose);
- its responses by status (`responses`), each with its schema by content
  type (`content`) and, when it has any, its headers (`headers`).
Drops the prose (descriptions, examples, code samples), which is most of the
file and churns on every docs edit. Output is sorted and indented so a
refreshed copy diffs line by line against the committed one in
`tests/fixtures/api_surface/`. See `.github/scripts/refresh_api_snapshot.sh`.
"""

import json
import sys

PROSE = {
    "description",
    "example",
    "examples",
    "title",
    "x-codeSamples",
    "x-google-enum-descriptions",
    "x-speakeasy-enum-descriptions",
}


# Keys whose value maps names to schemas. A name there is data, not a schema
# keyword: a field called `description` or `title` (Function.description,
# UrlCitation.title) is kept, and only its schema is stripped.
NAME_MAPS = {"properties", "patternProperties", "$defs", "definitions"}


def strip(node):
    """A schema without its prose keywords."""
    if isinstance(node, dict):
        return {
            k: strip_names(v) if k in NAME_MAPS else strip(v)
            for k, v in node.items()
            if k not in PROSE
        }
    if isinstance(node, list):
        return [strip(v) for v in node]
    return node


def strip_names(node):
    """A name -> schema map with every name kept."""
    if isinstance(node, dict):
        return {name: strip(schema) for name, schema in node.items()}
    return strip(node)


METHODS = ("get", "post", "put", "patch", "delete")


def resolve(spec, node, kind):
    """A `$ref` to `#/components/<kind>/<name>` followed to what it names."""
    ref = node.get("$ref") if isinstance(node, dict) else None
    if ref is None:
        return node
    prefix = f"#/components/{kind}/"
    if not ref.startswith(prefix):
        sys.exit(f"unsupported reference {ref!r}: expected one under {prefix}")
    try:
        return spec["components"][kind][ref[len(prefix):]]
    except KeyError:
        sys.exit(f"dangling reference {ref!r}")


# Parameter locations every operation has a map for, empty when it has none.
# A parameter elsewhere (`cookie`) gets a map of its own; a path parameter
# none, since the endpoint's path names it.
LOCATIONS = ("query", "header")


def parameter(param):
    """A parameter without its prose, name and location, its required flag
    made explicit."""
    kept = {k: v for k, v in strip(param).items() if k not in ("name", "in")}
    kept["required"] = bool(param.get("required", False))
    return kept


def content(media_types):
    """A content map's schema by content type, each without its prose."""
    return {
        content_type: strip(media.get("schema", {}))
        for content_type, media in (media_types or {}).items()
    }


def operation(spec, path_item, op):
    """An operation's query and header parameters (name -> parameter), its
    request body (content type -> schema) and its responses (status ->
    content and headers). An operation's own parameter replaces the path
    item's of the same name and location."""
    params = {}
    for source in (path_item.get("parameters", []), op.get("parameters", [])):
        for param in source:
            param = resolve(spec, param, "parameters")
            params[(param.get("in"), param.get("name"))] = param
    out = {location: {} for location in LOCATIONS}
    for (location, name), param in params.items():
        if location != "path":
            out.setdefault(location, {})[name] = parameter(param)
    body = resolve(spec, op.get("requestBody"), "requestBodies")
    if body:
        out["body"] = content(body.get("content"))
    responses = {}
    for status, response in op.get("responses", {}).items():
        response = resolve(spec, response, "responses")
        entry = {"content": content(response.get("content"))}
        headers = {
            name: strip(resolve(spec, header, "headers"))
            for name, header in response.get("headers", {}).items()
        }
        if headers:
            entry["headers"] = headers
        responses[str(status)] = entry
    out["responses"] = responses
    return out


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    path, source, fetched = sys.argv[1:]
    with open(path, encoding="utf-8") as f:
        spec = json.load(f)
    schemas = spec.get("components", {}).get("schemas")
    if not schemas:
        sys.exit(f"{path}: no components.schemas: not an OpenAPI 3 spec, or restructured")
    operations = {
        f"{method.upper()} {route}": operation(spec, ops, ops[method])
        for route, ops in spec.get("paths", {}).items()
        for method in ops
        if method in METHODS
    }
    snapshot = {
        "source": source,
        "fetched": fetched,
        "version": spec.get("info", {}).get("version"),
        "endpoints": sorted(operations),
        "operations": operations,
        "schemas": strip_names(schemas),
    }
    print(json.dumps(snapshot, indent=1, sort_keys=True, ensure_ascii=False))


if __name__ == "__main__":
    main()
