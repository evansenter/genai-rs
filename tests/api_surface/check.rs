//! Walking a JSON value against a type: [`Surface::check`] reports what does
//! not fit (the request side), [`Surface::sites`] names the `Class.field` of
//! every key, and [`Surface::reachable_classes`] lists what a type reaches.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use super::{Class, Surface, Ty};

/// One finding of [`Surface::check`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// JSON path, `$.tools[0].type`.
    pub path: String,
    /// `Class.field` the finding is about (for an unknown key: the class
    /// and the key), the allowlist key.
    pub site: String,
    pub message: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} [{}]: {}", self.path, self.site, self.message)
    }
}

impl Surface {
    /// Walks `value` against `ty` and returns what does not fit.
    pub fn check(&self, ty: &Ty, value: &Value) -> Vec<Finding> {
        self.check_at(ty, value, "$", "<root>")
    }

    pub(super) fn check_at(&self, ty: &Ty, value: &Value, path: &str, site: &str) -> Vec<Finding> {
        if let Ty::Ref(name) = ty
            && let Some(alias) = self.aliases.get(name)
        {
            if let Ty::Union(arms) = self.resolve(alias) {
                return self.check_union(Some(name), arms, value, path, site);
            }
            return self.check_at(alias, value, path, site);
        }
        let wrong = |expected: &str| {
            vec![Finding {
                path: path.to_string(),
                site: site.to_string(),
                message: format!("expected {expected}, got {}", abbreviate(value)),
            }]
        };
        match self.resolve(ty) {
            Ty::Any => vec![],
            Ty::Str | Ty::Bytes | Ty::DateTime if value.is_string() => vec![],
            Ty::Str => wrong("a string"),
            Ty::Bytes => wrong("base64 text"),
            Ty::DateTime => wrong("an RFC 3339 timestamp"),
            Ty::Int | Ty::IntString if is_integer(value) => vec![],
            Ty::Int | Ty::IntString => wrong("an integer"),
            Ty::Float if value.is_number() => vec![],
            Ty::Float => wrong("a number"),
            Ty::Bool if value.is_boolean() => vec![],
            Ty::Bool => wrong("a boolean"),
            Ty::Literal { values, open } => {
                if values.contains(value) || (*open && value.is_string()) {
                    vec![]
                } else {
                    let mut findings = wrong(&format!("one of {values:?}"));
                    if let Some(s) = value.as_str() {
                        findings[0].site = format!("{site}={s}");
                    }
                    findings
                }
            }
            Ty::List(item) => match value.as_array() {
                Some(items) => items
                    .iter()
                    .enumerate()
                    .flat_map(|(i, v)| self.check_at(item, v, &format!("{path}[{i}]"), site))
                    .collect(),
                None => wrong("an array"),
            },
            Ty::Map(inner) => match value.as_object() {
                Some(map) => map
                    .iter()
                    .flat_map(|(k, v)| self.check_at(inner, v, &format!("{path}.{k}"), site))
                    .collect(),
                None => wrong("an object"),
            },
            Ty::Union(arms) => self.check_union(None, arms, value, path, site),
            Ty::Ref(name) => match self.classes.get(name) {
                Some(class) => self.check_class(name, class, value, path),
                None => panic!("unresolved {name}"),
            },
        }
    }

    /// Passes if an arm fits; otherwise reports the likeliest arm's
    /// findings: the one the value is tagged as, else one of the value's JSON
    /// shape, else the one with the fewest findings.
    ///
    /// A value carrying an arm's tag is checked against the tagged arms
    /// alone. Otherwise a free-form arm (`ResponseFormat`'s
    /// `Dict[str, Any]`) would take every object, and a wrong key on
    /// `{"type": "audio", ...}` would pass unseen.
    fn check_union(
        &self,
        name: Option<&str>,
        arms: &[Ty],
        value: &Value,
        path: &str,
        site: &str,
    ) -> Vec<Finding> {
        // A tagged object whose tag no arm carries: one finding naming the
        // union and the tag, rather than the closest arm's mismatches.
        if let Some(tag) = self.untagged_by_any_arm(arms, value) {
            let union = name.unwrap_or(site);
            return vec![Finding {
                path: path.to_string(),
                site: format!("{union}={tag}"),
                message: format!("no arm of {union} is tagged {tag:?}"),
            }];
        }
        let tagged: Vec<&Ty> = arms
            .iter()
            .filter(|arm| self.discriminator_matches(arm, value))
            .collect();
        let candidates: Vec<&Ty> = if tagged.is_empty() {
            arms.iter().collect()
        } else {
            tagged
        };
        let mut best: Option<((bool, bool, usize), Vec<Finding>)> = None;
        for arm in candidates {
            let findings = self.check_at(arm, value, path, site);
            if findings.is_empty() {
                return findings;
            }
            let rank = (
                !self.discriminator_matches(arm, value),
                !self.shape_matches(arm, value),
                findings.len(),
            );
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((rank, findings));
            }
        }
        best.map(|(_, f)| f).unwrap_or_default()
    }

    /// The value's tag, when the union's arms are tagged classes and none
    /// carries it.
    fn untagged_by_any_arm(&self, arms: &[Ty], value: &Value) -> Option<String> {
        let object = value.as_object()?;
        let mut tag_fields = BTreeSet::new();
        for arm in arms {
            let (_, class) = self.class_of(arm)?;
            for (wire, field) in &class.fields {
                if matches!(self.resolve(&field.ty), Ty::Literal { values, open: false } if values.len() == 1)
                {
                    tag_fields.insert(wire.as_str());
                }
            }
        }
        let tag = tag_fields
            .iter()
            .find_map(|f| object.get(*f).and_then(Value::as_str))?;
        let tagged = arms
            .iter()
            .any(|arm| self.discriminator_matches(arm, value));
        (!tagged).then(|| tag.to_string())
    }

    /// Whether `value` has the JSON shape `ty` takes (object, array, ...).
    fn shape_matches(&self, ty: &Ty, value: &Value) -> bool {
        match self.resolve(ty) {
            Ty::Any => true,
            Ty::Str | Ty::Bytes | Ty::DateTime | Ty::Literal { .. } => value.is_string(),
            Ty::Int | Ty::IntString => value.is_number() || value.is_string(),
            Ty::Float => value.is_number(),
            Ty::Bool => value.is_boolean(),
            Ty::List(_) => value.is_array(),
            Ty::Map(_) | Ty::Ref(_) => value.is_object(),
            Ty::Union(arms) => arms.iter().any(|a| self.shape_matches(a, value)),
        }
    }

    /// Whether `value` carries the arm's tag: a single-valued literal field
    /// (`type`, `event_type`) with that value.
    pub(super) fn discriminator_matches(&self, arm: &Ty, value: &Value) -> bool {
        let Some((_, class)) = self.class_of(arm) else {
            return false;
        };
        class.fields.iter().any(|(wire, field)| {
            matches!(self.resolve(&field.ty), Ty::Literal { values, open: false }
                if values.len() == 1 && value.get(wire) == Some(&values[0]))
        })
    }

    fn check_class(&self, name: &str, class: &Class, value: &Value, path: &str) -> Vec<Finding> {
        let Some(object) = value.as_object() else {
            return vec![Finding {
                path: path.to_string(),
                site: name.to_string(),
                message: format!("expected a {name} object, got {}", abbreviate(value)),
            }];
        };
        let mut findings = Vec::new();
        for (key, v) in object {
            let site = format!("{name}.{key}");
            let child = format!("{path}.{key}");
            match class.fields.get(key) {
                Some(field) => findings.extend(self.check_at(&field.ty, v, &child, &site)),
                None if class.open => {}
                None => findings.push(Finding {
                    path: child,
                    site,
                    message: format!("{name} has no field {key:?}"),
                }),
            }
        }
        for (key, field) in &class.fields {
            if field.required && !field.read_only && !object.contains_key(key) {
                findings.push(Finding {
                    path: format!("{path}.{key}"),
                    site: format!("{name}.{key}"),
                    message: "required by the class, not sent".into(),
                });
            }
        }
        findings
    }
}

/// A value for a message: its JSON, cut at 80 characters.
fn abbreviate(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(80) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text,
    }
}

fn is_integer(value: &Value) -> bool {
    value.is_i64()
        || value.is_u64()
        || value.as_str().is_some_and(|s| {
            !s.is_empty()
                && s.trim_start_matches('-')
                    .bytes()
                    .all(|b| b.is_ascii_digit())
        })
}

impl Surface {
    /// Every class `ty` reaches, through fields, lists, maps, unions and
    /// aliases.
    pub fn reachable_classes(&self, ty: &Ty) -> BTreeSet<String> {
        fn walk(surface: &Surface, ty: &Ty, out: &mut BTreeSet<String>) {
            match ty {
                Ty::List(inner) | Ty::Map(inner) => walk(surface, inner, out),
                Ty::Union(arms) => arms.iter().for_each(|a| walk(surface, a, out)),
                Ty::Ref(name) => {
                    if let Some(alias) = surface.aliases.get(name) {
                        walk(surface, alias, out);
                    } else if out.insert(name.clone()) {
                        for field in surface.classes[name].fields.values() {
                            walk(surface, &field.ty, out);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut out = BTreeSet::new();
        walk(self, ty, &mut out);
        out
    }

    /// `JSON path -> Class.field` for every object key in `value`, walked
    /// against `ty` the way [`Surface::check`] walks a request.
    pub fn sites(&self, ty: &Ty, value: &Value) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        self.sites_at(ty, value, "$", &mut out);
        out
    }

    fn sites_at(&self, ty: &Ty, value: &Value, path: &str, out: &mut BTreeMap<String, String>) {
        match self.resolve(ty) {
            Ty::List(item) => {
                if let Some(items) = value.as_array() {
                    for (i, v) in items.iter().enumerate() {
                        self.sites_at(item, v, &format!("{path}[{i}]"), out);
                    }
                }
            }
            Ty::Map(inner) => {
                if let Some(map) = value.as_object() {
                    for (k, v) in map {
                        self.sites_at(inner, v, &format!("{path}.{k}"), out);
                    }
                }
            }
            Ty::Union(arms) => {
                let arm = arms
                    .iter()
                    .find(|a| self.discriminator_matches(a, value))
                    .or_else(|| {
                        arms.iter()
                            .find(|a| self.check_at(a, value, path, "").is_empty())
                    });
                if let Some(arm) = arm {
                    self.sites_at(arm, value, path, out);
                }
            }
            Ty::Ref(name) => {
                if let (Some(class), Some(object)) = (self.classes.get(name), value.as_object()) {
                    for (k, v) in object {
                        let child = format!("{path}.{k}");
                        out.insert(child.clone(), format!("{name}.{k}"));
                        if let Some(field) = class.fields.get(k) {
                            self.sites_at(&field.ty, v, &child, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
