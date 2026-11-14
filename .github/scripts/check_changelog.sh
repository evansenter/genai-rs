#!/usr/bin/env bash
# Structural lint for a Keep a Changelog file (#458).
#
# Usage: check_changelog.sh <changelog.md> [version]
#
# `## [Unreleased]` is a merge serialization point: parallel PRs all insert
# at the same anchor, and "keep both" preserves every line but not the
# heading it was written under. The result ships verbatim as the release
# body (extract_changelog_section.sh), so these rules run before tag time:
#
#   1. No `###` heading appears twice within one `## [...]` section.
#      Checked for `[Unreleased]` and, if given, `[version]` only: older
#      sections predate this lint and their release bodies are already
#      published, so rewriting them would change nothing a reader saw.
#   2. No merge-conflict markers, anywhere in the file.
#   3. Every `##`/`###` heading after the first line is preceded by a blank
#      line — the artifact a hand-relocated section leaves behind.
#   4. No `## [...]` heading appears twice, anywhere. A keep-both merge can
#      produce two `## [Unreleased]`; rule 1 would then check each copy
#      separately, and extract_changelog_section.sh ships only one of them.
#   5. Every breaking entry (a list item that starts `- **Breaking`) says
#      what to do about it in a `Migration:` of its own (CLAUDE.md, D-007).
#      An entry runs from its `- ` to the next line that is neither blank
#      nor indented. Checked where rule 1 is.
#   6. No entry appears twice within one `## [...]` section: no two
#      entries (list items at column 0) share their first line, and no two
#      open alike. An entry opens with the first sentence (up to the first
#      period outside a code span that a space or the end follows) of its
#      bold title (the `**...**` it opens with, wrapped or not, a leading
#      `Breaking:` aside), or of its first paragraph when it has none. Two
#      openings are alike when their first code spans are the same (or
#      neither has one), and they hold the same words, or the shorter holds
#      at least 3, the other holds 85% of those, and the words they share
#      are 45% of all the words of both. Words are case-folded, a code span
#      is one word, and one-letter words, a few function words (`the`,
#      `of`, `not`, ...) and a plural's trailing `s` are left out. So a copy
#      that lost or gained its `Breaking:` marker or its bold, or that a
#      rewrite retitled but kept the subject and most of the words of,
#      still matches, while two entries that say the same of different
#      code, or share a few words, do not: no two different entries in any
#      section this CHANGELOG has had open alike (D-073). A keep-both
#      merge that re-adds an entry's old text beside its rewrite leaves a
#      contradictory pair that every other rule passes. Checked where rule
#      1 is, across its `###` subsections.
#
# Fenced code blocks are skipped for all six. An unclosed fence is itself
# reported: otherwise everything after it would go unchecked and pass.
#
# Output: one `<file>:<line>: <problem>` per violation, on stdout.
# Exit codes:
#   0  clean
#   1  at least one violation
#   2  a caller or environment error: wrong arguments, or no such file
#
# No `::error::` prefix: each caller picks the severity.
set -euo pipefail

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    echo "usage: $0 <changelog.md> [version]" >&2
    exit 2
fi

changelog=$1
version=${2:-}

if [ ! -f "$changelog" ]; then
    echo "changelog not found: $changelog" >&2
    exit 2
fi

awk -v file="$changelog" -v want="## [$version]" -v have_version="${version:+1}" '
    function report(msg) { printf "%s:%d: %s\n", file, NR, msg; bad = 1 }

    # Rule 5: the breaking entry that just ended has its migration.
    function finish_entry() {
        if (breaking && !migration) {
            printf "%s:%d: breaking entry with no \"Migration:\": %s\n", file, entry_line, entry_head
            bad = 1
        }
        breaking = 0
        migration = 0
    }

    # Rule 6: an entry opens with the first sentence of its bold title (a
    # leading `Breaking:` or `BREAKING` aside), or of its first paragraph
    # when the title is only that marker or there is none. One whose `**`
    # does not close in that paragraph opens with nothing.
    function finish_opening(    text, bold_end, title) {
        if (!opening_open) {
            return
        }
        opening_open = 0
        text = opening_text
        gsub(/[[:space:]]+/, " ", text)
        sub(/^ /, "", text)
        sub(/ $/, "", text)
        if (substr(text, 1, 2) == "**") {
            bold_end = index(substr(text, 3), "**")
            if (!bold_end) {
                return
            }
            title = substr(text, 3, bold_end - 1)
            sub(/^ /, "", title)
            sub(/ $/, "", title)
            if (tolower(substr(title, 1, 8)) == "breaking") {
                title = substr(title, 9)
                sub(/^:? */, "", title)
            }
            if (title != "") {
                text = title
            } else {
                text = substr(text, bold_end + 4)
                sub(/^ /, "", text)
            }
        }
        compare_opening(first_sentence(text))
    }

    # Up to the first period outside a code span that a space or the end
    # follows.
    function first_sentence(text,    n, i, ch, tick) {
        n = length(text)
        for (i = 1; i <= n; i++) {
            ch = substr(text, i, 1)
            if (ch == "`") {
                tick = !tick
            } else if (ch == "." && !tick && (i == n || substr(text, i + 1, 1) == " ")) {
                return substr(text, 1, i)
            }
        }
        return text
    }

    # The words of an opening, as keys of `words`; returns how many, and
    # sets opening_span to its first code span ("" when it has none).
    # Case-folded; a code span is one word; one-letter words, the function
    # words in `stop` and a plural s are left out.
    function opening_words(h, words,    s, span, parts, n, i, w, count) {
        for (w in words) {
            delete words[w]
        }
        s = tolower(h)
        opening_span = ""
        count = 0
        while (match(s, /`[^`]*`/)) {
            span = substr(s, RSTART, RLENGTH)
            if (opening_span == "") {
                opening_span = span
            }
            if (!(span in words)) {
                words[span] = 1
                count++
            }
            s = substr(s, 1, RSTART - 1) " " substr(s, RSTART + RLENGTH)
        }
        n = split(s, parts, /[^a-z0-9_]+/)
        for (i = 1; i <= n; i++) {
            w = parts[i]
            if (length(w) < 2 || w in stop) {
                continue
            }
            if (length(w) > 3 && w ~ /[^s]s$/) {
                w = substr(w, 1, length(w) - 1)
            }
            if (!(w in words)) {
                words[w] = 1
                count++
            }
        }
        return count
    }

    # Reports an opening alike an earlier one in the section (the rule in
    # the header), then records it. The thresholds are in integers: 85% is
    # 17 in 20, 45% is 9 in 20.
    function compare_opening(h,    n, i, w, shared, fewer, all) {
        n = opening_words(h, cur)
        if (!n) {
            return
        }
        for (i = 1; i <= openings && !opening_dup; i++) {
            if (span_of[i] != opening_span) {
                continue
            }
            shared = 0
            for (w in cur) {
                if ((i, w) in has_word) {
                    shared++
                }
            }
            fewer = n < word_count[i] ? n : word_count[i]
            all = n + word_count[i] - shared
            if (shared == all || (fewer >= 3 && 20 * shared >= 17 * fewer && 20 * shared >= 9 * all)) {
                printf "%s:%d: two entries open alike in %s (first at line %d): \"%s\"\n", file, opening_line, section, line_of[i], h
                bad = 1
                break
            }
        }
        openings++
        line_of[openings] = opening_line
        span_of[openings] = opening_span
        word_count[openings] = n
        for (w in cur) {
            has_word[openings, w] = 1
        }
    }

    BEGIN {
        n = split("an and are as at be been by for from in is it its no not of on or that the this to was were which with", list, " ")
        for (i = 1; i <= n; i++) {
            stop[list[i]] = 1
        }
    }

    # Rule 6: a blank line, a fence, a sub-item or a line that is not
    # indented ends the paragraph an entry opens in.
    opening_open && (/^[[:space:]]*$/ || /^[^[:space:]]/ || /^[[:space:]]+[-*+][[:space:]]/ || /^[[:space:]]*```/) {
        finish_opening()
    }

    # Fences toggle on their own line; nothing inside one is structure.
    /^[[:space:]]*```/ {
        fenced = !fenced
        if (fenced) { fence_line = NR }
        prev = $0
        next
    }
    fenced { prev = $0; next }

    /^(<<<<<<< |<<<<<<<$|=======$|>>>>>>> |>>>>>>>$)/ {
        report("merge-conflict marker")
    }

    # A line that is neither blank nor indented ends a list entry; one that
    # starts a breaking entry where rule 1 checks begins the next.
    /^[^[:space:]]/ {
        finish_entry()
        if (checked && /^- \*\*Breaking/) {
            breaking = 1
            entry_line = NR
            entry_head = $0
        }
    }
    breaking && /Migration:/ { migration = 1 }

    # Rule 6. The opening paragraph takes each indented line that follows.
    opening_open && /^[[:space:]]+[^[:space:]]/ {
        opening_text = opening_text " " $0
    }
    checked && /^- / {
        entry = $0
        sub(/[[:space:]]+$/, "", entry)
        same_line = entry in seen_entry
        if (same_line) {
            report("duplicate entry in " section " (first at line " seen_entry[entry] "): " entry)
        } else {
            seen_entry[entry] = NR
        }
        opening_open = 1
        opening_text = substr(entry, 3)
        opening_line = NR
        opening_dup = same_line
    }

    /^###? / {
        if (NR > 1 && prev !~ /^[[:space:]]*$/) {
            report("heading not preceded by a blank line: " $0)
        }
    }

    /^## / {
        # Matched up to the closing bracket, as extract_changelog_section.sh
        # does, so a date suffix is ignored and `0.1` cannot match `[0.10.0]`.
        close_bracket = index($0, "]")
        head = close_bracket > 0 ? substr($0, 1, close_bracket) : $0
        checked = (head == "## [Unreleased]") || (have_version && head == want)
        section = head
        if (head in seen_section) {
            report("duplicate section \"" head "\" (first at line " seen_section[head] ")")
        } else {
            seen_section[head] = NR
        }
        delete seen
        delete seen_entry
        delete has_word
        openings = 0
    }

    checked && /^### / {
        heading = $0
        sub(/[[:space:]]+$/, "", heading)
        if (heading in seen) {
            report("duplicate \"" heading "\" in " section " (first at line " seen[heading] ")")
        } else {
            seen[heading] = NR
        }
    }

    { prev = $0 }

    END {
        finish_opening()
        finish_entry()
        if (fenced) {
            printf "%s:%d: unclosed code fence; nothing after it was checked\n", file, fence_line
            bad = 1
        }
        exit bad ? 1 : 0
    }
' "$changelog"
