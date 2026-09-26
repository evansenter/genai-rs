#!/usr/bin/env bash
# Fixture tests for check_changelog.sh.
#
# Run directly: .github/scripts/tests/test_check_changelog.sh
#
# Every rule is pinned in both directions — it fires on its defect, at the
# right line, and stays quiet on the look-alike — since a lint that cannot
# fail is the same defect one level up.
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
CHECK="$script_dir/../check_changelog.sh"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0

fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# expect <label> <exit> <expected stdout> <file> [version]
# Output is compared whole, so a violation reported at the wrong line, or an
# extra one, fails as surely as a missing one.
expect() {
    local label=$1 want_rc=$2 want_out=$3
    shift 3
    local out rc=0
    out=$("$CHECK" "$@" 2>/dev/null) || rc=$?
    if [ "$rc" -ne "$want_rc" ]; then
        fail "$label: exit $rc, expected $want_rc
  output: [$out]"
    fi
    if [ "$out" != "$want_out" ]; then
        fail "$label: output mismatch
  expected: [$want_out]
  actual:   [$out]"
    fi
}

# --- clean ------------------------------------------------------------------

cat >"$work/clean.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

### Fixed

- b

## [0.2.0] - 2026-07-01

### Added

- c
EOF
expect "clean file" 0 "" "$work/clean.md"
expect "clean file, with version" 0 "" "$work/clean.md" 0.2.0

# --- rule 1: duplicate ### within a section ---------------------------------

cat >"$work/dup_unreleased.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

### Fixed

- b

### Added

- c
EOF
expect "duplicate in [Unreleased]" 1 \
    "$work/dup_unreleased.md:13: duplicate \"### Added\" in ## [Unreleased] (first at line 5)" \
    "$work/dup_unreleased.md"

# Trailing whitespace is not a different heading.
printf '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- a\n\n### Added  \n\n- b\n' \
    >"$work/dup_trailing_space.md"
expect "duplicate differing only in trailing space" 1 \
    "$work/dup_trailing_space.md:9: duplicate \"### Added\" in ## [Unreleased] (first at line 5)" \
    "$work/dup_trailing_space.md"

# The same heading in two *different* sections is the normal shape.
cat >"$work/same_heading_two_sections.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a

## [0.2.0] - 2026-07-01

### Added

- b
EOF
expect "same heading across sections" 0 "" "$work/same_heading_two_sections.md" 0.2.0

cat >"$work/dup_versioned.md" <<'EOF'
# Changelog

## [0.10.0] - 2026-08-16

### Fixed

- a

### Fixed

- b
EOF
expect "duplicate in the named version" 1 \
    "$work/dup_versioned.md:9: duplicate \"### Fixed\" in ## [0.10.0] (first at line 5)" \
    "$work/dup_versioned.md" 0.10.0
# Older, already-published sections are out of scope: without the version,
# or with a version that is only a prefix of it, nothing is checked.
expect "duplicate in an unnamed older section" 0 "" "$work/dup_versioned.md"
expect "version 0.1.0 does not select [0.10.0]" 0 "" "$work/dup_versioned.md" 0.1.0

# --- rule 2: conflict markers -----------------------------------------------

cat >"$work/conflict.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

<<<<<<< HEAD
- ours
=======
- theirs
>>>>>>> branch
EOF
expect "conflict markers" 1 \
    "$work/conflict.md:7: merge-conflict marker
$work/conflict.md:9: merge-conflict marker
$work/conflict.md:11: merge-conflict marker" \
    "$work/conflict.md"

# --- rule 3: blank line before headings -------------------------------------

cat >"$work/no_blank.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a
### Fixed

- b
## [0.2.0] - 2026-07-01
EOF
expect "headings with no blank line above" 1 \
    "$work/no_blank.md:8: heading not preceded by a blank line: ### Fixed
$work/no_blank.md:11: heading not preceded by a blank line: ## [0.2.0] - 2026-07-01" \
    "$work/no_blank.md"

# --- fences -----------------------------------------------------------------

# Inside a fence, heading-, marker- and duplicate-shaped lines are content.
# Unindented on purpose: every rule is anchored at column 0, so an indented
# fence (inside a list item, say) would pass with the fence skip deleted.
cat >"$work/fenced.md" <<'EOF'
# Changelog

## [Unreleased]

### Added

- a migration example follows.

```text
### Added
<<<<<<< HEAD
=======
## not a heading
```

- b
EOF
expect "structure inside a fence is ignored" 0 "" "$work/fenced.md"

# --- usage ------------------------------------------------------------------

expect "no arguments" 2 ""
expect "too many arguments" 2 "" "$work/clean.md" 0.2.0 extra
expect "missing file" 2 "" "$work/does-not-exist.md"

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "All checks passed."
