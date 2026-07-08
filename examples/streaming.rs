//! Streaming Example
//!
//! This example demonstrates streaming responses from the Gemini API,
//! where text is printed as it arrives rather than waiting for the complete response.
//!
//! It shows how to handle all streaming event types:
//! - `Created`: Interaction accepted, provides early access to interaction ID
//! - `StatusUpdate`: Status changes during processing
//! - `StepStart`: A new step begins (model output, thought, tool call, ...)
//! - `StepDelta`: Incremental step content (text, thought summary, function args)
//! - `StepStop`: Step ends, with optional per-step usage
//! - `Completed`: Final complete interaction response
//! - `Error`: Error occurred during streaming
//!
//! # Running
//!
//! ```bash
//! cargo run --example streaming
//! ```
//!
//! With debug logging to see all SSE events:
//! ```bash
//! LOUD_WIRE=1 cargo run --example streaming
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use futures_util::StreamExt;
use genai_rs::{Client, StepDelta, StreamChunk};
use std::env;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== STREAMING EXAMPLE ===\n");

    let prompt = "Write a short poem about programming. Be creative!";
    println!("User: {}\n", prompt);
    println!("Assistant (streaming): ");

    // Create a streaming request
    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .with_store_enabled()
        .create_stream();

    // Track statistics for each event type
    let mut created_count = 0;
    let mut status_update_count = 0;
    let mut step_start_count = 0;
    let mut delta_count = 0;
    let mut step_stop_count = 0;
    let mut complete_count = 0;
    let mut total_chars = 0;
    let mut interaction_id: Option<String> = None;
    // Track last event_id for potential stream resumption
    let mut last_event_id: Option<String> = None;

    // Process the stream as events arrive
    // Each StreamEvent contains a chunk and an event_id for resume support
    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                // Save event_id for potential resume (useful if connection drops)
                if event.event_id.is_some() {
                    last_event_id = event.event_id.clone();
                }

                match &event.chunk {
                    StreamChunk::Created { interaction } => {
                        // Interaction has been created - provides early access to interaction ID
                        created_count += 1;
                        interaction_id = interaction.id.clone();
                        eprintln!(
                            "[Created] Interaction created: id={:?}, status={:?}",
                            interaction.id, interaction.status
                        );
                    }
                    StreamChunk::StatusUpdate {
                        interaction_id: id,
                        status,
                    } => {
                        // Status change during processing (common for background/agent interactions)
                        status_update_count += 1;
                        eprintln!("[StatusUpdate] id={}, status={:?}", id, status);
                    }
                    StreamChunk::StepStart { index, step } => {
                        // A new step begins at this output position
                        step_start_count += 1;
                        eprintln!(
                            "[StepStart] index={}, step_type={}",
                            index,
                            step.step_type()
                        );
                    }
                    StreamChunk::StepDelta { index: _, delta } => {
                        delta_count += 1;
                        // Print text deltas as they arrive
                        if let Some(text) = delta.as_text() {
                            print!("{}", text);
                            io::stdout().flush()?; // Flush to show immediately
                            total_chars += text.len();
                        }
                        // Handle thought signature deltas (thinking mode)
                        // Note: Signatures are cryptographic tokens, not readable text
                        if let StepDelta::ThoughtSignature {
                            signature: Some(_), ..
                        } = delta
                        {
                            eprintln!("[Thought] (signature present)");
                        }
                    }
                    StreamChunk::StepStop {
                        index,
                        usage: _,
                        step_usage,
                    } => {
                        // Step ends at this output position
                        step_stop_count += 1;
                        eprintln!(
                            "\n[StepStop] index={}, has_step_usage={}",
                            index,
                            step_usage.is_some()
                        );
                    }
                    StreamChunk::Completed(response) => {
                        // Final response with full metadata
                        complete_count += 1;
                        println!("\n");
                        println!("--- Stream Complete ---");
                        println!("Interaction ID: {:?}", response.id);
                        println!("Status: {:?}", response.status);
                        if let Some(usage) = &response.usage
                            && let Some(total) = usage.total_tokens
                        {
                            println!("Total tokens: {}", total);
                        }
                    }
                    StreamChunk::Error { message, code } => {
                        // Error occurred during streaming - terminal event
                        eprintln!("\n[Error] message={}, code={:?}", message, code);
                        break;
                    }
                    _ => {
                        // Unknown variant - forward compatibility for new event types
                        eprintln!("[Unknown] Received unrecognized event type");
                    }
                }
            }
            Err(e) => {
                eprintln!("\nStream error: {:?}", e);
                break;
            }
        }
    }

    println!("\n--- Stream Stats ---");
    println!("Interaction ID: {:?}", interaction_id);
    println!("Created events: {}", created_count);
    println!("StatusUpdate events: {}", status_update_count);
    println!("StepStart events: {}", step_start_count);
    println!("StepDelta chunks received: {}", delta_count);
    println!("StepStop events: {}", step_stop_count);
    println!("Complete events: {}", complete_count);
    println!("Total characters: {}", total_chars);
    if let Some(event_id) = &last_event_id {
        println!(
            "Last event_id: {} (can be used to resume if needed)",
            event_id
        );
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Streaming Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• create_stream() returns a Stream of StreamEvent (chunk + event_id)");
    println!("• StreamChunk event lifecycle:");
    println!("    1. Created - Interaction accepted (provides early access to ID)");
    println!("    2. StatusUpdate - Status changes (for background/agent interactions)");
    println!("    3. StepStart - Step begins (with index and step type)");
    println!("    4. StepDelta - Incremental text/thought/function-arg content");
    println!("    5. StepStop - Step ends (with optional per-step usage)");
    println!("    6. Completed - Final response with usage metadata");
    println!("• Error events indicate terminal failures");
    println!("• event_id can be saved for stream resumption after disconnection\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input text + model + store:true");
    println!(
        "  [RES#1] SSE stream: interaction.created → step.start → step.delta(s) → step.stop → interaction.completed\n"
    );

    println!("--- Production Considerations ---");
    println!("• Handle stream errors gracefully (connection drops, timeouts)");
    println!("• Use buffering strategies for high-frequency deltas");
    println!("• Save event_id to resume streams after network interruptions");
    println!("• StreamChunk::Completed contains the same data as non-streaming response");
    println!("• Use chunk.interaction_id() to track which interaction events belong to\n");

    println!("--- Resume Pattern ---");
    println!("  // If connection drops, resume from last_event_id:");
    println!("  // let resumed_stream = client.get_interaction_stream(");
    println!("  //     &interaction_id,");
    println!("  //     Some(&last_event_id),");
    println!("  // );");

    Ok(())
}
