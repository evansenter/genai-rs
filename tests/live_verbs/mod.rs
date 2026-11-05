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
//! or a local bound to one (`let stores = client.file_search_stores();`)
//! until a `let`, an assignment or a pattern binds that name again. So
//! `client.webhooks().get(..)` does not cover `Agents::get`, which a match on
//! `.get(` anywhere would, and `envs.files().list(..)` on an untracked `envs`
//! covers neither `Files::list` nor `EnvironmentFiles::list`. Calls inside macros (`assert!`, `tokio::join!`)
//! are read too, and the source is parsed with `syn`, so formatting cannot
//! hide or fake a call.
//!
//! [`body_verbs`] finds, for `tests/request_wire_oracle.rs`, the verbs whose
//! implementation reaches a POST, PATCH or PUT request through the
//! functions and methods it calls: the verbs that send a body.
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

/// The source files in `src/`, without the antigravity runtime.
fn src_files() -> Vec<syn::File> {
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    files
        .iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "antigravity"))
        .map(|p| parse(p))
        .collect()
}

/// The handles in `src/` (without the antigravity runtime).
pub fn handles() -> Handles {
    handles_in(&src_files())
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

    /// The handle type `expr` evaluates to, when it is known.
    ///
    /// An accessor on a receiver of unknown type is resolved only when that
    /// cannot pick the wrong handle: on a receiver named like a client
    /// (`client`, `self.client`, `get_client()`), as a `Client` accessor;
    /// otherwise only when every type with an accessor of that name returns
    /// the same handle. `.files()` is on both `Client` and `Environments`,
    /// so `envs.files()` on an untracked `envs` counts for neither.
    fn handle_of(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::MethodCall(call) => {
                let method = call.method.to_string();
                match self.handle_of(&call.receiver) {
                    Some(owner) => self.handles.accessors.get(&(owner, method)).cloned(),
                    None if names_a_client(&call.receiver) => self
                        .handles
                        .accessors
                        .get(&("Client".to_string(), method))
                        .cloned(),
                    None => {
                        let returns: BTreeSet<&String> = self
                            .handles
                            .accessors
                            .iter()
                            .filter(|((_, name), _)| *name == method)
                            .map(|(_, handle)| handle)
                            .collect();
                        match returns.into_iter().collect::<Vec<_>>().as_slice() {
                            [only] => Some((*only).clone()),
                            _ => None,
                        }
                    }
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

/// Whether an expression is named like a client: a path, field, call or
/// method call whose last name is `client` or ends in `client`
/// (`client`, `self.client`, `test_client`, `stub.client()`).
fn names_a_client(expr: &Expr) -> bool {
    let name = match expr {
        Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        Expr::Field(f) => match &f.member {
            syn::Member::Named(ident) => Some(ident.to_string()),
            syn::Member::Unnamed(_) => None,
        },
        Expr::Call(call) => match &*call.func {
            Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        },
        Expr::MethodCall(call) => Some(call.method.to_string()),
        Expr::Paren(e) => return names_a_client(&e.expr),
        Expr::Reference(e) => return names_a_client(&e.expr),
        Expr::Try(e) => return names_a_client(&e.expr),
        _ => None,
    };
    name.is_some_and(|n| n.to_ascii_lowercase().ends_with("client"))
}

/// The name a `let` binds as a whole (`x`, `mut x`, `x: T`), and its type
/// annotation.
fn whole_binding(pat: &Pat) -> Option<(String, Option<&Type>)> {
    match pat {
        Pat::Ident(p) if p.subpat.is_none() => Some((p.ident.to_string(), None)),
        Pat::Type(t) => whole_binding(&t.pat).map(|(name, _)| (name, Some(&*t.ty))),
        _ => None,
    }
}

impl Visit<'_> for Calls<'_> {
    /// A `let` rebinds every name its pattern binds: each one's handle is
    /// the initializer's (or the annotation's), or none. The initializer,
    /// and a `let`-`else` block, run before the new bindings exist, so they
    /// are read with the old ones.
    fn visit_local(&mut self, local: &syn::Local) {
        let resolved = local
            .init
            .as_ref()
            .and_then(|init| self.handle_of(&init.expr));
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
        // Clears every name the pattern binds (`visit_pat_ident`).
        self.visit_pat(&local.pat);
        if let Some((name, ty)) = whole_binding(&local.pat) {
            let annotated = ty
                .and_then(last_ident)
                .filter(|ty| self.handles.verbs.contains_key(ty));
            if let Some(handle) = resolved.or(annotated) {
                self.locals.insert(name, handle);
            }
        }
    }

    /// `x = expr` rebinds `x` like a `let` does.
    fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
        self.visit_expr(&assign.right);
        let resolved = self.handle_of(&assign.right);
        self.visit_expr(&assign.left);
        if let Expr::Path(p) = &*assign.left
            && let Some(name) = p.path.get_ident()
        {
            let name = name.to_string();
            match resolved {
                Some(handle) => self.locals.insert(name, handle),
                None => self.locals.remove(&name),
            };
        }
    }

    /// A name bound anywhere (a `let`, a closure parameter, a `match` arm,
    /// a `for` loop) no longer holds the handle an earlier binding did.
    fn visit_pat_ident(&mut self, pat: &syn::PatIdent) {
        self.locals.remove(&pat.ident.to_string());
        visit::visit_pat_ident(self, pat);
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

// =============================================================================
// Verbs that send a request body
// =============================================================================

/// Whether an item is compiled for tests only (`#[cfg(test)]`).
fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.parse_args::<syn::Ident>()
                .is_ok_and(|ident| ident == "test")
    })
}

/// What a function body does: whether it makes a request that can carry a
/// body (`Method::POST`, `PATCH` or `PUT`, or reqwest's `.post(..)`,
/// `.put(..)`, `.patch(..)`), the functions it calls by path
/// (`helper(..)`, `http::helper(..)`, `Self::helper(..)`), and the methods
/// it calls (`self.helper(..)`, `request.send_with(..)`).
#[derive(Default)]
struct Reach {
    /// The type of the `impl` the body is in, for `self.method(..)`.
    owner: Option<String>,
    sends: bool,
    callees: BTreeSet<String>,
    /// Each method called: its name, and the `impl`'s type when the
    /// receiver is `self` (otherwise the receiver's type is unknown).
    methods: BTreeSet<(Option<String>, String)>,
}

impl Visit<'_> for Reach {
    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        let names: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        if let [.., method, verb] = names.as_slice()
            && method == "Method"
            && matches!(verb.as_str(), "POST" | "PATCH" | "PUT")
        {
            self.sends = true;
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        let method = call.method.to_string();
        if matches!(method.as_str(), "post" | "put" | "patch") {
            self.sends = true;
        }
        let on_self = matches!(&*call.receiver, Expr::Path(p) if p.path.is_ident("self"));
        let owner = self.owner.clone().filter(|_| on_self);
        self.methods.insert((owner, method));
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let Expr::Path(p) = &*call.func
            && let Some(segment) = p.path.segments.last()
        {
            self.callees.insert(segment.ident.to_string());
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &syn::Macro) {
        for stmt in macro_stmts(mac) {
            self.visit_stmt(&stmt);
        }
    }
}

/// Every `(handle, verb)` that sends a request body: whose implementation
/// reaches a POST, PATCH or PUT request through the functions it calls by
/// path (`crate::http::webhooks::ping_webhook(..)`) and the methods it calls
/// (`self.send_patch(..)`, `request.send_with(..)`), test code aside.
///
/// A method on `self` resolves to the `impl` type's own method of that name
/// when it has one. On any other receiver the type is unknown, so it
/// resolves to every method of that name in the crate: a verb is reported
/// when it might send a body, never left out when it does.
pub fn body_verbs(handles: &Handles) -> BTreeSet<(String, String)> {
    body_verbs_in(&src_files(), handles)
}

/// Every non-test function body in the crate, and how calls reach them.
#[derive(Default)]
struct Bodies {
    reaches: Vec<Reach>,
    /// Every function, by name: what a path call reaches.
    by_name: HashMap<String, Vec<usize>>,
    /// Every function with a receiver, by name: what a method call on a
    /// receiver of unknown type reaches.
    methods: HashMap<String, Vec<usize>>,
    /// Every function with a receiver, by `impl` type and name.
    by_owner: HashMap<(String, String), Vec<usize>>,
    verbs: Vec<((String, String), usize)>,
}

impl Bodies {
    fn add(&mut self, owner: Option<&str>, sig: &syn::Signature, block: &syn::Block) -> usize {
        let mut reach = Reach {
            owner: owner.map(str::to_string),
            ..Reach::default()
        };
        reach.visit_block(block);
        let at = self.reaches.len();
        self.reaches.push(reach);
        let name = sig.ident.to_string();
        self.by_name.entry(name.clone()).or_default().push(at);
        if sig.receiver().is_some() {
            self.methods.entry(name.clone()).or_default().push(at);
            if let Some(owner) = owner {
                self.by_owner
                    .entry((owner.to_string(), name))
                    .or_default()
                    .push(at);
            }
        }
        at
    }

    fn collect(&mut self, items: &[Item], handles: &Handles) {
        for item in items {
            match item {
                Item::Fn(f) if !test_only(&f.attrs) => {
                    self.add(None, &f.sig, &f.block);
                }
                Item::Mod(m) if !test_only(&m.attrs) => {
                    if let Some((_, inner)) = &m.content {
                        self.collect(inner, handles);
                    }
                }
                Item::Impl(imp) if !test_only(&imp.attrs) => {
                    let owner = last_ident(&imp.self_ty).unwrap_or_default();
                    for item in &imp.items {
                        let ImplItem::Fn(f) = item else { continue };
                        if test_only(&f.attrs) {
                            continue;
                        }
                        let at = self.add(Some(&owner), &f.sig, &f.block);
                        let name = f.sig.ident.to_string();
                        let is_verb = imp.trait_.is_none()
                            && handles
                                .verbs
                                .get(&owner)
                                .is_some_and(|verbs| verbs.contains(&name));
                        if is_verb {
                            self.verbs.push(((owner.clone(), name), at));
                        }
                    }
                }
                Item::Trait(t) if !test_only(&t.attrs) => {
                    for item in &t.items {
                        if let syn::TraitItem::Fn(f) = item
                            && let Some(block) = &f.default
                        {
                            self.add(None, &f.sig, block);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether the body at `start` makes a body-carrying request, itself or
    /// through anything it calls.
    fn sends(&self, start: usize) -> bool {
        let none = Vec::new();
        let mut queue = vec![start];
        let mut seen = BTreeSet::new();
        while let Some(at) = queue.pop() {
            if !seen.insert(at) {
                continue;
            }
            let reach = &self.reaches[at];
            if reach.sends {
                return true;
            }
            for name in &reach.callees {
                queue.extend(self.by_name.get(name).unwrap_or(&none));
            }
            for (owner, name) in &reach.methods {
                let own = owner
                    .as_ref()
                    .and_then(|owner| self.by_owner.get(&(owner.clone(), name.clone())));
                queue.extend(own.or_else(|| self.methods.get(name)).unwrap_or(&none));
            }
        }
        false
    }
}

pub fn body_verbs_in(files: &[syn::File], handles: &Handles) -> BTreeSet<(String, String)> {
    let mut bodies = Bodies::default();
    for file in files {
        bodies.collect(&file.items, handles);
    }
    bodies
        .verbs
        .iter()
        .filter(|(_, at)| bodies.sends(*at))
        .map(|(verb, _)| verb.clone())
        .collect()
}

/// Every `(handle, verb)` called on its handle by the function `name` in
/// `file`, or by a function in `file` it reaches by name.
pub fn calls_from(file: &syn::File, name: &str, handles: &Handles) -> BTreeSet<(String, String)> {
    let fns: HashMap<String, &syn::Block> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(f) => Some((f.sig.ident.to_string(), &*f.block)),
            _ => None,
        })
        .collect();
    assert!(fns.contains_key(name), "no fn {name} in the file");
    let mut queue = vec![name.to_string()];
    let mut seen = BTreeSet::new();
    let mut found = BTreeSet::new();
    while let Some(name) = queue.pop() {
        let Some(block) = fns.get(&name) else {
            continue;
        };
        if !seen.insert(name) {
            continue;
        }
        let mut calls = Calls::new(handles);
        calls.visit_block(block);
        found.extend(calls.found);
        let mut callees = Callees::default();
        callees.visit_block(block);
        queue.extend(callees.0);
    }
    found
}
