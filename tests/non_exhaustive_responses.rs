//! Guards the `#[non_exhaustive]` convention for response structs.
//!
//! Lives in its own test binary for the same reason as
//! `tests/model_literals.rs`: it scans the source tree, and `tests/common`
//! is compiled into every integration test crate.

use std::collections::BTreeSet;

/// Request-side types, deliberately exempt.
///
/// The convention exists because *responses* are the crate's to grow.
/// Requests are the caller's to build, and `#[non_exhaustive]` on one takes
/// away struct-literal construction with no `..Default::default()` escape —
/// which is a cost with no matching benefit, since the crate does not gain
/// the freedom to add required fields to something a user assembles.
///
/// Anything listed here should be a type a caller constructs to *send*.
const REQUEST_SIDE: &[&str] = &[
    // Interaction request and its config tree.
    "InteractionRequest",
    "GenerationConfig",
    "TranscriptionConfig",
    "SpeechConfig",
    "ImageConfig",
    "VideoConfig",
    "AgentConfig",
    "SafetySetting",
    // Tool declarations and configs the caller builds.
    "FunctionDeclaration",
    "FunctionParameters",
    "AllowedTools",
    "VertexAiSearchConfig",
    "ExaAiSearchConfig",
    "ParallelAiSearchConfig",
    "RagResource",
    "HybridSearchConfig",
    "RagFilter",
    "RagRanking",
    "RagRetrievalConfig",
    "RagStoreConfig",
    // Resource create/update bodies.
    "CreateEnvironmentRequest",
    "EnvironmentSource",
    "AllowlistEntry",
    "TriggerCreateParams",
    "TriggerUpdate",
    "WebhookUpdate",
    "WebhookConfig",
    // Borrowed view over a response, built by the crate for callers to read;
    // it holds references, so it is not a mock-construction hazard.
    "FunctionCallInfo",
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
    let mut offenders = BTreeSet::new();

    for entry in walk("src") {
        let path = entry.to_string_lossy().to_string();
        if path.contains("antigravity") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };

        let lines: Vec<&str> = text.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let Some(rest) = line.strip_prefix("pub struct ") else {
                continue;
            };
            // Split before any generics so `Foo<'a>` compares as `Foo`.
            let name = rest
                .split(['<', '{', '(', ' ', ';'])
                .next()
                .unwrap_or("")
                .trim();
            if name.is_empty() || REQUEST_SIDE.contains(&name) {
                continue;
            }

            // Walk back over the contiguous attribute/doc block.
            let mut attrs = String::new();
            let mut cursor = index;
            while cursor > 0 {
                let prev = lines[cursor - 1];
                let is_meta = prev.starts_with("#[")
                    || prev.starts_with("///")
                    || prev.starts_with("//")
                    || prev.starts_with("    ");
                if !is_meta {
                    break;
                }
                attrs.push_str(prev);
                attrs.push('\n');
                cursor -= 1;
            }

            if attrs.contains("Deserialize") && !attrs.contains("non_exhaustive") {
                offenders.insert(format!("{path}:{}: {name}", index + 1));
            }
        }
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

/// Recursively collects `.rs` files under `dir`.
fn walk(dir: &str) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
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
