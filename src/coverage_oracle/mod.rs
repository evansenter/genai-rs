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
// Under `strict-unknown` an unmodeled enum value fails a whole body instead
// of showing as `Unknown`; the default build is the one the response side
// measures.
#[cfg(not(feature = "strict-unknown"))]
mod response;

use api_surface::{Finding, Surface};
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

/// The bindings snapshot is the version `api-surface-sweep` is swept to.
#[test]
fn snapshot_matches_the_sweep_baseline() {
    let baseline = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/last-swept-sdk-version"),
    )
    .expect("read .github/last-swept-sdk-version");
    let bindings = Surface::bindings();
    assert_eq!(
        bindings.version,
        baseline.trim(),
        "tests/fixtures/api_surface/bindings.json is for google-genai {}, the sweep \
         baseline is {}: regenerate it with .github/scripts/refresh_api_snapshot.sh",
        bindings.version,
        baseline.trim()
    );
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
