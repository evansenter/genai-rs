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
//! with [`library_sources`]; each keeps its own floor as well. It and
//! `tests/non_exhaustive_responses.rs` tell test-only code from the
//! library's with [`is_test_gated`] and [`test_modules`].

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

/// Whether the attributes gate an item to test builds: a test attribute
/// (`#[test]`, `#[tokio::test]`), or a `cfg` that cannot hold unless `test`
/// is set, whatever every other option is (`test`, `all(test, unix)`,
/// `any(test, all(test, x))`). One that can hold without it builds outside
/// tests, so it does not count: `any(target_os = "linux", test)` (a Linux
/// release), `any(test, fuzzing)`, `not(test)`.
pub fn is_test_gated(attrs: &[syn::Attribute]) -> bool {
    use syn::punctuated::Punctuated;
    /// Whether `meta` can hold, and whether it can fail, in a build
    /// without `test` where every other option may be either way. A
    /// predicate this cannot read can do both.
    fn without_test(meta: &syn::Meta) -> (bool, bool) {
        match meta {
            syn::Meta::Path(path) if path.is_ident("test") => (false, true),
            syn::Meta::List(list) => {
                let Ok(nested) =
                    list.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                else {
                    return (true, true);
                };
                let parts: Vec<(bool, bool)> = nested.iter().map(without_test).collect();
                if list.path.is_ident("not") {
                    match parts.as_slice() {
                        [(hold, fail)] => (*fail, *hold),
                        _ => (true, true),
                    }
                } else if list.path.is_ident("all") {
                    (
                        parts.iter().all(|(hold, _)| *hold),
                        parts.iter().any(|(_, fail)| *fail),
                    )
                } else if list.path.is_ident("any") {
                    (
                        parts.iter().any(|(hold, _)| *hold),
                        parts.iter().all(|(_, fail)| *fail),
                    )
                } else {
                    (true, true)
                }
            }
            _ => (true, true),
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
                    .is_ok_and(|meta| !without_test(&meta).0))
    })
}

/// The out-of-line modules `file`, at repo-relative `rel`, declares, each
/// as an extension-less repo path and whether its declaration is
/// test-gated ([`is_test_gated`]): `mod x;` names `x` in
/// [`child_module_dir`], and one with a `#[path = "x_tests.rs"]` names that
/// file, relative to the declaring file's own directory. Inline modules
/// are not followed: an out-of-line `mod` inside one would resolve to a
/// nested directory, and `src/` has none.
pub fn declared_modules(rel: &str, file: &syn::File) -> Vec<(String, bool)> {
    let dir = child_module_dir(rel);
    let file_dir = Path::new(rel)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if module.content.is_none() => {
                let path = match path_attr(&module.attrs) {
                    Some(path) => {
                        let path = path.strip_suffix(".rs").unwrap_or(&path).to_owned();
                        format!("{file_dir}/{path}")
                    }
                    None => format!("{dir}/{}", module.ident),
                };
                Some((path, is_test_gated(&module.attrs)))
            }
            _ => None,
        })
        .collect()
}

/// The test-only modules among `files` (repo-relative path and parsed
/// source), as [`declared_modules`] names them: each a test-gated `mod`
/// declares, and each module a test-only file declares, gated or not
/// (`proptest_tests.rs`'s `#[path = "proptest_content.rs"] mod content;`).
pub fn test_modules<'a, I>(files: I) -> BTreeSet<String>
where
    I: IntoIterator<Item = (&'a str, &'a syn::File)>,
{
    let declared: Vec<(&str, Vec<(String, bool)>)> = files
        .into_iter()
        .map(|(rel, file)| (rel, declared_modules(rel, file)))
        .collect();
    let mut found: BTreeSet<String> = declared
        .iter()
        .flat_map(|(_, modules)| modules.iter())
        .filter(|(_, gated)| *gated)
        .map(|(module, _)| module.clone())
        .collect();
    loop {
        let before = found.len();
        for (rel, modules) in &declared {
            if is_test_only_path(rel, &found) {
                found.extend(modules.iter().map(|(module, _)| module.clone()));
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

/// Whether `rel` is a test-only file: the file of a module `m` in
/// `test_modules` ([`test_modules`]), as `<m>.rs` or `<m>/mod.rs`, or any
/// file below `<m>/`.
pub fn is_test_only_path(rel: &str, test_modules: &BTreeSet<String>) -> bool {
    test_modules
        .iter()
        .any(|m| rel == format!("{m}.rs") || rel.starts_with(&format!("{m}/")))
}

/// Where the children of `rel`'s module live: `src/x.rs` declares into
/// `src/x/`, while `lib.rs`, `main.rs` and `mod.rs` declare into their own
/// directory.
pub fn child_module_dir(rel: &str) -> String {
    let path = Path::new(rel);
    let stem = path
        .file_stem()
        .expect("a source file has a stem")
        .to_string_lossy();
    let parent = path
        .parent()
        .expect("a source file has a directory")
        .to_string_lossy();
    if stem == "lib" || stem == "mod" || stem == "main" {
        parent.into_owned()
    } else {
        format!("{parent}/{stem}")
    }
}

/// The value of a `#[path = "..."]` attribute, if present.
fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        syn::Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// The library's own source, parsed and keyed by repo-relative path: every
/// `.rs` file under `src/` but the test-only ones ([`test_modules`],
/// [`is_test_only_path`]), with its test-only items removed
/// ([`is_test_gated`]) at any depth of inline modules, and from `impl` and
/// `trait` blocks.
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
    let parsed: Vec<(String, syn::File)> = rust_files(&root().join("src"))
        .iter()
        .map(|path| (repo_relative(path), parse_file(path)))
        .collect();
    let test_modules = test_modules(parsed.iter().map(|(rel, file)| (rel.as_str(), file)));
    assert!(
        !test_modules.is_empty(),
        "no test-gated module found under src/: the test-only files cannot be told \
         from the library's"
    );
    let sources: Vec<(String, syn::File)> = parsed
        .into_iter()
        .filter(|(rel, _)| !is_test_only_path(rel, &test_modules))
        .map(|(rel, mut file)| {
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
