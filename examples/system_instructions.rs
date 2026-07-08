//! Example: System Instructions with Gemini
//!
//! This example demonstrates how to use system instructions to configure
//! model behavior, set personas, and control output format.
//!
//! System instructions are persistent directives that influence all model
//! responses throughout a conversation.
//!
//! Run with: cargo run --example system_instructions

use futures_util::StreamExt;
use genai_rs::{Client, GenaiError, StreamChunk};
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");
    let client = Client::builder(api_key).build()?;
    let model_name = "gemini-3-flash-preview";

    // =========================================================================
    // Example 1: Setting a Persona
    // =========================================================================
    println!("=== Example 1: Persona (Pirate) ===\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_system_instruction(
            "You are a friendly pirate captain. Speak in pirate dialect, \
             use nautical terms, and occasionally say 'Arrr!' Keep responses brief.",
        )
        .with_text("What's the weather like today?")
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("Pirate says: {text}\n");
    }

    // =========================================================================
    // Example 2: Output Format Control (JSON)
    // =========================================================================
    println!("=== Example 2: JSON Output Format ===\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_system_instruction(
            "Always respond with valid JSON only. No markdown, no explanation. \
             Use this schema: {\"answer\": string, \"confidence\": number 0-100}",
        )
        .with_text("What is the capital of France?")
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("JSON response: {text}\n");

        // Parse to verify it's valid JSON
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(json) => println!("Parsed successfully: {json}\n"),
            Err(e) => println!("Note: Response wasn't pure JSON: {e}\n"),
        }
    }

    // =========================================================================
    // Example 3: Behavioral Constraints
    // =========================================================================
    println!("=== Example 3: Behavioral Constraints ===\n");

    let response = client
        .interaction()
        .with_model(model_name)
        .with_system_instruction(
            "You are a coding assistant that ONLY helps with Rust programming. \
             If asked about other topics, politely redirect to Rust. \
             Always include code examples when relevant.",
        )
        .with_text("How do I write a for loop?")
        .create()
        .await?;

    if let Some(text) = response.as_text() {
        println!("Rust assistant: {text}\n");
    }

    // =========================================================================
    // Example 4: Multi-turn with System Instructions
    // =========================================================================
    println!("=== Example 4: Multi-turn Conversation ===\n");

    // System instructions persist across conversation turns
    let first_response = client
        .interaction()
        .with_model(model_name)
        .with_system_instruction(
            "You are a helpful math tutor. Explain concepts step by step. \
             Use simple language suitable for a 10-year-old.",
        )
        .with_text("What is multiplication?")
        .with_store_enabled()
        .create()
        .await?;

    println!("Turn 1 - Student: What is multiplication?");
    if let Some(text) = first_response.as_text() {
        println!("Tutor: {text}\n");
    }

    // Continue the conversation - system instruction carries forward
    let second_response = client
        .interaction()
        .with_model(model_name)
        .with_text("Can you give me an example with cookies?")
        .with_previous_interaction(first_response.id.as_ref().expect("id should exist"))
        .create()
        .await?;

    println!("Turn 2 - Student: Can you give me an example with cookies?");
    if let Some(text) = second_response.as_text() {
        println!("Tutor: {text}\n");
    }

    // =========================================================================
    // Example 5: Streaming with System Instructions
    // =========================================================================
    println!("=== Example 5: Streaming Response ===\n");

    println!("Storyteller (streaming): ");
    let mut stream = client
        .interaction()
        .with_model(model_name)
        .with_system_instruction(
            "You are a creative storyteller. Tell very short stories (2-3 sentences). \
             Make them whimsical and fun.",
        )
        .with_text("Tell me a story about a cat who learned to code.")
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
                StreamChunk::Completed(_) => {
                    println!("\n");
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
    // Example 6: Handling Errors
    // =========================================================================
    println!("=== Example 6: Error Handling ===\n");

    // Empty system instructions are handled gracefully
    match client
        .interaction()
        .with_model(model_name)
        .with_system_instruction("") // Empty instruction
        .with_text("Hello!")
        .create()
        .await
    {
        Ok(response) => {
            if let Some(text) = response.as_text() {
                println!("Response with empty instruction: {text}\n");
            }
        }
        Err(e) => match &e {
            GenaiError::Api {
                status_code,
                message,
                ..
            } => {
                println!("API Error (HTTP {status_code}): {message}\n");
            }
            _ => println!("Error: {e}\n"),
        },
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ System Instructions Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_system_instruction() sets model behavior for a turn");
    println!("• Available at any point in builder chaining");
    println!("• API does NOT inherit system_instruction via previousInteractionId");
    println!("• Set explicitly on each turn if needed; auto_functions reuses request internally");
    println!("• Use for personas, output format control, and behavioral constraints\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with systemInstruction + input text");
    println!("  [RES#1] completed: text following system instruction\n");
    println!("Multi-turn:");
    println!("  [REQ#2] POST with systemInstruction + input + store:true");
    println!("  [RES#2] completed: text response");
    println!("  [REQ#3] POST with systemInstruction + input + previousInteractionId");
    println!("  [RES#3] completed: response follows system instruction\n");

    println!("--- Production Considerations ---");
    println!("• System instructions consume tokens - keep them concise");
    println!("• Use structured output (with_response_format) for guaranteed JSON");
    println!("• Empty system instructions are handled gracefully");
    println!("• Combine with thinking for complex reasoning personas");

    Ok(())
}
