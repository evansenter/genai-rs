//! The request side's values are full struct literals: no field is left to
//! a `Default` fill, so a field added to a request type fails to compile
//! here until it is set, and so is checked.
//!
//! A source scan holds this for every file in this directory and for every
//! proptest strategy the oracle samples (`strategies::arb_*`), followed
//! through the functions it calls, into `prop_oneof!` and the other macros
//! whose arguments parse as expressions. A `default()` call (`Default::default()`,
//! `Self::default()`, `T::default()`) may be the whole value of an `extra`
//! field (`extra`, `arguments_extra`): that map holds the caller's keys,
//! which the crate never sends on its own. Anywhere else, as a struct
//! update (`..Default::default()`) or as a value (`Some(Default::default())`),
//! it fails the scan. A struct update from a full literal
//! (`..remote_environment()`) is fine: a new field fails to compile there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// A `default()` call: a path call, no arguments, last segment `default`.
fn is_default_call(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(path) = &*call.func else {
        return false;
    };
    call.args.is_empty()
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "default")
}

/// Whether a struct field may be `Default::default()`: an `extra` map.
fn is_extra_field(member: &syn::Member) -> bool {
    match member {
        syn::Member::Named(name) => {
            let name = name.to_string();
            name == "extra" || name.ends_with("_extra")
        }
        syn::Member::Unnamed(_) => false,
    }
}

/// The expressions a macro's tokens hold, when they parse as expressions
/// separated by `,`, `;` or `=>` (`prop_oneof![a, 2 => b]`, `vec![..]`);
/// nothing for a macro whose arguments are not Rust expressions (`json!`).
fn macro_exprs(mac: &syn::Macro) -> Vec<syn::Expr> {
    let parser = |input: syn::parse::ParseStream| {
        let mut exprs = Vec::new();
        while !input.is_empty() {
            exprs.push(input.parse::<syn::Expr>()?);
            if input.peek(syn::Token![=>]) {
                input.parse::<syn::Token![=>]>()?;
            } else if input.peek(syn::Token![;]) {
                input.parse::<syn::Token![;]>()?;
            } else if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }
        Ok(exprs)
    };
    syn::parse::Parser::parse2(parser, mac.tokens.clone()).unwrap_or_default()
}

/// What a scan of one item finds: the fills, and the functions it names.
struct Scan {
    /// The function the scan is in, for the messages.
    within: String,
    fills: Vec<String>,
    /// Every path's last segment: the functions it calls or names.
    names: BTreeSet<String>,
    /// Every `strategies::<name>` path: the strategies the oracle samples.
    strategies: BTreeSet<String>,
}

impl Scan {
    fn new(within: impl Into<String>) -> Self {
        Self {
            within: within.into(),
            fills: Vec::new(),
            names: BTreeSet::new(),
            strategies: BTreeSet::new(),
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        let ty = node
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        for field in &node.fields {
            if is_default_call(&field.expr) && is_extra_field(&field.member) {
                continue;
            }
            self.visit_expr(&field.expr);
        }
        match &node.rest {
            Some(rest) if is_default_call(rest) => self.fills.push(format!(
                "{}: `{ty} {{ .. }}` fills its fields from `default()`",
                self.within
            )),
            Some(rest) => self.visit_expr(rest),
            None => {}
        }
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if is_default_call(&syn::Expr::Call(node.clone())) {
            self.fills.push(format!(
                "{}: a `default()` value outside an `extra` field",
                self.within
            ));
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        let segments = &node.path.segments;
        if let Some(last) = segments.last() {
            self.names.insert(last.ident.to_string());
            if segments.len() == 2 && segments[0].ident == "strategies" {
                self.strategies.insert(last.ident.to_string());
            }
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        for expr in macro_exprs(node) {
            self.visit_expr(&expr);
        }
    }
}

fn scan_fn(item: &syn::ItemFn) -> Scan {
    let mut scan = Scan::new(item.sig.ident.to_string());
    scan.visit_item_fn(item);
    scan
}

/// The top-level functions of `files`, by name (a name defined twice keeps
/// both).
fn functions(files: &[syn::File]) -> BTreeMap<String, Vec<syn::ItemFn>> {
    let mut out: BTreeMap<String, Vec<syn::ItemFn>> = BTreeMap::new();
    for file in files {
        for item in &file.items {
            if let syn::Item::Fn(f) = item {
                out.entry(f.sig.ident.to_string())
                    .or_default()
                    .push(f.clone());
            }
        }
    }
    out
}

/// The fills in `oracle` (every item), and in the strategies of
/// `strategy_files` that `oracle` samples, followed through every function
/// of `strategy_files` they name. Also the strategies reached, so a caller
/// can tell the scan saw them.
fn fills_in(oracle: &[syn::File], strategy_files: &[syn::File]) -> (Vec<String>, BTreeSet<String>) {
    let mut fills = Vec::new();
    let mut roots = BTreeSet::new();
    for file in oracle {
        let mut scan = Scan::new("the oracle");
        scan.visit_file(file);
        fills.append(&mut scan.fills);
        roots.append(&mut scan.strategies);
    }
    let defined = functions(strategy_files);
    let mut reached = BTreeSet::new();
    let mut queue: Vec<String> = roots.into_iter().collect();
    while let Some(name) = queue.pop() {
        if !reached.insert(name.clone()) {
            continue;
        }
        for item in defined.get(&name).into_iter().flatten() {
            let mut scan = scan_fn(item);
            fills.append(&mut scan.fills);
            queue.extend(
                scan.names
                    .into_iter()
                    .filter(|called| defined.contains_key(called) && !reached.contains(called)),
            );
        }
    }
    (fills, reached)
}

fn parse(path: &Path) -> syn::File {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    syn::parse_file(&source).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn request_values_are_full_literals() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut oracle_paths: Vec<PathBuf> = std::fs::read_dir(src.join("coverage_oracle/request"))
        .expect("read src/coverage_oracle/request")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    oracle_paths.sort();
    let mut strategy_paths: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("read src")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("proptest_") && name.ends_with(".rs"))
        })
        .collect();
    strategy_paths.sort();
    let oracle: Vec<syn::File> = oracle_paths.iter().map(|p| parse(p)).collect();
    let strategies: Vec<syn::File> = strategy_paths.iter().map(|p| parse(p)).collect();

    let (fills, reached) = fills_in(&oracle, &strategies);
    for strategy in [
        "arb_generation_config",
        "arb_allowed_tools",
        "arb_video_config",
        "arb_webhook_config",
        "arb_environment_source",
    ] {
        assert!(
            reached.contains(strategy),
            "the scan did not reach {strategy}: it follows {reached:?}"
        );
    }
    assert!(
        fills.is_empty(),
        "request values the coverage oracle checks leave fields to `default()` (write \
         every field, `extra: Default::default()` included, so a new field fails to \
         compile until it is set):\n  {}",
        fills.join("\n  ")
    );
}

/// The scan finds a fill in the oracle and in a strategy it reaches through
/// a macro and a helper, and passes an `extra` default and a struct update
/// from a full literal.
#[test]
fn a_default_fill_is_found_where_the_oracle_reaches() {
    let oracle: syn::File = syn::parse_quote! {
        fn configs() -> Vec<Value> {
            let full = VideoConfig { task: Some(VideoTask::Edit), extra: Default::default() };
            let spec = RemoteEnvironment { network: None, ..remote() };
            let image = ImageConfig { aspect_ratio: None, ..Default::default() };
            samples(strategies::arb_config(), 8)
        }
    };
    let strategy_files: syn::File = syn::parse_quote! {
        fn arb_config() -> impl Strategy<Value = Config> {
            prop_oneof![
                Just(Config::Off),
                2 => arb_inner().prop_map(Config::On),
            ]
        }
        fn arb_inner() -> impl Strategy<Value = Inner> {
            arb_mode().prop_map(|mode| Inner { mode, ..Self::default() })
        }
        fn arb_mode() -> impl Strategy<Value = Mode> {
            Just(Mode { speech: Some(Default::default()), arguments_extra: Default::default() })
        }
        fn arb_unsampled() -> impl Strategy<Value = Inner> {
            Just(Inner { ..Default::default() })
        }
    };
    let (fills, reached) = fills_in(&[oracle], &[strategy_files]);
    assert_eq!(
        reached,
        BTreeSet::from(["arb_config".into(), "arb_inner".into(), "arb_mode".into()])
    );
    assert_eq!(
        fills,
        vec![
            "the oracle: `ImageConfig { .. }` fills its fields from `default()`".to_string(),
            "arb_inner: `Inner { .. }` fills its fields from `default()`".to_string(),
            "arb_mode: a `default()` value outside an `extra` field".to_string(),
        ]
    );
}
