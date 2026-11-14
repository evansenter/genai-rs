//! The query options of request types, for
//! `every_verb_is_called_by_the_query_test`: the public `with_*` methods
//! that set a field a verb sends on the query string, such as
//! `WebhookUpdate::with_update_mask` or `EnvironmentFileUpload`'s
//! `with_overwrite` and `with_extract`. A list builder's options are found
//! from `list_builder!` and its `impl`; these are found by following the
//! data instead, so a request type that gains one needs no list.
//!
//! The parsed `src/` is read as a call graph. Every value a `with_query`
//! call sends (each pair's value in a `&[(key, value), ..]`, or the whole
//! pairs argument when it is built elsewhere) is traced back through the
//! function it is in: through `let`, `if let`, `match` and `for` patterns
//! and the closures a method is given, to a parameter, and from there into
//! each call that passes it, or to a field of a request type. A field is
//! read on a value whose type is known (`update.update_mask`, a parameter
//! typed `Option<&WebhookUpdate>` included), bound from one by a struct
//! pattern (`let EnvironmentFileUpload { overwrite, .. } = upload;`), or
//! read from `self` by a method of the type the value calls
//! (`filters.query_pairs()`). The options are the public `with_*` methods
//! of that type's `impl` blocks that assign the field. The query test must
//! call each on a value of that type.
//!
//! The trace fails closed: a value it cannot follow (a name nothing binds,
//! a closure's parameter, a macro it cannot parse, `self` used whole, or a
//! whole request type passed along rather than one of its fields) is
//! reported in [`Trace::unmodeled`], and the query test fails on it, so a
//! query option built some new way is taught to the trace rather than
//! missed.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use syn::visit::{self, Visit};

use crate::live_verbs::source_scan::{is_test_gated, item_attrs};

/// A request type's field a verb sends on the query string, `(type, field)`.
pub type QueryField = (String, String);

/// What tracing every `with_query` value found.
#[derive(Debug, Default)]
pub struct Trace {
    /// The fields of a type the values come from.
    pub fields: BTreeSet<QueryField>,
    /// The values the trace could not follow, as `function: what`: each
    /// could carry a query option the test does not set.
    pub unmodeled: BTreeSet<String>,
}

/// Where a value in a function comes from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    /// The function's parameter at this index (a receiver counts).
    Param(usize),
    /// A field of a type.
    Field(String, String),
    /// Something the trace cannot follow, described.
    Unknown(String),
}

/// A function or method: its name, the type of its `impl`, its parameters
/// (name, the type of the value), and its body.
struct Function<'a> {
    name: String,
    owner: Option<String>,
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

/// The type of the value `ty` holds: its last ident, through references and
/// an `Option` (`Option<&WebhookUpdate>` holds a `WebhookUpdate`).
fn value_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => {
            let last = p.path.segments.last()?;
            if last.ident == "Option"
                && let syn::PathArguments::AngleBracketed(args) = &last.arguments
                && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
            {
                return value_type(inner);
            }
            Some(last.ident.to_string())
        }
        syn::Type::Reference(r) => value_type(&r.elem),
        syn::Type::Paren(p) => value_type(&p.elem),
        _ => None,
    }
}

/// The items of `items` and of the modules among them, test code left out:
/// an item under a test attribute or a `cfg` that needs `test`
/// (`source_scan::is_test_gated`), which sends nothing a user's call does.
fn flatten<'a>(items: &'a [syn::Item], out: &mut Vec<&'a syn::Item>) {
    for item in items {
        if is_test_gated(item_attrs(item)) {
            continue;
        }
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
                (name, value_type(&typed.ty))
            }
        })
        .collect()
}

/// A macro body read as comma-separated expressions (`format!`, `vec!`),
/// or `None` when it is not one.
fn macro_exprs(mac: &syn::Macro) -> Option<Vec<syn::Expr>> {
    let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    mac.parse_body_with(parser)
        .ok()
        .map(|exprs| exprs.into_iter().collect())
}

/// The parsed `src/`: every free function and inherent or trait method,
/// and the request types (those with a public `with_*` method).
struct Program<'a> {
    functions: Vec<Function<'a>>,
    request_types: BTreeSet<String>,
}

impl<'a> Program<'a> {
    fn new(files: &'a [syn::File]) -> Self {
        let mut functions = Vec::new();
        let mut request_types = BTreeSet::new();
        for item in items(files) {
            match item {
                syn::Item::Fn(f) => functions.push(Function {
                    name: f.sig.ident.to_string(),
                    owner: None,
                    params: params(&f.sig, None),
                    block: &f.block,
                }),
                syn::Item::Impl(imp) => {
                    let owner = last_ident(&imp.self_ty);
                    for item in &imp.items {
                        let syn::ImplItem::Fn(f) = item else { continue };
                        let name = f.sig.ident.to_string();
                        if let Some(owner) = &owner
                            && imp.trait_.is_none()
                            && name.starts_with("with_")
                            && f.sig.receiver().is_some()
                            && matches!(f.vis, syn::Visibility::Public(_))
                        {
                            request_types.insert(owner.clone());
                        }
                        functions.push(Function {
                            name,
                            owner: owner.clone(),
                            params: params(&f.sig, owner.as_deref()),
                            block: &f.block,
                        });
                    }
                }
                _ => {}
            }
        }
        Self {
            functions,
            request_types,
        }
    }

    /// Where the value of `ty`'s method `method` comes from: the fields it
    /// reads from `self`, following its calls to `ty`'s other methods. `None`
    /// when `src/` defines no such method.
    fn self_reads(
        &self,
        ty: &str,
        method: &str,
        seen: &mut BTreeSet<String>,
    ) -> Option<Vec<Origin>> {
        struct Reads<'r> {
            ty: &'r str,
            fields: BTreeSet<String>,
            methods: BTreeSet<String>,
            whole: bool,
        }
        impl Visit<'_> for Reads<'_> {
            fn visit_expr_field(&mut self, f: &syn::ExprField) {
                if let syn::Expr::Path(base) = &*f.base
                    && base.path.is_ident("self")
                    && let syn::Member::Named(field) = &f.member
                {
                    self.fields.insert(field.to_string());
                    return;
                }
                visit::visit_expr_field(self, f);
            }
            fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
                if let syn::Expr::Path(base) = &*call.receiver
                    && base.path.is_ident("self")
                {
                    self.methods.insert(call.method.to_string());
                    for arg in &call.args {
                        self.visit_expr(arg);
                    }
                    return;
                }
                visit::visit_expr_method_call(self, call);
            }
            fn visit_local(&mut self, local: &syn::Local) {
                // `let Self { a, b, .. } = self;` reads the fields it binds.
                if let syn::Pat::Struct(_) = &local.pat
                    && let Some(init) = &local.init
                    && let syn::Expr::Path(p) = &*init.expr
                    && p.path.is_ident("self")
                {
                    self.visit_pat(&local.pat);
                    return;
                }
                visit::visit_local(self, local);
            }
            fn visit_pat_struct(&mut self, s: &syn::PatStruct) {
                if s.path
                    .segments
                    .last()
                    .is_some_and(|seg| seg.ident == "Self" || seg.ident == self.ty)
                {
                    for field in &s.fields {
                        if let syn::Member::Named(member) = &field.member {
                            self.fields.insert(member.to_string());
                        }
                    }
                }
                visit::visit_pat_struct(self, s);
            }
            fn visit_expr_path(&mut self, p: &syn::ExprPath) {
                if p.path.is_ident("self") {
                    self.whole = true;
                }
            }
            fn visit_macro(&mut self, mac: &syn::Macro) {
                for expr in macro_exprs(mac).unwrap_or_default() {
                    self.visit_expr(&expr);
                }
            }
        }
        let bodies: Vec<&syn::Block> = self
            .functions
            .iter()
            .filter(|f| f.owner.as_deref() == Some(ty) && f.name == method)
            .map(|f| f.block)
            .collect();
        if bodies.is_empty() {
            return None;
        }
        if !seen.insert(method.to_string()) {
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        for body in bodies {
            let mut reads = Reads {
                ty,
                fields: BTreeSet::new(),
                methods: BTreeSet::new(),
                whole: false,
            };
            reads.visit_block(body);
            out.extend(
                reads
                    .fields
                    .into_iter()
                    .map(|field| Origin::Field(ty.to_string(), field)),
            );
            if reads.whole {
                out.push(Origin::Unknown(format!(
                    "`{ty}::{method}` uses `self` whole, not a field of it"
                )));
            }
            for called in reads.methods {
                match self.self_reads(ty, &called, seen) {
                    Some(origins) => out.extend(origins),
                    None => out.push(Origin::Unknown(format!(
                        "`{ty}::{method}` calls `self.{called}()`, which src/ does not define"
                    ))),
                }
            }
        }
        Some(out)
    }
}

/// What a function's names stand for: its parameters, and each name a
/// `let`, `if let`, `match` or `for` pattern or a closure binds.
struct Bindings {
    names: HashMap<String, Vec<Origin>>,
    types: HashMap<String, String>,
}

impl Bindings {
    fn of(function: &Function<'_>, program: &Program<'_>) -> Self {
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
        struct Patterns<'b, 'p> {
            bindings: &'b mut Bindings,
            program: &'p Program<'p>,
        }
        impl Patterns<'_, '_> {
            /// Binds `pat` to where `expr`'s value comes from.
            fn bind_to(&mut self, pat: &syn::Pat, expr: &syn::Expr) {
                let origins = self.bindings.resolve(expr, self.program);
                let ty = self.bindings.type_of(expr);
                self.bindings.bind(pat, &origins, ty.as_deref());
            }
        }
        impl Visit<'_> for Patterns<'_, '_> {
            fn visit_local(&mut self, local: &syn::Local) {
                let Some(init) = &local.init else {
                    let what = Origin::Unknown("a `let` without a value".to_string());
                    self.bindings.bind(&local.pat, &[what], None);
                    return;
                };
                self.visit_expr(&init.expr);
                if let Some((_, diverge)) = &init.diverge {
                    self.visit_expr(diverge);
                }
                self.bind_to(&local.pat, &init.expr);
            }
            fn visit_expr_let(&mut self, e: &syn::ExprLet) {
                self.visit_expr(&e.expr);
                self.bind_to(&e.pat, &e.expr);
            }
            fn visit_expr_match(&mut self, m: &syn::ExprMatch) {
                self.visit_expr(&m.expr);
                for arm in &m.arms {
                    self.bind_to(&arm.pat, &m.expr);
                    if let Some((_, guard)) = &arm.guard {
                        self.visit_expr(guard);
                    }
                    self.visit_expr(&arm.body);
                }
            }
            fn visit_expr_for_loop(&mut self, f: &syn::ExprForLoop) {
                self.visit_expr(&f.expr);
                let origins = self.bindings.resolve(&f.expr, self.program);
                self.bindings.bind(&f.pat, &origins, None);
                self.visit_block(&f.body);
            }
            // A closure a method is given takes the receiver's values
            // (`update.and_then(|u| u.update_mask.as_deref())`).
            fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
                self.visit_expr(&call.receiver);
                for arg in &call.args {
                    if let syn::Expr::Closure(closure) = arg {
                        for input in &closure.inputs {
                            self.bind_to(input, &call.receiver);
                        }
                        self.visit_expr(&closure.body);
                    } else {
                        self.visit_expr(arg);
                    }
                }
            }
            // Any other closure's parameters come from wherever it is called.
            fn visit_expr_closure(&mut self, closure: &syn::ExprClosure) {
                let what = Origin::Unknown("a parameter of a closure".to_string());
                for input in &closure.inputs {
                    self.bindings.bind(input, std::slice::from_ref(&what), None);
                }
                self.visit_expr(&closure.body);
            }
            fn visit_macro(&mut self, mac: &syn::Macro) {
                for expr in macro_exprs(mac).unwrap_or_default() {
                    self.visit_expr(&expr);
                }
            }
        }
        Patterns {
            bindings: &mut bindings,
            program,
        }
        .visit_block(function.block);
        bindings
    }

    /// Binds every name in `pat` to `origins`; a struct pattern's fields to
    /// the fields of its type. `ty` is the type of the value matched.
    fn bind(&mut self, pat: &syn::Pat, origins: &[Origin], ty: Option<&str>) {
        match pat {
            syn::Pat::Ident(p) => {
                let name = p.ident.to_string();
                self.names
                    .entry(name.clone())
                    .or_default()
                    .extend(origins.iter().cloned());
                if let Some(ty) = ty {
                    self.types.insert(name, ty.to_string());
                }
                if let Some((_, sub)) = &p.subpat {
                    self.bind(sub, origins, ty);
                }
            }
            syn::Pat::Struct(s) => {
                let named = s.path.segments.last().map(|seg| seg.ident.to_string());
                let named = match named.as_deref() {
                    Some("Self") => self.types.get("self").cloned(),
                    _ => named,
                };
                for field in &s.fields {
                    match (&named, &field.member) {
                        (Some(named), syn::Member::Named(member)) => {
                            let origin = Origin::Field(named.clone(), member.to_string());
                            self.bind(&field.pat, &[origin], None);
                        }
                        _ => self.bind(&field.pat, origins, None),
                    }
                }
            }
            syn::Pat::TupleStruct(t) => {
                for elem in &t.elems {
                    self.bind(elem, origins, ty);
                }
            }
            syn::Pat::Tuple(t) => {
                for elem in &t.elems {
                    self.bind(elem, origins, None);
                }
            }
            syn::Pat::Slice(s) => {
                for elem in &s.elems {
                    self.bind(elem, origins, None);
                }
            }
            syn::Pat::Or(o) => {
                for case in &o.cases {
                    self.bind(case, origins, ty);
                }
            }
            syn::Pat::Reference(r) => self.bind(&r.pat, origins, ty),
            syn::Pat::Paren(p) => self.bind(&p.pat, origins, ty),
            syn::Pat::Type(t) => {
                let declared = value_type(&t.ty);
                self.bind(&t.pat, origins, declared.as_deref().or(ty));
            }
            _ => {}
        }
    }

    /// The type of the value `expr` holds, when it is a name of known type
    /// seen through a reference or an `Option` accessor.
    fn type_of(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Path(p) => p
                .path
                .get_ident()
                .and_then(|name| self.types.get(&name.to_string()).cloned()),
            syn::Expr::Reference(r) => self.type_of(&r.expr),
            syn::Expr::Paren(p) => self.type_of(&p.expr),
            syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => self.type_of(&u.expr),
            syn::Expr::MethodCall(call)
                if matches!(
                    call.method.to_string().as_str(),
                    "as_ref"
                        | "as_deref"
                        | "as_mut"
                        | "clone"
                        | "cloned"
                        | "copied"
                        | "unwrap"
                        | "expect"
                ) =>
            {
                self.type_of(&call.receiver)
            }
            _ => None,
        }
    }

    /// Where the value of `expr` comes from, in this function: every name
    /// and field it reads, anywhere in it (a condition, a branch, a call's
    /// arguments), so nothing it reads is skipped.
    fn resolve(&self, expr: &syn::Expr, program: &Program<'_>) -> Vec<Origin> {
        struct Reads<'r, 'p> {
            bindings: &'r Bindings,
            program: &'r Program<'p>,
            out: Vec<Origin>,
        }
        impl Visit<'_> for Reads<'_, '_> {
            fn visit_expr_path(&mut self, p: &syn::ExprPath) {
                // A constant, a variant (`None`) or a path is not a value
                // this function was given.
                let Some(name) = p.path.get_ident().map(ToString::to_string) else {
                    return;
                };
                if name.starts_with(|c: char| c.is_uppercase()) {
                    return;
                }
                match self.bindings.names.get(&name) {
                    Some(origins) => self.out.extend(origins.iter().cloned()),
                    None => self.out.push(Origin::Unknown(format!(
                        "`{name}`, which no parameter or pattern binds"
                    ))),
                }
            }
            fn visit_expr_field(&mut self, f: &syn::ExprField) {
                if let syn::Expr::Path(base) = &*f.base
                    && let Some(name) = base.path.get_ident()
                    && let syn::Member::Named(field) = &f.member
                    && let Some(ty) = self.bindings.types.get(&name.to_string())
                {
                    self.out.push(Origin::Field(ty.clone(), field.to_string()));
                    return;
                }
                visit::visit_expr_field(self, f);
            }
            fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
                let ty = self.bindings.type_of(&call.receiver);
                if let Some(ty) = &ty
                    && let Some(origins) =
                        self.program
                            .self_reads(ty, &call.method.to_string(), &mut BTreeSet::new())
                {
                    self.out.extend(origins);
                    for arg in &call.args {
                        self.visit_expr(arg);
                    }
                    return;
                }
                // A closure given a value of known type reads it through
                // its parameters, which are bound to the receiver with its
                // type (`update.and_then(|u| u.update_mask.as_deref())`), so
                // the closure's reads stand for the receiver's.
                let through_closure = ty.is_some()
                    && call.args.iter().any(|arg| {
                        matches!(arg, syn::Expr::Closure(closure) if !closure.inputs.is_empty())
                    });
                if !through_closure {
                    self.visit_expr(&call.receiver);
                }
                for arg in &call.args {
                    self.visit_expr(arg);
                }
            }
            // A call's value comes from its arguments, and from the callee
            // when that is a local closure; a function's name is not a value.
            fn visit_expr_call(&mut self, call: &syn::ExprCall) {
                match &*call.func {
                    syn::Expr::Path(p) => {
                        if let Some(name) = p.path.get_ident()
                            && let Some(origins) = self.bindings.names.get(&name.to_string())
                        {
                            self.out.extend(origins.iter().cloned());
                        }
                    }
                    func => self.visit_expr(func),
                }
                for arg in &call.args {
                    self.visit_expr(arg);
                }
            }
            fn visit_macro(&mut self, mac: &syn::Macro) {
                match macro_exprs(mac) {
                    Some(exprs) => {
                        for expr in exprs {
                            self.visit_expr(&expr);
                        }
                    }
                    None => {
                        let name = mac.path.segments.last().map(|s| s.ident.to_string());
                        self.out.push(Origin::Unknown(format!(
                            "a `{}!` the trace cannot read",
                            name.unwrap_or_default()
                        )));
                    }
                }
            }
        }
        let mut reads = Reads {
            bindings: self,
            program,
            out: Vec::new(),
        };
        reads.visit_expr(expr);
        reads.out
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
            for expr in macro_exprs(mac).unwrap_or_default() {
                self.visit_expr(&expr);
            }
        }
    }
    let mut found = Calls(Vec::new());
    found.visit_block(block);
    found.0
}

/// The values a `with_query` call's pairs argument sends: each pair's value
/// in a `&[(key, value), ..]`, or the whole argument when it is built
/// elsewhere.
fn query_values(pairs: &syn::Expr) -> Vec<&syn::Expr> {
    if let syn::Expr::Reference(r) = pairs
        && let syn::Expr::Array(array) = &*r.expr
    {
        return array
            .elems
            .iter()
            .map(|pair| match pair {
                syn::Expr::Tuple(t) if t.elems.len() == 2 => &t.elems[1],
                other => other,
            })
            .collect();
    }
    vec![pairs]
}

/// Records `origins`, from the function at `at`: a field is found, a
/// parameter is followed into the calls that pass it (unless it is a whole
/// request type, which is reported), and anything else is reported.
fn take(
    program: &Program<'_>,
    origins: Vec<Origin>,
    at: usize,
    trace: &mut Trace,
    queue: &mut Vec<(usize, usize)>,
) {
    let function = &program.functions[at];
    for origin in origins {
        match origin {
            Origin::Param(i) => match &function.params[i] {
                (name, Some(ty)) if program.request_types.contains(ty) => {
                    trace.unmodeled.insert(format!(
                        "{}: the whole {ty} `{name}`, not a field of it",
                        function.name
                    ));
                }
                _ => queue.push((at, i)),
            },
            Origin::Field(ty, field) => {
                trace.fields.insert((ty, field));
            }
            Origin::Unknown(what) => {
                trace.unmodeled.insert(format!("{}: {what}", function.name));
            }
        }
    }
}

/// Every field a `with_query` value comes from, and what the trace could
/// not follow.
pub fn query_fields(files: &[syn::File]) -> Trace {
    let program = Program::new(files);
    let bindings: Vec<Bindings> = program
        .functions
        .iter()
        .map(|f| Bindings::of(f, &program))
        .collect();
    let calls: Vec<Vec<(String, Vec<syn::Expr>)>> =
        program.functions.iter().map(|f| calls(f.block)).collect();

    let mut trace = Trace::default();
    // A parameter a query value comes from: `(function, index)`.
    let mut queue: Vec<(usize, usize)> = Vec::new();
    for (at, calls) in calls.iter().enumerate() {
        for (callee, args) in calls {
            if callee != "with_query" {
                continue;
            }
            let Some(pairs) = args.get(1) else { continue };
            for value in query_values(pairs) {
                let origins = bindings[at].resolve(value, &program);
                take(&program, origins, at, &mut trace, &mut queue);
            }
        }
    }
    let mut seen = BTreeSet::new();
    while let Some((callee, index)) = queue.pop() {
        if !seen.insert((callee, index)) {
            continue;
        }
        let callee = &program.functions[callee].name;
        for (at, calls) in calls.iter().enumerate() {
            for (name, args) in calls {
                // A method call's arguments start at its receiver, so they
                // line up with a method's parameters; a path call to a
                // method (`Self::f(x)`) passes the receiver first too.
                if name == callee
                    && let Some(arg) = args.get(index)
                {
                    let origins = bindings[at].resolve(arg, &program);
                    take(&program, origins, at, &mut trace, &mut queue);
                }
            }
        }
    }
    trace
}

/// The options that set the query fields: the public `with_*` methods of
/// each type's `impl` blocks that assign one of `fields`
/// (`self.update_mask = ..`), as `(type, method)`. Also returns each field
/// of a type with `with_*` options that none of them assigns, as
/// `type.field`: the test could not find the option that sets it.
pub fn query_options(
    files: &[syn::File],
    fields: &BTreeSet<QueryField>,
) -> (BTreeSet<(String, String)>, Vec<String>) {
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
    let mut with_options = BTreeSet::new();
    let mut assigned = BTreeSet::new();
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
            with_options.insert(ty.clone());
            let mut assigns = Assigns(BTreeSet::new());
            assigns.visit_block(&f.block);
            for field in assigns.0 {
                let key = (ty.clone(), field);
                if fields.contains(&key) {
                    options.insert((ty.clone(), name.clone()));
                    assigned.insert(key);
                }
            }
        }
    }
    let unassigned = fields
        .iter()
        .filter(|key| with_options.contains(&key.0) && !assigned.contains(*key))
        .map(|(ty, field)| format!("{ty}.{field}"))
        .collect();
    (options, unassigned)
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
    let trace = query_fields(&src);
    assert!(trace.unmodeled.is_empty(), "{:?}", trace.unmodeled);
    assert_eq!(
        trace.fields,
        BTreeSet::from([
            ("Update".to_string(), "mask".to_string()),
            ("Upload".to_string(), "overwrite".to_string()),
        ])
    );
    let (options, unassigned) = query_options(&src, &trace.fields);
    assert!(unassigned.is_empty(), "{unassigned:?}");
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

/// The other ways a query value is built reach the field too: pairs a
/// method of the type builds and a local carries, a value chosen by `if`
/// or `match`, an `Option<&T>` parameter opened by a closure or `if let`.
#[test]
fn query_fields_are_found_however_the_value_is_built() {
    let src: syn::File = syn::parse_quote! {
        pub struct Upload { overwrite: bool, extract: bool, notify: bool, mode: Option<Mode> }
        impl Upload {
            pub fn with_overwrite(mut self, overwrite: bool) -> Self { self.overwrite = overwrite; self }
            pub fn with_extract(mut self, extract: bool) -> Self { self.extract = extract; self }
            pub fn with_notify(mut self, notify: bool) -> Self { self.notify = notify; self }
            pub fn with_mode(mut self, mode: Mode) -> Self { self.mode = Some(mode); self }
            fn query_pairs(&self) -> Vec<(&'static str, String)> {
                let mut out = Vec::new();
                if self.overwrite { out.push(("overwrite", "true".to_string())); }
                out.extend(self.extract_pair());
                out
            }
            fn extract_pair(&self) -> Option<(&'static str, String)> {
                self.extract.then(|| ("extract", "true".to_string()))
            }
        }
        pub struct Update { mask: Option<String>, validate: bool }
        impl Update {
            pub fn with_mask(mut self, mask: &str) -> Self { self.mask = Some(mask.into()); self }
            pub fn with_validate(mut self) -> Self { self.validate = true; self }
        }
        fn upload_url(ctx: &HttpContext, upload: &Upload) -> String {
            let pairs = upload.query_pairs();
            let query: Vec<(&str, Option<&str>)> = pairs
                .iter()
                .map(|(key, value)| (*key, Some(value.as_str())))
                .collect();
            let url = with_query(base(ctx), &query);
            with_query(
                url,
                &[
                    ("notify", if upload.notify { Some("true") } else { None }),
                    ("mode", match &upload.mode { Some(mode) => Some(mode.as_str()), None => None }),
                ],
            )
        }
        fn update_url(ctx: &HttpContext, update: Option<&Update>) -> String {
            let url = with_query(base(ctx), &[("mask", update.and_then(|u| u.mask.as_deref()))]);
            if let Some(u) = update {
                return with_query(url, &[("validate", u.validate.then_some("true"))]);
            }
            url
        }
    };
    let src = [src];
    let trace = query_fields(&src);
    assert!(trace.unmodeled.is_empty(), "{:?}", trace.unmodeled);
    let field = |ty: &str, field: &str| (ty.to_string(), field.to_string());
    assert_eq!(
        trace.fields,
        BTreeSet::from([
            field("Upload", "overwrite"),
            field("Upload", "extract"),
            field("Upload", "notify"),
            field("Upload", "mode"),
            field("Update", "mask"),
            field("Update", "validate"),
        ])
    );
    let (options, unassigned) = query_options(&src, &trace.fields);
    assert!(unassigned.is_empty(), "{unassigned:?}");
    assert_eq!(options.len(), 6, "{options:?}");
}

/// A query value the trace cannot follow to a field is reported, not
/// dropped: a whole request type passed to a function, a closure's
/// parameter, a name nothing binds, and a field that no `with_*` option
/// assigns.
#[test]
fn a_query_value_the_trace_cannot_follow_is_reported() {
    let src: syn::File = syn::parse_quote! {
        pub struct Upload { overwrite: bool, extract: bool }
        impl Upload {
            pub fn with_overwrite(mut self, overwrite: bool) -> Self { self.set_overwrite(overwrite); self }
            fn set_overwrite(&mut self, overwrite: bool) { self.overwrite = overwrite; }
        }
        fn whole(ctx: &HttpContext, upload: &Upload) -> String {
            with_query(base(ctx), &Upload::pairs(upload))
        }
        fn closure(ctx: &HttpContext) -> String {
            let pick = |flag: bool| flag.then_some("true");
            with_query(base(ctx), &[("x", pick(true)), ("y", pick_flag)])
        }
        fn field(ctx: &HttpContext, upload: &Upload) -> String {
            with_query(base(ctx), &[("overwrite", upload.overwrite.then_some("true"))])
        }
    };
    let src = [src];
    let trace = query_fields(&src);
    assert_eq!(
        trace.unmodeled,
        BTreeSet::from([
            "whole: the whole Upload `upload`, not a field of it".to_string(),
            "closure: a parameter of a closure".to_string(),
            "closure: `pick_flag`, which no parameter or pattern binds".to_string(),
        ])
    );
    let (_, unassigned) = query_options(&src, &trace.fields);
    assert_eq!(unassigned, ["Upload.overwrite"]);
}

/// Test code is left out of the trace: a module or an item under a `cfg`
/// that needs `test`, `all(test, unix)` as well as `test`, and a test
/// function. One under `cfg(any(test, ..))` builds outside tests, so its
/// query is traced.
#[test]
fn test_code_is_left_out_of_the_trace() {
    let src: syn::File = syn::parse_quote! {
        #[cfg(all(test, unix))]
        mod unix_tests {
            fn probe(ctx: &HttpContext) { let _ = with_query(ctx.url(), &[("probe", made_up)]); }
        }
        #[cfg(test)]
        fn helper(ctx: &HttpContext) { let _ = with_query(ctx.url(), &[("probe", made_up)]); }
        #[cfg(all(test, unix))]
        impl Probe {
            fn probe(&self, ctx: &HttpContext) { let _ = with_query(ctx.url(), &[("probe", made_up)]); }
        }
        #[tokio::test]
        async fn a_test() { let _ = with_query(url(), &[("probe", made_up)]); }
        #[cfg(any(test, feature = "x"))]
        fn shipped(ctx: &HttpContext) { let _ = with_query(ctx.url(), &[("probe", shipped_value)]); }
    };
    let trace = query_fields(&[src]);
    assert_eq!(
        trace.unmodeled,
        BTreeSet::from([
            "shipped: `shipped_value`, which no parameter or pattern binds".to_string()
        ])
    );
}
