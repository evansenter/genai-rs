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

/// A file module, as [`module_files`] finds it: its path, and the
/// predicate of every `#[cfg(..)]` on the way to it, as written (on its
/// own declaration, on an inline module around it, and on the declarations
/// of the files that declare it).
pub struct ModuleFile {
    pub path: PathBuf,
    pub cfgs: Vec<String>,
}

/// `root` (a crate root, or a test binary's top-level file) and every file
/// module it declares, recursively, breadth first, as rustc resolves them:
/// a `#[path]` attribute's file relative to the declaring file's directory,
/// else `name.rs` or `name/mod.rs` in the declaring file's own directory
/// when that file is a crate root, a `mod.rs` or loaded by `#[path]`, and
/// in its `<stem>/` subdirectory otherwise; inside an inline module `m`,
/// each in that directory's `m/`. Each path once, as rustc forms it, a
/// `#[path]`'s `..` kept: the name rustc gives the file, and llvm-cov with
/// it ([`lexically_normal`] resolves it). A declared module whose file does
/// not exist fails, rather than leaving the file out.
pub fn module_files(root: &Path) -> Vec<ModuleFile> {
    struct Found {
        file: ModuleFile,
        owns_dir: bool,
    }
    fn declared(
        items: &[syn::Item],
        dir: &Path,
        path_dir: &Path,
        cfgs: &[String],
        out: &mut Vec<Found>,
    ) {
        for item in items {
            let syn::Item::Mod(module) = item else {
                continue;
            };
            let mut cfgs = cfgs.to_vec();
            let mut path = None;
            for attr in &module.attrs {
                if attr.path().is_ident("cfg") {
                    cfgs.push(
                        attr.meta
                            .require_list()
                            .map(|list| list.tokens.to_string())
                            .unwrap_or_default(),
                    );
                } else if attr.path().is_ident("path")
                    && let syn::Meta::NameValue(value) = &attr.meta
                    && let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(text),
                        ..
                    }) = &value.value
                {
                    path = Some(text.value());
                }
            }
            let name = module.ident.to_string();
            match &module.content {
                Some((_, inner)) => {
                    let inner_dir = dir.join(&name);
                    declared(inner, &inner_dir, &inner_dir, &cfgs, out);
                }
                None => {
                    let flat = dir.join(format!("{name}.rs"));
                    let (path, owns_dir) = match path {
                        Some(path) => (path_dir.join(path), true),
                        None if flat.exists() => (flat, false),
                        None => (dir.join(&name).join("mod.rs"), true),
                    };
                    out.push(Found {
                        file: ModuleFile { path, cfgs },
                        owns_dir,
                    });
                }
            }
        }
    }
    let mut files = vec![Found {
        file: ModuleFile {
            path: root.to_path_buf(),
            cfgs: Vec::new(),
        },
        owns_dir: true,
    }];
    let mut next = 0;
    while next < files.len() {
        let file = files[next].file.path.clone();
        let cfgs = files[next].file.cfgs.clone();
        let owns_dir = files[next].owns_dir;
        next += 1;
        let dir = file.parent().expect("a file has a directory");
        let own_dir = if owns_dir {
            dir.to_path_buf()
        } else {
            dir.join(file.file_stem().expect("a file has a stem"))
        };
        let mut found = Vec::new();
        declared(&parse_file(&file).items, &own_dir, dir, &cfgs, &mut found);
        for module in found {
            assert!(
                module.file.path.exists(),
                "{} declares a module in {}, which does not exist",
                file.display(),
                module.file.path.display()
            );
            if !files
                .iter()
                .any(|known| known.file.path == module.file.path)
            {
                files.push(module);
            }
        }
    }
    files.into_iter().map(|found| found.file).collect()
}

/// `path` with each `.` dropped and each `..` taking off the component
/// before it, without touching the file system: the file a `#[path]` with a
/// `..` in it names, spelt as a walk of the directory spells it.
pub fn lexically_normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
                if matches!(
                    out.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) =>
            {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
