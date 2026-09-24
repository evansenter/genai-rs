//! The smallest useful request: one prompt in, text and usage out.
//!
//! Run with: `cargo run --example simple_interaction`
//! (`LOUD_WIRE=1` prints the request and response on the wire.)

use genai_rs::Client;
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let prompt = "Explain the concept of recursion in programming in one paragraph.";
    println!("Model: {}\nPrompt: {prompt}\n", genai_rs::DEFAULT_MODEL);

    // Every failure mode (HTTP, API status, malformed response) arrives as a
    // `GenaiError`; `?` is all most callers need. Match on
    // `GenaiError::Api { status_code, .. }` when a status needs special handling.
    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text(prompt)
        .create()
        .await?;

    println!("Status: {:?}", response.status);
    println!("Interaction ID: {:?}\n", response.id);
    println!("{}", response.as_text().ok_or("response had no text")?);

    if let Some(total) = response.usage.as_ref().and_then(|u| u.total_tokens) {
        println!("\nTotal tokens: {total}");
    }

    Ok(())
}
