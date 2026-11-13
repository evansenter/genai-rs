//! Guards the single-source-of-truth for model ids.
//!
//! Lives in its own test binary rather than `tests/common`, which is
//! compiled into every integration test crate and would run this scan
//! a dozen times per suite. Walks the tree with the guards' shared walker
//! (`tests/common/source_scan.rs`, D-068), which fails on a scan root that
//! is gone or an entry it cannot read.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    reason = "a test reports a failure by panicking"
)]

#[path = "common/source_scan.rs"]
mod source_scan;

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
    //
    // Dated agent ids ("deep-research-preview-04-2026") are covered too: they
    // are retired on the same schedule as models.
    static MODEL_LITERAL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r#""(models/)?gemini-\d|"(deep-research|antigravity)(-[a-z]+)*-\d{2}-\d{4}""#,
        )
        .unwrap()
    });

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

    fn scan(dir: &Path, hits: &mut Vec<String>) -> usize {
        // A scan root that does not exist must not read as one that is
        // clean. This test's whole argument is that a missed literal is
        // invisible, so it should not be able to stop covering a directory
        // silently when one is moved or renamed: the shared walker fails
        // on a missing root, and on an entry it cannot read.
        let files = source_scan::rust_files(dir);
        for path in &files {
            let rel = source_scan::repo_relative(path);
            if ALLOWED.contains(&rel.as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("could not read {rel}: {e}"));
            for (i, line) in text.lines().enumerate() {
                // A quoted id, so `genai_rs::DEFAULT_MODEL` does not match.
                if MODEL_LITERAL.is_match(line) {
                    hits.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                }
            }
        }
        files.len()
    }

    // The root crate *is* the workspace root, so `target/` sits beside
    // these and is never walked.
    let root = source_scan::root();
    let mut hits = Vec::new();
    let mut scanned = scan(&root.join("src"), &mut hits);
    // Walks recursively, so the trybuild fixtures in tests/ui/ are covered
    // too — they are ordinary .rs files and would pin a model just as
    // invisibly as any other.
    scanned += scan(&root.join("tests"), &mut hits);
    scanned += scan(&root.join("examples"), &mut hits);
    // The other workspace member, which is Cargo.toml + src/ only. Clean
    // today, but a literal pinned there would be exactly the invisible one
    // this test exists to catch.
    scanned += scan(&root.join("genai-rs-macros/src"), &mut hits);
    // A walk that finds nothing reads as a clean tree (D-068's floors).
    assert!(scanned > 300, "only {scanned} .rs files scanned");

    assert!(
        hits.is_empty(),
        "hardcoded model id(s) found — use genai_rs::DEFAULT_MODEL (or \
         DEFAULT_IMAGE_MODEL / DEFAULT_TTS_MODEL / DEFAULT_TRANSCRIPTION_MODEL / \
         CONTINUATION_MODEL / DEFAULT_DEEP_RESEARCH_AGENT / \
         DEFAULT_ANTIGRAVITY_AGENT) so a bump \
         stays a one-line change:\n  {}",
        hits.join("\n  ")
    );
}
