//! The OpenAPI specs' query parameters against the bindings' and the
//! crate's, for `every_query_key_a_verb_sends_is_a_binding_query_parameter`.
//!
//! The query test holds the crate to the bindings (rank 1). The specs
//! (rank 1b) list each operation's query parameters too, with whether each
//! is required (`operations` in each snapshot, kept by
//! `snapshot_openapi.py`), and they disagree with the bindings in both
//! directions. Each difference is a finding at `METHOD /path?key: kind`
//! until [`SPEC_QUERY`] gives its reason:
//!
//! - `spec only`: a spec parameter its endpoint lacks in the bindings and
//!   no verb sends;
//! - `bindings only`: a binding parameter its endpoint lacks in the spec;
//! - `required`: a parameter the spec marks required, which the bindings
//!   cannot say (they record names only), so the crate's handling of each
//!   is stated.
//!
//! Each spec copy is diffed on its own, and its findings begin with its
//! version (`v1beta GET /{}/agents?parent: required`), so a reason given
//! for one copy does not cover the other. So a parameter added to a spec
//! copy, or removed from it, or a change to whether one is required, fails
//! offline even where the other copy still has it as it was, and the daily
//! watch (`watch_spec_urls.sh`) sees the served specs' parameters change.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::api_surface;
use super::endpoints_for;

/// The spec snapshots whose operations are checked, each with the version
/// its findings begin with. v1 holds the interactions only.
const SPECS: [(&str, &str); 2] = [("v1beta", "openapi-v1beta.json"), ("v1", "openapi-v1.json")];

/// Why `parent` on the agents endpoints is not modeled as the spec has it.
const PARENT_IGNORED: &str = "The spec's `parent` (`projects/{project}/locations/{location}`, \
     required) is ignored: an agent created with any value (such a name, `bogus`, an empty \
     one) or none is the same agent, and a listing with any value or none lists every agent \
     (live 2026-11-09, docs/INTERACTIONS_API_GAP.md \"Spec-only query parameters\").";

/// Why the interaction read's `include_input` is not in the spec.
const INCLUDE_INPUT: &str = "`Interactions::get_with_input` sends it, and the stored input \
     comes back as a `user_input` step (live 2026-11-04, docs/INTERACTIONS_API_GAP.md \
     \"Coverage oracle\"); the spec does not list it.";

/// The differences between each spec copy's query parameters and the
/// bindings', each with its reason, as `VERSION METHOD /path?key: kind`.
pub const SPEC_QUERY: &[(&str, &str)] = &[
    (
        "v1beta GET /{}/agents?parent: required",
        "Optional in the bindings, and on `ListAgents`, whose `with_parent` sends it \
         only when set: the spec's `parent` (`projects/{project}/locations/{location}`, \
         required) is ignored, so a listing with any value or none lists every agent \
         (live 2026-11-09, docs/INTERACTIONS_API_GAP.md \"Spec-only query parameters\").",
    ),
    ("v1beta POST /{}/agents?parent: spec only", PARENT_IGNORED),
    ("v1beta POST /{}/agents?parent: required", PARENT_IGNORED),
    (
        "v1beta POST /{}/webhooks?webhook_id: spec only",
        "Not modeled: the endpoint takes it and ignores it, so a created webhook gets an \
         ID the server generates whatever the value (live 2026-11-09, \
         docs/INTERACTIONS_API_GAP.md \"Spec-only query parameters\").",
    ),
    (
        "v1beta GET /{}/triggers/{}/executions?filter: spec only",
        "Not modeled: ignored, so every expression, a malformed `(((` and one no \
         execution matches included, lists every execution (live 2026-11-09, \
         docs/INTERACTIONS_API_GAP.md \"Spec-only query parameters\").",
    ),
    (
        "v1beta GET /{}/interactions/{}?include_input: bindings only",
        INCLUDE_INPUT,
    ),
    (
        "v1 GET /{}/interactions/{}?include_input: bindings only",
        INCLUDE_INPUT,
    ),
    (
        "v1beta PATCH /{}/webhooks/{}?update_mask: bindings only",
        "`WebhookUpdate::with_update_mask` sends it, as the bindings define it; the spec \
         puts the mask in a wrapped update body (`UpdateWebhookRequest`) instead. The \
         endpoint takes the query parameter and ignores it, applying the fields in the \
         body (live 2026-07, docs/ENUM_WIRE_FORMATS.md \"Webhooks\").",
    ),
];

/// A spec's operations: `METHOD /path` (each path parameter as `{}`, the
/// bindings' form) and each query parameter with whether it is required.
pub type SpecEndpoints = BTreeMap<String, BTreeMap<String, bool>>;

/// `/{api_version}/agents/{agentsId}` -> `/{}/agents/{}`.
fn bindings_form(endpoint: &str) -> String {
    let mut out = String::new();
    let mut in_param = false;
    for c in endpoint.chars() {
        match c {
            '{' => {
                in_param = true;
                out.push_str("{}");
            }
            '}' => in_param = false,
            c if !in_param => out.push(c),
            _ => {}
        }
    }
    out
}

/// The operations of a spec snapshot (its `operations`).
pub fn spec_endpoints(snapshot: &Value) -> SpecEndpoints {
    let operations = snapshot["operations"]
        .as_object()
        .expect("the spec snapshot has operations: refresh it with snapshot_openapi.py");
    operations
        .iter()
        .map(|(endpoint, operation)| {
            let query = operation["query"]
                .as_object()
                .unwrap_or_else(|| panic!("{endpoint}: no query object"))
                .iter()
                .map(|(name, param)| {
                    let required = param["required"]
                        .as_bool()
                        .unwrap_or_else(|| panic!("{endpoint}?{name}: no required flag"));
                    (name.clone(), required)
                })
                .collect();
            (bindings_form(endpoint), query)
        })
        .collect()
}

/// Each spec snapshot's operations, by its version.
pub fn spec_copies() -> Vec<(&'static str, SpecEndpoints)> {
    SPECS
        .iter()
        .map(|(version, file)| (*version, spec_endpoints(&api_surface::read_json(file))))
        .collect()
}

/// Every difference between each spec copy's query parameters and
/// `bindings'`, as `VERSION METHOD /path?key: kind` (see the module doc),
/// that `reasoned` does not list, and every `reasoned` entry no difference
/// matches. `requests` are the `(method, target)` pairs the query test
/// captured: a spec-only parameter one of them sends is not a difference
/// here, since the bindings check already holds that key to its reason.
pub fn spec_query_findings(
    specs: &[(&str, SpecEndpoints)],
    bindings: &BTreeMap<String, BTreeSet<String>>,
    requests: &[(String, String)],
    reasoned: &[(&str, &str)],
) -> Vec<String> {
    let mut differences: BTreeMap<String, &str> = BTreeMap::new();
    for (version, spec) in specs {
        differences.extend(
            copy_differences(spec, bindings, requests)
                .into_iter()
                .map(|(site, what)| (format!("{version} {site}"), what)),
        );
    }
    let mut problems: Vec<String> = differences
        .iter()
        .filter(|(key, _)| !reasoned.iter().any(|(entry, _)| entry == key))
        .map(|(key, what)| format!("{key}: {what} (list it in SPEC_QUERY with the reason)"))
        .collect();
    problems.extend(
        reasoned
            .iter()
            .filter(|(entry, _)| !differences.contains_key(*entry))
            .map(|(entry, _)| format!("SPEC_QUERY: {entry} matches nothing; delete the entry")),
    );
    problems
}

/// One spec copy's differences from `bindings`, as `METHOD /path?key: kind`
/// with what each is.
fn copy_differences(
    spec: &SpecEndpoints,
    bindings: &BTreeMap<String, BTreeSet<String>>,
    requests: &[(String, String)],
) -> BTreeMap<String, &'static str> {
    let mut sent: BTreeSet<String> = BTreeSet::new();
    for (method, target) in requests {
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        for endpoint in endpoints_for(spec.keys().map(String::as_str), method, path) {
            for pair in query.split('&').filter(|pair| !pair.is_empty()) {
                let key = pair.split('=').next().unwrap_or_default();
                sent.insert(format!("{endpoint}?{key}"));
            }
        }
    }
    let none = BTreeSet::new();
    let mut differences = BTreeMap::new();
    for (endpoint, params) in spec {
        let binding = bindings.get(endpoint).unwrap_or(&none);
        for (name, required) in params {
            let site = format!("{endpoint}?{name}");
            if !binding.contains(name) && !sent.contains(&site) {
                differences.insert(
                    format!("{site}: spec only"),
                    "a spec parameter the bindings lack, and no verb sends it",
                );
            }
            if *required {
                differences.insert(
                    format!("{site}: required"),
                    "required by the spec; say how the crate sends it",
                );
            }
        }
        for name in binding.difference(&params.keys().cloned().collect()) {
            differences.insert(
                format!("{endpoint}?{name}: bindings only"),
                "a binding parameter the spec does not list",
            );
        }
    }
    differences
}

/// The snapshots carry each operation's query parameters, and both specs
/// load, each on its own: an inert loader would pass the check above
/// vacuously.
#[test]
fn spec_snapshots_carry_query_parameters() {
    let copies: BTreeMap<&str, SpecEndpoints> = spec_copies().into_iter().collect();
    let (v1beta, v1) = (&copies["v1beta"], &copies["v1"]);
    assert!(
        v1beta.len() > 30,
        "too few v1beta operations: {}",
        v1beta.len()
    );
    assert!(v1.len() >= 4, "too few v1 operations: {}", v1.len());
    assert_eq!(
        v1beta["GET /{}/webhooks"],
        BTreeMap::from([("page_size".into(), false), ("page_token".into(), false)])
    );
    for spec in [v1beta, v1] {
        assert_eq!(
            spec["GET /{}/interactions/{}"],
            BTreeMap::from([("last_event_id".into(), false), ("stream".into(), false)])
        );
    }
    for (_, file) in SPECS {
        let snapshot = api_surface::read_json(file);
        let endpoints: BTreeSet<String> = snapshot["endpoints"]
            .as_array()
            .expect("endpoints")
            .iter()
            .map(|e| e.as_str().expect("an endpoint").to_string())
            .collect();
        let operations: BTreeSet<String> = snapshot["operations"]
            .as_object()
            .expect("operations")
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            endpoints, operations,
            "{file}: an endpoint without its operation"
        );
        for (endpoint, operation) in snapshot["operations"].as_object().expect("operations") {
            for schema in operation["body"]
                .as_object()
                .into_iter()
                .flat_map(|b| b.values())
            {
                for reference in schema_refs(schema) {
                    let name = reference.rsplit('/').next().unwrap_or_default();
                    assert!(
                        snapshot["schemas"].get(name).is_some(),
                        "{file}: {endpoint}'s body names {reference}, which it does not define"
                    );
                }
            }
        }
    }
}

/// Every `$ref` in a schema.
fn schema_refs(schema: &Value) -> Vec<String> {
    match schema {
        Value::Object(map) => map
            .iter()
            .flat_map(|(key, value)| match (key.as_str(), value) {
                ("$ref", Value::String(reference)) => vec![reference.clone()],
                _ => schema_refs(value),
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(schema_refs).collect(),
        _ => Vec::new(),
    }
}

/// A parameter added to a spec copy, one removed from it, and one whose
/// required flag changes are each a finding, named by the copy, even where
/// the other copy still has it as it was; a reason for one copy does not
/// cover the other; a spec-only parameter a verb sends is the bindings
/// check's to reason about; and a reasoned entry that matches nothing is
/// stale.
#[test]
fn a_spec_query_parameter_change_is_a_finding() {
    let bindings: BTreeMap<String, BTreeSet<String>> = BTreeMap::from([
        (
            "GET /{}/things".to_string(),
            BTreeSet::from(["page_size".to_string(), "page_token".to_string()]),
        ),
        ("POST /{}/things".to_string(), BTreeSet::new()),
    ]);
    let copy = |extra: &[(&str, &str, bool)], drop: &str| -> SpecEndpoints {
        let mut spec = SpecEndpoints::from([
            (
                "GET /{}/things".to_string(),
                BTreeMap::from([("page_size".into(), false), ("page_token".into(), false)]),
            ),
            ("POST /{}/things".to_string(), BTreeMap::new()),
        ]);
        for (endpoint, name, required) in extra {
            spec.get_mut(*endpoint)
                .expect("an endpoint")
                .insert((*name).to_string(), *required);
        }
        spec.get_mut("GET /{}/things")
            .expect("listing")
            .remove(drop);
        spec
    };
    // Copy `a` as given, beside an unchanged copy `b`.
    let specs = |a: SpecEndpoints| vec![("a", a), ("b", copy(&[], ""))];
    let requests = vec![(
        "GET".to_string(),
        "/v1beta/things?page_size=5&page_token=t".to_string(),
    )];
    let check = |specs: &[(&str, SpecEndpoints)], reasoned: &[(&str, &str)]| {
        spec_query_findings(specs, &bindings, &requests, reasoned)
    };

    assert!(check(&specs(copy(&[], "")), &[]).is_empty());
    let added = check(
        &specs(copy(&[("POST /{}/things", "thing_id", false)], "")),
        &[],
    );
    assert_eq!(added.len(), 1, "{added:?}");
    assert!(added[0].starts_with("a POST /{}/things?thing_id: spec only"));
    let removed = check(&specs(copy(&[], "page_token")), &[]);
    assert_eq!(removed.len(), 1, "{removed:?}");
    assert!(removed[0].starts_with("a GET /{}/things?page_token: bindings only"));
    let required = check(
        &specs(copy(&[("GET /{}/things", "page_size", true)], "")),
        &[],
    );
    assert_eq!(required.len(), 1, "{required:?}");
    assert!(required[0].starts_with("a GET /{}/things?page_size: required"));

    // Sent though only the spec lists it: the bindings check reasons about it.
    let sent = vec![("GET".to_string(), "/v1beta/things?filter=x".to_string())];
    let with_filter = specs(copy(&[("GET /{}/things", "filter", false)], ""));
    assert!(spec_query_findings(&with_filter, &bindings, &sent, &[]).is_empty());

    // A reason is for one copy: a copy that loses the parameter makes it
    // stale, and one that gains it needs its own.
    let thing_id = || copy(&[("POST /{}/things", "thing_id", false)], "");
    let reasoned = [("a POST /{}/things?thing_id: spec only", "Ignored.")];
    assert!(check(&specs(thing_id()), &reasoned).is_empty());
    assert_eq!(
        check(&specs(copy(&[], "")), &reasoned),
        ["SPEC_QUERY: a POST /{}/things?thing_id: spec only matches nothing; delete the entry"]
    );
    let both = check(&[("a", thing_id()), ("b", thing_id())], &reasoned);
    assert_eq!(both.len(), 1, "{both:?}");
    assert!(both[0].starts_with("b POST /{}/things?thing_id: spec only"));
}
