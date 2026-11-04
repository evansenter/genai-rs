//! An offline model of the Interactions API surface, read from the committed
//! snapshot in `tests/fixtures/api_surface/`, for the coverage oracle.
//!
//! Two sources load into the same model:
//!
//! - `bindings.json`: google-genai's generated `_gaos` classes, plus the
//!   Files, File Search Store and Document classes from its hand-written
//!   `types.py`, with every type reference resolved
//!   (`.github/scripts/dump_bindings_schema.py --json`).
//! - `openapi-v1beta.json` / `openapi-v1.json`: the published OpenAPI
//!   schemas without their prose (`.github/scripts/snapshot_openapi.py`).
//!
//! On that model this module offers two checks:
//!
//! - [`Surface::check`] walks a JSON value the crate sends against a class
//!   and reports every key, at any depth, that the class does not define,
//!   every required key that is missing, and every value of the wrong shape.
//! - [`Surface::fixtures`] synthesizes response bodies that carry every
//!   field a class defines, with every union arm and enum value, so a
//!   deserialize-then-inspect pass can find what the crate does not model.
//!
//! Shared by the in-crate oracle (`src/coverage_oracle/`, through `#[path]`)
//! and `tests/request_wire_oracle.rs`. Not a test target itself: Cargo only
//! builds `tests/*.rs` and `tests/*/main.rs`.

#![allow(dead_code)]

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// `tests/fixtures/api_surface/`.
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api_surface")
}

/// A field or alias type.
#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Str,
    Int,
    /// A protobuf int64: a JSON string of digits on the wire.
    IntString,
    Float,
    Bool,
    /// Base64 text.
    Bytes,
    /// RFC 3339 text.
    DateTime,
    Any,
    /// String values; `open` means any other string is valid too.
    Literal {
        values: Vec<Value>,
        open: bool,
    },
    List(Box<Ty>),
    Map(Box<Ty>),
    /// A class or alias by name.
    Ref(String),
    Union(Vec<Ty>),
}

#[derive(Clone, Debug)]
pub struct Field {
    pub ty: Ty,
    pub required: bool,
    pub deprecated: bool,
    /// Output only (`readOnly` in the spec).
    pub read_only: bool,
    /// Input only (`writeOnly` in the spec): never in a response.
    pub write_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Class {
    /// Takes keys beyond `fields` (`DynamicAgentConfig`).
    pub open: bool,
    pub fields: BTreeMap<String, Field>,
}

/// One source's classes and aliases.
pub struct Surface {
    /// `bindings`, `openapi-v1beta`, ...
    pub name: String,
    /// The google-genai version, or the spec's fetch date.
    pub version: String,
    pub classes: BTreeMap<String, Class>,
    pub aliases: BTreeMap<String, Ty>,
}

/// A snapshot file, parsed.
pub fn read_json(file: &str) -> Value {
    let path = fixtures_dir().join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

// =============================================================================
// Loading
// =============================================================================

impl Surface {
    /// `bindings.json`, the google-genai classes.
    pub fn bindings() -> Self {
        Self::from_bindings(&read_json("bindings.json"))
    }

    /// A `bindings.json` document (the oracle's self-tests edit one).
    pub fn from_bindings(root: &Value) -> Self {
        let version = root["sdk_version"]
            .as_str()
            .expect("sdk_version")
            .to_string();
        let aliases = root["aliases"]
            .as_object()
            .expect("aliases")
            .iter()
            .map(|(name, ty)| (name.clone(), bindings_ty(ty)))
            .collect();
        let classes = root["classes"]
            .as_object()
            .expect("classes")
            .iter()
            .map(|(name, class)| {
                let fields: BTreeMap<String, Field> = class["fields"]
                    .as_object()
                    .expect("fields")
                    .iter()
                    .map(|(wire, field)| {
                        (
                            wire.clone(),
                            Field {
                                ty: bindings_ty(&field["type"]),
                                required: field["required"].as_bool().unwrap_or(false),
                                deprecated: field["deprecated"].as_bool().unwrap_or(false),
                                read_only: false,
                                write_only: false,
                            },
                        )
                    })
                    .collect();
                // A class with no fields (`FunctionResultStepResult`) is an
                // opaque object: the spec types it as a bare `object`.
                let open = class["open"].as_bool().unwrap_or(false) || fields.is_empty();
                (name.clone(), Class { open, fields })
            })
            .collect();
        let surface = Self {
            name: "bindings".into(),
            version,
            classes,
            aliases,
        };
        surface.assert_closed();
        surface
    }

    /// An `openapi-*.json` snapshot.
    pub fn openapi(file: &str) -> Self {
        let root = read_json(file);
        let mut surface = Self {
            name: file.trim_end_matches(".json").into(),
            version: root["fetched"].as_str().unwrap_or("unknown").into(),
            classes: BTreeMap::new(),
            aliases: BTreeMap::new(),
        };
        for (name, schema) in root["schemas"].as_object().expect("schemas") {
            if schema.get("properties").is_some() {
                surface.openapi_class(name, schema);
            } else {
                let ty = surface.openapi_ty(schema, name);
                surface.aliases.insert(name.clone(), ty);
            }
        }
        surface.assert_closed();
        surface
    }

    fn openapi_class(&mut self, name: &str, schema: &Value) {
        let required: BTreeSet<&str> = schema["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut class = Class::default();
        let props = schema["properties"].as_object().expect("properties");
        for (wire, prop) in props {
            let ty = self.openapi_ty(prop, &format!("{name}.{wire}"));
            class.fields.insert(
                wire.clone(),
                Field {
                    ty,
                    required: required.contains(wire.as_str()),
                    deprecated: prop["deprecated"].as_bool().unwrap_or(false),
                    read_only: prop["readOnly"].as_bool().unwrap_or(false),
                    write_only: prop["writeOnly"].as_bool().unwrap_or(false),
                },
            );
        }
        class.open = schema["additionalProperties"].as_bool().unwrap_or(false);
        self.classes.insert(name.to_string(), class);
    }

    /// Converts a schema; an inline object with properties becomes a class
    /// named after where it sits (`Parent.field`).
    fn openapi_ty(&mut self, schema: &Value, owner: &str) -> Ty {
        if let Some(target) = schema["$ref"].as_str() {
            return Ty::Ref(target.rsplit('/').next().unwrap().to_string());
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(arms) = schema[key].as_array() {
                return Ty::Union(arms.iter().map(|a| self.openapi_ty(a, owner)).collect());
            }
        }
        if let Some(all) = schema["allOf"].as_array()
            && all.len() == 1
        {
            return self.openapi_ty(&all[0], owner);
        }
        if let Some(value) = schema.get("const") {
            return Ty::Literal {
                values: vec![value.clone()],
                open: false,
            };
        }
        if let Some(values) = schema["enum"].as_array() {
            return Ty::Literal {
                values: values.clone(),
                open: schema["x-speakeasy-unknown-values"] == "allow",
            };
        }
        match schema["type"].as_str() {
            Some("string") => match schema["format"].as_str() {
                Some("byte") => Ty::Bytes,
                Some("date-time") => Ty::DateTime,
                Some("int64" | "uint64") => Ty::IntString,
                _ => Ty::Str,
            },
            Some("integer") => Ty::Int,
            Some("number") => Ty::Float,
            Some("boolean") => Ty::Bool,
            Some("array") => Ty::List(Box::new(self.openapi_ty(&schema["items"], owner))),
            _ if schema.get("properties").is_some() => {
                self.openapi_class(owner, schema);
                Ty::Ref(owner.to_string())
            }
            _ => match schema.get("additionalProperties") {
                Some(inner) if inner.is_object() => {
                    Ty::Map(Box::new(self.openapi_ty(inner, owner)))
                }
                _ => Ty::Any,
            },
        }
    }

    /// Every reference names a class or alias: a dangling one would make the
    /// checks below silently permissive.
    fn assert_closed(&self) {
        fn refs(ty: &Ty, out: &mut Vec<String>) {
            match ty {
                Ty::Ref(name) => out.push(name.clone()),
                Ty::List(inner) | Ty::Map(inner) => refs(inner, out),
                Ty::Union(arms) => arms.iter().for_each(|a| refs(a, out)),
                _ => {}
            }
        }
        let mut names = Vec::new();
        for ty in self.aliases.values() {
            refs(ty, &mut names);
        }
        for class in self.classes.values() {
            for field in class.fields.values() {
                refs(&field.ty, &mut names);
            }
        }
        let dangling: BTreeSet<_> = names
            .into_iter()
            .filter(|n| !self.classes.contains_key(n) && !self.aliases.contains_key(n))
            .collect();
        assert!(
            dangling.is_empty(),
            "{}: unresolved references {dangling:?}",
            self.name
        );
    }

    /// A class or alias by name, panicking with the near misses.
    pub fn ty(&self, name: &str) -> Ty {
        assert!(
            self.classes.contains_key(name) || self.aliases.contains_key(name),
            "{}: no class or alias {name:?}; similar: {:?}",
            self.name,
            self.similar(name)
        );
        Ty::Ref(name.to_string())
    }

    fn similar(&self, name: &str) -> Vec<&String> {
        let needle = name.rsplit('.').next().unwrap_or(name).to_lowercase();
        self.classes
            .keys()
            .chain(self.aliases.keys())
            .filter(|k| k.to_lowercase().contains(&needle))
            .collect()
    }

    /// Follows alias references to a non-alias type.
    pub fn resolve<'a>(&'a self, mut ty: &'a Ty) -> &'a Ty {
        while let Ty::Ref(name) = ty {
            match self.aliases.get(name) {
                Some(next) => ty = next,
                None => break,
            }
        }
        ty
    }

    /// The class a type names, after aliases.
    pub fn class_of(&self, ty: &Ty) -> Option<(&str, &Class)> {
        match self.resolve(ty) {
            Ty::Ref(name) => self
                .classes
                .get_key_value(name)
                .map(|(k, c)| (k.as_str(), c)),
            _ => None,
        }
    }
}

fn bindings_ty(v: &Value) -> Ty {
    match v["kind"].as_str().expect("type kind") {
        "str" => Ty::Str,
        "int" => Ty::Int,
        "float" => Ty::Float,
        "bool" => Ty::Bool,
        "bytes" => Ty::Bytes,
        "datetime" => Ty::DateTime,
        "any" => Ty::Any,
        "literal" => Ty::Literal {
            values: v["values"].as_array().cloned().unwrap_or_default(),
            open: v["open"].as_bool().unwrap_or(false),
        },
        "list" => Ty::List(Box::new(bindings_ty(&v["item"]))),
        "map" => Ty::Map(Box::new(bindings_ty(&v["value"]))),
        "ref" => Ty::Ref(v["name"].as_str().expect("ref name").to_string()),
        "union" => Ty::Union(
            v["arms"]
                .as_array()
                .expect("arms")
                .iter()
                .map(bindings_ty)
                .collect(),
        ),
        other => panic!("unknown type kind {other:?}"),
    }
}

// =============================================================================
// Request side: is every key the crate sends on the class?
// =============================================================================

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

    fn check_at(&self, ty: &Ty, value: &Value, path: &str, site: &str) -> Vec<Finding> {
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

    /// Passes if any arm fits; otherwise reports the likeliest arm's
    /// findings: the one the value is tagged as, else one of the value's JSON
    /// shape, else the one with the fewest findings.
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
        let mut best: Option<((bool, bool, usize), Vec<Finding>)> = None;
        for arm in arms {
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
    fn discriminator_matches(&self, arm: &Ty, value: &Value) -> bool {
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
pub fn abbreviate(value: &Value) -> String {
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

// =============================================================================
// Response side: fixtures carrying every field
// =============================================================================

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

impl Surface {
    /// Response bodies for `ty`, together covering every field, union arm
    /// and enum value it reaches: a base body with every field set (first
    /// alternative each), then one body per further alternative of each
    /// field. Lists carry every alternative of their item at once.
    pub fn fixtures(&self, ty: &Ty, options: &SynthOptions<'_>) -> Vec<Value> {
        let mut cache = BTreeMap::new();
        self.alternatives(ty, "", options, &mut cache, 0)
    }

    fn alternatives(
        &self,
        ty: &Ty,
        site: &str,
        options: &SynthOptions<'_>,
        cache: &mut BTreeMap<String, Vec<Value>>,
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
                    .filter(|v| {
                        let key = format!("{site}={}", v.as_str().unwrap_or_default());
                        !options.skip_values.contains(key.as_str())
                    })
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    vec![string_for(field_name)]
                } else {
                    kept
                }
            }
            Ty::List(item) => {
                let mut items = self.alternatives(item, site, options, cache, depth + 1);
                items.truncate(64);
                vec![Value::Array(items)]
            }
            Ty::Map(inner) => {
                let values = self.alternatives(inner, site, options, cache, depth + 1);
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
                    for v in self.alternatives(arm, site, options, cache, depth + 1) {
                        if !out.contains(&v) {
                            out.push(v);
                        }
                    }
                }
                out
            }
            Ty::Ref(name) => {
                if let Some(alias) = self.aliases.get(name) {
                    return self.alternatives(alias, site, options, cache, depth);
                }
                if let Some(hit) = cache.get(name) {
                    return hit.clone();
                }
                let class = &self.classes[name];
                // Guard against recursive classes: below the limit, only
                // required fields.
                let shallow = depth > 12;
                let mut columns: Vec<(String, Vec<Value>)> = Vec::new();
                for (wire, field) in &class.fields {
                    let field_site = format!("{name}.{wire}");
                    if options.skip_fields.contains(field_site.as_str())
                        || field.write_only
                        || (shallow && !field.required)
                    {
                        continue;
                    }
                    let alts = self.alternatives(&field.ty, &field_site, options, cache, depth + 1);
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
                cache.insert(name.clone(), out.clone());
                out
            }
        }
    }

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

// =============================================================================
// Reading what a deserialized value kept, from its `Debug` output
// =============================================================================
//
// Every response struct keeps unmodeled keys in a flattened `extra` map, and
// every growable enum has an `Unknown` variant. Both show in `{:?}`, which is
// the one view of a value that tells modeled from unmodeled: serializing it
// again puts `extra` keys back beside the modeled ones.

/// The keys of every non-empty `extra` map in a `{:?}` rendering.
///
/// Matches `extra: {"key": ..}` (a derived `Debug` of a JSON map) and
/// `extra: ["key", ..]` (the redacting impls, which list keys only).
pub fn debug_extra_keys(debug: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for (at, _) in debug.match_indices("extra: ") {
        let rest = &debug[at + "extra: ".len()..];
        let (open, close) = match rest.as_bytes().first() {
            Some(b'{') => (b'{', b'}'),
            Some(b'[') => (b'[', b']'),
            _ => continue,
        };
        let body = &rest[1..];
        let mut depth = 0usize;
        let mut chars = body.char_indices().peekable();
        let mut expect_key = true;
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    let start = i + 1;
                    let mut end = start;
                    let mut escaped = false;
                    for (j, d) in chars.by_ref() {
                        if escaped {
                            escaped = false;
                        } else if d == '\\' {
                            escaped = true;
                        } else if d == '"' {
                            end = j;
                            break;
                        }
                    }
                    if depth == 0 && expect_key {
                        keys.push(body[start..end].replace("\\\"", "\""));
                        expect_key = false;
                    }
                }
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' if depth > 0 => depth -= 1,
                c if depth == 0 && c as u32 == u32::from(close) => break,
                ',' if depth == 0 => expect_key = true,
                _ => {}
            }
        }
        let _ = open;
    }
    keys
}

/// The wire value of every `Unknown { <field>: "value", .. }` variant in a
/// `{:?}` rendering.
pub fn debug_unknown_values(debug: &str) -> Vec<String> {
    let mut values = Vec::new();
    for (at, _) in debug.match_indices("Unknown { ") {
        let rest = &debug[at + "Unknown { ".len()..];
        if let Some(colon) = rest.find(": \"") {
            let value_start = colon + 3;
            if let Some(len) = rest[value_start..].find('"') {
                values.push(rest[value_start..value_start + len].to_string());
            }
        }
    }
    values
}

/// Every object-key path (`$.steps`) and list-element path (`$.steps[0]`)
/// in `value`. The element paths matter: a key on an object that sits
/// directly in a list (`$.steps[0].content[0].annotations[0].title`) has an
/// element as its parent, and a key counts as dropped only under a kept
/// parent.
pub fn key_paths(value: &Value) -> BTreeSet<String> {
    fn walk(v: &Value, path: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    let p = format!("{path}.{k}");
                    out.insert(p.clone());
                    walk(child, &p, out);
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    let p = format!("{path}[{i}]");
                    out.insert(p.clone());
                    walk(child, &p, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(value, "$", &mut out);
    out
}

/// Removes the value at a `key_paths` path (an object key or a list
/// element); false if it is not there.
pub fn remove_path(value: &mut Value, path: &str) -> bool {
    let Some(rest) = path.strip_prefix('$') else {
        return false;
    };
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                let mut index = String::from("[");
                for d in chars.by_ref() {
                    index.push(d);
                    if d == ']' {
                        break;
                    }
                }
                segments.push(index);
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    let Some((last, parents)) = segments.split_last() else {
        return false;
    };
    let mut node = value;
    for segment in parents {
        node = match segment.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Some(index) => match index.parse::<usize>().ok().and_then(|i| node.get_mut(i)) {
                Some(n) => n,
                None => return false,
            },
            None => match node.get_mut(segment.as_str()) {
                Some(n) => n,
                None => return false,
            },
        };
    }
    match last.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(index) => match (index.parse::<usize>(), node.as_array_mut()) {
            (Ok(i), Some(items)) if i < items.len() => {
                items.remove(i);
                true
            }
            _ => false,
        },
        None => node
            .as_object_mut()
            .is_some_and(|map| map.remove(last.as_str()).is_some()),
    }
}

/// The string value at each path, for finding where an `Unknown` came from.
pub fn string_values(value: &Value) -> BTreeMap<String, String> {
    fn walk(v: &Value, path: &str, out: &mut BTreeMap<String, String>) {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    walk(child, &format!("{path}.{k}"), out);
                }
            }
            Value::Array(items) => {
                for (i, child) in items.iter().enumerate() {
                    walk(child, &format!("{path}[{i}]"), out);
                }
            }
            Value::String(s) => {
                out.insert(path.to_string(), s.clone());
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    walk(value, "$", &mut out);
    out
}
