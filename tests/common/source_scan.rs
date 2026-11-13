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

/// Every file directly in `dir`, or at any depth when `deep`, whose
/// extension is one of `extensions`, in path order. Fails as
/// [`rust_files`] does on a scan root that does not exist or a directory
/// that cannot be read.
pub fn files_with_extensions(dir: &Path, extensions: &[&str], deep: bool) -> Vec<PathBuf> {
    fn walk(dir: &Path, extensions: &[&str], deep: bool, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|e| panic!("could not read an entry of {}: {e}", dir.display()))
                .path();
            if path.is_dir() {
                if deep {
                    walk(&path, extensions, deep, out);
                }
            } else if path
                .extension()
                .is_some_and(|ext| extensions.iter().any(|want| ext == *want))
            {
                out.push(path);
            }
        }
    }
    assert!(dir.is_dir(), "scan root {} does not exist", dir.display());
    let mut out = Vec::new();
    walk(dir, extensions, deep, &mut out);
    out.sort();
    out
}
