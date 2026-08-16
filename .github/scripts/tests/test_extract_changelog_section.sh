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

# The trailing trim needs a fixture whose trailing blank line carries
# whitespace — note the two spaces on the blank line before `## [0.2.0]`
# above. Without them this cannot be tested at all from outside: the
# extractor assigns `section` from a command substitution, which strips every
# trailing newline before the final `printf` ever runs, so the output ends in
# exactly one newline whatever the awk trim does. Trailing *spaces* survive
# that stripping, so a dropped trim shows up as a whitespace-only last line.
last_line=${out##*$'\n'}
case "$last_line" in
    *[![:space:]]*) ;;
    *) fail "trailing blank line was not trimmed; last line is [$last_line]" ;;
esac

# The emitted file must end in exactly one newline. This is a guard on the
# final `printf`, not on the trim: the extractor assigns `section` from a
# command substitution, which strips every trailing newline before that
# `printf` runs, so no change to the awk trim can break this. It is worth
# pinning anyway — a later edit to that `printf` could — but the trim itself
# is covered by the `$last_line` check above.
#
# `tail -c2` rather than `od | tail | head`: this harness runs under
# `pipefail`, where a `head` that closes the pipe early would abort the whole
# run, and two bytes cannot straddle an od line boundary the way a doubled
# newline in a full dump can. Note `tr -s ' '` squeezes the padding od puts
# between fields, so the doubled-newline pattern is `\n \n` with one space.
"$EXTRACT" "$work/CHANGELOG.md" 0.10.0 >"$work/body.md"
tail_bytes=$(tail -c2 "$work/body.md" | od -An -c | tr -s ' ')
case "$tail_bytes" in
    *'\n \n'*) fail "the body ends in more than one newline:$tail_bytes" ;;
    *'\n'*) ;;
    *) fail "the body does not end in a newline:$tail_bytes" ;;
esac

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

# --- a level-two heading inside a fence ---------------------------------------
# Pins current (truncating) behaviour rather than asserting it is desirable:
# `/^## /` does not track fence state, so a fenced line at column zero ends the
# section. Documented in the script header. If this ever needs fixing, this
# assertion is what changes.
cat >"$work/fenced.md" <<'EOF'
# Changelog

## [0.4.0] - 2026-03-03

- before the fence

```markdown
## [not a heading]
```

- after the fence

## [0.3.0] - 2026-02-02

- previous
EOF
assert_eq "a level-two heading inside a fence truncates the section (known limitation)" \
    '- before the fence

```markdown' "$("$EXTRACT" "$work/fenced.md" 0.4.0)"

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
