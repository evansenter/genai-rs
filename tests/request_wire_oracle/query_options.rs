//! The query options of request types, for
//! `every_verb_is_called_by_the_query_test`: the public `with_*` methods
//! that set a field a verb sends on the query string, such as
//! `WebhookUpdate::with_update_mask` or `EnvironmentFileUpload`'s
//! `with_overwrite` and `with_extract`. A list builder's options are found
//! from `list_builder!` and its `impl`; these are found by following the
//! data instead, so a request type that gains one needs no list.
//!
//! The parsed `src/` is read as a call graph. Every value in a
//! `with_query(url, &[(key, value), ..])` pair is traced back through the
//! function it is in: to a parameter, and from there into each call that
//! passes it, or to a field of a request type, read on a parameter
//! (`update.update_mask`) or bound from one by a struct pattern
//! (`let EnvironmentFileUpload { overwrite, .. } = upload;`). The options
//! are the public `with_*` methods of that type's `impl` blocks that assign
//! the field. The query test must call each on a value of that type.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use syn::visit::{self, Visit};

/// A request type's field a verb sends on the query string, `(type, field)`.
pub type QueryField = (String, String);

/// Where a value in a function comes from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    /// The function's parameter at this index (a receiver counts).
    Param(usize),
    /// A field of a request type.
    Field(String, String),
}

/// A function or method: its parameters (name, the type's last ident),
/// its `impl` type, and its body.
struct Function<'a> {
    name: String,
    params: Vec<(String, Option<String>)>,
    block: &'a syn::Block,
}

fn last_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        syn::Type::Reference(r) => last_ident(&r.elem),
        syn::Type::Paren(p) => last_ident(&p.elem),
        _ => None,
    }
}

fn flatten<'a>(items: &'a [syn::Item], out: &mut Vec<&'a syn::Item>) {
    for item in items {
        if let syn::Item::Mod(m) = item
            && let Some((_, inner)) = &m.content
        {
            flatten(inner, out);
        }
        out.push(item);
    }
}

fn items(files: &[syn::File]) -> Vec<&syn::Item> {
    let mut out = Vec::new();
    for file in files {
        flatten(&file.items, &mut out);
    }
    out
}

fn params(sig: &syn::Signature, self_ty: Option<&str>) -> Vec<(String, Option<String>)> {
    sig.inputs
        .iter()
        .map(|input| match input {
            syn::FnArg::Receiver(_) => ("self".to_string(), self_ty.map(str::to_string)),
            syn::FnArg::Typed(typed) => {
                let name = match &*typed.pat {
                    syn::Pat::Ident(p) => p.ident.to_string(),
                    _ => String::new(),
                };
                (name, last_ident(&typed.ty))
            }
        })
        .collect()
}

/// Every free function and inherent or trait method in `files`.
fn functions(files: &[syn::File]) -> Vec<Function<'_>> {
    let mut out = Vec::new();
    for item in items(files) {
        match item {
            syn::Item::Fn(f) => out.push(Function {
                name: f.sig.ident.to_string(),
                params: params(&f.sig, None),
                block: &f.block,
            }),
            syn::Item::Impl(imp) => {
                let self_ty = last_ident(&imp.self_ty);
                for item in &imp.items {
                    if let syn::ImplItem::Fn(f) = item {
                        out.push(Function {
                            name: f.sig.ident.to_string(),
                            params: params(&f.sig, self_ty.as_deref()),
                            block: &f.block,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The names and fields an expression reads its value from: a bare name,
/// `base.field` on a bare name, through method calls (`x.as_deref()`,
/// `x.then_some("true")`), calls (`Some(x)`), references and parentheses.
fn roots(expr: &syn::Expr) -> Vec<(String, Option<String>)> {
    match expr {
        syn::Expr::Path(p) => p
            .path
            .get_ident()
            .map(|name| vec![(name.to_string(), None)])
            .unwrap_or_default(),
        syn::Expr::Field(f) => match (&*f.base, &f.member) {
            (syn::Expr::Path(base), syn::Member::Named(field))
                if base.path.get_ident().is_some() =>
            {
                let base = base
                    .path
                    .get_ident()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                vec![(base, Some(field.to_string()))]
            }
            _ => roots(&f.base),
        },
        syn::Expr::MethodCall(call) => roots(&call.receiver),
        syn::Expr::Call(call) => call.args.iter().flat_map(roots).collect(),
        syn::Expr::Reference(r) => roots(&r.expr),
        syn::Expr::Paren(p) => roots(&p.expr),
        _ => Vec::new(),
    }
}

/// What a function's names stand for: its parameters, and each local a
/// `let` binds, from its initializer or a struct pattern's field.
struct Bindings {
    names: HashMap<String, Vec<Origin>>,
    types: HashMap<String, String>,
}

impl Bindings {
    fn of(function: &Function<'_>) -> Self {
        let mut bindings = Self {
            names: HashMap::new(),
            types: HashMap::new(),
        };
        for (i, (name, ty)) in function.params.iter().enumerate() {
            bindings.names.insert(name.clone(), vec![Origin::Param(i)]);
            if let Some(ty) = ty {
                bindings.types.insert(name.clone(), ty.clone());
            }
        }
        struct Lets<'b>(&'b mut Bindings);
        impl Visit<'_> for Lets<'_> {
            fn visit_local(&mut self, local: &syn::Local) {
                if let Some(init) = &local.init {
                    self.visit_expr(&init.expr);
                }
                match &local.pat {
                    syn::Pat::Struct(s) => {
                        let ty = s.path.segments.last().map(|seg| seg.ident.to_string());
                        for field in &s.fields {
                            if let (Some(ty), syn::Member::Named(member), syn::Pat::Ident(bound)) =
                                (&ty, &field.member, &*field.pat)
                            {
                                self.0.names.insert(
                                    bound.ident.to_string(),
                                    vec![Origin::Field(ty.clone(), member.to_string())],
                                );
                            }
                        }
                    }
                    syn::Pat::Ident(p) => {
                        let origins = local
                            .init
                            .as_ref()
                            .map(|init| self.0.resolve(&init.expr))
                            .unwrap_or_default();
                        self.0.names.insert(p.ident.to_string(), origins);
                    }
                    _ => {}
                }
            }
        }
        Lets(&mut bindings).visit_block(function.block);
        bindings
    }

    /// Where the value of `expr` comes from, in this function.
    fn resolve(&self, expr: &syn::Expr) -> Vec<Origin> {
        let mut out = Vec::new();
        for (name, field) in roots(expr) {
            match field {
                None => out.extend(self.names.get(&name).into_iter().flatten().cloned()),
                Some(field) => {
                    if let Some(ty) = self.types.get(&name) {
                        out.push(Origin::Field(ty.clone(), field));
                    }
                }
            }
        }
        out
    }
}

/// Every call in a function body, macros included: the callee's name and
/// its arguments (a method call's receiver first, as its parameter 0).
fn calls(block: &syn::Block) -> Vec<(String, Vec<syn::Expr>)> {
    struct Calls(Vec<(String, Vec<syn::Expr>)>);
    impl Visit<'_> for Calls {
        fn visit_expr_call(&mut self, call: &syn::ExprCall) {
            if let syn::Expr::Path(p) = &*call.func
                && let Some(last) = p.path.segments.last()
            {
                self.0
                    .push((last.ident.to_string(), call.args.iter().cloned().collect()));
            }
            visit::visit_expr_call(self, call);
        }
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            let args = std::iter::once((*call.receiver).clone())
                .chain(call.args.iter().cloned())
                .collect();
            self.0.push((call.method.to_string(), args));
            visit::visit_expr_method_call(self, call);
        }
        fn visit_macro(&mut self, mac: &syn::Macro) {
            let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
            if let Ok(exprs) = mac.parse_body_with(parser) {
                for expr in exprs {
                    self.visit_expr(&expr);
                }
            }
        }
    }
    let mut found = Calls(Vec::new());
    found.visit_block(block);
    found.0
}

/// Every request-type field a `with_query` pair's value comes from.
pub fn query_fields(files: &[syn::File]) -> BTreeSet<QueryField> {
    let functions = functions(files);
    let bindings: Vec<Bindings> = functions.iter().map(Bindings::of).collect();
    let calls: Vec<Vec<(String, Vec<syn::Expr>)>> =
        functions.iter().map(|f| calls(f.block)).collect();

    let mut fields = BTreeSet::new();
    // A parameter a query value comes from: `(function, index)`.
    let mut queue: Vec<(String, usize)> = Vec::new();
    let mut take = |origins: Vec<Origin>, function: &str, queue: &mut Vec<(String, usize)>| {
        for origin in origins {
            match origin {
                Origin::Param(i) => queue.push((function.to_string(), i)),
                Origin::Field(ty, field) => {
                    fields.insert((ty, field));
                }
            }
        }
    };
    for (i, function) in functions.iter().enumerate() {
        for (callee, args) in &calls[i] {
            if callee != "with_query" {
                continue;
            }
            let Some(syn::Expr::Reference(pairs)) = args.get(1) else {
                continue;
            };
            let syn::Expr::Array(pairs) = &*pairs.expr else {
                continue;
            };
            for pair in &pairs.elems {
                if let syn::Expr::Tuple(pair) = pair
                    && let Some(value) = pair.elems.iter().nth(1)
                {
                    take(bindings[i].resolve(value), &function.name, &mut queue);
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    while let Some((callee, index)) = queue.pop() {
        if !seen.insert((callee.clone(), index)) {
            continue;
        }
        for (i, caller) in functions.iter().enumerate() {
            for (name, args) in &calls[i] {
                // A method call's arguments start at its receiver, so they
                // line up with a method's parameters; a path call to a
                // method (`Self::f(x)`) passes the receiver first too.
                if *name == callee
                    && let Some(arg) = args.get(index)
                {
                    take(bindings[i].resolve(arg), &caller.name, &mut queue);
                }
            }
        }
    }
    fields
}

/// The public `with_*` methods of each type's `impl` blocks that assign
/// one of `fields` (`self.update_mask = ..`), as `(type, method)`.
pub fn query_options(
    files: &[syn::File],
    fields: &BTreeSet<QueryField>,
) -> BTreeSet<(String, String)> {
    struct Assigns(BTreeSet<String>);
    impl Visit<'_> for Assigns {
        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if let syn::Expr::Field(f) = &*assign.left
                && let syn::Expr::Path(base) = &*f.base
                && base.path.is_ident("self")
                && let syn::Member::Named(field) = &f.member
            {
                self.0.insert(field.to_string());
            }
            visit::visit_expr_assign(self, assign);
        }
    }
    let mut options = BTreeSet::new();
    for item in items(files) {
        let syn::Item::Impl(imp) = item else { continue };
        let Some(ty) = last_ident(&imp.self_ty).filter(|_| imp.trait_.is_none()) else {
            continue;
        };
        for item in &imp.items {
            let syn::ImplItem::Fn(f) = item else { continue };
            let name = f.sig.ident.to_string();
            if !name.starts_with("with_")
                || f.sig.receiver().is_none()
                || !matches!(f.vis, syn::Visibility::Public(_))
            {
                continue;
            }
            let mut assigns = Assigns(BTreeSet::new());
            assigns.visit_block(&f.block);
            if assigns
                .0
                .iter()
                .any(|field| fields.contains(&(ty.clone(), field.clone())))
            {
                options.insert((ty.clone(), name));
            }
        }
    }
    options
}

/// The public methods of each type's `impl` blocks that return the type
/// (`Self`): a chain through them keeps the type.
fn self_returning(files: &[syn::File]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in items(files) {
        let syn::Item::Impl(imp) = item else { continue };
        let Some(ty) = last_ident(&imp.self_ty).filter(|_| imp.trait_.is_none()) else {
            continue;
        };
        for item in &imp.items {
            let syn::ImplItem::Fn(f) = item else { continue };
            if let syn::ReturnType::Type(_, ret) = &f.sig.output
                && last_ident(ret).is_some_and(|ret| ret == "Self" || ret == ty)
            {
                out.entry(ty.clone())
                    .or_default()
                    .insert(f.sig.ident.to_string());
            }
        }
    }
    out
}

/// The `(type, method)` pairs of `options` the function `test` in `file`
/// calls on a value of that type: one a constructor makes (`T::new(..)`),
/// a local bound to one, or a chain through `T`'s methods that return it.
pub fn query_options_set(
    file: &syn::File,
    test: &str,
    src: &[syn::File],
    options: &BTreeSet<(String, String)>,
) -> BTreeSet<(String, String)> {
    struct Set<'a> {
        keeps: &'a BTreeMap<String, BTreeSet<String>>,
        options: &'a BTreeSet<(String, String)>,
        locals: HashMap<String, String>,
        found: BTreeSet<(String, String)>,
    }
    impl Set<'_> {
        fn type_of(&self, expr: &syn::Expr) -> Option<String> {
            match expr {
                syn::Expr::Call(call) => match &*call.func {
                    syn::Expr::Path(p) if p.path.segments.len() >= 2 => {
                        let segments = &p.path.segments;
                        let ty = segments[segments.len() - 2].ident.to_string();
                        self.keeps.contains_key(&ty).then_some(ty)
                    }
                    _ => None,
                },
                syn::Expr::MethodCall(call) => self
                    .type_of(&call.receiver)
                    .filter(|ty| self.keeps[ty].contains(&call.method.to_string())),
                syn::Expr::Path(p) => p
                    .path
                    .get_ident()
                    .and_then(|name| self.locals.get(&name.to_string()).cloned()),
                syn::Expr::Reference(r) => self.type_of(&r.expr),
                syn::Expr::Paren(p) => self.type_of(&p.expr),
                _ => None,
            }
        }
    }
    impl Visit<'_> for Set<'_> {
        fn visit_local(&mut self, local: &syn::Local) {
            if let Some(init) = &local.init {
                self.visit_expr(&init.expr);
            }
            if let syn::Pat::Ident(p) = &local.pat {
                let name = p.ident.to_string();
                match local
                    .init
                    .as_ref()
                    .and_then(|init| self.type_of(&init.expr))
                {
                    Some(ty) => self.locals.insert(name, ty),
                    None => self.locals.remove(&name),
                };
            }
        }
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if let Some(ty) = self.type_of(&call.receiver) {
                let key = (ty, call.method.to_string());
                if self.options.contains(&key) {
                    self.found.insert(key);
                }
            }
            visit::visit_expr_method_call(self, call);
        }
    }
    let function = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == test => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no function {test}"));
    let keeps = self_returning(src);
    let mut set = Set {
        keeps: &keeps,
        options,
        locals: HashMap::new(),
        found: BTreeSet::new(),
    };
    set.visit_item_fn(function);
    set.found
}

/// A query field is found through a parameter, a struct pattern and two
/// calls, its options are the `with_*` methods that assign it (not one that
/// sets a body field), and the test's calls count on a chain or a local of
/// that type only.
#[test]
fn request_type_query_options_are_found_through_the_calls() {
    let src: syn::File = syn::parse_quote! {
        pub struct Update { pub name: Option<String>, pub mask: Option<String> }
        impl Update {
            pub fn new() -> Self { todo!() }
            pub fn with_name(mut self, name: &str) -> Self { self.name = Some(name.into()); self }
            pub fn with_mask(mut self, mask: &str) -> Self { self.mask = Some(mask.into()); self }
        }
        pub struct Upload { pub data: Vec<u8>, pub overwrite: bool }
        impl Upload {
            pub fn new(data: Vec<u8>) -> Self { todo!() }
            pub fn with_overwrite(mut self, overwrite: bool) -> Self { self.overwrite = overwrite; self }
        }
        impl Things<'_> {
            pub async fn update(self, id: &str, update: &Update) -> Result<(), GenaiError> {
                update_thing(&self.client.http, id, update, update.mask.as_deref()).await
            }
            pub async fn upload(self, upload: Upload) -> Result<(), GenaiError> {
                upload_thing(&self.client.http, upload).await
            }
            pub async fn delete(self, id: &str) -> Result<(), GenaiError> {
                delete_thing(&self.client.http, id, false).await
            }
        }
        async fn update_thing(ctx: &HttpContext, id: &str, update: &Update, mask: Option<&str>) {
            let url = with_query(thing_url(ctx, id), &[("update_mask", mask)]);
        }
        async fn upload_thing(ctx: &HttpContext, upload: Upload) {
            let Upload { data, overwrite } = upload;
            let url = start_url(ctx, overwrite)?;
        }
        fn start_url(ctx: &HttpContext, overwrite: bool) -> String {
            with_query(ctx.upload_url("things"), &[("overwrite", overwrite.then_some("true"))])
        }
        async fn delete_thing(ctx: &HttpContext, id: &str, force: bool) {
            let url = with_query(thing_url(ctx, id), &[("force", force.then_some("true"))]);
        }
    };
    let src = [src];
    let fields = query_fields(&src);
    assert_eq!(
        fields,
        BTreeSet::from([
            ("Update".to_string(), "mask".to_string()),
            ("Upload".to_string(), "overwrite".to_string()),
        ])
    );
    let options = query_options(&src, &fields);
    assert_eq!(
        options,
        BTreeSet::from([
            ("Update".to_string(), "with_mask".to_string()),
            ("Upload".to_string(), "with_overwrite".to_string()),
        ])
    );

    let test: syn::File = syn::parse_quote! {
        async fn the_test() {
            client.things().update("t", &Update::new().with_name("n").with_mask("name")).await;
            let upload = Upload::new(vec![1]);
            other.with_overwrite(true);
            client.things().upload(upload).await;
        }
    };
    assert_eq!(
        query_options_set(&test, "the_test", &src, &options),
        BTreeSet::from([("Update".to_string(), "with_mask".to_string())]),
        "with_overwrite is called on a value of unknown type, so it does not count"
    );
}
