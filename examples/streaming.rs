//! Streaming Example
//!
//! This example demonstrates streaming responses from the Gemini API,
//! where text is printed as it arrives rather than waiting for the complete response.
//!
//! It shows how to handle all streaming event types:
//! - `Start`: Interaction accepted, provides early access to interaction ID
//! - `StatusUpdate`: Status changes during processing
//! - `ContentStart`: Content generation begins for an output
//! - `Delta`: Incremental content (text, thought, function_call)
//! - `ContentStop`: Content generation ends for an output
//! - `Complete`: Final complete interaction response
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
use genai_rs::{Client, StreamChunk};
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
    let mut start_count = 0;
    let mut status_update_count = 0;
    let mut content_start_count = 0;
    let mut delta_count = 0;
    let mut content_stop_count = 0;
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
                    StreamChunk::Start { interaction } => {
                        // Interaction has started - provides early access to interaction ID
                        start_count += 1;
                        interaction_id = interaction.id.clone();
                        eprintln!(
                            "[Start] Interaction started: id={:?}, status={:?}",
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
                    StreamChunk::ContentStart {
                        index,
                        content_type,
                    } => {
                        // Content generation begins for an output position
                        content_start_count += 1;
                        eprintln!(
                            "[ContentStart] index={}, content_type={:?}",
                            index, content_type
                        );
                    }
                    StreamChunk::Delta(delta) => {
                        delta_count += 1;
                        // Print text deltas as they arrive
                        if let Some(text) = delta.as_text() {
                            print!("{}", text);
                            io::stdout().flush()?; // Flush to show immediately
                            total_chars += text.len();
                        }
                        // Handle thought deltas (thinking mode)
                        // Note: Thoughts contain cryptographic signatures, not readable text
                        if delta.is_thought() && delta.thought_signature().is_some() {
                            eprintln!("[Thought] (signature present)");
                        }
                    }
                    StreamChunk::ContentStop { index } => {
                        // Content generation ends for an output position
                        content_stop_count += 1;
                        eprintln!("\n[ContentStop] index={}", index);
                    }
                    StreamChunk::Complete(response) => {
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
    println!("Start events: {}", start_count);
    println!("StatusUpdate events: {}", status_update_count);
    println!("ContentStart events: {}", content_start_count);
    println!("Delta chunks received: {}", delta_count);
    println!("ContentStop events: {}", content_stop_count);
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
    println!("    1. Start - Interaction accepted (provides early access to ID)");
    println!("    2. StatusUpdate - Status changes (for background/agent interactions)");
    println!("    3. ContentStart - Content generation begins (with index and type)");
    println!("    4. Delta - Incremental text/thought content");
    println!("    5. ContentStop - Content generation ends");
    println!("    6. Complete - Final response with usage metadata");
    println!("• Error events indicate terminal failures");
    println!("• event_id can be saved for stream resumption after disconnection\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input text + model + store:true");
    println!(
        "  [RES#1] SSE stream: interaction.start → content.start → content.delta(s) → content.stop → interaction.complete\n"
    );

    println!("--- Production Considerations ---");
    println!("• Handle stream errors gracefully (connection drops, timeouts)");
    println!("• Use buffering strategies for high-frequency deltas");
    println!("• Save event_id to resume streams after network interruptions");
    println!("• StreamChunk::Complete contains the same data as non-streaming response");
    println!("• Use chunk.interaction_id() to track which interaction events belong to\n");

    println!("--- Resume Pattern ---");
    println!("  // If connection drops, resume from last_event_id:");
    println!("  // let resumed_stream = client.get_interaction_stream(");
    println!("  //     &interaction_id,");
    println!("  //     Some(&last_event_id),");
    println!("  //     false, // set to true to include original input");
    println!("  // );");

    Ok(())
}
