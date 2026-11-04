//! Finds every public verb on every resource handle, and every verb a live
//! test calls on its handle, for `tests/ci_coverage.rs`.
//!
//! The handles are the structs marked `#[must_use = "a resource handle does
//! nothing until you call one of its methods"]` (`docs/BUILDER_API.md`,
//! "Resource handles and list builders"); their verbs are the public methods
//! of their `impl` blocks, wherever those live. The marker is checked from
//! the other side too: every `Client` accessor that returns a borrowed
//! struct, and every borrowed struct a handle returns that is not a request
//! builder, must carry it, so a handle written without the marker is
//! reported rather than skipped ([`Handles::problems`]).
//!
//! A live test is a test carrying `#[ignore = "Requires API key"]`, together
//! with every function in `tests/` it reaches by name (cleanup helpers
//! count). A call counts only on a receiver whose handle type is known: a
//! chain from an accessor (`client.file_search_stores().documents().get(..)`),
//! or a local bound to one (`let stores = client.file_search_stores();`). So
//! `client.webhooks().get(..)` does not cover `Agents::get`, which a match on
//! `.get(` anywhere would. Calls inside macros (`assert!`, `tokio::join!`)
//! are read too, and the source is parsed with `syn`, so formatting cannot
//! hide or fake a call.
//!
//! Not a test target itself: Cargo only builds `tests/*.rs` and
//! `tests/*/main.rs`.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Expr, ImplItem, Item, Pat, ReturnType, Type};

/// The attribute that marks a resource handle.
pub const HANDLE_MARKER: &str = "a resource handle does nothing until you call one of its methods";

/// The one ignore reason for a live test (CLAUDE.md, "Tests").
pub const LIVE_REASON: &str = "Requires API key";

/// Borrowed structs a `Client` accessor returns that are not resource
/// handles, each with its reason.
pub const NOT_HANDLES: &[(&str, &str)] = &[(
    "InteractionBuilder",
    "A request builder, not a resource handle: `client.interaction()` builds \
     the request `create()` sends, which every live interaction test does.",
)];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
}

fn parse(path: &Path) -> syn::File {
    let text = std::fs::read_to_string(path).expect("read source");
    syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Type::Reference(r) => last_ident(&r.elem),
        _ => None,
    }
}

fn attr_string(attr: &syn::Attribute, name: &str) -> Option<String> {
    if !attr.path().is_ident(name) {
        return None;
    }
    match &attr.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    }
}

/// Every handle and its verbs, and every accessor: `(receiver, method)` to
/// the handle it returns, with `Client` as the receiver of the root ones.
pub struct Handles {
    pub verbs: BTreeMap<String, BTreeSet<String>>,
    pub accessors: HashMap<(String, String), String>,
    /// Handles declared without the marker, and stale [`NOT_HANDLES`]
    /// entries.
    pub problems: Vec<String>,
}

/// The struct a return type borrows into (`Webhooks<'_>`,
/// `ListWebhooks<'a>`): a path type with a lifetime argument.
fn borrowed_struct(ty: &Type) -> Option<String> {
    let Type::Path(p) = ty else { return None };
    let last = p.path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args
        .iter()
        .any(|a| matches!(a, syn::GenericArgument::Lifetime(_)))
        .then(|| last.ident.to_string())
}

/// The handles in `src/` (without the antigravity runtime).
pub fn handles() -> Handles {
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    let files: Vec<syn::File> = files
        .iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "antigravity"))
        .map(|p| parse(p))
        .collect();
    handles_in(&files)
}

/// A public method with a receiver.
struct Method {
    name: String,
    by_ref: bool,
    /// The struct it returns, when that borrows (`X<'_>`).
    returns: Option<String>,
}

pub fn handles_in(files: &[syn::File]) -> Handles {
    fn flatten(items: &[Item], out: &mut Vec<Item>) {
        for item in items {
            if let Item::Mod(m) = item
                && let Some((_, inner)) = &m.content
            {
                flatten(inner, out);
            }
            out.push(item.clone());
        }
    }
    let mut all = Vec::new();
    for file in files {
        flatten(&file.items, &mut all);
    }

    // Every struct, and whether it carries the marker.
    let structs: BTreeMap<String, bool> = all
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s) => Some((
                s.ident.to_string(),
                s.attrs
                    .iter()
                    .any(|a| attr_string(a, "must_use").as_deref() == Some(HANDLE_MARKER)),
            )),
            _ => None,
        })
        .collect();
    // Every type's public methods, from its inherent impls.
    let mut methods: BTreeMap<String, Vec<Method>> = BTreeMap::new();
    for item in &all {
        let Item::Impl(imp) = item else { continue };
        if imp.trait_.is_some() {
            continue;
        }
        let Some(owner) = last_ident(&imp.self_ty) else {
            continue;
        };
        for item in &imp.items {
            let ImplItem::Fn(f) = item else { continue };
            let Some(receiver) = f.sig.receiver() else {
                continue;
            };
            if !matches!(f.vis, syn::Visibility::Public(_)) {
                continue;
            }
            let returns = match &f.sig.output {
                ReturnType::Type(_, ty) => {
                    borrowed_struct(ty).filter(|name| structs.contains_key(name))
                }
                ReturnType::Default => None,
            };
            methods.entry(owner.clone()).or_default().push(Method {
                name: f.sig.ident.to_string(),
                by_ref: receiver.reference.is_some(),
                returns,
            });
        }
    }
    let names: BTreeSet<String> = structs
        .iter()
        .filter(|(_, marked)| **marked)
        .map(|(name, _)| name.clone())
        .collect();

    let mut problems = Vec::new();
    let no_methods = Vec::new();
    let methods_of = |owner: &str| methods.get(owner).unwrap_or(&no_methods);
    // A root handle is what a `&self` accessor on `Client` returns.
    let mut not_handles_used = BTreeSet::new();
    for m in methods_of("Client") {
        let Some(ret) = &m.returns else { continue };
        if !m.by_ref {
            continue;
        }
        if NOT_HANDLES.iter().any(|(name, _)| *name == ret.as_str()) {
            not_handles_used.insert(ret.clone());
        } else if !names.contains(ret) {
            problems.push(format!(
                "Client::{} returns {ret}, which lacks the handle marker \
                 #[must_use = \"{HANDLE_MARKER}\"] (or list it in NOT_HANDLES with \
                 the reason)",
                m.name
            ));
        }
    }
    for (name, _) in NOT_HANDLES {
        if !not_handles_used.contains(*name) {
            problems.push(format!(
                "NOT_HANDLES: no Client accessor returns {name}; delete the entry"
            ));
        }
    }
    // A nested handle is a borrowed struct a handle returns that is not a
    // request builder (one with a `send` method, like the list builders).
    for handle in &names {
        for m in methods_of(handle) {
            let Some(ret) = &m.returns else { continue };
            let builder = methods_of(ret).iter().any(|m| m.name == "send");
            if !names.contains(ret) && !builder {
                problems.push(format!(
                    "{handle}::{} returns {ret}, which is neither a request builder \
                     (no `send`) nor marked as a handle: add \
                     #[must_use = \"{HANDLE_MARKER}\"]",
                    m.name
                ));
            }
        }
    }

    let mut verbs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut accessors = HashMap::new();
    for owner in names.iter().map(String::as_str).chain(["Client"]) {
        for m in methods_of(owner) {
            if let Some(ret) = &m.returns
                && names.contains(ret)
            {
                accessors.insert((owner.to_string(), m.name.clone()), ret.clone());
            }
            if owner != "Client" {
                verbs
                    .entry(owner.to_string())
                    .or_default()
                    .insert(m.name.clone());
            }
        }
    }
    for name in &names {
        verbs.entry(name.clone()).or_default();
    }
    Handles {
        verbs,
        accessors,
        problems,
    }
}

// =============================================================================
// Live tests and the calls they make
// =============================================================================

/// A function in `tests/`: its name, whether it is a live test, and its body.
struct TestFn {
    name: String,
    live: bool,
    block: syn::Block,
}

fn test_fns() -> Vec<TestFn> {
    let mut files = Vec::new();
    rust_files(&root().join("tests"), &mut files);
    let mut out = Vec::new();
    fn collect(items: &[Item], out: &mut Vec<TestFn>) {
        for item in items {
            match item {
                Item::Fn(f) => out.push(TestFn {
                    name: f.sig.ident.to_string(),
                    live: f
                        .attrs
                        .iter()
                        .any(|a| attr_string(a, "ignore").as_deref() == Some(LIVE_REASON)),
                    block: (*f.block).clone(),
                }),
                Item::Mod(m) => {
                    if let Some((_, inner)) = &m.content {
                        collect(inner, out);
                    }
                }
                _ => {}
            }
        }
    }
    for path in files {
        // The harness suite drives the antigravity runtime, not these handles.
        if path.ends_with("antigravity_harness.rs") || path.starts_with(root().join("tests/ui")) {
            continue;
        }
        collect(&parse(&path).items, &mut out);
    }
    out
}

/// Names of the functions a body calls (`helper(..)`, `common::helper(..)`).
#[derive(Default)]
struct Callees(BTreeSet<String>);

impl Visit<'_> for Callees {
    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let Expr::Path(p) = &*call.func
            && let Some(segment) = p.path.segments.last()
        {
            self.0.insert(segment.ident.to_string());
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &syn::Macro) {
        for stmt in macro_stmts(mac) {
            self.visit_stmt(&stmt);
        }
    }
}

/// The statements in a macro's arguments: each top-level comma-separated
/// part that parses as an expression, and for a part that does not (a
/// `matches!` pattern, `retry_request!`'s `[captures] => { body }`), the
/// statements inside each bracketed group in it.
fn macro_stmts(mac: &syn::Macro) -> Vec<syn::Stmt> {
    use syn::parse::Parser;
    use syn::parse::discouraged::Speculative;
    let parser = |input: syn::parse::ParseStream| {
        let mut out = Vec::new();
        while !input.is_empty() {
            let fork = input.fork();
            match fork.parse::<Expr>() {
                Ok(expr) if fork.is_empty() || fork.peek(syn::Token![,]) => {
                    input.advance_to(&fork);
                    out.push(syn::Stmt::Expr(expr, None));
                }
                _ => input.step(|cursor| {
                    let mut rest = *cursor;
                    while let Some((tree, next)) = rest.token_tree() {
                        if tree.to_string() == "," {
                            break;
                        }
                        if let Some((inside, ..)) = rest.any_group()
                            && let Ok(stmts) =
                                syn::Block::parse_within.parse2(inside.token_stream())
                        {
                            out.extend(stmts);
                        }
                        rest = next;
                    }
                    Ok(((), rest))
                })?,
            }
            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(out)
    };
    mac.parse_body_with(parser).unwrap_or_default()
}

/// Records `(handle, verb)` for every call on a receiver of known handle
/// type.
pub struct Calls<'a> {
    handles: &'a Handles,
    locals: HashMap<String, String>,
    pub found: BTreeSet<(String, String)>,
}

impl<'a> Calls<'a> {
    pub fn new(handles: &'a Handles) -> Self {
        Self {
            handles,
            locals: HashMap::new(),
            found: BTreeSet::new(),
        }
    }

    fn handle_of(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::MethodCall(call) => {
                let method = call.method.to_string();
                match self.handle_of(&call.receiver) {
                    Some(owner) => self.handles.accessors.get(&(owner, method)).cloned(),
                    None => self
                        .handles
                        .accessors
                        .get(&("Client".to_string(), method))
                        .cloned(),
                }
            }
            Expr::Path(p) => p
                .path
                .get_ident()
                .and_then(|i| self.locals.get(&i.to_string()).cloned()),
            Expr::Paren(e) => self.handle_of(&e.expr),
            Expr::Reference(e) => self.handle_of(&e.expr),
            _ => None,
        }
    }
}

impl Visit<'_> for Calls<'_> {
    fn visit_local(&mut self, local: &syn::Local) {
        if let (Pat::Ident(name), Some(init)) = (&local.pat, &local.init)
            && let Some(handle) = self.handle_of(&init.expr)
        {
            self.locals.insert(name.ident.to_string(), handle);
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if let Some(owner) = self.handle_of(&call.receiver) {
            let method = call.method.to_string();
            if self.handles.verbs[&owner].contains(&method) {
                self.found.insert((owner, method));
            }
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &syn::Macro) {
        for stmt in macro_stmts(mac) {
            self.visit_stmt(&stmt);
        }
    }
}

/// Every `(handle, verb)` a live test calls, directly or through a helper.
pub fn live_calls(handles: &Handles) -> BTreeSet<(String, String)> {
    let fns = test_fns();
    let by_name: HashMap<&str, Vec<&TestFn>> = fns.iter().fold(HashMap::new(), |mut m, f| {
        m.entry(f.name.as_str()).or_default().push(f);
        m
    });
    let mut queue: Vec<&TestFn> = fns.iter().filter(|f| f.live).collect();
    assert!(queue.len() > 50, "found only {} live tests", queue.len());
    let mut seen: BTreeSet<*const TestFn> = BTreeSet::new();
    let mut found = BTreeSet::new();
    while let Some(f) = queue.pop() {
        if !seen.insert(f as *const TestFn) {
            continue;
        }
        let mut calls = Calls::new(handles);
        calls.visit_block(&f.block);
        found.extend(calls.found);
        let mut callees = Callees::default();
        callees.visit_block(&f.block);
        for name in callees.0 {
            if let Some(helpers) = by_name.get(name.as_str()) {
                queue.extend(helpers.iter().filter(|h| !h.live));
            }
        }
    }
    found
}
