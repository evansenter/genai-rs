//! Guards the single-source-of-truth for model ids.
//!
//! Lives in its own test binary rather than `tests/common`, which is
//! compiled into every integration test crate and would run this scan
//! a dozen times per suite.

// =============================================================================

/// Fails if a Gemini model id is hardcoded outside the crate's model
/// constants.
///
/// Bumping the default model used to mean editing ~600 occurrences across
/// tests, examples, docs and `src`. A sweep that size reliably misses a
/// few, and a missed one is invisible: the test keeps passing against a
/// model nobody meant to still be using, until that model is retired and
/// the failure arrives with no obvious cause.
///
/// So the constants in `src/lib.rs` are the only place a model id is
/// written down, and this test keeps it that way. It scans the source
/// tree rather than relying on review to notice.
///
/// Deliberately not scanning `docs/` or `*.md`: prose names real models on
/// purpose (capability notes, migration history), and a constant cannot be
/// interpolated into markdown anyway.
#[test]
fn no_hardcoded_model_ids_outside_the_constants() {
    use std::path::Path;

    // A quoted id followed by a digit: matches "gemini-3.7-flash" but not
    // an unrelated identifier like "gemini-base". The optional `models/`
    // covers the resource-name form the Files and Interactions APIs use
    // ("models/gemini-3.7-flash"), which the opening-quote anchor would
    // otherwise let past — nothing in the repo writes one today, and this
    // keeps it that way.
    static MODEL_LITERAL: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#""(models/)?gemini-\d"#).unwrap());

    /// Where a model id may legitimately appear as a literal.
    ///
    /// Matched as exact repo-relative paths, not suffixes: `src/lib.rs`
    /// must not also exempt some future nested `.../src/lib.rs`.
    const ALLOWED: &[&str] = &[
        // The constants themselves.
        "src/lib.rs",
        // This file: its own comment names a model to explain the pattern.
        "tests/model_literals.rs",
    ];

    fn scan(root: &Path, dir: &Path, hits: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(root, &path, hits);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if ALLOWED.contains(&rel.as_str()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                // A quoted id, so `genai_rs::DEFAULT_MODEL` does not match.
                if MODEL_LITERAL.is_match(line) {
                    hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                }
            }
        }
    }

    // The root crate *is* the workspace root, so `target/` sits beside
    // these and is never walked.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hits = Vec::new();
    scan(root, &root.join("src"), &mut hits);
    scan(root, &root.join("tests"), &mut hits);
    scan(root, &root.join("examples"), &mut hits);
    // The other workspace member. Clean today, but it has its own src/ and
    // tests/ (trybuild fixtures included), and a literal pinned there would
    // be exactly the invisible one this test exists to catch.
    scan(root, &root.join("genai-rs-macros/src"), &mut hits);
    scan(root, &root.join("genai-rs-macros/tests"), &mut hits);

    assert!(
        hits.is_empty(),
        "hardcoded model id(s) found — use genai_rs::DEFAULT_MODEL (or \
         INLINE_VIDEO_MODEL / MINIMAL_THINKING_MODEL / DEFAULT_IMAGE_MODEL / \
         DEFAULT_TTS_MODEL) so a model bump stays a one-line change:\n  {}",
        hits.join("\n  ")
    );
}
