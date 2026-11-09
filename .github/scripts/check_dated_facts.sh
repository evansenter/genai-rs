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
#    docs/ANTIGRAVITY_BRIDGE_DESIGN.md. A date is ISO (`2026-11-05`,
#    `2026-11-5`, or a month, `2026-07`), slashed (`2026/11/05`,
#    `11/05/2026`), dotted (`2026.11.05`, `05.11.2026`), dashed day last
#    (`11-05-2026`), or written with the month named (`November 5, 2026`,
#    `Nov 5, 2026`, `5 November 2026`, `November 2026`), on one line or
#    wrapped across two. D-032's exceptions are not read: DECISIONS.md and
#    CHANGELOG.md (history), any file under a `tests/` directory (each
#    test's evidence, fixtures included), and in Rust everything but the
#    comments of non-test code: the doc comments in src/ and
#    genai-rs-macros/src/, where a `//` comment beside the code an
#    observation justifies may carry its date, and every comment in
#    examples/ (examples/CLAUDE.md). Test code is a `*_tests.rs`,
#    `tests.rs`, `proptest_*.rs` or `test_subscriber.rs` file,
#    src/coverage_oracle/, and an item behind a `#[cfg(test)]` line. Not
#    dates: a timestamp, a date followed by a time with seconds
#    (`2026-01-01T10:30:45Z`, `21 Oct 2015 07:28:00 GMT`: example data); a
#    link's target (a home's heading anchor); and the API revision
#    `2026-05-20`, a version name. Not read: a date without a year
#    (`Nov 5`) or with two digits (`'26`).
# 2. A code block fenced `ignore` (`rust,ignore`, or `rust ignore`: rustdoc
#    splits the info string on commas, spaces and tabs) has a
#    `<!-- ignore: reason -->` line directly above its fence: in any
#    Markdown file, and in any doc comment of any Rust file, test code and
#    `tests/` included.
# 3. A Rust block that nothing compiles is `ignore`d, with rule 2's reason.
#    Rustdoc compiles the Markdown files src/lib.rs includes as docs
#    (`#[doc = include_str!("../<path>")]` in code, not in a comment, and
#    under no `cfg` that CI's doctest run, a debug build with every feature
#    on Linux, leaves out) and the doc comments of the library crates (src/
#    and genai-rs-macros/src/) outside test code and modules compiled only
#    under such a `cfg` (`#[cfg(test)] mod x;`). Anywhere else
#    (tests/, examples/, fuzz/, a Markdown file not included), a block
#    rustdoc would read as Rust (a fence whose info string names `rust` or
#    a rustdoc attribute such as `no_run` or `edition2021`, or, in a doc
#    comment, an untagged one) never compiles. History is exempt:
#    CHANGELOG.md's blocks show the API of the release each entry
#    describes. A fence tagged `rs` or `Rust` is Rust to GitHub but another
#    language to rustdoc, which never compiles it, so it fails everywhere:
#    tag it `rust`. `tests/doc_guards/` requires every guide to be
#    included.
# 4. A `text` block (also `txt` or `plain`, and an untagged one in a
#    Markdown file rule 3 reads) holds no Rust: a line that is a `let`
#    (single or multi-line, destructuring, or `let ... else`) or `use`
#    statement, a field assignment, a `fn`, `struct`, `enum`, `trait` or
#    `impl` header, an `if let`, `while let` or `match` header with its
#    arms, or a `#[derive]`, `#[test]` or `#[tokio::main]` attribute. Fence
#    the block as rust so it compiles. A bare expression is not read.
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
# The library crates, whose doc comments rustdoc compiles.
DOCTESTED_ROOTS = ("src/", "genai-rs-macros/src/")

MONTH = (
    r"(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|June?|July?|Aug(?:ust)?"
    r"|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)"
)
DAY = r"(?:0?[1-9]|[12]\d|3[01])"
MONTH_NUM = r"(?:0?[1-9]|1[0-2])"
DAY_SUFFIX = r"(?:st|nd|rd|th)?"
# A time with seconds after a date makes it a timestamp, example data rather
# than a fact. A time without seconds is how prose dates an observation.
NOT_TIME = r"(?!\s*,?\s*\d{1,2}:\d\d:\d\d)"
DATE = re.compile(
    # ISO: 2026-11-05 (or 2026-11-5), or the month 2026-07.
    rf"(?<![\w.-])20\d\d-(?:(?:0[1-9]|1[0-2])(?!-\d)|{MONTH_NUM}-{DAY})(?![\w-]|T\d){NOT_TIME}"
    # Slashed: 2026/11/05, and 11/05/2026 or 05/11/2026.
    rf"|(?<![\w/.])20\d\d/{MONTH_NUM}/{DAY}(?![\w/]){NOT_TIME}"
    rf"|(?<![\w/.]){DAY}/{DAY}/20\d\d(?![\w/]){NOT_TIME}"
    # Dotted: 2026.11.05, and 05.11.2026; dashed with the year last:
    # 11-05-2026.
    rf"|(?<![\w.])20\d\d\.{MONTH_NUM}\.{DAY}(?!\w|\.\d){NOT_TIME}"
    rf"|(?<![\w.]){DAY}\.{DAY}\.20\d\d(?!\w|\.\d){NOT_TIME}"
    rf"|(?<![\w-]){DAY}-{DAY}-20\d\d(?![\w-]){NOT_TIME}"
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
# One attribute, with one level of nested brackets, and a run of them.
ATTR = r'#\[(?:[^\[\]"]|"(?:\\.|[^"\\])*"|\[(?:[^\[\]"]|"(?:\\.|[^"\\])*")*\])*\]'
ATTR_RUN = re.compile(rf"(?:{ATTR}\s*)+")
ATTR_ONE = re.compile(ATTR)
# What src/lib.rs includes as docs, and so doctests.
DOC_INCLUDE = re.compile(r'#\[doc\s*=\s*include_str!\(\s*"\.\./([^"]+\.md)"\s*\)\s*\]')
CFG_ATTR = re.compile(r"#\[\s*cfg\s*\((.*)\)\s*\]$", re.S)
CFG_TOKEN = re.compile(r'\s*(?:([A-Za-z_]\w*)|("(?:\\.|[^"\\])*")|(\S))')
# What holds in CI's doctest run (see cfg_holds).
TRUE_CFGS = {"doctest", "unix", "debug_assertions"}
TRUE_CFG_VALUES = {("target_os", '"linux"'), ("target_family", '"unix"')}
PATH_ATTR = re.compile(r'#\[\s*path\s*=\s*"([^"]+)"\s*\]')
# A file module declaration after its attributes.
MOD_DECL = re.compile(rf"((?:{ATTR}\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
RAW_STRING = re.compile(r'(?<!\w)b?r(#*)"')
CHAR_LITERAL = re.compile(r"b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^\\'\n])'")
# Info-string tokens that make rustdoc read a block as Rust.
RUSTDOC_ATTRS = {"rust", "no_run", "should_panic", "compile_fail", "test_harness", "standalone_crate"}
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
# Rule 4's statements that span lines: a `let` whose `;` comes on a later
# line (a builder chain), a destructuring `let` or a `let ... else`, and a
# header (`if let`, `while let`, `match`) whose body follows.
LET_OPEN = re.compile(
    r"^\s*let\s+(?:mut\s+)?(?:[a-z_]\w*|\(.*\)|[A-Z]\w*(?:::\w+)*\s*[({].*?[)}])"
    r"\s*(?::\s*[^=;]+)?=(?!=)"
)
ENDS_STATEMENT = re.compile(r";\s*(?://.*)?$")
LET_HEADER = re.compile(r"^\s*(?:if|while)\s+let\s+\S.*=.*\{\s*$")
MATCH_HEADER = re.compile(r"^\s*match\s+[^{}]+\{\s*$")
MATCH_ARM = re.compile(r"^\s*[^=\s][^=]*=>")

findings = []


def strip_comments(src):
    """Rust source with its comments blanked (their newlines kept), so a
    commented-out attribute is not read. String, raw-string and char
    literals are kept whole."""
    out = []
    i, n = 0, len(src)
    while i < n:
        if src.startswith("//", i):
            end = src.find("\n", i)
            i = n if end < 0 else end
            continue
        if src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append("\n" * src.count("\n", i, j))
            i = j
            continue
        m = RAW_STRING.match(src, i)
        if m:
            close = src.find('"' + m.group(1), m.end())
            end = n if close < 0 else close + 1 + len(m.group(1))
            out.append(src[i:end])
            i = end
            continue
        if src[i] == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            out.append(src[i : j + 1])
            i = j + 1
            continue
        m = CHAR_LITERAL.match(src, i) if src[i] in "'b" else None
        if m:
            out.append(m.group(0))
            i = m.end()
            continue
        out.append(src[i])
        i += 1
    return "".join(out)


def cfg_holds(predicate):
    """Whether a `cfg(...)` predicate holds in CI's doctest run, a debug
    build with every feature on Linux: `doctest`, `unix`,
    `debug_assertions`, every `feature = "..."`, `target_os = "linux"` and
    `target_family = "unix"` do; any other name (`test`, `fuzzing`, `any()`
    with nothing in it) does not."""
    tokens = [m.group(1) or m.group(2) or m.group(3) for m in CFG_TOKEN.finditer(predicate)]
    tokens = [t for t in tokens if t]

    def parse(i):
        name = tokens[i]
        if name in ("all", "any", "not") and tokens[i + 1] == "(":
            i += 2
            values = []
            while tokens[i] != ")":
                value, i = parse(i)
                values.append(value)
                if tokens[i] == ",":
                    i += 1
            i += 1
            if name == "all":
                return all(values), i
            if name == "any":
                return any(values), i
            return not values[0], i
        if i + 1 < len(tokens) and tokens[i + 1] == "=":
            value = tokens[i + 2]
            return (name, value) in TRUE_CFG_VALUES or name == "feature", i + 3
        return name in TRUE_CFGS, i + 1

    try:
        value, end = parse(0)
    except IndexError:
        return False
    return value and end == len(tokens)


def attrs_hold(run):
    """Whether every `cfg` in an attribute run holds in the doctest run."""
    for attr in ATTR_ONE.findall(run):
        m = CFG_ATTR.match(attr)
        if m and not cfg_holds(m.group(1)):
            return False
    return True


def doc_includes(lib_rs):
    """The Markdown files `lib_rs` includes as docs in code that the
    doctest run compiles."""
    included = set()
    for run in ATTR_RUN.finditer(strip_comments(lib_rs)):
        paths = DOC_INCLUDE.findall(run.group(0))
        if paths and attrs_hold(run.group(0)):
            included.update(paths)
    return included


def gated_modules(path, text):
    """Path prefixes of the modules `path` declares under a `cfg` the
    doctest run leaves out (`#[cfg(test)] mod x;`): their doc comments are
    never doctested."""
    here = os.path.dirname(path)
    base = os.path.basename(path)
    own = here if base in ("lib.rs", "main.rs", "mod.rs") else os.path.join(here, base[:-3])
    gated = []
    for m in MOD_DECL.finditer(strip_comments(text)):
        if attrs_hold(m.group(1)):
            continue
        found = PATH_ATTR.search(m.group(1))
        if found:
            file = os.path.normpath(os.path.join(here, found.group(1)))
        else:
            file = os.path.join(own, m.group(2) + ".rs")
        gated.append(file)
        # Its own modules: beside a mod.rs, under its stem otherwise.
        gated.append(os.path.join(own, m.group(2)) + "/")
        gated.append(file[: -len(".rs")] + "/")
    return gated


def clean(text):
    """`text` without what holds a date that is not a fact: link targets
    and URLs."""
    text = LINK_DEFINITION.sub(r"\1", text)
    for pattern in (LINK_TARGET, AUTOLINK, BARE_URL):
        text = pattern.sub("", text)
    return text


def dates(text):
    return [d for d in DATE.findall(clean(text)) if d not in NAMES]


def wrapped_dates(first, second):
    """Dates that start on line `first` and end on line `second`: prose is
    hard-wrapped, and a named-month date has spaces to break at."""
    head = clean(first).rstrip()
    joined = head + " " + clean(second).lstrip()
    return [
        m.group(0)
        for m in DATE.finditer(joined)
        if m.start() < len(head) < m.end() and m.group(0) not in NAMES
    ]


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


def rust_in_text(body):
    """The index in `body` of the first line rule 4 reads as Rust, or None."""
    for k, line in enumerate(body):
        if RUST_STATEMENT.match(line) or RUST_ITEM.match(line) or LET_HEADER.match(line):
            return k
        if LET_OPEN.match(line) and any(ENDS_STATEMENT.search(rest) for rest in body[k:]):
            return k
        if MATCH_HEADER.match(line) and any(MATCH_ARM.match(rest) for rest in body[k + 1 :]):
            return k
    return None


def check_fence(path, n, event, prev, doc_line, compiled):
    """Rules 2 to 4 for one fence event in `path`. `doc_line`: the fence is
    in a doc comment, where rustdoc reads an untagged block as Rust.
    `compiled`: whether rustdoc compiles a Rust block here."""
    if event is None:
        return
    if event[0] == "open":
        tokens = event[1]
        if "ignore" in tokens and not REASON.match(prev or ""):
            where = "doc line" if doc_line else "line"
            findings.append(
                f"{path}:{n}: an ignore fence needs a `<!-- ignore: reason -->` {where} directly above it"
            )
        aliases = [t for t in tokens if t.lower() in ("rs", "rust") and t != "rust"]
        if aliases:
            findings.append(
                f"{path}:{n}: a `{aliases[0]}` fence is Rust to GitHub but another language to "
                "rustdoc, which never compiles it: fence the block as `rust`"
            )
            return
        rusty = (
            RUSTDOC_ATTRS.intersection(tokens)
            or any(t.startswith("edition") for t in tokens)
            or (doc_line and not tokens)
        )
        if not compiled and "ignore" not in tokens and rusty:
            if doc_line:
                findings.append(
                    f"{path}:{n}: a Rust block in this doc comment never compiles, because rustdoc "
                    "doctests only the library crates' non-test code: mark it `ignore` with a "
                    "`<!-- ignore: reason -->` doc line above it, or fence it as `text` if it is "
                    "not Rust"
                )
            else:
                findings.append(
                    f"{path}:{n}: a Rust block here never compiles, because src/lib.rs does not "
                    f"include {path} in its doc_tests: include the file there, or mark the block "
                    "`ignore` with a `<!-- ignore: reason -->` line above it"
                )
        return
    _, tokens, start, body = event
    texty = (tokens[:1] and tokens[0] in TEXT_INFOS) or (not tokens and not compiled and not doc_line)
    if texty:
        k = rust_in_text(body)
        if k is not None:
            findings.append(
                f"{path}:{start + 1 + k}: Rust in a text block never compiles: fence the block "
                f"(line {start}) as rust, or as `rust,ignore` with a reason"
            )


def dated_fact(path, n, d, where=""):
    if where == "doc":
        findings.append(
            f"{path}:{n}: dated live fact ({d}) in a doc comment: state the current behavior "
            "without the date, and keep the dated evidence in a home or in a `//` comment "
            "beside the code it justifies (D-032)"
        )
    elif where == "example":
        findings.append(
            f"{path}:{n}: dated live fact ({d}) in an example: state what the API "
            "does now and keep the dated observation in a home (D-032)"
        )
    else:
        findings.append(
            f"{path}:{n}: dated live fact ({d}) outside its home: record it in "
            "docs/ENUM_WIRE_FORMATS.md or docs/INTERACTIONS_API_GAP.md (Antigravity: "
            "docs/ANTIGRAVITY_BRIDGE_DESIGN.md) and state the current behavior here (D-032)"
        )


def check_markdown(path, text, doctested):
    dated = path not in HOMES and path not in HISTORY and "/tests/" not in "/" + path
    compiled = path in doctested or path in HISTORY
    fences = Fences()
    prev = ""
    for n, line in enumerate(text.splitlines(), 1):
        check_fence(path, n, fences.step(line, n), prev, False, compiled)
        if dated:
            for d in wrapped_dates(prev, line) if n > 1 else []:
                dated_fact(path, n - 1, d)
            for d in dates(line):
                dated_fact(path, n, d)
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


def check_rust(path, text, dated, doctested_file):
    """Rules 2 to 4 in every doc comment; rule 1, when `dated`, in the
    comments of non-test code. `doctested_file`: rustdoc compiles the doc
    comments here, outside its test items."""
    every_comment = path.startswith("examples/")
    lines = text.splitlines()
    tests = test_items(lines)
    skip = tests if dated else set(range(len(lines)))
    fences = Fences()
    prev = None  # the previous doc line's text; None outside a doc block
    carry = None  # (line number, kind, text) of the last comment line rule 1 read
    for n, line in enumerate(lines, 1):
        m = COMMENT.match(line)
        doc = bool(m and m.group(1))
        if not doc:
            prev = None
            fences = Fences()
        else:
            compiled = doctested_file and n - 1 not in tests
            check_fence(path, n, fences.step(m.group(2), n), prev, True, compiled)
            prev = m.group(2)
        kind = "doc" if doc else "example" if m and every_comment else None
        if kind is None or n - 1 in skip:
            carry = None
            continue
        if carry and carry[1] == kind:
            for d in wrapped_dates(carry[2], m.group(2)):
                dated_fact(path, carry[0], d, kind)
        for d in dates(m.group(2)):
            dated_fact(path, n, d, kind)
        carry = (n, kind, m.group(2))


def read(path):
    with open(os.path.join(root, path), encoding="utf-8") as f:
        return f.read()


listed = subprocess.run(
    ["git", "-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard",
     "--", "*.md", "*.rs"],
    check=True,
    capture_output=True,
).stdout.decode()
paths = sorted({p for p in listed.split("\0") if p and os.path.isfile(os.path.join(root, p))})
doctested = set()
if os.path.isfile(os.path.join(root, "src", "lib.rs")):
    doctested = doc_includes(read("src/lib.rs"))
gated = []
for path in paths:
    if path.endswith(".rs") and path.startswith(DOCTESTED_ROOTS):
        gated.extend(gated_modules(path, read(path)))
checked = 0
for path in paths:
    text = read(path)
    if path.endswith(".md"):
        check_markdown(path, text, doctested)
    else:
        test_file = (
            "/tests/" in "/" + path
            or path.startswith("src/coverage_oracle/")
            or TEST_FILE.search(os.path.basename(path))
        )
        dated = path.startswith(RUST_ROOTS) and not test_file
        doctested_file = (
            path.startswith(DOCTESTED_ROOTS)
            and not test_file
            and not any(path == g or (g.endswith("/") and path.startswith(g)) for g in gated)
        )
        check_rust(path, text, dated, doctested_file)
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
