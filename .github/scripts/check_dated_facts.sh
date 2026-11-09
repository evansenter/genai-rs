#!/usr/bin/env bash
# Enforces D-032 (one home per dated live fact) and keeps every Rust code
# block either compiled or marked with the reason it is not.
#
# Usage: .github/scripts/check_dated_facts.sh [ROOT]
#
# ROOT (default: the current directory) must be a git work tree; its tracked
# and untracked, not ignored, `*.md` and `*.rs` files are read, so a new file
# is checked before it is committed. Four rules:
#
# 1. A date is a dated live fact, and one lives only in its homes:
#    docs/ENUM_WIRE_FORMATS.md, docs/INTERACTIONS_API_GAP.md and
#    docs/ANTIGRAVITY_BRIDGE_DESIGN.md. A date is ISO (`2026-11-05`, or a
#    month, `2026-07`), a slashed one (`2026/11/05`, `11/05/2026`), or one
#    with the month named (`November 5, 2026`, `Nov 5, 2026`,
#    `5 November 2026`, `November 2026`). D-032's exceptions are not read:
#    DECISIONS.md and CHANGELOG.md (history), any file under a `tests/`
#    directory (each test's evidence, fixtures included), and in Rust
#    everything but the comments of non-test code: the doc comments in src/
#    and genai-rs-macros/src/, where a `//` comment beside the code an
#    observation justifies may carry its date, and every comment in
#    examples/ (examples/CLAUDE.md). Test code is a `*_tests.rs`,
#    `tests.rs`, `proptest_*.rs` or `test_subscriber.rs` file,
#    src/coverage_oracle/, and an item behind a `#[cfg(test)]` line. Not
#    dates: a timestamp (`2026-01-01T10:30:45Z`, `21 Oct 2015 07:28:00 GMT`:
#    example data), a link's target (a home's heading anchor), and the API
#    revision `2026-05-20`, a version name.
# 2. A code block fenced `ignore` (`rust,ignore`, or `rust ignore`: rustdoc
#    splits the info string on commas, spaces and tabs) has a
#    `<!-- ignore: reason -->` line directly above its fence: in any
#    Markdown file, and in any doc comment of any Rust file, test code and
#    `tests/` included.
# 3. A Markdown file that src/lib.rs does not include as docs
#    (`#[doc = include_str!("../<path>")]`, the `doc_tests` module) is
#    never doctested, so a Rust block in it (a fence whose info string
#    starts with `rust`, or names a rustdoc attribute such as `no_run`) is
#    `ignore`d, with rule 2's reason. History is exempt: CHANGELOG.md's
#    blocks show the API of the release each entry describes.
#    `tests/doc_guards/` requires every guide to be included.
# 4. A `text` block (also `txt` or `plain`, and an untagged one in a
#    Markdown file rule 3 reads) holds no Rust: a line that is a `let` or
#    `use` statement, a field assignment, a `fn`, `struct`, `enum`,
#    `trait` or `impl` header, or a `#[derive]`, `#[test]` or
#    `#[tokio::main]` attribute. Fence the block as rust so it compiles.
#
# Exit status: 0 when the tree follows the rules, 1 with each finding
# listed as `path:line: ...`, 2 on bad usage or when ROOT is not a git work
# tree. Needs python3 and git. `make test-scripts` runs it on the checkout.

set -euo pipefail

if [ $# -gt 1 ]; then
    awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0" >&2
    exit 2
fi
root=${1:-.}
for tool in git python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "$tool is required" >&2
        exit 2
    fi
done
if ! git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "not a git work tree: $root" >&2
    exit 2
fi

python3 -I - "$root" <<'EOF'
import os
import re
import subprocess
import sys

root = sys.argv[1]

HOMES = {
    "docs/ENUM_WIRE_FORMATS.md",
    "docs/INTERACTIONS_API_GAP.md",
    "docs/ANTIGRAVITY_BRIDGE_DESIGN.md",
}
HISTORY = {"DECISIONS.md", "CHANGELOG.md"}
# Dates that name something rather than date an observation.
NAMES = {"2026-05-20"}  # the API revision (D-032)
RUST_ROOTS = ("src/", "genai-rs-macros/src/", "examples/")

MONTH = (
    r"(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|June?|July?|Aug(?:ust)?"
    r"|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)"
)
DAY = r"(?:0?[1-9]|[12]\d|3[01])"
DAY_SUFFIX = r"(?:st|nd|rd|th)?"
# A time after a date makes it a timestamp, example data rather than a fact.
NOT_TIME = r"(?!\s*,?\s*\d{1,2}:\d\d)"
DATE = re.compile(
    # ISO: 2026-11-05, or the month 2026-07.
    r"(?<![\w.-])20\d\d-(?:0[1-9]|1[0-2])(?:-(?:0[1-9]|[12]\d|3[01]))?(?![\w-]|T\d)"
    # Slashed: 2026/11/05, and 11/05/2026 or 05/11/2026.
    rf"|(?<![\w/.])20\d\d/(?:0?[1-9]|1[0-2])/{DAY}(?![\w/]){NOT_TIME}"
    rf"|(?<![\w/.]){DAY}/{DAY}/20\d\d(?![\w/]){NOT_TIME}"
    # The month named: November 5, 2026; Nov. 5 2026; 5 November 2026;
    # November 2026.
    rf"|\b{MONTH}\.? {DAY}{DAY_SUFFIX},? 20\d\d\b{NOT_TIME}"
    rf"|\b{DAY}{DAY_SUFFIX} {MONTH}\.?,? 20\d\d\b{NOT_TIME}"
    rf"|\b{MONTH}\.?,? 20\d\d\b{NOT_TIME}"
)
LINK_TARGET = re.compile(r"\]\([^)\s]*\)")
AUTOLINK = re.compile(r"<https?://[^>\s]*>")
BARE_URL = re.compile(r"https?://\S+")
LINK_DEFINITION = re.compile(r"^(\s*\[[^\]]+\]:\s*)\S+")
FENCE = re.compile(r"^\s*(`{3,}|~{3,})\s*([^`]*)")
# Rustdoc splits a fence's info string on commas, spaces and tabs.
INFO_SEP = re.compile(r"[,\s]+")
REASON = re.compile(r"^\s*<!--\s*ignore:\s*\S.*-->\s*$")
# A whole-line comment; group 1 is `/` or `!` for a doc comment.
COMMENT = re.compile(r"^\s*//(/(?!/)|!)? ?(.*)$")
TEST_FILE = re.compile(r"(_tests|^tests|^proptest_\w+|^test_subscriber)\.rs$")
# What src/lib.rs includes as docs, and so doctests.
DOC_INCLUDE = re.compile(r'#\[doc\s*=\s*include_str!\(\s*"\.\./([^"]+\.md)"\s*\)\s*\]')
# Info-string tokens that make rustdoc read a block as Rust.
RUSTDOC_ATTRS = {"rust", "no_run", "should_panic", "compile_fail", "test_harness"}
TEXT_INFOS = {"text", "txt", "plain"}
# A line of Rust, for rule 4: a statement ending in `;`, or an item header.
RUST_STATEMENT = re.compile(
    r"^\s*(?:let\s+(?:mut\s+)?[a-z_]\w*\b.*=.*|use\s+[a-z_]\w*::\S.*"
    r"|[a-z_]\w*(?:\.[a-z_]\w*)+\s*=[^=].*);\s*(?://.*)?$"
)
RUST_ITEM = re.compile(
    r"^\s*(?:(?:pub(?:\(\w+\))?\s+)?(?:async\s+)?fn\s+[a-z_]\w*\s*[<(]"
    r"|(?:pub(?:\(\w+\))?\s+)?(?:struct|enum|trait)\s+[A-Z]\w*"
    r"|impl\b.*\{\s*$"
    r"|#\[(?:derive\(|test\]|tokio::main))"
)

findings = []


def dates(text):
    text = LINK_DEFINITION.sub(r"\1", text)
    for pattern in (LINK_TARGET, AUTOLINK, BARE_URL):
        text = pattern.sub("", text)
    return [d for d in DATE.findall(text) if d not in NAMES]


def info_tokens(info):
    return [t for t in INFO_SEP.split(info.strip()) if t]


class Fences:
    """Tracks one Markdown text's fences, line by line."""

    def __init__(self):
        self.open = None  # the opening run of backticks or tildes
        self.tokens = []  # the open fence's info string, split
        self.start = 0  # the open fence's line number
        self.body = []  # the open fence's lines so far

    def step(self, line, n):
        """Returns `("open", tokens)` on a fence's first line, `("close",
        tokens, start, body)` on its last, and None otherwise."""
        m = FENCE.match(line)
        if self.open is not None:
            closes = m and m.group(1)[0] == self.open[0] and len(m.group(1)) >= len(self.open)
            if closes and not line.strip().strip(m.group(1)[0]):
                self.open = None
                return ("close", self.tokens, self.start, self.body)
            self.body.append(line)
            return None
        if not m:
            return None
        self.open = m.group(1)
        self.tokens = info_tokens(m.group(2))
        self.start = n
        self.body = []
        return ("open", self.tokens)


def check_fence(path, n, event, prev, doc_line, compiled):
    """Rules 2 to 4 for one fence event in `path`. `compiled`: whether
    rustdoc compiles a Rust block here (rule 3 reads the rest)."""
    if event is None:
        return
    if event[0] == "open":
        tokens = event[1]
        if "ignore" in tokens and not REASON.match(prev or ""):
            where = "doc line" if doc_line else "line"
            findings.append(
                f"{path}:{n}: an ignore fence needs a `<!-- ignore: reason -->` {where} directly above it"
            )
        if not compiled and "ignore" not in tokens and RUSTDOC_ATTRS.intersection(tokens):
            findings.append(
                f"{path}:{n}: a Rust block here never compiles, because src/lib.rs does not include "
                f"{path} in its doc_tests: include the file there, or mark the block `ignore` with "
                "a `<!-- ignore: reason -->` line above it"
            )
        return
    _, tokens, start, body = event
    texty = (tokens[:1] and tokens[0] in TEXT_INFOS) or (not tokens and not compiled)
    if texty:
        for k, line in enumerate(body, start + 1):
            if RUST_STATEMENT.match(line) or RUST_ITEM.match(line):
                findings.append(
                    f"{path}:{k}: Rust in a text block never compiles: fence the block (line "
                    f"{start}) as rust, or as `rust,ignore` with a reason"
                )
                break


def check_markdown(path, text, doctested):
    dated = path not in HOMES and path not in HISTORY and "/tests/" not in "/" + path
    compiled = path in doctested or path in HISTORY
    fences = Fences()
    prev = ""
    for n, line in enumerate(text.splitlines(), 1):
        check_fence(path, n, fences.step(line, n), prev, False, compiled)
        if dated:
            for d in dates(line):
                findings.append(
                    f"{path}:{n}: dated live fact ({d}) outside its home: record it in "
                    "docs/ENUM_WIRE_FORMATS.md or docs/INTERACTIONS_API_GAP.md (Antigravity: "
                    "docs/ANTIGRAVITY_BRIDGE_DESIGN.md) and state the current behavior here (D-032)"
                )
        prev = line


def test_items(lines):
    """Indexes of the lines that belong to an item behind `#[cfg(test)]`."""
    skip = set()
    i = 0
    while i < len(lines):
        if lines[i].strip() != "#[cfg(test)]":
            i += 1
            continue
        indent = lines[i][: len(lines[i]) - len(lines[i].lstrip())]
        j = i + 1
        while j < len(lines) and lines[j].strip().startswith(("#[", "///", "//")):
            j += 1
        if j >= len(lines):
            break
        # A signature can span lines: the body opens on the first line that
        # ends in `{`, unless a `;` ends the item first.
        end = next((k for k in range(j, len(lines)) if lines[k].rstrip().endswith(("{", ";"))), j)
        if lines[end].rstrip().endswith("{"):
            close = indent + "}"
            end = next((k for k in range(end + 1, len(lines)) if lines[k].rstrip() == close), len(lines) - 1)
        skip.update(range(i, end + 1))
        i = end + 1
    return skip


def check_rust(path, text, dated):
    """Rule 2 and 4 in every doc comment; rule 1, when `dated`, in the
    comments of non-test code."""
    every_comment = path.startswith("examples/")
    lines = text.splitlines()
    skip = test_items(lines) if dated else set(range(len(lines)))
    fences = Fences()
    prev = None  # the previous doc line's text; None outside a doc block
    for n, line in enumerate(lines, 1):
        m = COMMENT.match(line)
        if not m or not m.group(1):
            prev = None
            fences = Fences()
            if m and every_comment and n - 1 not in skip:
                for d in dates(m.group(2)):
                    findings.append(
                        f"{path}:{n}: dated live fact ({d}) in an example: state what the API "
                        "does now and keep the dated observation in a home (D-032)"
                    )
            continue
        doc = m.group(2)
        check_fence(path, n, fences.step(doc, n), prev, True, True)
        if n - 1 not in skip:
            for d in dates(doc):
                findings.append(
                    f"{path}:{n}: dated live fact ({d}) in a doc comment: state the current behavior "
                    "without the date, and keep the dated evidence in a home or in a `//` comment "
                    "beside the code it justifies (D-032)"
                )
        prev = doc


listed = subprocess.run(
    ["git", "-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard",
     "--", "*.md", "*.rs"],
    check=True,
    capture_output=True,
).stdout.decode()
paths = sorted({p for p in listed.split("\0") if p})
lib_rs = os.path.join(root, "src", "lib.rs")
doctested = set()
if os.path.isfile(lib_rs):
    with open(lib_rs, encoding="utf-8") as f:
        doctested = set(DOC_INCLUDE.findall(f.read()))
checked = 0
for path in paths:
    full = os.path.join(root, path)
    if not os.path.isfile(full):
        continue
    with open(full, encoding="utf-8") as f:
        text = f.read()
    if path.endswith(".md"):
        check_markdown(path, text, doctested)
    else:
        dated = (
            path.startswith(RUST_ROOTS)
            and "/tests/" not in "/" + path
            and not path.startswith("src/coverage_oracle/")
            and not TEST_FILE.search(os.path.basename(path))
        )
        check_rust(path, text, dated)
    checked += 1

for finding in findings:
    print(finding)
if findings:
    print(
        f"\n{len(findings)} finding(s). D-032 (DECISIONS.md) lists the homes and the "
        "exceptions; CONTRIBUTING.md has the ignore-reason convention.",
        file=sys.stderr,
    )
    sys.exit(1)
print(
    "No dated live fact outside its home, every ignore fence has a reason, and every "
    f"Rust block compiles or says why not ({checked} files)."
)
EOF
