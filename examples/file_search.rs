//! Example: File Search for Semantic Document Retrieval
//!
//! Demonstrates the full File Search loop end to end: provision a store,
//! upload a document, wait for it to be indexed, retrieve from it, and clean
//! up. No prior setup is needed — the example creates everything it uses and
//! deletes it again on the way out.
//!
//! Run with: cargo run --example file_search

use genai_rs::{Client, FileSearchConfig};
use std::env;
use std::error::Error;
use std::io::Write;

/// A document with a fact the model cannot already know, so a correct answer
/// demonstrates retrieval rather than recall.
const DOCUMENT: &str = "\
Ferrymead Engineering — internal handbook (excerpt)

Async runtime policy: all services standardize on Tokio with the
multi-threaded scheduler. The agreed worker-thread count for edge services
is 6, chosen after the 2026 latency review.

Error handling: services return `thiserror`-derived enums at module
boundaries and reserve `anyhow` for binaries only.
";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let model_name = genai_rs::DEFAULT_MODEL;

    println!("=== File Search Example ===\n");

    // 1. Provision a store. The returned `name` is the full resource name
    //    (e.g. "fileSearchStores/my-docs-4kws71n2ybpr") — that is what the
    //    File Search tool takes, not the display name.
    let store = client
        .create_file_search_store(Some("genai-rs-example"))
        .await?;
    println!("Created store: {}", store.name);

    // Run the rest inside a closure so a failure still hits the cleanup below
    // rather than leaking a store into the project.
    let result = run_example(&client, model_name, &store.name).await;

    println!("\n--- Cleanup ---");
    // `force` is required while the store still holds documents.
    client.delete_file_search_store(&store.name, true).await?;
    println!("Deleted store: {}", store.name);

    result
}

async fn run_example(
    client: &Client,
    model_name: &str,
    store_name: &str,
) -> Result<(), Box<dyn Error>> {
    // 2. Upload a document into the store.
    let mut file = tempfile::Builder::new().suffix(".txt").tempfile()?;
    write!(file, "{DOCUMENT}")?;
    file.flush()?;

    let document = client
        .upload_to_file_search_store(store_name, file.path(), Some("engineering-handbook"))
        .await?;
    println!("Uploaded document: {}", document.name);
    println!("  state: {:?}", document.state);

    // 3. Indexing is asynchronous. A freshly uploaded document is Pending,
    //    and File Search silently returns nothing for it until it is Active —
    //    so waiting here is not optional.
    let active = client
        .wait_for_document_active(&document.name, None)
        .await?;
    println!("  indexed: {:?}\n", active.state);

    // 4. Retrieve from the store.
    println!("--- Basic File Search ---");
    let prompt = "According to my documents, how many worker threads do edge services use?";
    println!("Prompt: {prompt}\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .add_tool(FileSearchConfig::new(vec![store_name.to_string()]))
        .with_store_enabled()
        .create()
        .await?;

    println!("Status: {:?}\n", response.status);
    if let Some(text) = response.as_text() {
        println!("Model Response:\n{text}\n");
    }

    // The model issues a `file_search_call` and receives a
    // `file_search_result`, which is how you can tell retrieval happened:
    let step_types: Vec<&str> = response
        .steps
        .iter()
        .map(genai_rs::Step::step_type)
        .collect();
    println!("Steps: {step_types:?}");

    // Note: as of 2026-08-16 the API returns `file_search_result` steps
    // carrying only `call_id` and `signature` — the retrieved chunks
    // themselves are not included. So `has_file_search_results()` is true
    // while `file_search_results()` is empty, and the grounded content is
    // visible only through the model's answer above.
    if response.has_file_search_results() {
        let results = response.file_search_results();
        println!("Retrieved chunks exposed by the API: {}", results.len());
        for (i, item) in results.iter().enumerate() {
            println!("{}. {} (store: {})", i + 1, item.title, item.store);
            let preview: String = item.text.chars().take(100).collect();
            println!("   Preview: {preview}...");
        }
    } else {
        println!("No file search result steps in response");
    }

    // 5. Narrow the retrieval with top_k.
    println!("--- File Search with top_k ---");
    let response = client
        .interaction()
        .with_model(model_name)
        .with_text("What is the error handling policy?")
        .add_tool(FileSearchConfig::new(vec![store_name.to_string()]).with_top_k(3))
        .with_store_enabled()
        .create()
        .await?;

    println!("Status: {:?}", response.status);
    if let Some(text) = response.as_text() {
        let preview: String = text.chars().take(300).collect();
        println!("Response: {preview}\n");
    }

    // 6. File Search cannot be combined with Google Search. The API rejects
    //    the pair outright (verified live 2026-08-16):
    //
    //      400 'google_search' and 'file_search' cannot be combined in the
    //          same request. Please choose one to continue.
    //
    //    To ground an answer against both, run two interactions and combine
    //    the results yourself.
    println!("--- File Search + Google Search is rejected by the API ---");
    let combined = client
        .interaction()
        .with_model(model_name)
        .with_text("Compare my async runtime policy with current Rust community practice.")
        .add_tool(FileSearchConfig::new(vec![store_name.to_string()]))
        .with_google_search()
        .with_store_enabled()
        .create()
        .await;

    match combined {
        Ok(response) => println!(
            "Unexpectedly accepted (status {:?}) — the API may have lifted this restriction.",
            response.status
        ),
        Err(e) => println!("Rejected as expected: {e}"),
    }

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST /fileSearchStores with display_name");
    println!("  [RES#1] 200: store with name + embeddingModel\n");
    println!("  [REQ#2] POST /upload/.../:uploadToFileSearchStore (raw protocol)");
    println!("  [RES#2] 200: operation wrapper carrying response.documentName\n");
    println!("  [REQ#3] GET .../documents/<id> (polled until STATE_ACTIVE)");
    println!("  [RES#3] 200: document with state + sizeBytes (a JSON string)\n");
    println!("  [REQ#4] POST /interactions with input + tools=[file_search]");
    println!("  [RES#4] completed: file_search_call, file_search_result, model_output\n");

    println!("--- Production Considerations ---");
    println!("• Store identifiers are `fileSearchStores/<id>` — use the full");
    println!("  resource name the create response returns, not the display name");
    println!("• Indexing is async: a fresh upload is STATE_PENDING and matches");
    println!("  nothing until STATE_ACTIVE — always wait_for_document_active()");
    println!("• Deleting an indexed document or a non-empty store needs force=true");
    println!("• Use metadata_filter for targeted queries across large document sets");
    println!("• Set top_k to balance result quality vs. token usage");
    println!("• file_search and google_search cannot be combined in one request —");
    println!("  run two interactions and merge the results yourself");
    println!("• file_search_result steps carry no chunk contents today, so the");
    println!("  grounded material is visible only through the model's answer");

    Ok(())
}
