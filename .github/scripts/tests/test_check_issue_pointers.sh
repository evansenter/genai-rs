#!/usr/bin/env bash
# Exercises check_issue_pointers.sh against a fixture tree and a stubbed
# `gh` (issue states and the rolling issue), so what it reads, what it
# leaves to the writer and when it opens, updates, comments on or closes
# its issue are asserted here rather than on the first morning a pointer
# goes stale.
#
# Run directly: .github/scripts/tests/test_check_issue_pointers.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$(dirname "$HERE")/check_issue_pointers.sh"
TITLE='Issue pointers name closed issues'
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# A tree with one of each: pointers to an open, a closed and a missing
# issue and to a closed PR, one wrapped across two comment lines, and what
# is not read (history, fixtures, the harnesses, a provenance tag, "see
# #N" without "for").
T="$WORK/tree"
mkdir -p "$T/docs" "$T/src" "$T/tests/fixtures" "$T/.github/scripts/tests"
git -C "$T" init -q
cat >"$T/docs/GUIDE.md" <<'MD'
# A guide

A background consumer is tracked in #535.
Windows is unverified until #536 lands; see #500 for what is left.
Subagent controls (#493 item 3) hide the tool; see #493.
MD
cat >"$T/src/lib.rs" <<'RS'
/// Reads a store.
pub fn read() {
    // The upload name is tracked
    // in #539, and the stream fix is TRACKED BY #12.
}
// Waits until #404 is fixed.
RS
printf '%s\n' '- Fixed; tracked in #500.' >"$T/CHANGELOG.md"
printf '%s\n' 'Rejected until #500 lands.' >"$T/DECISIONS.md"
printf '%s\n' 'tracked in #500' >"$T/tests/fixtures/body.txt"
printf '%s\n' 'echo "tracked in #500"' >"$T/.github/scripts/tests/test_x.sh"
printf '%s\n' 'Untracked: tracked in #500.' >"$T/NOTES.md"
git -C "$T" add docs src CHANGELOG.md DECISIONS.md tests .github

# A `gh` that answers `api repos/{owner}/{repo}/issues/N` from
# $WORK/issues/N (a JSON object, `404`, or `fail`), `issue list` from
# $GH_LIST and `issue view` from $GH_VIEW (either fails when it is `fail`),
# and logs every other call, with its --body, to $WORK/gh.log.
mkdir -p "$WORK/bin" "$WORK/issues"
cat >"$WORK/bin/gh" <<'EOF'
#!/usr/bin/env bash
if [ "$1" = api ]; then
    n=${2##*/}
    answer=$(cat "$GH_ISSUES/$n" 2>/dev/null || echo 404)
    case "$answer" in
        404) echo "gh: Not Found (HTTP 404)" >&2; exit 1 ;;
        fail) echo "HTTP 502" >&2; exit 1 ;;
    esac
    printf '%s\n' "$answer"
    exit 0
fi
echo "gh $*" >>"$GH_LOG"
case "$1 $2" in
    "issue list")
        [ "$GH_LIST" = fail ] && { echo "HTTP 502" >&2; exit 1; }
        printf '%s\n' "$GH_LIST"
        ;;
    "issue view")
        [ "$GH_VIEW" = fail ] && { echo "HTTP 502" >&2; exit 1; }
        printf '%s\n' "$GH_VIEW"
        ;;
esac
EOF
chmod +x "$WORK/bin/gh"

issue() { jq -n --arg s "$2" --arg t "$3" '{state: $s, title: $t}' >"$WORK/issues/$1"; }
issue 535 open "Antigravity: consume trigger turns"
issue 536 open "Antigravity on Windows"
issue 539 open "Files API: a chosen upload name"
issue 500 closed "Streaming and HTTP robustness follow-ups"
issue 493 closed "Antigravity parity backlog"
issue 12 closed "A merged pull request"
# #404 has no file: the API answers 404.

NO_BODY='{"body": ""}'
run() {
    : >"$WORK/gh.log"
    rc=0
    out=$(PATH="$WORK/bin:$PATH" GH_LOG="$WORK/gh.log" GH_ISSUES="$WORK/issues" \
        GH_LIST="${GH_LIST:-[]}" GH_VIEW="${GH_VIEW:-$NO_BODY}" \
        bash "$SCRIPT" "$@" 2>&1) || rc=$?
}
expect_rc() {
    if [ "$rc" -ne "$2" ]; then
        fail "$1: expected exit $2, got $rc: $out"
    fi
}
calls() { grep -c "^gh issue $1" "$WORK/gh.log" || true; }
logged() { grep -qF -- "$1" "$WORK/gh.log"; }
ours() { jq -n --arg t "$TITLE" --arg s "$1" '[{number: 560, title: $t, state: $s}]'; }
OTHER=$(jq -n '[{number: 7, title: "Issue pointers name closed issues, almost", state: "OPEN"}]')

# --print lists every pointer, wrapped and in any case, with its issue's
# state, and nothing it should not read; it never touches an issue. It
# fails on a pointer to a closed or missing issue.
run --print "$T"
expect_rc "--print with stale pointers" 1
for want in 'docs/GUIDE.md:3: "tracked in #535" names #535 (open: Antigravity: consume trigger turns)' \
    'docs/GUIDE.md:4: "until #536" names #536 (open: Antigravity on Windows)' \
    'docs/GUIDE.md:4: "see #500 for" names #500 (closed: Streaming and HTTP robustness follow-ups)' \
    'src/lib.rs:3: "tracked in #539" names #539 (open: Files API: a chosen upload name)' \
    'src/lib.rs:4: "TRACKED BY #12" names #12 (closed: A merged pull request)' \
    'src/lib.rs:6: "until #404" names #404 (no such issue)' \
    '6 pointer(s) to 6 issue(s); 3 name a closed one.'; do
    grep -qF -- "$want" <<<"$out" || fail "--print lacks: $want"$'\n'"$out"
done
[ "$(grep -c ' names #' <<<"$out")" = 6 ] || fail "--print lists other than the 6 pointers:"$'\n'"$out"
for unwanted in '#493' 'CHANGELOG.md' 'DECISIONS.md' 'fixtures' 'test_x.sh' 'NOTES.md'; do
    grep -qF -- "$unwanted" <<<"$out" && fail "--print reports $unwanted:"$'\n'"$out"
done
[ -s "$WORK/gh.log" ] && fail "--print called gh issue: $(cat "$WORK/gh.log")"

# The wrapped pointer, "tracked" then "in #539" on the next comment line,
# is found on its first line.
issue 539 closed "Files API: a chosen upload name"
run --print "$T"
grep -qF 'src/lib.rs:3: "tracked in #539" names #539' <<<"$out" ||
    fail "a pointer wrapped across two comment lines was missed: $out"
issue 539 open "Files API: a chosen upload name"

# Hard-wrapped Markdown can break just before the `#N`, so the next line
# starts with it: that `#` is the issue's, not a heading's or a comment's.
W="$WORK/wrapped"
mkdir -p "$W"
git -C "$W" init -q
printf '%s\n' 'Prose that is tracked in' '#500 and more.' 'Also see' '#12 for the rest.' \
    '- An item wrapped until' '  #404.' '' '## Not a pointer: tracked in' '# 500 heading' >"$W/GUIDE.md"
git -C "$W" add GUIDE.md
run --print "$W"
expect_rc "--print, pointers wrapped before the #" 1
for want in 'GUIDE.md:1: "tracked in #500" names #500' 'GUIDE.md:3: "see #12 for" names #12' \
    'GUIDE.md:5: "until #404" names #404' '3 pointer(s) to 3 issue(s); 3 name a closed one.'; do
    grep -qF -- "$want" <<<"$out" || fail "a pointer wrapped before its # was missed: $want"$'\n'"$out"
done

# No issue yet: one is filed, labeled, listing each finding, with the
# marker; an issue with a similar title is not ours.
GH_LIST=$OTHER run "$T"
expect_rc "stale, no issue" 0
[ "$(calls create)" = 1 ] || fail "expected one new issue: $(cat "$WORK/gh.log")"
logged "--title $TITLE --label documentation" || fail "the issue lacks its title or label: $(cat "$WORK/gh.log")"
for want in '`docs/GUIDE.md:4`: "see #500 for", and #500 is closed' '"until #404", and #404 names no issue' \
    '<!-- issue-pointers: '; do
    logged "$want" || fail "the issue lacks: $want"$'\n'"$(cat "$WORK/gh.log")"
done
[ "$(calls "edit 7")" = 0 ] || fail "edited an issue with another title"

# The same findings on our open issue, whose body carries their marker: the
# body is refreshed, with no comment.
marker=$(grep -o '<!-- issue-pointers: [0-9]* -->' "$WORK/gh.log")
GH_LIST=$(ours OPEN) GH_VIEW=$(jq -n --arg b "old $marker" '{body: $b}') run "$T"
expect_rc "stale, already reported" 0
[ "$(calls "edit 560")" = 1 ] || fail "the body was not refreshed: $(cat "$WORK/gh.log")"
[ "$(calls comment)" = 0 ] || fail "commented on findings already reported: $(cat "$WORK/gh.log")"
[ "$(calls create)" = 0 ] || fail "filed a duplicate: $(cat "$WORK/gh.log")"

# New findings on a closed issue: reopened, refreshed and commented on.
GH_LIST=$(ours CLOSED) GH_VIEW=$(jq -n '{body: "old <!-- issue-pointers: 1 -->"}') run "$T"
expect_rc "new findings, issue closed" 0
[ "$(calls "reopen 560")" = 1 ] || fail "the closed issue was not reopened: $(cat "$WORK/gh.log")"
[ "$(calls "edit 560")" = 1 ] || fail "the body was not refreshed: $(cat "$WORK/gh.log")"
[ "$(calls "comment 560")" = 1 ] || fail "new findings were not announced: $(cat "$WORK/gh.log")"

# Every pointer names an open issue: our open issue is closed with a
# comment; a closed one stays closed; nothing is filed.
issue 500 open "Streaming and HTTP robustness follow-ups"
issue 12 open "A pull request"
issue 404 open "An issue"
GH_LIST=$(ours OPEN) run "$T"
expect_rc "clean, issue open" 0
[ "$(calls "comment 560")" = 1 ] || fail "no closing comment: $(cat "$WORK/gh.log")"
[ "$(calls "close 560")" = 1 ] || fail "the issue was not closed: $(cat "$WORK/gh.log")"
GH_LIST=$(ours CLOSED) run "$T"
expect_rc "clean, issue closed" 0
[ "$(calls close)" = 0 ] || fail "closed a closed issue: $(cat "$WORK/gh.log")"
[ "$(calls create)" = 0 ] || fail "filed an issue with nothing to report"
run --print "$T"
expect_rc "--print, clean" 0

# A failed gh call fails the run, rather than reading as "open" or "no
# issue".
echo fail >"$WORK/issues/535"
run "$T"
expect_rc "an issue lookup fails" 1
issue 535 open "Antigravity: consume trigger turns"
issue 500 closed "Streaming and HTTP robustness follow-ups"
GH_LIST=fail run "$T"
expect_rc "gh issue list fails" 1
GH_LIST=$(ours OPEN) GH_VIEW=fail run "$T"
expect_rc "gh issue view fails" 1

# Bad usage.
run "$T" extra
expect_rc "two roots" 2
run "$WORK/not-a-tree"
expect_rc "not a git work tree" 2

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)" >&2
    exit 1
fi
echo "check_issue_pointers.sh: all checks passed"
