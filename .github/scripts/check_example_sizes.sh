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
    echo "       $0 render <sizes.json>" >&2
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

    {
        for f in "$dir"/*; do
            [ -f "$f" ] || continue
            local name
            name=$(basename "$f")
            case "$name" in
                *.d) continue ;;
            esac
            # Cargo's rebuild duplicates end in `-<hex>`, and the whole tail
            # after the final hyphen is hex — so that is what is required
            # here, rather than "starts with 8 hex characters" as a bare glob
            # would. `foo-deadbeef_bar` is a plausible example name and used
            # to be discarded as a duplicate.
            #
            # A false positive matters more than it looks: the example
            # vanishes from *both* sides, so it shows up in neither the
            # comparison table, nor `added`, nor `removed`. The only trace is
            # the compared count — the one signal the removed/renamed notice
            # below cannot reach.
            case "$name" in
                *-*)
                    local suffix=${name##*-}
                    case "$suffix" in
                        # At least 8 characters and every one of them hex.
                        [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]*)
                            case "$suffix" in
                                *[!0-9a-f]*) ;;
                                *) continue ;;
                            esac
                            ;;
                    esac
                    ;;
            esac
            [ -x "$f" ] || continue

            local size
            # `wc -c`, not `stat -c%s`: the latter is GNU-only, and `measure` is the
            # one subcommand that would then not run on a macOS checkout — against
            # the whole reason this was extracted into a script.
            size=$(wc -c < "$f")
            # Name and size as two raw lines, folded into an object by jq
            # below. The previous version built the JSON by hand with
            # `printf`, which did no escaping — a name containing a quote or a
            # backslash emitted a malformed map, and `compare` then reported
            # it as "the build under test is unmeasurable", a confusing
            # diagnosis for what is really a naming problem. Not reachable
            # from `examples/*.rs` stems, but this script already carries the
            # same class of insurance for the threshold and both input shapes.
            #
            # `%d` rather than `%s` for the size: BSD `wc -c` pads with
            # leading spaces, which `tonumber` would reject.
            #
            # The one assumption left is that a name contains no newline,
            # which would desynchronise the pairing. Cargo target names cannot.
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

    # `max_growth` reaches jq through `--argjson`, where the two ways it can
    # be wrong differ in exactly the way this script cares about. A malformed
    # value (`15%`, empty) makes jq refuse the argument and exit 2, taking the
    # `report=` assignment down under `set -e` — a raw jq error reading as a
    # size regression. A well-formed non-number is worse: `"15"` binds a
    # string, jq orders every number before every string, so `> $max` is false
    # for every example and the job prints "All examples within +"15"% of
    # baseline" and goes green. That is a gate reporting a clean run while
    # comparing nothing — the disjoint-key failure arriving through a
    # different door. Not reachable from the workflow, which passes a literal
    # 15; drift insurance, like the object check below.
    case "$max_growth" in
        '' | *[!0-9]*)
            echo "::error::threshold must be a non-negative integer, got: $max_growth" >&2
            return 1
            ;;
    esac

    # The current file is deliberately NOT given the warn-and-skip treatment
    # the baseline gets below, and the asymmetry is the point: a bad baseline
    # is benign — there is nothing to compare against, so skipping is honest —
    # while a bad current file means the build under test is unmeasurable, and
    # skipping that is the vacuous pass the disjoint guard exists to reject.
    # So this is a hard error. `measure` writes `current-sizes.json` in the
    # preceding step and fails the job on an empty measurement, so it cannot
    # reach here in CI; this covers running `compare` by hand, or a future
    # caller that fetches the current side rather than measuring it.
    if [ ! -s "$current" ] || ! jq -e 'type == "object" and all(.[]; type == "number")' "$current" >/dev/null 2>&1; then
        echo "::error::Current sizes file is missing, empty or not a JSON object: $current"
        echo "The build under test is unmeasurable, so the delta check cannot run."
        return 1
    fi

    # Shape, not just parseability or emptiness. The `-s` test catches the empty file
    # the fetch step writes when no baseline exists; a *truncated* one — `gh
    # run download` interrupted mid-transfer — is non-empty and non-JSON, and
    # jq would abort on it, taking the report assignment down under `set -e`
    # and exiting the step non-zero on a raw parse error. That is a broken
    # checker reading as a size regression, which is exactly the confusion
    # the zero-divisor guard below exists to avoid.
    #
    # And an object of numbers, not merely well-formed JSON. `jq empty`
    # accepts an array, a bare string, `null`; the filters below index the
    # value with `$base[$k]`, which jq rejects on any of those — same abort,
    # same raw-jq-error exit, same misreading. Checking the values matters
    # for the same reason: jq orders numbers before strings, so a string size
    # passes `> 0` and only fails later at `(.value - $was)` with "string and
    # number cannot be subtracted", which is the identical broken-checker-
    # reads-as-a-regression failure one filter further along. Not reachable
    # while `measure` writes every file through `tonumber`; this is
    # shape-drift insurance against a format change or a `size-baseline`
    # artifact from some other producer.
    if [ ! -s "$baseline" ] || ! jq -e 'type == "object" and all(.[]; type == "number")' "$baseline" >/dev/null 2>&1; then
        # `warning`, not `notice`: a gate that is not running must not look
        # the same as a gate that ran clean. Main has had >90-day gaps in
        # this repo, which outlives artifact retention, so this path is
        # reachable without anyone doing anything wrong.
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
            # `> 0`, not just non-null: jq aborts the whole filter on a zero
            # divisor, which exits non-zero and reads as a broken checker
            # rather than a size regression. Not reachable from a linked
            # binary, but `measure` records 0 for any executable-but-empty
            # file — which the fixtures create — so the first test that pipes
            # `measure` output into `compare` would trip it. A zero baseline
            # entry now behaves like a new example: reported, not compared.
            | select($was != null and $was > 0)
            | ((.value - $was) / $was * 100) as $pct
            | {
                name: $k,
                was: $was,
                now: .value,
                # Rounded once, then both printed and compared. Comparing
                # the raw value while printing the rounded one made +15.04%
                # fail with "grew +15% ... over the +15% threshold", which
                # reads as a bug in the checker rather than a real
                # regression.
                # `+ 0` normalises negative zero. `round` on a small
                # negative yields -0, which is `>= 0` in jq, so the `+` prefix
                # was applied and the value stringified as `-0` — rendering
                # `+-0%`. A one-byte shrink on a 13MB binary lands there, so
                # it is the routine case rather than an edge one.
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
    # `<= 0`, matching the filter above: a key present in the baseline with a
    # zero size is dropped from the comparison, and `== null` would drop it
    # from here too — leaving it in neither the table, nor `added`, nor
    # `removed`, with the `N compared` count its only trace. That is the
    # shape the removed/renamed notice was added to close.
    added=$(jq -r '. as $cur | input as $base
        | [$cur | keys[] | select(($base[.] // 0) <= 0)] | join(", ")' "$current" "$baseline")
    if [ -n "$added" ]; then
        echo
        # "No *usable* baseline": round 7 widened the selector to fold in
        # zero-size baseline entries, for which a baseline does exist — it is
        # just unusable as a divisor. Saying "no baseline" for a key plainly
        # present in the artifact starts the reader from a wrong hypothesis.
        echo "::notice::No usable baseline, not checked (new, or zero-size in the baseline): $added"
    fi

    # Symmetric to the above. A removed or renamed example silently leaves
    # the comparison, and the "N compared" count was its only trace — which
    # is the same shape as the disjoint-key mode, just partial.
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

    # An empty intersection means the check compared nothing and would
    # otherwise report "All examples within threshold" — the dangerous mode,
    # because a silently disabled gate is indistinguishable from a passing
    # one. The hash-suffix filter in `measure` guards the one known cause;
    # this guards the symptom, whatever the cause.
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
    # Both totals, not a bare compared count. The disjoint guard above catches
    # a *total* key-set divergence, but a partial one reads identically to a
    # clean run: rename 19 of 20 examples and this line says "1 compared" and
    # goes green with 95% of the surface ungated. Nobody reading a passing
    # summary notices that the 1 used to be 20 — the added/removed notices name
    # the keys but read as routine churn. With both denominators the gradient
    # is visible at a glance.
    local n_current n_baseline
    n_current=$(jq 'length' < "$current")
    n_baseline=$(jq 'length' < "$baseline")
    echo "All examples within +${max_growth}% of baseline \
(${compared} compared of ${n_current} built / ${n_baseline} in baseline)."
}

# Renders the step-summary table from a size map.
#
# A subcommand rather than inline jq in the workflow, so it sits behind the
# same fixtures as `measure` and `compare` — it was the last size logic in
# the job with no test, which undercut the point of consolidating the
# definitions in the first place.
#
# One decimal always, so a 13.0MB binary does not render `13 MB` beside a
# neighbour rendering `13.4 MB`, and a `<0.1 MB` floor so a small binary
# reads as small rather than as `0 MB`.
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
    compare) shift; [ $# -ge 2 ] || usage; compare "$@" ;;
    render)  shift; [ $# -eq 1 ] || usage; render "$@" ;;
    *) usage ;;
esac
