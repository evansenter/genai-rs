//! File Search Store resource tests (`/v1beta/fileSearchStores`).
//!
//! Covers the store lifecycle, document upload and indexing, and — the part
//! that matters — an end-to-end retrieval through
//! [`Tool::FileSearch`](genai_rs::Tool::FileSearch) against a store this
//! suite provisions itself.
//!
//! Provisioning here is the point: before these endpoints existed, a file
//! search test had to be handed a store created out-of-band, which is why
//! #307 sat blocked. Every test below cleans up the stores it creates.
//!
//! ```bash
//! cargo test --test file_search_stores_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::{get_client, stateful_builder};
use genai_rs::{Client, Content, DocumentState, InteractionInput, InteractionStatus, Tool};

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

    let store = create_test_store(&client, "lifecycle").await;
    println!("created store: {}", store.name);

    assert!(
        store.name.starts_with("fileSearchStores/"),
        "store name should be a full resource name, got {:?}",
        store.name
    );
    assert!(store.display_name.is_some());
    assert!(store.create_time.is_some());

    // Get returns the same resource.
    let fetched = client
        .get_file_search_store(&store.name)
        .await
        .expect("get store failed");
    assert_eq!(fetched.name, store.name);
    assert_eq!(fetched.display_name, store.display_name);

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

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_document_upload_and_indexing() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let store = create_test_store(&client, "docs").await;
    let file = temp_doc("The quarterly budget for Project Aurora is 4.2 million dollars.\n");

    let document = client
        .upload_to_file_search_store(&store.name, file.path(), Some("budget-memo"))
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
        .wait_for_document_active(&document.name, None)
        .await
        .expect("document never became active");
    assert_eq!(active.state, Some(DocumentState::Active));

    let listed = client
        .list_documents(&store.name, None, None)
        .await
        .expect("list documents failed");
    assert!(
        listed.documents.iter().any(|d| d.name == document.name),
        "uploaded document should appear in the store listing"
    );

    // An indexed document needs force=true; without it the API rejects the
    // delete with "Cannot delete non-empty Document".
    assert!(
        client.delete_document(&document.name, false).await.is_err(),
        "deleting an indexed document without force should fail"
    );
    client
        .delete_document(&document.name, true)
        .await
        .expect("forced document delete failed");

    client
        .delete_file_search_store(&store.name, true)
        .await
        .expect("delete store failed");
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

    let store = create_test_store(&client, "retrieval").await;

    // A distinctive fact the model cannot know without retrieving it, so a
    // correct answer is evidence of retrieval rather than of pretraining.
    let file = temp_doc(
        "Internal reference: the maintenance codename for the Vega ground station \
         relay is HALCYON-девять-42. It is reviewed every 90 days.\n",
    );

    let document = client
        .upload_to_file_search_store(&store.name, file.path(), Some("vega-reference"))
        .await
        .expect("upload failed");

    client
        .wait_for_document_active(&document.name, None)
        .await
        .expect("document never became active");

    let response = stateful_builder(&client)
        .with_input(InteractionInput::Content(vec![Content::text(
            "What is the maintenance codename for the Vega ground station relay? \
             Search the files.",
        )]))
        .add_tool(Tool::FileSearch {
            store_names: vec![store.name.clone()],
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

    client
        .delete_file_search_store(&store.name, true)
        .await
        .expect("delete store failed");
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_store_delete_without_force_rejects_non_empty() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let store = create_test_store(&client, "force").await;
    let file = temp_doc("Some indexed content.\n");

    let document = client
        .upload_to_file_search_store(&store.name, file.path(), Some("doc"))
        .await
        .expect("upload failed");
    client
        .wait_for_document_active(&document.name, None)
        .await
        .expect("document never became active");

    // Documented behavior: a store holding documents needs force.
    let unforced = client.delete_file_search_store(&store.name, false).await;
    println!("unforced delete of non-empty store: {unforced:?}");

    // Either way, force must clean up so the test leaves nothing behind.
    client
        .delete_file_search_store(&store.name, true)
        .await
        .expect("forced delete should always succeed");
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
        .get_document("fileSearchStores/abc123")
        .await
        .expect_err("a store name is not a document name");
    assert!(
        err.to_string().contains("/documents/"),
        "expected a local validation error naming the missing segment, got: {err}"
    );
}
