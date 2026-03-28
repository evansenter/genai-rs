//! Example demonstrating Google Maps tool for location-grounded responses.
//!
//! This example shows how to use Gemini's Google Maps tool to find places
//! and get location-grounded responses with structured place data.
//!
//! Run with: cargo run --example google_maps

use genai_rs::{Client, GoogleMapsConfig};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    let model_name = "gemini-3-flash-preview";

    // === Basic Google Maps usage ===
    println!("=== Google Maps: Find Places ===\n");

    let prompt = "Find the best-rated Italian restaurants near the Eiffel Tower in Paris";
    println!("Prompt: {prompt}\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .with_google_maps() // Simple shorthand
        .with_store_enabled()
        .create()
        .await?;

    println!("Status: {:?}", response.status);

    // Access Google Maps results with place data
    if response.has_google_maps_results() {
        let results = response.google_maps_results();
        println!("\nGoogle Maps Results ({} groups):", results.len());
        for result in &results {
            println!("  Call ID: {}", result.call_id);
            for item in result.items {
                if let Some(places) = &item.places {
                    for place in places {
                        println!(
                            "    {} ({})",
                            place.name.as_deref().unwrap_or("(unnamed)"),
                            place.formatted_address.as_deref().unwrap_or("no address"),
                        );
                        if let Some(rating) = place.rating {
                            println!("      Rating: {rating}");
                        }
                    }
                }
            }
        }
    }

    // Display the model's response
    if let Some(text) = response.as_text() {
        println!("\nModel Response:\n{text}");
    }

    // === Using GoogleMapsConfig with widget ===
    println!("\n=== Google Maps: With Widget Token ===\n");

    let prompt = "Find coffee shops in downtown Seattle";
    println!("Prompt: {prompt}\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_text(prompt)
        .add_tool(GoogleMapsConfig::new().with_widget()) // Config with widget enabled
        .with_store_enabled()
        .create()
        .await?;

    if response.has_google_maps_results() {
        let results = response.google_maps_results();
        for result in &results {
            for item in result.items {
                if let Some(token) = &item.widget_context_token {
                    println!(
                        "  Widget context token: {}...",
                        &token[..30.min(token.len())]
                    );
                }
                if let Some(places) = &item.places {
                    println!("  Found {} places", places.len());
                }
            }
        }
    }

    if let Some(text) = response.as_text() {
        println!("\nModel Response:\n{text}");
    }

    // Show content summary
    let summary = response.content_summary();
    println!("\nContent summary: {summary}");

    // Show token usage
    if let Some(usage) = response.usage {
        println!("\nToken Usage:");
        if let Some(input) = usage.total_input_tokens {
            println!("  Input tokens: {input}");
        }
        if let Some(output) = usage.total_output_tokens {
            println!("  Output tokens: {output}");
        }
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Google Maps Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("  with_google_maps() enables location-grounded responses");
    println!("  add_tool(GoogleMapsConfig::new().with_widget()) enables widget tokens");
    println!("  response.google_maps_results() returns structured place data");
    println!("  Place struct includes name, address, coordinates, rating, and more");
    println!("  Unknown Place fields are preserved via the `extra` field (Evergreen)\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + google_maps tool");
    println!("  [RES#1] completed: google_maps_call + google_maps_result + text\n");

    println!("--- Production Considerations ---");
    println!("  Google Maps results may vary by region and query specificity");
    println!("  Widget context tokens are for rendering interactive map widgets");
    println!("  Place data fields are all optional - check before accessing");
    println!("  The `extra` field on Place captures new API fields automatically");

    Ok(())
}
