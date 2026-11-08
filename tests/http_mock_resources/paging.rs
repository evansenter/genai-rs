//! List paging: `.pages()` / `.items()` over several stub pages, and D-016's
//! receiver rule: a compile guard on sampled verbs, and a source scan of
//! every public method on every resource handle.

use super::files::file_names;
use super::*;

fn agents_page(ids: &[&str], next: Option<&str>) -> Reply {
    let agents: Vec<Value> = ids.iter().map(|id| json!({"id": id})).collect();
    let mut body = json!({"agents": agents});
    if let Some(next) = next {
        body["next_page_token"] = json!(next);
    }
    Reply::json(200, body)
}

fn agent_ids(agents: &[Agent]) -> Vec<&str> {
    agents.iter().filter_map(|a| a.id.as_deref()).collect()
}

#[tokio::test]
async fn list_items_follow_every_page_and_resend_the_query() {
    let stub = Stub::replying(vec![
        agents_page(&["a1", "a2"], Some("p2")),
        // An empty page with a token is followed, not taken as the end.
        agents_page(&[], Some("p/3")),
        agents_page(&["a3"], Some("")),
    ])
    .await;
    let client = stub.client();

    let agents: Vec<Agent> = client
        .agents()
        .list()
        .with_page_size(2)
        .with_parent("projects/p")
        .items()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(agent_ids(&agents), ["a1", "a2", "a3"]);
    // Page size and filter ride on every page; only the token changes.
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/agents?page_size=2&parent=projects%2Fp",
            "/v1beta/agents?page_size=2&page_token=p2&parent=projects%2Fp",
            "/v1beta/agents?page_size=2&page_token=p%2F3&parent=projects%2Fp",
        ]
    );
}

#[tokio::test]
async fn list_pages_resume_from_with_page_token() {
    let stub = Stub::replying(vec![
        agents_page(&["a3"], Some("p3")),
        agents_page(&["a4"], None),
    ])
    .await;
    let client = stub.client();

    let pages: Vec<_> = client
        .agents()
        .list()
        .with_page_token("p2")
        .pages()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(agent_ids(&pages[0].agents), ["a3"]);
    assert_eq!(pages[1].next_page_token, None);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/agents?page_token=p2",
            "/v1beta/agents?page_token=p3"
        ]
    );
}

#[tokio::test]
async fn list_send_fetches_exactly_one_page() {
    let stub = Stub::replying(vec![agents_page(&["a1"], Some("p2"))]).await;
    let client = stub.client();

    let page = client.agents().list().send().await.unwrap();

    assert_eq!(agent_ids(&page.agents), ["a1"]);
    assert_eq!(page.next_page_token.as_deref(), Some("p2"));
    assert_eq!(targets(&stub), ["/v1beta/agents"]);
}

#[tokio::test]
async fn list_items_stop_on_a_repeated_page_token() {
    let stub = Stub::replying(vec![
        agents_page(&["a1"], Some("same")),
        agents_page(&["a2"], Some("same")),
    ])
    .await;
    let client = stub.client();

    let results: Vec<_> = client.agents().list().items().collect().await;

    assert_eq!(results.len(), 3, "{results:?}");
    assert_eq!(results[0].as_ref().unwrap().id.as_deref(), Some("a1"));
    // The repeating page's items are still delivered, then the error.
    assert_eq!(results[1].as_ref().unwrap().id.as_deref(), Some("a2"));
    assert!(
        matches!(&results[2], Err(GenaiError::MalformedResponse(m)) if m.contains("\"same\"")),
        "{:?}",
        results[2]
    );
    assert_eq!(
        stub.requests().len(),
        2,
        "the repeated token is not fetched"
    );
}

#[tokio::test]
async fn list_items_end_at_the_first_error() {
    // The stub answers a third request with 500, which would show up here.
    let stub = Stub::replying(vec![agents_page(&["a1"], Some("p2")), not_found()]).await;
    let client = stub.client();

    let results: Vec<_> = client.agents().list().items().collect().await;

    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].is_ok());
    assert!(
        matches!(
            results[1],
            Err(GenaiError::Api {
                status_code: 404,
                ..
            })
        ),
        "{:?}",
        results[1]
    );
    assert_eq!(stub.requests().len(), 2);
}

#[tokio::test]
async fn list_streams_send_nothing_until_polled() {
    let stub = Stub::replying(vec![agents_page(&["a1"], None)]).await;
    let client = stub.client();

    let mut items = client.agents().list().items();
    let pages = client.agents().list().pages();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        stub.requests().is_empty(),
        "a request before the first poll"
    );

    drop(pages);
    let first = items.next().await.unwrap().unwrap();
    assert_eq!(first.id.as_deref(), Some("a1"));
    assert!(items.next().await.is_none());
    assert_eq!(stub.requests().len(), 1);
}

/// Compile guard for D-016. Handle methods take `self`, so a call's future
/// holds the client borrow rather than the temporary handle (an `&self`
/// receiver fails these with E0515), and list streams own a client clone,
/// so one built from a temporary client can be spawned. The verbs here are
/// a sample; the scan below checks every one.
#[tokio::test]
async fn handle_futures_and_list_streams_outlive_their_temporaries() {
    fn assert_send<T: Send>(_: &T) {}

    let stub = Stub::start(|request, _| {
        if request.target.starts_with("/v1beta/agents?") || request.target == "/v1beta/agents" {
            agents_page(&["listed"], None)
        } else if let Some(id) = request.target.strip_prefix("/v1beta/files/") {
            Reply::json(
                200,
                json!({"name": format!("files/{id}"), "mimeType": "text/plain"}),
            )
        } else {
            let id = request.target.rsplit('/').next().unwrap_or_default();
            Reply::json(200, json!({"id": id}))
        }
    })
    .await;
    let client = stub.client();
    let c = &client;

    let ids = ["a1".to_string(), "a2".to_string()];
    let fetched = futures_util::future::join_all(ids.iter().map(|id| c.agents().get(id))).await;
    let fetched: Vec<Agent> = fetched.into_iter().collect::<Result<_, _>>().unwrap();
    assert_eq!(agent_ids(&fetched), ["a1", "a2"]);

    let stored = c.agents().get("a3");
    assert_send(&stored);
    assert_eq!(stored.await.unwrap().id.as_deref(), Some("a3"));

    let names = ["files/f1".to_string(), "files/f2".to_string()];
    let files = futures_util::future::join_all(names.iter().map(|n| c.files().get(n))).await;
    let files: Vec<FileMetadata> = files.into_iter().collect::<Result<_, _>>().unwrap();
    assert_eq!(file_names(&files), ["files/f1", "files/f2"]);
    let waiting = c.files().wait_until_active("files/f3", PollOptions::new());
    assert_send(&waiting);
    // A nested handle's future holds only the client borrow too.
    let indexing = c
        .file_search_stores()
        .documents()
        .wait_until_active(DOC, PollOptions::new());
    assert_send(&indexing);
    let documents = c.file_search_stores().documents().list(STORE).items();
    assert_send(&documents);

    // The client this stream came from is a temporary, gone before the
    // stream is first polled on another task.
    let items = stub.client().agents().list().items();
    assert_send(&items);
    let listed: Vec<Agent> = tokio::spawn(items.try_collect()).await.unwrap().unwrap();
    assert_eq!(agent_ids(&listed), ["listed"]);

    let pages = c.agents().list().with_page_size(1).pages();
    let spawned = tokio::spawn(async move { pages.try_collect::<Vec<_>>().await });
    assert_eq!(spawned.await.unwrap().unwrap().len(), 1);
}

// =============================================================================
// D-016's receiver rule, on every handle
// =============================================================================

/// The attribute that marks a resource handle (as `tests/live_verbs/` finds
/// them; `ci_coverage`'s `resource_handles_carry_the_marker` checks that
/// every handle carries it).
const HANDLE_MARKER: &str = "a resource handle does nothing until you call one of its methods";

/// The handles the scan must find, so a renamed marker cannot empty it.
const HANDLES: [&str; 11] = [
    "Agents",
    "Credentials",
    "EnvironmentFiles",
    "Environments",
    "FileSearchDocuments",
    "FileSearchStores",
    "Files",
    "Interactions",
    "Triggers",
    "Voices",
    "Webhooks",
];

/// Every `.rs` file under `dir`, parsed.
fn parse_tree(dir: &std::path::Path, out: &mut Vec<syn::File>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            parse_tree(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            out.push(syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display())));
        }
    }
}

/// Every item, inline modules' included.
fn all_items(files: &[syn::File]) -> Vec<&syn::Item> {
    fn walk<'a>(items: &'a [syn::Item], out: &mut Vec<&'a syn::Item>) {
        for item in items {
            if let syn::Item::Mod(module) = item
                && let Some((_, inner)) = &module.content
            {
                walk(inner, out);
            }
            out.push(item);
        }
    }
    let mut out = Vec::new();
    for file in files {
        walk(&file.items, &mut out);
    }
    out
}

/// A type's name for a report: a path's last segment, `&` before a
/// reference, `_` for anything else.
fn type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map_or_else(|| "_".to_string(), |s| s.ident.to_string()),
        syn::Type::Reference(r) => format!("&{}", type_name(&r.elem)),
        _ => "_".to_string(),
    }
}

fn has_must_use(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("must_use"))
}

/// The handle structs (those carrying [`HANDLE_MARKER`]), and every
/// D-016 problem with their public methods: a receiver other than `self` or
/// `mut self` (an `&self` verb's future borrows the temporary handle, E0515),
/// no receiver at all, or a returned value that is not `#[must_use]`. A
/// value is `#[must_use]` when the method says so, or its type is a `Result`
/// or a type declared `#[must_use]` (a handle, a list builder); an `async`
/// method is judged by the value its future yields.
fn handle_receiver_problems(
    files: &[syn::File],
) -> (std::collections::BTreeSet<String>, Vec<String>) {
    use std::collections::BTreeSet;

    let items = all_items(files);
    let mut handles = BTreeSet::new();
    let mut must_use_types: BTreeSet<String> = BTreeSet::from(["Result".to_string()]);
    let mut list_builders = BTreeSet::new();
    let mut builder_template_must_use = false;
    for item in &items {
        match item {
            syn::Item::Struct(s) => {
                let marked = s.attrs.iter().any(|attr| {
                    matches!(&attr.meta, syn::Meta::NameValue(nv)
                        if nv.path.is_ident("must_use")
                            && matches!(&nv.value, syn::Expr::Lit(syn::ExprLit {
                                lit: syn::Lit::Str(text), ..
                            }) if text.value() == HANDLE_MARKER))
                });
                if marked {
                    handles.insert(s.ident.to_string());
                }
                if has_must_use(&s.attrs) {
                    must_use_types.insert(s.ident.to_string());
                }
            }
            syn::Item::Enum(e) if has_must_use(&e.attrs) => {
                must_use_types.insert(e.ident.to_string());
            }
            // The list builders are declared inside `list_builder!`, whose
            // template marks each `#[must_use]`.
            syn::Item::Macro(m) if m.ident.as_ref().is_some_and(|i| i == "list_builder") => {
                let template: String = m.mac.tokens.to_string().split_whitespace().collect();
                // The transcriber's struct is the last one, after the
                // caller's `$(#[$meta])*` and the template's own attributes.
                builder_template_must_use = template.rfind("pubstruct$name").is_some_and(|at| {
                    let before = &template[..at];
                    before[before.rfind(")*").map_or(0, |i| i + 2)..].contains("#[must_use")
                });
            }
            syn::Item::Macro(m)
                if m.mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "list_builder") =>
            {
                let tokens = m.mac.tokens.to_string();
                let words: Vec<&str> = tokens.split_whitespace().collect();
                for pair in words.windows(2) {
                    if pair[0] == "struct" {
                        list_builders.insert(pair[1].to_string());
                    }
                }
            }
            _ => {}
        }
    }
    if builder_template_must_use {
        must_use_types.extend(list_builders);
    }

    let mut problems = Vec::new();
    for item in &items {
        let syn::Item::Impl(imp) = item else { continue };
        if imp.trait_.is_some() {
            continue;
        }
        let syn::Type::Path(owner) = &*imp.self_ty else {
            continue;
        };
        let Some(owner) = owner.path.segments.last().map(|s| s.ident.to_string()) else {
            continue;
        };
        if !handles.contains(&owner) {
            continue;
        }
        for item in &imp.items {
            let syn::ImplItem::Fn(f) = item else { continue };
            if !matches!(f.vis, syn::Visibility::Public(_)) {
                continue;
            }
            let verb = format!("{owner}::{}", f.sig.ident);
            match f.sig.receiver() {
                None => problems.push(format!("{verb} takes no `self`")),
                Some(receiver) => {
                    let taken = match (&receiver.reference, &receiver.mutability) {
                        (Some(_), Some(_)) => Some("&mut self".to_string()),
                        (Some(_), None) => Some("&self".to_string()),
                        (None, _) => match &*receiver.ty {
                            syn::Type::Path(p) if p.path.is_ident("Self") => None,
                            other => Some(format!("self: {}", type_name(other))),
                        },
                    };
                    if let Some(taken) = taken {
                        problems.push(format!("{verb} takes `{taken}`, not `self`"));
                    }
                }
            }
            let syn::ReturnType::Type(_, returned) = &f.sig.output else {
                continue;
            };
            let unit = matches!(&**returned, syn::Type::Tuple(t) if t.elems.is_empty());
            let must_use_type = matches!(&**returned, syn::Type::Path(p)
                if p.path.segments.last().is_some_and(|s| must_use_types.contains(&s.ident.to_string())));
            if !unit && !must_use_type && !has_must_use(&f.attrs) {
                problems.push(format!(
                    "{verb} returns `{}`, which is not `#[must_use]`: mark the method",
                    type_name(returned)
                ));
            }
        }
    }
    (handles, problems)
}

/// D-016: every public method of every resource handle takes `self` by
/// value, and what it returns is `#[must_use]`. The compile guard above
/// covers sampled verbs; this covers them all.
#[test]
fn every_handle_method_takes_self_and_its_value_is_must_use() {
    let mut files = Vec::new();
    parse_tree(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let (handles, problems) = handle_receiver_problems(&files);
    let missing: Vec<&str> = HANDLES
        .into_iter()
        .filter(|name| !handles.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "the scan found no handle named {missing:?} (found {handles:?}); update HANDLES \
         or HANDLE_MARKER"
    );
    assert!(
        problems.is_empty(),
        "Resource handles break D-016 (docs/BUILDER_API.md):\n  {}",
        problems.join("\n  ")
    );
}

/// The receiver scan reports what it claims to, and nothing on a
/// conforming handle or on a type that is not one.
#[test]
fn the_receiver_scan_catches_what_it_claims_to() {
    let file: syn::File = syn::parse_quote! {
        #[must_use = "a resource handle does nothing until you call one of its methods"]
        pub struct Things<'a> { client: &'a Client }
        #[must_use = "not the marker"]
        pub struct NotAHandle<'a> { client: &'a Client }
        macro_rules! list_builder {
            ($name:ident) => {
                #[must_use = "a list request does nothing"]
                pub struct $name<'a> {}
            };
        }
        paging::list_builder! {
            pub struct ListThings<'a> { parent: Option<String> }
        }
        impl<'a> Things<'a> {
            pub async fn get(self, id: &str) -> Result<Thing, GenaiError> { todo!() }
            pub async fn delete(mut self, id: &str) { todo!() }
            pub fn list(self) -> ListThings<'a> { todo!() }
            pub fn nested(self) -> Things<'a> { todo!() }
            #[must_use = "streams do nothing unless polled"]
            pub fn stream(self, id: &str) -> BoxStream<'static, Thing> { todo!() }
            pub(crate) fn private(&self) -> Thing { todo!() }

            pub async fn by_ref(&self, id: &str) -> Result<Thing, GenaiError> { todo!() }
            pub fn by_mut_ref(&mut self) -> Things<'a> { todo!() }
            pub fn typed(self: &Self) -> Things<'a> { todo!() }
            pub fn boxed(self: Box<Self>) -> Things<'a> { todo!() }
            pub fn associated(id: &str) -> Things<'static> { todo!() }
            pub fn unmarked(self) -> BoxStream<'static, Thing> { todo!() }
            pub async fn unmarked_output(self) -> Vec<Thing> { todo!() }
        }
        impl NotAHandle<'_> {
            pub fn by_ref(&self) -> Vec<Thing> { todo!() }
        }
    };
    let (handles, problems) = handle_receiver_problems(&[file]);
    assert_eq!(
        handles,
        std::collections::BTreeSet::from(["Things".to_string()])
    );
    let expected = [
        "Things::by_ref takes `&self`, not `self`",
        "Things::by_mut_ref takes `&mut self`, not `self`",
        "Things::typed takes `self: &Self`, not `self`",
        "Things::boxed takes `self: Box`, not `self`",
        "Things::associated takes no `self`",
        "Things::unmarked returns `BoxStream`, which is not `#[must_use]`: mark the method",
        "Things::unmarked_output returns `Vec`, which is not `#[must_use]`: mark the method",
    ];
    assert_eq!(problems, expected, "{problems:#?}");
}
