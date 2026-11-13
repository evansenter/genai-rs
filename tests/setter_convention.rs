//! Guards the setter convention (`docs/BUILDER_API.md`): `with_*`
//! configures a setting, so calling it twice replaces; `add_*` appends to a
//! collection.
//!
//! Every public `with_*` and `add_*` method in the library's own source is
//! read through `tests/common/source_scan.rs` (D-068). A `with_*` body that
//! pushes, inserts, extends or appends onto a collection it reaches through
//! `self` appends, so it is an `add_*`; an `add_*` body that assigns a whole
//! field through `self` replaces, so it is a `with_*`. Either reaches
//! `self` directly (`self.tools.push(..)`) or through a name the body binds
//! to a part of it (`env` in `if let Stdio { env, .. } = &mut
//! self.transport`). #504's I-10 renamed two that had slipped in:
//! `AgentBuilder::with_tool_service`, which pushed, and
//! `CreateFileSearchStoreRequest::with_extra`, which inserted. A method
//! that follows the convention another way is named in `EXCEPTIONS` with
//! the reason.

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

/// Setters the scan flags that keep the convention, each with the reason,
/// keyed `Type::method`.
const EXCEPTIONS: &[(&str, &str)] = &[];

/// The method calls that grow a collection rather than replace it.
const APPENDS: &[&str] = &[
    "push",
    "push_back",
    "push_front",
    "push_str",
    "insert",
    "extend",
    "extend_from_slice",
    "append",
];

/// How many public `with_*` and `add_*` methods the library has at least:
/// a scan that finds fewer has stopped reading the source.
const FLOOR: usize = 250;

#[test]
fn setters_follow_the_with_and_add_convention() {
    let sources = source_scan::library_sources();
    let files: Vec<syn::File> = sources.into_iter().map(|(_, file)| file).collect();
    let items = source_scan::all_items(&files);
    let methods = source_scan::public_methods(&items);
    let mut checked = 0;
    let mut found = BTreeSet::new();
    for (owner, fns) in &methods {
        for f in fns {
            let name = f.sig.ident.to_string();
            if !(name.starts_with("with_") || name.starts_with("add_")) {
                continue;
            }
            checked += 1;
            if let Some(problem) = violation(&name, &f.block) {
                found.insert(format!("{owner}::{name}: {problem}"));
            }
        }
    }
    assert!(
        checked >= FLOOR,
        "only {checked} public with_*/add_* methods found (floor {FLOOR})"
    );
    let excepted: BTreeSet<&str> = EXCEPTIONS.iter().map(|(key, _)| *key).collect();
    let unlisted: Vec<&String> = found
        .iter()
        .filter(|hit| !excepted.contains(hit.split(':').next().unwrap()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "a setter breaks the with_/add_ convention (docs/BUILDER_API.md): rename it, \
         or name it in EXCEPTIONS with the reason:\n  {}",
        unlisted
            .iter()
            .map(|hit| hit.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    let stale: Vec<&&str> = excepted
        .iter()
        .filter(|key| !found.iter().any(|hit| hit.split(':').next() == Some(**key)))
        .collect();
    assert!(
        stale.is_empty(),
        "EXCEPTIONS names setters the scan no longer flags: {stale:?}"
    );
}

/// What breaks the convention in a setter's body: for `with_*`, a call
/// that grows a collection reached through `self`; for `add_*`, an
/// assignment to a field of `self`, or through a binding of one. `None`
/// when the body keeps it.
fn violation(name: &str, body: &syn::Block) -> Option<String> {
    struct Scan<'a> {
        bound: &'a BTreeSet<String>,
        appends: Vec<String>,
        assigns: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let method = call.method.to_string();
            if APPENDS.contains(&method.as_str()) && rooted_at_self(&call.receiver, self.bound) {
                self.appends.push(format!(".{method}(..)"));
            }
            visit::visit_expr_method_call(self, call);
        }
        fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
            let whole = match &*assign.left {
                syn::Expr::Field(_) => true,
                syn::Expr::Unary(unary) => matches!(unary.op, syn::UnOp::Deref(_)),
                _ => false,
            };
            if whole && rooted_at_self(&assign.left, self.bound) {
                self.assigns.push("a field assigned whole".to_string());
            }
            visit::visit_expr_assign(self, assign);
        }
        // A nested item has a `self` of its own.
        fn visit_item(&mut self, _: &'ast syn::Item) {}
    }
    let bound = self_bindings(body);
    let mut scan = Scan {
        bound: &bound,
        appends: Vec::new(),
        assigns: Vec::new(),
    };
    scan.visit_block(body);
    if name.starts_with("with_") {
        scan.appends.first().cloned()
    } else {
        scan.assigns.first().cloned()
    }
}

/// The names a body binds to a part of `self`, so a call or an assignment
/// through one reaches `self` as `self.x.push(..)` does: a `let`, an
/// `if let` or `while let`, a `match` arm or a `for` loop whose value is
/// rooted at `self` (`let v = self.x.get_or_insert_with(Vec::new)`,
/// `if let Transport::Stdio { env, .. } = &mut self.transport`), and the
/// parameters of a closure passed to a method called on one
/// (`self.x.iter_mut().for_each(|v| ..)`), followed through bindings of
/// bindings. Scopes are not tracked: a name bound to `self` anywhere in the
/// body counts everywhere in it.
fn self_bindings(body: &syn::Block) -> BTreeSet<String> {
    struct Bind<'a> {
        bound: &'a mut BTreeSet<String>,
    }
    impl Bind<'_> {
        fn bind(&mut self, pat: &syn::Pat, value: &syn::Expr) {
            if rooted_at_self(value, self.bound) {
                pattern_names(pat, self.bound);
            }
        }
    }
    impl<'ast> Visit<'ast> for Bind<'_> {
        fn visit_local(&mut self, local: &'ast syn::Local) {
            if let Some(init) = &local.init {
                self.bind(&local.pat, &init.expr);
            }
            visit::visit_local(self, local);
        }
        fn visit_expr_let(&mut self, expr: &'ast syn::ExprLet) {
            self.bind(&expr.pat, &expr.expr);
            visit::visit_expr_let(self, expr);
        }
        fn visit_expr_match(&mut self, expr: &'ast syn::ExprMatch) {
            for arm in &expr.arms {
                self.bind(&arm.pat, &expr.expr);
            }
            visit::visit_expr_match(self, expr);
        }
        fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
            self.bind(&expr.pat, &expr.expr);
            visit::visit_expr_for_loop(self, expr);
        }
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if rooted_at_self(&call.receiver, self.bound) {
                for arg in &call.args {
                    if let syn::Expr::Closure(closure) = arg {
                        for input in &closure.inputs {
                            pattern_names(input, self.bound);
                        }
                    }
                }
            }
            visit::visit_expr_method_call(self, call);
        }
        fn visit_item(&mut self, _: &'ast syn::Item) {}
    }
    let mut bound = BTreeSet::new();
    loop {
        let before = bound.len();
        Bind { bound: &mut bound }.visit_block(body);
        if bound.len() == before {
            return bound;
        }
    }
}

/// Every name a pattern binds (`env` in `Transport::Stdio { env, .. }`,
/// `map` in `Value::Object(ref mut map)`).
fn pattern_names(pat: &syn::Pat, out: &mut BTreeSet<String>) {
    struct Names<'a>(&'a mut BTreeSet<String>);
    impl<'ast> Visit<'ast> for Names<'_> {
        fn visit_pat_ident(&mut self, ident: &'ast syn::PatIdent) {
            self.0.insert(ident.ident.to_string());
            visit::visit_pat_ident(self, ident);
        }
    }
    Names(out).visit_pat(pat);
}

/// Whether an expression reaches its value through `self` or a name bound
/// to a part of it ([`self_bindings`]): `self.x`, `self.x.y`,
/// `self.x.get_or_insert_with(..)`, `&mut self.x`, `env` after
/// `if let Stdio { env, .. } = &mut self.transport`.
fn rooted_at_self(expr: &syn::Expr, bound: &BTreeSet<String>) -> bool {
    match expr {
        syn::Expr::Path(path) => {
            path.qself.is_none()
                && path
                    .path
                    .get_ident()
                    .is_some_and(|ident| ident == "self" || bound.contains(&ident.to_string()))
        }
        syn::Expr::Field(field) => rooted_at_self(&field.base, bound),
        syn::Expr::MethodCall(call) => rooted_at_self(&call.receiver, bound),
        syn::Expr::Reference(reference) => rooted_at_self(&reference.expr, bound),
        syn::Expr::Paren(paren) => rooted_at_self(&paren.expr, bound),
        syn::Expr::Index(index) => rooted_at_self(&index.expr, bound),
        syn::Expr::Unary(unary) => rooted_at_self(&unary.expr, bound),
        syn::Expr::Try(attempt) => rooted_at_self(&attempt.expr, bound),
        _ => false,
    }
}

#[test]
fn the_scan_detects_what_it_claims_to() {
    let file: syn::File = syn::parse_str(
        r#"
        impl AgentBuilder {
            // I-10's first rename, reverted: a with_ that pushes.
            pub fn with_tool_service(mut self, service: Arc<dyn ToolService>) -> Self {
                self.tool_services.push(service);
                self
            }
            // ... and through an Option.
            pub fn with_tool(mut self, tool: Tool) -> Self {
                self.tools.get_or_insert_with(Vec::new).push(tool);
                self
            }
            // A with_ that builds a collection and assigns it replaces.
            pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
                let mut all = Vec::new();
                all.extend(tools);
                self.tools = Some(all);
                self
            }
            // An add_ that appends.
            pub fn add_header(mut self, name: String, value: String) -> Self {
                self.config.headers.insert(name, value);
                self
            }
            // An add_ that replaces.
            pub fn add_model(mut self, model: String) -> Self {
                self.model = Some(model);
                self
            }
        }
        impl CreateFileSearchStoreRequest {
            // I-10's second rename, reverted: a with_ that inserts.
            pub fn with_extra(mut self, key: impl Into<String>, value: Value) -> Self {
                self.extra.insert(key.into(), value);
                self
            }
        }
        impl McpServer {
            // Appends through a binding of a self field, in each form.
            pub fn with_env(mut self, key: String, value: String) -> Self {
                if let McpTransport::Stdio { env, .. } = &mut self.transport {
                    env.insert(key, value);
                }
                self
            }
            pub fn with_header(mut self, key: String, value: String) -> Self {
                match &mut self.transport {
                    McpTransport::Http { headers, .. } => {
                        headers.insert(key, value);
                    }
                    McpTransport::Stdio { .. } => {}
                }
                self
            }
            pub fn with_tool(mut self, tool: String) -> Self {
                let tools = self.tools.get_or_insert_with(Vec::new);
                let all = tools;
                all.push(tool);
                self
            }
            pub fn with_property(mut self, name: String, schema: Value) -> Self {
                if let Value::Object(ref mut map) = self.properties {
                    map.insert(name, schema);
                }
                self
            }
            pub fn with_arg(mut self, arg: String) -> Self {
                for list in self.arg_lists.iter_mut() {
                    list.push(arg.clone());
                }
                self.groups.iter_mut().for_each(|group| group.push(arg.clone()));
                self
            }
            // A with_ that binds a self field and replaces it keeps the
            // convention; so does one that pushes onto a local of its own.
            pub fn with_timeout(mut self, timeout: u64) -> Self {
                let config = self.config.get_or_insert_with(Config::default);
                config.timeout = Some(timeout);
                let mut local = Vec::new();
                local.push(timeout);
                self
            }
            // An add_ that replaces through a binding.
            pub fn add_url(mut self, url: String) -> Self {
                if let McpTransport::Http { url: slot, .. } = &mut self.transport {
                    *slot = url;
                }
                self
            }
            pub fn add_port(mut self, port: u16) -> Self {
                let config = self.config.get_or_insert_with(Config::default);
                config.port = port;
                self
            }
            // An add_ that appends through a binding keeps it.
            pub fn add_env(mut self, key: String, value: String) -> Self {
                if let McpTransport::Stdio { env, .. } = &mut self.transport {
                    env.insert(key, value);
                }
                self
            }
        }
        "#,
    )
    .unwrap();
    let items = source_scan::all_items(std::slice::from_ref(&file));
    let methods = source_scan::public_methods(&items);
    let flagged: Vec<String> = methods
        .iter()
        .flat_map(|(owner, fns)| {
            fns.iter().filter_map(move |f| {
                let name = f.sig.ident.to_string();
                violation(&name, &f.block).map(|_| format!("{owner}::{name}"))
            })
        })
        .collect();
    assert_eq!(
        flagged,
        [
            "AgentBuilder::with_tool_service",
            "AgentBuilder::with_tool",
            "AgentBuilder::add_model",
            "CreateFileSearchStoreRequest::with_extra",
            "McpServer::with_env",
            "McpServer::with_header",
            "McpServer::with_tool",
            "McpServer::with_property",
            "McpServer::with_arg",
            "McpServer::add_url",
            "McpServer::add_port",
        ]
    );
}
