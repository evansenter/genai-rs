//! # Session Resume — an agent that remembers across process restarts
//!
//! Two *separate* agent lifecycles against one persisted conversation,
//! which is what a CLI assistant actually does: you run it, it exits, you
//! run it again tomorrow and it still knows what you told it.
//!
//! The harness owns the persistence. Point it at a save directory with
//! [`with_save_dir`], and on shutdown it writes the trajectory there under
//! the conversation id. Hand that same directory *and* id back via
//! [`with_conversation_id`] and the next `spawn()` restores the history —
//! [`initial_history`] returns the restored steps, and the model answers
//! from them without you replaying anything.
//!
//! What this example demonstrates that the others don't:
//!
//! - **`with_save_dir` + `conversation_id()` + `with_conversation_id`** —
//!   the full round trip, including persisting the id the way a real CLI
//!   would (a dotfile next to the save dir).
//! - **`initial_history()`** — inspecting what was restored, which is the
//!   only programmatic evidence that a resume actually resumed rather
//!   than silently starting fresh.
//! - **Recall across the boundary** — run 2 answers a question that can
//!   only be answered from run 1's turn.
//! - **The distinction that matters**: `shutdown()` persists,
//!   dropping does not.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example session_resume --features antigravity
//! LOUD_WIRE=1 cargo run --example session_resume --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === Session Resume ===
//!
//! --- Run 1: fresh session ---
//! Started conversation: 01JD...
//! Restored steps: 0 (fresh conversation)
//! Agent: Noted — the deploy key rotates on the first Monday of each quarter.
//! Persisted id to /tmp/.../conversation-id
//! Shut down cleanly (trajectory saved).
//!
//! --- Run 2: resuming the same conversation ---
//! Resuming conversation: 01JD...
//! Restored steps: 4
//!   [1] user     -> "Remember this: the deploy key rotates ..."
//!   [2] model    -> "Noted — the deploy key rotates ..."
//! Agent: The deploy key rotates on the first Monday of each quarter.
//! ✓ The agent recalled the fact from run 1.
//! ```

use genai_rs::antigravity::{AntigravityAgent, AntigravityError, Capabilities};
use std::path::Path;

/// The fact planted in run 1 and recalled in run 2. Deliberately arbitrary
/// — nothing in the model's training could supply it, so a correct answer
/// in run 2 can *only* have come from restored history.
const PLANTED_FACT: &str = "the deploy key rotates on the first Monday of each quarter";

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

    // A real CLI would use a stable location (e.g. ~/.config/my-agent).
    // A temp dir keeps the example self-contained and re-runnable.
    let save_dir = tempfile::Builder::new()
        .prefix("session-resume-")
        .tempdir()?;
    let save_path = save_dir.path().to_string_lossy().to_string();
    let id_file = save_dir.path().join("conversation-id");

    println!("=== Session Resume ===\n");
    println!("Save directory: {save_path}\n");

    // ---------------------------------------------------------------
    // Run 1 — a fresh conversation. Stands in for the first invocation.
    // ---------------------------------------------------------------
    println!("--- Run 1: fresh session ---");
    let conversation_id = run_one(&api_key, &save_path, &id_file).await?;

    // ---------------------------------------------------------------
    // Run 2 — same save dir, same id. Stands in for a later invocation
    // in a brand-new process: nothing is carried over in memory, only
    // what the harness wrote to disk.
    // ---------------------------------------------------------------
    println!("\n--- Run 2: resuming the same conversation ---");
    run_two(&api_key, &save_path, &conversation_id).await?;

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  HARNESS /path/to/localharness (pid N) - spawned once per run");
    println!("  WS Send: {{\"config\": {{... \"cascadeId\": \"\"}}}} - run 1, no id to resume");
    println!(
        "  WS Receive: {{\"initializeConversationResponse\": {{\"cascadeId\": \"01JD...\"}}}}"
    );
    println!("    - run 1: history absent/empty (fresh conversation)");
    println!(
        "  WS Send: {{\"config\": {{... \"cascadeId\": \"01JD...\"}}}} - run 2 asks to resume"
    );
    println!("  WS Receive: {{\"initializeConversationResponse\": {{\"history\": [...]}}}}");
    println!("    - run 2: the restored steps, which initial_history() exposes\n");

    println!("--- Production Considerations ---");
    println!("• Persist BOTH halves: the save dir and the conversation id. Either");
    println!("  one alone silently starts a fresh conversation rather than failing");
    println!("• Always shutdown() — dropping the agent kills the harness without");
    println!("  writing the trajectory, so the next run has nothing to resume");
    println!("• Check initial_history() rather than assuming: a resume with an");
    println!("  unknown id is not an error, it just comes back empty");
    println!("• The save dir grows per conversation; prune it on your own schedule");
    println!("• Restored history counts toward the context window like any other");
    println!("  history — long-lived conversations still compact");

    Ok(())
}

/// First invocation: start fresh, plant a fact, persist the id.
async fn run_one(
    api_key: &str,
    save_path: &str,
    id_file: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut agent = AntigravityAgent::builder()
        .with_api_key(api_key.to_string())
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "You are a terse note-keeping assistant. Acknowledge facts the user \
             gives you, and recall them verbatim when asked.",
        )
        // No builtin tools needed — this example is about persistence, and
        // the read-only set is the safe default anyway.
        .with_capabilities(Capabilities::none())
        .with_save_dir(save_path.to_string())
        .spawn()
        .await?;

    let conversation_id = agent
        .conversation_id()
        .ok_or("harness did not assign a conversation id")?
        .to_string();
    println!("Started conversation: {conversation_id}");
    println!(
        "Restored steps: {} (fresh conversation)",
        agent.initial_history().len()
    );

    let response = agent
        .chat(&format!("Remember this: {PLANTED_FACT}."))
        .await?;
    println!("Agent: {}", response.text().trim());

    // A real CLI persists the id next to (or alongside) the save dir —
    // without it, run 2 has a trajectory on disk it cannot address.
    std::fs::write(id_file, &conversation_id)?;
    println!("Persisted id to {}", id_file.display());

    // shutdown() is what makes the trajectory durable. Dropping here
    // would leave run 2 with nothing to restore.
    agent.shutdown().await?;
    println!("Shut down cleanly (trajectory saved).");

    Ok(conversation_id)
}

/// Second invocation: resume, prove the history came back, prove recall.
async fn run_two(
    api_key: &str,
    save_path: &str,
    conversation_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Resuming conversation: {conversation_id}");

    let mut agent = AntigravityAgent::builder()
        .with_api_key(api_key.to_string())
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "You are a terse note-keeping assistant. Acknowledge facts the user \
             gives you, and recall them verbatim when asked.",
        )
        .with_capabilities(Capabilities::none())
        // Both halves are required: the directory holds the trajectory,
        // the id selects which one.
        .with_save_dir(save_path.to_string())
        .with_conversation_id(conversation_id.to_string())
        .spawn()
        .await?;

    let history = agent.initial_history();
    println!("Restored steps: {}", history.len());
    for (i, step) in history.iter().take(4).enumerate() {
        // StepSource is a wire enum (no Display); Debug is fine for a
        // human-readable audit line.
        let source = step
            .source
            .as_ref()
            .map_or_else(|| "?".to_string(), |s| format!("{s:?}"));
        let text = step.text.as_deref().unwrap_or("").replace('\n', " ");
        let preview: String = text.chars().take(60).collect();
        println!("  [{}] {source:<8} -> {preview:?}", i + 1);
    }

    // An empty history here is the failure this example exists to make
    // visible: resume is not an error path, so without this check a
    // silently-fresh conversation looks exactly like a working one.
    if history.is_empty() {
        agent.shutdown().await?;
        return Err(Box::new(AntigravityError::Config(
            "resume restored no history — the save dir or conversation id did not match".into(),
        )));
    }

    let response = agent.chat("What did I ask you to remember?").await?;
    let answer = response.text().trim().to_string();
    println!("Agent: {answer}");

    // Structural check, not a phrasing check: the model is free to
    // rephrase, but "deploy key" can only have come from run 1.
    if answer.to_lowercase().contains("deploy key") {
        println!("✓ The agent recalled the fact from run 1.");
    } else {
        println!("⚠ Recall unclear — the answer did not mention the planted fact.");
    }

    agent.shutdown().await?;
    Ok(())
}
