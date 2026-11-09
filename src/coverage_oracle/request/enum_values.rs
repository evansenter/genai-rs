//! The reverse check for enumeration values, on the request side: every
//! value of an enumeration only a request reaches is sent by some value in
//! [`super::cases`], or listed in [`NOT_SENT_VALUES`] with its reason.
//!
//! An enumeration a response also reaches needs no check here: the
//! response side synthesizes a body with each of its values, and one the
//! crate does not model lands in `Unknown` and fails
//! `binding_fields_deserialize_without_extra`. One only a request reaches
//! (`webhooks.WebhookUpdateState`, `credentials.InjectionLocationEnum`) had
//! no such check: a value a baseline bump added to it passed every test. A
//! request-only enumeration is a literal alias, or a literal a field spells
//! out, that a request case reaches and no response target does.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::super::api_surface::{Finding, Surface};
use super::super::{Allowed, assert_clean, response_targets};
use super::cases;

/// Values of a request-only enumeration the crate never sends, each with
/// its reason, as `Enumeration=value`.
pub(in crate::coverage_oracle) const NOT_SENT_VALUES: &[Allowed] = &[];

/// The enumerations `classes` reach, by the name
/// [`Surface::reachable_literals`] gives each, with their values.
fn literals_reached<'a>(
    surface: &Surface,
    classes: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, Vec<Value>> {
    let mut out = BTreeMap::new();
    for class in classes {
        out.extend(surface.reachable_literals(&surface.ty(class)));
    }
    out
}

/// Each value of an enumeration in `request` but not in `response` that
/// `sent` lacks, as a finding at `Enumeration=value`.
fn unsent_values(
    request: &BTreeMap<String, Vec<Value>>,
    response: &BTreeMap<String, Vec<Value>>,
    sent: &BTreeSet<String>,
) -> Vec<Finding> {
    request
        .iter()
        .filter(|(name, _)| !response.contains_key(*name))
        .flat_map(|(name, values)| {
            values.iter().filter_map(move |value| {
                let text = value
                    .as_str()
                    .map_or_else(|| value.to_string(), String::from);
                let site = format!("{name}={text}");
                (!sent.contains(&site)).then(|| Finding {
                    path: name.clone(),
                    site,
                    message: "a value only a request takes, and no request value sends it".into(),
                })
            })
        })
        .collect()
}

#[test]
fn request_only_enum_values_are_sent() {
    let surface = Surface::bindings();
    let mut sent = BTreeSet::new();
    let cases = cases();
    for case in &cases {
        let ty = surface.ty(case.binding);
        for value in &case.values {
            sent.extend(surface.literal_values(&ty, value));
        }
    }
    let request = literals_reached(&surface, cases.iter().map(|case| case.binding));
    let response = literals_reached(&surface, response_targets::target_classes());
    for known in [
        "credentials.InjectionLocationEnum",
        "triggers.TriggerUpdateStatus",
        "webhooks.WebhookUpdateState",
        "webhooks.WebhookUpdateSubscribedEvent",
        "environments.CreateEnvironmentRequestNetworkEnum",
    ] {
        assert!(
            request.contains_key(known) && !response.contains_key(known),
            "{known} is no longer request-only, or the walk missed it"
        );
    }
    assert!(
        response.contains_key("interactions.HarmCategory"),
        "the response walk missed an enumeration both sides reach"
    );
    assert_clean(
        &format!(
            "Values of request-only enumerations no request value sends (google-genai {})",
            surface.version
        ),
        &surface,
        unsent_values(&request, &response, &sent),
        &[("NOT_SENT_VALUES", NOT_SENT_VALUES)],
    );
}

/// The verifier's mutation: a value added to a request-only enumeration is
/// a finding until a request sends it; one a response also reaches is left
/// to the response side; and an inline literal is named by its field.
#[test]
fn an_unsent_request_only_value_is_a_finding() {
    let surface = Surface::from_bindings(&serde_json::json!({
        "sdk_version": "test",
        "aliases": {
            "t.UpdateState": {"kind": "literal", "values": ["enabled", "archived"]},
            "t.Shared": {"kind": "literal", "values": ["a", "b"]},
        },
        "classes": {
            "t.Update": {"fields": {
                "state": {"type": {"kind": "ref", "name": "t.UpdateState"}},
                "shared": {"type": {"kind": "ref", "name": "t.Shared"}},
                "mode": {"type": {"kind": "list", "item":
                    {"kind": "literal", "values": ["fast", "slow"]}}},
            }},
            "t.Resource": {"fields": {
                "shared": {"type": {"kind": "ref", "name": "t.Shared"}},
            }},
        },
    }));
    let ty = surface.ty("t.Update");
    let sent = surface.literal_values(
        &ty,
        &serde_json::json!({"state": "enabled", "shared": "a", "mode": ["fast"]}),
    );
    assert_eq!(
        sent,
        BTreeSet::from(
            ["t.UpdateState=enabled", "t.Shared=a", "t.Update.mode=fast"].map(String::from)
        )
    );
    let request = literals_reached(&surface, ["t.Update"]);
    let response = literals_reached(&surface, ["t.Resource"]);
    let sites: Vec<String> = unsent_values(&request, &response, &sent)
        .into_iter()
        .map(|finding| finding.site)
        .collect();
    assert_eq!(sites, ["t.Update.mode=slow", "t.UpdateState=archived"]);
}
