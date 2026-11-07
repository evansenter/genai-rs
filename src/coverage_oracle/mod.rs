//! The coverage oracle: the crate's wire types checked against the committed
//! API-surface snapshot in `tests/fixtures/api_surface/` (google-genai's
//! bindings and the OpenAPI spec), offline.
//!
//! - [`request`]: every key a fully populated request value emits, at any
//!   depth, exists on the binding class (and spec schema) it is sent as.
//! - [`response`]: a body carrying every field, union arm and enum value a
//!   response class defines deserializes with an empty `extra`, no `Unknown`
//!   variant and nothing dropped.
//!
//! Every exception is a `(site, reason)` entry, where a site is
//! `Class.field` (`interactions.Interaction.agent_config` for the bindings,
//! `Interaction.agent_config` for the spec) or `Class.field=value` for an
//! enum value. An entry nothing matches fails the test, so the lists only
//! hold live exceptions. `docs/TESTING.md` says how to read a failure and how
//! to refresh the snapshot.

#[path = "../../tests/api_surface/mod.rs"]
mod api_surface;
mod request;
// Under `genai_strict_unknown` an unmodeled enum value fails a whole body instead
// of showing as `Unknown`; the default build is the one the response side
// measures.
#[cfg(not(genai_strict_unknown))]
mod response;
#[cfg(not(genai_strict_unknown))]
mod response_minimal;
#[cfg(not(genai_strict_unknown))]
mod response_targets;

use api_surface::{Finding, Surface};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// A reasoned exception: the site (or `prefix.*`) it covers, and why.
type Allowed = (&'static str, &'static str);

fn covers(entry: &str, site: &str) -> bool {
    match entry.strip_suffix('*') {
        Some(prefix) => site.starts_with(prefix),
        None => entry == site,
    }
}

/// Whether a site belongs to the bindings (`interactions.Interaction.id`,
/// lowercase namespace first) rather than the spec (`Interaction.id`).
fn is_binding_site(site: &str) -> bool {
    site.starts_with(|c: char| c.is_ascii_lowercase())
}

/// Fails with every finding no entry covers, grouped by site, and with every
/// entry for this surface that covered nothing.
fn assert_clean(
    what: &str,
    surface: &Surface,
    findings: Vec<Finding>,
    lists: &[(&str, &[Allowed])],
) {
    let bindings = surface.name == "bindings";
    let lists: Vec<(&str, Vec<Allowed>)> = lists
        .iter()
        .map(|(name, entries)| {
            let mine = entries
                .iter()
                .copied()
                .filter(|(entry, _)| is_binding_site(entry) == bindings)
                .collect();
            (*name, mine)
        })
        .collect();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    let mut open: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    for finding in findings {
        let hit = lists
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .find(|(entry, _)| covers(entry, &finding.site));
        match hit {
            Some((entry, _)) => {
                used.insert(entry);
            }
            None => open.entry(finding.site.clone()).or_default().push(finding),
        }
    }
    let stale: Vec<String> = lists
        .iter()
        .flat_map(|(list, entries)| entries.iter().map(move |(entry, _)| (list, entry)))
        .filter(|(_, entry)| !used.contains(*entry))
        .map(|(list, entry)| format!("{list}: {entry}"))
        .collect();

    let mut report = String::new();
    for (site, findings) in &open {
        let first = &findings[0];
        report.push_str(&format!(
            "\n  {site}: {} (at {}{})",
            first.message,
            first.path,
            if findings.len() > 1 {
                format!(", and {} more", findings.len() - 1)
            } else {
                String::new()
            }
        ));
    }
    assert!(
        open.is_empty() && stale.is_empty(),
        "{what}\n\nUnexplained findings ({}): fix the crate, or add a reasoned entry to \
         the allowlist that fits (see docs/TESTING.md, \"Coverage oracle\"):{report}\n\n\
         Stale entries ({}), which no longer match anything; delete them:\n  {}\n",
        open.len(),
        stale.len(),
        stale.join("\n  ")
    );
}

/// Binding classes no request case or response target reaches, each with
/// its reason. (Like the response side, checked in the default build only.)
#[cfg(not(genai_strict_unknown))]
const UNMAPPED: &[Allowed] = &[
    (
        "interactions.Empty",
        "The empty body some deletes return; the crate reads no body there.",
    ),
    (
        "interactions.InteractionSSEStreamEvent",
        "The SSE frame (`{data: InteractionSSEEvent}`): the crate parses the \
         `data:` line itself (src/http/sse_parser.rs), and the response side \
         checks InteractionSSEEvent.",
    ),
    (
        "types.Security",
        "The SDK's client credentials (api key, access token), not API surface.",
    ),
    (
        "types.Unset",
        "The SDK's sentinel for an unset field, not API surface.",
    ),
    (
        "voices.DeleteVoiceResponse",
        "Empty; `Voices::delete` reads no body.",
    ),
    (
        "webhooks.PingWebhookRequest",
        "Empty. Checked at the wire by tests/request_wire_oracle.rs: \
         `Webhooks::ping` sends `{}`.",
    ),
    (
        "webhooks.WebhookPingResponse",
        "Empty; `Webhooks::ping` returns `()`.",
    ),
    (
        "webhooks.RotateSigningSecretRequest",
        "Checked at the wire by tests/request_wire_oracle.rs: \
         `rotate_signing_secret` builds the body itself from its arguments.",
    ),
];

/// Every binding class is reached from a request case or a response target,
/// or listed in [`UNMAPPED`] with its reason. The checks only see classes
/// those tables reach, so a class a baseline bump adds (a new resource, a
/// new request body) fails here until it is mapped.
#[cfg(not(genai_strict_unknown))]
#[test]
fn every_binding_class_is_mapped() {
    let surface = Surface::bindings();
    let mut reached = BTreeSet::new();
    for class in request::case_classes()
        .into_iter()
        .chain(response_targets::target_classes())
    {
        reached.extend(surface.reachable_classes(&surface.ty(class)));
    }
    let findings = surface
        .classes
        .keys()
        .filter(|class| !reached.contains(*class))
        .map(|class| Finding {
            path: class.clone(),
            site: class.clone(),
            message: "no request case or response target reaches it".into(),
        })
        .collect();
    assert_clean(
        &format!(
            "Binding classes the oracle does not check (google-genai {})",
            surface.version
        ),
        &surface,
        findings,
        &[("UNMAPPED", UNMAPPED)],
    );
}

/// The OpenAPI snapshots the response side checks. v1 defines a subset of
/// v1beta's schemas (the interactions, none of the resources) with a few
/// values of its own (`v1_only_values_are_checked`), and the crate speaks
/// both (`ClientBuilder::with_api_version`). Each spec test runs over both
/// and reads one set of exceptions: an entry is stale only when neither
/// file matches it.
#[cfg(not(genai_strict_unknown))]
const SPECS: [&str; 2] = ["openapi-v1beta.json", "openapi-v1.json"];

/// A schema or alias `surface` defines.
#[cfg(not(genai_strict_unknown))]
fn defines(surface: &Surface, name: &str) -> bool {
    surface.classes.contains_key(name) || surface.aliases.contains_key(name)
}

/// A target's spec schema in `surface`. v1beta defines every target's, so a
/// missing one fails there (in `Surface::ty`); v1 defines only the
/// interactions' (`InteractionResponse`, `StreamEvent`), and its other
/// targets are skipped.
#[cfg(not(genai_strict_unknown))]
fn spec_class(surface: &Surface, target: &response_targets::Target) -> Option<&'static str> {
    target
        .spec
        .filter(|class| surface.name == "openapi-v1beta" || defines(surface, class))
}

/// Spec schemas no response target reaches, each with its reason. Requests
/// are checked against the bindings only, which lead the spec there (see
/// [`request`]), so the request bodies are here.
#[cfg(not(genai_strict_unknown))]
const SPEC_UNMAPPED: &[Allowed] = &[
    ("CreateEnvironmentRequest", REQUEST_BODY),
    ("CreateModelInteractionParams", REQUEST_BODY),
    (
        "CreateAgentInteractionParams",
        "A request body (see REQUEST_BODY). v1beta reaches it from a \
         response too, `Trigger.interaction`; v1 has no triggers.",
    ),
    ("CreateVoiceRequest", REQUEST_BODY),
    ("CreateWebhookRequest", REQUEST_BODY),
    ("EnvironmentVariableConfig", REQUEST_BODY),
    ("EnvironmentVariableUpdateConfig", REQUEST_BODY),
    ("HttpBearerConfig", REQUEST_BODY),
    ("HttpBearerUpdateConfig", REQUEST_BODY),
    ("OAuth2Config", REQUEST_BODY),
    ("OAuth2UpdateConfig", REQUEST_BODY),
    ("PingWebhookRequest", REQUEST_BODY),
    ("RotateSigningSecretRequest", REQUEST_BODY),
    ("RunTriggerRequest", REQUEST_BODY),
    ("TriggerUpdate", REQUEST_BODY),
    ("UpdateTriggerRequest", REQUEST_BODY),
    ("UpdateWebhookRequest", REQUEST_BODY),
    ("WebhookUpdate", REQUEST_BODY),
    (
        "UploadEnvironmentFileRequest",
        "The upload's options, which `EnvironmentFiles::upload` sends as the \
         `overwrite` and `extract` query parameters; the start request has no \
         body.",
    ),
    (
        "HttpBody",
        "A raw media body: `environments.files.download` serves the file's \
         bytes under its own content type (live 2026-11-05), which \
         `EnvironmentFiles::download` returns as they come.",
    ),
    (
        "InteractionSseStreamEvent",
        "The SSE frame (`{data: InteractionSseEvent}`): the crate parses the \
         `data:` line itself (src/http/sse_parser.rs), and the response side \
         checks InteractionSseEvent.",
    ),
];

#[cfg(not(genai_strict_unknown))]
const REQUEST_BODY: &str = "A request body, or part of one: the request side \
     checks what the crate sends against its binding class, which leads the \
     spec on requests.";

/// The spec twin of [`every_binding_class_is_mapped`]: every schema each
/// spec snapshot defines is reached from a response target, or listed in
/// [`SPEC_UNMAPPED`] with its reason. A spec-only response the crate parses
/// (`UploadEnvironmentFileResponse`, which the bindings fold into
/// `GetEnvironmentFilesResponse`) is a target of its own, so a schema a new
/// spec adds fails here until it is mapped.
#[cfg(not(genai_strict_unknown))]
#[test]
fn every_spec_schema_is_mapped() {
    let mut findings = Vec::new();
    let mut fetched = Vec::new();
    for file in SPECS {
        let surface = Surface::openapi(file);
        let mut reached = BTreeSet::new();
        for target in response_targets::targets() {
            if let Some(class) = spec_class(&surface, &target) {
                reached.extend(surface.reachable_classes(&surface.ty(class)));
            }
        }
        findings.extend(
            surface
                .classes
                .keys()
                .filter(|class| !reached.contains(*class))
                .map(|class| Finding {
                    path: format!("{} {class}", surface.name),
                    site: class.clone(),
                    message: "no response target reaches it".into(),
                }),
        );
        fetched.push(format!("{} {}", surface.name, surface.version));
    }
    assert_clean(
        &format!(
            "Spec schemas the oracle does not check ({})",
            fetched.join(", ")
        ),
        &Surface::openapi(SPECS[0]),
        findings,
        &[("SPEC_UNMAPPED", SPEC_UNMAPPED)],
    );
}

/// Every exception list's entries, as `(list, entry, reason)`.
fn reasons() -> Vec<(&'static str, &'static str, &'static str)> {
    let lists: &[(&str, &[Allowed])] = &[
        ("request ALLOWED", request::ALLOWED),
        ("request NOT_SENT", request::NOT_SENT),
        #[cfg(not(genai_strict_unknown))]
        ("UNMAPPED", UNMAPPED),
        #[cfg(not(genai_strict_unknown))]
        ("SPEC_UNMAPPED", SPEC_UNMAPPED),
        #[cfg(not(genai_strict_unknown))]
        ("response ALLOWED", response::ALLOWED),
        #[cfg(not(genai_strict_unknown))]
        ("response GAPS", response::GAPS),
        #[cfg(not(genai_strict_unknown))]
        ("SPARSE", response_minimal::SPARSE),
    ];
    lists
        .iter()
        .flat_map(|&(list, entries)| {
            entries
                .iter()
                .map(move |(entry, reason)| (list, *entry, *reason))
        })
        .collect()
}

/// Whether a reason states what the API did, or does (a word like
/// "served", "rejected", "accepted" or "live", or an HTTP error status),
/// without the `YYYY-MM` date of the observation (D-010): a live fact goes
/// stale, and the date says how stale.
fn undated_live_claim(reason: &str) -> bool {
    const STEMS: &[&str] = &[
        "live", "serv", "reject", "accept", "ignor", "observ", "seen", "probe",
    ];
    let lower = reason.to_lowercase();
    let claims = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| {
            STEMS.iter().any(|stem| word.starts_with(stem))
                || (word.len() == 3
                    && word.starts_with(['4', '5'])
                    && word.bytes().all(|b| b.is_ascii_digit()))
        });
    let dated = reason.as_bytes().windows(7).any(|w| {
        w.starts_with(b"20")
            && w[2..4].iter().all(u8::is_ascii_digit)
            && w[4] == b'-'
            && w[5..7].iter().all(u8::is_ascii_digit)
    });
    claims && !dated
}

/// Every reason that states a live fact dates it.
#[test]
fn live_claims_are_dated() {
    let undated: Vec<String> = reasons()
        .into_iter()
        .filter(|(_, _, reason)| undated_live_claim(reason))
        .map(|(list, entry, reason)| format!("{list}: {entry}: {reason}"))
        .collect();
    assert!(
        undated.is_empty(),
        "Reasons that state a live fact without its YYYY-MM date (D-010):\n  {}",
        undated.join("\n  ")
    );
}

/// The check rejects the undated reasons it was written for and passes
/// their dated forms, and leaves a reason that claims nothing live alone.
#[test]
fn an_undated_live_claim_is_rejected() {
    for undated in [
        "The protobuf default; never served.",
        "Deprecated; rejected live with 400 Unknown parameter (2.25.0 sweep, \
         docs/INTERACTIONS_API_GAP.md, D-005).",
        "A function tool sent with only a name is accepted.",
        "Answered with a 404.",
    ] {
        assert!(undated_live_claim(undated), "{undated}");
    }
    for fine in [
        "Deprecated; rejected live with 400 Unknown parameter (2026-09-24).",
        "Served on function_call steps (live 2026-07).",
        "Empty; `Voices::delete` reads no body.",
        "The SDK's sentinel for an unset field, not API surface.",
    ] {
        assert!(!undated_live_claim(fine), "{fine}");
    }
}

/// The types `document_from_file()` sends as documents are the bindings'
/// document MIME lists, so a baseline bump that adds a type fails here until
/// the type is probed live and added to `DOCUMENT_MIME_TYPES`. A type the
/// endpoint rejects would need an exception here with its dated reason.
#[test]
fn document_mime_types_are_the_bindings_list() {
    let surface = Surface::bindings();
    let sent: BTreeSet<&str> = crate::multimodal::DOCUMENT_MIME_TYPES
        .iter()
        .copied()
        .collect();
    for alias in [
        "interactions.DocumentContentMimeType",
        "interactions.DocumentDeltaMimeType",
    ] {
        let Some(api_surface::Ty::Literal { values, .. }) = surface.aliases.get(alias) else {
            panic!("{alias} is not a literal alias in the snapshot");
        };
        let listed: BTreeSet<&str> = values
            .iter()
            .map(|v| v.as_str().expect("a string literal"))
            .collect();
        assert_eq!(sent, listed, "{alias}");
    }
}

/// Each source loads and is not empty: an inert loader would pass every
/// check vacuously.
#[test]
fn snapshot_sources_load() {
    let bindings = Surface::bindings();
    assert!(bindings.classes.len() > 150, "{}", bindings.classes.len());
    for class in [
        "interactions.Interaction",
        "legacy.File",
        "webhooks.Webhook",
    ] {
        assert!(
            bindings.classes.contains_key(class),
            "bindings lack {class}"
        );
    }
    for file in ["openapi-v1beta.json", "openapi-v1.json"] {
        let spec = Surface::openapi(file);
        assert!(
            spec.classes.contains_key("Interaction"),
            "{file} lacks Interaction"
        );
    }
}

/// A synthetic bindings document: `t.Root` reaches `t.X` first through a
/// chain of `depth` required links (below the synthesizer's depth limit
/// when `depth` is large), and with `shallow`, also directly. `t.X` has an
/// optional field and an optional `t.Y`, which only `t.X` reaches.
fn deep_first_surface(depth: usize, shallow: bool) -> Surface {
    let link = |name: String| json!({"required": true, "type": {"kind": "ref", "name": name}});
    let optional = |ty: serde_json::Value| json!({"required": false, "type": ty});
    let mut classes = serde_json::Map::new();
    let mut root = json!({"a_deep": optional(json!({"kind": "ref", "name": "t.L1"}))});
    if shallow {
        root["b_shallow"] = optional(json!({"kind": "ref", "name": "t.X"}));
    }
    classes.insert("t.Root".into(), json!({ "fields": root }));
    for i in 1..=depth {
        let next = if i == depth {
            "t.X".to_string()
        } else {
            format!("t.L{}", i + 1)
        };
        classes.insert(format!("t.L{i}"), json!({"fields": {"next": link(next)}}));
    }
    classes.insert(
        "t.X".into(),
        json!({"fields": {
            "opt": optional(json!({"kind": "str"})),
            "child": optional(json!({"kind": "ref", "name": "t.Y"})),
        }}),
    );
    classes.insert(
        "t.Y".into(),
        json!({"fields": {"leaf": optional(json!({"kind": "str"}))}}),
    );
    Surface::from_bindings(&json!({
        "sdk_version": "test",
        "aliases": {},
        "classes": classes,
    }))
}

/// A class first reached below the depth limit, where it gets only its
/// required fields, is synthesized in full where it is reached shallow:
/// the cut-down form is not reused there. So `t.X.opt` and `t.Y.leaf`,
/// reachable only through `t.X`, are in the bodies.
#[test]
fn fixtures_cover_a_class_first_reached_deep() {
    let surface = deep_first_surface(13, true);
    let ty = surface.ty("t.Root");
    let bodies = surface.fixtures(&ty, &api_surface::SynthOptions::default());
    let sites: BTreeSet<String> = bodies
        .iter()
        .flat_map(|body| surface.sites(&ty, body).into_values())
        .collect();
    for site in ["t.X.opt", "t.X.child", "t.Y.leaf"] {
        assert!(sites.contains(site), "{site} not synthesized: {sites:?}");
    }
}

/// Fields the depth limit leaves out fail `fixtures` loudly, rather than
/// shrinking the coverage unseen.
#[test]
#[should_panic(expected = "miss 4 site(s)")]
fn fixtures_fail_on_a_field_they_cannot_reach() {
    let surface = deep_first_surface(13, false);
    let ty = surface.ty("t.Root");
    surface.fixtures(&ty, &api_surface::SynthOptions::default());
}
