//! File Search Store resource tests (`/v1beta/fileSearchStores`).
//!
//! Covers the store lifecycle, document upload and indexing, and — the part
//! that matters — an end-to-end retrieval through
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
use futures_util::FutureExt;
use genai_rs::{Client, Content, DocumentState, InteractionInput, InteractionStatus, Tool};
use std::panic::AssertUnwindSafe;

/// Creates a uniquely-named store so concurrent runs don't collide.
async fn create_test_store(client: &Client, label: &str) -> genai_rs::FileSearchStore {
    // Display names are sanitized into the resource name by the API, so a
    // nanosecond suffix is enough to keep parallel test binaries apart.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .subsec_nanos();

    client
        .create_file_search_store(Some(&format!("genai-rs-test-{label}-{unique}")))
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
    if client.get_file_search_store(&name).await.is_ok()
        && let Err(e) = client.delete_file_search_store(&name, true).await
    {
        eprintln!("cleanup failed for {name}: {e:?}");
    }

    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
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
                .get_file_search_store(&name)
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

            // The new store appears in a listing.
            let listed = client
                .list_file_search_stores(None, None)
                .await
                .expect("list stores failed");
            assert!(
                listed.stores.iter().any(|s| s.name == store.name),
                "created store should appear in the list"
            );

            client
                .delete_file_search_store(&store.name, true)
                .await
                .expect("delete store failed");

            // Deleted stores are gone.
            assert!(
                client.get_file_search_store(&store.name).await.is_err(),
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
                .upload_to_file_search_store(&store_name, file.path(), Some("budget-memo"))
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
            let active = client
                .wait_for_document_active(&document.name, None, None)
                .await
                .expect("document never became active");
            assert_eq!(active.state, Some(DocumentState::Active));

            let listed = client
                .list_file_search_documents(&store_name, None, None)
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
                    .delete_file_search_document(&document.name, false)
                    .await
                    .is_err(),
                "deleting an indexed document without force should fail"
            );
            client
                .delete_file_search_document(&document.name, true)
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

            // Routed through the explicit-MIME variant so it has live
            // coverage too — the inferring variant is exercised by the other
            // tests in this file, and this file's temp docs are all `.txt`,
            // so the two differ only in who supplies "text/plain".
            let document = client
                .upload_to_file_search_store_with_mime(
                    &store_name,
                    file.path(),
                    Some("vega-reference"),
                    "text/plain",
                )
                .await
                .expect("upload failed");

            client
                .wait_for_document_active(&document.name, None, None)
                .await
                .expect("document never became active");

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
                .upload_to_file_search_store(&store_name, file.path(), Some("doc"))
                .await
                .expect("upload failed");
            client
                .wait_for_document_active(&document.name, None, None)
                .await
                .expect("document never became active");

            // The documented behavior, asserted rather than printed: without
            // this the test passes identically whether the API rejects the
            // delete or silently accepts it, while ENUM_WIRE_FORMATS.md and the
            // CHANGELOG both state the requirement as live-verified.
            let unforced = client.delete_file_search_store(&store_name, false).await;
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

// --- Local validation (no API key needed) ---

#[tokio::test]
async fn test_malformed_store_name_rejected_before_network() {
    // An invalid key proves we never reached the network: a request would
    // fail with an auth error rather than InvalidInput.
    let client = Client::new("invalid_key".to_string());

    let err = client
        .get_file_search_store("abc123")
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
        .get_file_search_document("fileSearchStores/abc123")
        .await
        .expect_err("a store name is not a document name");
    assert!(
        err.to_string().contains("/documents/"),
        "expected a local validation error naming the missing segment, got: {err}"
    );
}
