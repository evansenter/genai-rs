#!/usr/bin/env bash
# Print the body of one version's section from a Keep a Changelog file.
#
# Usage: extract_changelog_section.sh <changelog.md> <version>
#
# `version` is bare (0.10.0), not tag-shaped (v0.10.0) — the caller strips
# the prefix, because the tag and the heading do not have to agree on it.
#
# Exits 1 with a message on stderr when the section is absent, so a caller
# can fall back rather than publish an empty release body.
set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 <changelog.md> <version>" >&2
    exit 2
fi

changelog=$1
version=$2

if [ ! -f "$changelog" ]; then
    echo "::error::changelog not found: $changelog" >&2
    exit 1
fi

# Compared against the heading up to and including the closing bracket, so
# the date suffix cannot affect the match: `## [0.10.0]`,
# `## [0.10.0] - 2026-08-16` and `## [0.10.0] — 2026-08-16` are all the same
# section. Only the bracketed version is load-bearing.
#
# The brackets are what keep `0.1` and `0.1.0` from finding `[0.10.0]` —
# `## [0.1]` is not a prefix of `## [0.10.0]`, because the character after
# `0.1` is `0` and not `]`. A bare-version search would match, which is why
# the delimiter is part of the compared string rather than stripped off.
section=$(awk -v want="## [$version]" '
    # A heading ends the section we are in, whichever version it names —
    # including an identical one, which would mean a duplicated section.
    /^## / {
        if (collecting) { exit }
        # $0 up to and including the closing bracket, so a trailing date or
        # dash cannot affect the comparison.
        close_bracket = index($0, "]")
        if (close_bracket > 0 && substr($0, 1, close_bracket) == want) {
            collecting = 1
            next
        }
    }
    collecting { print }
' "$changelog")

# Trim leading and trailing blank lines; a Keep a Changelog section always
# opens with one, and the release body should not.
section=$(printf '%s\n' "$section" | sed -e '/./,$!d' | tac | sed -e '/./,$!d' | tac)

if [ -z "$section" ]; then
    echo "::error::no [$version] section in $changelog" >&2
    exit 1
fi

printf '%s\n' "$section"
