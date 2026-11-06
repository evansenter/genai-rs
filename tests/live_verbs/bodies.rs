//! The verbs that send a request body, for `tests/request_wire_oracle.rs`:
//! those whose implementation reaches a POST, PATCH or PUT request through
//! the functions and methods it calls ([`body_verbs`]).

use std::collections::{BTreeSet, HashMap};

use syn::visit::{self, Visit};
use syn::{Expr, ImplItem, Item};

use super::{Handles, last_ident, macro_stmts, src_files};

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
