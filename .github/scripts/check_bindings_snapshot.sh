#!/usr/bin/env bash
# Check that the committed bindings snapshot matches the swept baseline.
#
# Usage: check_bindings_snapshot.sh <snapshot> <version-file> [fresh-dump]
#
# Always: the snapshot's `# google-genai <version>` header must equal the
# version in <version-file>. Bumping .github/last-swept-sdk-version without
# regenerating the snapshot fails here, offline.
#
# With [fresh-dump] (a dump_bindings_schema.py output for that same version,
# as the API surface sweep produces): the snapshot body must equal it, which
# catches a hand-edited snapshot.
set -euo pipefail

if [ $# -lt 2 ] || [ $# -gt 3 ]; then
    echo "usage: $0 <snapshot> <version-file> [fresh-dump]" >&2
    exit 2
fi
snapshot=$1 version_file=$2 fresh=${3:-}

for f in "$snapshot" "$version_file" ${fresh:+"$fresh"}; do
    if [ ! -f "$f" ]; then
        echo "not found: $f" >&2
        exit 2
    fi
done

baseline=$(tr -d '[:space:]' < "$version_file")
header=$(head -n 1 "$snapshot")
expected="# google-genai $baseline"

if [ "$header" != "$expected" ]; then
    echo "bindings snapshot is for '${header#\# google-genai }', baseline is '$baseline'." >&2
    echo "Regenerate it: .github/scripts/regen_bindings_snapshot.sh" >&2
    exit 1
fi

body=$(grep -v '^#' "$snapshot" || true)
if [ -z "$body" ]; then
    echo "bindings snapshot has a header but no schema lines: $snapshot" >&2
    exit 1
fi

if [ -n "$fresh" ] && ! diff -q <(printf '%s\n' "$body") "$fresh" > /dev/null; then
    echo "bindings snapshot body differs from a fresh dump of google-genai $baseline." >&2
    echo "Regenerate it rather than editing it: .github/scripts/regen_bindings_snapshot.sh" >&2
    exit 1
fi
