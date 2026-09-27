//! File Search Store resource tests (`/v1beta/fileSearchStores`).
//!
//! Covers the store lifecycle, document uploads from disk and from memory,
//! indexing, listing across pages, and — the part that matters — an
//! end-to-end retrieval through
//! [`Tool::FileSearch`](genai_rs::Tool::FileSearch) against a store this
//! suite provisions itself.
//!
//! Provisioning here is the point: before these endpoints existed, a file
//! search test had to be handed a store created out-of-band, which is why
//! #307 sat blocked.
//!
//! Every test runs its body through [`with_store`], which deletes the store
//! even when the body panics. Without that, a single failed assertion leaks
//! a `genai-rs-test-*` store and its indexed documents into the project, and
//! those accumulate silently across runs. That includes `test_store_lifecycle`,
//! whose body deletes the store itself: the helper probes before deleting, so
//! a body that already cleaned up costs one GET and prints nothing.
//!
//! ```bash
//! cargo test --test file_search_stores_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::{get_client, stateful_builder};
use futures_util::{FutureExt, StreamExt};
use genai_rs::wire::{WireEvent, WireInspector};
use genai_rs::{
    Client, Content, CreateFileSearchStoreRequest, DocumentState, FileSearchDocument, FileUpload,
    GenaiError, InteractionInput, InteractionStatus, PollOptions, Tool,
};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

/// Creates a uniquely-named store so concurrent runs don't collide.
async fn create_test_store(client: &Client, label: &str) -> genai_rs::FileSearchStore {
    // Display names are sanitized into the resource name by the API, so a
    // nanosecond suffix is enough to keep parallel test binaries apart.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .subsec_nanos();

    client
        .file_search_stores()
        .create(
            &CreateFileSearchStoreRequest::new()
                .with_display_name(format!("genai-rs-test-{label}-{unique}")),
        )
        .await
        .expect("failed to create file search store")
}

/// Runs `body` against a freshly created store and deletes the store
/// afterwards, **including when `body` panics**.
///
/// A plain `delete` at the end of a test body only runs on the happy path:
/// every assertion above it is a potential early exit, so a single failure
/// leaks the store. `catch_unwind` lets the cleanup run first and then
/// re-raises, so a failing test still reports as a failing test.
async fn with_store<F, Fut>(client: &Client, label: &str, body: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let store = create_test_store(client, label).await;
    let name = store.name.clone();

    let outcome = AssertUnwindSafe(body(name.clone())).catch_unwind().await;

    // Probed rather than deleted unconditionally. A body whose subject *is*
    // the delete (see `test_store_lifecycle`) leaves nothing to clean up, and
    // an unconditional delete would print "cleanup failed" on every green run
    // — noise that teaches the reader to ignore the one message here that
    // means something. Costs one GET per test and makes the helper idempotent,
    // which is what lets every test go through it.
    //
    // Only a 404 counts as already-gone. Treating *any* probe failure as
    // nothing-to-clean-up would let a transient 503, a timeout, or a rate
    // limit during a matrix run skip the delete and print nothing — leaking
    // the store in precisely the silent way this harness exists to prevent,
    // and worse than the unconditional delete it replaced. Every other
    // outcome falls through and attempts the delete, so an unexpected status
    // (403 rather than 404, say) degrades to the old behavior: the delete
    // runs, and says so if it fails.
    let already_gone = matches!(
        client.file_search_stores().get(&name).await,
        Err(GenaiError::Api {
            status_code: 404,
            ..
        })
    );
    if !already_gone && let Err(e) = client.file_search_stores().force_delete(&name).await {
        eprintln!("cleanup failed for {name}: {e:?}");
    }

    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

/// Waits for a document to be indexed, with the default 60 s / 500 ms.
async fn wait_active(client: &Client, document: &FileSearchDocument) -> FileSearchDocument {
    client
        .file_search_stores()
        .documents()
        .wait_until_active(&document.name, PollOptions::new())
        .await
        .expect("document never became active")
}

/// Writes a temp file and returns its path, keeping the handle alive.
fn temp_doc(contents: &str) -> tempfile::NamedTempFile {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile()
        .expect("failed to create temp file");
    write!(file, "{contents}").expect("failed to write temp file");
    file.flush().expect("failed to flush temp file");
    file
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_store_lifecycle() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Through `with_store` like every other test, even though this one deletes
    // the store itself: the four assertions before that delete are each an
    // early exit that would otherwise leak the store. The helper's cleanup
    // probes before deleting, so the successful path costs one extra GET and
    // prints nothing.
    with_store(&client, "lifecycle", |name| {
        let client = &client;
        async move {
            let store = client
                .file_search_stores()
                .get(&name)
                .await
                .expect("get store failed");
            println!("created store: {}", store.name);

            assert!(
                store.name.starts_with("fileSearchStores/"),
                "store name should be a full resource name, got {:?}",
                store.name
            );
            assert!(store.display_name.is_some());
            assert!(store.create_time.is_some());

            // The new store appears in a listing: one page, then streamed
            // one store per page (the list is oldest first, so the new
            // store comes last; other tests' stores may come before it).
            let page = client
                .file_search_stores()
                .list()
                .with_page_size(20)
                .send()
                .await
                .expect("list stores failed");
            println!(
                "first page: {} store(s), more: {}",
                page.stores.len(),
                page.next_page_token.is_some()
            );
            let mut streamed = 0;
            let mut found = false;
            let mut stores = client
                .file_search_stores()
                .list()
                .with_page_size(1)
                .items()
                .take(50);
            while let Some(listed) = stores.next().await {
                let listed = listed.expect("streaming stores failed");
                streamed += 1;
                if listed.name == store.name {
                    found = true;
                    break;
                }
            }
            println!("streamed {streamed} store(s) to find it");
            assert!(found, "created store should appear in the list");

            // An empty store deletes without force.
            client
                .file_search_stores()
                .delete(&store.name)
                .await
                .expect("delete store failed");

            // Deleted stores are gone.
            assert!(
                client.file_search_stores().get(&store.name).await.is_err(),
                "get should fail after delete"
            );
        }
    })
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_document_upload_and_indexing() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_store(&client, "docs", |store_name| {
        let client = &client;
        async move {
            let file =
                temp_doc("The quarterly budget for Project Aurora is 4.2 million dollars.\n");

            let document = client
                .file_search_stores()
                .upload(
                    &store_name,
                    FileUpload::from_path(file.path()).with_display_name("budget-memo"),
                )
                .await
                .expect("upload failed");

            println!("uploaded document: {}", document.name);
            assert!(document.name.contains("/documents/"));
            assert_eq!(document.display_name.as_deref(), Some("budget-memo"));
            assert_eq!(document.mime_type.as_deref(), Some("text/plain"));
            assert!(
                document.size_bytes.is_some_and(|n| n > 0),
                "sizeBytes arrives as a JSON string and must parse to a positive number, got {:?}",
                document.size_bytes
            );

            // Indexing is asynchronous — a fresh document is Pending, not Active.
            let active = wait_active(client, &document).await;
            assert_eq!(active.state, Some(DocumentState::Active));

            let listed = client
                .file_search_stores()
                .documents()
                .list(&store_name)
                .send()
                .await
                .expect("list documents failed");
            assert!(
                listed.documents.iter().any(|d| d.name == document.name),
                "uploaded document should appear in the store listing"
            );

            // An indexed document needs force=true; without it the API rejects the
            // delete with "Cannot delete non-empty Document".
            assert!(
                client
                    .file_search_stores()
                    .documents()
                    .delete(&document.name)
                    .await
                    .is_err(),
                "deleting an indexed document without force should fail"
            );
            client
                .file_search_stores()
                .documents()
                .force_delete(&document.name)
                .await
                .expect("forced document delete failed");
        }
    })
    .await;
}

/// The end-to-end case #307 was blocked on: provision a store, add a
/// document, and retrieve from it through the file search tool.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_file_search_retrieval_end_to_end() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_store(&client, "retrieval", |store_name| {
        let client = &client;
        async move {
            // A distinctive fact the model cannot know without retrieving it, so a
            // correct answer is evidence of retrieval rather than of pretraining.
            let file = temp_doc(
                "Internal reference: the maintenance codename for the Vega ground station \
         relay is HALCYON-девять-42. It is reviewed every 90 days.\n",
            );

            // An explicit MIME type, so `with_mime_type` has live coverage
            // too — the other tests infer it, and this file's temp docs are
            // all `.txt`, so the two differ only in who supplies
            // "text/plain".
            let document = client
                .file_search_stores()
                .upload(
                    &store_name,
                    FileUpload::from_path(file.path())
                        .with_display_name("vega-reference")
                        .with_mime_type("text/plain"),
                )
                .await
                .expect("upload failed");

            wait_active(client, &document).await;

            let response = stateful_builder(client)
                .with_input(InteractionInput::Content(vec![Content::text(
                    "What is the maintenance codename for the Vega ground station relay? \
             Search the files.",
                )]))
                .add_tool(Tool::FileSearch {
                    store_names: vec![store_name.clone()],
                    top_k: None,
                    metadata_filter: None,
                })
                .create()
                .await
                .expect("file search interaction failed");

            assert_eq!(response.status, InteractionStatus::Completed);

            let step_types: Vec<&str> = response
                .steps
                .iter()
                .map(genai_rs::Step::step_type)
                .collect();
            println!("steps: {step_types:?}");
            assert!(
                step_types.contains(&"file_search_call"),
                "expected the model to issue a file_search_call, got {step_types:?}"
            );

            let text = response.as_text().expect("expected a text response");
            println!("retrieval answer: {text}");
            // Deterministic check: the codename is a literal string from the
            // uploaded document, so this is an exact-value assertion rather than a
            // brittle guess at the model's phrasing.
            assert!(
                text.contains("HALCYON"),
                "answer should quote the retrieved codename, got: {text}"
            );
        }
    })
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_store_delete_without_force_rejects_non_empty() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_store(&client, "force", |store_name| {
        let client = &client;
        async move {
            let file = temp_doc("Some indexed content.\n");

            let document = client
                .file_search_stores()
                .upload(
                    &store_name,
                    FileUpload::from_path(file.path()).with_display_name("doc"),
                )
                .await
                .expect("upload failed");
            wait_active(client, &document).await;

            // The documented behavior, asserted rather than printed: without
            // this the test passes identically whether the API rejects the
            // delete or silently accepts it, while ENUM_WIRE_FORMATS.md and the
            // CHANGELOG both state the requirement as live-verified.
            let unforced = client.file_search_stores().delete(&store_name).await;
            println!("unforced delete of non-empty store: {unforced:?}");
            assert!(
                unforced.is_err(),
                "deleting a store that still holds documents must be rejected \
             without force=true; got Ok"
            );
        }
    })
    .await;
}

/// Records the URL of every GET the client sends.
#[derive(Default)]
struct GetUrls(Mutex<Vec<String>>);

impl WireInspector for GetUrls {
    fn on_event(&self, event: &WireEvent) {
        if let WireEvent::Request { method, url, .. } = event
            && method == "GET"
        {
            self.0.lock().unwrap().push(url.clone());
        }
    }
}

/// Uploads from memory: the same raw protocol as a path upload (verified
/// live 2026-09-27). With a display name the document carries it; without
/// one it is unnamed, as `FileUpload::from_bytes` documents. The store's
/// two documents then stream one per page.
#[tokio::test]
#[ignore = "Requires API key"]
async fn test_bytes_upload_indexes_and_lists_across_pages() {
    let Some(api_key) = std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
    else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let urls = Arc::new(GetUrls::default());
    let client = Client::builder(api_key)
        .add_wire_inspector(urls.clone())
        .build()
        .expect("client");

    with_store(&client, "bytes", |store_name| {
        let client = &client;
        let urls = &urls;
        async move {
            let named_data = b"The Orion rollout freeze starts on the 14th.\n".to_vec();
            let named_len = named_data.len();
            let named = client
                .file_search_stores()
                .upload(
                    &store_name,
                    FileUpload::from_bytes(named_data, "text/plain")
                        .with_display_name("orion-memo"),
                )
                .await
                .expect("named bytes upload failed");
            println!("uploaded document: {}", named.name);
            assert!(named.name.starts_with(&format!("{store_name}/documents/")));
            assert_eq!(named.display_name.as_deref(), Some("orion-memo"));
            assert_eq!(named.mime_type.as_deref(), Some("text/plain"));
            assert_eq!(named.size_bytes, Some(named_len as i64));

            let unnamed = client
                .file_search_stores()
                .upload(
                    &store_name,
                    FileUpload::from_bytes(b"Unnamed in-memory notes.\n".to_vec(), "text/plain"),
                )
                .await
                .expect("unnamed bytes upload failed");
            println!("uploaded document: {}", unnamed.name);
            assert_eq!(
                unnamed.display_name, None,
                "an in-memory upload without a display name stays unnamed"
            );

            for document in [&named, &unnamed] {
                let active = wait_active(client, document).await;
                assert_eq!(active.state, Some(DocumentState::Active));
            }

            urls.0.lock().unwrap().clear();
            let mut seen = Vec::new();
            let mut documents = client
                .file_search_stores()
                .documents()
                .list(&store_name)
                .with_page_size(1)
                .items()
                .take(10);
            while let Some(document) = documents.next().await {
                seen.push(document.expect("streaming documents failed").name);
            }
            println!("streamed documents: {seen:?}");
            let mut expected = vec![named.name.clone(), unnamed.name.clone()];
            expected.sort_unstable();
            seen.sort_unstable();
            assert_eq!(seen, expected, "each document exactly once");

            let urls = urls.0.lock().unwrap().clone();
            let lists: Vec<&String> = urls.iter().filter(|u| u.contains("/documents?")).collect();
            println!("sent {} list request(s)", lists.len());
            assert!(
                lists.len() >= 2,
                "two documents at page_size=1 need two pages: {lists:?}"
            );
            for (i, url) in lists.iter().enumerate() {
                assert!(
                    url.contains("page_size=1"),
                    "page {i} dropped the page size: {url}"
                );
                assert_eq!(url.contains("page_token="), i > 0, "page {i}: {url}");
            }
        }
    })
    .await;
}

// --- Local validation (no API key needed) ---

#[tokio::test]
async fn test_malformed_store_name_rejected_before_network() {
    // An invalid key proves we never reached the network: a request would
    // fail with an auth error rather than InvalidInput.
    let client = Client::new("invalid_key".to_string());

    let err = client
        .file_search_stores()
        .get("abc123")
        .await
        .expect_err("a bare ID should be rejected locally");
    assert!(
        err.to_string().contains("fileSearchStores/<id>"),
        "expected a local validation error naming the required shape, got: {err}"
    );
}

#[tokio::test]
async fn test_malformed_document_name_rejected_before_network() {
    let client = Client::new("invalid_key".to_string());

    let err = client
        .file_search_stores()
        .documents()
        .get("fileSearchStores/abc123")
        .await
        .expect_err("a store name is not a document name");
    assert!(
        err.to_string().contains("/documents/"),
        "expected a local validation error naming the missing segment, got: {err}"
    );
}
