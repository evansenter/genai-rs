#!/usr/bin/env bash
# Counts the tests nextest retried to a pass in CI logs (#492).
#
# A live test that fails and then passes under `--retries` leaves its job
# green, so the flakiness report, which used to read only failed runs, never
# saw it. nextest marks such a test `FLAKY n/m` in its final status list,
# and `TRY n PASS` (n > 1) as it happens; this reads both, and every run's
# `Summary` line for the number of tests run, so the report can give a
# rate.
#
# Usage: count_retried_flakes.sh [log...]    (reads stdin with no argument)
#
# A log is nextest's output as a job log holds it: `gh run view --log`
# lines (`job<TAB>step<TAB>timestamp line`), a raw job log (`timestamp
# line`), or plain output, with or without ANSI colour. One log may hold
# several nextest runs (the integration job runs one per binary); a test
# counts once per run it was flaky in.
#
# Output (JSON on stdout):
#   {"tests_run": N, "flaky": F,
#    "tests": [{"binary": "temp_file_tests", "name": "test_x",
#               "flaky": 1, "failed_tries": 1}, ...]}
# `tests_run` sums the runs' `Summary` lines; `flaky` is the number of
# (run, test) pairs that passed on a retry. `binary` is the test binary
# without the crate prefix (`lib` for the library's unit tests).

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
  echo "count_retried_flakes.sh: jq is required" >&2
  exit 2
}

# `cat --` with no files reads stdin. `AWK` lets the harness try each awk
# (Ubuntu runners have mawk). ESC is passed in rather than written
# as `\x1b`, which mawk (Ubuntu's awk) does not understand in a regex.
cat -- "$@" | "${AWK:-awk}" -v esc=$'\033' '
  {
    line = $0
    sub(/\r$/, "", line)
    # `gh run view --log` prefixes "job<TAB>step<TAB>"; nextest itself
    # writes no tabs.
    n = split(line, cols, "\t")
    if (n > 1) line = cols[n]
    # The runner timestamp, then ANSI colour: a raw job log carries ESC
    # itself, and `gh run view --log` prints it as the two characters `^[`.
    sub(/^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9:.]+Z /, "", line)
    gsub(esc "\\[[0-9;]*m", "", line)
    gsub(/\^\[\[[0-9;]*m/, "", line)

    # One nextest run starts each section; flakes are counted per run.
    if (line ~ /^ *Starting [0-9]+ tests? across /) { run++ }

    words = split(line, w, " ")
    if (words < 4) next

    if (w[1] == "Summary") {
      for (i = 1; i <= words; i++) {
        if (w[i] == "run:" && i > 2) tests_run += w[i - 2]
      }
      next
    }

    # `FLAKY 2/3 [ 0.011s] (2/4) crate::binary test` and
    # `TRY 2 PASS [ 0.011s] (2/4) crate::binary test`.
    tries = 0
    if (w[1] == "FLAKY" && w[2] ~ /^[0-9]+\/[0-9]+$/) {
      split(w[2], frac, "/")
      tries = frac[1] - 1
    } else if (w[1] == "TRY" && w[2] ~ /^[0-9]+$/ && w[3] == "PASS" && w[2] > 1) {
      tries = w[2] - 1
    } else {
      next
    }
    binary = w[words - 1]
    test = w[words]
    if (index(binary, "::") > 0) sub(/^[^:]*::/, "", binary)
    else binary = "lib"
    key = run SUBSEP binary SUBSEP test
    if (!(key in failed) || tries > failed[key]) failed[key] = tries
  }
  END {
    printf "run\t%d\n", tests_run
    for (key in failed) {
      split(key, k, SUBSEP)
      printf "flaky\t%s\t%s\t%d\n", k[2], k[3], failed[key]
    }
  }
' | jq -R -s '
  split("\n") | map(select(length > 0) | split("\t")) as $rows
  | {
      tests_run: ([$rows[] | select(.[0] == "run") | .[1] | tonumber] | add // 0),
      flaky: ([$rows[] | select(.[0] == "flaky")] | length),
      tests: (
        [$rows[] | select(.[0] == "flaky")]
        | group_by([.[1], .[2]])
        | map({
            binary: .[0][1],
            name: .[0][2],
            flaky: length,
            failed_tries: (map(.[3] | tonumber) | add)
          })
        | sort_by(-.flaky, .name)
      )
    }
'
