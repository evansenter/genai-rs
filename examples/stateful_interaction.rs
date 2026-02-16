use genai_rs::Client;
use std::env;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Get API Key from environment variable
    let api_key = env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // Create the client
    let client = Client::builder(api_key).build()?;

    let model_name = "gemini-3-flash-preview";

    // === First interaction ===
    println!("=== FIRST INTERACTION ===\n");

    let first_prompt = "My name is Alice and I like programming in Rust.";
    println!("User: {first_prompt}\n");

    let first_response = client
        .interaction()
        .with_model(model_name)
        .with_text(first_prompt)
        .with_store_enabled() // Important: Store for stateful conversation
        .create()
        .await?;
    let interaction_id = first_response
        .id
        .clone()
        .expect("id should exist when store=true");

    println!("Interaction ID: {interaction_id}");
    println!("Status: {:?}\n", first_response.status);

    if !first_response.outputs.is_empty() {
        println!("Assistant:");
        for output in &first_response.outputs {
            if let Some(t) = output.as_text() {
                println!("{t}");
            }
        }
    }

    // === Second interaction - referencing the first ===
    println!("\n=== SECOND INTERACTION (Stateful) ===\n");

    let second_prompt = "What is my name and what language do I like?";
    println!("User: {second_prompt}\n");

    let second_response = client
        .interaction()
        .with_model(model_name)
        .with_text(second_prompt)
        .with_previous_interaction(&interaction_id) // Reference first interaction
        .with_store_enabled()
        .create()
        .await?;

    println!("Interaction ID: {:?}", second_response.id);
    println!("Previous Interaction ID: {}", interaction_id);
    println!("Status: {:?}\n", second_response.status);

    if !second_response.outputs.is_empty() {
        println!("Assistant:");
        for output in &second_response.outputs {
            if let Some(t) = output.as_text() {
                println!("{t}");
            }
        }
    }

    // === Third interaction - continuation ===
    println!("\n=== THIRD INTERACTION (Continued) ===\n");

    let third_prompt = "Why is that language interesting?";
    println!("User: {third_prompt}\n");

    let third_response = client
        .interaction()
        .with_model(model_name)
        .with_text(third_prompt)
        .with_previous_interaction(second_response.id.as_ref().expect("id should exist")) // Reference second interaction
        .with_store_enabled()
        .create()
        .await?;

    println!("Interaction ID: {:?}", third_response.id);
    println!("Previous Interaction ID: {:?}", second_response.id);
    println!("Status: {:?}\n", third_response.status);

    if !third_response.outputs.is_empty() {
        println!("Assistant:");
        for output in &third_response.outputs {
            if let Some(t) = output.as_text() {
                println!("{t}");
            }
        }
    }

    // === Demonstrate retrieval ===
    println!("\n=== RETRIEVING FIRST INTERACTION ===\n");

    match client.get_interaction(&interaction_id, false).await {
        Ok(retrieved) => {
            println!("Retrieved Interaction ID: {:?}", retrieved.id);
            println!("Status: {:?}", retrieved.status);
            println!("Input parts: {}", retrieved.input.len());
            println!("Output parts: {}", retrieved.outputs.len());
        }
        Err(e) => {
            eprintln!("Error retrieving interaction: {e}");
        }
    }

    // === Cleanup (optional) ===
    println!("\n=== CLEANUP ===\n");

    // Uncomment to actually delete:
    // client.delete_interaction(&interaction_id).await?;
    // client.delete_interaction(&second_response.id).await?;
    // client.delete_interaction(&third_response.id).await?;
    println!("Interactions can be deleted using client.delete_interaction(id)");
    println!("(Skipped in this example - they will auto-expire based on your API tier)");

    // =========================================================================
    // Summary
    // =========================================================================
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Stateful Interaction Demo Complete\n");

    println!("--- Key Takeaways ---");
    println!("• with_store_enabled() saves interactions server-side for multi-turn conversations");
    println!("• with_previous_interaction(id) chains turns together for context");
    println!("• Each turn returns an ID to reference in the next turn");
    println!("• client.get_interaction(id) retrieves stored interactions\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  [REQ#1] POST with input + store:true");
    println!("  [RES#1] completed: text response + interaction ID");
    println!("  [REQ#2] POST with input + previousInteractionId + store:true");
    println!("  [RES#2] completed: text response (remembers context)");
    println!("  [REQ#3] POST with input + previousInteractionId + store:true");
    println!("  [RES#3] completed: text response (full conversation context)");
    println!("  [REQ#4] GET interaction by ID");
    println!("  [RES#4] retrieved: stored interaction with inputs/outputs\n");

    println!("--- Production Considerations ---");
    println!("• Store only when multi-turn is needed (costs storage)");
    println!("• Implement conversation cleanup to avoid orphaned interactions");
    println!("• Handle ID persistence across sessions for resumable conversations");
    println!("• Consider conversation expiry policies for your API tier");

    Ok(())
}
