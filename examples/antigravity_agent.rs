//! Antigravity agent example: spawn the local harness, register a custom
//! Rust tool, set policies, and run an agentic conversation.
//!
//! Requirements:
//! - The `localharness` binary (ships in the `google-antigravity` Python
//!   wheel): `pip install google-antigravity==0.1.5`, or set
//!   `ANTIGRAVITY_HARNESS_PATH` to the binary.
//! - `GEMINI_API_KEY` for model calls.
//!
//! Run with:
//! ```bash
//! cargo run --example antigravity_agent --features antigravity
//! LOUD_WIRE=1 cargo run --example antigravity_agent --features antigravity
//! ```

use futures_util::StreamExt;
use genai_rs::CallableFunction;
use genai_rs::antigravity::{AgentEvent, AntigravityAgent, BuiltinTool, Capabilities, policy};
use genai_rs_macros::tool;

/// Returns the current weather for a city.
#[tool(city(description = "The city to get weather for"))]
fn get_weather(city: String) -> String {
    // A real tool would call a weather API here.
    format!("Sunny and 22 degrees C in {city}")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set");

    println!("=== Antigravity Agent ===\n");

    // Spawn the harness: read-only built-ins plus our custom Rust tool.
    // Policies are evaluated Rust-side before every tool dispatch.
    let mut agent = AntigravityAgent::builder()
        .with_api_key(api_key)
        .with_model("gemini-3-flash-preview")
        .with_system_instructions("You are a concise assistant. Prefer tools over guessing.")
        .with_capabilities(Capabilities::read_only().disable(BuiltinTool::AskQuestion))
        .add_tool(GetWeatherCallable.declaration())
        .add_policy(policy::deny_all())
        .add_policy(policy::allow("get_weather"))
        .spawn()
        .await?;

    println!(
        "Harness up. conversation_id={:?}\n",
        agent.conversation_id()
    );

    // Simple one-shot chat: the agent may call get_weather mid-turn; the
    // crate dispatches it through the #[tool] registry automatically.
    let response = agent.chat("What's the weather in Tokyo right now?").await?;
    println!("Agent: {}\n", response.text());
    if let Some(usage) = response.usage() {
        println!(
            "Usage: prompt={:?} total={:?}",
            usage.prompt_token_count, usage.total_token_count
        );
    }

    // Streaming: watch deltas and tool activity as the turn runs. The
    // stream mutably borrows the agent, so scope it before shutdown.
    println!("\n--- Streaming turn ---");
    {
        let mut stream = agent
            .send_streaming("And what about Paris? One sentence.")
            .await?;
        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::TextDelta(delta) => print!("{delta}"),
                AgentEvent::ThinkingDelta(_) => print!("."),
                AgentEvent::ToolCallDispatched { name, .. } => {
                    println!("\n[custom tool dispatched: {name}]");
                }
                AgentEvent::ToolAction(action) => println!("\n[harness action: {action:?}]"),
                AgentEvent::Finished(_) => break,
                AgentEvent::Error(message) => eprintln!("\n[error: {message}]"),
                _ => {}
            }
        }
        println!();
    }

    let conversation_id = agent.conversation_id().map(ToString::to_string);
    agent.shutdown().await?;
    println!("\nHarness shut down cleanly. (conversation_id={conversation_id:?})");

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  HARNESS /path/to/localharness (pid N) - process spawn");
    println!("  WS Send: {{\"config\": ...}} - conversation init with models/tools/policies");
    println!("  WS Receive: {{\"initializeConversationResponse\": ...}} - cascade id");
    println!("  WS Send: {{\"userInput\": ...}} - each chat turn");
    println!("  WS Receive: {{\"stepUpdate\": ...}} - streaming step/thinking/text updates");
    println!(
        "  WS Receive: {{\"toolCall\": ...}} / WS Send: {{\"toolResponse\": ...}} - custom tools"
    );
    println!("  STDERR: ... - harness diagnostics\n");

    println!("--- Production Considerations ---");
    println!(
        "• Pin the harness: pip install google-antigravity==0.1.5 (see SUPPORTED_HARNESS_VERSION)"
    );
    println!("• Always add policies before enabling write tools (run_command, edit_file)");
    println!(
        "• Call agent.shutdown() for graceful exit; dropping kills the harness without persistence"
    );
    println!("• Use with_save_dir + conversation_id() to resume sessions across runs");
    println!("• Set with_turn_timeout to bound runaway agent turns");

    Ok(())
}
