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
//!   field a class defines, with every union arm and enum value (and fails
//!   if they do not), so a deserialize-then-inspect pass can find what the
//!   crate does not model.
//!
//! Shared by the in-crate oracle (`src/coverage_oracle/`, through `#[path]`)
//! and `tests/request_wire_oracle.rs`, along with the date check on their
//! reasoned exceptions ([`undated_live_claim`]), which `tests/ci_coverage.rs`
//! includes on its own. Not a test target itself: Cargo only builds
//! `tests/*.rs` and `tests/*/main.rs`.

#![allow(dead_code)]

mod check;
mod claims;
mod readback;
mod synth;

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

// Each includer uses a different part.
#[allow(unused_imports)]
pub use check::Finding;
#[allow(unused_imports)]
pub use claims::{undated_live_claim, undated_live_claims};
#[allow(unused_imports)]
pub use readback::{debug_extra_keys, debug_unknown_values, key_paths, remove_path, string_values};
#[allow(unused_imports)]
pub use synth::SynthOptions;

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
