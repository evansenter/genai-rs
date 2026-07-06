//! Example: Multi-Turn Conversations with Thinking
//!
//! This example demonstrates multi-turn conversations when thinking mode is enabled.
//!
//! # Replaying Thoughts
//!
//! Thoughts arrive as `Step::Thought { signature, summary }` steps in the response.
//! To preserve thought context across turns in stateless (manual history) mode,
//! include `response.output_steps()` in the history you send back - this replays
//! the thought steps (with their signatures) alongside the model output.
//!
//! Alternatively, use `with_previous_interaction(id)` and the server preserves
//! thought context automatically.
//!
//! # Running
//!
//! ```bash
//! cargo run --example thought_echo
//! ```
//!
//! # Prerequisites
//!
//! Set the `GEMINI_API_KEY` environment variable with your API key.

use genai_rs::{Client, Step, ThinkingLevel};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get API key from environment
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY environment variable not set");

    let client = Client::builder(api_key).build()?;

    println!("=== MULTI-TURN WITH THINKING EXAMPLE ===\n");

    // ==========================================================================
    // Method 1: Using previous_interaction_id (server-side context)
    // ==========================================================================
    println!("--- Method 1: Using previous_interaction_id ---\n");
    println!("The server preserves thought context automatically.\n");

    let initial_prompt = "What is 17 * 23? Think through this step by step.";
    println!("User: {}\n", initial_prompt);

    // First interaction - must enable store for multi-turn
    let response1 = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(initial_prompt)
        .with_thinking_level(ThinkingLevel::Medium)
        .with_store_enabled()
        .create()
        .await?;

    // Display thought count (signatures are cryptographic, not human-readable)
    if response1.has_thoughts() {
        let sig_count = response1.thought_signatures().count();
        println!(
            "Model used internal reasoning ({} thought signature(s))",
            sig_count
        );
    }

    if let Some(text) = response1.as_text() {
        println!("\nModel's answer: {}\n", text);
    }

    let interaction_id = response1
        .id
        .as_ref()
        .expect("id should exist when store=true");
    println!("Interaction ID: {}\n", interaction_id);

    // Follow-up using previous_interaction_id - server preserves thought context
    let followup = "Now what is that result divided by 17?";
    println!("User: {}\n", followup);

    let response2 = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(followup)
        .with_previous_interaction(interaction_id)
        .with_thinking_level(ThinkingLevel::Medium)
        .with_store_enabled()
        .create()
        .await?;

    if response2.has_thoughts() {
        let sig_count = response2.thought_signatures().count();
        println!(
            "Model used internal reasoning ({} thought signature(s))",
            sig_count
        );
    }

    if let Some(text) = response2.as_text() {
        println!("\nModel's answer: {}\n", text);
    }

    // ==========================================================================
    // Method 2: Manual step-based history (echo thoughts via output_steps)
    // ==========================================================================
    println!("--- Method 2: Manual Step History (Stateless) ---\n");
    println!("response.output_steps() includes Thought steps (with signatures),");
    println!("so replaying them preserves the model's reasoning context.\n");

    let prompt = "What is 13 * 19?";
    println!("User: {}\n", prompt);

    let resp_manual = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .with_thinking_level(ThinkingLevel::Low)
        .create()
        .await?;

    println!(
        "Model's answer: {}\n",
        resp_manual.as_text().unwrap_or("(no answer)")
    );
    println!(
        "Output steps to replay: {} (including {} thought signature(s))\n",
        resp_manual.output_steps().len(),
        resp_manual.thought_signatures().count()
    );

    // Build manual history: user step + ALL output steps (thoughts included)
    let mut history: Vec<Step> = vec![Step::user_text(prompt)];
    history.extend(resp_manual.output_steps());
    history.push(Step::user_text("Now divide that by 13."));

    println!("User: Now divide that by 13.\n");

    let resp_followup = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .with_thinking_level(ThinkingLevel::Low)
        .create()
        .await?;

    if let Some(text) = resp_followup.as_text() {
        println!("Model's answer: {}\n", text);
    }

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Multi-Turn with Thinking Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_previous_interaction() lets the server preserve thought context");
    println!("• For stateless flows, extend history with response.output_steps()");
    println!("• output_steps() includes Step::Thought entries with their signatures");
    println!("• Thought signatures are cryptographic, not human-readable text\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Method 1 (previous_interaction_id):");
    println!("  [REQ#1] POST with input + thinkingConfig + store:true");
    println!("  [RES#1] completed: thought step (signature) + model_output + interaction ID");
    println!("  [REQ#2] POST with input + previousInteractionId");
    println!("  [RES#2] completed: server-side thought context preserved\n");
    println!("Method 2 (manual step history):");
    println!("  [REQ#3] POST with input + thinkingConfig");
    println!("  [RES#3] completed: thought step + model_output");
    println!("  [REQ#4] POST with steps [user_input, thought, model_output, user_input]\n");

    println!("--- Production Considerations ---");
    println!("• Prefer with_previous_interaction() when storage is available");
    println!("• In stateless mode, always replay output_steps() to keep signatures intact");
    println!("• Enable with_store_enabled() to get interaction IDs for chaining");
    println!("• Thought signatures are for verification, not user display");

    Ok(())
}
