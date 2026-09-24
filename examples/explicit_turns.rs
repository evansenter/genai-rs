//! Client-side conversation history, sent as `Step` arrays.
//!
//! The alternative to `previous_interaction_id` (see `stateful_interaction`):
//! you hold the history and send all of it on every turn. Useful for
//! stateless deployments, your own persistence, or trimming history between
//! turns. Requests here use `with_store_disabled()`, so nothing is kept
//! server-side.
//!
//! 1. `conversation()` builds a history inline
//! 2. `with_history()` sends one you already have
//! 3. A live loop extends history with `response.output_steps()` — which
//!    includes the model's `Step::Thought` entries and their signatures, the
//!    reasoning context a stateless follow-up needs passed back unchanged
//!
//! Run with: `cargo run --example explicit_turns`

use genai_rs::{Client, Step, ThinkingLevel};
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let model = genai_rs::DEFAULT_MODEL;

    println!("--- conversation() builder ---");
    let response = client
        .interaction()
        .with_model(model)
        .with_store_disabled()
        .conversation()
        .user("What is 2+2?")
        .model("2+2 equals 4.")
        .user("And what's that times 3?")
        .done()
        .create()
        .await?;
    println!("{}\n", response.as_text().ok_or("no text in response")?);

    println!("--- with_history() ---");
    let history = vec![
        Step::user_text("I'm planning a trip to Paris."),
        Step::model_text("Wonderful! What are you most interested in?"),
        Step::user_text("Museums and good food. Name one museum, in one sentence."),
    ];
    let response = client
        .interaction()
        .with_model(model)
        .with_store_disabled()
        .with_history(history)
        .create()
        .await?;
    println!("{}\n", response.as_text().ok_or("no text in response")?);

    println!("--- Growing history with output_steps() ---");
    let mut history: Vec<Step> = Vec::new();
    let turns = [
        "A farmer has 17 sheep; all but 9 run away. How many are left? Just the number.",
        "Double that, then subtract 5. Just the number.",
    ];
    for (turn, prompt) in turns.iter().enumerate() {
        history.push(Step::user_text(*prompt));
        let response = client
            .interaction()
            .with_model(model)
            .with_store_disabled()
            .with_thinking_level(ThinkingLevel::Medium)
            .with_history(history.clone())
            .create()
            .await?;
        println!("User: {prompt}");
        println!(
            "Model: {}",
            response.as_text().ok_or("no text in response")?
        );

        // Replaying only the text would drop the thought signatures.
        history.extend(response.output_steps());
        if turn == 0 {
            let signed_thoughts = history
                .iter()
                .filter(|s| {
                    matches!(
                        s,
                        Step::Thought {
                            signature: Some(_),
                            ..
                        }
                    )
                })
                .count();
            if signed_thoughts == 0 {
                return Err("expected a signed Step::Thought in the replayed history".into());
            }
            println!("(history now carries {signed_thoughts} signed thought step(s))");
        }
        println!();
    }

    Ok(())
}
