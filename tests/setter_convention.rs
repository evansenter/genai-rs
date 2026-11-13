//! Guards the setter convention (`docs/BUILDER_API.md`): `with_*`
//! configures a setting, so calling it twice replaces; `add_*` appends to a
//! collection.
//!
//! Every public `with_*` and `add_*` method in the library's own source is
//! read through `tests/common/source_scan.rs` (D-068). A `with_*` body that
//! pushes, inserts, extends or appends onto a collection it reaches through
//! `self` appends, so it is an `add_*`; an `add_*` body that assigns a whole
//! field through `self` replaces, so it is a `with_*`. #504's I-10 renamed
//! two that had slipped in: `AgentBuilder::with_tool_service`, which pushed,
//! and `CreateFileSearchStoreRequest::with_extra`, which inserted. A method
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
/// assignment to a field of `self`. `None` when the body keeps it.
fn violation(name: &str, body: &syn::Block) -> Option<String> {
    #[derive(Default)]
    struct Scan {
        appends: Vec<String>,
        assigns: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Scan {
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let method = call.method.to_string();
            if APPENDS.contains(&method.as_str()) && rooted_at_self(&call.receiver) {
                self.appends.push(format!(".{method}(..)"));
            }
            visit::visit_expr_method_call(self, call);
        }
        fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
            if matches!(&*assign.left, syn::Expr::Field(_)) && rooted_at_self(&assign.left) {
                self.assigns.push("a field assigned whole".to_string());
            }
            visit::visit_expr_assign(self, assign);
        }
        // A nested item has a `self` of its own.
        fn visit_item(&mut self, _: &'ast syn::Item) {}
    }
    let mut scan = Scan::default();
    scan.visit_block(body);
    if name.starts_with("with_") {
        scan.appends.first().cloned()
    } else {
        scan.assigns.first().cloned()
    }
}

/// Whether an expression reaches its value through `self`: `self.x`,
/// `self.x.y`, `self.x.get_or_insert_with(..)`, `&mut self.x`.
fn rooted_at_self(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(path) => path.path.is_ident("self"),
        syn::Expr::Field(field) => rooted_at_self(&field.base),
        syn::Expr::MethodCall(call) => rooted_at_self(&call.receiver),
        syn::Expr::Reference(reference) => rooted_at_self(&reference.expr),
        syn::Expr::Paren(paren) => rooted_at_self(&paren.expr),
        syn::Expr::Index(index) => rooted_at_self(&index.expr),
        syn::Expr::Unary(unary) => rooted_at_self(&unary.expr),
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
        ]
    );
}
