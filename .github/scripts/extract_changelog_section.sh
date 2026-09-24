#!/usr/bin/env bash
# Print the body of one version's section from a Keep a Changelog file.
#
# Usage: extract_changelog_section.sh <changelog.md> <version>
#
# `version` is bare (0.10.0), not tag-shaped (v0.10.0).
#
# Known limitation: a `## ` line inside a fenced code block ends the section
# early. Pinned by a harness fixture.
#
# Exit codes:
#   0  the section was found and printed
#   1  the file exists but has no such section — a recoverable case
#   2  a caller or environment error: wrong arguments, or no such file
#
# No `::error::` prefix: each caller picks the severity.
set -euo pipefail

if [ $# -ne 2 ]; then
    echo "usage: $0 <changelog.md> <version>" >&2
    exit 2
fi

changelog=$1
version=$2

if [ ! -f "$changelog" ]; then
    echo "changelog not found: $changelog" >&2
    exit 2
fi

# Matched up to the closing bracket, so a date suffix is ignored and `0.1`
# cannot match `[0.10.0]`. One awk pass rather than `tac`, which macOS lacks.
section=$(awk -v want="## [$version]" '
    # A heading ends the current section, even a duplicate of the same version.
    /^## / {
        if (collecting) { exit }
        close_bracket = index($0, "]")
        if (close_bracket > 0 && substr($0, 1, close_bracket) == want) {
            collecting = 1
            next
        }
    }
    collecting {
        # Blank lines are held, so leading and trailing runs are dropped.
        if ($0 ~ /^[[:space:]]*$/) {
            if (started) { pending = pending $0 "\n" }
            next
        }
        if (pending != "") { printf "%s", pending; pending = "" }
        print
        started = 1
    }
' "$changelog")

if [ -z "$section" ]; then
    echo "no [$version] section in $changelog" >&2
    exit 1
fi

printf '%s\n' "$section"
