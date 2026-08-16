#!/usr/bin/env bash
#
# Compares release example binary sizes against a baseline and fails on
# per-example growth beyond a threshold.
#
# Replaces an absolute ceiling that was raised three times (10 -> 12 -> 16 MB),
# each time only after it went red on an unrelated PR. That failure mode is
# structural: an absolute tripwire fires once drift has accumulated past the
# margin, and the only remedy is to widen the margin — so it catches toolchain
# drift loudly while a genuine per-PR regression *under* the ceiling passes
# silently. A delta check inverts both halves. See #403.
#
# Usage:
#   check_example_sizes.sh measure <dir> <out.json>
#   check_example_sizes.sh compare <baseline.json> <current.json> [max_growth_pct]
#
# Sizes are toolchain- and linker-dependent, so the baseline is carried as a
# CI artifact refreshed on every main push rather than committed — a committed
# number would encode one machine's toolchain and drift against every other.

set -euo pipefail

usage() {
    echo "usage: $0 measure <dir> <out.json>" >&2
    echo "       $0 compare <baseline.json> <current.json> [max_growth_pct]" >&2
    exit 2
}

# Records `{"example": size_bytes, ...}` for every example binary in <dir>.
#
# Skips `.d` depfiles and the hash-suffixed duplicates cargo leaves alongside
# the stable names (`simple_interaction` and `simple_interaction-a1b2c3`);
# without that, a rebuild changes the key set and every entry reads as new.
measure() {
    local dir="$1" out="$2"

    if [ ! -d "$dir" ]; then
        echo "::error::examples directory not found: $dir" >&2
        exit 1
    fi

    local first=1
    {
        echo "{"
        for f in "$dir"/*; do
            [ -f "$f" ] || continue
            local name
            name=$(basename "$f")
            case "$name" in
                *.d) continue ;;
                *-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*) continue ;;
            esac
            [ -x "$f" ] || continue

            local size
            size=$(stat -c%s "$f")
            [ $first -eq 1 ] || echo ","
            first=0
            printf '  "%s": %s' "$name" "$size"
        done
        [ $first -eq 1 ] || echo
        echo "}"
    } > "$out"

    echo "Measured $(jq 'length' < "$out") example binaries -> $out"
}

# Fails when any example present in BOTH files grew by more than the
# threshold. New examples are reported but never fail: there is nothing to
# compare against, and a first appearance is not a regression.
compare() {
    local baseline="$1" current="$2" max_growth="${3:-15}"

    if [ ! -s "$baseline" ]; then
        echo "::notice::No baseline available — skipping the delta check."
        echo "A baseline is published on every push to main; the first PR after"
        echo "this job lands has nothing to compare against."
        return 0
    fi

    echo "Comparing against baseline (threshold: +${max_growth}%)"
    echo

    local report failed
    report=$(jq -r --argjson max "$max_growth" '
        . as $cur
        | input as $base
        | [ $cur | to_entries[]
            | .key as $k
            | ($base[$k] // null) as $was
            | select($was != null)
            | ((.value - $was) / $was * 100) as $pct
            | {
                name: $k,
                was: $was,
                now: .value,
                pct: ($pct * 10 | round / 10),
                over: ($pct > $max)
              }
          ]
        | sort_by(-.pct)[]
        | "\(if .over then "FAIL" else "ok  " end)\t\(.name)\t\(.was)\t\(.now)\t\(if .pct >= 0 then "+" else "" end)\(.pct)%"
    ' "$current" "$baseline")

    printf '%-6s %-34s %12s %12s %8s\n' "" "example" "baseline" "current" "delta"
    while IFS=$'\t' read -r status name was now pct; do
        [ -n "$name" ] || continue
        printf '%-6s %-34s %12s %12s %8s\n' "$status" "$name" "$was" "$now" "$pct"
    done <<< "$report"

    # New examples: reported, never fatal.
    local added
    added=$(jq -r '. as $cur | input as $base
        | [$cur | keys[] | select($base[.] == null)] | join(", ")' "$current" "$baseline")
    if [ -n "$added" ]; then
        echo
        echo "::notice::New examples (no baseline, not checked): $added"
    fi

    failed=$(grep -c '^FAIL' <<< "$report" || true)
    if [ "${failed:-0}" -gt 0 ]; then
        echo
        while IFS=$'\t' read -r status name was now pct; do
            [ "$status" = "FAIL" ] || continue
            echo "::error::$name grew $pct (${was} -> ${now} bytes), over the +${max_growth}% threshold"
        done <<< "$report"
        echo
        echo "If this growth is intended, say so in the PR — the baseline"
        echo "refreshes automatically once the change lands on main."
        return 1
    fi

    echo
    echo "All examples within +${max_growth}% of baseline."
}

case "${1:-}" in
    measure) shift; [ $# -eq 2 ] || usage; measure "$@" ;;
    compare) shift; [ $# -ge 2 ] || usage; compare "$@" ;;
    *) usage ;;
esac
