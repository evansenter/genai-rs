//! Streaming: print text as it arrives, then resume an interrupted stream.
//!
//! Part 1 handles every `StreamChunk` variant. Part 2 abandons a stream
//! partway through (standing in for a dropped connection) and picks it up
//! again with `get_interaction_stream(id, Some(last_event_id))`.
//!
//! Resuming needs a background interaction: only then does the server keep
//! generating after the client disconnects and tag events with IDs. A GET
//! stream of an ordinary interaction is rejected ("Streaming retrieval of
//! interactions is not supported for this model").
//!
//! Run with: `cargo run --example streaming`

use futures_util::StreamExt;
use genai_rs::{Client, StreamChunk};
use std::env;
use std::error::Error;
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    // -------------------------------------------------------------------------
    // Part 1: the event lifecycle
    // -------------------------------------------------------------------------
    println!("--- Streaming a response ---\n");
    let mut stream = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("Write a four-line poem about programming.")
        .create_stream();

    // Lifecycle events go to stderr so stdout carries only the model's text.
    while let Some(event) = stream.next().await {
        match event?.chunk {
            StreamChunk::Created { interaction } => eprintln!("[created] id={:?}", interaction.id),
            StreamChunk::StatusUpdate { status, .. } => eprintln!("[status] {status:?}"),
            StreamChunk::StepStart { index, step } => {
                eprintln!("[step {index} start] {}", step.step_type());
            }
            StreamChunk::StepDelta { delta, .. } => {
                if let Some(text) = delta.as_text() {
                    print!("{text}");
                    io::stdout().flush()?;
                }
            }
            StreamChunk::StepStop { index, .. } => eprintln!("\n[step {index} stop]"),
            StreamChunk::Completed(response) => {
                let total = response.usage.as_ref().and_then(|u| u.total_tokens);
                eprintln!(
                    "[completed] status={:?} total_tokens={total:?}",
                    response.status
                );
            }
            // Terminal: the API reports a failure inside an otherwise healthy stream.
            StreamChunk::Error { message, code } => {
                return Err(format!("stream error ({code:?}): {message}").into());
            }
            // `StreamChunk` is non-exhaustive; new event types land here.
            other => eprintln!("[unrecognized event] {:?}", other.unknown_chunk_type()),
        }
    }

    // -------------------------------------------------------------------------
    // Part 2: resume from the last event ID
    // -------------------------------------------------------------------------
    println!("\n--- Interrupting and resuming a stream ---\n");
    let mut stream = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("List the planets of the solar system, one per line, with one fact each.")
        .with_background(true)
        .create_stream();

    let mut interaction_id = None;
    let mut last_event_id = None;
    while let Some(event) = stream.next().await {
        let event = event?;
        // Only events that carry an ID can be resumed from.
        if event.event_id.is_some() {
            last_event_id = event.event_id.clone();
        }
        match event.chunk {
            StreamChunk::Created { interaction } => interaction_id = interaction.id,
            StreamChunk::StepDelta { delta, .. } => {
                if let Some(text) = delta.as_text() {
                    print!("{text}");
                    io::stdout().flush()?;
                }
            }
            StreamChunk::Error { message, code } => {
                return Err(format!("stream error ({code:?}): {message}").into());
            }
            _ => {}
        }
        // Drop the connection at the first point we could resume from.
        if last_event_id.is_some() {
            break;
        }
    }
    drop(stream);

    let interaction_id = interaction_id.ok_or("stream never reported an interaction ID")?;
    let last_event_id = last_event_id.ok_or("stream carried no event IDs to resume from")?;
    eprintln!("\n[connection dropped after event {last_event_id}; resuming]");

    let mut resumed = client.get_interaction_stream(&interaction_id, Some(&last_event_id));
    let mut completed = false;
    while let Some(event) = resumed.next().await {
        match event?.chunk {
            StreamChunk::StepDelta { delta, .. } => {
                if let Some(text) = delta.as_text() {
                    print!("{text}");
                    io::stdout().flush()?;
                }
            }
            StreamChunk::Completed(response) => {
                eprintln!("\n[completed] status={:?}", response.status);
                completed = true;
            }
            StreamChunk::Error { message, code } => {
                return Err(format!("resumed stream error ({code:?}): {message}").into());
            }
            _ => {}
        }
    }
    if !completed {
        return Err("resumed stream ended without a completed event".into());
    }

    Ok(())
}
