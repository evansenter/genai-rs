//! Response side: every field a response class defines is modeled.
//!
//! For each response type, bodies are synthesized from the snapshot so that
//! together they carry every field, union arm and enum value the class
//! reaches (`Surface::fixtures`). Each must deserialize, and then:
//!
//! - no key lands in an `extra` map (an unmodeled field): a response
//!   struct's flattened `extra`, or, since #491, the `extra` of a known
//!   `Step`, `StepDelta`, `Content` or `Annotation` variant, the
//!   `arguments_extra` of a call step or delta, or the `extra` of a result
//!   item, place, review snippet, step error or static video segment
//!   (`a_new_field_on_a_known_variant_is_reported`),
//! - no value deserializes as an `Unknown` variant (an unmodeled enum value
//!   or union arm), and
//! - no key is missing when the value is serialized again (a field dropped
//!   by a type with no `extra`).
//!
//! Each finding is traced to its `Class.field` site by removing candidate
//! keys one at a time until the finding goes away. Because the bodies are
//! generated, a snapshot refresh that adds a field fails here until the field
//! is modeled or listed below.
//!
//! Compiled out under `genai_strict_unknown`, where an unmodeled enum value fails
//! the whole body instead of showing as `Unknown`: the default build is the
//! one this measures.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use super::api_surface::{
    Finding, Surface, SynthOptions, Ty, debug_extra_keys, debug_unknown_values, key_paths,
    remove_path, string_values,
};
use super::response_targets::{Probe, Target, probe, probe_stream_event, targets};
use super::{Allowed, assert_clean};

/// Fields and values the crate does not model on purpose.
const ALLOWED: &[Allowed] = &[
    ("interactions.Interaction.cached_content", DEPRECATED),
    ("interactions.Interaction.response_mime_type", DEPRECATED),
    (
        "interactions.CreateAgentInteraction.response_mime_type",
        DEPRECATED,
    ),
    ("Interaction.response_mime_type", DEPRECATED),
    (
        "CreateAgentInteractionParams.response_mime_type",
        DEPRECATED,
    ),
    (
        "legacy.File.state=STATE_UNSPECIFIED",
        "The protobuf default; never served.",
    ),
    (
        "legacy.Document.state=STATE_UNSPECIFIED",
        "The protobuf default; never served.",
    ),
    (
        "legacy.File.source=SOURCE_UNSPECIFIED",
        "The protobuf default; never served.",
    ),
    ("legacy.ListFilesResponse.data", CLAUDE_FILES),
    ("legacy.ListFilesResponse.nextPage", CLAUDE_FILES),
    ("CreateAgentInteractionParams.created", TRIGGER_OUTPUT_ONLY),
    (
        "CreateAgentInteractionParams.environment_id",
        TRIGGER_OUTPUT_ONLY,
    ),
    ("CreateAgentInteractionParams.id", TRIGGER_OUTPUT_ONLY),
    ("CreateAgentInteractionParams.status", TRIGGER_OUTPUT_ONLY),
    ("CreateAgentInteractionParams.updated", TRIGGER_OUTPUT_ONLY),
    (
        "interactions.Interaction.continuation_ids",
        "Never served: not on the stored reads, plain or include_input, of a \
         decode continued twice on CONTINUATION_MODEL (live 2026-11-05), and \
         rejected as a request field (`400 Unknown parameter`, 2026-11-01). \
         It lands in `extra` if it ever comes.",
    ),
    ("interactions.Interaction.output_audio", OUTPUT_CONVENIENCE),
    ("interactions.Interaction.output_image", OUTPUT_CONVENIENCE),
    ("interactions.Interaction.output_video", OUTPUT_CONVENIENCE),
    (
        "interactions.SpeakerConfig.speakers",
        "`speech_config` reads both arms of its union, the list and the \
         `{speakers: [...]}` object a stored read serves (live 2026-11-05), \
         into one list, and serializes it back as the list, which every TTS \
         model accepts (live 2026-09-24): the object arm's key is not kept.",
    ),
    (
        "interactions.TranscriptionConfig.language_hints",
        "Deprecated, and rejected on create with 400 Unknown parameter (2.25.0 \
         sweep, D-005), so a stored interaction never carries it.",
    ),
    (
        "interactions.VideoConfig.task=upscale",
        "Rejected on create: `400 ... Supported values: text_to_video, \
         image_to_video, reference_to_video, edit, extend` (2.34.0 sweep, \
         2026-11-01), so a stored interaction never carries it.",
    ),
    (
        "Interaction.max_total_tokens",
        "Spec only, and rejected on create with `400 Unknown parameter \
         'max_total_tokens'` on model and agent interactions alike (live \
         2026-11-05), so never served (D-005). The agent budget it names is \
         `agent_config.max_total_tokens` (`AntigravityConfig`).",
    ),
];

const OUTPUT_CONVENIENCE: &str = "A convenience copy of the output in the \
     bindings, never served: not on a create response or stored read of a TTS \
     or image interaction, nor on a stream's completed event (live \
     2026-11-05; `output_text` is not served either). The output is in \
     `steps`, read by `images()` and `audios()`; no model outputs video. \
     It lands in `extra` if it ever comes.";

const CLAUDE_FILES: &str = "The Claude Files API's list shape (`data` of \
     `ClaudeFile`, `nextPage`), new in google-genai 3.0.0 and documented there \
     as not supported in the Gemini API: the Gemini Files API list serves \
     `files` and `nextPageToken` only (live 2026-11-07, two uploaded files at \
     `pageSize=1`), and rejects the cursor's `ids` query parameter (`400 \
     Unknown name \"ids\"`). It lands in `FileListResponse::extra` if it ever \
     comes.";
const DEPRECATED: &str = "Deprecated, superseded by response_format (or rejected live, \
     D-005), and not served: a stored interaction read back carries neither \
     (live 2026-11-04).";
const TRIGGER_OUTPUT_ONLY: &str = "readOnly on the params schema the spec shares with \
     interactions; a trigger's stored interaction carries none of them (live \
     2026-11-04: store, object, input, agent). One that did would land in \
     `InteractionRequest::extra` and be sent back when the interaction is \
     reused (before #500 it was dropped).";

/// Fields and values the crate does not model yet.
const GAPS: &[Allowed] = &[
    // Files.
    (
        "legacy.File.downloadUri",
        "Not served on an uploaded file, nor on one that failed processing (live \
         2026-11-05). The bindings' `files.download` reads it on a generated \
         file (`source` GENERATED), which only a video generation makes, so it \
         is unprobed; it lands in `FileMetadata::extra` until it is seen.",
    ),
];

/// Values for sites whose snapshot type is looser than what the API sends,
/// so a synthesized value would test the synthesizer rather than the crate.
pub(super) fn values() -> BTreeMap<&'static str, Value> {
    BTreeMap::from([
        // Typed `Any`; what is served is the JSON Schema the caller sent.
        // `FunctionParameters` keeps keywords beyond `type`, `properties`
        // and `required` in a private `extra`, which a synthesized key
        // would land in.
        (
            "interactions.Function.parameters",
            json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        ),
        (
            "Function.parameters",
            json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        ),
        // `Dict[str, Union[EnvVar, str]]`; the string arm is rejected live
        // (`docs/INTERACTIONS_API_GAP.md`, the 2.25.0 sweep), so only the
        // object arm is ever served.
        (
            "interactions.Environment.env",
            json!({"PLAIN": {"value": "v"}, "SECRET": {"credential": "credentials/c"}}),
        ),
        (
            "EnvironmentConfig.env",
            json!({"PLAIN": {"value": "v"}, "SECRET": {"credential": "credentials/c"}}),
        ),
        // `dict[str, Any]` in the bindings; the API serves `videoDuration`
        // (live 2026-11-05), the one key `VideoMetadata` models. It keeps
        // any other in its `extra`, where a synthesized key would land.
        ("legacy.File.videoMetadata", json!({"videoDuration": "12s"})),
        // The spec types these as plain strings; the bindings enumerate
        // them, and each enumerated value is checked from there.
        (
            "Webhook.subscribed_events",
            json!(["interaction.completed"]),
        ),
        (
            "WebhookUpdate.subscribed_events",
            json!(["interaction.completed"]),
        ),
    ])
}

/// The `Class.field` site of a JSON path: the path's own, or its nearest
/// ancestor's (a key inside a free-form map has none of its own).
fn site_of(sites: &BTreeMap<String, String>, path: &str) -> String {
    let mut p = path.to_string();
    loop {
        if let Some(site) = sites.get(&p) {
            return site.clone();
        }
        match p.rfind(['.', '[']) {
            Some(i) if i > 0 => p.truncate(i),
            _ => return "<root>".into(),
        }
    }
}

/// Strips trailing `[i]` segments: the key path a list element sits under.
fn key_path(path: &str) -> &str {
    let mut p = path;
    while p.ends_with(']') {
        match p.rfind('[') {
            Some(i) => p = &p[..i],
            None => break,
        }
    }
    p
}

/// The body paths a kept path stands for where the crate reshapes an
/// `input`: a single object it keeps as a one-element list
/// (`$.input[0].text` was `$.input.text`), and content a request re-sends
/// wrapped in one `user_input` step (a trigger's interaction:
/// `$.interaction.input[0].content[2]` was `$.interaction.input[2]`, or
/// `$.interaction.input` for a single object).
fn input_aliases(path: &str) -> Vec<String> {
    let Some(at) = path.find(".input[0]") else {
        return Vec::new();
    };
    let head = &path[..at + ".input".len()];
    let rest = &path[at + ".input[0]".len()..];
    let mut out = vec![format!("{head}{rest}")];
    if let Some((index, tail)) = rest
        .strip_prefix(".content[")
        .and_then(|r| r.split_once(']'))
    {
        out.push(format!("{head}[{index}]{tail}"));
        if index == "0" {
            out.push(format!("{head}{tail}"));
        }
    }
    out
}

fn count(items: &[String], item: &str) -> usize {
    items.iter().filter(|i| *i == item).count()
}

/// Everything wrong with one body. `traced` holds the sites already traced
/// to an `extra` key or an `Unknown` value in an earlier body, whose
/// candidates need no second round of removals.
pub(super) fn analyze(
    surface: &Surface,
    ty: &Ty,
    body: &Value,
    probe: Probe,
    name: &str,
    traced: &mut BTreeSet<String>,
) -> Vec<Finding> {
    let sites = surface.sites(ty, body);
    let finding = |path: &str, site: String, message: String| Finding {
        path: format!("{name} {path}"),
        site,
        message,
    };
    let without = |path: &str| {
        let mut v = body.clone();
        remove_path(&mut v, path).then_some(v)
    };
    let paths = key_paths(body);
    let mut out = Vec::new();

    let (debug, back) = match probe(body.clone()) {
        Ok(ok) => ok,
        Err(error) => {
            // The deepest key whose removal lets the body parse.
            let culprit = paths
                .iter()
                .rev()
                .find(|p| without(p).is_some_and(|v| probe(v).is_ok()));
            let path = culprit.map_or("$", String::as_str);
            out.push(finding(
                path,
                site_of(&sites, path),
                format!("does not deserialize: {error}"),
            ));
            return out;
        }
    };

    let extras = debug_extra_keys(&debug);
    for key in extras.iter().collect::<BTreeSet<_>>() {
        let before = count(&extras, key);
        let candidates: Vec<&String> = paths
            .iter()
            .filter(|p| p.ends_with(&format!(".{key}")))
            .collect();
        if candidates
            .iter()
            .all(|p| traced.contains(&format!("extra {}", site_of(&sites, p))))
        {
            continue;
        }
        for path in candidates {
            let fewer = without(path)
                .and_then(|v| probe(v).ok())
                .is_some_and(|(d, _)| count(&debug_extra_keys(&d), key) < before);
            if fewer {
                traced.insert(format!("extra {}", site_of(&sites, path)));
                out.push(finding(
                    path,
                    site_of(&sites, path),
                    "lands in `extra`: not modeled".into(),
                ));
            }
        }
    }

    let unknowns = debug_unknown_values(&debug);
    for value in unknowns.iter().collect::<BTreeSet<_>>() {
        let before = count(&unknowns, value);
        let candidates: BTreeSet<String> = string_values(body)
            .into_iter()
            .filter(|(_, s)| s == value)
            .map(|(path, _)| key_path(&path).to_string())
            .collect();
        if candidates
            .iter()
            .all(|p| traced.contains(&format!("unknown {}={value}", site_of(&sites, p))))
        {
            continue;
        }
        for path in &candidates {
            let fewer = without(path).is_none_or(|v| {
                probe(v).map_or(true, |(d, _)| {
                    count(&debug_unknown_values(&d), value) < before
                })
            });
            if fewer {
                traced.insert(format!("unknown {}={value}", site_of(&sites, path)));
                out.push(finding(
                    path,
                    format!("{}={value}", site_of(&sites, path)),
                    "deserializes as an `Unknown` variant".into(),
                ));
            }
        }
    }

    let mut kept = key_paths(&back);
    let reshaped: Vec<String> = kept.iter().flat_map(|p| input_aliases(p)).collect();
    kept.extend(reshaped);
    // A key or list element counts as dropped only under a kept parent (the
    // parent's own finding covers the rest). The parent of a key on a list
    // element is the element (`$.steps[0]`), which `key_paths` records.
    for path in &paths {
        let parent_kept = match path.rfind(['.', '[']) {
            Some(i) => &path[..i] == "$" || kept.contains(&path[..i]),
            None => true,
        };
        if !kept.contains(path) && parent_kept {
            out.push(finding(
                path,
                site_of(&sites, path),
                "dropped: not modeled, and not kept in `extra`".into(),
            ));
        }
    }
    out
}

fn findings(
    surface: &Surface,
    targets: Vec<Target>,
    pick: impl Fn(&Target) -> Option<&'static str>,
) -> Vec<Finding> {
    let options = SynthOptions {
        values: values(),
        ..SynthOptions::default()
    };
    let mut out = BTreeSet::new();
    let mut traced = BTreeSet::new();
    for target in targets {
        let Some(class) = pick(&target) else { continue };
        let ty = surface.ty(class);
        let bodies = surface.fixtures(&ty, &options);
        assert!(!bodies.is_empty(), "{}: no fixtures", target.name);
        for body in &bodies {
            out.extend(analyze(
                surface,
                &ty,
                body,
                target.probe,
                target.name,
                &mut traced,
            ));
        }
    }
    // One finding per site and message is enough to act on.
    let mut seen = BTreeSet::new();
    out.into_iter()
        .filter(|f| seen.insert((f.site.clone(), f.message.clone())))
        .collect()
}

/// Classes reached from a step or a delta whose fields the crate reads as
/// free-form JSON, so a key the snapshot adds there is the tool's own data,
/// kept verbatim, not a field to model: a function or MCP tool's `result`
/// object (`FunctionResultPayload::Json`), and a file search delta's result
/// items, which the spec and the bindings type as objects with no fields.
const FREE_FORM: &[&str] = &[
    "interactions.FileSearchResult",
    "interactions.FunctionResultDeltaResult",
    "interactions.FunctionResultStepResult",
    "interactions.MCPServerToolResultDeltaResult",
    "interactions.MCPServerToolResultStepResult",
];

/// A field the snapshot adds to any class a `Step` or `StepDelta` reaches is
/// reported, and as landing in `extra`: since #491 every known variant of
/// `Step`, `StepDelta`, `Content` and `Annotation` keeps such a key in its
/// `extra` (a call's `arguments` keys in `arguments_extra`), as do the
/// result items, places, review snippets, step errors and static video
/// segments inside them. Before, the key was dropped on re-serialization.
/// Either way it shows in `{:?}`, which is what the oracle reads, so a new
/// field on a known variant fails `binding_fields_deserialize_without_extra`
/// until it is modeled or listed. Deltas go through the stream pipeline, as
/// in the `StreamEvent` target.
#[test]
fn a_new_field_on_a_known_variant_is_reported() {
    let mut root = super::api_surface::read_json("bindings.json");
    let snapshot = Surface::from_bindings(&root);
    let mut classes = BTreeSet::new();
    for union in ["interactions.Step", "interactions.StepDeltaData"] {
        classes.extend(snapshot.reachable_classes(&snapshot.ty(union)));
    }
    assert!(classes.len() > 70, "{classes:?}");
    for class in &classes {
        root["classes"][class]["fields"]["brand_new_field"] =
            json!({"required": false, "type": {"kind": "str"}});
    }
    let surface = Surface::from_bindings(&root);
    let targets = vec![
        Target {
            name: "Step",
            binding: Some("interactions.Step"),
            spec: None,
            probe: probe::<crate::steps::Step>,
        },
        Target {
            name: "StreamEvent",
            binding: Some("interactions.StepDelta"),
            spec: None,
            probe: probe_stream_event,
        },
    ];
    let reported: BTreeMap<String, String> = findings(&surface, targets, |t| t.binding)
        .into_iter()
        .filter(|f| f.site.ends_with(".brand_new_field"))
        .map(|f| (f.site, f.message))
        .collect();
    let mut wrong = Vec::new();
    for class in &classes {
        let site = format!("{class}.brand_new_field");
        let expected =
            (!FREE_FORM.contains(&class.as_str())).then_some("lands in `extra`: not modeled");
        let got = reported.get(&site).map(String::as_str);
        if got != expected {
            wrong.push(format!("{site}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A field on a type that keeps no `extra` is reported as dropped, including
/// on an object directly inside a list, where serde drops it without a
/// trace: `key_paths` records list elements, so the element counts as the
/// key's kept parent. (The crate's step and content types all keep an
/// `extra` now, so a type of the test's own stands in.)
#[test]
fn a_dropped_field_on_a_list_element_is_reported() {
    #[derive(Debug, serde::Deserialize, Serialize)]
    struct Holder {
        items: Vec<Item>,
    }
    #[derive(Debug, serde::Deserialize, Serialize)]
    struct Item {
        name: String,
    }
    let optional = |kind: &str| json!({"required": false, "type": {"kind": kind}});
    let surface = Surface::from_bindings(&json!({
        "sdk_version": "test",
        "aliases": {},
        "classes": {
            "t.Holder": {"fields": {"items": {
                "required": true,
                "type": {"kind": "list", "item": {"kind": "ref", "name": "t.Item"}},
            }}},
            "t.Item": {"fields": {
                "name": {"required": true, "type": {"kind": "str"}},
                "brand_new_field": optional("str"),
            }},
        },
    }));
    let holder = Target {
        name: "Holder",
        binding: Some("t.Holder"),
        spec: None,
        probe: probe::<Holder>,
    };
    let reported: Vec<(String, String)> = findings(&surface, vec![holder], |t| t.binding)
        .into_iter()
        .map(|f| (f.site, f.message))
        .collect();
    assert_eq!(
        reported,
        [(
            "t.Item.brand_new_field".to_string(),
            "dropped: not modeled, and not kept in `extra`".to_string()
        )]
    );
}

#[test]
fn binding_fields_deserialize_without_extra() {
    let surface = Surface::bindings();
    assert_clean(
        &format!(
            "Response fields vs google-genai {} bindings",
            surface.version
        ),
        &surface,
        findings(&surface, targets(), |t| t.binding),
        &[("ALLOWED", ALLOWED), ("GAPS", GAPS)],
    );
}

#[test]
fn spec_fields_deserialize_without_extra() {
    let surface = Surface::openapi("openapi-v1beta.json");
    assert_clean(
        &format!(
            "Response fields vs the v1beta OpenAPI spec ({})",
            surface.version
        ),
        &surface,
        findings(&surface, targets(), |t| t.spec),
        &[("ALLOWED", ALLOWED), ("GAPS", GAPS)],
    );
}
