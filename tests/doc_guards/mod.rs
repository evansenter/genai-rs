//! Guards that keep two docs facts true as the tree grows, for
//! `tests/ci_coverage.rs`.
//!
//! - **The test-kind table** at the top of `docs/TESTING.md` names every
//!   top-level test crate in exactly one row, under the right kind: a crate
//!   with a `#[ignore = "Requires API key"]` test in a row whose "Needs a
//!   key" cell says yes, every other crate in one that says no, and a guard
//!   (a crate that reads the repository and never names the library) in
//!   the Guards row. A new test file left out of the table fails.
//! - **Every guide is doctested.** Each `docs/*.md`, and each top-level
//!   Markdown file that holds a Rust block, is included in `src/lib.rs`'s
//!   `doc_tests` module, so its blocks compile in CI, unless
//!   [`NOT_DOCTESTED`] gives the reason it is not.
//!   `.github/scripts/check_dated_facts.sh` covers the other side: a Rust
//!   block in a Markdown file that is not doctested must be `ignore`d with
//!   a reason.
//!
//! Not a test target itself: Cargo only builds `tests/*.rs` and
//! `tests/*/main.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{LIVE_REASON, ignore_reasons, root, test_sources_in};

/// What the test-kind table needs to know about one top-level test crate.
#[derive(Debug, Clone)]
struct TestCrate {
    name: String,
    /// Has a test ignored with [`LIVE_REASON`].
    live: bool,
    /// Never names `genai_rs` (or the macros crate), nor drives trybuild:
    /// it reads the repository rather than exercising the library.
    guard: bool,
}

/// Whether Rust source `text` names any of `idents` as a token: comments
/// and string literals do not count, so a message that mentions
/// `genai_rs::DEFAULT_MODEL` is not a use of the crate.
fn names_ident(text: &str, idents: &[&str]) -> bool {
    use syn::buffer::Cursor;
    use syn::parse::Parser;

    fn walk(mut cursor: Cursor<'_>, idents: &[&str]) -> bool {
        while !cursor.eof() {
            if let Some((ident, next)) = cursor.ident() {
                if idents.iter().any(|name| ident == name) {
                    return true;
                }
                cursor = next;
            } else if let Some((inside, _, _, next)) = cursor.any_group() {
                if walk(inside, idents) {
                    return true;
                }
                cursor = next;
            } else if let Some((_, next)) = cursor.token_tree() {
                cursor = next;
            } else {
                break;
            }
        }
        false
    }

    let parser = |input: syn::parse::ParseStream<'_>| {
        input.step(|cursor| {
            let found = walk(*cursor, idents);
            let mut rest = *cursor;
            while let Some((_, next)) = rest.token_tree() {
                rest = next;
            }
            Ok((found, rest))
        })
    };
    parser
        .parse_str(text)
        .unwrap_or_else(|e| panic!("cannot tokenize a test source: {e}"))
}

/// Every top-level test crate in `dir`, classified from all its module files.
fn test_crates(dir: &Path) -> Vec<TestCrate> {
    test_sources_in(dir)
        .into_iter()
        .map(|(name, files)| {
            let texts: Vec<String> = files
                .iter()
                .map(|path| std::fs::read_to_string(path).expect("read test source"))
                .collect();
            let live = texts
                .iter()
                .any(|text| ignore_reasons(text).any(|reason| reason == LIVE_REASON));
            let uses_library = texts
                .iter()
                .any(|text| names_ident(text, &["genai_rs", "genai_rs_macros", "trybuild"]));
            TestCrate {
                name,
                live,
                guard: !uses_library,
            }
        })
        .collect()
}

/// One row of the test-kind table: its kind, whether it needs a key, and
/// the test crates its "Where" cell names.
#[derive(Debug)]
struct KindRow {
    kind: String,
    needs_key: bool,
    crates: Vec<String>,
}

/// The rows of the table under `## Test kinds` in `markdown`. A crate is a
/// backticked `tests/<name>.rs`, or a bare `<name>.rs` with no directory,
/// in the "Where" cell.
fn kind_rows(markdown: &str) -> Vec<KindRow> {
    let section = markdown
        .split("\n## Test kinds\n")
        .nth(1)
        .expect("TESTING.md has no `## Test kinds` section");
    let mut lines = section
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'));
    let header: Vec<String> = cells(lines.next().expect("the table has a header"));
    assert_eq!(
        header,
        ["Kind", "Where", "Needs a key", "Run with"],
        "the test-kind table's columns changed; update this guard"
    );
    lines
        .skip(1) // the |---| separator
        .map(|line| {
            let row = cells(line);
            assert_eq!(row.len(), 4, "a test-kind row without four cells: {line}");
            let needs_key = match row[2].split([' ', ',']).next() {
                Some("Yes") => true,
                Some("No") => false,
                _ => panic!("a \"Needs a key\" cell starts with neither Yes nor No: {line}"),
            };
            let crates = row[1]
                .split('`')
                .skip(1)
                .step_by(2)
                .filter_map(|code| {
                    let name = code.strip_prefix("tests/").unwrap_or(code);
                    let stem = name.strip_suffix(".rs")?;
                    (!stem.contains(['/', '*', '<'])).then(|| stem.to_string())
                })
                .collect();
            KindRow {
                kind: row[0].clone(),
                needs_key,
                crates,
            }
        })
        .collect()
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// What is wrong with how `rows` place `crates`: a crate in no row or in
/// two, a row naming a crate that does not exist, a live crate in a row
/// that needs no key (or the reverse), and a guard outside the Guards row
/// (or a crate that is not one inside it).
fn test_kind_problems(rows: &[KindRow], crates: &[TestCrate]) -> Vec<String> {
    let mut problems = Vec::new();
    let known: BTreeMap<&str, &TestCrate> = crates.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut placed: BTreeMap<&str, Vec<&KindRow>> = BTreeMap::new();
    for row in rows {
        for name in &row.crates {
            match known.get(name.as_str()) {
                Some(found) => placed.entry(found.name.as_str()).or_default().push(row),
                None => problems.push(format!(
                    "the {:?} row names tests/{name}.rs, which is not a top-level test file",
                    row.kind
                )),
            }
        }
    }
    for found in crates {
        let rows = placed.get(found.name.as_str()).map_or(&[][..], Vec::as_slice);
        let [row] = rows else {
            let kinds: Vec<&str> = rows.iter().map(|row| row.kind.as_str()).collect();
            problems.push(format!(
                "tests/{}.rs must be in exactly one row of the test-kind table, not {kinds:?}",
                found.name
            ));
            continue;
        };
        if found.live != row.needs_key {
            let what = if found.live {
                "has a live test"
            } else {
                "has no live test"
            };
            problems.push(format!(
                "tests/{}.rs {what}, but its row ({:?}) says it {}",
                found.name,
                row.kind,
                if row.needs_key {
                    "needs a key"
                } else {
                    "needs no key"
                }
            ));
        }
        let in_guards = row.kind == "Guards";
        if found.guard && !in_guards {
            problems.push(format!(
                "tests/{}.rs never names the library, so it is a guard; list it in the Guards row, not {:?}",
                found.name, row.kind
            ));
        }
        if !found.guard && in_guards {
            problems.push(format!(
                "tests/{}.rs exercises the library, so it is not a guard; move it out of the Guards row",
                found.name
            ));
        }
    }
    problems
}

/// Every top-level test crate is in `docs/TESTING.md`'s test-kind table,
/// under the kind it is.
#[test]
fn the_test_kind_table_places_every_test_file() {
    let testing = std::fs::read_to_string(root().join("docs/TESTING.md")).unwrap();
    let crates = test_crates(&root().join("tests"));
    assert!(
        crates.iter().filter(|c| c.live).count() > 10 && crates.iter().any(|c| c.guard),
        "the classification found too few live or guard crates: {crates:?}"
    );
    let problems = test_kind_problems(&kind_rows(&testing), &crates);
    assert!(
        problems.is_empty(),
        "docs/TESTING.md's test-kind table is out of date:\n  {}",
        problems.join("\n  ")
    );
}

/// The table check fails on each way the table can drift: a new file in no
/// row, a file in two, a live file under an offline kind (and the reverse),
/// a guard outside the Guards row, and a row naming a file that is gone.
#[test]
fn the_test_kind_check_finds_each_drift() {
    let table = "\
# T

## Test kinds

| Kind | Where | Needs a key | Run with |
|------|-------|-------------|----------|
| Offline | `tests/offline.rs`, `src/x_tests.rs`, `tests/common/` | No | `make test` |
| Guards | `tests/guard.rs` | No | `make test` |
| Live integration | In `tests/`: `live.rs` | Yes | `make test-all` |
";
    let crates = |extra: &[(&str, bool, bool)]| -> Vec<TestCrate> {
        [("offline", false, false), ("guard", false, true), ("live", true, false)]
            .iter()
            .chain(extra)
            .map(|&(name, live, guard)| TestCrate {
                name: name.to_string(),
                live,
                guard,
            })
            .collect()
    };
    let rows = kind_rows(table);
    assert_eq!(rows[0].crates, ["offline"]);
    assert_eq!(rows[2].crates, ["live"]);
    assert!(test_kind_problems(&rows, &crates(&[])).is_empty());

    let new = test_kind_problems(&rows, &crates(&[("added", false, false)]));
    assert!(
        matches!(&new[..], [one] if one.contains("tests/added.rs must be in exactly one row")),
        "{new:?}"
    );

    let cases = [
        (
            table.replace("`tests/offline.rs`", "`tests/offline.rs`, `tests/live.rs`"),
            "tests/live.rs must be in exactly one row",
        ),
        (
            table.replace("`live.rs`", "`live.rs`, `offline.rs`"),
            "tests/offline.rs must be in exactly one row",
        ),
        (
            table
                .replace("`tests/offline.rs`, ", "`tests/live.rs`, ")
                .replace(": `live.rs`", ": `offline.rs`"),
            "tests/live.rs has a live test, but its row (\"Offline\")",
        ),
        (
            table
                .replace("`tests/guard.rs`", "`tests/offline.rs`")
                .replace("`tests/offline.rs`, `src", "`tests/guard.rs`, `src"),
            "tests/guard.rs never names the library, so it is a guard",
        ),
        (
            table.replace("`live.rs`", "`live.rs`, `gone.rs`"),
            "names tests/gone.rs, which is not a top-level test file",
        ),
    ];
    for (drifted, expected) in cases {
        let problems = test_kind_problems(&kind_rows(&drifted), &crates(&[]));
        assert!(
            problems.iter().any(|p| p.contains(expected)),
            "expected {expected:?} in {problems:?}"
        );
    }
    let moved = table
        .replace("`tests/guard.rs`", "`tests/offline.rs`, `tests/guard.rs`")
        .replace("`tests/offline.rs`, `src", "`src");
    let problems = test_kind_problems(&kind_rows(&moved), &crates(&[]));
    assert!(
        problems
            .iter()
            .any(|p| p.contains("tests/offline.rs exercises the library")),
        "{problems:?}"
    );
}

/// A crate is classified from its tokens: a mention in a comment or a
/// string is not a use of the library.
#[test]
fn a_guard_is_a_crate_that_never_names_the_library() {
    let guard = "//! Reads genai_rs's sources.\nfn f() { let _ = \"genai_rs::DEFAULT_MODEL\"; }\n";
    assert!(!names_ident(guard, &["genai_rs"]));
    for user in [
        "use genai_rs::Client;",
        "fn f() { assert!(matches!(x, genai_rs::Kind::A)); }",
        "#[test] fn t() { let t = trybuild::TestCases::new(); }",
    ] {
        assert!(names_ident(user, &["genai_rs", "trybuild"]), "{user}");
    }
}

/// Markdown files that hold Rust blocks but are not in `doc_tests`, each
/// with its reason. `check_dated_facts.sh` requires each of their Rust
/// blocks to be `ignore`d with a reason, except `CHANGELOG.md`'s.
const NOT_DOCTESTED: &[(&str, &str)] = &[
    (
        "docs/INTERACTIONS_API_GAP.md",
        "A maintainer note: Cargo.toml's include leaves it out of the published crate, and it holds no Rust.",
    ),
    (
        "docs/ANTIGRAVITY_BRIDGE_DESIGN.md",
        "A maintainer note, left out of the published crate; its one Rust block is a sketch of a wire type.",
    ),
    (
        "CHANGELOG.md",
        "History: each block shows the API of the release it describes, so old ones cannot build against today's crate.",
    ),
    (
        "CLAUDE.md",
        "Guidance for agents working in the repository, not a guide for users; its one Rust block is a pattern.",
    ),
];

/// The repository-relative Markdown files `lib_rs`'s `doc_tests` module
/// includes, by their `include_str!("../<path>")`.
fn doctested(lib_rs: &str) -> BTreeSet<String> {
    let module = lib_rs
        .split("mod doc_tests {")
        .nth(1)
        .expect("src/lib.rs has no doc_tests module");
    module
        .split("include_str!(\"../")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|path| path.ends_with(".md"))
        .map(str::to_string)
        .collect()
}

/// Whether Markdown `text` holds a block rustdoc would read as Rust: a
/// fence whose info string starts with `rust`.
fn holds_rust(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        ["```", "~~~"].iter().any(|fence| {
            line.strip_prefix(fence)
                .map(|info| info.trim_start_matches(['`', '~']).trim_start())
                .is_some_and(|info| {
                    info.split([',', ' ', '\t'])
                        .next()
                        .is_some_and(|first| first == "rust")
                })
        })
    })
}

/// The guides `doc_tests` misses: every `docs/*.md`, and every top-level
/// Markdown file holding Rust, among `files` (repository-relative path and
/// text), unless `exempt` names it; and every exemption that no longer
/// applies, because `doc_tests` includes the file or the file is gone.
fn undoctested(lib_rs: &str, files: &[(String, String)], exempt: &[(&str, &str)]) -> Vec<String> {
    let included = doctested(lib_rs);
    let exempt: BTreeSet<&str> = exempt.iter().map(|(path, _)| *path).collect();
    let mut problems = Vec::new();
    for (path, text) in files {
        let guide = path.starts_with("docs/") && !path["docs/".len()..].contains('/');
        let top_level = !path.contains('/') && holds_rust(text);
        if (guide || top_level) && !included.contains(path) && !exempt.contains(path.as_str()) {
            problems.push(format!(
                "{path} is not in src/lib.rs's doc_tests module, so its Rust blocks never \
                 compile: add `#[doc = include_str!(\"../{path}\")]` there, or list it in \
                 NOT_DOCTESTED with the reason"
            ));
        }
    }
    let present: BTreeSet<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
    for path in exempt {
        if included.contains(path) || !present.contains(path) {
            problems.push(format!(
                "NOT_DOCTESTED lists {path}, which is doctested or gone; drop the entry"
            ));
        }
    }
    for path in &included {
        if !present.contains(path.as_str()) {
            problems.push(format!("doc_tests includes {path}, which is not here"));
        }
    }
    problems
}

/// The Markdown files a guide could be: `docs/*.md` and the top level's.
fn guide_candidates(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for (dir, prefix) in [(root.to_path_buf(), ""), (root.join("docs"), "docs/")] {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "md") && path.is_file() {
                let name = path.file_name().unwrap().to_string_lossy();
                files.push((
                    format!("{prefix}{name}"),
                    std::fs::read_to_string(&path).unwrap(),
                ));
            }
        }
    }
    files.sort();
    files
}

/// Every guide's Rust blocks compile in the doctest run: each `docs/*.md`
/// but the maintainer notes, and each top-level Markdown file with Rust, is
/// in `doc_tests`.
#[test]
fn every_guide_is_doctested() {
    let lib_rs = std::fs::read_to_string(root().join("src/lib.rs")).unwrap();
    let files = guide_candidates(&root());
    assert!(
        doctested(&lib_rs).len() > 20,
        "found too few doc_tests includes"
    );
    let problems = undoctested(&lib_rs, &files, NOT_DOCTESTED);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The doctest check fails on a new guide left out, a top-level file with
/// Rust left out, and a stale exemption; it passes a top-level file with no
/// Rust and an exempt note.
#[test]
fn the_doctest_check_finds_a_guide_left_out() {
    let lib_rs = "#[cfg(doctest)]\nmod doc_tests {\n    #[doc = include_str!(\"../README.md\")]\n    mod readme {}\n    #[cfg(feature = \"x\")]\n    #[doc = include_str!(\"../docs/GUIDE.md\")]\n    mod guide {}\n}\n";
    let file = |path: &str, text: &str| (path.to_string(), text.to_string());
    let files = vec![
        file("README.md", "```rust\nfn main() {}\n```\n"),
        file("docs/GUIDE.md", "# Guide\n"),
        file("docs/NOTES.md", "# Notes\n"),
        file("docs/NEW.md", "# New\n"),
        file("HISTORY.md", "```rust,no_run\nlet x = 1;\n```\n"),
        file("SHELL.md", "```bash\nls\n```\n"),
        file("docs/sub/DEEP.md", "```rust\n```\n"),
    ];
    let exempt = [("docs/NOTES.md", "a note"), ("docs/GUIDE.md", "stale")];
    let problems = undoctested(lib_rs, &files, &exempt);
    let flagged = |needle: &str| problems.iter().any(|p| p.starts_with(needle));
    assert!(flagged("docs/NEW.md is not in"), "{problems:?}");
    assert!(flagged("HISTORY.md is not in"), "{problems:?}");
    assert!(
        flagged("NOT_DOCTESTED lists docs/GUIDE.md"),
        "{problems:?}"
    );
    assert_eq!(problems.len(), 3, "{problems:?}");
}
