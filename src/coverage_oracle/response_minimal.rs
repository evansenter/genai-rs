//! Response side, sparse bodies: every class read with only its required
//! fields.
//!
//! The bodies [`super::response`] checks set every field, so a type that
//! demands a field the snapshot makes optional passes there and still
//! fails on a served object that leaves the field out: `Tool::Function`
//! read a function tool with no `description` as `Unknown`, which no full
//! body could show. Here each class reached from a response target is cut
//! down, one class at a time, to its required fields and constant
//! discriminators (`Surface::minimal_variants`), and the body must still
//! deserialize with no `Unknown`, nothing in `extra` and nothing dropped
//! (`response::analyze`). A site listed below is a known exception.

use std::collections::BTreeSet;

use serde_json::Value;

use super::api_surface::{Finding, Surface, SynthOptions, Ty};
use super::response::{analyze, values};
use super::response_targets::{Target, targets};
use super::{Allowed, assert_clean, spec_class};

/// Optional fields the crate cannot read a class without.
pub(super) const SPARSE: &[Allowed] = &[
    (
        "legacy.File.name",
        "`FileMetadata` requires `name`, the file's resource name, which every \
         served file carries (live 2026-11-05: files().get and list); the \
         hand-written legacy classes mark every field optional. Kept required \
         after the resource residue (#504, 2026-11-05): the `Files` verbs and \
         `import_file` address a file by it.",
    ),
    ("interactions.StepDeltaMetadata.total_usage", EMPTY_METADATA),
    ("StepDeltaMetadata.total_usage", EMPTY_METADATA),
];

const EMPTY_METADATA: &str = "A step.delta's `metadata` holds only \
     `total_usage`, which `StreamChunk::StepDelta` carries; an empty \
     `metadata: {}` holds nothing, and the chunk does not keep the empty \
     wrapper. No step.delta has carried `metadata` live (2026-11-05).";

fn findings(
    surface: &Surface,
    targets: Vec<Target>,
    pick: impl Fn(&Target) -> Option<&'static str>,
) -> Vec<Finding> {
    let options = SynthOptions {
        values: values(),
        ..SynthOptions::default()
    };
    let mut out = Vec::new();
    // What the full bodies already show is `response`'s to report: it is
    // collected first, which also traces those sites once, so the sparse
    // bodies, which carry the same fields elsewhere, skip that work.
    let mut full = BTreeSet::new();
    let mut traced = BTreeSet::new();
    // A class is cut down once, in the first target that reaches it: the
    // list targets reach their items' classes again, in far bigger bodies.
    let mut done = BTreeSet::new();
    for target in targets {
        let Some(class) = pick(&target) else { continue };
        let ty = surface.ty(class);
        let bodies = surface.fixtures(&ty, &options);
        for body in &bodies {
            for finding in analyze(surface, &ty, body, target.probe, target.name, &mut traced) {
                full.insert((finding.site, finding.message));
            }
        }
        for (cut, index, body) in surface.minimal_variants(&ty, &bodies, &mut done) {
            let name = format!("{} (only the required fields of {cut})", target.name);
            for finding in analyze(surface, &ty, &body, target.probe, &name, &mut traced) {
                if full.contains(&(finding.site.clone(), finding.message.clone())) {
                    continue;
                }
                let finding = attribute(surface, &ty, &bodies[index], &cut, &target, finding);
                // One finding per site and message is enough to act on.
                if full.insert((finding.site.clone(), finding.message.clone())) {
                    out.push(finding);
                }
            }
        }
    }
    out
}

/// Names the optional field of `class` whose absence alone causes
/// `finding`, by removing each from the full body in turn: a parse failure
/// on a cut body otherwise has no site of its own (removing keys cannot
/// cure a missing one). A finding no single field causes is kept as is.
fn attribute(
    surface: &Surface,
    ty: &Ty,
    full_body: &Value,
    class: &str,
    target: &Target,
    finding: Finding,
) -> Finding {
    let reproduces = |body: &Value| {
        let mut traced = BTreeSet::new();
        analyze(surface, ty, body, target.probe, target.name, &mut traced)
            .iter()
            .any(|f| f.site == finding.site && f.message == finding.message)
    };
    for (field, spec) in &surface.classes[class].fields {
        if !Surface::optional(spec) {
            continue;
        }
        let Some(body) = surface.without_field(ty, full_body, class, field) else {
            continue;
        };
        if reproduces(&body) {
            return Finding {
                path: finding.path,
                site: format!("{class}.{field}"),
                message: format!(
                    "the crate needs this optional field: without it the body {}",
                    finding.message
                ),
            };
        }
    }
    finding
}

#[test]
fn sparse_binding_bodies_deserialize() {
    let surface = Surface::bindings();
    assert_clean(
        &format!(
            "Response classes read with only their required fields, vs google-genai {} bindings",
            surface.version
        ),
        &surface,
        findings(&surface, targets(), |t| t.binding),
        &[("SPARSE", SPARSE)],
    );
}

/// Both spec snapshots ([`super::SPECS`]), with one set of exceptions.
#[test]
fn sparse_spec_bodies_deserialize() {
    let mut all = Vec::new();
    let mut fetched = Vec::new();
    for file in super::SPECS {
        let surface = Surface::openapi(file);
        for mut finding in findings(&surface, targets(), |t| spec_class(&surface, t)) {
            finding.path = format!("{} {}", surface.name, finding.path);
            all.push(finding);
        }
        fetched.push(format!("{} {}", surface.name, surface.version));
    }
    assert_clean(
        &format!(
            "Response classes read with only their required fields, vs the OpenAPI specs ({})",
            fetched.join(", ")
        ),
        &Surface::openapi(super::SPECS[0]),
        all,
        &[("SPARSE", SPARSE)],
    );
}

/// A class whose optional field the crate requires is reported: a stand-in
/// type that demands `name` fails once the body leaves it out.
#[test]
fn a_required_optional_field_is_reported() {
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct Holder {
        items: Vec<Item>,
    }
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct Item {
        id: String,
        name: String,
    }
    let surface = Surface::from_bindings(&serde_json::json!({
        "sdk_version": "test",
        "aliases": {},
        "classes": {
            "t.Holder": {"fields": {"items": {
                "required": true,
                "type": {"kind": "list", "item": {"kind": "ref", "name": "t.Item"}},
            }}},
            "t.Item": {"fields": {
                "id": {"required": true, "type": {"kind": "str"}},
                "name": {"required": false, "type": {"kind": "str"}},
            }},
        },
    }));
    let holder = Target {
        name: "Holder",
        binding: Some("t.Holder"),
        spec: None,
        probe: super::response_targets::probe::<Holder>,
    };
    let reported: Vec<(String, String)> = findings(&surface, vec![holder], |t| t.binding)
        .into_iter()
        .map(|f| (f.site, f.message))
        .collect();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].0, "t.Item.name", "{reported:?}");
    assert!(
        reported[0].1.starts_with(
            "the crate needs this optional field: without it the body does not deserialize"
        ),
        "{reported:?}"
    );
}
