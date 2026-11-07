#!/usr/bin/env bash
# Exercises `categorize_failures.sh` against `gh run view --log-failed`
# output, with `gh` stubbed (#504).
#
# Three miscounts this pins: a test nextest retried to a pass inside a job
# that failed for another reason was counted as a failure, because its
# failed try printed `test ... FAILED` into the log; the Windows CRLF
# break, which failed on every Windows run and nowhere else, was filed as a
# "panic" flake; and a test that failed on one Windows leg while the other
# passed it was then called deterministic, though only the failed jobs are
# in the log. The Windows fixture is trimmed from a real run of both
# Windows legs (2026-11-07); the retry fixture is the counter's own.
#
# Each case runs under every awk on the machine (mawk on CI, BSD awk on a
# Mac).

set -euo pipefail

command -v jq >/dev/null 2>&1 || {
    echo "jq is required to run this harness (brew install jq / apt install jq)" >&2
    exit 1
}

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/categorize_failures.sh"
FIXTURES="$HERE/fixtures"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# `gh run view <id> --log-failed` prints the log the case names, and
# `gh run view <id> --json jobs` its jobs (it fails when the case names
# none, as a run whose jobs cannot be listed would).
cat > "$WORK/gh" <<'STUB'
#!/usr/bin/env bash
case " $* " in
  *" --log-failed "*) cat "$FAKE_LOG" ;;
  *" --json "*) [ -n "${FAKE_JOBS:-}" ] && cat "$FAKE_JOBS" ;;
esac
STUB
chmod +x "$WORK/gh"

# The jobs of the Windows fixture's run: every Cross Platform leg, and the
# Test Suite, which is named for no OS.
JOBS="$WORK/jobs.json"
jq -n '{jobs: ([
  "Cross Platform (ubuntu-latest, stable)", "Cross Platform (ubuntu-latest, beta)",
  "Cross Platform (macos-latest, stable)", "Cross Platform (macos-latest, beta)",
  "Cross Platform (windows-latest, stable)", "Cross Platform (windows-latest, beta)",
  "Test Suite"] | map({name: .}))}' > "$JOBS"

failures=0

categorize() {
    out=$(FAKE_LOG="$1" FAKE_JOBS="${2-$JOBS}" PATH="$WORK:$PATH" "$SCRIPT" 42)
}

expect_json() {
    local label="$1" filter="$2" want="$3" got
    got=$(jq -c "$filter" <<<"$out")
    if [ "$got" = "$want" ]; then
        echo "ok   - $label"
    else
        echo "FAIL - $label: $filter is $got, expected $want"
        failures=$((failures + 1))
    fi
}

WINDOWS="$FIXTURES/flakiness/windows_legs_run_view.txt"
CRLF_TEST="wire_streaming::tests::test_enum_wire_formats_doc_lists_the_serializers_names"

awks=()
for candidate in awk mawk gawk; do
    command -v "$candidate" >/dev/null 2>&1 && awks+=("$candidate")
done

for awk_impl in "${awks[@]}"; do
    echo "# awk: $awk_impl"
    export AWK="$awk_impl"

    # --- Both Windows legs fail one test, every try; no other job does.
    categorize "$WINDOWS"
    expect_json "one OS: the test is deterministic" '.categories.deterministic' "[\"$CRLF_TEST\"]"
    expect_json "one OS: named with its OS" ".os[\"$CRLF_TEST\"]" '"windows"'
    expect_json "one OS: not a panic flake" '.totals.panic' 0
    expect_json "one OS: counted once" '[.totals[]] | add' 1

    # --- The same test failing in a job named for no OS (Test Suite runs on
    #     Ubuntu) is not a one-OS break: categorized by its error text.
    cp "$WINDOWS" "$WORK/everywhere.log"
    printf 'Test Suite\tRun unit tests\t2026-11-07T00:20:00.0000000Z         FAIL [   0.030s] (1206/1670) genai-rs %s\n' \
        "$CRLF_TEST" >> "$WORK/everywhere.log"
    printf 'Test Suite\tRun unit tests\t2026-11-07T00:20:00.0000001Z     thread panicked at src/wire_streaming/mod.rs:686:14\n' \
        >> "$WORK/everywhere.log"
    categorize "$WORK/everywhere.log"
    expect_json "every OS: not deterministic" '.totals.deterministic' 0
    expect_json "every OS: by its error text" '.categories.panic' "[\"$CRLF_TEST\"]"

    # --- Two different OS legs: not a one-OS break either.
    sed 's/windows-latest, stable/macos-latest, stable/' "$WINDOWS" > "$WORK/two_os.log"
    categorize "$WORK/two_os.log"
    expect_json "two OSes: not deterministic" '.totals.deterministic' 0

    # --- One Windows leg fails the test and the other passes it (the beta
    #     leg is absent from the failed log): a flake, by its error text.
    grep -F 'windows-latest, stable' "$WINDOWS" > "$WORK/one_leg.log"
    categorize "$WORK/one_leg.log"
    expect_json "one leg of two: not deterministic" '.totals.deterministic' 0
    expect_json "one leg of two: by its error text" '.categories.panic' "[\"$CRLF_TEST\"]"

    # --- Both legs fail, but the run's jobs cannot be listed: the other
    #     legs are unknown, so not deterministic.
    categorize "$WINDOWS" ""
    expect_json "no job list: not deterministic" '.totals.deterministic' 0
    expect_json "no job list: by its error text" '.categories.panic' "[\"$CRLF_TEST\"]"

    # --- A failed integration job: one test failed all three tries, one
    #     passed on its second. Only the first is a failure.
    categorize "$FIXTURES/nextest_retries/red_run_view.txt"
    expect_json "retried to a pass: not a failure" \
        '[.categories[][]] | sort' '["test_system_instruction_inheritance"]'

    # --- The error text near the test's name picks the category of a test
    #     with no OS (the 21 lines between keep the two apart).
    {
        printf 'Integration Tests (core)\tRun\t2026-11-05T10:00:00.0Z   TRY 3 FAIL [   1.000s] (1/2) genai-rs::voices_tests test_voice_list\n'
        printf 'Integration Tests (core)\tRun\t2026-11-05T10:00:00.1Z     Api { status_code: 503, message: "Service Unavailable" }\n'
        for _ in $(seq 21); do printf 'Integration Tests (core)\tRun\t2026-11-05T10:00:00.2Z     ...\n'; done
        printf 'Integration Tests (core)\tRun\t2026-11-05T10:00:01.0Z         FAIL [   1.000s] (2/2) genai-rs::credentials_tests test_credential_list\n'
    } > "$WORK/api.log"
    categorize "$WORK/api.log"
    expect_json "error text: a 503 is an API error" '.categories.api_errors' '["test_voice_list"]'
    expect_json "error text: none is unknown" '.categories.unknown' '["test_credential_list"]'

    # --- A run whose log cannot be read: zeros, every category present.
    categorize /dev/null
    expect_json "empty log: zeros" '[.totals[]] | add' 0
    expect_json "empty log: every category" '.categories | keys' \
        '["api_errors","assertion_failures","deterministic","panic","rate_limit","unknown"]'
done

echo
if [ "$failures" -gt 0 ]; then
    echo "$failures check(s) failed."
    exit 1
fi
echo "All checks passed (awk: ${awks[*]})."
