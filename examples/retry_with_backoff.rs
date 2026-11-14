//! Retrying transient failures with the `backon` crate.
//!
//! `InteractionBuilder::build()` produces an `InteractionRequest` that is
//! `Clone`, so the same request can be re-sent with `Client::execute()`.
//! `GenaiError::is_retryable()` picks out transport failures, timeouts, 408s,
//! 429s and the 5xx a retry can fix, and `retry_after()` carries the
//! server's suggested delay when it sends one, which this waits for up to
//! the backoff's cap. See `docs/RELIABILITY.md`.
//!
//! Run with: `cargo run --example retry_with_backoff`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "an example may stop on a missing key or a poisoned lock; the library may not"
)]

use backon::{ExponentialBuilder, Retryable};
use genai_rs::{Client, GenaiError, InteractionRequest};
use std::env;
use std::error::Error;
use std::time::Duration;

/// The longest wait between attempts.
const MAX_DELAY: Duration = Duration::from_secs(30);

/// The wait before the next attempt, or `None` once the attempts are used
/// up (keep it that way, so a server that keeps sending `Retry-After` can't
/// extend the budget): the server's `Retry-After` when it sent one, else
/// backon's delay, but never past `MAX_DELAY`. `adjust` replaces backon's
/// delay after `with_max_delay` has capped it and the jitter has added up
/// to as much again, so without the `min` a server asking for an hour gets
/// one.
fn next_delay(e: &GenaiError, backoff: Option<Duration>) -> Option<Duration> {
    backoff.map(|d| e.retry_after().unwrap_or(d).min(MAX_DELAY))
}

/// The error and each `source()` below it: `GenaiError::Http`'s `Display`
/// leaves the transport error to its source.
fn chain(e: &(dyn Error + 'static)) -> String {
    std::iter::successors(Some(e), |&e| e.source())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(": ")
}

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
        .with_max_delay(MAX_DELAY)
        .with_max_times(3)
        .with_jitter();

    let response = (|| async { client.execute(request.clone()).await })
        .retry(backoff)
        .when(GenaiError::is_retryable)
        .adjust(next_delay)
        .notify(|e, delay| eprintln!("retryable error ({}); retrying in {delay:?}", chain(e)))
        .await?;

    println!("{}", response.as_text().ok_or("no text in response")?);
    Ok(())
}
