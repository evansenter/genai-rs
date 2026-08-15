//! # MCP Toolbelt — giving an agent tools it didn't ship with
//!
//! The harness can connect to [Model Context
//! Protocol](https://modelcontextprotocol.io) servers and expose their
//! tools to the model. Unlike `#[tool]` functions — which *your* process
//! executes — MCP tools run in the server's process, and the harness
//! brokers the call. That makes MCP the way to reach a tool you didn't
//! write: a git server, a database bridge, an internal API wrapper.
//!
//! This example spawns a small stdio server (`widgets_server.py`, next to
//! this file) whose answers are values no model could guess, so a correct
//! response is evidence of a real round trip rather than of the model
//! improvising.
//!
//! What this example demonstrates that the others don't:
//!
//! - **`add_mcp_server`** with a stdio transport, plus `with_name` — the
//!   name is not cosmetic, it is the `<server>` half of the policy target.
//! - **`mcp_<server>_<tool>` naming**, which is what a policy or an
//!   `on_pre_tool` hook has to match on. Getting this wrong is the usual
//!   reason an MCP policy silently fails to apply.
//! - **Policy gating an MCP tool** exactly like a builtin — MCP tools run
//!   harness-side, so they go through the same engine.
//! - **`Capabilities::none()` alongside MCP** — MCP servers are configured
//!   independently of the builtin capability set, so an agent can have
//!   *only* external tools.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example mcp_toolbelt --features antigravity
//! LOUD_WIRE=mcpTool,summary cargo run --example mcp_toolbelt --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === MCP Toolbelt ===
//!
//! Serving tools from: .../widgets_server.py
//!
//! [mcp] mcp_widgets_list_widgets     (allowed)
//! [mcp] mcp_widgets_lookup_widget    (allowed)
//! Agent: The flange has code plonkish-4402-vex, with 42 in stock.
//!
//! ✓ The answer carries a value only the MCP server knows.
//! ```

use futures_util::StreamExt;
use genai_rs::antigravity::{AgentEvent, AntigravityAgent, Capabilities, McpServer, policy};

/// A code only the MCP server knows (see `widgets_server.py`). Its
/// presence in the answer is the proof the round trip happened — the model
/// cannot derive it.
const FLANGE_CODE: &str = "plonkish-4402-vex";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = match std::env::var("GEMINI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => {
            // Empty counts as absent: a fork push gets the secret as ""
            // rather than unset, and spawning with it fails mid-turn
            // instead of skipping.
            println!("Skipping: GEMINI_API_KEY not set");
            return Ok(());
        }
    };

    // Ship the server alongside the example so `cargo run` works from any
    // directory. A real deployment would name a command on PATH instead,
    // e.g. McpServer::stdio("uvx", ["mcp-server-git"]).
    let server_script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/real_world/mcp_toolbelt/widgets_server.py");
    if !server_script.is_file() {
        return Err(format!("missing MCP server script: {}", server_script.display()).into());
    }

    println!("=== MCP Toolbelt ===\n");
    println!("Serving tools from: {}\n", server_script.display());

    let mut agent = AntigravityAgent::builder()
        // The builder default is *unlimited*; always set a budget.
        .with_turn_timeout(std::time::Duration::from_secs(120))
        .with_api_key(api_key)
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_system_instructions(
            "Widget codes and stock levels come only from the `widgets` MCP \
             tools. Never guess them. List the widgets first if you need to \
             know what exists, then look up what the user asked about.",
        )
        .add_mcp_server(
            McpServer::stdio("python3", [server_script.to_string_lossy().to_string()])
                // The name is the `<server>` half of `mcp_<server>_<tool>`,
                // so policies have to agree with it. Without this it would
                // default to the command basename ("python3"), which is
                // both wrong and a footgun for the policy target.
                .with_name("widgets"),
        )
        // No builtins at all: this agent's entire toolbelt is external.
        // MCP servers are configured independently of the capability set,
        // so `none()` does not disable them.
        .with_capabilities(Capabilities::none())
        // MCP tools run harness-side, so the policy engine sees them just
        // like `view_file` or `run_command`. Swap this for a targeted
        // allow to gate individual tools:
        //     policy::allow("mcp_widgets_lookup_widget")
        .add_policy(policy::allow_all())
        .spawn()
        .await?;

    let mut calls = Vec::new();
    let mut answer = String::new();
    {
        let mut stream = agent
            .send_streaming("What is the code for the flange, and how many are in stock?")
            .await?;
        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::ToolAction {
                    action, decision, ..
                } => {
                    let name = action.tool_name();
                    println!("[mcp] {name:<28} ({decision:?})");
                    calls.push(name);
                }
                AgentEvent::Finished(response) => {
                    answer = response.text().trim().to_string();
                    break;
                }
                _ => {}
            }
        }
    }

    println!("Agent: {answer}\n");

    // Two independent checks, because they fail for different reasons: no
    // tool action at all means the server never connected, whereas an
    // action without the code means the call happened but its result did
    // not reach the model.
    if !calls.iter().any(|c| c.starts_with("mcp_widgets_")) {
        agent.shutdown().await?;
        return Err("no MCP tool ran — the harness did not reach the server".into());
    }
    if answer.contains(FLANGE_CODE) {
        println!("✓ The answer carries a value only the MCP server knows.");
    } else {
        println!("⚠ The answer did not include the server's code ({FLANGE_CODE}).");
    }

    agent.shutdown().await?;

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  WS Send: {{\"config\": {{\"mcpServers\": [{{\"name\": \"widgets\", ...}}]}}}}");
    println!("    - the server config, sent once at conversation init");
    println!("  WS Receive: {{\"stepUpdate\": {{\"mcpTool\": {{\"serverName\": \"widgets\",");
    println!("    \"toolName\": \"lookup_widget\", \"argumentsJson\": \"...\"}}}}}}");
    println!("    - one per call; the harness brokers it, your process does not");
    println!("  Try LOUD_WIRE=mcpTool,summary to see only these, one line each\n");

    println!("--- Production Considerations ---");
    println!("• The server name is the `<server>` in `mcp_<server>_<tool>` —");
    println!("  a policy naming the wrong one silently never matches");
    println!("• MCP tools run in the SERVER's process, not yours: they get the");
    println!("  server's filesystem, network and credentials, not your agent's");
    println!("• Capabilities and MCP are independent — Capabilities::none() still");
    println!("  leaves MCP tools available, which is how you build an agent whose");
    println!("  only tools are external");
    println!("• Stdio servers are spawned per agent, so a slow-starting server");
    println!("  delays spawn() itself. with_timeout_seconds bounds each tool");
    println!("  CALL, not startup — budget the spawn on your side if it matters");
    println!("• Tool errors (isError) come back as tool results, not transport");
    println!("  failures — the model sees them and can adapt");

    Ok(())
}
