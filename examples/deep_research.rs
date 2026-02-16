//! Example: Deep Research Agent with Gemini
//!
//! This example demonstrates how to use Gemini's Deep Research agent, which conducts
//! multi-step research tasks by executing iterative searches, synthesizing information
//! across multiple sources, and generating comprehensive research reports.
//!
//! The Deep Research agent operates as a long-running asynchronous operation that
//! requires background mode. The workflow is:
//! 1. Start the research task with `background=true`
//! 2. Poll for completion using the interaction ID
//! 3. Retrieve the final research report
//!
//! **Expected runtime**: Deep research queries typically take 30-120 seconds depending
//! on query complexity. Simple queries may complete in under 30 seconds. This example
//! polls for up to 2 minutes; very complex research may require longer timeouts.
//!
//! Note: The Deep Research agent may not be available in all accounts or regions.
//!
//! Run with: cargo run --example deep_research

use genai_rs::{Client, DeepResearchConfig, GenaiError, InteractionStatus, ThinkingSummaries};
use std::env;
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Maximum time to wait for research to complete when polling
const MAX_POLL_DURATION: Duration = Duration::from_secs(120);

/// Initial delay between polls (will increase with exponential backoff).
///
/// A conservative 2-second initial delay reduces API calls since deep research
/// tasks typically take 30+ seconds to complete.
const INITIAL_POLL_DELAY: Duration = Duration::from_secs(2);

/// Maximum delay between polls
const MAX_POLL_DELAY: Duration = Duration::from_secs(10);

/// Maximum characters to display in results
const DISPLAY_LIMIT: usize = 2000;

/// Truncates text at a safe UTF-8 boundary for display.
fn truncate_for_display(text: &str, limit: usize) -> String {
    if text.len() > limit {
        // Find a safe truncation point at a UTF-8 character boundary
        let safe_limit = text
            .char_indices()
            .take_while(|(i, _)| *i < limit)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!(
            "{}...\n\n[Response truncated, {} total chars]",
            &text[..safe_limit],
            text.len()
        )
    } else {
        text.to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    // The Deep Research agent identifier
    let agent_name = "deep-research-pro-preview-12-2025";

    println!("=== Deep Research Agent Example ===\n");

    // 2. Define the research query
    let prompt = "What are the current best practices for building production-ready \
                  REST APIs in Rust? Include framework comparisons and security considerations.";

    println!("Research query: {prompt}\n");
    println!("Starting deep research (this may take 30-120 seconds)...\n");

    // 3. Start the research in background mode (required for agent interactions)
    //    We also configure agent-specific settings using DeepResearchConfig.
    let result = client
        .interaction()
        .with_agent(agent_name)
        .with_text(prompt)
        // Optional: Configure agent-specific settings
        .with_agent_config(
            DeepResearchConfig::new().with_thinking_summaries(ThinkingSummaries::Auto),
        )
        .with_background(true) // Required for agent interactions
        .with_store_enabled() // Required to retrieve results by interaction ID
        .create()
        .await;

    match result {
        Ok(initial_response) => {
            println!("Research initiated!");
            println!("Initial status: {:?}", initial_response.status);
            println!("Interaction ID: {:?}\n", initial_response.id);

            // 4. If already completed (fast response), display results
            if initial_response.status == InteractionStatus::Completed {
                println!("Research completed immediately (very fast response).\n");
                display_research_results(&initial_response);
                return Ok(());
            }

            // Handle unusual initial statuses
            if initial_response.status == InteractionStatus::RequiresAction {
                eprintln!("Research requires action before continuing.");
                eprintln!("This is unusual for deep research. Check the API response for details.");
                return Err("Interaction requires action".into());
            }

            // 5. Poll for completion with exponential backoff
            println!(
                "Polling for completion (max wait: {:?})...\n",
                MAX_POLL_DURATION
            );

            match poll_for_completion(
                &client,
                initial_response.id.as_ref().expect("id should exist"),
            )
            .await
            {
                Ok(final_response) => {
                    println!("\nResearch completed!");
                    display_research_results(&final_response);
                }
                Err(PollError::Timeout { interaction_id }) => {
                    eprintln!(
                        "\nPolling timed out after {:?}. The research may still be running.",
                        MAX_POLL_DURATION
                    );
                    eprintln!(
                        "You can retrieve results later using interaction ID: {interaction_id}"
                    );
                    // Include interaction_id in error message since Box<dyn Error> loses the typed variant
                    return Err(
                        format!("Research timed out (interaction: {interaction_id})").into(),
                    );
                }
                Err(PollError::Failed { interaction_id }) => {
                    eprintln!("\nResearch task failed (interaction: {interaction_id}).");
                    // Include interaction_id in error message since Box<dyn Error> loses the typed variant
                    return Err(format!("Research failed (interaction: {interaction_id})").into());
                }
                Err(PollError::Api(e)) => {
                    eprintln!("\nAPI error during polling: {e:?}");
                    return Err(e.into());
                }
            }
        }
        Err(e) => {
            handle_research_error(&e);
            return Err(e.into());
        }
    }

    Ok(())
}

/// Error type for polling operations
#[derive(Debug)]
enum PollError {
    /// Polling timed out before completion
    Timeout { interaction_id: String },
    /// The interaction failed or was cancelled
    Failed { interaction_id: String },
    /// An API error occurred
    Api(GenaiError),
}

impl From<GenaiError> for PollError {
    fn from(err: GenaiError) -> Self {
        PollError::Api(err)
    }
}

/// Polls for interaction completion with exponential backoff.
///
/// This function queries the API for the interaction status, using exponential
/// backoff to reduce API calls while still detecting completion quickly.
///
/// Note: This polling logic is implemented inline so users can copy this
/// example directly without external dependencies.
async fn poll_for_completion(
    client: &Client,
    interaction_id: &str,
) -> Result<genai_rs::InteractionResponse, PollError> {
    let start = Instant::now();
    let mut delay = INITIAL_POLL_DELAY;
    let mut poll_count = 0;

    loop {
        // Check if we've exceeded the maximum wait time
        if start.elapsed() > MAX_POLL_DURATION {
            return Err(PollError::Timeout {
                interaction_id: interaction_id.to_string(),
            });
        }

        // Wait before polling (skip on first iteration for instant detection)
        if poll_count > 0 {
            sleep(delay).await;
            // Exponential backoff up to maximum
            delay = (delay * 2).min(MAX_POLL_DELAY);
        }
        poll_count += 1;

        // Query the interaction status
        let response = client.get_interaction(interaction_id, false).await?;

        println!(
            "  Poll #{}: status={:?} (elapsed: {:.1}s)",
            poll_count,
            response.status,
            start.elapsed().as_secs_f64()
        );

        // Check the status
        match response.status {
            InteractionStatus::Completed => return Ok(response),
            InteractionStatus::Failed => {
                return Err(PollError::Failed {
                    interaction_id: interaction_id.to_string(),
                });
            }
            InteractionStatus::InProgress => {
                // Continue polling
            }
            InteractionStatus::RequiresAction => {
                eprintln!("    Note: Interaction requires action (unusual for deep research)");
            }
            InteractionStatus::Cancelled => {
                eprintln!("    Interaction was cancelled");
                return Err(PollError::Failed {
                    interaction_id: interaction_id.to_string(),
                });
            }
            other => {
                // Following Evergreen principles (see CLAUDE.md), we continue polling
                // on unknown statuses rather than failing. This ensures forward
                // compatibility when the API adds new status variants.
                // MAX_POLL_DURATION timeout protects against infinite loops.
                eprintln!("    Unhandled status {:?}, continuing to poll...", other);
            }
        }
    }
}

/// Displays the research results from a completed interaction
fn display_research_results(response: &genai_rs::InteractionResponse) {
    println!("Status: {:?}", response.status);
    println!("Interaction ID: {:?}\n", response.id);

    if let Some(text) = response.as_text() {
        let display_text = truncate_for_display(text, DISPLAY_LIMIT);
        println!("Research Results:\n{display_text}\n");
    } else {
        println!("No text response received.\n");
    }

    // Show token usage
    if let Some(usage) = &response.usage {
        println!("--- Token Usage ---");
        if let Some(input) = usage.total_input_tokens {
            println!("  Input tokens: {input}");
        }
        if let Some(output) = usage.total_output_tokens {
            println!("  Output tokens: {output}");
        }
    }
}

/// Handles errors from the Deep Research agent with helpful messages
fn handle_research_error(e: &GenaiError) {
    match e {
        GenaiError::Api {
            status_code,
            message,
            request_id,
            ..
        } => {
            eprintln!("API Error (HTTP {}): {}", status_code, message);
            if let Some(id) = request_id {
                eprintln!("  Request ID: {}", id);
            }

            // Provide helpful context for common errors
            if message.contains("not found") || message.contains("not available") {
                eprintln!("\nNote: The Deep Research agent may not be available in your account.");
                eprintln!("This is a preview feature that requires specific access permissions.");
            } else if message.contains("quota") || message.contains("rate") {
                eprintln!("\nNote: You may have exceeded API quota or rate limits.");
            }
        }
        GenaiError::Http(http_err) => {
            eprintln!("HTTP Error: {http_err}");
            eprintln!("Check your network connection and try again.");
        }
        GenaiError::Json(json_err) => {
            eprintln!("JSON Error: {json_err}");
        }
        _ => {
            eprintln!("Error: {e}");
        }
    }
}

// =============================================================================
// Summary (printed by display_research_results, but documented here for clarity)
// =============================================================================
//
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ✅ Deep Research Demo Complete
//
// --- Key Takeaways ---
// • with_agent("deep-research-pro-preview-12-2025") uses the research agent
// • with_agent_config(DeepResearchConfig::new()...) for agent-specific settings
// • with_background(true) is required for agent interactions
// • Poll for completion using client.get_interaction(id)
// • Research typically takes 30-120 seconds
//
// --- What You'll See with LOUD_WIRE=1 ---
// Start research:
//   [REQ#1] POST with input + agent + background:true + store:true
//   [RES#1] in_progress: interaction ID returned
//
// Polling (multiple rounds):
//   [REQ#2] GET interaction by ID
//   [RES#2] in_progress: still researching
//   ...
//   [REQ#N] GET interaction by ID
//   [RES#N] completed: research report
//
// --- Production Considerations ---
// • Implement exponential backoff when polling (see INITIAL_POLL_DELAY)
// • Set appropriate timeouts (MAX_POLL_DURATION) for your use case
// • Handle PollError::Timeout gracefully - research may still complete
// • Store interaction IDs to retrieve results later if timeout occurs
// • Deep Research may not be available in all accounts/regions
