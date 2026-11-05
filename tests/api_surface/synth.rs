//! Response bodies synthesized from the snapshot, together carrying every
//! field, union arm and enum value a class reaches ([`Surface::fixtures`]).

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::{Field, Surface, Ty};

/// What [`Surface::fixtures`] leaves out.
#[derive(Default)]
pub struct SynthOptions<'a> {
    /// `Class.field` sites never synthesized (deprecated, Vertex-only, never
    /// served), each a reasoned allowlist entry in the caller.
    pub skip_fields: BTreeSet<&'a str>,
    /// `Class.field=value` enum values never synthesized.
    pub skip_values: BTreeSet<&'a str>,
    /// `Class.field` -> the value to use (a format the crate parses, where
    /// the snapshot only says "string").
    pub values: BTreeMap<&'a str, Value>,
}

/// `Class.field=value`, the site of an enum value.
fn literal_site(site: &str, value: &Value) -> String {
    format!("{site}={}", value.as_str().unwrap_or_default())
}

/// Values for a `str` field whose name says what it holds.
fn string_for(field: &str) -> Value {
    let f = field.to_ascii_lowercase();
    if f.ends_with("time")
        || f == "created"
        || f == "updated"
        || f == "modified"
        || f.starts_with("last_")
        || f.ends_with("_at")
    {
        json!("2026-11-01T12:00:00Z")
    } else if f.contains("uri") || f.contains("url") {
        json!("https://example.com/item")
    } else if f.contains("mime") || f == "content_type" {
        json!("text/plain")
    } else {
        json!(format!("{field}-value"))
    }
}

/// Below this depth a class gets only its required fields, so a recursive
/// class ends. [`Surface::fixtures`] fails if that leaves out a field.
const MAX_DEPTH: usize = 12;

/// The synthesizer's memo. A class's bodies are cached once per depth when
/// the depth limit cut something below it, and once for every depth
/// otherwise: a class first reached deep must not be reused, cut down, where
/// it is reached shallow.
#[derive(Default)]
struct Synth {
    cache: BTreeMap<(String, Option<usize>), Vec<Value>>,
    /// How many times the depth limit has left a field out (or a cached
    /// result that it cut down was reused).
    cuts: usize,
}

impl Surface {
    /// Response bodies for `ty`, together covering every field, union arm
    /// and enum value it reaches: a base body with every field set (first
    /// alternative each), then one body per further alternative of each
    /// field. Lists carry every alternative of their item at once.
    ///
    /// Panics if the bodies miss a field, class or enum value they should
    /// carry, so the coverage cannot shrink unseen.
    pub fn fixtures(&self, ty: &Ty, options: &SynthOptions<'_>) -> Vec<Value> {
        let bodies = self.alternatives(ty, "", options, &mut Synth::default(), 0);
        let mut expected = BTreeSet::new();
        self.expected_sites(ty, "", options, &mut BTreeSet::new(), &mut expected);
        let mut covered = BTreeSet::new();
        for body in &bodies {
            self.covered_sites(ty, body, "", &mut covered);
        }
        let missing: Vec<&String> = expected.difference(&covered).collect();
        assert!(
            missing.is_empty(),
            "{}: the synthesized bodies for {ty:?} miss {} site(s) they should \
             carry (a class reached only below depth {MAX_DEPTH}?): {missing:?}",
            self.name,
            missing.len()
        );
        bodies
    }

    fn alternatives(
        &self,
        ty: &Ty,
        site: &str,
        options: &SynthOptions<'_>,
        synth: &mut Synth,
        depth: usize,
    ) -> Vec<Value> {
        let field_name = site.rsplit('.').next().unwrap_or(site);
        if let Some(value) = options.values.get(site) {
            return vec![value.clone()];
        }
        match ty {
            Ty::Str => vec![string_for(field_name)],
            Ty::Int => vec![json!(7)],
            Ty::IntString => vec![json!("7")],
            Ty::Float => vec![json!(0.5)],
            Ty::Bool => vec![json!(true)],
            Ty::Bytes => vec![json!("AAAA")],
            Ty::DateTime => vec![json!("2026-11-01T12:00:00Z")],
            Ty::Any => vec![json!({"any_key": "any-value"})],
            Ty::Literal { values, .. } => {
                let kept: Vec<Value> = values
                    .iter()
                    .filter(|v| !options.skip_values.contains(literal_site(site, v).as_str()))
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    vec![string_for(field_name)]
                } else {
                    kept
                }
            }
            Ty::List(item) => {
                let items = self.alternatives(item, site, options, synth, depth + 1);
                vec![Value::Array(items)]
            }
            Ty::Map(inner) => {
                let values = self.alternatives(inner, site, options, synth, depth + 1);
                vec![Value::Object(
                    values
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (format!("key{i}"), v))
                        .collect(),
                )]
            }
            Ty::Union(arms) => {
                let mut out: Vec<Value> = Vec::new();
                for arm in arms {
                    for v in self.alternatives(arm, site, options, synth, depth + 1) {
                        if !out.contains(&v) {
                            out.push(v);
                        }
                    }
                }
                out
            }
            Ty::Ref(name) => {
                if let Some(alias) = self.aliases.get(name) {
                    return self.alternatives(alias, site, options, synth, depth);
                }
                if let Some(hit) = synth.cache.get(&(name.clone(), None)) {
                    return hit.clone();
                }
                if let Some(hit) = synth.cache.get(&(name.clone(), Some(depth))) {
                    // Cut down by the depth limit, which makes whatever
                    // embeds it depth-dependent too.
                    synth.cuts += 1;
                    return hit.clone();
                }
                let cuts = synth.cuts;
                let class = &self.classes[name];
                // Guard against recursive classes: below the limit, only
                // required fields.
                let shallow = depth > MAX_DEPTH;
                let mut columns: Vec<(String, Vec<Value>)> = Vec::new();
                for (wire, field) in &class.fields {
                    let field_site = format!("{name}.{wire}");
                    if options.skip_fields.contains(field_site.as_str()) || field.write_only {
                        continue;
                    }
                    if shallow && !field.required {
                        synth.cuts += 1;
                        continue;
                    }
                    let alts = self.alternatives(&field.ty, &field_site, options, synth, depth + 1);
                    if !alts.is_empty() {
                        columns.push((wire.clone(), alts));
                    }
                }
                let base: Map<String, Value> = columns
                    .iter()
                    .map(|(k, alts)| (k.clone(), alts[0].clone()))
                    .collect();
                let mut out = vec![Value::Object(base.clone())];
                for (k, alts) in &columns {
                    for alt in alts.iter().skip(1) {
                        let mut variant = base.clone();
                        variant.insert(k.clone(), alt.clone());
                        out.push(Value::Object(variant));
                    }
                }
                let exact = synth.cuts == cuts;
                synth
                    .cache
                    .insert((name.clone(), (!exact).then_some(depth)), out.clone());
                out
            }
        }
    }

    /// One body per class `bodies` reach, with every instance of that class
    /// cut down to its required fields: a served object may leave out any
    /// optional field, so the crate must read it without one. A field whose
    /// type is a single closed literal stays, being the discriminator a
    /// union is told apart by (`type`, `event_type`), which the API always
    /// sends even where the bindings mark it optional ([`Self::optional`]).
    ///
    /// Returns `(class, index into bodies, cut body)`, in the order the
    /// classes are first met; a class with no optional field to drop has
    /// none. Classes in `done` are skipped, and every class met is added to
    /// it, so a caller can share it across targets that reach the same
    /// classes.
    pub fn minimal_variants(
        &self,
        ty: &Ty,
        bodies: &[Value],
        done: &mut BTreeSet<String>,
    ) -> Vec<(String, usize, Value)> {
        let mut out = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            let mut classes = BTreeSet::new();
            self.covered_sites(ty, body, "", &mut classes);
            for class in classes {
                let Some(fields) = self.classes.get(&class).map(|c| &c.fields) else {
                    continue;
                };
                if !done.insert(class.clone()) {
                    continue;
                }
                let mut reduced = body.clone();
                let drop = |key: &str| fields.get(key).is_some_and(Self::optional);
                if self.reduce(ty, &mut reduced, &class, &drop) {
                    out.push((class, index, reduced));
                }
            }
        }
        out
    }

    /// Whether [`Self::minimal_variants`] cuts a field: optional, and not a
    /// constant discriminator.
    pub fn optional(field: &Field) -> bool {
        !field.required
            && !matches!(&field.ty, Ty::Literal { values, open: false } if values.len() == 1)
    }

    /// `body` with `field` removed from every instance of `class`, or `None`
    /// when no instance has it: to find which cut field a finding on a cut
    /// body comes from.
    pub fn without_field(&self, ty: &Ty, body: &Value, class: &str, field: &str) -> Option<Value> {
        let mut out = body.clone();
        self.reduce(ty, &mut out, class, &|key| key == field)
            .then_some(out)
    }

    /// Removes the keys `drop` names from every instance of `class` in
    /// `value`, walking `value` against `ty` as [`Self::covered_sites`]
    /// does. Returns whether anything was removed.
    fn reduce(&self, ty: &Ty, value: &mut Value, class: &str, drop: &dyn Fn(&str) -> bool) -> bool {
        match ty {
            Ty::List(item) => value
                .as_array_mut()
                .into_iter()
                .flatten()
                .fold(false, |cut, v| self.reduce(item, v, class, drop) | cut),
            Ty::Map(inner) => value
                .as_object_mut()
                .into_iter()
                .flat_map(Map::values_mut)
                .fold(false, |cut, v| self.reduce(inner, v, class, drop) | cut),
            Ty::Union(arms) => {
                let mut picked: Vec<&Ty> = arms
                    .iter()
                    .filter(|a| self.discriminator_matches(a, value))
                    .collect();
                if picked.is_empty() {
                    let scored: Vec<(usize, &Ty)> = arms
                        .iter()
                        .map(|a| (self.check_at(a, value, "$", "").len(), a))
                        .collect();
                    let best = scored.iter().map(|(n, _)| *n).min().unwrap_or(0);
                    picked = scored
                        .into_iter()
                        .filter(|(n, _)| *n == best)
                        .map(|(_, a)| a)
                        .collect();
                }
                // Only the arm that is `class` itself drops keys; the others
                // just walk on into their fields.
                picked
                    .into_iter()
                    .fold(false, |cut, arm| self.reduce(arm, value, class, drop) | cut)
            }
            Ty::Ref(name) => {
                if let Some(alias) = self.aliases.get(name) {
                    return self.reduce(alias, value, class, drop);
                }
                let Some(object) = value.as_object_mut() else {
                    return false;
                };
                let fields = &self.classes[name].fields;
                let mut cut = false;
                if name == class {
                    object.retain(|key, _| {
                        let keep = !drop(key);
                        cut |= !keep;
                        keep
                    });
                }
                for (key, v) in object.iter_mut() {
                    if let Some(field) = fields.get(key) {
                        cut |= self.reduce(&field.ty, v, class, drop);
                    }
                }
                cut
            }
            _ => false,
        }
    }

    /// What [`Surface::fixtures`] must cover for `ty`: every class it
    /// reaches, each of their fields, and each enum value, as `Class`,
    /// `Class.field` and `Class.field=value`, less what `options` leaves out.
    fn expected_sites(
        &self,
        ty: &Ty,
        site: &str,
        options: &SynthOptions<'_>,
        seen: &mut BTreeSet<String>,
        out: &mut BTreeSet<String>,
    ) {
        match ty {
            Ty::Literal { values, .. } if !site.is_empty() => {
                for v in values {
                    let key = literal_site(site, v);
                    if !options.skip_values.contains(key.as_str()) {
                        out.insert(key);
                    }
                }
            }
            Ty::List(inner) | Ty::Map(inner) => {
                self.expected_sites(inner, site, options, seen, out);
            }
            Ty::Union(arms) => {
                for arm in arms {
                    self.expected_sites(arm, site, options, seen, out);
                }
            }
            Ty::Ref(name) => {
                if let Some(alias) = self.aliases.get(name) {
                    return self.expected_sites(alias, site, options, seen, out);
                }
                if !seen.insert(name.clone()) {
                    return;
                }
                out.insert(name.clone());
                for (wire, field) in &self.classes[name].fields {
                    let field_site = format!("{name}.{wire}");
                    if options.skip_fields.contains(field_site.as_str()) || field.write_only {
                        continue;
                    }
                    out.insert(field_site.clone());
                    if !options.values.contains_key(field_site.as_str()) {
                        self.expected_sites(&field.ty, &field_site, options, seen, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// The `Class`, `Class.field` and `Class.field=value` sites `value`
    /// carries, walked against `ty`. A value in a union counts for every arm
    /// it is tagged as or, untagged, every arm it fits.
    fn covered_sites(&self, ty: &Ty, value: &Value, site: &str, out: &mut BTreeSet<String>) {
        match ty {
            Ty::Literal { .. } => {
                if let Some(s) = value.as_str() {
                    out.insert(format!("{site}={s}"));
                }
            }
            Ty::List(item) => {
                for v in value.as_array().into_iter().flatten() {
                    self.covered_sites(item, v, site, out);
                }
            }
            Ty::Map(inner) => {
                for v in value.as_object().into_iter().flat_map(Map::values) {
                    self.covered_sites(inner, v, site, out);
                }
            }
            Ty::Union(arms) => {
                let mut picked: Vec<&Ty> = arms
                    .iter()
                    .filter(|a| self.discriminator_matches(a, value))
                    .collect();
                if picked.is_empty() {
                    let scored: Vec<(usize, &Ty)> = arms
                        .iter()
                        .map(|a| (self.check_at(a, value, "$", site).len(), a))
                        .collect();
                    let best = scored.iter().map(|(n, _)| *n).min().unwrap_or(0);
                    picked = scored
                        .into_iter()
                        .filter(|(n, _)| *n == best)
                        .map(|(_, a)| a)
                        .collect();
                }
                for arm in picked {
                    self.covered_sites(arm, value, site, out);
                }
            }
            Ty::Ref(name) => {
                if let Some(alias) = self.aliases.get(name) {
                    return self.covered_sites(alias, value, site, out);
                }
                let Some(object) = value.as_object() else {
                    return;
                };
                out.insert(name.clone());
                let class = &self.classes[name];
                for (k, v) in object {
                    if let Some(field) = class.fields.get(k) {
                        let field_site = format!("{name}.{k}");
                        out.insert(field_site.clone());
                        self.covered_sites(&field.ty, v, &field_site, out);
                    }
                }
            }
            _ => {}
        }
    }
}
