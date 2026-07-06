//! Explicit multi-turn conversation using Step arrays.
//!
//! This example demonstrates two ways to have multi-turn conversations without
//! relying on server-side storage (`previous_interaction_id`):
//!
//! 1. **ConversationBuilder** - Fluent API for inline conversation construction
//! 2. **with_history()** - Direct array of Step objects for external history
//!
//! Use these approaches when you need:
//! - Stateless deployments where interaction storage isn't used
//! - Custom history management (sliding window, summarization)
//! - Migration from other providers with existing conversation history
//! - Testing with controlled conversation states

use genai_rs::{Client, Step};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set");
    let client = Client::new(api_key);

    // Approach 1: Using ConversationBuilder fluent API
    // This is best for building conversations inline with readable syntax
    println!("=== ConversationBuilder Example ===\n");

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .conversation()
        .user("What is 2+2?")
        .model("2+2 equals 4.")
        .user("And what's that times 3?")
        .done()
        .create()
        .await?;

    println!(
        "Model response: {}\n",
        response.as_text().unwrap_or("No response")
    );

    // Approach 2: Using with_history() with pre-built history
    // This is best when you have conversation history from an external source
    println!("=== with_history() Example ===\n");

    let history = vec![
        Step::user_text("I'm planning a trip to Paris."),
        Step::model_text(
            "Paris is a wonderful destination! The city offers incredible art, cuisine, and architecture. What aspects of Paris are you most interested in exploring?",
        ),
        Step::user_text("I love museums and good food."),
        Step::model_text(
            "Perfect! For museums, I'd recommend the Louvre, Musée d'Orsay, and Centre Pompidou. For food, try Le Marais for falafel, Saint-Germain for classic bistros, and don't miss the bakeries everywhere for croissants and pain au chocolat.",
        ),
        Step::user_text("What's one thing I absolutely shouldn't miss?"),
    ];

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history)
        .create()
        .await?;

    println!(
        "Model response: {}\n",
        response.as_text().unwrap_or("No response")
    );

    // Approach 3: Building history dynamically
    // Useful for chatbot applications that manage their own history.
    // Note: response.output_steps() returns ALL output steps (thoughts,
    // function calls, model output) so signatures and tool state are replayed.
    println!("=== Dynamic History Example ===\n");

    let mut history: Vec<Step> = Vec::new();

    // Simulating a conversation loop
    let user_messages = [
        "Let's play a word game. I'll say a word and you respond with a word that starts with my word's last letter.",
        "Apple",
        "Elephant",
    ];

    for user_msg in user_messages {
        println!("User: {}", user_msg);

        // Add user message to history
        history.push(Step::user_text(user_msg));

        // Send full conversation history
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_history(history.clone())
            .create()
            .await?;

        println!("Model: {}\n", response.as_text().unwrap_or("No response"));

        // Add the model's output steps to history for the next turn.
        // This preserves thoughts and signatures, not just the text.
        history.extend(response.output_steps());
    }

    println!("=== Done ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("Example 1: ConversationBuilder");
    println!("  [REQ#1] POST with input as steps array [{{user_input, model_output, user_input}}]");
    println!("  [RES#1] completed: text response\n");
    println!("Example 2: with_history()");
    println!(
        "  [REQ#2] POST with input as steps array [{{user_input, model_output, user_input, model_output, user_input}}]"
    );
    println!("  [RES#2] completed: text response\n");
    println!("Example 3: Dynamic history loop");
    println!("  [REQ#3] POST with input as steps array [{{user_input}}]");
    println!("  [RES#3] completed: text response");
    println!(
        "  [REQ#4] POST with input as steps array [{{user_input, ...output steps, user_input}}]"
    );
    println!("  [RES#4] completed: text response");
    println!("  [REQ#5] POST with input as steps array [{{...previous steps, user_input}}]");
    println!("  [RES#5] completed: text response\n");

    println!("--- Production Considerations ---");
    println!("• Use with_history() for stateless deployments or custom history management");
    println!("• ConversationBuilder is syntactic sugar - both produce the same wire format");
    println!("• Extend history with response.output_steps() to preserve thoughts/signatures");
    println!("• Clone history before passing to with_history() if you need to reuse it");
    println!("• No validation of step alternation - API handles invalid sequences");
    println!("• For very long conversations, consider sliding window or summarization");
    println!("• Step arrays work with all features: streaming, function calling, thinking");

    Ok(())
}
