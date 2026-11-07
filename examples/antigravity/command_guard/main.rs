//! # Command Guard — policies that read the call, not just its name
//!
//! A policy names a tool; `Policy::when` narrows it to the calls a
//! condition accepts, and `with_reason` sets what the model is told when
//! the rule denies one. Together they give an agent a shell that can read
//! but not change anything:
//!
//! - `allow("run_command").when(read_only)`: plain `ls`, `cat`, `wc`,
//!   `head`, `tail` and `grep`, with no shell operators.
//! - `deny("run_command").when(deletes)`: deletions, with a reason that
//!   tells the model to carry on without them.
//! - `deny_all()`: everything else, with a reason listing what may run.
//!
//! The user asks the agent to delete a file and then count another's
//! lines. The example fails unless a deletion was denied, a read-only
//! command ran, and the file is still there.
//!
//! ## Things to know
//!
//! - Precedence is the harness evaluator's: a rule naming the tool beats
//!   `"*"`, and within a tier deny beats ask beats allow, whatever the
//!   order. A rule whose condition rejects a call does not apply to it, so
//!   the call falls through to the next rule.
//! - Conditions read the model's arguments in the tool's own schema
//!   (`CommandLine` for `run_command`). A deny condition should treat a
//!   missing argument as a match, so a renamed field cannot wave calls
//!   through.
//! - Each rule is evaluated client-side before the call runs; the model
//!   sees a denial's reason as the call's error and adapts.
//! - `data.txt` and `scratch.tmp` are stub files written by the example.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example command_guard --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === Command Guard ===
//!
//! [denied]  rm scratch.tmp
//!           Nothing in this workspace may be deleted. Leave the file and carry on.
//! [allowed] wc -l data.txt
//!
//! Agent: I couldn't delete scratch.tmp (deletion isn't allowed); data.txt has 5 lines.
//! scratch.tmp is still there.
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "an example may stop on a missing key or a poisoned lock; the library may not"
)]

use futures_util::StreamExt;
use genai_rs::antigravity::{
    AgentEvent, AntigravityAgent, BuiltinTool, Capabilities, ToolAction, ToolInvocation, policy,
};
use std::time::Duration;

/// Commands that read without changing anything.
const READ_ONLY_COMMANDS: [&str; 6] = ["ls", "cat", "wc", "head", "tail", "grep"];

/// A plain read-only command: an allowed first word and no shell operator
/// that could chain, redirect or substitute another command.
fn read_only(call: &ToolInvocation) -> bool {
    let Some(command) = call.args["CommandLine"].as_str() else {
        return false;
    };
    let chains = ['|', '&', ';', '>', '<', '`', '$', '\n']
        .iter()
        .any(|op| command.contains(*op));
    let first = command.split_whitespace().next().unwrap_or_default();
    !chains && READ_ONLY_COMMANDS.contains(&first)
}

/// A command that deletes something, or one without a readable command
/// line (fail closed).
fn deletes(call: &ToolInvocation) -> bool {
    call.args["CommandLine"].as_str().is_none_or(|command| {
        command.split_whitespace().any(|word| {
            matches!(word, "rm" | "rmdir" | "unlink" | "shred" | "-delete") || word.ends_with("/rm")
        })
    })
}

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

    println!("=== Command Guard ===\n");

    // Removed when dropped, on every exit path.
    let workspace = tempfile::Builder::new()
        .prefix("command-guard-")
        .tempdir()?;
    let root = workspace.path().canonicalize()?;
    std::fs::write(
        root.join("data.txt"),
        "alpha\nbeta\ngamma\ndelta\nepsilon\n",
    )?;
    let scratch = root.join("scratch.tmp");
    std::fs::write(&scratch, "temporary\n")?;

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(Duration::from_secs(120))
        .with_api_key(api_key)
        .with_model(genai_rs::DEFAULT_MODEL)
        .add_workspace(root.to_string_lossy())
        // A shell and nothing else, so every call goes through the rules
        // below.
        .with_capabilities(Capabilities::none().enable(BuiltinTool::RunCommand))
        .add_policy(
            policy::deny_all()
                .with_reason("Only plain ls, cat, wc, head, tail and grep commands may run here."),
        )
        .add_policy(policy::allow("run_command").when(read_only))
        .add_policy(
            policy::deny("run_command").when(deletes).with_reason(
                "Nothing in this workspace may be deleted. Leave the file and carry on.",
            ),
        )
        .spawn()
        .await?;

    let mut denied = 0;
    let mut allowed = 0;
    let mut answer = String::new();
    {
        let mut stream = agent
            .send_streaming(&format!(
                "In {}, delete scratch.tmp, then tell me how many lines data.txt has.",
                root.display()
            ))
            .await?;
        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::ToolAction {
                    action, decision, ..
                } => match action.as_ref() {
                    ToolAction::Denied { name, args } => {
                        denied += 1;
                        let shown = args["CommandLine"].as_str().unwrap_or(name.as_str());
                        println!("[denied]  {shown}");
                        if let Some(reason) = decision.denial_reason() {
                            println!("          {reason}");
                        }
                    }
                    ToolAction::RunCommand(run) => {
                        allowed += 1;
                        println!("[allowed] {}", run.command_line.as_deref().unwrap_or("?"));
                    }
                    _ => {}
                },
                AgentEvent::Finished(response) => {
                    answer = response.text().trim().to_string();
                    break;
                }
                _ => {}
            }
        }
    }
    agent.shutdown().await?;

    println!("\nAgent: {answer}");
    if !scratch.exists() {
        return Err("scratch.tmp was deleted: the policies did not hold".into());
    }
    println!("scratch.tmp is still there.");
    if denied == 0 {
        return Err("the agent never tried to delete scratch.tmp, so no rule was shown".into());
    }
    if allowed == 0 {
        return Err("no read-only command ran".into());
    }
    Ok(())
}
