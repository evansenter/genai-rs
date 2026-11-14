//! Deep Research: a long-running agent, polled to completion or cancelled.
//!
//! Agent interactions run in the background: `create()` returns at once with
//! an in-progress interaction, and you poll `interactions().get(id)` (or register
//! a webhook, see `webhooks_and_background`). If the wait exceeds your budget,
//! `interactions().cancel(id)` stops the agent so it stops consuming tokens.
//!
//! The poll is the one in `docs/AGENTS_AND_BACKGROUND.md` ("Polling"): a
//! transient failure (a dropped connection, a 429, a 503) does not end it,
//! each status request gets only the time left, and the wait is the backoff
//! or the server's `Retry-After`, whichever is longer.
//!
//! Research takes minutes. Set `DEEP_RESEARCH_MAX_WAIT_SECS` (default 900)
//! to change the budget; a small value exercises the cancel path.
//!
//! Run with: `cargo run --example deep_research`

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "an example may stop on a missing key or a poisoned lock; the library may not"
)]

use genai_rs::{
    Client, DeepResearchConfig, InteractionResponse, InteractionStatus, ThinkingSummaries,
};
use std::env;
use std::error::Error;
use std::time::Duration;
use tokio::time::{Instant, sleep, timeout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let client = Client::builder(api_key).build()?;
    let max_wait = Duration::from_secs(match env::var("DEEP_RESEARCH_MAX_WAIT_SECS") {
        Ok(secs) => secs.parse()?,
        Err(_) => 900,
    });

    let started = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("What changed in the most recent stable Rust release? Keep the report short.")
        .with_agent_config(
            DeepResearchConfig::new().with_thinking_summaries(ThinkingSummaries::Auto),
        )
        .with_background(true)
        .create()
        .await?;
    let id = started
        .id
        .clone()
        .ok_or("background interaction has no ID")?;
    println!(
        "Started {id} (status {:?}); waiting up to {max_wait:?}",
        started.status
    );

    match poll(&client, &id, max_wait).await? {
        Some(report) => {
            let summaries = report.thought_summaries().len();
            println!(
                "\n{}",
                report.as_text().ok_or("completed without a report")?
            );
            println!("\n({summaries} thought summaries along the way)");
        }
        None => {
            let cancelled = client.interactions().cancel(&id).await?;
            println!(
                "Gave up after {max_wait:?}; cancelled, status now {:?}",
                cancelled.status
            );
        }
    }
    Ok(())
}

/// Polls `id` with exponential backoff until it ends. `Ok(None)` means the
/// budget ran out first.
async fn poll(
    client: &Client,
    id: &str,
    max_wait: Duration,
) -> Result<Option<InteractionResponse>, Box<dyn Error>> {
    let start = Instant::now();
    let deadline = start
        .checked_add(max_wait)
        .ok_or("DEEP_RESEARCH_MAX_WAIT_SECS is too large")?;
    let mut delay = Duration::from_secs(2);
    loop {
        let mut wait = delay;
        // Each request gets only the time left, so a stalled connection
        // can't hold the poll past its budget.
        let left = deadline.saturating_duration_since(Instant::now());
        match timeout(left, client.interactions().get(id)).await {
            Err(_) => return Ok(None),
            Ok(Ok(response)) => {
                println!("  {:>4}s  {:?}", start.elapsed().as_secs(), response.status);
                match response.status {
                    InteractionStatus::Completed => return Ok(Some(response)),
                    InteractionStatus::Failed
                    | InteractionStatus::Cancelled
                    | InteractionStatus::Incomplete
                    | InteractionStatus::BudgetExceeded => {
                        return Err(format!("research ended as {:?}", response.status).into());
                    }
                    // InProgress, and any status this crate doesn't know
                    // yet: keep polling, bounded by the budget below.
                    _ => {}
                }
            }
            // A transient failure does not end the run: poll on, waiting at
            // least as long as the server asked.
            Ok(Err(e)) if e.is_retryable() => {
                println!("  {:>4}s  {e}; polling on", start.elapsed().as_secs());
                wait = e.retry_after().map_or(delay, |asked| asked.max(delay));
            }
            Ok(Err(e)) => return Err(e.into()),
        }
        // Give up now rather than sleep into the deadline. (Comparing
        // durations, not `Instant::now() + wait`, which panics on a
        // Retry-After too large to add.)
        if wait >= deadline.saturating_duration_since(Instant::now()) {
            return Ok(None);
        }
        sleep(wait).await;
        delay = (delay * 2).min(Duration::from_secs(10));
    }
}
