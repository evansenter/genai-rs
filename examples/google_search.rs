//! Example: Google Search Grounding with Gemini
//!
//! This example demonstrates how to use Gemini's built-in Google Search
//! grounding capability to get responses informed by real-time web data.
//!
//! Shows both non-streaming and streaming usage.
//!
//! Run with: cargo run --example google_search

use futures_util::StreamExt;
use genai_rs::{Client, GenaiError, StreamChunk};
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    // 2. Create an interaction with Google Search enabled
    let model_name = "gemini-3-flash-preview";
    let prompt = "What are the latest developments in Rust programming language in 2024? \
                  Include specific version numbers and features.";

    println!("Creating interaction with model: {model_name}");
    println!("Prompt: {prompt}\n");

    // 3. Send the interaction request with Google Search grounding
    match client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .with_google_search() // Enable real-time web search
        .with_store_enabled()
        .create()
        .await
    {
        Ok(response) => {
            println!("--- Google Search Grounded Response ---");
            println!("Interaction ID: {:?}", response.id);
            println!("Status: {:?}\n", response.status);

            // 4. Display the model's text response
            if let Some(text) = response.as_text() {
                println!("Model Response:\n{text}\n");
            }

            // 5. Check if response is grounded and display sources
            if response.has_google_search_calls() || response.has_google_search_results() {
                println!("--- Grounding Information ---");

                // Display search queries from GoogleSearchCall steps
                let search_queries = response.google_search_calls();
                if !search_queries.is_empty() {
                    println!("Search Queries:");
                    for query in &search_queries {
                        println!("  - {query}");
                    }
                    println!();
                }

                // Display web sources from GoogleSearchResult steps
                let search_results = response.google_search_results();
                if !search_results.is_empty() {
                    println!("Web Sources ({} total):", search_results.len());
                    for (i, result) in search_results.iter().enumerate() {
                        println!("  {}. {}", i + 1, result.title);
                        println!("     {}", result.url);
                    }
                }
            } else {
                println!("Note: No grounding metadata returned (may vary by API response)");
            }

            // 6. Display inline citations (annotations)
            if response.has_annotations() {
                println!("\n--- Inline Citations ---");
                let text = response.all_text();
                for annotation in response.all_annotations() {
                    if let Some(span) = annotation.extract_span(&text) {
                        println!(
                            "  \"{}\" (bytes {:?}..{:?}) → {}",
                            span,
                            annotation.start_index(),
                            annotation.end_index(),
                            annotation.source().unwrap_or("<no source>")
                        );
                    }
                }
            }

            // 7. Show token usage
            if let Some(usage) = response.usage {
                println!("\n--- Token Usage ---");
                if let Some(input) = usage.total_input_tokens {
                    println!("  Input tokens: {input}");
                }
                if let Some(output) = usage.total_output_tokens {
                    println!("  Output tokens: {output}");
                }
            }
        }
        Err(e) => {
            match &e {
                GenaiError::Api {
                    status_code,
                    message,
                    request_id,
                    ..
                } => {
                    eprintln!("API Error (HTTP {}): {}", status_code, message);
                    if let Some(id) = request_id {
                        eprintln!("  Request ID: {}", id);
                    }
                    if message.contains("not supported") || message.contains("not available") {
                        eprintln!(
                            "Note: Google Search grounding may not be available in all regions or accounts."
                        );
                    }
                }
                GenaiError::Http(http_err) => eprintln!("HTTP Error: {http_err}"),
                GenaiError::Json(json_err) => eprintln!("JSON Error: {json_err}"),
                _ => eprintln!("Error: {e}"),
            }
            return Err(e.into());
        }
    }

    println!("\n--- End Non-Streaming Response ---");

    // 7. Streaming example with Google Search
    println!("\n=== Streaming with Google Search ===\n");

    let stream_prompt = "What are the top 3 tech news stories today?";
    println!("Prompt: {stream_prompt}\n");
    println!("Response (streaming):");

    let mut stream = client
        .interaction()
        .with_model(model_name)
        .with_text(stream_prompt)
        .with_google_search()
        .create_stream();

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event.chunk {
                StreamChunk::StepDelta { delta, .. } => {
                    if let Some(text) = delta.as_text() {
                        print!("{}", text);
                        stdout().flush()?;
                    }
                }
                StreamChunk::Completed(response) => {
                    println!("\n");
                    let search_results = response.google_search_results();
                    if !search_results.is_empty() {
                        println!("Sources ({} total):", search_results.len());
                        for result in search_results.iter().take(3) {
                            println!("  - {}", result.title);
                        }
                    }
                }
                _ => {} // Handle unknown variants
            },
            Err(e) => {
                eprintln!("\nStream error: {e}");
                break;
            }
        }
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Google Search Grounding Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_google_search() enables real-time web search grounding");
    println!("• response.google_search_calls() returns the search queries executed");
    println!("• response.google_search_results() returns sources with title/url");
    println!("• response.all_annotations() links text spans to sources (inline citations)");
    println!("• Works with both streaming and non-streaming requests\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Non-streaming:");
    println!("  [REQ#1] POST with input + googleSearch tool");
    println!(
        "  [RES#1] completed: text with annotations + google_search_call + google_search_result\n"
    );
    println!("Streaming:");
    println!("  [REQ#2] POST streaming with input + googleSearch tool");
    println!("  [RES#2] SSE stream: text deltas → completed with grounding outputs\n");

    println!("--- Production Considerations ---");
    println!("• Google Search may not be available in all regions/accounts");
    println!("• Display source citations to users for transparency");
    println!("• Grounding adds latency - use only when real-time data needed");
    println!("• Combine with structured output for grounded JSON responses");

    Ok(())
}
