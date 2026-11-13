//! The OpenAPI specs' query and header parameters against the bindings'
//! and the crate's, for
//! `every_query_key_a_verb_sends_is_a_binding_query_parameter`.
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
//!
//! Header parameters (`header` in each operation) are held to what the
//! verbs send, since the bindings do not record headers. Each is a finding
//! at `VERSION METHOD /path header NAME: kind` until [`SPEC_HEADERS`] gives
//! its reason:
//!
//! - `not sent`: a request to its endpoint goes without it, or no verb
//!   reaches the endpoint;
//! - `value`: the spec fixes its value (a `const` or an `enum`, followed
//!   through a `$ref` and `allOf`, `oneOf` or `anyOf`), and a request sends
//!   another.
//!
//! The snapshot also keeps each operation's path parameters, each
//! parameter's schema, its request body (with whether it is required) and
//! its responses, which the watch sees change; nothing here diffs them
//! against the crate (D-060).

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

/// The spec header parameters the verbs do not send as the spec gives
/// them, each with its reason, as `VERSION METHOD /path header NAME: kind`.
/// Empty: the four the spec gives, all on the environment-file upload's
/// start request, are sent with the values it fixes.
pub const SPEC_HEADERS: &[(&str, &str)] = &[];

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

/// A spec's header parameters: `METHOD /path` (the bindings' form) and each
/// header the operation names, with the values its schema allows when it
/// fixes them (see [`fixed_values`]), `None` when it does not.
pub type SpecHeaders = BTreeMap<String, BTreeMap<String, Option<BTreeSet<String>>>>;

/// The header parameters of a spec snapshot (`header` in each operation).
pub fn spec_headers(snapshot: &Value) -> SpecHeaders {
    let operations = snapshot["operations"]
        .as_object()
        .expect("the spec snapshot has operations: refresh it with snapshot_openapi.py");
    operations
        .iter()
        .map(|(endpoint, operation)| {
            let headers = operation["header"]
                .as_object()
                .unwrap_or_else(|| {
                    panic!("{endpoint}: no header object: refresh it with snapshot_openapi.py")
                })
                .iter()
                .map(|(name, param)| {
                    let values = fixed_values(&param["schema"], &snapshot["schemas"], 0)
                        .unwrap_or_else(|e| panic!("{endpoint} header {name}: {e}"));
                    (name.clone(), values)
                })
                .collect();
            (bindings_form(endpoint), headers)
        })
        .collect()
}

/// The values a parameter schema allows, when it fixes them, or `None`.
/// Its `const`, its `enum`, what its `$ref` names (in `schemas`, the
/// snapshot's) and each `allOf` member each constrain it, so the values are
/// those every constraint that fixes them allows; a `oneOf` or `anyOf` fixes
/// them only when each member does, to any member's. A value that is not a
/// string is kept as its JSON text. A reference `schemas` does not define,
/// or one nested too deep (a cycle), is an error rather than "not fixed",
/// which would leave the value check nothing to hold.
fn fixed_values(
    schema: &Value,
    schemas: &Value,
    depth: usize,
) -> Result<Option<BTreeSet<String>>, String> {
    if depth > 16 {
        return Err("a schema nested more than 16 references deep".into());
    }
    let text = |value: &Value| match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut constraints: Vec<Option<BTreeSet<String>>> = Vec::new();
    if let Some(value) = schema.get("const") {
        constraints.push(Some(BTreeSet::from([text(value)])));
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        constraints.push(Some(values.iter().map(text).collect()));
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = reference
            .strip_prefix("#/components/schemas/")
            .and_then(|name| schemas.get(name))
            .ok_or_else(|| format!("{reference} names no schema in the snapshot"))?;
        constraints.push(fixed_values(target, schemas, depth + 1)?);
    }
    for member in schema
        .get("allOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        constraints.push(fixed_values(member, schemas, depth + 1)?);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(members) = schema.get(key).and_then(Value::as_array) {
            let each = members
                .iter()
                .map(|member| fixed_values(member, schemas, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            let any = each
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(|sets| sets.into_iter().flatten().collect());
            constraints.push(any);
        }
    }
    Ok(constraints
        .into_iter()
        .flatten()
        .reduce(|a, b| a.intersection(&b).cloned().collect()))
}

/// Each spec snapshot's header parameters, by its version.
pub fn spec_header_copies() -> Vec<(&'static str, SpecHeaders)> {
    SPECS
        .iter()
        .map(|(version, file)| (*version, spec_headers(&api_surface::read_json(file))))
        .collect()
}

/// A request the stub captured: its method, its target (path and query) and
/// its headers.
pub type Sent = (String, String, Vec<(String, String)>);

/// Every spec header parameter that `requests` do not send as the spec
/// gives it, in each copy, as `VERSION METHOD /path header NAME: kind` (see
/// the module doc), that `reasoned` does not list, and every `reasoned`
/// entry no finding matches. A header is sent when every request to its
/// endpoint carries it, its name compared without case.
pub fn spec_header_findings(
    specs: &[(&str, SpecHeaders)],
    requests: &[Sent],
    reasoned: &[(&str, &str)],
) -> Vec<String> {
    let mut differences: BTreeMap<String, &str> = BTreeMap::new();
    for (version, spec) in specs {
        let mut reaching: BTreeMap<&str, Vec<&[(String, String)]>> = BTreeMap::new();
        for (method, target, headers) in requests {
            let path = target
                .split_once('?')
                .map_or(target.as_str(), |(path, _)| path);
            for endpoint in endpoints_for(spec.keys().map(String::as_str), method, path) {
                reaching.entry(endpoint).or_default().push(headers);
            }
        }
        for (endpoint, params) in spec {
            let requests = reaching
                .get(endpoint.as_str())
                .map_or(&[][..], Vec::as_slice);
            for (name, values) in params {
                let site = format!("{version} {endpoint} header {name}");
                let sent: Vec<Option<&str>> = requests
                    .iter()
                    .map(|headers| {
                        headers
                            .iter()
                            .find(|(key, _)| key.eq_ignore_ascii_case(name))
                            .map(|(_, value)| value.as_str())
                    })
                    .collect();
                if sent.is_empty() || sent.contains(&None) {
                    differences.insert(
                        format!("{site}: not sent"),
                        "a spec header a request to its endpoint goes without",
                    );
                }
                if let Some(values) = values
                    && sent.iter().flatten().any(|value| !values.contains(*value))
                {
                    differences.insert(
                        format!("{site}: value"),
                        "sent with a value the spec does not allow",
                    );
                }
            }
        }
    }
    let mut problems: Vec<String> = differences
        .iter()
        .filter(|(key, _)| !reasoned.iter().any(|(entry, _)| entry == key))
        .map(|(key, what)| format!("{key}: {what} (list it in SPEC_HEADERS with the reason)"))
        .collect();
    problems.extend(
        reasoned
            .iter()
            .filter(|(entry, _)| !differences.contains_key(*entry))
            .map(|(entry, _)| format!("SPEC_HEADERS: {entry} matches nothing; delete the entry")),
    );
    problems
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

/// The snapshots carry each operation's path, query and header parameters,
/// its request body's required flag and its responses, and both specs load,
/// each on its own: an inert loader would pass the checks above vacuously.
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
    let headers: BTreeMap<&str, SpecHeaders> = spec_header_copies().into_iter().collect();
    let upload = &headers["v1beta"]["PUT /upload/{}/environments/{}/files/{}"];
    assert_eq!(
        upload.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "X-Goog-Upload-Command",
            "X-Goog-Upload-Header-Content-Length",
            "X-Goog-Upload-Header-Content-Type",
            "X-Goog-Upload-Protocol",
        ]
    );
    assert_eq!(
        upload["X-Goog-Upload-Protocol"],
        Some(BTreeSet::from(["resumable".to_string()]))
    );
    assert_eq!(upload["X-Goog-Upload-Header-Content-Length"], None);
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
            // Each path parameter the endpoint names is kept, with its schema.
            let path = operation["path"]
                .as_object()
                .unwrap_or_else(|| panic!("{file}: {endpoint} has no path parameters"));
            for name in endpoint.split(['{', '}']).skip(1).step_by(2) {
                assert!(
                    path.get(name).is_some_and(|p| p["schema"].is_object()),
                    "{file}: {endpoint} keeps no path parameter {name} with its schema"
                );
            }
            let responses = operation["responses"]
                .as_object()
                .unwrap_or_else(|| panic!("{file}: {endpoint} has no responses"));
            assert!(!responses.is_empty(), "{file}: {endpoint} has no responses");
            if let Some(body) = operation.get("body") {
                assert!(
                    body["required"].is_boolean(),
                    "{file}: {endpoint}'s body has no required flag"
                );
            }
            let bodies = operation["body"]["content"].as_object().into_iter();
            let replies = responses
                .values()
                .filter_map(|response| response["content"].as_object());
            for schema in bodies.chain(replies).flat_map(|content| content.values()) {
                for reference in schema_refs(schema) {
                    let name = reference.rsplit('/').next().unwrap_or_default();
                    assert!(
                        snapshot["schemas"].get(name).is_some(),
                        "{file}: {endpoint} names {reference}, which it does not define"
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

/// A spec header no request to its endpoint sends, or one sent by only some
/// of them, is a finding, and so is a value the spec's `const` or `enum`
/// does not allow; a header sent by every request, in any case, is not; a
/// reason is for one copy; and a reasoned entry that matches nothing is
/// stale.
#[test]
fn a_spec_header_parameter_change_is_a_finding() {
    let copy = |headers: &[(&str, Option<&[&str]>)]| -> SpecHeaders {
        SpecHeaders::from([
            (
                "PUT /upload/{}/things/{}".to_string(),
                headers
                    .iter()
                    .map(|(name, values)| {
                        let values = values.map(|v| v.iter().map(ToString::to_string).collect());
                        ((*name).to_string(), values)
                    })
                    .collect(),
            ),
            ("GET /{}/things".to_string(), BTreeMap::new()),
        ])
    };
    let request = |headers: &[(&str, &str)]| -> Sent {
        (
            "PUT".to_string(),
            "/upload/v1beta/things/t?overwrite=true".to_string(),
            headers
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    };
    let protocol: &[&str] = &["resumable"];
    let both = vec![
        request(&[("x-upload-protocol", "resumable"), ("x-upload-length", "2")]),
        request(&[("X-Upload-Protocol", "resumable"), ("X-Upload-Length", "9")]),
    ];
    let spec = copy(&[
        ("X-Upload-Protocol", Some(protocol)),
        ("X-Upload-Length", None),
    ]);
    let check = |spec: SpecHeaders, sent: &[Sent], reasoned: &[(&str, &str)]| {
        spec_header_findings(&[("a", spec), ("b", copy(&[]))], sent, reasoned)
    };

    assert!(check(spec.clone(), &both, &[]).is_empty());
    let added = check(
        copy(&[
            ("X-Upload-Protocol", Some(protocol)),
            ("X-Api-Revision", None),
        ]),
        &both,
        &[],
    );
    assert_eq!(added.len(), 1, "{added:?}");
    assert!(added[0].starts_with("a PUT /upload/{}/things/{} header X-Api-Revision: not sent"));
    let some = [both[0].clone(), request(&[("x-upload-length", "2")])];
    let missing = check(spec.clone(), &some, &[]);
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(
        missing[0].starts_with("a PUT /upload/{}/things/{} header X-Upload-Protocol: not sent")
    );
    let unreached = check(spec.clone(), &[], &[]);
    assert_eq!(unreached.len(), 2, "{unreached:?}");
    let other = [request(&[
        ("x-upload-protocol", "multipart"),
        ("x-upload-length", "2"),
    ])];
    let value = check(spec.clone(), &other, &[]);
    assert_eq!(value.len(), 1, "{value:?}");
    assert!(value[0].starts_with("a PUT /upload/{}/things/{} header X-Upload-Protocol: value"));

    // A reason is for one copy: a copy that loses the header makes it
    // stale, and one that gains it needs its own.
    let revision = || copy(&[("X-Api-Revision", None)]);
    let reasoned = [(
        "a PUT /upload/{}/things/{} header X-Api-Revision: not sent",
        "Unused.",
    )];
    assert!(
        spec_header_findings(&[("a", revision()), ("b", copy(&[]))], &both, &reasoned).is_empty()
    );
    assert_eq!(
        spec_header_findings(&[("a", copy(&[])), ("b", copy(&[]))], &both, &reasoned),
        [
            "SPEC_HEADERS: a PUT /upload/{}/things/{} header X-Api-Revision: not sent matches \
          nothing; delete the entry"
        ]
    );
    let gained = spec_header_findings(&[("a", revision()), ("b", revision())], &both, &reasoned);
    assert_eq!(gained.len(), 1, "{gained:?}");
    assert!(gained[0].starts_with("b PUT /upload/{}/things/{} header X-Api-Revision: not sent"));
}

/// A header's fixed values are read through a `$ref`, `allOf`, `oneOf` and
/// `anyOf`, so a spec that moves them behind one keeps the value check; a
/// member that fixes nothing leaves a `oneOf` or `anyOf` unfixed; and a
/// reference the snapshot does not define, or a cycle, is an error, not
/// "not fixed".
#[test]
fn spec_header_values_are_read_through_references() {
    let snapshot = serde_json::json!({
        "schemas": {
            "Protocol": {"type": "string", "enum": ["resumable", "multipart"]},
            "Resumable": {"$ref": "#/components/schemas/Protocol", "const": "resumable"},
            "Loop": {"allOf": [{"$ref": "#/components/schemas/Loop"}]},
        },
        "operations": {
            "PUT /upload/{api_version}/things/{id}": {"header": {
                "A": {"schema": {"$ref": "#/components/schemas/Protocol"}},
                "B": {"schema": {"$ref": "#/components/schemas/Resumable"}},
                "C": {"schema": {"allOf": [
                    {"$ref": "#/components/schemas/Protocol"},
                    {"enum": ["multipart", "other"]},
                ]}},
                "D": {"schema": {"oneOf": [{"const": "start"}, {"const": "upload"}]}},
                "E": {"schema": {"anyOf": [{"const": "start"}, {"type": "string"}]}},
                "F": {"schema": {"type": "integer"}},
            }},
            "GET /{api_version}/things": {"header": {}},
        },
    });
    let copy = spec_headers(&snapshot);
    let headers = &copy["PUT /upload/{}/things/{}"];
    let set = |values: &[&str]| Some(values.iter().map(ToString::to_string).collect());
    assert_eq!(headers["A"], set(&["multipart", "resumable"]));
    assert_eq!(headers["B"], set(&["resumable"]));
    assert_eq!(headers["C"], set(&["multipart"]));
    assert_eq!(headers["D"], set(&["start", "upload"]));
    assert_eq!(headers["E"], None);
    assert_eq!(headers["F"], None);

    // So a value outside the referenced enum is a finding.
    let sent: Vec<Sent> = vec![(
        "PUT".to_string(),
        "/upload/v1beta/things/t".to_string(),
        ["A", "B", "C", "D", "E", "F"]
            .iter()
            .map(|name| ((*name).to_string(), "multipart".to_string()))
            .collect(),
    )];
    let findings = spec_header_findings(&[("a", copy.clone())], &sent, &[]);
    let kinds: Vec<&str> = findings
        .iter()
        .map(|f| f.split(": ").next().unwrap_or_default())
        .collect();
    assert_eq!(
        kinds,
        [
            "a PUT /upload/{}/things/{} header B",
            "a PUT /upload/{}/things/{} header D",
        ],
        "{findings:?}"
    );

    let schemas = &snapshot["schemas"];
    let dangling = serde_json::json!({"$ref": "#/components/schemas/Missing"});
    assert!(fixed_values(&dangling, schemas, 0).is_err());
    let elsewhere = serde_json::json!({"$ref": "#/components/parameters/Protocol"});
    assert!(fixed_values(&elsewhere, schemas, 0).is_err());
    let cycle = serde_json::json!({"$ref": "#/components/schemas/Loop"});
    assert!(fixed_values(&cycle, schemas, 0).is_err());
}
