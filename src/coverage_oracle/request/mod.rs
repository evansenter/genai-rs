//! Request side: every key a request value emits exists on its class.
//!
//! Each request type is serialized fully populated, as struct literals with
//! no `..Default::default()` and no constructor between them and the value,
//! so a field added to one of these structs fails to compile here until it
//! is populated and checked. `fills` scans this directory, and the
//! strategies it samples, for a `default()` fill, and this directory for a
//! call to a crate type's associated function (`CustomMetadata::string`),
//! which can leave a new field unset, unless its `TRUSTED` list gives the
//! reason one sets every field. (The agent configs keep their fields
//! private, so each builds its own literal, `fully_populated()` in
//! `src/request/agent_config.rs`; `agent_configs_set_every_field` fails on
//! a field there that sends no key.) Samples from the proptest strategies
//! add the union arms and enum values the literals do not reach. Each value is walked, recursively and
//! path by path, against the google-genai binding class it is sent as
//! (rank 1, D-004; the spec lags the bindings on requests, so it is checked
//! on the response side only).
//!
//! The reverse is checked too: every field of a request class that no value
//! here sends is a known gap, listed below with its reason.

mod bodies;
mod content;
// Reads the response targets, which strict mode does not build.
#[cfg(not(genai_strict_unknown))]
mod enum_values;
mod fills;

#[cfg(not(genai_strict_unknown))]
pub(super) use enum_values::NOT_SENT_VALUES;
pub(super) use fills::TRUSTED;

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::api_surface::{Finding, Surface, debug_extra_keys};
use super::{Allowed, assert_clean};
use crate::content::Content;
use crate::proptest_tests as strategies;
use crate::steps::Step;
use bodies::{
    agent_config_types, agent_configs, agent_requests, agents, credential_requests,
    credential_updates, document_imports, document_uploads, environment_requests,
    environment_specs, file_search_store_requests, generation_configs, inputs, model_requests,
    response_formats, safety_settings, samples, to_value, trigger_params, trigger_updates, values,
    voice_requests, webhook_config, webhook_updates, webhooks,
};
use content::{contents, steps, tools};

/// Keys the crate sends that a class lacks, each on purpose.
pub(super) const ALLOWED: &[Allowed] = &[
    (
        "interactions.FunctionCallStep.signature",
        "Served on function_call steps and required when one is replayed in \
         stateless history (live 2026-07, src/steps/mod.rs); the bindings omit it.",
    ),
    (
        "interactions.FunctionResultStep.signature",
        "Served on the function_result of a server-run MCP call (live \
         2026-11-05, a deepwiki call on gemini-3.8-flash), so a replayed \
         history step sends it back; not in the bindings.",
    ),
    (
        "legacy.ImportFileConfig.fileName",
        "google-genai's `import_file` takes `file_name` as its own argument and \
         sends it in the body beside the config's fields; the API requires it \
         there (live 2026-11-05).",
    ),
];

/// Request fields the crate never sends.
pub(super) const NOT_SENT: &[Allowed] = &[
    // Deprecated in the bindings, and either superseded or rejected live.
    (
        "interactions.CreateModelInteraction.response_mime_type",
        "Deprecated; superseded by response_format, and removed from the crate.",
    ),
    (
        "interactions.CreateAgentInteraction.response_mime_type",
        "Deprecated; superseded by response_format, and removed from the crate.",
    ),
    (
        "interactions.CreateModelInteraction.cached_content",
        "Deprecated; rejected live with 400 Unknown parameter (2026-09-24, \
         the 2.25.0 sweep in docs/INTERACTIONS_API_GAP.md; D-005).",
    ),
    (
        "interactions.TranscriptionConfig.language_hints",
        "Deprecated; rejected live with 400 Unknown parameter (2026-09-24, \
         the 2.25.0 sweep in docs/INTERACTIONS_API_GAP.md; D-005).",
    ),
    (
        "interactions.SpeakerConfig.speakers",
        "The crate sends speech_config as the list arm, accepted on every TTS model \
         (live 2026-09-24); the {speakers} object arm is read, never sent.",
    ),
    (
        "voices.VoiceInput.sample_audio",
        "Output only: a sample sent on create is ignored, and the response carries \
         one the API generated (live 2026-11-05). Read on `Voice::sample_audio`.",
    ),
];

// =============================================================================
// The table
// =============================================================================

/// A request type, the binding class it is sent as, and the values to check.
struct Case {
    name: &'static str,
    binding: &'static str,
    values: Vec<Value>,
}

fn cases() -> Vec<Case> {
    const N: usize = 64;
    let case = |name, binding, values| Case {
        name,
        binding,
        values,
    };
    vec![
        case(
            "InteractionRequest (model)",
            "interactions.CreateModelInteraction",
            values(model_requests()),
        ),
        case(
            "InteractionRequest (agent)",
            "interactions.CreateAgentInteraction",
            values(agent_requests()),
        ),
        case(
            "GenerationConfig",
            "interactions.GenerationConfig",
            [
                values(generation_configs()),
                samples(strategies::arb_generation_config(), N),
            ]
            .concat(),
        ),
        case(
            "ToolChoice",
            "interactions.ToolChoice",
            samples(strategies::arb_tool_choice(), N),
        ),
        case(
            "SpeechConfig",
            "interactions.SpeechConfig",
            samples(strategies::arb_speech_config(), N),
        ),
        case(
            "TranscriptionConfig",
            "interactions.TranscriptionConfig",
            samples(strategies::arb_transcription_config(), N),
        ),
        case("Tool", "interactions.Tool", values(tools())),
        case(
            "SafetySetting",
            "interactions.SafetySetting",
            [
                values(safety_settings()),
                samples(strategies::arb_safety_setting(), N),
            ]
            .concat(),
        ),
        case(
            "AgentConfig",
            "interactions.CreateAgentInteractionAgentConfig",
            values(agent_configs()),
        ),
        case(
            "ResponseFormat",
            "interactions.ResponseFormat",
            [
                values(response_formats()),
                samples(strategies::arb_response_format(), N),
            ]
            .concat(),
        ),
        case(
            "ResponseFormatSpec",
            "interactions.CreateModelInteractionResponseFormat",
            samples(strategies::arb_response_format_spec(), N),
        ),
        case(
            "EnvironmentSpec",
            "interactions.CreateModelInteractionEnvironment",
            [
                values(environment_specs()),
                samples(strategies::arb_environment_spec(), N),
            ]
            .concat(),
        ),
        case(
            "WebhookConfig",
            "interactions.WebhookConfig",
            [
                vec![to_value(&webhook_config())],
                samples(strategies::arb_webhook_config(), N),
            ]
            .concat(),
        ),
        case(
            "InteractionInput",
            "interactions.InteractionsInput",
            values(inputs())
                .into_iter()
                .map(wrap_content_input)
                .collect(),
        ),
        case("Step (history)", "interactions.Step", values(steps())),
        case("Content", "interactions.Content", values(contents())),
        case("Webhook (create)", "webhooks.WebhookInput", webhooks()),
        case("WebhookUpdate", "webhooks.WebhookUpdate", webhook_updates()),
        case(
            "CreateTriggerRequest",
            "triggers.TriggerCreateParams",
            trigger_params(),
        ),
        case("TriggerUpdate", "triggers.TriggerUpdate", trigger_updates()),
        case("Agent (create)", "agents.Agent", agents()),
        case(
            "CreateEnvironmentRequest",
            "environments.CreateEnvironmentRequest",
            environment_requests(),
        ),
        case(
            "CreateCredentialRequest",
            "credentials.CredentialCreateParams",
            credential_requests(),
        ),
        case(
            "CredentialUpdate",
            "credentials.CredentialUpdate",
            credential_updates(),
        ),
        case(
            "CreateVoiceRequest",
            "voices.CreateVoiceRequest",
            voice_requests(),
        ),
        case(
            "CreateFileSearchStoreRequest",
            "legacy.CreateFileSearchStoreConfig",
            file_search_store_requests(),
        ),
        case(
            "DocumentUpload",
            "legacy.UploadToFileSearchStoreConfig",
            document_uploads(),
        ),
        case(
            "DocumentImport",
            "legacy.ImportFileConfig",
            document_imports(),
        ),
    ]
}

/// The binding class of every case, for the completeness check in
/// [`super`].
#[cfg(not(genai_strict_unknown))]
pub(super) fn case_classes() -> Vec<&'static str> {
    cases().into_iter().map(|case| case.binding).collect()
}

/// `InteractionInput` serializes faithfully (a bare content array); only the
/// request field wraps content in a `user_input` step. Check the wrapped
/// shape, which is what goes on the wire.
fn wrap_content_input(value: Value) -> Value {
    match &value {
        Value::Array(items)
            if !items.is_empty()
                && items.iter().all(|i| {
                    matches!(
                        i["type"].as_str(),
                        Some("text" | "image" | "audio" | "video" | "document")
                    )
                }) =>
        {
            json!([{"type": "user_input", "content": value}])
        }
        _ => value,
    }
}

fn findings(surface: &Surface) -> Vec<Finding> {
    let mut out = Vec::new();
    for case in cases() {
        let class = case.binding;
        assert!(!case.values.is_empty(), "{}: no values", case.name);
        let ty = surface.ty(class);
        for value in &case.values {
            for mut finding in surface.check(&ty, value) {
                finding.path = format!("{} {}", case.name, finding.path);
                out.push(finding);
            }
        }
    }
    out
}

#[test]
fn request_wire_keys_exist_in_bindings() {
    let surface = Surface::bindings();
    assert_clean(
        &format!("Request keys vs google-genai {} bindings", surface.version),
        &surface,
        findings(&surface),
        &[("ALLOWED", ALLOWED)],
    );
}

/// Renames `from` to `to` in the object at the top of `value`.
fn rename_key(mut value: Value, from: &str, to: &str) -> Value {
    let object = value.as_object_mut().expect("an object");
    let moved = object.remove(from).unwrap_or_else(|| panic!("no {from:?}"));
    object.insert(to.into(), moved);
    value
}

/// The check catches the bugs it exists for, at every depth: SafetySetting's
/// pre-0.12.0 `category` key, and a wrong key inside `response_format`,
/// whose union has a free-form `Dict[str, Any]` arm that would take any
/// object if a tagged value were not held to its tagged arm.
#[test]
fn wrong_request_keys_are_reported() {
    let surface = Surface::bindings();
    let safety = rename_key(to_value(&safety_settings()[0]), "type", "category");
    let audio = rename_key(
        to_value(&response_formats()[1]),
        "sample_rate",
        "sampleRate",
    );
    assert_eq!(audio["type"], "audio");
    let mut bogus = to_value(&response_formats()[0]);
    bogus["bogus_key"] = json!(1);
    let mut request = to_value(&model_requests()[0]);
    request["safety_settings"] = json!([safety]);
    request["response_format"] = json!([audio]);
    let cases = [
        (
            "interactions.SafetySetting",
            safety.clone(),
            "interactions.SafetySetting.category",
        ),
        (
            "interactions.ResponseFormat",
            audio.clone(),
            "interactions.AudioResponseFormat.sampleRate",
        ),
        (
            "interactions.ResponseFormat",
            bogus,
            "interactions.TextResponseFormat.bogus_key",
        ),
        (
            "interactions.CreateModelInteraction",
            request.clone(),
            "interactions.SafetySetting.category",
        ),
        (
            "interactions.CreateModelInteraction",
            request,
            "interactions.AudioResponseFormat.sampleRate",
        ),
    ];
    for (class, value, site) in cases {
        let sites: BTreeSet<String> = surface
            .check(&surface.ty(class), &value)
            .into_iter()
            .map(|f| f.site)
            .collect();
        assert!(
            sites.contains(site),
            "{class}: {site} not reported; got {sites:?}"
        );
    }
}

/// `InteractionRequest::extra` is caller-supplied passthrough: its keys are
/// the caller's to choose and the server's to validate (`400 Unknown
/// parameter`, live 2026-11-04), not keys the crate emits. So every case
/// leaves it empty, as with every other request type's `extra`, and this
/// pins why that must hold. A populated `extra` is sent flat, beside the
/// modeled keys, so the checks read its keys as the class's own: an unknown
/// one is reported like a key the crate sent by mistake, and one that names
/// a binding field counts as sent in the reverse check without the crate
/// modeling it (here `cached_content`, which [`NOT_SENT`] lists).
#[test]
fn a_populated_extra_is_sent_flat_and_not_vouched_for() {
    for request in model_requests().iter().chain(&agent_requests()) {
        assert!(request.extra.is_empty(), "{:?}", request.extra);
    }
    let surface = Surface::bindings();
    let ty = surface.ty("interactions.CreateModelInteraction");
    let mut request = model_requests().remove(0);
    request
        .extra
        .insert("brand_new_field".into(), json!({"on": true}));
    request
        .extra
        .insert("cached_content".into(), json!("cachedContents/c"));
    let body = to_value(&request);
    assert_eq!(body["brand_new_field"], json!({"on": true}), "sent flat");
    assert!(body.get("extra").is_none(), "{body}");

    let reported: BTreeSet<String> = surface
        .check(&ty, &body)
        .into_iter()
        .map(|f| f.site)
        .collect();
    assert_eq!(
        reported,
        BTreeSet::from(["interactions.CreateModelInteraction.brand_new_field".to_string()])
    );
    let site = "interactions.CreateModelInteraction.cached_content";
    assert!(NOT_SENT.iter().any(|(entry, _)| *entry == site));
    assert!(
        surface.sites(&ty, &body).values().any(|sent| sent == site),
        "an extra key naming a binding field reads as sent"
    );
}

/// The same holds one level down, for the wire fields a response carried
/// that the crate does not model (#491): the `extra` of every known `Step`,
/// `Content` and `Annotation` variant, the `arguments_extra` of the five
/// call steps, and the `extra` of the result items, places, review
/// snippets, step errors and static video segments inside them. They are
/// echoed when history is replayed, not emitted by the crate, so the
/// literals in `content.rs` leave every one empty, and the checks vouch
/// only for modeled fields. A populated one is sent flat, in its own object
/// (a call's `arguments` for `arguments_extra`), so its keys read as that
/// class's own: one the bindings lack is reported at the class it sits on,
/// like a key the crate sent by mistake.
#[test]
fn a_populated_variant_extra_is_sent_flat_and_not_vouched_for() {
    for debug in steps()
        .iter()
        .map(|step| format!("{step:?}"))
        .chain(contents().iter().map(|content| format!("{content:?}")))
    {
        assert!(debug_extra_keys(&debug).is_empty(), "{debug}");
    }

    let surface = Surface::bindings();
    let ty = surface.ty("interactions.Step");
    let sites = |steps: &[Step]| -> BTreeSet<String> {
        steps
            .iter()
            .flat_map(|step| surface.check(&ty, &to_value(step)))
            .map(|f| f.site)
            .collect()
    };
    let new = || serde_json::Map::from_iter([("brand_new_field".to_string(), json!(1))]);
    let mut populated = steps();
    for step in &mut populated {
        match step {
            Step::FunctionCall { extra, .. } => *extra = new(),
            Step::CodeExecutionCall {
                arguments_extra, ..
            } => *arguments_extra = new(),
            Step::ModelOutput { content, error, .. } => {
                error.as_mut().expect("an error").extra = new();
                let Content::Text {
                    extra, annotations, ..
                } = &mut content[0]
                else {
                    panic!("text first");
                };
                *extra = new();
                let citation = &mut annotations.as_mut().expect("annotations")[0];
                *citation.extra_mut().expect("a known annotation") = new();
            }
            Step::GoogleMapsResult { result, .. } => {
                result[0].places.as_mut().expect("places")[0].extra = new();
            }
            _ => {}
        }
    }
    let wire = |step_type: &str| {
        let step = populated.iter().find(|s| s.step_type() == step_type);
        to_value(step.expect("the step is in the history"))
    };
    let call = wire("function_call");
    assert_eq!(call["brand_new_field"], 1, "sent flat: {call}");
    let code = wire("code_execution_call");
    assert_eq!(code["arguments"]["brand_new_field"], 1, "{code}");

    let before = sites(&steps());
    let added: BTreeSet<String> = sites(&populated).difference(&before).cloned().collect();
    let expected = [
        "interactions.FunctionCallStep",
        "interactions.CodeExecutionCallArguments",
        "interactions.Status",
        "interactions.TextContent",
        "interactions.URLCitation",
        "interactions.GoogleMapsResultPlaces",
    ]
    .map(|class| format!("{class}.brand_new_field"));
    assert_eq!(added, BTreeSet::from(expected));
}

/// The field names in a `{:?}` string: each identifier that opens a field
/// (after the type's `{` or a `,`) and is followed by `: `. String literals
/// are blanked first, so a `: ` inside one is not read as a field.
fn debug_field_names(debug: &str) -> BTreeSet<String> {
    let mut blanked = String::with_capacity(debug.len());
    let (mut in_string, mut escaped) = (false, false);
    for c in debug.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                blanked.push(c);
            }
            continue;
        }
        in_string = c == '"';
        blanked.push(c);
    }
    blanked
        .split(['{', ','])
        .filter_map(|segment| segment.trim_start().split_once(": "))
        .map(|(name, _)| name)
        .filter(|name| {
            !name.is_empty() && name.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
        })
        .map(String::from)
        .collect()
}

/// Every object key in `value`, at any depth.
fn object_keys(value: &Value) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                keys.extend(map.keys().cloned());
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    keys
}

/// The fields of a config, as its `{:?}` names them, that name no key of
/// the JSON it emits. A key counts at any depth, so `max_rounds`, sent
/// inside `session_config`, is sent.
fn unsent_config_fields(debug: &str, emitted: &Value) -> Vec<String> {
    let keys = object_keys(emitted);
    debug_field_names(debug)
        .into_iter()
        .filter(|field| !keys.contains(field))
        .collect()
}

/// The agent configs' `fully_populated()` literals set every field to a
/// value `From` sends. A field added to a config type and set to what
/// `From` leaves out (`None`, an empty list, `false`) to make the literal
/// compile would emit no key, and so go unchecked: every field named in a
/// config's `{:?}` must name a key of its JSON. (Sent, the key fails
/// `request_wire_keys_exist_in_bindings` unless the bindings have it.) A
/// field `From` sends under another name needs that name here. `None` and
/// `[]` are rejected outright too, since another field may send the name.
#[test]
fn agent_configs_set_every_field() {
    let types = agent_config_types();
    assert_eq!(types.len(), 4, "one per typed agent config");
    for (debug, config) in &types {
        let unsent = unsent_config_fields(debug, config.as_value());
        assert!(
            unsent.is_empty(),
            "{unsent:?} send no key: {debug} emits {}",
            config.as_value()
        );
        for unset in [": None", ": []"] {
            assert!(
                !debug.contains(unset),
                "a field is unset ({unset}): {debug}"
            );
        }
    }
}

/// The verifier's mutation in the form the `None` check misses: a `bool`
/// flag set to `false` in the literal, which `From` sends only when true.
#[test]
fn a_config_field_that_sends_no_key_is_caught() {
    let debug = r#"AntigravityConfig { model: Some("m: x, y"), max_total_tokens: Some(1000), mut_budget: false }"#;
    let emitted = json!({"type": "antigravity", "model": "m: x, y", "max_total_tokens": 1000});
    assert_eq!(unsent_config_fields(debug, &emitted), ["mut_budget"]);

    let nested = r#"CodeMenderConfig { max_rounds: Some(3), find_request: Some(CodeMenderFindRequest { source_files: [CodeMenderFile { path: Some("a") }] }) }"#;
    let emitted = json!({
        "session_config": {"max_rounds": 3},
        "find_request": {"source_files": [{"path": "a"}]},
    });
    assert!(unsent_config_fields(nested, &emitted).is_empty());
}

/// The other direction: every field of every class a request reaches is
/// sent by some value above, or listed in [`NOT_SENT`] with its reason.
#[test]
fn request_classes_have_no_unsent_fields() {
    let surface = Surface::bindings();
    let mut sent: BTreeSet<String> = BTreeSet::new();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    for case in cases() {
        let ty = surface.ty(case.binding);
        reached.extend(surface.reachable_classes(&ty));
        for value in &case.values {
            sent.extend(surface.sites(&ty, value).into_values());
        }
    }
    let mut findings = Vec::new();
    for class in &reached {
        let Some(fields) = surface.classes.get(class) else {
            continue;
        };
        for (wire, field) in &fields.fields {
            let site = format!("{class}.{wire}");
            if !sent.contains(&site) {
                findings.push(Finding {
                    path: class.clone(),
                    site,
                    message: format!(
                        "never sent{}",
                        if field.deprecated {
                            " (deprecated)"
                        } else {
                            ""
                        }
                    ),
                });
            }
        }
    }
    assert_clean(
        &format!(
            "Request-class fields no value sends (google-genai {})",
            surface.version
        ),
        &surface,
        findings,
        &[("NOT_SENT", NOT_SENT)],
    );
}
