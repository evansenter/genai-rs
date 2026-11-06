//! # File Watcher — an agent that reacts to files, and returns what it did
//!
//! `triggers::on_file_change` watches a directory and, when files change,
//! sends the agent a message your callback builds from the changes. The
//! turn that message starts is the agent's own, with no `chat` behind it;
//! `next_trigger_turn` waits for it, drives it and returns its response,
//! so a program can act on what the agent made of the change.
//!
//! Here the watched directory is an inbox. The example drops a request
//! file into it, the watch reports it, and the agent reads the file and
//! answers. The example fails unless the change started a turn and that
//! turn's response came back.
//!
//! ## Things to know
//!
//! - The watch polls (every 500ms here), comparing sizes and modification
//!   times. Its baseline is taken when `on_file_change` is called, so a
//!   file written right after `spawn()` is reported.
//! - Changes during a turn are batched and delivered after it. Returning
//!   `None` from the callback skips a batch (here: deletions only).
//! - Without `next_trigger_turn`, a trigger's turn runs unobserved and the
//!   next `chat` discards its events (see `proactive_agent`).
//! - `add_custom_trigger` takes any `FnOnce(TriggerContext) -> Future`;
//!   `on_file_change` is one such trigger, `TriggerContext::send` the way to
//!   write your own.
//! - The request file is stub data written by the example.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example file_watcher --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === File Watcher ===
//!
//! Watching: /private/var/folders/.../file-watcher-.../inbox
//! Wrote inbox/request-1042.txt
//! [watch] Added request-1042.txt
//!
//! Agent: request-1042.txt asks for 3 widgets shipped to Leeds by Friday.
//! ```

use genai_rs::antigravity::triggers::{self, FileChangeKind};
use genai_rs::antigravity::{AntigravityAgent, Capabilities};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Stub request for the agent to read.
const REQUEST: &str = "Please send 3 widgets to our Leeds office by Friday. -- Birch & Co\n";

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

    println!("=== File Watcher ===\n");

    // Removed when dropped, on every exit path.
    let workspace = tempfile::Builder::new().prefix("file-watcher-").tempdir()?;
    let root = workspace.path().canonicalize()?;
    let inbox = root.join("inbox");
    std::fs::create_dir(&inbox)?;
    println!("Watching: {}", inbox.display());

    // Every change the watch reported, checked at the end.
    let reported: Arc<Mutex<Vec<String>>> = Arc::default();

    let mut agent = AntigravityAgent::builder()
        .with_turn_timeout(Duration::from_secs(120))
        .with_api_key(api_key)
        .with_model(genai_rs::DEFAULT_MODEL)
        .add_workspace(root.to_string_lossy())
        // Reading is all the agent needs; read-only builtins need no policy.
        .with_capabilities(Capabilities::read_only())
        .add_custom_trigger(triggers::on_file_change(
            &inbox,
            Duration::from_millis(500),
            {
                let reported = Arc::clone(&reported);
                move |changes| {
                    let arrived: Vec<String> = changes
                        .iter()
                        .filter(|c| c.kind != FileChangeKind::Deleted)
                        .map(|c| c.path.display().to_string())
                        .collect();
                    for change in changes {
                        let name = change.path.file_name().unwrap_or_default();
                        println!("[watch] {:?} {}", change.kind, name.to_string_lossy());
                        reported
                            .lock()
                            .unwrap()
                            .push(name.to_string_lossy().into_owned());
                    }
                    if arrived.is_empty() {
                        return None;
                    }
                    Some(format!(
                        "New in the inbox: {}. Read each file and reply with one line \
                         per file saying what it asks for.",
                        arrived.join(", ")
                    ))
                }
            },
        ))
        .spawn()
        .await?;

    std::fs::write(inbox.join("request-1042.txt"), REQUEST)?;
    println!("Wrote inbox/request-1042.txt");

    let turn = agent.next_trigger_turn(Duration::from_secs(30)).await;
    agent.shutdown().await?;
    let response = turn?.ok_or("no turn started within 30s of the file change")?;

    let answer = response.text().trim().to_string();
    println!("\nAgent: {answer}");
    if answer.is_empty() {
        return Err("the trigger's turn returned no text".into());
    }
    if !reported
        .lock()
        .unwrap()
        .iter()
        .any(|name| name == "request-1042.txt")
    {
        return Err("the watch never reported request-1042.txt".into());
    }
    Ok(())
}
