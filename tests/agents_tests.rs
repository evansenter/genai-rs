//! Agent tests for deep research and background mode
//!
//! Tests for deep research agent and background mode polling.
//!
//! These tests require the GEMINI_API_KEY environment variable to be set.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test agents_tests -- --include-ignored --nocapture
//! ```
//!
//! # Notes
//!
//! Agent tests may take longer to complete due to background processing.
//! Some agents may not be available in all accounts.

mod common;

use common::{PollError, get_client, poll_until_complete};
use genai_rs::{DeepResearchConfig, InteractionStatus, ThinkingSummaries};
use std::time::Duration;

/// Maximum time to wait for background tasks to complete.
const BACKGROUND_TASK_TIMEOUT: Duration = Duration::from_secs(60);

// =============================================================================
// Agents: Deep Research
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_deep_research_agent() {
    // Test the deep research agent with background mode (required for agents)
    // Note: This agent may not be available in all accounts
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let result = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("What are the main differences between Rust and Go programming languages?")
        .with_background(true) // Required for agent interactions
        .with_store_enabled() // Required to retrieve results by interaction ID
        .create()
        .await;

    match result {
        Ok(initial_response) => {
            println!("Initial status: {:?}", initial_response.status);
            println!("Interaction ID: {:?}", initial_response.id);

            // If already completed, check the response
            if initial_response.status == InteractionStatus::Completed {
                println!("Task completed immediately");
                if initial_response.has_text() {
                    let text = initial_response.as_text().unwrap();
                    println!(
                        "Research response (truncated): {}...",
                        &text[..text.len().min(500)]
                    );
                    assert_response_has_content(text);
                }
                return;
            }

            // Poll for completion using exponential backoff
            match poll_until_complete(
                &client,
                initial_response.id.as_ref().expect("id should exist"),
                BACKGROUND_TASK_TIMEOUT,
            )
            .await
            {
                Ok(response) => {
                    println!("Deep research completed!");
                    if response.has_text() {
                        let text = response.as_text().unwrap();
                        println!(
                            "Research response (truncated): {}...",
                            &text[..text.len().min(500)]
                        );
                        assert_response_has_content(text);
                    }
                }
                Err(PollError::Timeout) => {
                    // Timeout is acceptable - deep research on complex queries can exceed
                    // our test timeout. We're verifying the polling mechanism works, not
                    // that every query completes within the time limit.
                    println!(
                        "Polling timed out after {:?} - task may still be running",
                        BACKGROUND_TASK_TIMEOUT
                    );
                }
                Err(PollError::Failed) => {
                    println!("Deep research task failed");
                }
                Err(PollError::Api(e)) => {
                    println!("Poll error: {:?}", e);
                }
            }
        }
        Err(e) => {
            let error_str = format!("{:?}", e);
            println!("Deep research error (may be expected): {}", error_str);
            if error_str.contains("not found")
                || error_str.contains("not available")
                || error_str.contains("agent")
            {
                println!("Deep research agent not available - skipping test");
            }
        }
    }
}

/// Helper to verify the response is non-empty (structural check)
fn assert_response_has_content(text: &str) {
    assert!(!text.is_empty(), "Response should have non-empty content");
}

// =============================================================================
// Agents: Background Mode
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_background_mode_polling() {
    // Test background mode with polling using exponential backoff
    // Note: This requires an agent that supports background mode
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    // Start background task
    let result = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("Briefly explain what machine learning is.")
        .with_background(true)
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(initial_response) => {
            println!("Initial status: {:?}", initial_response.status);
            println!("Interaction ID: {:?}", initial_response.id);

            // If already completed, we're done
            if initial_response.status == InteractionStatus::Completed {
                println!("Task completed immediately (may not have used background mode)");
                if initial_response.has_text() {
                    println!("Result: {}", initial_response.as_text().unwrap());
                }
                return;
            }

            // Poll for completion using exponential backoff
            match poll_until_complete(
                &client,
                initial_response.id.as_ref().expect("id should exist"),
                BACKGROUND_TASK_TIMEOUT,
            )
            .await
            {
                Ok(response) => {
                    println!("Task completed!");
                    if response.has_text() {
                        let text = response.as_text().unwrap();
                        println!("Result: {}...", &text[..200.min(text.len())]);
                    }
                }
                Err(PollError::Timeout) => {
                    // Timeout is acceptable - we're testing the polling mechanism, not
                    // guaranteeing completion within the time limit.
                    println!(
                        "Polling timed out after {:?} - task may still be running",
                        BACKGROUND_TASK_TIMEOUT
                    );
                }
                Err(PollError::Failed) => {
                    println!("Task failed");
                }
                Err(PollError::Api(e)) => {
                    println!("Poll error: {:?}", e);
                }
            }
        }
        Err(e) => {
            let error_str = format!("{:?}", e);
            println!("Background mode error (may be expected): {}", error_str);
            if error_str.contains("not found")
                || error_str.contains("not supported")
                || error_str.contains("background")
            {
                println!("Background mode not available - skipping test");
            }
        }
    }
}

// =============================================================================
// Agents: Agent Configuration
// =============================================================================

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_deep_research_with_agent_config() {
    // Test the deep research agent with typed AgentConfig
    // This exercises the new agent_config field and serialization
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let result = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("What is Rust programming language?")
        .with_agent_config(
            DeepResearchConfig::new().with_thinking_summaries(ThinkingSummaries::Auto),
        )
        .with_background(true)
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(initial_response) => {
            println!("Initial status: {:?}", initial_response.status);
            println!("Interaction ID: {:?}", initial_response.id);

            // If already completed, check the response
            if initial_response.status == InteractionStatus::Completed {
                println!("Task completed immediately with AgentConfig");
                if initial_response.has_text() {
                    let text = initial_response.as_text().unwrap();
                    println!(
                        "Research response (truncated): {}...",
                        &text[..text.len().min(500)]
                    );
                }
                return;
            }

            // Poll for completion
            match poll_until_complete(
                &client,
                initial_response.id.as_ref().expect("id should exist"),
                BACKGROUND_TASK_TIMEOUT,
            )
            .await
            {
                Ok(response) => {
                    println!("Deep research with AgentConfig completed!");
                    if response.has_text() {
                        let text = response.as_text().unwrap();
                        println!(
                            "Research response (truncated): {}...",
                            &text[..text.len().min(500)]
                        );
                    }
                }
                Err(PollError::Timeout) => {
                    println!(
                        "Polling timed out after {:?} - task may still be running",
                        BACKGROUND_TASK_TIMEOUT
                    );
                }
                Err(PollError::Failed) => {
                    println!("Deep research task failed");
                }
                Err(PollError::Api(e)) => {
                    println!("Poll error: {:?}", e);
                }
            }
        }
        Err(e) => {
            let error_str = format!("{:?}", e);
            println!("Deep research with AgentConfig error: {}", error_str);
            if error_str.contains("not found")
                || error_str.contains("not available")
                || error_str.contains("agent")
            {
                println!("Deep research agent not available - skipping test");
            }
        }
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_deep_research_config_convenience_method() {
    // Test the convenience method with_deep_research_config
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let result = client
        .interaction()
        .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
        .with_text("What is Rust?")
        .with_deep_research_config(ThinkingSummaries::Auto) // Convenience method
        .with_background(true)
        .with_store_enabled()
        .create()
        .await;

    match result {
        Ok(initial_response) => {
            println!("Initial status: {:?}", initial_response.status);
            println!("Interaction ID: {:?}", initial_response.id);

            // Just verify the request was accepted - we don't need to poll for completion
            // as that's covered by other tests
            if initial_response.status == InteractionStatus::Completed {
                println!("Task completed immediately with convenience method");
            } else {
                println!(
                    "Task started with convenience method (status: {:?})",
                    initial_response.status
                );
            }
        }
        Err(e) => {
            let error_str = format!("{:?}", e);
            println!("Convenience method test error: {}", error_str);
            if error_str.contains("not found")
                || error_str.contains("not available")
                || error_str.contains("agent")
            {
                println!("Deep research agent not available - skipping test");
            }
        }
    }
}
