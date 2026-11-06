//! Guards the `#[non_exhaustive]` and `extra` conventions for response
//! structs.
//!
//! Every public, deserializable struct in `src/` outside test-only code must
//! carry `#[non_exhaustive]`, so the crate can add fields the API grows
//! without a breaking change (#430 found five that had drifted). Request-side
//! types, which callers build to send, are exempt by name in `REQUEST_SIDE`.
//!
//! The same structs keep the fields they do not model in a
//! `#[serde(flatten)] extra` map (D-001), so a field the API adds survives a
//! deserialize/serialize round trip instead of being dropped. Types the
//! crate makes itself, which never come off the wire, are exempt with a
//! reason in `CRATE_MADE`.
//!
//! The source is parsed with `syn`, so attributes and `cfg` gates are read as
//! syntax: wrapped derives, multi-line gates and braces inside string
//! literals cannot mislead it. "Deserializable" means a `Deserialize` derive
//! or a hand-written `impl Deserialize for T` anywhere in non-test code.
//!
//! Not covered: types a macro generates (there is no source to parse), enums
//! (they hand-write `Deserialize` under a different convention), and
//! Serialize-only public views such as `FunctionCallInfo`.
//!
//! `src/antigravity/` is skipped: its JSON-RPC types are sent as well as
//! received, so the request/response split does not apply.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Item, Meta, Token};

/// Request-side types, deliberately exempt, keyed by repo-relative path.
///
/// `#[non_exhaustive]` on a type the caller assembles only takes away
/// struct-literal construction. Keyed by `path:Name`, not bare name, so a
/// future response type reusing a name like `WebhookConfig` is not exempted
/// by accident.
const REQUEST_SIDE: &[&str] = &[
    // Interaction request and its config tree.
    "src/request/mod.rs:InteractionRequest",
    "src/request/generation_config.rs:GenerationConfig",
    "src/request/generation_config.rs:TranscriptionConfig",
    "src/request/generation_config.rs:SpeechConfig",
    "src/request/generation_config.rs:ImageConfig",
    "src/request/generation_config.rs:VideoConfig",
    "src/request/agent_config.rs:AgentConfig",
    "src/safety.rs:SafetySetting",
    // Tool declarations and configs the caller builds.
    "src/tools/function.rs:FunctionDeclaration",
    "src/tools/function.rs:FunctionParameters",
    "src/tools/choice.rs:AllowedTools",
    "src/tools/retrieval.rs:VertexAiSearchConfig",
    "src/tools/retrieval.rs:ExaAiSearchConfig",
    "src/tools/retrieval.rs:ParallelAiSearchConfig",
    "src/tools/retrieval.rs:RagResource",
    "src/tools/retrieval.rs:HybridSearchConfig",
    "src/tools/retrieval.rs:RagFilter",
    "src/tools/retrieval.rs:RagRanking",
    "src/tools/retrieval.rs:RagRetrievalConfig",
    "src/tools/retrieval.rs:RagStoreConfig",
    // Resource create/update bodies.
    "src/environments/mod.rs:CreateEnvironmentRequest",
    "src/environments/spec.rs:EnvironmentSource",
    "src/environments/spec.rs:AllowlistEntry",
    // Passed to `with_environment`; reached only via a hand-written
    // Deserialize impl.
    "src/environments/spec.rs:RemoteEnvironment",
    "src/triggers.rs:CreateTriggerRequest",
    "src/triggers.rs:TriggerUpdate",
    "src/webhooks.rs:WebhookUpdate",
    "src/webhooks.rs:WebhookConfig",
];

/// Deserializable structs with no `extra` map, because the crate makes them
/// itself: no API response carries them, so there is no unmodeled field to
/// keep. Keyed like `REQUEST_SIDE`, each with its reason.
const CRATE_MADE: &[(&str, &str)] = &[
    (
        "src/response/views.rs:OwnedFunctionCallInfo",
        "an owned copy of a function call the crate already parsed",
    ),
    (
        "src/streaming.rs:FunctionExecutionResult",
        "a record of a function the auto-function loop ran",
    ),
    (
        "src/streaming.rs:AutoFunctionResult",
        "the auto-function loop's result; its response keeps its own extra",
    ),
    (
        "src/streaming.rs:AutoFunctionStreamEvent",
        "the crate's envelope around an auto-function chunk; the wire's \
         unmodeled fields live in the chunk's response and steps",
    ),
    (
        "src/wire_streaming/mod.rs:StreamEvent",
        "the crate's `{chunk_type, data, event_id}` envelope, made from the \
         wire event after parsing; the chunk inside keeps the unmodeled fields",
    ),
];

/// Test-only modules declared out of line; finding them proves the gate
/// detection works, since otherwise test files would be scanned as API.
const EXPECTED_TEST_MODULES: &[&str] = &[
    "src/content/content_tests",
    "src/files_tests",
    "src/paging_tests",
    "src/proptest_tests",
    "src/request/request_tests",
    "src/response/response_tests",
    "src/streaming_tests",
    "src/test_subscriber",
    "src/triggers_tests",
    "src/request_builder/tests",
];

/// `(repo-relative path, text)` of every `.rs` file in `src/` outside
/// `src/antigravity/`.
fn sources() -> Vec<(String, String)> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("src");
    // A root that cannot be read must not read as a tree that is clean.
    assert!(root.is_dir(), "scan root {} does not exist", root.display());

    let mut files = Vec::new();
    walk(&root, &mut files);
    let sources: Vec<(String, String)> = files
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&manifest)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("could not read {rel}: {e}"));
            (rel, text)
        })
        .filter(|(rel, _)| !rel.starts_with("src/antigravity/"))
        .collect();
    assert!(!sources.is_empty(), "no .rs files under {}", root.display());
    sources
}

#[test]
fn response_structs_are_non_exhaustive() {
    let report = analyze(&sources(), REQUEST_SIDE, &crate_made());

    for expected in EXPECTED_TEST_MODULES {
        assert!(
            report.test_modules.contains(*expected),
            "no `#[cfg(test)] mod` found for {expected}; found {:?}",
            report.test_modules
        );
    }

    let stale: Vec<&str> = REQUEST_SIDE
        .iter()
        .copied()
        .filter(|key| !report.exempted.contains(*key))
        .collect();
    assert!(
        stale.is_empty(),
        "These REQUEST_SIDE exemptions no longer match any type:\n  {}\n\n\
         Remove them, or update the path if the type moved.",
        stale.join("\n  ")
    );

    assert!(
        report.offenders.is_empty(),
        "These deserializable structs are missing `#[non_exhaustive]`:\n  {}\n\n\
         Response structs carry it so the crate can add fields the API grows \
         without a breaking change — see docs/ENUM_WIRE_FORMATS.md. If one of \
         these is a type callers *construct to send*, add it to REQUEST_SIDE in \
         this file with a note saying why.",
        report
            .offenders
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// Every response struct keeps unmodeled fields in a flattened map (D-001):
/// one without it drops a field the API adds on the next round trip
/// (`WhiteSpaceConfig` did, until #504's follow-ups).
#[test]
fn response_structs_keep_unmodeled_fields() {
    let crate_made = crate_made();
    let report = analyze(&sources(), REQUEST_SIDE, &crate_made);

    let stale: Vec<&str> = crate_made
        .iter()
        .copied()
        .filter(|key| !report.extra_exempted.contains(*key))
        .collect();
    assert!(
        stale.is_empty(),
        "These CRATE_MADE exemptions no longer match a struct without an \
         `extra` map:\n  {}\n\nRemove them, or update the path if the type moved.",
        stale.join("\n  ")
    );

    assert!(
        report.without_extra.is_empty(),
        "These deserializable response structs have no `#[serde(flatten)] extra` \
         map, so a field the API adds is dropped on a round trip:\n  {}\n\n\
         Add one, as `ChunkingConfig` has (D-001). If the crate makes the type \
         itself and it never comes off the wire, add it to CRATE_MADE in this \
         file with the reason.",
        report
            .without_extra
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

fn crate_made() -> Vec<&'static str> {
    CRATE_MADE.iter().map(|(key, _)| *key).collect()
}

#[derive(Debug, Default)]
struct Report {
    /// `path:Name` of each offending struct.
    offenders: BTreeSet<String>,
    /// `path:Name` of each response struct with named fields and no
    /// flattened `extra` map.
    without_extra: BTreeSet<String>,
    /// `path:Name` of each `CRATE_MADE` exemption that matched.
    extra_exempted: BTreeSet<String>,
    /// `path:Name` of each exemption that matched a deserializable struct.
    exempted: BTreeSet<String>,
    /// Out-of-line test-only modules, as extension-less repo paths.
    test_modules: BTreeSet<String>,
}

fn analyze(sources: &[(String, String)], exemptions: &[&str], crate_made: &[&str]) -> Report {
    let parsed: Vec<(&str, syn::File)> = sources
        .iter()
        .map(|(rel, text)| {
            let file =
                syn::parse_file(text).unwrap_or_else(|e| panic!("could not parse {rel}: {e}"));
            (rel.as_str(), file)
        })
        .collect();

    let mut report = Report::default();
    for (rel, file) in &parsed {
        let mut finder = GatedModules {
            dir: child_module_dir(rel),
            file_dir: Path::new(rel)
                .parent()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            found: &mut report.test_modules,
        };
        finder.visit_file(file);
    }

    let in_scope: Vec<&(&str, syn::File)> = parsed
        .iter()
        .filter(|(rel, _)| !is_test_only_path(rel, &report.test_modules))
        .collect();

    // Unioned across files: a hand-written impl need not live beside its type.
    let mut manual = BTreeSet::new();
    for (_, file) in &in_scope {
        ManualDeserialize(&mut manual).visit_file(file);
    }

    for (rel, file) in &in_scope {
        let mut check = StructCheck {
            path: rel,
            manual: &manual,
            exemptions,
            crate_made,
            report: &mut report,
        };
        check.visit_file(file);
    }
    report
}

/// True if the attributes include a `cfg` that can only hold under `test`:
/// `test` appears outside any `not(..)`. `any(test, x)` counts, as in the
/// convention this replaced.
fn is_test_gated(attrs: &[Attribute]) -> bool {
    fn mentions_test(meta: &Meta, negated: bool) -> bool {
        match meta {
            Meta::Path(path) => !negated && path.is_ident("test"),
            Meta::List(list) => {
                let negated = negated ^ list.path.is_ident("not");
                list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                    .map(|nested| nested.iter().any(|m| mentions_test(m, negated)))
                    .unwrap_or(false)
            }
            Meta::NameValue(_) => false,
        }
    }
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|meta| mentions_test(&meta, false))
    })
}

fn derives_deserialize(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths
                        .iter()
                        .any(|p| p.segments.last().is_some_and(|s| s.ident == "Deserialize"))
                })
    })
}

/// Whether the struct keeps unmodeled fields: a field named `extra`, or one
/// with `#[serde(flatten)]`.
fn has_extra(fields: &syn::Fields) -> bool {
    fields.iter().any(|field| {
        field.ident.as_ref().is_some_and(|ident| ident == "extra")
            || field.attrs.iter().any(|attr| {
                attr.path().is_ident("serde")
                    && attr
                        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                        .is_ok_and(|metas| metas.iter().any(|m| m.path().is_ident("flatten")))
            })
    })
}

fn has_non_exhaustive(attrs: &[Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().is_ident("non_exhaustive"))
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        _ => &[],
    }
}

/// Where the children of `rel`'s module live: `src/x.rs` declares into
/// `src/x/`, while `lib.rs` and `mod.rs` declare into their own directory.
fn child_module_dir(rel: &str) -> String {
    let path = Path::new(rel);
    let stem = path.file_stem().unwrap().to_string_lossy();
    let parent = path.parent().unwrap().to_string_lossy();
    if stem == "lib" || stem == "mod" || stem == "main" {
        parent.into_owned()
    } else {
        format!("{parent}/{stem}")
    }
}

/// The value of a `#[path = "..."]` attribute, if present.
fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// A file is test-only if a gated `mod` declared it: `<m>.rs`, `<m>/mod.rs`,
/// or anything below `<m>/`.
fn is_test_only_path(rel: &str, test_modules: &BTreeSet<String>) -> bool {
    test_modules
        .iter()
        .any(|m| rel == format!("{m}.rs") || rel.starts_with(&format!("{m}/")))
}

/// Collects out-of-line `#[cfg(test)] mod x;` declarations, following a
/// `#[path = "x_tests.rs"]` attribute when there is one.
struct GatedModules<'a> {
    /// Where an unattributed `mod x;` resolves (see [`child_module_dir`]).
    dir: String,
    /// The declaring file's own directory, which a top-level `#[path]` is
    /// relative to.
    file_dir: String,
    found: &'a mut BTreeSet<String>,
}

impl<'ast> Visit<'ast> for GatedModules<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.content.is_none() && is_test_gated(&module.attrs) {
            let target = match path_attr(&module.attrs) {
                Some(path) => {
                    let path = path.strip_suffix(".rs").unwrap_or(&path).to_owned();
                    format!("{}/{path}", self.file_dir)
                }
                None => format!("{}/{}", self.dir, module.ident),
            };
            self.found.insert(target);
        }
        // Inline modules are not followed: an out-of-line `mod` inside one
        // would resolve to a nested directory, and `src/` has none.
    }
}

/// Collects the self types of hand-written `impl Deserialize for T`.
struct ManualDeserialize<'a>(&'a mut BTreeSet<String>);

impl<'ast> Visit<'ast> for ManualDeserialize<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !is_test_gated(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_item_impl(&mut self, imp: &'ast syn::ItemImpl) {
        let is_deserialize = imp
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_some_and(|segment| segment.ident == "Deserialize");
        if is_deserialize
            && let syn::Type::Path(ty) = &*imp.self_ty
            && let Some(segment) = ty.path.segments.last()
        {
            self.0.insert(segment.ident.to_string());
        }
        visit::visit_item_impl(self, imp);
    }
}

/// Checks each public struct outside test-gated items.
struct StructCheck<'a> {
    path: &'a str,
    manual: &'a BTreeSet<String>,
    exemptions: &'a [&'a str],
    crate_made: &'a [&'a str],
    report: &'a mut Report,
}

impl<'ast> Visit<'ast> for StructCheck<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !is_test_gated(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        let name = item.ident.to_string();
        let public = matches!(item.vis, syn::Visibility::Public(_));
        let deserializable = derives_deserialize(&item.attrs) || self.manual.contains(&name);
        let key = format!("{}:{name}", self.path);
        let request_side = self.exemptions.contains(&key.as_str());
        if public && deserializable && !has_non_exhaustive(&item.attrs) {
            if request_side {
                self.report.exempted.insert(key.clone());
            } else {
                self.report.offenders.insert(key.clone());
            }
        }
        let named = matches!(item.fields, syn::Fields::Named(_));
        if public && deserializable && named && !request_side && !has_extra(&item.fields) {
            if self.crate_made.contains(&key.as_str()) {
                self.report.extra_exempted.insert(key);
            } else {
                self.report.without_extra.insert(key);
            }
        }
        visit::visit_item_struct(self, item);
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_scan_detects_what_it_claims_to() {
    let lib = r#"
        #[cfg(test)]
        mod helper;
        #[cfg(all(test, feature = "x"))]
        pub(crate) mod gated_all;
        #[cfg(not(test))]
        mod not_gated;
        pub mod http;
    "#;
    let request = r#"
        #[derive(Deserialize)]
        #[non_exhaustive]
        pub struct Annotated {}

        #[derive(Debug, Deserialize, Serialize)]
        pub struct Bare {}

        #[derive(
            Debug,
            serde::Deserialize,
        )]
        pub struct WrappedQualifiedDerive {}

        #[derive(Deserialize)]
        pub struct GenerationConfig {}

        #[derive(Serialize)]
        pub struct SerializeOnly {}

        pub struct ManualImpl {}
        impl<'de> Deserialize<'de> for ManualImpl {}

        #[derive(Deserialize)]
        pub(crate) struct CrateVisible {}

        const JSON: &str = "{ { {";

        mod inner {
            #[derive(Deserialize)]
            pub struct Indented {}
        }

        #[cfg(test)]
        mod tests {
            #[derive(Deserialize)]
            pub struct TestOnlyFixture {}
            // Must not make SerializeOnly deserializable crate-wide.
            impl<'de> Deserialize<'de> for SerializeOnly {}
        }

        #[cfg(
            all(
                test,
                feature = "a-feature-name-long-enough-that-rustfmt-wraps-this-gate",
            )
        )]
        mod wrapped_gate {
            #[derive(Deserialize)]
            pub struct HiddenByWrappedGate {}
        }

        #[cfg(not(test))]
        mod not_test {
            #[derive(Deserialize)]
            pub struct ScannedUnderNotTest {}
        }

        #[cfg(test)]
        #[derive(Deserialize)]
        pub struct GatedItem {}

        #[derive(Deserialize)]
        pub struct AfterEverything {}
    "#;
    let gated_file = "#[derive(Deserialize)] pub struct InGatedFile {}";
    let http = "#[cfg(test)] mod helpers;";
    let sources: Vec<(String, String)> = [
        ("src/lib.rs", lib),
        ("src/request.rs", request),
        ("src/helper.rs", gated_file),
        ("src/gated_all/mod.rs", gated_file),
        (
            "src/not_gated.rs",
            "#[derive(Deserialize)] pub struct InUngatedFile {}",
        ),
        ("src/http/mod.rs", http),
        ("src/http/helpers.rs", gated_file),
    ]
    .into_iter()
    .map(|(p, t)| (p.to_string(), t.to_string()))
    .collect();

    let report = analyze(&sources, &["src/request.rs:GenerationConfig"], &[]);

    let expected_offenders: BTreeSet<String> = [
        "src/request.rs:Bare",
        "src/request.rs:WrappedQualifiedDerive",
        "src/request.rs:ManualImpl",
        "src/request.rs:Indented",
        "src/request.rs:ScannedUnderNotTest",
        "src/request.rs:AfterEverything",
        "src/not_gated.rs:InUngatedFile",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(report.offenders, expected_offenders);
    assert_eq!(
        report.exempted,
        BTreeSet::from(["src/request.rs:GenerationConfig".to_string()])
    );
    assert_eq!(
        report.test_modules,
        BTreeSet::from([
            "src/gated_all".to_string(),
            "src/helper".to_string(),
            "src/http/helpers".to_string(),
        ])
    );
}

#[test]
fn the_extra_scan_detects_what_it_claims_to() {
    let types = r#"
        #[derive(Deserialize)]
        #[non_exhaustive]
        pub struct Flattened {
            pub name: Option<String>,
            #[serde(flatten)]
            pub extra: serde_json::Map<String, serde_json::Value>,
        }

        #[derive(Deserialize)]
        #[non_exhaustive]
        pub struct FlattenedUnderAnotherName {
            #[serde(default, flatten)]
            pub unmodeled: serde_json::Map<String, serde_json::Value>,
        }

        // A hand-written Deserialize that fills `extra` itself.
        #[non_exhaustive]
        pub struct ManualWithExtra {
            pub extra: serde_json::Map<String, serde_json::Value>,
        }
        impl<'de> Deserialize<'de> for ManualWithExtra {}

        #[derive(Deserialize, Serialize)]
        #[non_exhaustive]
        pub struct DropsUnknownFields {
            #[serde(rename = "flatten")]
            pub max_tokens: Option<u32>,
        }

        #[derive(Deserialize)]
        pub struct RequestBody {
            pub max_tokens: Option<u32>,
        }

        #[derive(Deserialize)]
        #[non_exhaustive]
        pub struct LoopRecord {
            pub name: String,
        }

        #[derive(Deserialize)]
        #[non_exhaustive]
        pub struct Newtype(pub String);

        #[derive(Serialize)]
        #[non_exhaustive]
        pub struct SerializeOnlyView {
            pub name: String,
        }

        #[derive(Deserialize)]
        #[non_exhaustive]
        pub(crate) struct CrateVisible {
            pub name: String,
        }
    "#;
    let sources = vec![("src/types.rs".to_string(), types.to_string())];
    let report = analyze(
        &sources,
        &["src/types.rs:RequestBody"],
        &["src/types.rs:LoopRecord"],
    );
    assert_eq!(
        report.without_extra,
        BTreeSet::from(["src/types.rs:DropsUnknownFields".to_string()])
    );
    assert_eq!(
        report.extra_exempted,
        BTreeSet::from(["src/types.rs:LoopRecord".to_string()])
    );
}

#[test]
fn gated_module_paths_are_scoped_to_their_declaration() {
    let modules = BTreeSet::from(["src/http/helpers".to_string()]);
    assert!(is_test_only_path("src/http/helpers.rs", &modules));
    assert!(is_test_only_path("src/http/helpers/mod.rs", &modules));
    assert!(is_test_only_path("src/http/helpers/deep.rs", &modules));
    assert!(!is_test_only_path("src/http/helpers_extra.rs", &modules));
    assert!(!is_test_only_path("src/helpers.rs", &modules));

    assert_eq!(child_module_dir("src/lib.rs"), "src");
    assert_eq!(child_module_dir("src/http/mod.rs"), "src/http");
    assert_eq!(
        child_module_dir("src/request_builder.rs"),
        "src/request_builder"
    );
}

#[test]
fn the_cfg_test_gate_recognises_compound_forms_and_only_those() {
    fn gated(src: &str) -> bool {
        let item: syn::ItemMod = syn::parse_str(&format!("{src} mod m {{}}")).unwrap();
        is_test_gated(&item.attrs)
    }
    assert!(gated("#[cfg(test)]"));
    assert!(gated("#[cfg(all(test, not(miri)))]"));
    assert!(gated("#[cfg(any(test, genai_strict_unknown))]"));
    assert!(gated("#[allow(dead_code)] #[cfg(test)]"));
    assert!(!gated("#[cfg(not(test))]"));
    assert!(!gated("#[cfg(all(not(test), unix))]"));
    assert!(!gated("#[cfg(not(any(test, miri)))]"));
    assert!(!gated("#[cfg(feature = \"test\")]"));
    assert!(!gated("#[cfg(unix)]"));
    assert!(!gated("#[derive(Debug)]"));
}
