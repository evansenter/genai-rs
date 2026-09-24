//! Deep Research: a long-running agent, polled to completion or cancelled.
//!
//! Agent interactions run in the background: `create()` returns at once with
//! an in-progress interaction, and you poll `get_interaction(id)` (or register
//! a webhook, see `webhooks_and_background`). If the wait exceeds your budget,
//! `cancel_interaction(id)` stops the agent so it stops consuming tokens.
//!
//! Research takes minutes. Set `DEEP_RESEARCH_MAX_WAIT_SECS` (default 900)
//! to change the budget; a small value exercises the cancel path.
//!
//! Run with: `cargo run --example deep_research`

use genai_rs::{
    Client, DeepResearchConfig, InteractionResponse, InteractionStatus, ThinkingSummaries,
};
use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

const MAX_POLL_DELAY: Duration = Duration::from_secs(15);

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

    match poll(&client, started, max_wait).await? {
        Some(report) => {
            let summaries = report.thought_summaries().count();
            println!(
                "\n{}",
                report.as_text().ok_or("completed without a report")?
            );
            println!("\n({summaries} thought summaries along the way)");
        }
        None => {
            let cancelled = client.cancel_interaction(&id).await?;
            println!(
                "Gave up after {max_wait:?}; cancelled, status now {:?}",
                cancelled.status
            );
        }
    }
    Ok(())
}

/// Polls with exponential backoff. `Ok(None)` means the budget ran out.
async fn poll(
    client: &Client,
    mut response: InteractionResponse,
    max_wait: Duration,
) -> Result<Option<InteractionResponse>, Box<dyn Error>> {
    let id = response.id.clone().ok_or("interaction has no ID")?;
    let start = Instant::now();
    let mut delay = Duration::from_secs(2);
    loop {
        match response.status {
            InteractionStatus::Completed => return Ok(Some(response)),
            InteractionStatus::Failed | InteractionStatus::Cancelled => {
                return Err(format!("research ended with status {:?}", response.status).into());
            }
            // InProgress, and any status this crate doesn't know yet: keep
            // polling, bounded by the budget below.
            _ => {}
        }
        if start.elapsed() + delay > max_wait {
            return Ok(None);
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_POLL_DELAY);
        response = client.get_interaction(&id).await?;
        println!("  {:>4}s  {:?}", start.elapsed().as_secs(), response.status);
    }
}
