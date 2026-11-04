#!/usr/bin/env python3
"""Reduce an Interactions OpenAPI spec to the snapshot the coverage oracle reads.

Usage: snapshot_openapi.py <spec.json> <source URL> <fetched YYYY-MM-DD>

Keeps every schema's structure (properties, types, formats, enums, `$ref`,
`oneOf`, `required`, `deprecated`, `readOnly`) and the endpoint list, and
drops the prose (descriptions, examples, code samples), which is most of the
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


def strip(node):
    if isinstance(node, dict):
        return {k: strip(v) for k, v in node.items() if k not in PROSE}
    if isinstance(node, list):
        return [strip(v) for v in node]
    return node


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    path, source, fetched = sys.argv[1:]
    with open(path, encoding="utf-8") as f:
        spec = json.load(f)
    schemas = spec.get("components", {}).get("schemas")
    if not schemas:
        sys.exit(f"{path}: no components.schemas: not an OpenAPI 3 spec, or restructured")
    endpoints = sorted(
        f"{method.upper()} {route}"
        for route, ops in spec.get("paths", {}).items()
        for method in ops
        if method in ("get", "post", "put", "patch", "delete")
    )
    snapshot = {
        "source": source,
        "fetched": fetched,
        "version": spec.get("info", {}).get("version"),
        "endpoints": endpoints,
        "schemas": strip(schemas),
    }
    print(json.dumps(snapshot, indent=1, sort_keys=True, ensure_ascii=False))


if __name__ == "__main__":
    main()
