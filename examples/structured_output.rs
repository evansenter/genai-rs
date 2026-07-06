//! Structured Output Example
//!
//! This example demonstrates how to use JSON schema to enforce structured output
//! from the Gemini API. The model will return responses that conform to your schema.
//!
//! Shows both non-streaming and streaming usage.
//!
//! Run with: cargo run --example structured_output

use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

/// A struct representing the structured output we want from the model.
/// Using serde, we can easily parse the JSON response into this type.
#[derive(Debug, Serialize, Deserialize)]
struct MovieReview {
    title: String,
    year: i32,
    rating: f64,
    genre: String,
    summary: String,
    pros: Vec<String>,
    cons: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;

    println!("=== Structured Output Examples ===\n");

    // Example 1: Basic structured output
    println!("--- Example 1: Basic Structured Output ---");
    basic_structured_output(&client).await?;

    // Example 2: Structured output with complex nested schema
    println!("\n--- Example 2: Complex Nested Schema ---");
    complex_nested_schema(&client).await?;

    // Example 3: Structured output combined with Google Search
    println!("\n--- Example 3: Structured Output + Google Search ---");
    structured_with_search(&client).await?;

    // Example 4: Streaming with structured output
    println!("\n--- Example 4: Streaming Structured Output ---");
    streaming_structured_output(&client).await?;

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Structured Output Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_response_format(schema) enforces JSON schema on model output");
    println!("• Response is guaranteed valid JSON - parse directly with serde");
    println!("• Supports nested objects, arrays, and enums in schema");
    println!("• Works with streaming and can combine with Google Search\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Example 1-2 (basic/nested schemas):");
    println!("  [REQ#1] POST with input + responseFormat schema");
    println!("  [RES#1] completed: JSON matching schema\n");
    println!("Example 3 (structured + search):");
    println!("  [REQ#3] POST with input + responseFormat + googleSearch tool");
    println!("  [RES#3] completed: JSON with grounding metadata\n");
    println!("Example 4 (streaming):");
    println!("  [REQ#4] POST streaming with input + responseFormat");
    println!("  [RES#4] SSE stream: JSON chunks → completed\n");

    println!("--- Production Considerations ---");
    println!("• Define Rust structs matching your schema for type-safe parsing");
    println!("• Use enums in schema to constrain model outputs to valid values");
    println!("• Combine with Google Search for grounded structured data");
    println!("• Schema validation is server-side - invalid prompts may still fail");

    Ok(())
}

/// Basic example: Extract structured movie review data
async fn basic_structured_output(client: &Client) -> Result<(), Box<dyn Error>> {
    // Define the JSON schema for our expected output
    let schema = json!({
        "type": "object",
        "properties": {
            "title": {"type": "string"},
            "year": {"type": "integer"},
            "rating": {"type": "number"},
            "genre": {"type": "string"},
            "summary": {"type": "string"},
            "pros": {
                "type": "array",
                "items": {"type": "string"}
            },
            "cons": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["title", "year", "rating", "genre", "summary", "pros", "cons"]
    });

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Write a review for the movie 'Inception' (2010). Be concise.")
        .with_response_format(schema)
        .create()
        .await?;

    // The response is guaranteed to be valid JSON matching our schema.
    // Note: In production code, handle the Option properly instead of using expect().
    let text = response.as_text().expect("Should have text response");
    println!("Raw JSON response:\n{}\n", text);

    // Parse into our Rust struct
    let review: MovieReview = serde_json::from_str(text)?;
    println!("Parsed MovieReview struct:");
    println!("  Title: {} ({})", review.title, review.year);
    println!("  Rating: {}/10", review.rating);
    println!("  Genre: {}", review.genre);
    println!("  Summary: {}", review.summary);
    println!("  Pros: {:?}", review.pros);
    println!("  Cons: {:?}", review.cons);

    Ok(())
}

/// Complex example: Nested objects and arrays
async fn complex_nested_schema(client: &Client) -> Result<(), Box<dyn Error>> {
    // A more complex schema with nested structures
    let schema = json!({
        "type": "object",
        "properties": {
            "recipe": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "cuisine": {"type": "string"},
                    "difficulty": {
                        "type": "string",
                        "enum": ["easy", "medium", "hard"]
                    },
                    "prep_time_minutes": {"type": "integer"},
                    "cook_time_minutes": {"type": "integer"}
                },
                "required": ["name", "cuisine", "difficulty"]
            },
            "ingredients": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "item": {"type": "string"},
                        "amount": {"type": "string"},
                        "optional": {"type": "boolean"}
                    },
                    "required": ["item", "amount"]
                }
            },
            "steps": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["recipe", "ingredients", "steps"]
    });

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Give me a simple pasta recipe with 5 ingredients and 4 steps.")
        .with_response_format(schema)
        .create()
        .await?;

    // Note: In production code, handle the Option properly instead of using expect().
    let text = response.as_text().expect("Should have text response");

    // Parse and pretty-print the JSON
    let json: serde_json::Value = serde_json::from_str(text)?;
    println!("Recipe JSON:\n{}", serde_json::to_string_pretty(&json)?);

    // Extract and display specific fields
    if let Some(recipe) = json.get("recipe") {
        println!(
            "\nRecipe: {} ({} cuisine, {} difficulty)",
            recipe
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown"),
            recipe
                .get("cuisine")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown"),
            recipe
                .get("difficulty")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
        );
    }

    Ok(())
}

/// Combining structured output with Google Search for real-time data
async fn structured_with_search(client: &Client) -> Result<(), Box<dyn Error>> {
    // Schema for stock information
    let schema = json!({
        "type": "object",
        "properties": {
            "company": {"type": "string"},
            "ticker": {"type": "string"},
            "current_status": {"type": "string"},
            "recent_news": {
                "type": "array",
                "items": {"type": "string"}
            }
        },
        "required": ["company", "ticker", "current_status"]
    });

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("What's the current status of Apple Inc (AAPL) stock? Include any recent news.")
        .with_google_search() // Enable real-time web search
        .with_response_format(schema)
        .create()
        .await?;

    // Note: In production code, handle the Option properly instead of using expect().
    let text = response.as_text().expect("Should have text response");
    let json: serde_json::Value = serde_json::from_str(text)?;
    println!("Stock Info JSON:\n{}", serde_json::to_string_pretty(&json)?);

    // Show grounding sources if available
    let search_results = response.google_search_results();
    if !search_results.is_empty() {
        println!(
            "\nGrounded with {} sources from web search",
            search_results.len()
        );
    }

    Ok(())
}

/// Streaming example: Watch structured JSON being generated in real-time
async fn streaming_structured_output(client: &Client) -> Result<(), Box<dyn Error>> {
    // Simple schema for a product review
    let schema = json!({
        "type": "object",
        "properties": {
            "product": {"type": "string"},
            "rating": {"type": "integer"},
            "pros": {
                "type": "array",
                "items": {"type": "string"}
            },
            "cons": {
                "type": "array",
                "items": {"type": "string"}
            },
            "verdict": {"type": "string"}
        },
        "required": ["product", "rating", "pros", "cons", "verdict"]
    });

    println!("Streaming structured JSON response:");
    print!("  ");

    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Write a brief review for a wireless mouse. Be concise.")
        .with_response_format(schema)
        .create_stream();

    let mut final_response = None;

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
                    final_response = Some(response);
                }
                _ => {} // Handle unknown variants
            },
            Err(e) => {
                eprintln!("\nStream error: {e}");
                break;
            }
        }
    }

    println!("\n");

    // Parse and display the final structured result
    if let Some(text) = final_response.as_ref().and_then(|r| r.as_text()) {
        let json: serde_json::Value = serde_json::from_str(text)?;
        println!("Parsed result:");
        println!(
            "  Product: {}",
            json.get("product").and_then(|v| v.as_str()).unwrap_or("?")
        );
        println!(
            "  Rating: {}/5",
            json.get("rating").and_then(|v| v.as_i64()).unwrap_or(0)
        );
        println!(
            "  Verdict: {}",
            json.get("verdict").and_then(|v| v.as_str()).unwrap_or("?")
        );
    }

    Ok(())
}
