//! Guards the `Debug` redaction of secrets (D-064, D-067) where the sentinel
//! tests cannot reach: the sentinels format the types they list, so a new
//! type is covered only once someone adds it.
//!
//! - `src/redact.rs` holds the crate's one placeholder and its helpers. A
//!   string literal anywhere else in the library's non-test code that
//!   spells a placeholder (`"[REDACTED]"`, or any other that says
//!   "redacted") fails `the_placeholder_lives_in_redact_rs`: an impl prints
//!   `redact::REDACTED` or goes through a `redact` helper.
//! - A type that derives `Debug` over a field the `LOUD_WIRE` printer
//!   redacts (its `REDACT_FIELDS`, `REDACT_VALUES_BENEATH` and `ENV_FIELD`,
//!   read from `src/wire/mod.rs` and matched in snake case, by field name
//!   or `#[serde(rename)]`) fails `no_derived_debug_prints_a_secret_field`,
//!   unless `NOT_SECRET` names it with the reason its value is not one.
//!
//! Both read the library's own source through `tests/common/source_scan.rs`
//! (D-068): `src/` without its test-only files and items. Each scan tests
//! itself on a small source first.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    reason = "a test reports a failure by panicking"
)]

#[path = "common/source_scan.rs"]
mod source_scan;

use std::collections::BTreeSet;
use syn::visit::{self, Visit};

/// The one file that may spell the placeholder.
const PLACEHOLDER_HOME: &str = "src/redact.rs";

/// Derived `Debug` impls over a field the wire redacts, each with the
/// reason its value is not a secret. Keyed `path:Type.field` (a variant's
/// field is `path:Type::Variant.field`).
const NOT_SECRET: &[(&str, &str)] = &[];

// =============================================================================
// The placeholder
// =============================================================================

#[test]
fn the_placeholder_lives_in_redact_rs() {
    let sources = source_scan::library_sources();
    assert!(
        sources.iter().any(|(rel, _)| rel == PLACEHOLDER_HOME),
        "{PLACEHOLDER_HOME} is not among the library's sources"
    );
    let hits: Vec<String> = sources
        .iter()
        .filter(|(rel, _)| rel != PLACEHOLDER_HOME)
        .flat_map(|(rel, file)| placeholders(rel, file))
        .collect();
    assert!(
        hits.is_empty(),
        "a placeholder is spelled outside {PLACEHOLDER_HOME}; print \
         `redact::REDACTED` or go through a `redact` helper (D-067):\n  {}",
        hits.join("\n  ")
    );
}

/// Every string literal in `file` that spells a placeholder, as
/// `path: "literal"`: in expressions, patterns and consts, and in macro
/// arguments (`format!`, `json!`), but not in attributes, which hold the
/// doc comments that name the placeholder.
fn placeholders(rel: &str, file: &syn::File) -> Vec<String> {
    /// The string literals in a macro's tokens, plain or raw, paired from
    /// the left so a closing quote never opens one.
    static QUOTED: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#""(?:[^"\\]|\\.)*""#).unwrap());
    struct Scan<'a> {
        rel: &'a str,
        hits: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}
        fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
            let value = literal.value();
            if value.to_ascii_lowercase().contains("redacted") {
                self.hits.push(format!("{}: {value:?}", self.rel));
            }
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            let tokens = mac.tokens.to_string();
            for literal in QUOTED.find_iter(&tokens) {
                if literal.as_str().to_ascii_lowercase().contains("redacted") {
                    self.hits
                        .push(format!("{}: {}", self.rel, literal.as_str()));
                }
            }
            visit::visit_macro(self, mac);
        }
    }
    let mut scan = Scan {
        rel,
        hits: Vec::new(),
    };
    scan.visit_file(file);
    scan.hits
}

#[test]
fn the_placeholder_scan_detects_what_it_claims_to() {
    let source = r##"
        /// Prints `[REDACTED]` in place of the key.
        fn literal() -> &'static str { "[REDACTED]" }
        const SECOND: &str = "<redacted>";
        fn in_a_macro() -> String { format!("key: {}", "REDACTED") }
        fn raw() -> &'static str { r#"[Redacted]"# }
        fn fine() -> &'static str { crate::redact::REDACTED }
        #[cfg(test)]
        mod tests {
            fn test_only() -> &'static str { "[REDACTED]" }
        }
    "##;
    // `library_sources` strips the test-only items before the scan; do the
    // same with the one gate this source has.
    let mut file = syn::parse_file(source).unwrap();
    file.items
        .retain(|item| !source_scan::is_test_gated(source_scan::item_attrs(item)));
    let hits = placeholders("src/x.rs", &file);
    assert_eq!(hits.len(), 4, "{hits:#?}");
    for value in ["[REDACTED]", "<redacted>", "REDACTED", "[Redacted]"] {
        assert!(
            hits.iter().any(|hit| hit.ends_with(&format!("{value:?}"))),
            "{value}: {hits:#?}"
        );
    }
    // A placeholder in a macro's tokens, and a raw one.
    let file = syn::parse_file(
        r###"
        fn json() -> serde_json::Value { serde_json::json!({"key": "[REDACTED]"}) }
        fn raw() -> String { format!(r#"{}: [redacted]"#, 1) }
        fn named() -> String { format!("{}", crate::redact::REDACTED) }
        fn between() -> String { format!("{}", redacted_value, "x") }
        "###,
    )
    .unwrap();
    let hits = placeholders("src/x.rs", &file);
    assert_eq!(
        hits,
        [
            r#"src/x.rs: "[REDACTED]""#,
            r##"src/x.rs: "{}: [redacted]""##
        ],
        "{hits:#?}"
    );
}

// =============================================================================
// Derived Debug over a secret field
// =============================================================================

#[test]
fn no_derived_debug_prints_a_secret_field() {
    let sources = source_scan::library_sources();
    let names = secret_field_names(&sources);
    let found: BTreeSet<String> = sources
        .iter()
        .flat_map(|(rel, file)| derived_secret_fields(rel, file, &names))
        .collect();
    let allowed: BTreeSet<String> = NOT_SECRET.iter().map(|(key, _)| key.to_string()).collect();
    let unlisted: Vec<&String> = found.difference(&allowed).collect();
    assert!(
        unlisted.is_empty(),
        "a derived `Debug` prints a field the wire redacts; write the `Debug` by hand \
         through `crate::redact` and add the type to its sentinel test (D-067), or \
         name it in NOT_SECRET with the reason its value is not a secret:\n  {}",
        unlisted
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    let stale: Vec<&String> = allowed.difference(&found).collect();
    assert!(
        stale.is_empty(),
        "NOT_SECRET names fields that no derived `Debug` prints any more: {stale:?}"
    );
}

/// The field names the `LOUD_WIRE` printer redacts, in snake case: its
/// `REDACT_FIELDS` and `REDACT_VALUES_BENEATH` lists and its `ENV_FIELD`,
/// read from `src/wire/mod.rs`, so a name added there is guarded here.
fn secret_field_names(sources: &[(String, syn::File)]) -> BTreeSet<String> {
    let (_, wire) = sources
        .iter()
        .find(|(rel, _)| rel == "src/wire/mod.rs")
        .expect("src/wire/mod.rs is among the library's sources");
    let mut names = BTreeSet::new();
    for wanted in ["REDACT_FIELDS", "REDACT_VALUES_BENEATH", "ENV_FIELD"] {
        let value = wire
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Const(c) if c.ident == wanted => Some(&*c.expr),
                _ => None,
            })
            .unwrap_or_else(|| panic!("src/wire/mod.rs has no `const {wanted}`"));
        let before = names.len();
        collect_strings(value, &mut names);
        assert!(names.len() > before, "`{wanted}` holds no name");
    }
    names.into_iter().map(|name| snake_case(&name)).collect()
}

/// Every string literal in a const's value (`&["a", "b"]` or `"a"`).
fn collect_strings(expr: &syn::Expr, out: &mut BTreeSet<String>) {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) => {
            out.insert(text.value());
        }
        syn::Expr::Reference(reference) => collect_strings(&reference.expr, out),
        syn::Expr::Array(array) => {
            for element in &array.elems {
                collect_strings(element, out);
            }
        }
        _ => {}
    }
}

/// `apiKey` and `httpHeaders` as Rust spells the field: `api_key`,
/// `http_headers`.
fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The fields in `file` that a derived `Debug` prints and `names` holds,
/// keyed as `NOT_SECRET` is.
fn derived_secret_fields(rel: &str, file: &syn::File, names: &BTreeSet<String>) -> Vec<String> {
    struct Scan<'a> {
        rel: &'a str,
        names: &'a BTreeSet<String>,
        hits: Vec<String>,
    }
    impl Scan<'_> {
        fn fields(&mut self, owner: &str, fields: &syn::Fields) {
            for field in fields {
                let Some(ident) = &field.ident else { continue };
                let ident = ident.to_string();
                let ident = ident.trim_start_matches("r#");
                let renamed = serde_rename(&field.attrs).map(|name| snake_case(&name));
                if self.names.contains(ident)
                    || renamed.is_some_and(|name| self.names.contains(&name))
                {
                    self.hits.push(format!("{}:{owner}.{ident}", self.rel));
                }
            }
        }
    }
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
            if derives_debug(&item.attrs) {
                self.fields(&item.ident.to_string(), &item.fields);
            }
            visit::visit_item_struct(self, item);
        }
        fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
            if derives_debug(&item.attrs) {
                for variant in &item.variants {
                    self.fields(
                        &format!("{}::{}", item.ident, variant.ident),
                        &variant.fields,
                    );
                }
            }
            visit::visit_item_enum(self, item);
        }
    }
    let mut scan = Scan {
        rel,
        names,
        hits: Vec::new(),
    };
    scan.visit_file(file);
    scan.hits
}

/// Whether the attributes derive `Debug` (`derive(Debug)` or
/// `derive(std::fmt::Debug)`), alone or beside other derives.
fn derives_debug(attrs: &[syn::Attribute]) -> bool {
    use syn::punctuated::Punctuated;
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .parse_args_with(Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths
                        .iter()
                        .any(|path| path.segments.last().is_some_and(|s| s.ident == "Debug"))
                })
    })
}

/// A field's `#[serde(rename = "...")]`, if it has one.
fn serde_rename(attrs: &[syn::Attribute]) -> Option<String> {
    use syn::punctuated::Punctuated;
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("serde"))
        .filter_map(|attr| {
            attr.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                .ok()
        })
        .flatten()
        .find_map(|meta| match meta {
            syn::Meta::NameValue(nv) if nv.path.is_ident("rename") => match nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
}

#[test]
fn the_derived_debug_scan_detects_what_it_claims_to() {
    let names: BTreeSet<String> = ["api_key", "headers", "env", "http_headers"]
        .into_iter()
        .map(String::from)
        .collect();
    let source = r#"
        #[derive(Clone, Debug)]
        pub struct Endpoint { pub url: String, pub api_key: Option<String> }
        #[derive(std::fmt::Debug)]
        pub enum Transport { Http { url: String, headers: Vec<String> }, Stdio(String) }
        #[derive(Debug)]
        pub struct Renamed { #[serde(rename = "httpHeaders")] pub extra_headers: Vec<String> }
        #[derive(Debug)]
        pub struct Raw { pub r#env: Vec<String> }
        pub struct HandWritten { pub api_key: String }
        #[derive(Clone)]
        pub struct NoDebug { pub headers: Vec<String> }
        #[derive(Debug)]
        pub struct Plain { pub name: String, pub page_token: String }
    "#;
    let file = syn::parse_file(source).unwrap();
    assert_eq!(
        derived_secret_fields("src/x.rs", &file, &names),
        [
            "src/x.rs:Endpoint.api_key",
            "src/x.rs:Transport::Http.headers",
            "src/x.rs:Renamed.extra_headers",
            "src/x.rs:Raw.env",
        ]
    );
    assert_eq!(snake_case("httpHeaders"), "http_headers");
    assert_eq!(snake_case("api_key"), "api_key");
}

#[test]
fn the_wire_lists_are_read() {
    let names = secret_field_names(&source_scan::library_sources());
    for name in ["api_key", "headers", "transform", "env", "http_headers"] {
        assert!(names.contains(name), "{name}: {names:?}");
    }
}
