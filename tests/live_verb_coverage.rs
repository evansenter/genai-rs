//! Guards that every public verb on every resource handle is called by a
//! live test, on that handle.
//!
//! The handles are the structs marked `#[must_use = "a resource handle does
//! nothing until you call one of its methods"]`; their verbs are the public
//! methods of their `impl` blocks, wherever those live. A live test is a test
//! carrying `#[ignore = "Requires API key"]`, together with every function in
//! `tests/` it reaches by name (cleanup helpers count).
//!
//! A call counts only on a receiver whose handle type is known: a chain from
//! an accessor (`client.file_search_stores().documents().get(..)`), or a
//! local bound to one (`let stores = client.file_search_stores();`). So
//! `client.webhooks().get(..)` does not cover `Agents::get`, which a match
//! on `.get(` anywhere would. Calls inside macros (`assert!`, `tokio::join!`)
//! are read too, and the source is parsed with `syn`, so formatting cannot
//! hide or fake a call.
//!
//! A verb added without a live test fails here; add the test, or an
//! [`EXEMPT`] entry saying why it cannot have one.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Expr, ImplItem, Item, Pat, ReturnType, Type};

/// The attribute that marks a resource handle.
const HANDLE_MARKER: &str = "a resource handle does nothing until you call one of its methods";

/// The one ignore reason for a live test (CLAUDE.md, "Tests").
const LIVE_REASON: &str = "Requires API key";

/// `Handle::verb` pairs no live test needs to call, each with its reason.
const EXEMPT: &[(&str, &str)] = &[];

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
struct Handles {
    verbs: BTreeMap<String, BTreeSet<String>>,
    accessors: HashMap<(String, String), String>,
}

fn handles() -> Handles {
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    let files: Vec<syn::File> = files
        .iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "antigravity"))
        .map(|p| parse(p))
        .collect();

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
    for file in &files {
        flatten(&file.items, &mut all);
    }

    let names: BTreeSet<String> = all
        .iter()
        .filter_map(|item| match item {
            Item::Struct(s)
                if s.attrs
                    .iter()
                    .any(|a| attr_string(a, "must_use").as_deref() == Some(HANDLE_MARKER)) =>
            {
                Some(s.ident.to_string())
            }
            _ => None,
        })
        .collect();

    let mut verbs: BTreeMap<String, BTreeSet<String>> =
        names.iter().map(|n| (n.clone(), BTreeSet::new())).collect();
    let mut accessors = HashMap::new();
    for item in &all {
        let Item::Impl(imp) = item else { continue };
        if imp.trait_.is_some() {
            continue;
        }
        let Some(owner) = last_ident(&imp.self_ty) else {
            continue;
        };
        if owner != "Client" && !names.contains(&owner) {
            continue;
        }
        for item in &imp.items {
            let ImplItem::Fn(f) = item else { continue };
            if !matches!(f.vis, syn::Visibility::Public(_)) || f.sig.receiver().is_none() {
                continue;
            }
            let method = f.sig.ident.to_string();
            if let ReturnType::Type(_, ty) = &f.sig.output
                && let Some(ret) = last_ident(ty)
                && names.contains(&ret)
            {
                accessors.insert((owner.clone(), method.clone()), ret);
            }
            if let Some(set) = verbs.get_mut(&owner) {
                set.insert(method);
            }
        }
    }
    Handles { verbs, accessors }
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
struct Calls<'a> {
    handles: &'a Handles,
    locals: HashMap<String, String>,
    found: BTreeSet<(String, String)>,
}

impl Calls<'_> {
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
fn live_calls(handles: &Handles) -> BTreeSet<(String, String)> {
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
        let mut calls = Calls {
            handles,
            locals: HashMap::new(),
            found: BTreeSet::new(),
        };
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

#[test]
fn every_handle_verb_has_a_live_test() {
    let handles = handles();
    let verb_count: usize = handles.verbs.values().map(BTreeSet::len).sum();
    assert!(
        handles.verbs.len() >= 11 && verb_count > 50,
        "parsed too few handles ({}) or verbs ({verb_count}): {:?}",
        handles.verbs.len(),
        handles.verbs.keys()
    );
    let called = live_calls(&handles);

    let mut missing = Vec::new();
    let mut stale = Vec::new();
    for (handle, verbs) in &handles.verbs {
        for verb in verbs {
            let key = format!("{handle}::{verb}");
            let exempt = EXEMPT.iter().any(|(k, _)| *k == key);
            let covered = called.contains(&(handle.clone(), verb.clone()));
            if !covered && !exempt {
                missing.push(key);
            } else if covered && exempt {
                stale.push(format!("{key} (now called live)"));
            }
        }
    }
    for (key, _) in EXEMPT {
        let known = key
            .split_once("::")
            .is_some_and(|(h, v)| handles.verbs.get(h).is_some_and(|verbs| verbs.contains(v)));
        if !known {
            stale.push(format!("{key} (no such verb)"));
        }
    }
    assert!(
        missing.is_empty() && stale.is_empty(),
        "Verbs no live test calls on their handle ({}): add a live test \
         (#[ignore = \"{LIVE_REASON}\"]) that calls each, or an EXEMPT entry \
         with the reason:\n  {}\n\nStale EXEMPT entries ({}):\n  {}",
        missing.len(),
        missing.join("\n  "),
        stale.len(),
        stale.join("\n  ")
    );
}

/// The receiver typing is what makes the guard per handle: a `get` on one
/// handle must not count for another.
#[test]
fn a_call_counts_only_on_its_own_handle() {
    let handles = handles();
    let block: syn::Block = syn::parse_quote!({
        let stores = client.file_search_stores();
        stores.create(&request).await;
        client.webhooks().get("id").await;
        assert!(
            client
                .environments()
                .files()
                .list("e", "")
                .send()
                .await
                .is_ok()
        );
        let _ = client.files().get("f");
        self.client.voices().delete("v");
    });
    let mut calls = Calls {
        handles: &handles,
        locals: HashMap::new(),
        found: BTreeSet::new(),
    };
    calls.visit_block(&block);
    let found: BTreeSet<String> = calls
        .found
        .iter()
        .map(|(h, v)| format!("{h}::{v}"))
        .collect();
    let expected: BTreeSet<String> = [
        "Client::file_search_stores",
        "FileSearchStores::create",
        "Client::webhooks",
        "Webhooks::get",
        "Client::environments",
        "Environments::files",
        "EnvironmentFiles::list",
        "Client::files",
        "Files::get",
        "Client::voices",
        "Voices::delete",
    ]
    .into_iter()
    .filter(|k| !k.starts_with("Client::"))
    .map(String::from)
    .collect();
    assert_eq!(found, expected);
}
