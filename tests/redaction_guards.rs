//! Guards the `Debug` redaction of secrets (D-064, D-067) where the sentinel
//! tests cannot reach: the sentinels format the types they list, so a new
//! type is covered only once someone adds it.
//!
//! - `src/redact.rs` holds the crate's one placeholder and its helpers. A
//!   string literal anywhere else in the library's non-test code that
//!   spells a placeholder (`"[REDACTED]"`, or any other that says
//!   "redacted"), in an expression, a macro's arguments or an attribute
//!   (`#[error("..")]`) but not a doc comment, fails
//!   `the_placeholder_lives_in_redact_rs`: an impl prints
//!   `redact::REDACTED` or goes through a `redact` helper.
//! - A type that derives `Debug` over a field the `LOUD_WIRE` printer
//!   redacts (its `REDACT_FIELDS`, `REDACT_VALUES_BENEATH` and `ENV_FIELD`,
//!   read from `src/wire/mod.rs` and matched in snake case, by field name
//!   or `#[serde(rename)]`) fails `no_derived_debug_prints_a_secret_field`,
//!   unless `NOT_SECRET` names it with the reason its value is not one.
//!   The wire's `CONTEXT_REDACT_FIELD` (`value`) is not read: the wire
//!   redacts it only beside `"type": "environment_variable"` or beneath an
//!   `env`, and a field so named anywhere else is no secret. So a type that
//!   holds an environment variable's value as a `value` is not found by
//!   itself; one that derives `Debug` over the `env` holding it is.
//! - A hand-written `Debug` that redacts (its `fmt` names `redact::..` or
//!   `REDACTED`), or that is for a type holding such a type at any depth
//!   (`Client`, which prints its key through `HttpContext`'s), destructures
//!   its type: every pattern on `Self` or the type, a struct's or a
//!   variant's, names each field (`field: _` for one it does not print),
//!   with no `..` rest; a match on `self` (or `*self`, `&self`) has no
//!   catch-all arm (`_`, a binding such as `other`, or an or-pattern
//!   holding one); there is no `if let` or `let .. else` on `self`; and no
//!   field is read through `self`. Else
//!   `a_redacting_debug_destructures_its_type` fails, unless
//!   `NOT_DESTRUCTURED` names the type with the reason. So a field or a
//!   variant added to such a type fails to compile until its `Debug` prints
//!   or redacts it. Patterns inside a macro's arguments are not read, and
//!   omission is the safe direction: this is the convention D-067 states,
//!   not a leak check.
//!
//! All three read the library's own source through
//! `tests/common/source_scan.rs` (D-068): `src/` without its test-only
//! files and items. Each scan tests itself on a small source first.

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

use std::collections::{BTreeMap, BTreeSet};
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
/// `path: "literal"`: in expressions, patterns and consts, in macro
/// arguments (`format!`, `json!`) and in attributes (`#[error("..")]`, a
/// lint's `reason`), but not in doc comments, which name the placeholder.
fn placeholders(rel: &str, file: &syn::File) -> Vec<String> {
    /// The string literals in a macro's or an attribute's tokens, plain or
    /// raw, paired from the left so a closing quote never opens one.
    static QUOTED: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#""(?:[^"\\]|\\.)*""#).unwrap());
    struct Scan<'a> {
        rel: &'a str,
        hits: Vec<String>,
    }
    impl Scan<'_> {
        fn tokens(&mut self, tokens: &str) {
            for literal in QUOTED.find_iter(tokens) {
                if literal.as_str().to_ascii_lowercase().contains("redacted") {
                    self.hits
                        .push(format!("{}: {}", self.rel, literal.as_str()));
                }
            }
        }
    }
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
            match &attr.meta {
                syn::Meta::NameValue(nv) if nv.path.is_ident("doc") => {}
                syn::Meta::List(list) => self.tokens(&list.tokens.to_string()),
                _ => visit::visit_attribute(self, attr),
            }
        }
        fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
            let value = literal.value();
            if value.to_ascii_lowercase().contains("redacted") {
                self.hits.push(format!("{}: {value:?}", self.rel));
            }
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            self.tokens(&mac.tokens.to_string());
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
        #[derive(Debug, thiserror::Error)]
        #[error("harness key: [REDACTED]")]
        struct InAnAttribute;
        #[must_use = "a placeholder: <REDACTED>"]
        fn in_a_name_value() {}
        #[doc = "Prints [REDACTED]."]
        #[doc(hidden)]
        fn documented() {}
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
    assert_eq!(hits.len(), 6, "{hits:#?}");
    for value in [
        "[REDACTED]",
        "<redacted>",
        "REDACTED",
        "[Redacted]",
        "harness key: [REDACTED]",
        "a placeholder: <REDACTED>",
    ] {
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

// =============================================================================
// A redacting Debug destructures its type
// =============================================================================

/// Hand-written `Debug` impls the destructure check reads that do not
/// destructure, by type, each with the reason. One that destructures, or
/// is no longer read, fails the check until it leaves the list.
const NOT_DESTRUCTURED: &[(&str, &str)] = &[(
    "InteractionBuilder",
    "redacts nothing itself: it prints its tools and environment, whose `Debug` redacts, \
     through `self`. How its own `extra` prints is the owner's open call, and its `Debug` \
     is rewritten with it",
)];

#[test]
fn a_redacting_debug_destructures_its_type() {
    let sources = source_scan::library_sources();
    let read = read_debugs(&sources);
    let redacting = read.iter().filter(|debug| debug.redacts).count();
    // Twenty-five impls redact today; fewer than twenty means the scan
    // stopped finding them.
    assert!(
        redacting >= 20,
        "found only {redacting} hand-written Debug impls that redact"
    );
    // `Client` prints its key through `HttpContext`'s `Debug`, naming no
    // placeholder itself: the check reads it as one that delegates.
    assert!(
        read.iter()
            .any(|debug| debug.ty == "Client" && !debug.redacts),
        "the destructure check no longer reads Client's Debug, which redacts through \
         HttpContext's"
    );
    let mut problems = Vec::new();
    for debug in &read {
        let exempt = NOT_DESTRUCTURED.iter().any(|(ty, _)| *ty == debug.ty);
        if !exempt {
            problems.extend(
                debug
                    .problems
                    .iter()
                    .map(|problem| format!("{}: {}: {problem}", debug.rel, debug.ty)),
            );
        }
    }
    assert!(
        problems.is_empty(),
        "a hand-written `Debug` that redacts, or prints a type whose `Debug` does, reads \
         its type without destructuring every field and variant, so one added later \
         prints nothing rather than failing to compile; destructure with \
         `let Self {{ .. }} = self` (or each arm's pattern, with no catch-all arm) naming \
         every field, `field: _` for one it does not print (D-067):\n  {}",
        problems.join("\n  ")
    );
    let stale: Vec<&str> = NOT_DESTRUCTURED
        .iter()
        .map(|(ty, _)| *ty)
        .filter(|ty| {
            !read
                .iter()
                .any(|debug| debug.ty == *ty && !debug.problems.is_empty())
        })
        .collect();
    assert!(
        stale.is_empty(),
        "NOT_DESTRUCTURED names a type the check no longer reads, or one that \
         destructures now: {stale:?}"
    );
}

/// A hand-written `Debug`, as the destructure check reads it.
#[derive(Debug, PartialEq)]
struct ReadDebug {
    rel: String,
    ty: String,
    /// Whether its `fmt` names `redact::..` or `REDACTED` itself.
    redacts: bool,
    problems: Vec<String>,
}

/// Each hand-written `Debug` in `sources` the destructure check reads: one
/// whose `fmt` redacts, or one for a type that holds, at any depth, a type
/// whose `Debug` redacts ([`carriers`]), as `Client` holds `HttpContext`.
/// Each comes with what keeps it from destructuring its type: a pattern on
/// `Self` or the type (a struct, or a variant) with a `..` rest; a
/// catch-all arm (`_`, a binding such as `other`, or an or-pattern holding
/// one) in a match on `self`, `*self` or `&self`; an `if let`, `while let`
/// or `let .. else` on `self`; a field read through `self`; or no pattern
/// on the type at all. Patterns inside a macro's arguments are not read.
fn read_debugs(sources: &[(String, syn::File)]) -> Vec<ReadDebug> {
    let all: Vec<ReadDebug> = sources
        .iter()
        .flat_map(|(rel, file)| hand_written_debugs(rel, file))
        .collect();
    let direct = all
        .iter()
        .filter(|debug| debug.redacts)
        .map(|debug| debug.ty.clone())
        .collect();
    let carriers = carriers(direct, &field_types(sources));
    all.into_iter()
        .filter(|debug| carriers.contains(&debug.ty))
        .collect()
}

/// Every struct and enum in `sources`, by name, with the names its fields'
/// types mention (`Arc<HttpContext>` mentions `Arc` and `HttpContext`).
/// Types are told apart by name alone, so two of one name share their
/// fields, which reads more impls, not fewer.
fn field_types(sources: &[(String, syn::File)]) -> BTreeMap<String, BTreeSet<String>> {
    #[derive(Default)]
    struct Names(BTreeSet<String>);
    impl<'ast> Visit<'ast> for Names {
        fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
            self.0.insert(segment.ident.to_string());
            visit::visit_path_segment(self, segment);
        }
    }
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (_, file) in sources {
        for item in source_scan::all_items(std::slice::from_ref(file)) {
            let (name, fields): (_, Vec<&syn::Fields>) = match item {
                syn::Item::Struct(item) => (item.ident.to_string(), vec![&item.fields]),
                syn::Item::Enum(item) => (
                    item.ident.to_string(),
                    item.variants.iter().map(|v| &v.fields).collect(),
                ),
                _ => continue,
            };
            let mut names = Names::default();
            for field in fields.into_iter().flatten() {
                names.visit_type(&field.ty);
            }
            out.entry(name).or_default().extend(names.0);
        }
    }
    out
}

/// `found`, and every type in `fields` that holds one of them, to a fixed
/// point.
fn carriers(
    mut found: BTreeSet<String>,
    fields: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    loop {
        let more: Vec<String> = fields
            .iter()
            .filter(|(ty, names)| !found.contains(*ty) && names.iter().any(|n| found.contains(n)))
            .map(|(ty, _)| ty.clone())
            .collect();
        if more.is_empty() {
            return found;
        }
        found.extend(more);
    }
}

/// Whether `expr` is `self`, through `*`, `&` and parentheses.
fn is_self(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(path) => path.path.is_ident("self"),
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => is_self(&unary.expr),
        syn::Expr::Reference(reference) => is_self(&reference.expr),
        syn::Expr::Paren(paren) => is_self(&paren.expr),
        syn::Expr::Group(group) => is_self(&group.expr),
        _ => false,
    }
}

/// Whether an arm's pattern takes whatever the arms before it left: `_`, a
/// binding (`other`, `_rest`, `ref x`, `x @ _`), or an or-pattern, a
/// reference or parentheses holding one. A capitalized bare name is a
/// unit variant or a constant, not a binding.
fn catches_all(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Wild(_) => true,
        syn::Pat::Ident(binding) => match &binding.subpat {
            Some((_, sub)) => catches_all(sub),
            None => binding
                .ident
                .to_string()
                .starts_with(|c: char| c == '_' || c.is_lowercase()),
        },
        syn::Pat::Or(or) => or.cases.iter().any(catches_all),
        syn::Pat::Paren(paren) => catches_all(&paren.pat),
        syn::Pat::Reference(reference) => catches_all(&reference.pat),
        _ => false,
    }
}

/// Every hand-written `Debug` in `file`, as [`read_debugs`] describes it,
/// whether it redacts or not.
fn hand_written_debugs(rel: &str, file: &syn::File) -> Vec<ReadDebug> {
    #[derive(Default)]
    struct Body<'a> {
        ty: &'a str,
        redacts: bool,
        destructures: usize,
        problems: Vec<String>,
    }
    impl Body<'_> {
        /// Whether a pattern's path names the type: `Self`, `Self::Variant`,
        /// `Type` or `Type::Variant`.
        fn names_the_type(&self, path: &syn::Path) -> bool {
            path.segments
                .first()
                .is_some_and(|first| first.ident == "Self" || first.ident == self.ty)
        }
        fn pattern(&mut self, path: &syn::Path, rest: bool) {
            if self.names_the_type(path) {
                self.destructures += 1;
                if rest {
                    let path = quote_path(path);
                    self.problems
                        .push(format!("`{path}` leaves fields out with `..`"));
                }
            }
        }
    }
    impl<'ast> Visit<'ast> for Body<'_> {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if path
                .segments
                .iter()
                .any(|s| s.ident == "redact" || s.ident == "REDACTED")
            {
                self.redacts = true;
            }
            visit::visit_path(self, path);
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            let tokens = mac.tokens.to_string();
            if tokens.contains("redact ::") || tokens.contains("REDACTED") {
                self.redacts = true;
            }
            visit::visit_macro(self, mac);
        }
        fn visit_pat_struct(&mut self, pat: &'ast syn::PatStruct) {
            self.pattern(&pat.path, pat.rest.is_some());
            visit::visit_pat_struct(self, pat);
        }
        fn visit_pat_tuple_struct(&mut self, pat: &'ast syn::PatTupleStruct) {
            let rest = pat.elems.iter().any(|p| matches!(p, syn::Pat::Rest(_)));
            self.pattern(&pat.path, rest);
            visit::visit_pat_tuple_struct(self, pat);
        }
        fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
            if is_self(&node.expr) && node.arms.iter().any(|arm| catches_all(&arm.pat)) {
                self.problems
                    .push("a catch-all arm lets a new variant compile unprinted".to_string());
            }
            visit::visit_expr_match(self, node);
        }
        fn visit_expr_let(&mut self, node: &'ast syn::ExprLet) {
            if is_self(&node.expr) {
                self.problems
                    .push("an `if let` on `self` lets a new variant compile unprinted".to_string());
            }
            visit::visit_expr_let(self, node);
        }
        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let Some(init) = &local.init
                && init.diverge.is_some()
                && is_self(&init.expr)
            {
                self.problems.push(
                    "a `let .. else` on `self` lets a new variant compile unprinted".to_string(),
                );
            }
            visit::visit_local(self, local);
        }
        fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
            if is_self(&field.base) {
                let member = match &field.member {
                    syn::Member::Named(name) => name.to_string(),
                    syn::Member::Unnamed(index) => index.index.to_string(),
                };
                self.problems
                    .push(format!("reads `self.{member}` rather than destructuring"));
            }
            visit::visit_expr_field(self, field);
        }
    }
    fn quote_path(path: &syn::Path) -> String {
        path.segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::")
    }
    let mut out = Vec::new();
    for item in source_scan::all_items(std::slice::from_ref(file)) {
        let syn::Item::Impl(imp) = item else { continue };
        let is_debug = imp
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_some_and(|s| s.ident == "Debug");
        let syn::Type::Path(self_ty) = &*imp.self_ty else {
            continue;
        };
        let Some(ty) = self_ty.path.segments.last().map(|s| s.ident.to_string()) else {
            continue;
        };
        if !is_debug {
            continue;
        }
        for item in &imp.items {
            let syn::ImplItem::Fn(fmt) = item else {
                continue;
            };
            let mut body = Body {
                ty: &ty,
                ..Body::default()
            };
            body.visit_block(&fmt.block);
            if body.destructures == 0 {
                body.problems
                    .push("destructures no `Self` or variant pattern".to_string());
            }
            out.push(ReadDebug {
                rel: rel.to_string(),
                ty: ty.clone(),
                redacts: body.redacts,
                problems: body.problems,
            });
        }
    }
    out
}

/// The destructure check flags a `..` rest on a struct or a variant, a
/// tuple variant's `..`, a catch-all arm in a match on `self` (`_`, a
/// binding, an or-pattern holding one, through `*self` or `&self`), an
/// `if let` or `let .. else` on `self`, a field read through `self` and an
/// impl that destructures nothing. It reads an impl that redacts and one
/// for a type holding such a type at any depth (`Outer` through a derived
/// `Middle`), and no other.
#[test]
fn the_destructure_check_detects_what_it_claims_to() {
    let source = r#"
        impl std::fmt::Debug for Good {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let Self { name, secret: _ } = self;
                f.debug_struct("Good").field("name", name).field("secret", &redact::REDACTED).finish()
            }
        }
        impl std::fmt::Debug for Rest {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let Self { headers, .. } = self;
                f.debug_struct("Rest").field("headers", &redact::values(headers)).finish()
            }
        }
        impl fmt::Debug for Fields {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Fields").field("name", &self.name).field("key", &REDACTED).finish()
            }
        }
        impl Debug for Choice {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match self {
                    Self::Key { key: _, .. } => write!(f, "{}", REDACTED),
                    Choice::Pair(name, ..) => f.debug_tuple("Pair").field(name).finish(),
                    Self::Plain(text) => f.debug_tuple("Plain").field(text).finish(),
                    Self::Unit | Other => f.write_str("Unit"),
                }
            }
        }
        impl Debug for Wild {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match *self {
                    Self::Key { key: _ } => write!(f, "{}", REDACTED),
                    _ => f.write_str("Wild"),
                }
            }
        }
        impl Debug for BindArm {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match self {
                    Self::Key { key: _ } => write!(f, "{}", REDACTED),
                    other => f.write_str("BindArm"),
                }
            }
        }
        impl Debug for RefWild {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                match &self {
                    Self::Key { key: _ } => write!(f, "{}", REDACTED),
                    Self::Plain(_) | _ => f.write_str("RefWild"),
                }
            }
        }
        impl Debug for IfLet {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                if let Self::Key { key: _ } = self {
                    write!(f, "{}", REDACTED)
                } else {
                    f.write_str("IfLet")
                }
            }
        }
        impl Debug for LetElse {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let Self::Key { key: _ } = self else {
                    return f.write_str("LetElse");
                };
                write!(f, "{}", REDACTED)
            }
        }
        impl Debug for InMacro {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                write!(f, "InMacro({})", crate::redact::REDACTED)
            }
        }
        impl Debug for NoSecret {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let Self { name, .. } = self;
                f.debug_struct("NoSecret").field("name", &self.name).finish()
            }
        }
        impl Display for NotDebug {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                write!(f, "{}", self.key.map(|_| REDACTED).unwrap_or_default())
            }
        }
        struct Good { name: String, secret: String }
        #[derive(Debug)]
        struct Middle { inner: Option<Arc<Good>> }
        struct Outer { middle: Vec<Middle>, name: String }
        impl Debug for Outer {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let Self { middle, .. } = self;
                f.debug_struct("Outer").field("middle", middle).finish()
            }
        }
        struct Delegates { good: Good }
        impl Debug for Delegates {
            fn fmt(&self, f: &mut Formatter<'_>) -> Result {
                let Self { good } = self;
                f.debug_struct("Delegates").field("good", good).finish()
            }
        }
    "#;
    let sources = [("src/probe.rs".to_string(), syn::parse_file(source).unwrap())];
    let read = read_debugs(&sources);
    let found: Vec<(&str, bool, Vec<&str>)> = read
        .iter()
        .map(|debug| {
            assert_eq!(debug.rel, "src/probe.rs");
            (
                debug.ty.as_str(),
                debug.redacts,
                debug.problems.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let catch_all = "a catch-all arm lets a new variant compile unprinted";
    assert_eq!(
        found,
        [
            ("Good", true, vec![]),
            ("Rest", true, vec!["`Self` leaves fields out with `..`"]),
            (
                "Fields",
                true,
                vec![
                    "reads `self.name` rather than destructuring",
                    "destructures no `Self` or variant pattern",
                ]
            ),
            (
                "Choice",
                true,
                vec![
                    "`Self::Key` leaves fields out with `..`",
                    "`Choice::Pair` leaves fields out with `..`",
                ]
            ),
            ("Wild", true, vec![catch_all]),
            ("BindArm", true, vec![catch_all]),
            ("RefWild", true, vec![catch_all]),
            (
                "IfLet",
                true,
                vec!["an `if let` on `self` lets a new variant compile unprinted"]
            ),
            (
                "LetElse",
                true,
                vec!["a `let .. else` on `self` lets a new variant compile unprinted"]
            ),
            (
                "InMacro",
                true,
                vec!["destructures no `Self` or variant pattern"]
            ),
            ("Outer", false, vec!["`Self` leaves fields out with `..`"]),
            ("Delegates", false, vec![]),
        ]
    );
}
