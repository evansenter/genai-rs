//! Example: File Search for Semantic Document Retrieval
//!
//! This example demonstrates how to use the File Search tool for semantic
//! retrieval over document stores. Unlike other tools, File Search requires
//! pre-existing file search stores to be configured in your Google Cloud project.
//!
//! **Prerequisites:**
//! 1. Create a file search store in Google AI Studio or via the API
//! 2. Upload documents to the store
//! 3. Use the store identifier (e.g., "stores/my-store-123") in this example
//!
//! This example shows the API usage patterns but will fail without configured stores.
//!
//! Run with: cargo run --example file_search

use genai_rs::{Client, FileSearchConfig};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    // 2. Configure your file search store identifiers
    // Replace these with your actual store identifiers from Google AI Studio
    let store_names = vec![
        "stores/your-store-id-here".to_string(), // Replace with your store ID
    ];

    let model_name = "gemini-3-flash-preview";
    let prompt = "What information do my documents contain about Rust programming?";

    println!("=== File Search Example ===\n");
    println!("Model: {model_name}");
    println!("Prompt: {prompt}");
    println!("Stores: {store_names:?}\n");

    // 3. Create an interaction with File Search enabled (basic usage)
    println!("--- Basic File Search ---");
    let result = client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .add_tool(FileSearchConfig::new(store_names.clone()))
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(response) => {
            println!("Status: {:?}\n", response.status);

            // Display the model's text response
            if let Some(text) = response.as_text() {
                println!("Model Response:\n{text}\n");
            }

            // Check for file search results
            if response.has_file_search_results() {
                let results = response.file_search_results();
                println!("--- Retrieved Documents ({}) ---", results.len());
                for (i, item) in results.iter().enumerate() {
                    println!("{}. {}", i + 1, item.title);
                    println!("   Store: {}", item.store);
                    let preview: String = item.text.chars().take(100).collect();
                    println!("   Preview: {preview}...");
                    println!();
                }
            } else {
                println!("No file search results in response");
            }
        }
        Err(e) => {
            println!("Error: {e}");
            println!("\nNote: This is expected if you haven't configured file search stores.");
            println!("See the example header for prerequisites.");
        }
    }

    // 4. Advanced: File Search with configuration options
    println!("\n--- File Search with Configuration ---");
    let advanced_result = client
        .interaction()
        .with_model(model_name)
        .with_text("Find technical documentation about async/await")
        .add_tool(
            FileSearchConfig::new(store_names.clone())
                .with_top_k(5) // top_k: limit to 5 results
                .with_metadata_filter("category = 'technical'"), // metadata filter
        )
        .with_store_enabled()
        .create()
        .await;

    match advanced_result {
        Ok(response) => {
            println!("Status: {:?}", response.status);
            if let Some(text) = response.as_text() {
                let preview: String = text.chars().take(200).collect();
                println!("Response: {preview}\n");
            }
        }
        Err(e) => {
            println!("Error: {e}");
        }
    }

    // 5. Combining File Search with other tools
    println!("--- Combining File Search with Google Search ---");
    let combined_result = client
        .interaction()
        .with_model(model_name)
        .with_text("Compare my documentation with current Rust best practices")
        .add_tool(FileSearchConfig::new(store_names))
        .with_google_search() // Add web search for comparison
        .with_store_enabled()
        .create()
        .await;

    match combined_result {
        Ok(response) => {
            println!("Status: {:?}", response.status);

            // Check what tools were used
            let has_file_results = response.has_file_search_results();
            let has_web_results = response.has_google_search_results();
            println!(
                "Tools used - File Search: {}, Google Search: {}",
                has_file_results, has_web_results
            );

            if let Some(text) = response.as_text() {
                let preview: String = text.chars().take(300).collect();
                println!("Response preview: {preview}...");
            }
        }
        Err(e) => {
            println!("Error: {e}");
        }
    }

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + tools=[file_search]");
    println!("  [RES#1] completed: text + file_search_result content\n");

    println!("--- Production Considerations ---");
    println!("• Create file search stores via Google AI Studio or the Stores API");
    println!("• Store identifiers follow pattern: stores/<store-id>");
    println!("• Use metadata_filter for targeted queries across large document sets");
    println!("• Set top_k to balance result quality vs. token usage");
    println!("• Combine with Google Search for RAG + web grounding patterns");
    println!("• File Search results include document title, text snippet, and source store");

    Ok(())
}
