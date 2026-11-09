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
//!
//! A value built by a constructor escapes that rule: `CustomMetadata::string`
//! sets one field and fills the rest from `Default`, and even one written as
//! a full literal (`Tool::code_execution()`) sets a new field to whatever
//! its author chose there, out of the oracle's sight. So the scan also fails
//! on a call to an associated function of a crate type (`Type::name(..)`,
//! or `Type::name` passed as a value, which begins every builder chain) in
//! the oracle's own files, unless [`TRUSTED`] lists it with the reason it
//! sets every field. The strategies are sampled for their union arms and
//! enum values, which a constructor call does not hide; the literals here
//! cover the fields.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;

use super::super::Allowed;

/// Constructors the oracle may call, each with the reason the value it
/// builds still sets every field: the type's fields are private, so the
/// full literal sits beside the type, and a check catches a field it
/// leaves sending no key; or it destructures every field of a full literal
/// by name. Any other `Type::name(..)` of a crate type in the oracle fails
/// [`request_values_are_full_literals`].
pub(in crate::coverage_oracle) const TRUSTED: &[Allowed] = &[
    ("AntigravityConfig::fully_populated", AGENT_CONFIG),
    ("CodeMenderConfig::fully_populated", AGENT_CONFIG),
    ("DeepResearchConfig::fully_populated", AGENT_CONFIG),
    ("DynamicConfig::fully_populated", AGENT_CONFIG),
    (
        "FunctionParameters::new",
        "The fields are private to src/tools/function.rs, so the oracle cannot write \
         the literal. `new` is one: it sets each field from its arguments and leaves \
         only `extra`, the caller's keys, empty; \
         `function_parameters_set_every_field` fails on a field the oracle's value \
         leaves sending no key.",
    ),
    (
        "WebhookInput::from",
        "The create body `Webhooks::create` sends: it destructures every field of the \
         oracle's full `Webhook` literal by name, with no `..`, so a field added to \
         `Webhook` fails to compile there until it is sent or named output only.",
    ),
];

/// Why the agent configs' `fully_populated()` is trusted.
const AGENT_CONFIG: &str = "The config's fields are private, so its full literal sits \
     beside the type (`fully_populated()` in src/request/agent_config.rs), and \
     `agent_configs_set_every_field` fails on a field it leaves sending no key.";

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
    /// Every `Type::name` path: an associated function, called or passed
    /// (a type's name is CamelCase, a function's snake_case, so a variant,
    /// `Tool::CodeExecution`, and a constant are not one).
    associated: BTreeSet<String>,
}

impl Scan {
    fn new(within: impl Into<String>) -> Self {
        Self {
            within: within.into(),
            fills: Vec::new(),
            names: BTreeSet::new(),
            strategies: BTreeSet::new(),
            associated: BTreeSet::new(),
        }
    }
}

/// `Type::name` for a path whose last two segments are a type (CamelCase)
/// and a function (snake_case).
fn associated_function(path: &syn::Path) -> Option<String> {
    let mut segments = path.segments.iter().rev();
    let (function, ty) = (segments.next()?, segments.next()?);
    let (function, ty) = (function.ident.to_string(), ty.ident.to_string());
    let starts = |name: &str, upper: bool| {
        name.chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() == upper && c.is_ascii_alphabetic())
    };
    (starts(&ty, true) && starts(&function, false)).then(|| format!("{ty}::{function}"))
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
        self.associated.extend(associated_function(&node.path));
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

/// What the scan found: the fills, the strategies it followed, and every
/// associated function the oracle's own files name.
struct Found {
    fills: Vec<String>,
    reached: BTreeSet<String>,
    associated: BTreeSet<String>,
}

/// The fills in `oracle` (every item), and in the strategies of
/// `strategy_files` that `oracle` samples, followed through every function
/// of `strategy_files` they name. Also the strategies reached, so a caller
/// can tell the scan saw them, and the associated functions the oracle
/// names.
fn fills_in(oracle: &[syn::File], strategy_files: &[syn::File]) -> Found {
    let mut fills = Vec::new();
    let mut roots = BTreeSet::new();
    let mut associated = BTreeSet::new();
    for file in oracle {
        let mut scan = Scan::new("the oracle");
        scan.visit_file(file);
        fills.append(&mut scan.fills);
        roots.append(&mut scan.strategies);
        associated.append(&mut scan.associated);
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
    Found {
        fills,
        reached,
        associated,
    }
}

/// The calls among `associated` to a constructor of one of `types` that
/// `trusted` does not list, and the `trusted` entries none of them matched.
fn untrusted_constructors(
    associated: &BTreeSet<String>,
    types: &BTreeSet<String>,
    trusted: &[Allowed],
) -> (Vec<String>, Vec<String>) {
    let constructors: BTreeSet<&String> = associated
        .iter()
        .filter(|path| {
            path.split_once("::")
                .is_some_and(|(ty, _)| types.contains(ty))
        })
        .collect();
    let untrusted = constructors
        .iter()
        .filter(|path| !trusted.iter().any(|(entry, _)| entry == *path))
        .map(|path| {
            format!(
                "the oracle: `{path}` builds a value through a constructor, which can \
                 leave a new field unset"
            )
        })
        .collect();
    let stale = trusted
        .iter()
        .filter(|(entry, _)| !constructors.iter().any(|path| path == entry))
        .map(|(entry, _)| format!("TRUSTED: {entry} is called nowhere; delete the entry"))
        .collect();
    (untrusted, stale)
}

/// The names of the structs, enums and type aliases `files` define, at any
/// depth outside a `#[cfg(test)]` module: the crate's types.
fn type_names(files: &[syn::File]) -> BTreeSet<String> {
    fn is_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Meta>()
                    .is_ok_and(|meta| meta.path().is_ident("test"))
        })
    }
    fn collect(items: &[syn::Item], out: &mut BTreeSet<String>) {
        for item in items {
            match item {
                syn::Item::Struct(s) => {
                    out.insert(s.ident.to_string());
                }
                syn::Item::Enum(e) => {
                    out.insert(e.ident.to_string());
                }
                syn::Item::Type(t) => {
                    out.insert(t.ident.to_string());
                }
                syn::Item::Mod(m) if !is_test(&m.attrs) => {
                    if let Some((_, inner)) = &m.content {
                        collect(inner, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = BTreeSet::new();
    for file in files {
        collect(&file.items, &mut out);
    }
    out
}

/// The `.rs` files under `dir`, recursively, that `keep` accepts, sorted.
fn rust_files(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            if keep(&path) {
                out.extend(rust_files(&path, keep));
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") && keep(&path) {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The library's own source files: `src/` without the oracle, the unit-test
/// files and the property tests.
fn library_files(src: &Path) -> Vec<PathBuf> {
    rust_files(src, &|path| {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        !(name == "coverage_oracle"
            || name == "tests.rs"
            || name.ends_with("_tests.rs")
            || name.starts_with("proptest_"))
    })
}

fn parse(path: &Path) -> syn::File {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    syn::parse_file(&source).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn request_values_are_full_literals() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let oracle_paths = rust_files(&src.join("coverage_oracle/request"), &|_| true);
    let strategy_paths: Vec<PathBuf> = rust_files(&src, &|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("proptest_") && name.ends_with(".rs"))
    });
    let oracle: Vec<syn::File> = oracle_paths.iter().map(|p| parse(p)).collect();
    let strategies: Vec<syn::File> = strategy_paths.iter().map(|p| parse(p)).collect();
    let library: Vec<syn::File> = library_files(&src).iter().map(|p| parse(p)).collect();
    let types = type_names(&library);
    for ty in [
        "CustomMetadata",
        "EnvVar",
        "Tool",
        "VideoProcessing",
        "Webhook",
    ] {
        assert!(types.contains(ty), "the type scan missed {ty}");
    }

    let found = fills_in(&oracle, &strategies);
    for strategy in [
        "arb_generation_config",
        "arb_allowed_tools",
        "arb_video_config",
        "arb_webhook_config",
        "arb_environment_source",
    ] {
        assert!(
            found.reached.contains(strategy),
            "the scan did not reach {strategy}: it follows {:?}",
            found.reached
        );
    }
    assert!(
        found.fills.is_empty(),
        "request values the coverage oracle checks leave fields to `default()` (write \
         every field, `extra: Default::default()` included, so a new field fails to \
         compile until it is set):\n  {}",
        found.fills.join("\n  ")
    );
    let (untrusted, stale) = untrusted_constructors(&found.associated, &types, TRUSTED);
    assert!(
        untrusted.is_empty() && stale.is_empty(),
        "request values the coverage oracle builds through a constructor (write the \
         struct literal, or list the constructor in TRUSTED with the reason it sets \
         every field):\n  {}",
        untrusted
            .iter()
            .chain(&stale)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// `FunctionParameters::new` is trusted because the type's fields are
/// private: every field of the value the oracle builds with it names a key
/// it sends, so a field `new` leaves at a value that sends nothing fails
/// here. `type_` goes out as `type`; `extra` holds the caller's keys and is
/// empty, as everywhere in the oracle.
#[test]
fn function_parameters_set_every_field() {
    let tools = super::content::tools();
    let Some(crate::tools::Tool::Function {
        parameters: Some(parameters),
        ..
    }) = tools.first()
    else {
        panic!("the oracle's first tool is a function with parameters: {tools:?}");
    };
    let debug = format!("{parameters:?}").replacen("type_: ", "type: ", 1);
    let emitted = serde_json::to_value(parameters).expect("parameters serialize");
    let unsent: Vec<String> = super::unsent_config_fields(&debug, &emitted)
        .into_iter()
        .filter(|field| field != "extra")
        .collect();
    assert!(
        unsent.is_empty(),
        "{unsent:?} send no key: {debug} emits {emitted}"
    );
    assert!(debug.contains("extra: {}"), "{debug}");
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
    let Found { fills, reached, .. } = fills_in(&[oracle], &[strategy_files]);
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

/// A constructor of a crate type in the oracle is a finding unless trusted,
/// called or passed as a value; a variant, a constant, a foreign type's
/// function and a strategy's constructor are not; and a trusted entry
/// nothing calls is stale. The `CustomMetadata` mutation the scan exists
/// for: a field added to it is left to `Default` by `CustomMetadata::string`.
#[test]
fn a_constructor_call_in_the_oracle_is_found() {
    let oracle: syn::File = syn::parse_quote! {
        fn values() -> Vec<Value> {
            let metadata = vec![CustomMetadata::string("team", "search")];
            let env: Vec<EnvVar> = ids.into_iter().map(EnvVar::credential).collect();
            let tool = Tool::code_execution();
            let variant = Tool::CodeExecution { extra: serde_json::Map::new() };
            let constant = ServiceTier::DEFAULT_TIER;
            let processing = VideoProcessing::segment().with_fps(2.0).build();
            samples(strategies::arb_config(), 8)
        }
    };
    let strategy_files: syn::File = syn::parse_quote! {
        fn arb_config() -> impl Strategy<Value = Config> {
            Just(Config::new())
        }
    };
    let found = fills_in(&[oracle], &[strategy_files]);
    let types: BTreeSet<String> = ["Config", "CustomMetadata", "EnvVar", "ServiceTier", "Tool"]
        .into_iter()
        .chain(["VideoProcessing"])
        .map(String::from)
        .collect();
    let trusted: &[Allowed] = &[
        ("Tool::code_execution", "A full literal."),
        ("Tool::url_context", "A full literal."),
    ];
    let (untrusted, stale) = untrusted_constructors(&found.associated, &types, trusted);
    let named = |path: &str| {
        format!(
            "the oracle: `{path}` builds a value through a constructor, which can leave a \
             new field unset"
        )
    };
    assert_eq!(
        untrusted,
        [
            named("CustomMetadata::string"),
            named("EnvVar::credential"),
            named("VideoProcessing::segment"),
        ]
    );
    assert_eq!(
        stale,
        ["TRUSTED: Tool::url_context is called nowhere; delete the entry"]
    );
}

/// The type scan reads structs, enums and aliases at any depth, and skips a
/// test module's.
#[test]
fn crate_types_are_found_outside_test_modules() {
    let file: syn::File = syn::parse_quote! {
        pub struct Thing;
        pub(crate) enum Kind { A }
        pub type Alias = Thing;
        mod inner { pub struct Nested; }
        #[cfg(test)]
        mod tests { struct Fixture; }
    };
    assert_eq!(
        type_names(&[file]),
        BTreeSet::from(["Alias", "Kind", "Nested", "Thing"].map(String::from))
    );
}
