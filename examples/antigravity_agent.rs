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
use genai_rs::antigravity::{
    AgentEvent, AntigravityAgent, BuiltinTool, Capabilities, QuestionAnswer, QuestionReply, policy,
};
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
        .with_system_instructions(
            "You are a concise assistant. Prefer tools over guessing. When a request is \
             ambiguous (e.g. no city given for weather), use ask_question to clarify \
             instead of assuming.",
        )
        // read_only() does not include AskQuestion — enable it explicitly
        // so the on_questions hook below is reachable.
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::AskQuestion))
        // Answer agent questions (ask_question builtin) from policy: pick
        // the first choice when there is one, otherwise leave unanswered.
        // The hook runs inline in the event pump — never block in it
        // waiting for a human; answer from policy or pre-collected state.
        .on_questions(|questions| {
            QuestionReply::Answers(
                questions
                    .iter()
                    .map(|q| {
                        println!("[agent asked: {}]", q.question);
                        if q.choices.is_empty() {
                            QuestionAnswer::Unanswered
                        } else {
                            QuestionAnswer::Choices {
                                selected: vec![0],
                                freeform: None,
                            }
                        }
                    })
                    .collect(),
            )
        })
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
                AgentEvent::ToolAction {
                    action, decision, ..
                } => println!("\n[harness action ({decision:?}): {action:?}]"),
                AgentEvent::Finished(_) => break,
                AgentEvent::Error { message, severity } => {
                    eprintln!("\n[error ({severity:?}): {message}]");
                }
                _ => {}
            }
        }
        println!();
    }

    // A deliberately under-specified turn: no city, so per the system
    // instruction the agent should clarify via ask_question — answered by
    // the on_questions hook above (which picks the first choice). This is
    // the turn that exercises the questionsRequest/questionResponse
    // round-trip documented in the footer.
    println!("\n--- Ambiguous turn (may trigger ask_question) ---");
    let response = agent.chat("What's the weather like right now?").await?;
    println!("Agent: {}\n", response.text());

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
    println!(
        "  WS Receive: stepUpdate.questionsRequest / WS Send: {{\"questionResponse\": ...}} - \
         agent questions answered by the on_questions hook"
    );
    println!("  STDERR: ... - harness diagnostics\n");

    println!("--- Production Considerations ---");
    println!(
        "• Pin the harness: pip install google-antigravity==0.1.5 (see SUPPORTED_HARNESS_VERSION)"
    );
    println!(
        "• Always add policies before enabling write tools (run_command, edit_file, ask_question)"
    );
    println!(
        "• on_questions runs inline in the event pump - never block in it waiting for a human;"
    );
    println!("  answer from policy or pre-collected state (channel + try_recv)");
    println!("• AskQuestion counts as write-capable: enabling it without a policy or");
    println!("  on_pre_tool hook fails the spawn-time safety gate");
    println!(
        "• Call agent.shutdown() for graceful exit; dropping kills the harness without persistence"
    );
    println!("• Use with_save_dir + conversation_id() to resume sessions across runs");
    println!("• Set with_turn_timeout to bound runaway agent turns");

    Ok(())
}
