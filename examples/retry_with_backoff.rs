//! Retrying transient failures with the `backon` crate.
//!
//! `InteractionBuilder::build()` produces an `InteractionRequest` that is
//! `Clone`, so the same request can be re-sent with `Client::execute()`.
//! `GenaiError::is_retryable()` picks out 429s, 5xx and timeouts, and
//! `retry_after()` carries the server's suggested delay when it sends one.
//! See `docs/RELIABILITY.md`.
//!
//! Run with: `cargo run --example retry_with_backoff`

use backon::{ExponentialBuilder, Retryable};
use genai_rs::{Client, GenaiError, InteractionRequest};
use std::env;
use std::error::Error;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;

    let request: InteractionRequest = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text("What is the Rust programming language known for? One sentence.")
        .build()?;

    // backon's exponential backoff has no jitter unless asked for; without
    // it, clients that failed together retry together.
    let backoff = ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(500))
        .with_max_delay(Duration::from_secs(30))
        .with_max_times(3)
        .with_jitter();

    let response = (|| async { client.execute(request.clone()).await })
        .retry(backoff)
        .when(GenaiError::is_retryable)
        // Prefer the server's Retry-After over our own delay when present.
        // `delay` is None once the attempts are used up; keep it that way so
        // a server that keeps sending Retry-After cannot extend the budget.
        .adjust(|e: &GenaiError, delay| delay.map(|d| e.retry_after().unwrap_or(d)))
        .notify(|e, delay| eprintln!("retryable error ({e}); retrying in {delay:?}"))
        .await?;

    println!("{}", response.as_text().ok_or("no text in response")?);
    Ok(())
}
