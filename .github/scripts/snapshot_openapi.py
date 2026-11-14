#!/usr/bin/env python3
"""Reduce an Interactions OpenAPI spec to the snapshot the coverage oracle reads.

Usage: snapshot_openapi.py <spec.json> <source URL> <fetched YYYY-MM-DD>

Keeps what the spec says about the wire and drops only its prose
(descriptions, summaries, examples, code samples), which is most of the
file and churns on every docs edit, and, outside the schemas and
parameters, the names a generated SDK gives its code (`NOT_WIRE`). So it
keeps:
- every schema's structure (properties, types, formats, enums, `$ref`,
  `oneOf`, `required`, `deprecated`, `readOnly`, ...);
- the endpoint list, an operation under any of the eight HTTP methods
  OpenAPI names, and each endpoint's operation:
  - its path, query and header parameters (`path`, `query`, `header`, and
    `cookie` if it has one), each with whether it is required, its schema
    (type, array items, format, default, enum, const) and any other
    non-prose key it has (`style`, `explode`, ...);
  - its request body (`body`): whether it is required, its schema by
    content type (`content`, a `$ref`, or the schema without its prose),
    and a content type's other keys (`media`: its `encoding`, ...);
  - its responses by status (`responses`), each with its schema by
    content type (`content`), a content type's other keys (`media`: the
    `x-speakeasy-sse-sentinel` of an event stream, ...) and its headers
    (`headers`), when it has any;
  - every other key it has, or its path item has: `deprecated`,
    `servers`, `security`, `callbacks`, `x-speakeasy-retries`, ...;
- the spec's top-level keys but `paths` and `components` (`top_level`):
  its `servers`, `security`, `x-speakeasy-retries` (the reference SDK's
  retry policy), `x-speakeasy-globals` (the parameters every request may
  carry), and `info` without its prose;
- the security schemes (`security_schemes`), which a `security`
  requirement names.
Another component is kept where something kept names it: a parameter,
body, response or header inline where it is used, any other `$ref` but a
schema's inline too. Output is sorted and indented so a refreshed copy
diffs line by line against the committed one in `tests/fixtures/api_surface/`.
See `.github/scripts/refresh_api_snapshot.sh`.
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


# Every method an OpenAPI path item can hold, so an operation under one the
# crate never calls (a HEAD or an OPTIONS) is an endpoint like any other.
METHODS = ("get", "put", "post", "delete", "options", "head", "patch", "trace")

# Keys that name generated code or point at prose, not the wire: an
# operation's `operationId`, `tags`, `summary` and `externalDocs`, the
# Speakeasy SDK's names for it (`x-speakeasy-group`,
# `x-speakeasy-name-override`, `x-speakeasy-exports`), and its
# `x-speakeasy-sse-overload`, which overloads a method on a response the
# snapshot keeps (`text/event-stream`). Dropped from an operation, its path
# item, request body, responses and content types, and the spec's top
# level; a schema or a parameter keeps every key but its prose, these
# included.
NOT_WIRE = {
    "externalDocs",
    "operationId",
    "summary",
    "tags",
    "x-speakeasy-exports",
    "x-speakeasy-group",
    "x-speakeasy-name-override",
    "x-speakeasy-sse-overload",
}

# The keys of an operation, request body and response this script reduces
# itself; the rest are kept as they are, without prose (`rest`).
OPERATION_KEYS = {"parameters", "requestBody", "responses"}


def inline(spec, node, depth=0):
    """A node with every `$ref` into the components but a schema's replaced
    by what it names, so the snapshot keeps it without the component."""
    if depth > 64:
        sys.exit("a reference cycle outside the schemas")
    if isinstance(node, dict):
        ref = node.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/components/"):
            kind = ref[len("#/components/"):].split("/", 1)[0]
            if kind != "schemas":
                return inline(spec, resolve(spec, node, kind), depth + 1)
        return {k: inline(spec, v, depth + 1) for k, v in node.items()}
    if isinstance(node, list):
        return [inline(spec, v, depth + 1) for v in node]
    return node


def rest(spec, node, handled):
    """A node's keys but those in `handled`, prose and `NOT_WIRE`, each
    inlined and without its prose."""
    return {
        k: strip(inline(spec, v))
        for k, v in (node or {}).items()
        if k not in handled and k not in PROSE and k not in NOT_WIRE
    }


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
# A parameter elsewhere (`cookie`) gets a map of its own.
LOCATIONS = ("path", "query", "header")


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


def media(spec, media_types):
    """A content map's other keys by content type (`encoding`, ...), for
    the content types that have any."""
    kept = {
        content_type: rest(spec, media_type, {"schema"})
        for content_type, media_type in (media_types or {}).items()
    }
    return {content_type: keys for content_type, keys in kept.items() if keys}


def operation(spec, path_item, op):
    """An operation's parameters by location (name -> parameter), its
    request body (required flag and content type -> schema), its responses
    (status -> content and headers) and its other keys. An operation's own
    parameter replaces the path item's of the same name and location, and
    its own key the path item's (`servers`, ...)."""
    params = {}
    for source in (path_item.get("parameters", []), op.get("parameters", [])):
        for param in source:
            param = resolve(spec, param, "parameters")
            params[(param.get("in"), param.get("name"))] = param
    out = {location: {} for location in LOCATIONS}
    for (location, name), param in params.items():
        out.setdefault(location, {})[name] = parameter(param)
    body = resolve(spec, op.get("requestBody"), "requestBodies")
    if body:
        out["body"] = {
            "required": bool(body.get("required", False)),
            "content": content(body.get("content")),
        }
        body_media = media(spec, body.get("content"))
        if body_media:
            out["body"]["media"] = body_media
        out["body"].update(rest(spec, body, {"required", "content"}))
    responses = {}
    for status, response in op.get("responses", {}).items():
        response = resolve(spec, response, "responses")
        entry = {"content": content(response.get("content"))}
        response_media = media(spec, response.get("content"))
        if response_media:
            entry["media"] = response_media
        headers = {
            name: strip(resolve(spec, header, "headers"))
            for name, header in response.get("headers", {}).items()
        }
        if headers:
            entry["headers"] = headers
        entry.update(rest(spec, response, {"content", "headers"}))
        responses[str(status)] = entry
    out["responses"] = responses
    others = rest(spec, path_item, set(METHODS) | {"parameters"})
    others.update(rest(spec, op, OPERATION_KEYS))
    for key, value in others.items():
        if key in out:
            sys.exit(f"an operation key {key!r} would replace the snapshot's own")
        out[key] = value
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
        "top_level": rest(spec, spec, {"paths", "components"}),
        "security_schemes": strip_names(spec["components"].get("securitySchemes", {})),
    }
    print(json.dumps(snapshot, indent=1, sort_keys=True, ensure_ascii=False))


if __name__ == "__main__":
    main()
