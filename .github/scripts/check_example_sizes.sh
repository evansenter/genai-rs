#!/usr/bin/env bash
#
# Compares release example binary sizes against a baseline and fails on
# per-example growth beyond a threshold (#403).
#
# Usage:
#   check_example_sizes.sh measure <dir> <out.json>
#   check_example_sizes.sh compare <baseline.json> <current.json> [max_growth_pct]
#   check_example_sizes.sh render <sizes.json>

set -euo pipefail

usage() {
    echo "usage: $0 measure <dir> <out.json>" >&2
    echo "       $0 compare <baseline.json> <current.json> [max_growth_pct]" >&2
    echo "       $0 render <sizes.json>" >&2
    exit 2
}

# True for a regular executable that is neither a `.d` depfile nor one of the
# hash-suffixed duplicates cargo leaves beside the stable name; those would
# change the key set on every rebuild. One predicate for both of `measure`'s
# loops, so they cannot drift.
is_example_binary() {
    local f="$1" name suffix
    [ -f "$f" ] || return 1
    [ -x "$f" ] || return 1
    name=$(basename "$f")
    case "$name" in
        *.d) return 1 ;;
    esac
    # A duplicate's whole tail after the last hyphen is hex, so
    # `foo-deadbeef_bar` is kept. A false positive drops the example from both
    # sides unnoticed.
    case "$name" in
        *-*)
            suffix=${name##*-}
            case "$suffix" in
                # At least 8 characters and every one of them hex.
                [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*)
                    case "$suffix" in
                        *[!0-9a-f]*) ;;
                        *) return 1 ;;
                    esac
                    ;;
            esac
            ;;
    esac
    return 0
}

# Records `{"example": size_bytes, ...}` for every example binary in <dir>.
measure() {
    local dir="$1" out="$2"

    if [ ! -d "$dir" ]; then
        echo "::error::examples directory not found: $dir" >&2
        exit 1
    fi

    # Validated in their own pass, outside the pipeline below, so `return 1`
    # returns from `measure` rather than from a subshell. A tab would shift
    # `compare`'s tab-delimited columns, and a newline would break the name/size
    # fold. `$'\t'` rather than `$(printf ...)`, which strips a trailing
    # newline.
    local f name
    for f in "$dir"/*; do
        is_example_binary "$f" || continue
        name=$(basename "$f")
        case "$name" in
            *$'\t'*)
                echo "::error::Example name contains a tab, which the size report cannot carry: $name" >&2
                return 1
                ;;
            *$'\n'*)
                echo "::error::Example name contains a newline, which the size report cannot carry: $name" >&2
                return 1
                ;;
        esac
    done

    {
        for f in "$dir"/*; do
            is_example_binary "$f" || continue
            name=$(basename "$f")

            local size
            # `wc -c`, not GNU-only `stat -c%s`, so this runs on macOS.
            size=$(wc -c < "$f")
            # Two raw lines folded by jq, which escapes names properly. `%d`:
            # BSD `wc -c` pads with spaces, which `tonumber` rejects.
            printf '%s\n%d\n' "$name" "$size"
        done
    } | jq -Rn '
        [inputs] as $lines
        | reduce range(0; ($lines | length); 2) as $i
            ({}; .[$lines[$i]] = ($lines[$i + 1] | tonumber))
    ' > "$out"

    local count
    count=$(jq 'length' < "$out")
    if [ "$count" -eq 0 ]; then
        echo "::error::Measured 0 example binaries in $1 — nothing to compare." >&2
        return 1
    fi
    echo "Measured $count example binaries -> $out"
}

# Fails when any example present in BOTH files grew by more than the
# threshold. New examples are reported but never fail: there is nothing to
# compare against, and a first appearance is not a regression.
compare() {
    local baseline="$1" current="$2" max_growth="${3:-15}"

    # A string threshold would compare false against every number and pass
    # vacuously; anything else malformed would abort jq mid-report.
    case "$max_growth" in
        '' | *[!0-9]*)
            echo "::error::threshold must be a non-negative integer, got: $max_growth" >&2
            return 1
            ;;
    esac

    # A hard error, unlike a bad baseline: the build under test is unmeasurable,
    # and skipping would be a vacuous pass.
    if [ ! -s "$current" ] || ! jq -e 'type == "object" and all(.[]; type == "number")' "$current" >/dev/null 2>&1; then
        echo "::error::Current sizes file is missing, empty or not a JSON object: $current"
        echo "The build under test is unmeasurable, so the delta check cannot run."
        return 1
    fi

    # An object of numbers, not merely non-empty: a truncated download or any
    # other shape would abort jq below and read as a size regression.
    if [ ! -s "$baseline" ] || ! jq -e 'type == "object" and all(.[]; type == "number")' "$baseline" >/dev/null 2>&1; then
        # `warning`, not `notice`: a gate that did not run must not look clean.
        # Main can go quiet past artifact retention.
        echo "::warning::No usable baseline — the delta check did NOT run."
        echo "Either none was published yet (the first PR after this job lands has"
        echo "nothing to compare against; a baseline is published on every push to"
        echo "main), or the downloaded one was not valid JSON — a truncated"
        echo "artifact reads the same way here."
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
            # > 0: jq aborts on a zero divisor. A zero baseline entry is treated
            # as new.
            | select($was != null and $was > 0)
            | ((.value - $was) / $was * 100) as $pct
            | {
                name: $k,
                was: $was,
                now: .value,
                # Rounded once, then both printed and compared. + 0 normalises
                # negative zero, which would otherwise render as +-0%.
                pct: (($pct * 10 | round / 10) + 0),
                over: ((($pct * 10 | round / 10) + 0) > $max)
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
    # `<= 0`, matching the filter above, so a zero-size baseline entry is still
    # listed here.
    added=$(jq -r '. as $cur | input as $base
        | [$cur | keys[] | select(($base[.] // 0) <= 0)] | join(", ")' "$current" "$baseline")
    if [ -n "$added" ]; then
        echo
        echo "::notice::No usable baseline, not checked (new, or zero-size in the baseline): $added"
    fi

    # Removed or renamed examples would otherwise leave the comparison silently.
    local removed
    removed=$(jq -r '. as $cur | input as $base
        | [$base | keys[] | select($cur[.] == null)] | join(", ")' "$current" "$baseline")
    if [ -n "$removed" ]; then
        echo "::notice::Examples in the baseline but not in this build: $removed"
    fi

    failed=$(grep -c '^FAIL' <<< "$report" || true)
    if [ "${failed:-0}" -gt 0 ] && [ "${SIZE_GROWTH_OK:-}" = "true" ]; then
        echo
        echo "::notice::$failed example(s) over threshold, waved through by the \`size-growth-ok\` label."
        return 0
    fi
    if [ "${failed:-0}" -gt 0 ]; then
        echo
        while IFS=$'\t' read -r status name was now pct; do
            [ "$status" = "FAIL" ] || continue
            echo "::error::$name grew $pct (${was} -> ${now} bytes), over the +${max_growth}% threshold"
        done <<< "$report"
        echo
        echo "If this growth is intended: add the \`size-growth-ok\` label, then"
        echo "push a commit. Create the label if the picker does not offer it —"
        echo "the workflow matches on name. (Re-running does not work — a re-run replays the"
        echo "original event payload, so the new label is not in it, and this"
        echo "workflow does not trigger on \`labeled\`.) The baseline refreshes"
        echo "automatically once the change lands on main, so the override is"
        echo "only needed for the PR that introduces the growth."
        return 1
    fi

    # An empty intersection compared nothing and would otherwise report clean.
    local compared
    compared=$(grep -c . <<< "$report" || true)
    if [ "${compared:-0}" -eq 0 ] && [ "${SIZE_GROWTH_OK:-}" = "true" ]; then
        echo
        echo "::notice::Nothing was comparable, waved through by the \`size-growth-ok\` label."
        return 0
    fi
    if [ "${compared:-0}" -eq 0 ]; then
        echo
        echo "::error::No examples were comparable: the baseline has $(jq 'length' < "$baseline") \
entries and the current build has $(jq 'length' < "$current"), but none of them could be \
compared — either the key sets are disjoint, or every shared key has a zero-size baseline \
entry (both are dropped by the same filter). The delta check is not running."
        echo
        echo "If the key set changed on purpose — an examples/ reorganisation or a"
        echo "mass rename — this is self-clearing: the baseline refreshes on the next"
        echo "push to main. To land the change before then, add the \`size-growth-ok\`"
        echo "label and push a commit; it waives this guard as well as the"
        echo "threshold. Create the label if the picker does not offer it — the"
        echo "workflow matches on name."
        return 1
    fi

    echo
    # Both totals, so a partial key-set divergence is visible on a green run.
    local n_current n_baseline
    n_current=$(jq 'length' < "$current")
    n_baseline=$(jq 'length' < "$baseline")
    echo "All examples within +${max_growth}% of baseline \
(${compared} compared of ${n_current} built / ${n_baseline} in baseline)."
}

# Renders the step-summary table from a size map: one decimal always, with a
# `<0.1 MB` floor.
render() {
    local sizes="$1"

    echo ""
    echo "### Example Binary Sizes"
    echo ""
    echo "| Example | Size |"
    echo "|---------|------|"
    jq -r 'to_entries | sort_by(-.value)[]
        | (.value / 1048576) as $mb
        | "| \(.key) | \(if $mb < 0.1 then "<0.1" else ($mb * 10 | round / 10 | tostring)
            | (if (contains(".") | not) then . + ".0" else . end) end) MB |"' "$sizes"
}

case "${1:-}" in
    measure) shift; [ $# -eq 2 ] || usage; measure "$@" ;;
    # Two or three: `-ge 2` would swallow a stray fourth argument.
    compare) shift; { [ $# -eq 2 ] || [ $# -eq 3 ]; } || usage; compare "$@" ;;
    render)  shift; [ $# -eq 1 ] || usage; render "$@" ;;
    *) usage ;;
esac
