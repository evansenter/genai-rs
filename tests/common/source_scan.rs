//! The source scans the guards share (D-068): one recursive `.rs`
//! walker, and one discovery of the resource handles (D-016) and their
//! public methods.
//!
//! `tests/module_size.rs`, `tests/non_exhaustive_responses.rs` and
//! `tests/ci_coverage.rs` walk the tree with [`rust_files`];
//! `tests/live_verbs/` (for `ci_coverage` and `request_wire_oracle`) and
//! `tests/http_mock_resources/paging.rs` find the handles with [`structs`],
//! [`handles`] and [`public_methods`]. Each keeps its own floor, so a scan
//! that finds nothing fails there.
//!
//! Not part of `common/mod.rs`, which every integration test compiles: the
//! guards that scan include this file by `#[path]`, and nothing else
//! compiles it.
//!
//! `tests/model_literals.rs` and `tests/temp_file_tests.rs` walk with
//! [`rust_files`] too (#553). `tests/redaction_guards.rs`,
//! `tests/setter_convention.rs` and the coverage oracle
//! (`src/coverage_oracle/request/fills.rs`) read the library's own source
//! with [`library_sources`]; each keeps its own floor as well.

#![allow(
    dead_code,
    reason = "each guard that includes this file uses a part of it"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The attribute that marks a resource handle (`docs/BUILDER_API.md`,
/// "Resource handles and list builders").
pub const HANDLE_MARKER: &str = "a resource handle does nothing until you call one of its methods";

/// The repository root (the root crate is the workspace root).
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `path` relative to the repository root, with `/` separators on every OS.
pub fn repo_relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Every `.rs` file under `dir`, at any depth, in path order. A scan root
/// that does not exist, or a directory that cannot be read, fails: a tree
/// that is gone must not read as one with nothing in it.
pub fn rust_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|e| panic!("could not read an entry of {}: {e}", dir.display()))
                .path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    assert!(dir.is_dir(), "scan root {} does not exist", dir.display());
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

/// The source file at `path`, parsed.
pub fn parse_file(path: &Path) -> syn::File {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    syn::parse_file(&text).unwrap_or_else(|e| panic!("could not parse {}: {e}", path.display()))
}

/// Every item in `files`, those of inline modules included (after the
/// module's own item).
pub fn all_items(files: &[syn::File]) -> Vec<&syn::Item> {
    fn walk<'a>(items: &'a [syn::Item], out: &mut Vec<&'a syn::Item>) {
        for item in items {
            out.push(item);
            if let syn::Item::Mod(module) = item
                && let Some((_, inner)) = &module.content
            {
                walk(inner, out);
            }
        }
    }
    let mut out = Vec::new();
    for file in files {
        walk(&file.items, &mut out);
    }
    out
}

/// Whether the attributes include `#[must_use = "<HANDLE_MARKER>"]`.
pub fn carries_handle_marker(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(&attr.meta, syn::Meta::NameValue(nv)
            if nv.path.is_ident("must_use")
                && matches!(&nv.value, syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text), ..
                }) if text.value() == HANDLE_MARKER))
    })
}

/// Every struct in `items`, by name, and whether it is a resource handle:
/// one that carries [`HANDLE_MARKER`].
pub fn structs(items: &[&syn::Item]) -> BTreeMap<String, bool> {
    items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(s) => Some((s.ident.to_string(), carries_handle_marker(&s.attrs))),
            _ => None,
        })
        .collect()
}

/// The resource handles in `items`: the structs that carry
/// [`HANDLE_MARKER`].
pub fn handles(items: &[&syn::Item]) -> BTreeSet<String> {
    structs(items)
        .into_iter()
        .filter(|(_, handle)| *handle)
        .map(|(name, _)| name)
        .collect()
}

/// Every type's public methods, associated functions included, from its
/// inherent `impl` blocks wherever they live, keyed by the type's name (the
/// last segment of its path), in declaration order.
pub fn public_methods<'a>(items: &[&'a syn::Item]) -> BTreeMap<String, Vec<&'a syn::ImplItemFn>> {
    let mut methods: BTreeMap<String, Vec<&syn::ImplItemFn>> = BTreeMap::new();
    for item in items {
        let syn::Item::Impl(imp) = item else { continue };
        if imp.trait_.is_some() {
            continue;
        }
        let Some(owner) = type_name(&imp.self_ty) else {
            continue;
        };
        for item in &imp.items {
            if let syn::ImplItem::Fn(f) = item
                && matches!(f.vis, syn::Visibility::Public(_))
            {
                methods.entry(owner.clone()).or_default().push(f);
            }
        }
    }
    methods
}

/// A path type's last segment, through a reference (`&Client` is
/// `Client`).
fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => type_name(&r.elem),
        _ => None,
    }
}

/// Whether the attributes gate an item to test builds: a `cfg` in which
/// `test` appears outside any `not(..)` (`all(test, unix)` and
/// `any(test, fuzzing)` count), or a test attribute (`#[test]`,
/// `#[tokio::test]`).
pub fn is_test_gated(attrs: &[syn::Attribute]) -> bool {
    use syn::punctuated::Punctuated;
    fn mentions_test(meta: &syn::Meta, negated: bool) -> bool {
        match meta {
            syn::Meta::Path(path) => !negated && path.is_ident("test"),
            syn::Meta::List(list) => {
                let negated = negated ^ list.path.is_ident("not");
                list.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                    .is_ok_and(|nested| nested.iter().any(|m| mentions_test(m, negated)))
            }
            syn::Meta::NameValue(_) => false,
        }
    }
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "test")
            || (attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Meta>()
                    .is_ok_and(|meta| mentions_test(&meta, false)))
    })
}

/// Whether a repo-relative path is one of the library's test-only files:
/// one `Cargo.toml`'s `include` leaves out of `src/` (`!/src/...`), since
/// those build only under `cfg(test)` and the `check` job fails when one
/// ships (#501). Read from `Cargo.toml`, so a pattern added there counts
/// here too.
pub fn is_test_only_file(rel: &str) -> bool {
    static PATTERNS: std::sync::LazyLock<Vec<regex::Regex>> = std::sync::LazyLock::new(|| {
        let manifest = std::fs::read_to_string(root().join("Cargo.toml"))
            .unwrap_or_else(|e| panic!("could not read Cargo.toml: {e}"));
        let include = manifest
            .split_once("\ninclude = [")
            .and_then(|(_, rest)| rest.split_once("\n]"))
            .map(|(list, _)| list)
            .expect("Cargo.toml has an `include = [` list");
        let patterns: Vec<regex::Regex> = include
            .lines()
            .filter_map(|line| line.trim().strip_prefix("\"!/src/"))
            .map(|glob| {
                let glob = glob.trim_end_matches(',').trim_end_matches('"');
                let mut pattern = String::from("^src/");
                let mut rest = glob;
                while !rest.is_empty() {
                    if let Some(after) = rest.strip_prefix("**/") {
                        pattern.push_str("(?:.*/)?");
                        rest = after;
                    } else if let Some(after) = rest.strip_prefix("**") {
                        pattern.push_str(".*");
                        rest = after;
                    } else if let Some(after) = rest.strip_prefix('*') {
                        pattern.push_str("[^/]*");
                        rest = after;
                    } else {
                        let c = rest.chars().next().expect("not empty");
                        pattern.push_str(&regex::escape(&c.to_string()));
                        rest = &rest[c.len_utf8()..];
                    }
                }
                pattern.push('$');
                regex::Regex::new(&pattern).expect("a glob makes a valid pattern")
            })
            .collect();
        assert!(
            !patterns.is_empty(),
            "Cargo.toml's `include` leaves nothing out of src/: the test-only files \
             cannot be told from the library's"
        );
        patterns
    });
    PATTERNS.iter().any(|pattern| pattern.is_match(rel))
}

/// The library's own source, parsed and keyed by repo-relative path: every
/// `.rs` file under `src/` but the test-only ones ([`is_test_only_file`]),
/// with its test-only items removed ([`is_test_gated`]) at any depth of
/// inline modules, and from `impl` and `trait` blocks.
pub fn library_sources() -> Vec<(String, syn::File)> {
    fn strip(items: &mut Vec<syn::Item>) {
        items.retain(|item| !is_test_gated(item_attrs(item)));
        for item in items {
            match item {
                syn::Item::Mod(module) => {
                    if let Some((_, inner)) = &mut module.content {
                        strip(inner);
                    }
                }
                syn::Item::Impl(imp) => imp.items.retain(|item| match item {
                    syn::ImplItem::Fn(f) => !is_test_gated(&f.attrs),
                    syn::ImplItem::Const(c) => !is_test_gated(&c.attrs),
                    syn::ImplItem::Type(t) => !is_test_gated(&t.attrs),
                    _ => true,
                }),
                syn::Item::Trait(tr) => tr.items.retain(|item| match item {
                    syn::TraitItem::Fn(f) => !is_test_gated(&f.attrs),
                    _ => true,
                }),
                _ => {}
            }
        }
    }
    let sources: Vec<(String, syn::File)> = rust_files(&root().join("src"))
        .iter()
        .map(|path| (repo_relative(path), path))
        .filter(|(rel, _)| !is_test_only_file(rel))
        .map(|(rel, path)| {
            let mut file = parse_file(path);
            strip(&mut file.items);
            (rel, file)
        })
        .collect();
    assert!(
        sources.iter().any(|(rel, _)| rel == "src/lib.rs"),
        "the library's sources leave out src/lib.rs"
    );
    sources
}

/// An item's attributes, for the kinds that carry a `cfg` worth reading.
pub fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}
