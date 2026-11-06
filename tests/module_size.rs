//! Guards D-014's size bars, so a file that outgrows its home fails CI
//! instead of waiting for the next audit to notice (#504).
//!
//! Measured the way D-014 states them:
//!
//! - an implementation module in `src/` or `genai-rs-macros/src/`: its lines
//!   outside its own `#[cfg(test)]` test modules (`mod tests` or
//!   `mod <name>_tests`, inline or out of line), at most 800. Every test
//!   module sits at the end of its file today, so this is the "lines before
//!   its test module" D-014 states; one declared earlier does not hide the
//!   code after it;
//! - a unit-test file in `src/` (`*_tests.rs`, `tests.rs`, and the property
//!   tests `proptest_*.rs`): every line, at most 1,500;
//! - the shared test modules `tests/common/` and `tests/live_verbs/`: every
//!   line, at most 800;
//! - every other file under `tests/` (the integration tests and their
//!   modules): every line, at most 2,000.
//!
//! A file over its bar splits along its seams, by the feature its code or
//! tests cover (D-008), as a pure move recorded in `DECISIONS.md` (D-011).
//! There is no allowlist: every file was under its bar when this landed
//! (2026-11-06). A file that must stay over would need an entry here with
//! the reason.

use std::path::{Path, PathBuf};

/// Which bar a file is held to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Implementation,
    UnitTest,
    SharedTestModule,
    IntegrationTest,
}

impl Kind {
    fn limit(self) -> usize {
        match self {
            Kind::Implementation | Kind::SharedTestModule => 800,
            Kind::UnitTest => 1_500,
            Kind::IntegrationTest => 2_000,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Kind::Implementation => "an implementation module, counted before its test module",
            Kind::UnitTest => "a unit-test file",
            Kind::SharedTestModule => "a shared test module",
            Kind::IntegrationTest => "an integration-test file",
        }
    }
}

/// The bar for a repo-relative path (with `/` separators).
fn classify(rel: &str) -> Kind {
    if rel.starts_with("tests/common/") || rel.starts_with("tests/live_verbs/") {
        return Kind::SharedTestModule;
    }
    if rel.starts_with("tests/") {
        return Kind::IntegrationTest;
    }
    let file = rel.rsplit('/').next().unwrap_or(rel);
    if file.ends_with("_tests.rs") || file == "tests.rs" || file.starts_with("proptest_") {
        Kind::UnitTest
    } else {
        Kind::Implementation
    }
}

/// The lines a file is measured by: for an implementation module, all but
/// its test modules' (each from its `#[cfg(test)]` through its `mod` line,
/// or through the closing brace of an inline one); for any other file, all.
fn measured_lines(kind: Kind, text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    if kind != Kind::Implementation {
        return lines.len();
    }
    let mut counted = 0;
    let mut at = 0;
    while at < lines.len() {
        match test_module_end(&lines, at) {
            Some(end) => at = end + 1,
            None => {
                counted += 1;
                at += 1;
            }
        }
    }
    counted
}

/// If a top-level `#[cfg(test)]` at `start` gates a module named `tests` or
/// `<name>_tests`, the index of that module's last line: its `mod` line when
/// declared out of line, its column-0 closing brace when inline. Other
/// test-gated items, such as a helper or a test-support child module like
/// `test_subscriber`, count as the module's own code.
fn test_module_end(lines: &[&str], start: usize) -> Option<usize> {
    if !lines[start].starts_with("#[cfg(test)]") {
        return None;
    }
    let offset = lines[start + 1..]
        .iter()
        .position(|line| !line.starts_with("#[") && !line.starts_with("///"))?;
    let mod_line = start + 1 + offset;
    let item = lines[mod_line];
    let item = item
        .strip_prefix("pub(crate) ")
        .or_else(|| item.strip_prefix("pub(super) "))
        .or_else(|| item.strip_prefix("pub "))
        .unwrap_or(item);
    let rest = item.strip_prefix("mod ")?;
    let name = rest.trim_end_matches([';', '{', '}', ' ']);
    if name != "tests" && !name.ends_with("_tests") {
        return None;
    }
    if rest.ends_with(';') || rest.ends_with('}') {
        return Some(mod_line);
    }
    // Inline: rustfmt closes a top-level module with `}` in column 0.
    lines[mod_line + 1..]
        .iter()
        .position(|line| *line == "}")
        .map(|close| mod_line + 1 + close)
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// `(repo-relative path, kind, measured lines)` for every file the bars
/// cover.
fn measure() -> Vec<(String, Kind, usize)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "genai-rs-macros/src", "tests"] {
        let dir = root.join(dir);
        // A root that cannot be read must not read as a tree that is small.
        assert!(dir.is_dir(), "scan root {} does not exist", dir.display());
        rust_files(&dir, &mut files);
    }
    files
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("could not read {rel}: {e}"));
            let kind = classify(&rel);
            let lines = measured_lines(kind, &text);
            (rel, kind, lines)
        })
        .collect()
}

#[test]
fn modules_and_test_files_stay_under_their_size_bars() {
    let measured = measure();
    for kind in [
        Kind::Implementation,
        Kind::UnitTest,
        Kind::SharedTestModule,
        Kind::IntegrationTest,
    ] {
        let found = measured.iter().filter(|(_, k, _)| *k == kind).count();
        assert!(found > 3, "found only {found} files of kind {kind:?}");
    }
    let over: Vec<String> = measured
        .iter()
        .filter(|(_, kind, lines)| *lines > kind.limit())
        .map(|(rel, kind, lines)| {
            format!(
                "{rel}: {lines} lines, over the {} for {} (D-014)",
                kind.limit(),
                kind.describe()
            )
        })
        .collect();
    assert!(
        over.is_empty(),
        "Over the size bar:\n  {}\n\nSplit each along its seams, by the feature its code \
         or tests cover (D-008), as a pure move, and record the move in DECISIONS.md \
         (D-011). See D-014.",
        over.join("\n  ")
    );
}

#[test]
fn files_are_held_to_the_bar_of_their_kind() {
    for (rel, kind) in [
        ("src/steps/mod.rs", Kind::Implementation),
        ("src/lib.rs", Kind::Implementation),
        ("src/coverage_oracle/response.rs", Kind::Implementation),
        ("genai-rs-macros/src/lib.rs", Kind::Implementation),
        ("src/steps/accumulator_tests.rs", Kind::UnitTest),
        ("src/request_builder/tests.rs", Kind::UnitTest),
        ("src/proptest_steps.rs", Kind::UnitTest),
        ("tests/common/mod.rs", Kind::SharedTestModule),
        ("tests/live_verbs/bodies.rs", Kind::SharedTestModule),
        ("tests/antigravity_harness.rs", Kind::IntegrationTest),
        (
            "tests/http_mock_resources/webhooks.rs",
            Kind::IntegrationTest,
        ),
    ] {
        assert_eq!(classify(rel), kind, "{rel}");
    }
    let measured = measure();
    for expected in [
        "src/steps/mod.rs",
        "tests/common/mod.rs",
        "src/proptest_steps.rs",
    ] {
        assert!(
            measured.iter().any(|(rel, _, _)| rel == expected),
            "{expected} was not measured"
        );
    }
}

#[test]
fn an_implementation_module_is_counted_without_its_test_modules() {
    let inline =
        "fn a() {}\nfn b() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
    assert_eq!(measured_lines(Kind::Implementation, inline), 3);

    let out_of_line = "fn a() {}\n\n#[cfg(test)]\n#[path = \"a_tests.rs\"]\nmod tests;\n";
    assert_eq!(measured_lines(Kind::Implementation, out_of_line), 2);

    // A sibling `<name>_tests` module counts as the test module too.
    let named = "fn a() {}\n#[cfg(test)]\nmod serialize_tests;\n#[cfg(test)]\nmod tests;\n";
    assert_eq!(measured_lines(Kind::Implementation, named), 1);

    // Test-gated items that are not its test module are the module's code.
    let helpers = "#[cfg(test)]\npub(crate) mod test_subscriber;\n#[cfg(test)]\nfn helper() {}\nfn a() {}\n#[cfg(test)]\nmod tests;\n";
    assert_eq!(measured_lines(Kind::Implementation, helpers), 5);

    // An indented `#[cfg(test)]` is inside another item, not the module's.
    let nested = "mod inner {\n    #[cfg(test)]\n    mod tests;\n}\n";
    assert_eq!(measured_lines(Kind::Implementation, nested), 4);

    // A test module declared early does not hide the code after it.
    let early = "#[cfg(test)]\nmod tests;\nfn a() {}\nfn b() {}\n";
    assert_eq!(measured_lines(Kind::Implementation, early), 2);
    let early_inline =
        "use x;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn a() {}\nfn b() {}\n";
    assert_eq!(measured_lines(Kind::Implementation, early_inline), 3);
    // Lines after an out-of-line test module at the end still count.
    let trailing = "fn a() {}\n#[cfg(test)]\nmod tests;\n#[cfg(doctest)]\nmod readme {}\n";
    assert_eq!(measured_lines(Kind::Implementation, trailing), 3);

    // No test module: every line counts.
    assert_eq!(
        measured_lines(Kind::Implementation, "fn a() {}\nfn b() {}\n"),
        2
    );

    // Test files count whole, test modules included.
    assert_eq!(measured_lines(Kind::UnitTest, inline), 8);
    assert_eq!(measured_lines(Kind::IntegrationTest, inline), 8);
}
