#!/usr/bin/env bash
# Fixture tests for extract_changelog_section.sh.
#
# Run directly: .github/scripts/tests/test_extract_changelog_section.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
EXTRACT="$script_dir/../extract_changelog_section.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

assert_eq() {
    local label=$1 expected=$2 actual=$3
    if [ "$expected" != "$actual" ]; then
        fail "$label
  expected: [$expected]
  actual:   [$actual]"
    fi
}

# A changelog with the shapes that matter: an unreleased section, adjacent
# versions whose numbers are prefixes of one another (0.1.0 vs 0.10.0), a
# heading with no date, and an em-dash date separator.
cat >"$work/CHANGELOG.md" <<'EOF'
# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

- pending work

## [0.10.0] - 2026-08-16

### Added

- ten

### Fixed

- also ten

## [0.2.0] — 2026-07-01

- em dash date

## [0.1.0]

- no date at all
EOF

# --- the ordinary case -------------------------------------------------------
out=$("$EXTRACT" "$work/CHANGELOG.md" 0.10.0)
expected='### Added

- ten

### Fixed

- also ten'
assert_eq "extracts the whole section, stopping at the next heading" "$expected" "$out"

# The leading blank line after the heading is trimmed — a release body should
# not open on blank. Command substitution leaves leading newlines intact, so
# this check is real.
[ "${out:0:1}" != $'\n' ] || fail "leading blank line was not trimmed"

# The trailing trim has to be checked against a file, not against `$out`:
# command substitution strips every trailing newline itself, so an assertion
# on the last character of `$out` passes no matter what the extractor emits.
# The callers redirect straight to a file, which is where the difference is
# observable.
"$EXTRACT" "$work/CHANGELOG.md" 0.10.0 >"$work/body.md"
trailing=$(od -c "$work/body.md" | tail -2 | head -1)
case "$trailing" in
    *'\n  \n'*) fail "trailing blank line reached the file: $trailing" ;;
esac
# And the file must still end in exactly one newline — a body with no final
# newline is as wrong as one with three.
[ "$(tail -c1 "$work/body.md" | od -An -c | tr -d ' ')" = '\n' ] \
    || fail "the body does not end in a newline"

# --- prefix collisions -------------------------------------------------------
# The bug this guards: a substring match for "0.1.0" would find "[0.10.0]"
# first (it appears earlier in the file), and a search for "0.1" would match
# both. Only whole-version equality gets this right.
assert_eq "0.1.0 is not satisfied by [0.10.0]" \
    "- no date at all" "$("$EXTRACT" "$work/CHANGELOG.md" 0.1.0)"

if "$EXTRACT" "$work/CHANGELOG.md" 0.1 >/dev/null 2>&1; then
    fail "a partial version (0.1) matched a section; it must not"
fi

# --- heading variants --------------------------------------------------------
assert_eq "an em-dash date separator is still the same section" \
    "- em dash date" "$("$EXTRACT" "$work/CHANGELOG.md" 0.2.0)"

assert_eq "a heading with no date at all still matches" \
    "- no date at all" "$("$EXTRACT" "$work/CHANGELOG.md" 0.1.0)"

assert_eq "Unreleased is addressable like any other section" \
    "- pending work" "$("$EXTRACT" "$work/CHANGELOG.md" Unreleased)"

# --- absent section ----------------------------------------------------------
if out=$("$EXTRACT" "$work/CHANGELOG.md" 9.9.9 2>"$work/err"); then
    fail "a missing version exited 0; the caller would publish an empty body"
fi
grep -q '9.9.9' "$work/err" || fail "the missing-version message does not name the version"

# --- missing file ------------------------------------------------------------
if "$EXTRACT" "$work/nope.md" 0.10.0 >/dev/null 2>&1; then
    fail "a missing changelog exited 0"
fi

# --- last section in the file ------------------------------------------------
# No trailing `## ` heading to stop on, so this exercises the fall-off-the-end
# path rather than the exit-on-next-heading one.
cat >"$work/tail.md" <<'EOF'
# Changelog

## [0.1.0] - 2026-01-01

- only section
EOF
assert_eq "the final section is extracted without a following heading" \
    "- only section" "$("$EXTRACT" "$work/tail.md" 0.1.0)"

# --- empty section -----------------------------------------------------------
# A heading with nothing under it must not be reported as success — the body
# would be empty and the release would say nothing.
cat >"$work/empty.md" <<'EOF'
# Changelog

## [0.3.0] - 2026-02-02

## [0.2.0] - 2026-01-01

- previous
EOF
if "$EXTRACT" "$work/empty.md" 0.3.0 >/dev/null 2>&1; then
    fail "an empty section exited 0; it should fall back like a missing one"
fi

if [ "$failures" -eq 0 ]; then
    echo "extract_changelog_section.sh: all checks passed"
else
    echo "extract_changelog_section.sh: $failures check(s) failed" >&2
    exit 1
fi
