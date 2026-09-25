//! Server-side conversation state: chain turns with `previous_interaction_id`,
//! read a stored interaction back, and delete what the example created.
//!
//! Interactions are stored by default; that is what makes the ID in each
//! response usable as the next turn's `previous_interaction_id`. Only
//! conversation history is inherited this way — `system_instruction` and
//! `tools` must be sent again on every turn that needs them.
//!
//! Run with: `cargo run --example stateful_interaction`

use genai_rs::{Client, GenaiError};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let turns = [
        "My name is Alice and I like programming in Rust.",
        "What is my name and what language do I like?",
        "Why might someone like that language? One sentence.",
    ];

    let mut ids: Vec<String> = Vec::new();
    for prompt in turns {
        println!("User: {prompt}");
        let mut builder = client
            .interaction()
            .with_model(genai_rs::DEFAULT_MODEL)
            .with_text(prompt);
        if let Some(previous) = ids.last() {
            builder = builder.with_previous_interaction(previous);
        }
        let response = builder.create().await?;
        println!(
            "Model: {}\n",
            response.as_text().ok_or("no text in response")?
        );
        ids.push(response.id.ok_or("stored interaction returned no ID")?);
    }

    // `get_interaction_with_input` also returns what was sent, not just the output.
    let first = client.get_interaction_with_input(&ids[0]).await?;
    println!(
        "Retrieved {}: status={:?}, input present={}, {} step(s)",
        ids[0],
        first.status,
        first.input.is_some(),
        first.steps.len()
    );

    // Stored interactions count against your storage until they expire;
    // delete them once the conversation is over.
    for id in &ids {
        client.delete_interaction(id).await?;
    }
    match client.get_interaction(&ids[0]).await {
        Err(GenaiError::Api { status_code, .. }) => {
            println!(
                "Deleted {} interactions; a read now returns HTTP {status_code}",
                ids.len()
            );
        }
        Ok(_) => return Err("interaction still readable after delete".into()),
        Err(e) => return Err(e.into()),
    }

    Ok(())
}
