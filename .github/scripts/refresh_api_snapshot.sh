#!/usr/bin/env bash
# Regenerates the API-surface snapshot the coverage oracle reads
# (`tests/fixtures/api_surface/`), and with a version, bumps the sweep
# baseline to it in the same step.
#
# Usage:
#   .github/scripts/refresh_api_snapshot.sh [--version X.Y.Z] [--out DIR]
#       [--spec-v1beta FILE] [--spec-v1 FILE] [--spec-fetched YYYY-MM-DD]
#
# --version        Bump `.github/last-swept-sdk-version` to this google-genai
#                  release and dump its bindings. Without it, re-dumps the
#                  current baseline (after a change to the dump script).
# --out            Write the snapshot here instead (the sweep uses this to
#                  build an artifact without touching the checkout); the
#                  baseline file is then left alone.
# --spec-v1beta, --spec-v1
#                  A freshly downloaded OpenAPI spec to snapshot. The spec's
#                  URL is dead (see CLAUDE.md, "Sources of truth"), so this
#                  is manual: without these flags the committed spec
#                  snapshots are kept.
# --spec-fetched   The download date recorded with the spec (default today).
#
# Needs python3 with pip and unzip. After a bump, update the
# `docs/INTERACTIONS_API_GAP.md` header to match
# (`check_gap_tracker_baseline.sh` fails until it does), then run
# `cargo nextest run -E 'test(/coverage_oracle/)'`: every new field fails
# there until it is modeled or listed with a reason.

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo=$(cd "$script_dir/../.." && pwd -P)
baseline_file="$repo/.github/last-swept-sdk-version"
out="$repo/tests/fixtures/api_surface"
version=""
spec_v1beta=""
spec_v1=""
fetched=$(date -u +%Y-%m-%d)
spec_base="https://ai.google.dev/static/api"

while [ $# -gt 0 ]; do
    case "$1" in
        --version) version=${2:?--version needs a value}; shift 2 ;;
        --out) out=${2:?--out needs a value}; shift 2 ;;
        --spec-v1beta) spec_v1beta=${2:?--spec-v1beta needs a file}; shift 2 ;;
        --spec-v1) spec_v1=${2:?--spec-v1 needs a file}; shift 2 ;;
        --spec-fetched) fetched=${2:?--spec-fetched needs a date}; shift 2 ;;
        -h | --help) sed -n '2,27p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
    esac
done

for tool in python3 unzip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$tool is required" >&2
        exit 1
    fi
done

if [ -z "$version" ]; then
    version=$(tr -d '[:space:]' <"$baseline_file")
elif [ "$out" = "$repo/tests/fixtures/api_surface" ]; then
    echo "$version" >"$baseline_file"
    echo "Baseline bumped: .github/last-swept-sdk-version = $version"
fi
if [ -z "$version" ]; then
    echo "no version: $baseline_file is empty and --version was not given" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# `--only-binary=:all:` makes pip fetch the wheel (an sdist would leave no
# *.whl to unzip); `--no-deps` skips resolving google-genai's dependencies.
python3 -m pip download "google-genai==$version" --no-deps --only-binary=:all: -q -d "$work/wheel"
unzip -q "$work"/wheel/*.whl -d "$work/x"

mkdir -p "$out"
python3 "$script_dir/dump_bindings_schema.py" --json "$work/x/google/genai/_gaos" >"$work/bindings.json"
mv "$work/bindings.json" "$out/bindings.json"
echo "Wrote $out/bindings.json (google-genai $version)"

if [ -n "$spec_v1beta" ]; then
    python3 "$script_dir/snapshot_openapi.py" "$spec_v1beta" \
        "$spec_base/interactions.openapi.json" "$fetched" >"$out/openapi-v1beta.json"
    echo "Wrote $out/openapi-v1beta.json (fetched $fetched)"
fi
if [ -n "$spec_v1" ]; then
    python3 "$script_dir/snapshot_openapi.py" "$spec_v1" \
        "$spec_base/interactions-v1.openapi.json" "$fetched" >"$out/openapi-v1.json"
    echo "Wrote $out/openapi-v1.json (fetched $fetched)"
fi
