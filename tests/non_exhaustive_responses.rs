//! Guards the `#[non_exhaustive]` convention for response structs.
//!
//! Lives in its own test binary for the same reason as
//! `tests/model_literals.rs`: it scans the source tree, and `tests/common`
//! is compiled into every integration test crate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Request-side types, deliberately exempt, keyed by repo-relative path.
///
/// The convention exists because *responses* are the crate's to grow.
/// Requests are the caller's to build, and `#[non_exhaustive]` on one takes
/// away struct-literal construction while buying nothing — the crate does
/// not gain the freedom to add required fields to something a user
/// assembles.
///
/// Keyed by `path:Name` rather than by bare name so each exemption is scoped
/// to the type it was written for. `WebhookConfig`, `AgentConfig` and
/// `ImageConfig` are the kind of name a future *response* type could reuse,
/// and a bare-name list would exempt it silently. Same reasoning as the
/// exact-path allowlist in `tests/model_literals.rs`.
const REQUEST_SIDE: &[&str] = &[
    // Interaction request and its config tree.
    "src/request.rs:InteractionRequest",
    "src/request.rs:GenerationConfig",
    "src/request.rs:TranscriptionConfig",
    "src/request.rs:SpeechConfig",
    "src/request.rs:ImageConfig",
    "src/request.rs:VideoConfig",
    "src/request.rs:AgentConfig",
    "src/safety.rs:SafetySetting",
    // Tool declarations and configs the caller builds.
    "src/tools.rs:FunctionDeclaration",
    "src/tools.rs:FunctionParameters",
    "src/tools.rs:AllowedTools",
    "src/tools.rs:VertexAiSearchConfig",
    "src/tools.rs:ExaAiSearchConfig",
    "src/tools.rs:ParallelAiSearchConfig",
    "src/tools.rs:RagResource",
    "src/tools.rs:HybridSearchConfig",
    "src/tools.rs:RagFilter",
    "src/tools.rs:RagRanking",
    "src/tools.rs:RagRetrievalConfig",
    "src/tools.rs:RagStoreConfig",
    // Resource create/update bodies.
    "src/environments.rs:CreateEnvironmentRequest",
    "src/environment.rs:EnvironmentSource",
    "src/environment.rs:AllowlistEntry",
    // Built by the caller and passed to `with_environment`; its own doc
    // example assembles one from `EnvironmentSource` and `AllowlistEntry`.
    // Reached only via a hand-written Deserialize impl, which is why the
    // derive-only scan did not see it.
    "src/environment.rs:RemoteEnvironment",
    "src/triggers.rs:TriggerCreateParams",
    "src/triggers.rs:TriggerUpdate",
    "src/webhooks.rs:WebhookUpdate",
    "src/webhooks.rs:WebhookConfig",
];

/// Fails if a deserializable public struct in the API surface lacks
/// `#[non_exhaustive]` without being listed as request-side.
///
/// `docs/ENUM_WIRE_FORMATS.md` states the convention, and #430 found five
/// response structs that had quietly diverged from it. The cost of that
/// divergence is not visible at the point it happens — it shows up one
/// release later, when adding a field the API grew turns out to be a
/// breaking change. Review is a poor detector for "an attribute is absent",
/// so this scans instead.
///
/// Skips `src/antigravity/`: the localharness JSON-RPC protocol is a
/// separate wire format whose types the crate *sends* as well as receives,
/// so the response/request split this test encodes does not apply to it.
#[test]
fn response_structs_are_non_exhaustive() {
    // Anchored to the manifest, not the working directory: a test that
    // scans a path it cannot read would report zero offenders and pass,
    // which is the same "an absence is invisible" failure this exists to
    // prevent. Same treatment as `tests/model_literals.rs`.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(
        root.is_dir(),
        "scan root {} does not exist — a root that cannot be read must not \
         read as a tree that is clean",
        root.display()
    );

    let files = walk(&root);
    assert!(
        !files.is_empty(),
        "scan root {} contained no .rs files — the gate is inert",
        root.display()
    );

    let lib_rs = std::fs::read_to_string(root.join("lib.rs")).expect(
        "src/lib.rs must be readable — a scan that cannot classify \
                 test modules is not a clean scan",
    );
    let test_modules = cfg_test_modules(&lib_rs);
    assert!(
        !test_modules.is_empty(),
        "no #[cfg(test)] mod declarations found in lib.rs — the pattern this \
         parses has changed, and test-only files would be scanned as API"
    );

    // First pass: collect hand-written Deserialize targets across the whole
    // crate, since nothing requires the impl to live beside its type.
    let mut manual = BTreeSet::new();
    let mut sources = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.contains("antigravity") {
            continue;
        }
        let text =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("could not read {rel}: {e}"));
        manual.extend(manual_deserialize_targets(&text));
        sources.push((rel, text));
    }

    let mut offenders = BTreeSet::new();
    let mut matched_exemptions = BTreeSet::new();
    for (rel, text) in &sources {
        let (found, exempted) = offenders_and_exemptions_in(rel, text, &manual);
        offenders.extend(found);
        matched_exemptions.extend(exempted);
    }

    // An exemption that matches nothing is not harmless. The wrong-looking
    // direction — a type moving to another file — at least surfaces, since
    // it is then scanned unexempted. A *deleted* type leaves its key here
    // forever, pre-authorizing whatever future type happens to land on that
    // exact `path:Name`. Each entry carries a written justification, and a
    // stale one justifies something that no longer exists.
    let stale: Vec<&str> = REQUEST_SIDE
        .iter()
        .copied()
        .filter(|key| !matched_exemptions.contains(*key))
        .collect();
    assert!(
        stale.is_empty(),
        "These REQUEST_SIDE exemptions no longer match any type:\n  {}\n\n\
         Remove them, or update the path if the type moved.",
        stale.join("\n  ")
    );

    assert!(
        offenders.is_empty(),
        "These deserializable structs are missing `#[non_exhaustive]`:\n  {}\n\n\
         Response structs carry it so the crate can add fields the API grows \
         without a breaking change — see docs/ENUM_WIRE_FORMATS.md. If one of \
         these is a type callers *construct to send*, add it to REQUEST_SIDE in \
         this file with a note saying why.",
        offenders.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}

/// The scan, factored out so it can be run over a fixture.
///
/// `path` is repo-relative and only used for reporting and for matching
/// `REQUEST_SIDE`.
/// Returns the offenders in one file, plus the `path:Name` exemption keys it
/// actually matched — so a key that no longer excuses anything can be
/// reported.
fn offenders_and_exemptions_in(
    path: &str,
    text: &str,
    manual: &BTreeSet<String>,
) -> (Vec<String>, BTreeSet<String>) {
    let lines: Vec<&str> = text.lines().collect();

    let mut offenders = Vec::new();
    let mut matched = BTreeSet::new();
    // Skip whatever a `#[cfg(test)]` attribute gates — it is not public
    // API, so demanding `#[non_exhaustive]` on it would be wrong, and the
    // only silencer would be `REQUEST_SIDE`, whose doc says entries are
    // types a caller constructs to *send*.
    //
    // Two states, because the gated item may or may not have braces:
    // `#[cfg(test)] mod tests { .. }` and `#[cfg(test)] resource_markers! {
    // .. }` are brace-delimited, while `#[cfg(test)] pub(crate) mod
    // test_subscriber;` ends at a semicolon. A single counter seeded on the
    // attribute line cannot express that — it never returns to zero, so the
    // scan stops at the first cfg-test item and silently skips the rest of
    // the file.
    let mut cfg_test_depth = 0i32;
    let mut awaiting_gated_item = false;

    for (index, line) in lines.iter().enumerate() {
        // Trimmed, so a declaration indented inside an inline `mod` counts.
        let trimmed = line.trim_start();

        if cfg_test_depth > 0 {
            cfg_test_depth += line.matches('{').count() as i32;
            cfg_test_depth -= line.matches('}').count() as i32;
            continue;
        }
        if awaiting_gated_item {
            let opens = line.matches('{').count() as i32;
            if opens > 0 {
                cfg_test_depth = opens - line.matches('}').count() as i32;
                awaiting_gated_item = false;
            } else if trimmed.ends_with(';') {
                awaiting_gated_item = false;
            }
            continue;
        }
        if trimmed.starts_with("#[cfg(test)]") {
            awaiting_gated_item = true;
            continue;
        }

        let Some(rest) = trimmed.strip_prefix("pub struct ") else {
            continue;
        };
        // Split before any generics so `Foo<'a>` compares as `Foo`.
        let name = rest
            .split(['<', '{', '(', ' ', ';'])
            .next()
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        let key = format!("{path}:{name}");
        let exempt = REQUEST_SIDE.contains(&key.as_str());

        // Walk back over the contiguous attribute/doc block, collecting
        // *only* attribute lines.
        //
        // Doc prose is deliberately excluded from both the collected text
        // and the paren accounting. The crate writes
        // "This enum is marked `#[non_exhaustive]` for forward
        // compatibility." above ~25 items, so folding prose in would let a
        // struct satisfy the attribute check by *mentioning* it — the
        // silent pass this whole file argues against — and symmetrically
        // would report a Serialize-only struct whose docs mention
        // Deserialize. Counting parens in prose has the same shape of bug:
        // one unmatched `(` leaves the depth non-zero and walks the scan
        // into unrelated earlier source.
        let mut attrs = String::new();
        let mut cursor = index;
        let mut depth = 0i32;
        while cursor > 0 {
            let prev = lines[cursor - 1];
            let t = prev.trim_start();
            let is_doc = t.starts_with("///") || t.starts_with("//");
            // Inside an unclosed `#[derive(`, a line is a bare derive name.
            // `)]` is how such a block *ends*, and the walk-back meets it
            // first, so it has to be admitted before `depth` can carry the
            // names above it. Matched exactly, not as a leading `)`: a bare
            // close paren at depth 0 could be a macro invocation or a call
            // above the declaration, and admitting it would push depth to 1
            // and swallow arbitrary source back to the matching open paren.
            let in_attr = depth != 0;
            if !is_doc && !in_attr && !t.starts_with("#[") && !t.starts_with(")]") {
                break;
            }
            cursor -= 1;
            if is_doc {
                continue;
            }
            depth += prev.matches(')').count() as i32 - prev.matches('(').count() as i32;
            attrs.push_str(prev);
            attrs.push('\n');
        }

        // An exemption counts as *used* only when it actually suppressed an
        // offender. Recording it on sight would let an entry that never
        // excused anything — because the type is Serialize-only, say —
        // survive the staleness check forever, silently pre-authorizing the
        // day `Deserialize` is added to it. Staleness has to mean "excuses
        // nothing", not merely "names something".
        let deserializable = attrs.contains("Deserialize") || manual.contains(name);
        if deserializable && !attrs.contains("non_exhaustive") {
            if exempt {
                matched.insert(key);
            } else {
                offenders.push(format!("{path}:{}: {name}", index + 1));
            }
        }
    }
    (offenders, matched)
}

/// Pins that the scan discriminates, rather than that it happens to return
/// nothing on a tree that is already clean.
///
/// Without this, a refactor of the line matching could turn the guard into a
/// no-op that still reports green — the same class of silent inertness the
/// guard itself exists to catch.
#[test]
fn the_scan_detects_what_it_claims_to() {
    let fixture = r#"
#[derive(Deserialize)]
#[non_exhaustive]
pub struct Annotated {}

#[derive(Debug, Deserialize, Serialize)]
pub struct Bare {}

#[derive(Deserialize)]
pub struct GenerationConfig {}

#[derive(Serialize)]
pub struct SerializeOnly {}

pub struct ManualImpl {}
impl<'de> Deserialize<'de> for ManualImpl {}

#[derive(
    Debug,
    Deserialize,
)]
pub struct MultiLineDerive {}

mod inner {
    #[derive(Deserialize)]
    pub struct Indented {}
}

// Deserialize impl lives in another file; only the crate-wide union sees it.
pub struct ImplInAnotherFile {}

#[cfg(test)]
mod tests {
    #[derive(Deserialize)]
    pub struct TestOnlyFixture {}
}

// Below the cfg-test block: the position that catches a counter which never
// unwinds. With one, the scan stops at the attribute and everything from
// here to EOF is silently skipped.
#[derive(Deserialize)]
pub struct AfterTests {}

// And a semicolon-terminated gated item, which has no braces to balance.
#[cfg(test)]
pub(crate) mod helper;

#[derive(Deserialize)]
pub struct AfterSemicolonGatedItem {}
"#;

    // Supplied explicitly rather than derived from the fixture, so this
    // also covers the case the crate-wide union exists for: a hand-written
    // impl living in a *different* file from its struct.
    let manual = BTreeSet::from(["ManualImpl".to_string(), "ImplInAnotherFile".to_string()]);

    let (found, _) = offenders_and_exemptions_in("src/request.rs", fixture, &manual);
    let names: Vec<&str> = found
        .iter()
        .map(|f| f.rsplit(": ").next().unwrap())
        .collect();

    assert!(
        names.contains(&"Bare"),
        "plain missing attribute: {found:?}"
    );
    assert!(
        names.contains(&"ManualImpl"),
        "hand-written Deserialize impl: {found:?}"
    );
    assert!(
        names.contains(&"MultiLineDerive"),
        "multi-line derive block: {found:?}"
    );
    assert!(
        names.contains(&"Indented"),
        "declaration inside an inline mod: {found:?}"
    );
    assert!(
        names.contains(&"ImplInAnotherFile"),
        "manual impl in a different file, via the crate-wide union: {found:?}"
    );

    assert!(
        !names.contains(&"Annotated"),
        "already annotated, must not be reported: {found:?}"
    );
    assert!(
        !names.contains(&"SerializeOnly"),
        "not deserializable, must not be reported: {found:?}"
    );
    assert!(
        !names.contains(&"GenerationConfig"),
        "exempt at this path, must not be reported: {found:?}"
    );
    assert!(
        names.contains(&"AfterTests"),
        "scanning must resume after a cfg(test) block — a counter that never \
         unwinds skips the rest of the file: {found:?}"
    );
    assert!(
        names.contains(&"AfterSemicolonGatedItem"),
        "and after a gated item with no braces to balance: {found:?}"
    );
    assert!(
        !names.contains(&"TestOnlyFixture"),
        "cfg(test) surface is not public API and must not be reported — the \
         only escape hatch would be REQUEST_SIDE, which means something else: \
         {found:?}"
    );

    // The exemption set itself, which `offenders_in` discards — so the
    // stale check has a fixture behind it rather than only ever being
    // observed passing on a clean tree.
    let (_, exempted) = offenders_and_exemptions_in("src/request.rs", fixture, &manual);
    assert_eq!(
        exempted,
        BTreeSet::from(["src/request.rs:GenerationConfig".to_string()]),
        "an exemption is recorded under its full path:Name when it suppresses \
         an offender"
    );

    // And the exemption is scoped: the same name elsewhere is not exempt.
    let (elsewhere, _) = offenders_and_exemptions_in("src/response.rs", fixture, &manual);
    let elsewhere_names: Vec<&str> = elsewhere
        .iter()
        .map(|f| f.rsplit(": ").next().unwrap())
        .collect();
    assert!(
        elsewhere_names.contains(&"GenerationConfig"),
        "exemptions must be path-scoped, not global: {elsewhere:?}"
    );
    let (_, none_here) = offenders_and_exemptions_in("src/response.rs", fixture, &manual);
    assert!(
        none_here.is_empty(),
        "and nothing is recorded as exempt at a path with no matching entry: \
         {none_here:?}"
    );
}

/// Module paths `lib.rs` declares under a `#[cfg(test)]` attribute.
///
/// Those files compile only under test, so their `pub struct`s are not API —
/// but they sit at column 0 and are otherwise indistinguishable here. The
/// gating lives on the `mod NAME;` line in `lib.rs`, *not* as an inner
/// `#![cfg(test)]` in the file itself, so the file alone cannot be
/// classified. Derived rather than hand-listed so a sixth test module is
/// picked up automatically.
fn cfg_test_modules(lib_rs: &str) -> BTreeSet<String> {
    let lines: Vec<&str> = lib_rs.lines().collect();
    let mut found = BTreeSet::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("#[cfg(test)]") {
            continue;
        }
        let Some(next) = lines.get(index + 1) else {
            continue;
        };
        // `mod x;`, `pub mod x;`, `pub(crate) mod x;`
        let Some(rest) = next.trim_start().split(" mod ").nth(1) else {
            continue;
        };
        if let Some(name) = rest.split([';', ' ', '{']).next()
            && !name.is_empty()
        {
            found.insert(name.to_string());
        }
    }
    found
}

/// Names targeted by a hand-written `impl<'de> Deserialize<'de> for Foo`.
///
/// Such a struct carries no `Deserialize` token in its own attribute block,
/// so the derive check cannot see it — and this crate hand-writes
/// Deserialize across its Evergreen surface. Unioned across every file by
/// the caller rather than computed per file, because nothing requires the
/// impl to live beside its type; a split across modules would otherwise be
/// invisible, which is the same silent-absence mode this guard exists for.
fn manual_deserialize_targets(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("impl") || !trimmed.contains("Deserialize") {
                return None;
            }
            let after = trimmed.split(" for ").nth(1)?;
            Some(after.split(['<', ' ', '{']).next()?.trim().to_string())
        })
        .collect()
}

/// Recursively collects `.rs` files under `dir`.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", current.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
    }
    found
}
