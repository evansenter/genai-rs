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
    // Borrowed view over a response, built by the crate for callers to read;
    // it holds references, so it is not a mock-construction hazard.
    "src/response.rs:FunctionCallInfo",
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

    let mut offenders = BTreeSet::new();
    for file in &files {
        let rel = file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.contains("antigravity") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            panic!("could not read {rel} — refusing to report a clean scan");
        };
        offenders.extend(offenders_in(&rel, &text));
    }

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
fn offenders_in(path: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();

    // A hand-written `impl<'de> Deserialize<'de> for Foo` carries no
    // `Deserialize` token in Foo's attribute block, so the derive check
    // below cannot see it. This crate hand-writes Deserialize for much of
    // its Evergreen surface, so collect those target names first.
    let manual: BTreeSet<&str> = lines
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("impl") || !trimmed.contains("Deserialize") {
                return None;
            }
            let after = trimmed.split(" for ").nth(1)?;
            Some(after.split(['<', ' ', '{']).next()?.trim())
        })
        .collect();

    let mut offenders = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        // Trimmed, so a declaration indented inside an inline `mod` counts.
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("pub struct ") else {
            continue;
        };
        // Split before any generics so `Foo<'a>` compares as `Foo`.
        let name = rest
            .split(['<', '{', '(', ' ', ';'])
            .next()
            .unwrap_or("")
            .trim();
        if name.is_empty() || REQUEST_SIDE.contains(&format!("{path}:{name}").as_str()) {
            continue;
        }

        // Walk back over the contiguous attribute/doc block. A multi-line
        // `#[derive(` continues onto lines that start with neither `#[` nor
        // `///`, so keep going while the block is unbalanced.
        let mut attrs = String::new();
        let mut cursor = index;
        let mut depth = 0i32;
        while cursor > 0 {
            let prev = lines[cursor - 1];
            let t = prev.trim_start();
            // `)]` closes a multi-line `#[derive(...)]`; treat it as meta so
            // the walk-back enters the block, then let `depth` carry it over
            // the bare derive names inside, which start with none of these.
            let is_meta = t.starts_with("#[")
                || t.starts_with("///")
                || t.starts_with("//")
                || t.starts_with(')');
            if !is_meta && depth == 0 {
                break;
            }
            depth += prev.matches(')').count() as i32 - prev.matches('(').count() as i32;
            attrs.push_str(prev);
            attrs.push('\n');
            cursor -= 1;
        }

        let deserializable = attrs.contains("Deserialize") || manual.contains(name);
        if deserializable && !attrs.contains("non_exhaustive") {
            offenders.push(format!("{path}:{}: {name}", index + 1));
        }
    }
    offenders
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
"#;

    let found = offenders_in("src/request.rs", fixture);
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

    // And the exemption is scoped: the same name elsewhere is not exempt.
    let elsewhere = offenders_in("src/response.rs", fixture);
    let elsewhere_names: Vec<&str> = elsewhere
        .iter()
        .map(|f| f.rsplit(": ").next().unwrap())
        .collect();
    assert!(
        elsewhere_names.contains(&"GenerationConfig"),
        "exemptions must be path-scoped, not global: {elsewhere:?}"
    );
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
