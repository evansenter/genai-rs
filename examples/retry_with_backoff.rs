//! Demonstrates retry logic with exponential backoff using the `backon` crate.
//!
//! This example shows the recommended approach for handling transient failures
//! like rate limits (429) and server errors (5xx). We use a battle-tested retry
//! library rather than reimplementing retry logic ourselves.
//!
//! Key concepts demonstrated:
//! - `InteractionBuilder::build()` to create a reusable `InteractionRequest`
//! - `Client::execute()` to run a pre-built request
//! - `GenaiError::is_retryable()` to identify transient errors
//! - `backon` crate for production-grade retry with exponential backoff
//!
//! See also: docs/RETRY_PATTERNS.md for our retry philosophy
//!
//! Run with: cargo run --example retry_with_backoff

use backon::{ExponentialBuilder, Retryable};
use genai_rs::{Client, GenaiError, InteractionRequest, InteractionResponse};
use std::env;
use std::error::Error;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Get API key
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    let client = Client::builder(api_key).build()?;

    let model = "gemini-3.6-flash";
    let prompt = "What is the Rust programming language known for? Answer in one sentence.";

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Retry with Backoff Example (using backon crate)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Step 1: Build the request (without executing)
    println!("Step 1: Building request...");
    let request: InteractionRequest = client
        .interaction()
        .with_model(model)
        .with_text(prompt)
        .build()?;

    println!("  Model: {:?}", request.model);
    println!("  Request is Clone + Serialize + Deserialize");
    println!("  Request can be retried, logged, or persisted\n");

    // Step 2: Execute with retry logic using backon
    println!("Step 2: Executing with backon retry...");

    // Configure exponential backoff:
    // - Start with 100ms delay
    // - Double each attempt (100ms → 200ms → 400ms → ...)
    // - Cap at 30 seconds max delay
    // - Jitter is built-in to prevent thundering herd
    let backoff = ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(100))
        .with_max_delay(Duration::from_secs(30))
        .with_max_times(3);

    println!("  Backoff: exponential, 100ms-30s, max 3 retries\n");

    // The retry logic in 4 lines:
    // 1. Wrap the operation in a closure that clones the request
    // 2. Configure backoff strategy
    // 3. Specify which errors to retry (.when())
    // 4. Optionally log retries (.notify())
    let response: InteractionResponse = (|| async {
        // Clone request for each attempt (original preserved for retries)
        client.execute(request.clone()).await
    })
    .retry(backoff)
    .when(|e: &GenaiError| e.is_retryable())
    .notify(|err, dur| {
        println!("⟳ Retryable error: {}", err);
        println!("  Waiting {:?} before retry...", dur);
    })
    .await?;

    // Step 3: Process response
    println!("--- Response ---");
    if let Some(text) = response.as_text() {
        println!("{}", text);
    }

    if let Some(usage) = &response.usage
        && let Some(total) = usage.total_tokens
    {
        println!("\nTokens used: {}", total);
    }

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input text + model");
    println!("  [RES#1] completed: text response with usage stats");
    println!("  (If retries occur, you'll see REQ#2, REQ#3, etc.)\n");

    println!("--- Key Takeaways ---");
    println!("• Use `backon` crate for production retry logic (battle-tested)");
    println!("• `InteractionRequest` is Clone - safe to retry");
    println!("• `is_retryable()` identifies 429, 5xx, and timeout errors");
    println!("• `retry_after()` provides server-suggested delay for 429s");
    println!("• See docs/RETRY_PATTERNS.md for our retry philosophy");

    Ok(())
}
