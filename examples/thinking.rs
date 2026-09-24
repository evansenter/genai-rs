//! Thinking: reasoning depth and readable thought summaries.
//!
//! `with_thinking_level()` sets how much the model reasons before answering;
//! higher levels spend more thought tokens. The reasoning itself is not
//! returned — a `Step::Thought` carries an opaque signature (context to
//! replay in stateless history, see `explicit_turns`) and, only when
//! `with_thinking_summaries(ThinkingSummaries::Auto)` is set, a readable
//! summary.
//!
//! `ThinkingLevel::Minimal` is model-dependent: `DEFAULT_MODEL` rejects it,
//! so this example sends it to `MINIMAL_THINKING_MODEL`.
//!
//! Run with: `cargo run --example thinking`

use futures_util::StreamExt;
use genai_rs::{Client, StepDelta, StreamChunk, ThinkingLevel, ThinkingSummaries};
use std::env;
use std::error::Error;
use std::io::{Write, stdout};

const PROBLEM: &str = "A train travels 120 miles in 2 hours, stops for 30 minutes, then travels \
                       60 miles in 1 hour. What is its average speed for the whole journey? \
                       Answer in one sentence.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    println!("--- Thinking levels ---");
    let levels = [
        (genai_rs::MINIMAL_THINKING_MODEL, ThinkingLevel::Minimal),
        (genai_rs::DEFAULT_MODEL, ThinkingLevel::Low),
        (genai_rs::DEFAULT_MODEL, ThinkingLevel::High),
    ];
    for (model, level) in levels {
        let response = client
            .interaction()
            .with_model(model)
            .with_text(PROBLEM)
            .with_thinking_level(level.clone())
            .create()
            .await?;
        println!(
            "{level:?} ({model}): thought tokens {:?}\n  {}",
            response.thought_tokens(),
            response.as_text().ok_or("no text in response")?
        );
    }

    println!("\n--- Thought summaries ---");
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text(PROBLEM)
        .with_thinking_level(ThinkingLevel::Medium)
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .create()
        .await?;
    for summary in response.thought_summaries() {
        if let Some(text) = summary.as_text() {
            println!("[thought] {text}");
        }
    }
    println!(
        "[answer] {}",
        response.as_text().ok_or("no text in response")?
    );

    println!("\n--- Streaming thought summaries ---");
    let mut stream = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("Why is the sky blue? Two sentences.")
        .with_thinking_level(ThinkingLevel::Medium)
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .create_stream();

    while let Some(event) = stream.next().await {
        match event?.chunk {
            StreamChunk::StepStart { step, .. } => println!("\n[{}]", step.step_type()),
            StreamChunk::StepDelta { delta, .. } => {
                let text = match &delta {
                    StepDelta::ThoughtSummary { content } => {
                        content.as_ref().and_then(|c| c.as_text())
                    }
                    other => other.as_text(),
                };
                if let Some(text) = text {
                    print!("{text}");
                    stdout().flush()?;
                }
            }
            StreamChunk::Error { message, code } => {
                return Err(format!("stream error ({code:?}): {message}").into());
            }
            _ => {}
        }
    }
    println!();

    Ok(())
}
