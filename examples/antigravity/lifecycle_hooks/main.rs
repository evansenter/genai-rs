//! # Lifecycle Hooks — a definition of done the agent cannot skip
//!
//! The tool hooks (`on_pre_tool`, `on_post_tool`) see single calls. The
//! lifecycle hooks see the session and its turns: `on_session_start`,
//! `on_pre_turn`, `on_stop`, `on_post_turn` and `on_session_end`, plus
//! `on_tool_error` and `on_compaction`, which have the same shape.
//!
//! The one with teeth is `on_stop`. The harness asks it before a turn
//! ends; returning `StopDecision::continue_with(instruction)` sends the
//! instruction to the model and keeps the turn going. Here it is a
//! definition of done: the user asks for a summary of a CSV, and the stop
//! hook refuses to let the turn end until the summary is also saved to
//! `SUMMARY.md` in the workspace, something the user's message never asked
//! for. The example fails unless the hook kept the turn going and the file
//! exists afterwards.
//!
//! ## Things to know
//!
//! - Every hook is a synchronous closure on a thread of its own, bounded
//!   by the turn deadline. `on_session_end` runs during `shutdown()`.
//! - `StopRequest::continuation_count` says how many times a stop hook has
//!   already kept this turn going. Stop past a limit of your own, or a
//!   condition the model cannot meet keeps the turn running until the
//!   deadline.
//! - `on_pre_turn` can refuse a turn with `TurnDecision::deny(reason)`; the
//!   turn then fails with `AntigravityError::Turn` and the agent stays
//!   usable.
//! - `create_file` is write-capable, so `spawn()` needs a policy or a
//!   pre-tool hook. The harness's workspace containment (on by default)
//!   keeps the write inside the workspace.
//! - `orders.csv` is stub data written by the example.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example lifecycle_hooks --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === Lifecycle Hooks ===
//!
//! [session_start]
//! [pre_turn] "Summarize orders.csv in two sentences."
//! [stop #0] SUMMARY.md missing: continue
//! [stop #1] SUMMARY.md written: stop
//! [post_turn] 212 chars
//!
//! Agent: Orders.csv lists four orders ...
//!
//! SUMMARY.md:
//! Four orders, ...
//!
//! [session_end]
//! Hook order: session_start, pre_turn, stop, stop, post_turn, session_end
//! ```

use genai_rs::antigravity::{
    AntigravityAgent, BuiltinTool, Capabilities, StopDecision, TurnDecision, policy,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How many times the stop hook may keep one turn going.
const MAX_CONTINUATIONS: u32 = 3;

/// Stub order data for the agent to summarize.
const ORDERS_CSV: &str = "order_id,customer,item,quantity,status\n\
1041,Acme Ltd,widget,3,shipped\n\
1042,Birch & Co,gadget,1,pending\n\
1043,Acme Ltd,widget,10,pending\n\
1044,Cobalt Inc,sprocket,4,cancelled\n";

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

    println!("=== Lifecycle Hooks ===\n");

    // Removed when dropped, on every exit path.
    let workspace = tempfile::Builder::new()
        .prefix("lifecycle-hooks-")
        .tempdir()?;
    // Canonical, so the path the hook checks is the one the harness writes
    // (on macOS the temp dir is reached through a symlink).
    let root = workspace.path().canonicalize()?;
    std::fs::write(root.join("orders.csv"), ORDERS_CSV)?;
    let summary_path: PathBuf = root.join("SUMMARY.md");

    // The order the hooks fired in, checked at the end.
    let fired: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let continuations = Arc::new(Mutex::new(0u32));

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(Duration::from_secs(180))
        .with_api_key(api_key)
        .with_model(genai_rs::DEFAULT_MODEL)
        .add_workspace(root.to_string_lossy())
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::CreateFile))
        .add_policy(policy::allow_all())
        .on_session_start({
            let fired = Arc::clone(&fired);
            move || {
                println!("[session_start]");
                fired.lock().unwrap().push("session_start");
            }
        })
        .on_pre_turn({
            let fired = Arc::clone(&fired);
            move |turn| {
                println!("[pre_turn] {:?}", turn.text);
                fired.lock().unwrap().push("pre_turn");
                TurnDecision::Allow
            }
        })
        .on_stop({
            let fired = Arc::clone(&fired);
            let continuations = Arc::clone(&continuations);
            let summary_path = summary_path.clone();
            move |request| {
                fired.lock().unwrap().push("stop");
                let count = request.continuation_count;
                if summary_path.is_file() {
                    println!("[stop #{count}] SUMMARY.md written: stop");
                    return StopDecision::Stop;
                }
                if count >= MAX_CONTINUATIONS {
                    println!("[stop #{count}] SUMMARY.md still missing: giving up");
                    return StopDecision::Stop;
                }
                println!("[stop #{count}] SUMMARY.md missing: continue");
                *continuations.lock().unwrap() += 1;
                StopDecision::continue_with(format!(
                    "Before finishing, save the summary you wrote to {} with create_file.",
                    summary_path.display()
                ))
            }
        })
        .on_post_turn({
            let fired = Arc::clone(&fired);
            move |turn| {
                println!("[post_turn] {} chars", turn.response_text.chars().count());
                fired.lock().unwrap().push("post_turn");
            }
        })
        .on_session_end({
            let fired = Arc::clone(&fired);
            move || {
                println!("[session_end]");
                fired.lock().unwrap().push("session_end");
            }
        })
        .spawn()
        .await?;

    // The message says nothing about SUMMARY.md: the stop hook adds it.
    let result = agent
        .chat("Summarize orders.csv in two sentences.")
        .await
        .map(|response| response.text().trim().to_string());
    // Shut down even when the turn failed; on_session_end runs here.
    agent.shutdown().await?;
    let answer = result?;
    println!("\nAgent: {answer}\n");

    let summary = std::fs::read_to_string(&summary_path)
        .map_err(|e| format!("SUMMARY.md was not written ({e}); the stop hook did not hold"))?;
    println!("SUMMARY.md:\n{}\n", summary.trim());

    let continued = *continuations.lock().unwrap();
    if continued == 0 {
        return Err(
            "the stop hook never kept the turn going: the model wrote SUMMARY.md \
                    unasked, so this run showed nothing"
                .into(),
        );
    }

    let order = fired.lock().unwrap().clone();
    println!("Hook order: {}", order.join(", "));
    if order.first() != Some(&"session_start") || order.last() != Some(&"session_end") {
        return Err(format!("unexpected hook order: {order:?}").into());
    }
    Ok(())
}
