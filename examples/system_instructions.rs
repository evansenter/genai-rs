//! System instructions: personas and behavioral constraints.
//!
//! A system instruction applies only to the request that carries it. It is
//! **not** inherited through `previous_interaction_id` (conversation history
//! is), so a multi-turn conversation must send it on every turn. For
//! guaranteed JSON, use `with_response_format` (see `structured_output`)
//! rather than asking for JSON in the instruction.
//!
//! Run with: `cargo run --example system_instructions`

use genai_rs::Client;
use std::env;
use std::error::Error;

const TUTOR: &str = "You are a math tutor for a 10-year-old. Explain step by step in \
                     simple language, in at most four sentences.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    println!("--- Persona ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_system_instruction(
            "You are a friendly pirate captain. Speak in pirate dialect and keep it brief.",
        )
        .with_text("What's the best way to learn a new skill?")
        .create()
        .await?;
    println!("{}\n", response.as_text().ok_or("no text in response")?);

    println!("--- Behavioral constraint ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_system_instruction(
            "You only help with Rust programming. If asked about anything else, \
             politely redirect to Rust in one sentence.",
        )
        .with_text("What's a good recipe for banana bread?")
        .create()
        .await?;
    println!("{}\n", response.as_text().ok_or("no text in response")?);

    println!("--- Multi-turn: resend the instruction on every turn ---");
    let first = client
        .interaction()
        .with_model(model)
        .with_system_instruction(TUTOR)
        .with_text("What is multiplication?")
        .create()
        .await?;
    println!(
        "Turn 1: {}\n",
        first.as_text().ok_or("no text in response")?
    );

    let second = client
        .interaction()
        .with_model(model)
        .with_previous_interaction(first.id.as_deref().ok_or("stored interaction has no ID")?)
        // Dropping this line would lose the tutor persona: only the
        // conversation history carries over from the previous turn.
        .with_system_instruction(TUTOR)
        .with_text("Can you give me an example with cookies?")
        .create()
        .await?;
    println!("Turn 2: {}", second.as_text().ok_or("no text in response")?);

    Ok(())
}
