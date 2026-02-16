//! Example: Cancelling Background Interactions
//!
//! This example demonstrates how to cancel an in-progress background interaction.
//! This is useful when:
//! - User requirements change mid-execution
//! - You need cost control by stopping token-consuming interactions
//! - Implementing timeout handling in application logic
//! - Supporting user-initiated cancellation in UIs
//!
//! The cancel API only works on background interactions that are still in `InProgress` status.
//!
//! Run with: cargo run --example cancel_interaction

use genai_rs::{Client, GenaiError, InteractionStatus};
use std::env;
use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    println!("=== Cancel Interaction Example ===\n");

    // 2. Start a long-running background interaction
    // We use the deep research agent since it takes a while to complete,
    // giving us time to cancel it.
    let agent_name = "deep-research-pro-preview-12-2025";
    let prompt = "Analyze the history and future of renewable energy technologies";

    println!("Starting background research task...");
    println!("Prompt: {prompt}\n");

    let response = client
        .interaction()
        .with_agent(agent_name)
        .with_text(prompt)
        .with_background(true) // Required for agent interactions
        .with_store_enabled() // Required to retrieve/cancel by ID
        .create()
        .await?;

    println!("Initial status: {:?}", response.status);
    println!("Interaction ID: {:?}\n", response.id);

    // 3. Get the interaction ID
    let interaction_id = response
        .id
        .as_ref()
        .expect("stored interaction should have id");

    // 4. Check if still in progress, then cancel
    if response.status == InteractionStatus::InProgress {
        // Give it a moment to start processing
        println!("Waiting a moment before cancelling...");
        sleep(Duration::from_secs(2)).await;

        // Cancel the interaction
        println!("Cancelling the interaction...");
        match client.cancel_interaction(interaction_id).await {
            Ok(cancelled) => {
                println!("\nCancellation result:");
                println!("  Status: {:?}", cancelled.status);
                println!("  Interaction ID: {:?}", cancelled.id);

                // Verify it was cancelled
                if cancelled.status == InteractionStatus::Cancelled {
                    println!("\n✓ Interaction was successfully cancelled!");
                } else {
                    println!("\n⚠ Unexpected status after cancel: {:?}", cancelled.status);
                }

                // Demonstrate that you can still get info about the cancelled interaction
                println!("\nRetrieving interaction details after cancellation...");
                let retrieved = client.get_interaction(interaction_id, false).await?;
                println!("Retrieved status: {:?}", retrieved.status);
                println!("Output count: {} items", retrieved.outputs.len());
            }
            Err(GenaiError::Api {
                status_code: 404, ..
            }) => {
                // The cancel endpoint may not yet be deployed to the production API
                println!("\n⚠ Cancel endpoint not yet available (HTTP 404)");
                println!("The cancel API is documented but not yet deployed to production.");
                println!("The implementation is ready and will work once the API is available.");
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    } else {
        // The interaction completed or failed before we could cancel
        println!(
            "Interaction already finished with status {:?}",
            response.status
        );
        println!("(This can happen if the task completed very quickly)");
    }

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with agent + background:true + store:true");
    println!("  [RES#1] in_progress: interaction_id returned");
    println!("  [REQ#2] POST to cancel endpoint with interaction_id");
    println!("  [RES#2] cancelled: status update");
    println!("  [REQ#3] GET interaction by id");
    println!("  [RES#3] cancelled: full interaction details\n");

    println!("--- Production Considerations ---");
    println!("• Only background interactions can be cancelled");
    println!("• Only InProgress interactions can be cancelled");
    println!("• Cancelling an already completed/failed interaction returns an error");
    println!("• Consider implementing retry logic for the cancel call itself");
    println!("• The cancel endpoint may not yet be deployed to production API");

    Ok(())
}
