#!/usr/bin/env bash
# Regenerates the API-surface snapshot the coverage oracle reads
# (`tests/fixtures/api_surface/`), and with a version, bumps the sweep
# baseline to it in the same step.
#
# Usage:
#   .github/scripts/refresh_api_snapshot.sh [--version X.Y.Z] [--out DIR]
#       [--spec-v1beta FILE] [--spec-v1 FILE] [--spec-fetched YYYY-MM-DD]
#       [--spec-source TEXT]
#   .github/scripts/refresh_api_snapshot.sh --check
#
# --version        Bump `.github/last-swept-sdk-version` to this google-genai
#                  release and dump its bindings. Without it, re-dumps the
#                  current baseline (after a change to the dump script).
#                  The baseline is written last, once every snapshot file
#                  is, so a failed download or dump leaves it unchanged.
# --out            Write the snapshot here instead (the sweep uses this to
#                  build an artifact without touching the checkout); the
#                  baseline file is then left alone.
# --spec-v1beta, --spec-v1
#                  A freshly downloaded OpenAPI spec to snapshot. The spec's
#                  URL is dead (see CLAUDE.md, "Sources of truth"), so this
#                  is manual: without these flags the committed spec
#                  snapshots are kept.
# --spec-fetched   The download date recorded with the spec (default today).
# --spec-source    Where the spec came from, recorded with it (default
#                  "manual"): the URL, once the spec has a public one again.
# --check          Write nothing: dump the baseline release and exit 1 if the
#                  committed bindings.json differs (CI runs this when the dump
#                  script or the snapshot changes, and the daily sweep).
#
# Needs python3 with pip and unzip. After a bump, update the
# `docs/INTERACTIONS_API_GAP.md` header to match
# (`check_gap_tracker_baseline.sh` fails until it does), then run
# `cargo nextest run -E 'test(/coverage_oracle/) | binary(request_wire_oracle) | binary(ci_coverage)'`:
# every new field, class or endpoint fails there until it is handled or
# listed with a reason.

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo=$(cd "$script_dir/../.." && pwd -P)
baseline_file="$repo/.github/last-swept-sdk-version"
committed="$repo/tests/fixtures/api_surface"
out="$committed"
version=""
spec_v1beta=""
spec_v1=""
spec_source="manual"
check=false
fetched=$(date -u +%Y-%m-%d)

while [ $# -gt 0 ]; do
    case "$1" in
        --version) version=${2:?--version needs a value}; shift 2 ;;
        --out) out=${2:?--out needs a value}; shift 2 ;;
        --spec-v1beta) spec_v1beta=${2:?--spec-v1beta needs a file}; shift 2 ;;
        --spec-v1) spec_v1=${2:?--spec-v1 needs a file}; shift 2 ;;
        --spec-fetched) fetched=${2:?--spec-fetched needs a date}; shift 2 ;;
        --spec-source) spec_source=${2:?--spec-source needs a value}; shift 2 ;;
        --check) check=true; shift ;;
        -h | --help)
            awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
            exit 0
            ;;
        *) echo "unknown argument: $1 (see --help)" >&2; exit 2 ;;
    esac
done

for tool in python3 unzip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$tool is required" >&2
        exit 1
    fi
done

if [ "$check" = true ] && { [ -n "$version" ] || [ "$out" != "$committed" ] ||
    [ -n "$spec_v1beta$spec_v1" ]; }; then
    echo "--check takes no other option: it checks the baseline's bindings.json" >&2
    exit 2
fi

bump=false
if [ -z "$version" ]; then
    version=$(tr -d '[:space:]' <"$baseline_file")
elif [ "$out" = "$committed" ]; then
    bump=true
fi
if [ -z "$version" ]; then
    echo "no version: $baseline_file is empty and --version was not given" >&2
    exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Everything is built in $work first and moved into place only once all of
# it succeeded, so a failure leaves the snapshot and the baseline as they
# were. `--only-binary=:all:` makes pip fetch the wheel (an sdist would leave
# no *.whl to unzip); `--no-deps` skips resolving google-genai's
# dependencies.
python3 -m pip download "google-genai==$version" --no-deps --only-binary=:all: -q -d "$work/wheel"
unzip -q "$work"/wheel/*.whl -d "$work/x"
python3 "$script_dir/dump_bindings_schema.py" --json "$work/x/google/genai/_gaos" >"$work/bindings.json"

if [ "$check" = true ]; then
    if ! diff -u "$committed/bindings.json" "$work/bindings.json" >"$work/bindings.diff"; then
        head -c 20000 "$work/bindings.diff"
        echo
        echo "tests/fixtures/api_surface/bindings.json does not match a fresh dump of" \
            "google-genai $version (the baseline). Run" \
            ".github/scripts/refresh_api_snapshot.sh and commit the result." >&2
        exit 1
    fi
    echo "tests/fixtures/api_surface/bindings.json matches google-genai $version."
    exit 0
fi

written=(bindings.json)
if [ -n "$spec_v1beta" ]; then
    python3 "$script_dir/snapshot_openapi.py" "$spec_v1beta" "$spec_source" "$fetched" \
        >"$work/openapi-v1beta.json"
    written+=(openapi-v1beta.json)
fi
if [ -n "$spec_v1" ]; then
    python3 "$script_dir/snapshot_openapi.py" "$spec_v1" "$spec_source" "$fetched" \
        >"$work/openapi-v1.json"
    written+=(openapi-v1.json)
fi

mkdir -p "$out"
for file in "${written[@]}"; do
    mv "$work/$file" "$out/$file"
done
echo "Wrote ${written[*]} to $out (bindings: google-genai $version)"

if [ "$bump" = true ]; then
    printf '%s\n' "$version" >"$work/last-swept-sdk-version"
    mv "$work/last-swept-sdk-version" "$baseline_file"
    echo "Baseline bumped: .github/last-swept-sdk-version = $version"
fi
