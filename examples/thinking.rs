//! Example: Thinking/Reasoning Levels
//!
//! This example demonstrates Gemini's thinking capabilities, which expose
//! the model's chain-of-thought reasoning process.
//!
//! # Running
//!
//! ```bash
//! cargo run --example thinking
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.
//!
//! # Thinking Levels
//!
//! - `minimal`: Minimal reasoning, fastest responses
//! - `low`: Light reasoning for simple problems
//! - `medium`: Balanced reasoning for moderate complexity
//! - `high`: Extensive reasoning for complex problems
//!
//! Higher levels produce more detailed reasoning but consume more tokens.

use futures_util::StreamExt;
use genai_rs::{Client, StepDelta, StreamChunk, ThinkingLevel, ThinkingSummaries};
use std::env;
use std::io::{Write, stdout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== THINKING/REASONING LEVELS EXAMPLE ===\n");

    // ==========================================================================
    // Example 1: Basic Thinking with Medium Level
    // ==========================================================================
    println!("--- Example 1: Medium Thinking Level ---\n");

    let prompt = "Solve this step by step: If a train travels 120 miles in 2 hours, \
                  then stops for 30 minutes, then travels another 60 miles in 1 hour, \
                  what is the average speed for the entire journey?";

    println!("Prompt: {}\n", prompt);

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .with_thinking_level(ThinkingLevel::Medium)
        .with_store_enabled()
        .create()
        .await?;

    // Check if model produced thoughts
    // Note: Thought blocks contain cryptographic signatures for verification,
    // not human-readable reasoning text. Use has_thoughts() to detect thinking.
    if response.has_thoughts() {
        let signature_count = response.thought_signatures().count();
        println!(
            "=== Model Used Internal Reasoning ({} thought signatures) ===\n",
            signature_count
        );
    }

    // Print the final answer
    if let Some(text) = response.as_text() {
        println!("Final Answer:\n{}\n", text);
    }

    // Show step summary
    let summary = response.step_summary();
    println!(
        "Steps: {} thought steps, {} text blocks\n",
        summary.thought_count, summary.text_count
    );

    // Show token usage for thinking
    if let Some(thought_tokens) = response.thought_tokens() {
        println!("Thought tokens: {}", thought_tokens);
    }

    // ==========================================================================
    // Example 2: Thinking Summaries
    // ==========================================================================
    println!("\n--- Example 2: Thinking Summaries ---\n");

    let summary_prompt = "Explain the process of photosynthesis step by step.";
    println!("Prompt: {}\n", summary_prompt);

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(summary_prompt)
        .with_thinking_level(ThinkingLevel::Medium)
        .with_thinking_summaries(ThinkingSummaries::Auto)
        .with_store_enabled()
        .create()
        .await?;

    // ThinkingSummaries::Auto provides summarized reasoning in thought steps
    if response.has_thoughts() {
        let thought_count = response.thought_signatures().count();
        println!(
            "Received {} thought step(s) with summaries enabled",
            thought_count
        );
        for summary_content in response.thought_summaries() {
            if let Some(text) = summary_content.as_text() {
                println!("  Thought summary: {}", text);
            }
        }
    }

    if let Some(text) = response.as_text() {
        let preview = if text.len() > 200 {
            format!("{}...", &text[..200])
        } else {
            text.to_string()
        };
        println!("Answer: {}\n", preview);
    }

    // Show thought tokens
    if let Some(thought_tokens) = response.thought_tokens() {
        println!("Thought tokens: {}", thought_tokens);
    }

    // ==========================================================================
    // Example 3: Comparing Different Thinking Levels
    // ==========================================================================
    println!("\n--- Example 3: Comparing Thinking Levels ---\n");

    let complex_prompt = "What is the probability of getting exactly 3 heads \
                          when flipping a fair coin 5 times?";

    println!("Prompt: {}\n", complex_prompt);

    for level in [ThinkingLevel::Low, ThinkingLevel::High] {
        println!(">>> Thinking Level: {:?} <<<\n", level);

        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text(complex_prompt)
            .with_thinking_level(level)
            .with_store_enabled()
            .create()
            .await?;

        let summary = response.step_summary();

        if response.has_thoughts() {
            // Thoughts contain cryptographic signatures, not readable text
            let sig_count = response.thought_signatures().count();
            println!("Received {} thought signature(s)\n", sig_count);
        }

        if let Some(text) = response.as_text() {
            let preview = if text.len() > 150 {
                format!("{}...", &text[..150])
            } else {
                text.to_string()
            };
            println!("Answer preview: {}\n", preview);
        }

        println!(
            "Stats: {} thoughts, {} text blocks\n",
            summary.thought_count, summary.text_count
        );

        if let Some(thought_tokens) = response.thought_tokens() {
            println!("Thought tokens used: {}", thought_tokens);
        }
        if let Some(total) = response.usage.as_ref().and_then(|u| u.total_output_tokens) {
            println!("Total output tokens: {}", total);
        }
        println!();
    }

    // ==========================================================================
    // Example 4: Streaming with Thinking
    // ==========================================================================
    println!("--- Example 4: Streaming Thoughts ---\n");

    let stream_prompt = "Explain why the sky is blue, showing your reasoning.";
    println!("Prompt: {}\n", stream_prompt);

    let mut stream = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(stream_prompt)
        .with_thinking_level(ThinkingLevel::Medium)
        .create_stream();

    let mut in_thought = false;

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => match event.chunk {
                StreamChunk::StepDelta { delta, .. } => match delta {
                    // Signatures are cryptographic tokens, not readable text
                    StepDelta::ThoughtSignature { .. } => {
                        if !in_thought {
                            print!("\n[THINKING] (signature present) ");
                            in_thought = true;
                        }
                        stdout().flush()?;
                    }
                    // Thought summaries stream as readable content
                    StepDelta::ThoughtSummary { content } => {
                        if !in_thought {
                            print!("\n[THINKING] ");
                            in_thought = true;
                        }
                        if let Some(t) = content.as_ref().and_then(|c| c.as_text()) {
                            print!("{}", t);
                        }
                        stdout().flush()?;
                    }
                    _ => {
                        if let Some(t) = delta.as_text() {
                            if in_thought {
                                println!("\n[END THINKING]\n");
                                in_thought = false;
                                print!("[ANSWER] ");
                            }
                            print!("{}", t);
                            stdout().flush()?;
                        }
                    }
                },
                StreamChunk::Completed(response) => {
                    println!("\n");
                    let summary = response.step_summary();
                    println!(
                        "Complete: {} thoughts, {} text blocks",
                        summary.thought_count, summary.text_count
                    );
                }
                _ => {} // Handle unknown variants
            },
            Err(e) => {
                eprintln!("\nStream error: {}", e);
                break;
            }
        }
    }

    // ==========================================================================
    // Usage Notes
    // ==========================================================================
    println!("\n--- Usage Notes ---\n");
    println!("Thinking Level Guide:");
    println!("  minimal - Quick responses, minimal reasoning overhead");
    println!("  low     - Light reasoning for straightforward problems");
    println!("  medium  - Balanced approach, good for most use cases");
    println!("  high    - Extensive reasoning for complex problems");
    println!("\nBest Practices:");
    println!("  1. Use 'medium' for general problem-solving");
    println!("  2. Use 'high' for math, logic, and complex reasoning");
    println!("  3. Check response.has_thoughts() before iterating");
    println!("  4. Monitor response.thought_tokens() for cost tracking");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Thinking/Reasoning Levels Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_thinking_level() exposes model's chain-of-thought reasoning");
    println!("• Levels: minimal, low, medium (default), high (extensive reasoning)");
    println!("• with_thinking_summaries(Auto) provides summarized reasoning in output");
    println!("• response.thought_signatures() iterates over thought cryptographic signatures");
    println!("• Higher levels use more tokens but improve complex problem solving\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Non-streaming:");
    println!("  [REQ#1] POST with input + thinkingConfig(medium)");
    println!("  [RES#1] completed: thought steps + model_output (usage includes thought tokens)\n");
    println!("Streaming:");
    println!("  [REQ#2] POST streaming with input + thinkingConfig");
    println!("  [RES#2] SSE stream: thought summary/signature deltas → text deltas → completed\n");

    println!("--- Production Considerations ---");
    println!("• Monitor response.thought_tokens() for cost tracking");
    println!("• Use 'high' for math, logic, and complex reasoning tasks");
    println!("• Thought content may be internal (not exposed) in some cases");
    println!("• ThoughtSignature provides authenticity verification");

    Ok(())
}
