//! Response side: every field a response class defines is modeled.
//!
//! For each response type, bodies are synthesized from the snapshot so that
//! together they carry every field, union arm and enum value the class
//! reaches (`Surface::fixtures`). Each must deserialize, and then:
//!
//! - no key lands in a flattened `extra` map (an unmodeled field),
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
//! Compiled out under `strict-unknown`, where an unmodeled enum value fails
//! the whole body instead of showing as `Unknown`: the default build is the
//! one this measures.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::api_surface::{
    Finding, Surface, SynthOptions, Ty, debug_extra_keys, debug_unknown_values, key_paths,
    remove_path, string_values,
};
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
    ("CreateAgentInteractionParams.created", TRIGGER_OUTPUT_ONLY),
    (
        "CreateAgentInteractionParams.environment_id",
        TRIGGER_OUTPUT_ONLY,
    ),
    ("CreateAgentInteractionParams.id", TRIGGER_OUTPUT_ONLY),
    ("CreateAgentInteractionParams.status", TRIGGER_OUTPUT_ONLY),
    ("CreateAgentInteractionParams.updated", TRIGGER_OUTPUT_ONLY),
];

const DEPRECATED: &str = "Deprecated, superseded by response_format (or rejected live, \
     D-005), and not served: a stored interaction read back carries neither \
     (live 2026-11-04).";
const TRIGGER_OUTPUT_ONLY: &str = "readOnly on the params schema the spec shares with \
     interactions; a trigger's stored interaction carries none of them (live \
     2026-11-04: store, object, input, agent).";

/// Fields and values the crate does not model yet.
const GAPS: &[Allowed] = &[
    // Served on a stored interaction read back (live 2026-11-04), landing in
    // `extra`.
    ("interactions.Interaction.generation_config", SERVED),
    ("interactions.Interaction.response_format", SERVED),
    ("interactions.Interaction.response_modalities", SERVED),
    ("interactions.Interaction.safety_settings", SERVED),
    ("Interaction.response_format", SERVED),
    ("Interaction.response_modalities", SERVED),
    ("Interaction.safety_settings", SERVED),
    // In the snapshot; not seen live on a model interaction (2026-11-04).
    ("interactions.Interaction.agent_config", UNSEEN),
    ("interactions.Interaction.continuation_ids", UNSEEN),
    ("interactions.Interaction.environment", UNSEEN),
    ("interactions.Interaction.output_audio", UNSEEN),
    ("interactions.Interaction.output_image", UNSEEN),
    ("interactions.Interaction.output_video", UNSEEN),
    ("Interaction.agent_config", UNSEEN),
    ("Interaction.environment", UNSEEN),
    ("Interaction.max_total_tokens", UNSEEN),
    (
        "interactions.Interaction.status=queued",
        "Not an InteractionStatus variant yet (#501); WP2.",
    ),
    (
        "interactions.InteractionSseEventInteraction.status=queued",
        "Not an InteractionStatus variant yet (#501); WP2.",
    ),
    (
        "interactions.InteractionStatusUpdate.status=queued",
        "Not an InteractionStatus variant yet (#501); WP2.",
    ),
    (
        "interactions.StepDelta.metadata",
        "`StreamChunk::StepDelta` drops the event's `metadata.total_usage` (only \
         `interaction.completed` reads it); WP2.",
    ),
    (
        "StepDelta.metadata",
        "`StreamChunk::StepDelta` drops the event's `metadata.total_usage` (only \
         `interaction.completed` reads it); WP2.",
    ),
    ("agents.Agent.agent_config", UNSEEN),
    ("Agent.agent_config", UNSEEN),
    ("interactions.Environment.environment_id", UNSEEN),
    ("EnvironmentConfig.environment_id", UNSEEN),
    (
        "Function.type=function",
        "`Tool::Function` requires `description`, which the spec omits and the \
         bindings make optional, so a served function tool without one reads as \
         `Unknown`; WP2.",
    ),
    // Files and File Search Stores.
    (
        "legacy.FileSearchStore.activeDocumentsCount",
        "Served (live 2026-11-04, as a string int64); WP2.",
    ),
    (
        "legacy.FileSearchStore.sizeBytes",
        "Served (live 2026-11-04, as a string int64); WP2.",
    ),
    ("legacy.FileSearchStore.pendingDocumentsCount", UNSEEN),
    ("legacy.FileSearchStore.failedDocumentsCount", UNSEEN),
    ("legacy.Document.customMetadata", UNSEEN),
    (
        "legacy.File.updateTime",
        "Served on every file (live 2026-11-04); WP2.",
    ),
    (
        "legacy.File.source",
        "Served on every file (live 2026-11-04: UPLOADED); WP2.",
    ),
    ("legacy.File.downloadUri", UNSEEN),
    (
        "legacy.FileStatus.details",
        "`FileError` has no `details` and no `extra`, so the key is dropped; WP2.",
    ),
];

const SERVED: &str = "Served on a stored interaction read back (live 2026-11-04) \
     and kept only in `extra`; WP2.";
const UNSEEN: &str = "In the snapshot, not modeled, and not seen live yet \
     (2026-11-04); probe before modeling (D-010); WP2.";

/// Values for sites whose snapshot type is looser than what the API sends,
/// so a synthesized value would test the synthesizer rather than the crate.
fn values() -> BTreeMap<&'static str, Value> {
    BTreeMap::from([
        // Typed `Any`. The crate reads a JSON Schema object, which every
        // served function tool carries.
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
        // A protobuf int64: the Files API sends it as a JSON string. The
        // bindings type it `int` because google-genai converts it.
        ("legacy.File.sizeBytes", json!("7")),
        // `dict[str, Any]` in the bindings; the API serves `videoDuration`,
        // the one key `VideoMetadata` models (it keeps no others).
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

type Probe = fn(Value) -> Result<(String, Value), String>;

/// Deserializes, then returns the `{:?}` rendering and the value serialized
/// again.
fn probe<T: DeserializeOwned + Serialize + Debug>(value: Value) -> Result<(String, Value), String> {
    let parsed: T = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let back = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
    Ok((format!("{parsed:?}"), back))
}

/// A stream event the way the streaming path reads it: parsed as the wire
/// event, then dispatched to the public [`StreamEvent`]. Serialized again,
/// a `StreamEvent` uses the crate's own `{chunk_type, data}` envelope, which
/// is mapped back to the wire shape for the comparison.
fn probe_stream_event(value: Value) -> Result<(String, Value), String> {
    use crate::http::interactions::dispatch_stream_event;
    use crate::steps::StepAccumulator;
    use crate::wire_streaming::{InteractionStreamEvent, StreamEvent};

    let event: InteractionStreamEvent = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let event_id = event.event_id.clone();
    let chunk =
        dispatch_stream_event(event, &mut StepAccumulator::new()).ok_or("the event was dropped")?;
    let public = StreamEvent::new(chunk, event_id);
    let envelope = serde_json::to_value(&public).map_err(|e| e.to_string())?;
    let mut data = envelope.get("data").cloned().unwrap_or(Value::Null);
    let (event_type, wrap) = match envelope["chunk_type"].as_str().unwrap_or_default() {
        "created" => ("interaction.created", Some("interaction")),
        "completed" => ("interaction.completed", Some("interaction")),
        "status_update" => ("interaction.status_update", None),
        "step_start" => ("step.start", None),
        "step_delta" => ("step.delta", None),
        "step_stop" => ("step.stop", None),
        "error" => ("error", Some("error")),
        other => return Err(format!("unexpected chunk type {other:?}")),
    };
    if let Some(key) = wrap {
        data = json!({ key: data });
    }
    let mut wire = data.as_object().cloned().unwrap_or_default();
    wire.insert("event_type".into(), json!(event_type));
    if let Some(id) = envelope.get("event_id") {
        wire.insert("event_id".into(), id.clone());
    }
    Ok((format!("{public:?}"), Value::Object(wire)))
}

/// A response type and the class it is read from, in the bindings and the
/// spec.
struct Target {
    name: &'static str,
    binding: Option<&'static str>,
    spec: Option<&'static str>,
    probe: Probe,
}

fn targets() -> Vec<Target> {
    use crate::agents::{Agent, AgentListResponse};
    use crate::credentials::{Credential, CredentialListResponse};
    use crate::environments::{
        Environment, EnvironmentFile, EnvironmentFileList, EnvironmentListResponse,
    };
    use crate::file_search_stores::{
        DocumentListResponse, FileSearchDocument, FileSearchStore, FileSearchStoreListResponse,
    };
    use crate::files::{FileMetadata, ListFilesResponse};
    use crate::response::InteractionResponse;
    use crate::triggers::{
        Trigger, TriggerExecution, TriggerExecutionListResponse, TriggerListResponse,
    };
    use crate::voices::{Voice, VoiceListResponse};
    use crate::webhooks::{RotateSigningSecretResponse, Webhook, WebhookListResponse};

    let t = |name, binding, spec, probe| Target {
        name,
        binding,
        spec,
        probe,
    };
    vec![
        t(
            "InteractionResponse",
            Some("interactions.Interaction"),
            Some("Interaction"),
            probe::<InteractionResponse>,
        ),
        t(
            "StreamEvent",
            Some("interactions.InteractionSSEEvent"),
            Some("InteractionSseEvent"),
            probe_stream_event,
        ),
        t(
            "Webhook",
            Some("webhooks.Webhook"),
            Some("Webhook"),
            probe::<Webhook>,
        ),
        t(
            "WebhookListResponse",
            Some("webhooks.WebhookListResponse"),
            Some("ListWebhooksResponse"),
            probe::<WebhookListResponse>,
        ),
        t(
            "RotateSigningSecretResponse",
            Some("webhooks.WebhookRotateSigningSecretResponse"),
            Some("RotateSigningSecretResponse"),
            probe::<RotateSigningSecretResponse>,
        ),
        t(
            "Trigger",
            Some("triggers.Trigger"),
            Some("Trigger"),
            probe::<Trigger>,
        ),
        t(
            "TriggerListResponse",
            Some("triggers.ListTriggersResponse"),
            Some("ListTriggersResponse"),
            probe::<TriggerListResponse>,
        ),
        t(
            "TriggerExecution",
            Some("triggers.TriggerExecution"),
            Some("TriggerExecution"),
            probe::<TriggerExecution>,
        ),
        t(
            "TriggerExecutionListResponse",
            Some("triggers.ListTriggerExecutionsResponse"),
            Some("ListTriggerExecutionsResponse"),
            probe::<TriggerExecutionListResponse>,
        ),
        t("Agent", Some("agents.Agent"), Some("Agent"), probe::<Agent>),
        t(
            "AgentListResponse",
            Some("agents.AgentListResponse"),
            Some("ListAgentsResponse"),
            probe::<AgentListResponse>,
        ),
        t(
            "Environment",
            Some("environments.Environment"),
            Some("Environment"),
            probe::<Environment>,
        ),
        t(
            "EnvironmentListResponse",
            Some("environments.ListEnvironmentsResponse"),
            Some("ListEnvironmentsResponse"),
            probe::<EnvironmentListResponse>,
        ),
        t(
            "EnvironmentFile",
            Some("environments.EnvironmentFile"),
            Some("EnvironmentFile"),
            probe::<EnvironmentFile>,
        ),
        t(
            "EnvironmentFileList",
            Some("environments.GetEnvironmentFilesResponse"),
            Some("GetEnvironmentFilesResponse"),
            probe::<EnvironmentFileList>,
        ),
        t(
            "Credential",
            Some("credentials.Credential"),
            Some("Credential"),
            probe::<Credential>,
        ),
        t(
            "CredentialListResponse",
            Some("credentials.CredentialListResponse"),
            Some("ListCredentialsResponse"),
            probe::<CredentialListResponse>,
        ),
        t(
            "Voice",
            Some("voices.VoiceOutput"),
            Some("Voice"),
            probe::<Voice>,
        ),
        t(
            "VoiceListResponse",
            Some("voices.ListVoicesResponse"),
            Some("ListVoicesResponse"),
            probe::<VoiceListResponse>,
        ),
        t(
            "FileMetadata",
            Some("legacy.File"),
            None,
            probe::<FileMetadata>,
        ),
        t(
            "ListFilesResponse",
            Some("legacy.ListFilesResponse"),
            None,
            probe::<ListFilesResponse>,
        ),
        t(
            "FileSearchStore",
            Some("legacy.FileSearchStore"),
            None,
            probe::<FileSearchStore>,
        ),
        t(
            "FileSearchStoreListResponse",
            Some("legacy.ListFileSearchStoresResponse"),
            None,
            probe::<FileSearchStoreListResponse>,
        ),
        t(
            "FileSearchDocument",
            Some("legacy.Document"),
            None,
            probe::<FileSearchDocument>,
        ),
        t(
            "DocumentListResponse",
            Some("legacy.ListDocumentsResponse"),
            None,
            probe::<DocumentListResponse>,
        ),
    ]
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

fn count(items: &[String], item: &str) -> usize {
    items.iter().filter(|i| *i == item).count()
}

/// Everything wrong with one body. `traced` holds the sites already traced
/// to an `extra` key or an `Unknown` value in an earlier body, whose
/// candidates need no second round of removals.
fn analyze(
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

    // A single object the crate keeps as a one-element list (`input`) still
    // counts as kept.
    let mut kept = key_paths(&back);
    // So does an `input` the crate re-sends wrapped in a `user_input` step
    // (a trigger's interaction is a request).
    let unwrapped: Vec<String> = kept
        .iter()
        .flat_map(|p| {
            [
                p.replace("[0]", ""),
                p.replace(".input[0].content[0]", ".input"),
            ]
        })
        .collect();
    kept.extend(unwrapped);
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

fn findings(surface: &Surface, pick: impl Fn(&Target) -> Option<&'static str>) -> Vec<Finding> {
    let options = SynthOptions {
        values: values(),
        ..SynthOptions::default()
    };
    let mut out = BTreeSet::new();
    let mut traced = BTreeSet::new();
    for target in targets() {
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

#[test]
fn response_fields_from_the_bindings_are_modeled() {
    let surface = Surface::bindings();
    assert_clean(
        &format!(
            "Response fields vs google-genai {} bindings",
            surface.version
        ),
        &surface,
        findings(&surface, |t| t.binding),
        &[("ALLOWED", ALLOWED), ("GAPS", GAPS)],
    );
}

#[test]
fn response_fields_from_the_spec_are_modeled() {
    let surface = Surface::openapi("openapi-v1beta.json");
    assert_clean(
        &format!(
            "Response fields vs the v1beta OpenAPI spec ({})",
            surface.version
        ),
        &surface,
        findings(&surface, |t| t.spec),
        &[("ALLOWED", ALLOWED), ("GAPS", GAPS)],
    );
}
