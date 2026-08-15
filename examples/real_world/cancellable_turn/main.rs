//! # Cancellable Turn — stopping an agent mid-thought
//!
//! A long turn you can't interrupt is a hung UI. `cancel_handle()` gives
//! you a cheap, cloneable handle that can halt an in-flight turn from
//! anywhere — which matters because `send_streaming` borrows the agent
//! mutably for the duration, so the streaming loop itself cannot call
//! `cancel()`.
//!
//! **The part worth reading twice**: a cancelled turn does *not* fail. On
//! harness 0.1.10 a halt takes the trajectory to the same terminal state a
//! natural completion does, so the turn resolves normally, carrying
//! whatever text it had produced. Treat `cancel()` as "stop early and keep
//! what you have" — and record the cancellation yourself, because nothing
//! in the response distinguishes a halted turn from a finished one.
//!
//! (`AntigravityError::Turn` is still the outcome when the *harness*
//! cancels a turn of its own accord. That is a different event.)
//!
//! What this example demonstrates that the others don't:
//!
//! - **`cancel_handle()` used from another task**, escaping the `&mut`
//!   borrow the stream holds.
//! - **Cancelling on a condition** — here, once enough text has arrived —
//!   rather than on a timer that races the model.
//! - **Partial output is real output**: the text produced before the halt
//!   is in the response.
//! - **The contrast with `with_turn_timeout`**, which *does* produce an
//!   error and discards the turn.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example cancellable_turn --features antigravity
//! LOUD_WIRE=haltRequest,trajectoryStateUpdate cargo run --example cancellable_turn --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === Cancellable Turn ===
//!
//! Asking for a long essay, then halting once 200 characters arrive.
//! [stream] 200 chars produced — sending halt
//! ✓ Turn ended 0.4s after the halt (budget was 180s)
//!
//! Partial answer (312 chars kept):
//!   The history of the bicycle begins in the early nineteenth century...
//!
//! ✓ The turn resolved normally — cancellation is not an error.
//! ```

use futures_util::StreamExt;
use genai_rs::antigravity::{AgentEvent, AntigravityAgent, Capabilities};
use std::time::{Duration, Instant};

/// Halt once this much *answer text* has streamed. Keying off real output
/// rather than a timer means the turn is provably mid-generation when the
/// halt lands — a fixed delay races the model and can arrive after it
/// finished.
///
/// Deliberately counts `TextDelta` only, not `ThinkingDelta`: thinking is
/// not answer text, so halting during it keeps nothing. Getting this wrong
/// is how you end up with a "successful" cancel that returns an empty
/// string (observed while writing this example — 404 chars of thinking,
/// zero chars kept).
const HALT_AFTER_CHARS: usize = 200;

/// Generous: if the halt is ignored, we want to see *that*, not a timeout
/// that looks like a clean stop.
const TURN_BUDGET: Duration = Duration::from_secs(180);

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

    println!("=== Cancellable Turn ===\n");

    let mut agent = AntigravityAgent::builder()
        // Deliberately far above what this example needs: the timeout is
        // the *other* mechanism, and letting it fire here would mask
        // whether cancellation worked.
        .with_turn_timeout(TURN_BUDGET)
        .with_api_key(api_key)
        .with_model("gemini-3.6-flash")
        .with_capabilities(Capabilities::none())
        .spawn()
        .await?;

    // Taken BEFORE the stream borrows the agent. This is the whole point
    // of the handle: `send_streaming` holds `&mut agent` for the turn, so
    // there is no way to call `agent.cancel()` while consuming it.
    let handle = agent.cancel_handle();

    // The streaming loop signals; a separate task does the halting. A
    // oneshot rather than a flag so the canceller cannot fire twice, and
    // so a stream that ends early drops the sender and wakes it anyway.
    let (halt_tx, halt_rx) = tokio::sync::oneshot::channel::<()>();
    let canceller = tokio::spawn(async move {
        if halt_rx.await.is_err() {
            // Stream ended before the threshold; nothing to halt.
            return None;
        }
        Some(handle.cancel().await)
    });

    println!("Asking for a long essay, then halting once {HALT_AFTER_CHARS} characters arrive.");

    let started = Instant::now();
    let (answer, produced, thought, halted_at) = {
        let mut stream = agent
            .send_streaming(
                "Write an extremely detailed 3000-word essay on the history of \
                 the bicycle. Include many sections and go slowly.",
            )
            .await?;

        let mut halt_tx = Some(halt_tx);
        let mut produced = 0usize;
        let mut thought = 0usize;
        let mut halted_at = None;
        let mut answer = String::new();

        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::TextDelta(chunk) => {
                    produced += chunk.chars().count();
                    if produced >= HALT_AFTER_CHARS
                        && let Some(tx) = halt_tx.take()
                    {
                        println!("[stream] {produced} chars of answer text — sending halt");
                        let _ = tx.send(());
                        halted_at = Some(Instant::now());
                    }
                }
                // Counted separately and never used as the halt trigger:
                // see HALT_AFTER_CHARS.
                AgentEvent::ThinkingDelta(chunk) => thought += chunk.chars().count(),
                // The turn RESOLVES rather than erroring — this arm is the
                // one that runs after a cancel, not an error arm.
                AgentEvent::Finished(response) => {
                    answer = response.text().trim().to_string();
                    break;
                }
                _ => {}
            }
        }
        (answer, produced, thought, halted_at)
    };
    let elapsed = started.elapsed();

    match canceller.await.expect("canceller task") {
        Some(Ok(())) => {}
        Some(Err(err)) => return Err(err.into()),
        None => {
            agent.shutdown().await?;
            return Err(format!(
                "the turn streamed only {produced} chars of answer text \
                 ({thought} thinking), below the {HALT_AFTER_CHARS} needed to \
                 trigger a halt — nothing was cancelled, so this run shows \
                 nothing"
            )
            .into());
        }
    }

    if let Some(halted_at) = halted_at {
        println!(
            "✓ Turn ended {:.1}s after the halt (budget was {}s)",
            halted_at.elapsed().as_secs_f64(),
            TURN_BUDGET.as_secs()
        );
    }

    // Partial output is the deliverable, not a consolation prize.
    println!(
        "\nPartial answer ({} chars kept, after {thought} chars of thinking):",
        answer.chars().count()
    );
    let preview: String = answer.chars().take(160).collect();
    println!("  {preview}...");

    if elapsed >= TURN_BUDGET {
        println!("\n⚠ The turn ran to its budget — the halt did not stop it.");
    } else {
        println!("\n✓ The turn resolved normally — cancellation is not an error.");
    }

    agent.shutdown().await?;

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  WS Send: {{\"userInput\": \"Write an extremely detailed...\"}}");
    println!("  WS Receive: {{\"stepUpdate\": ...}} - deltas as the essay streams");
    println!("  WS Send: {{\"haltRequest\": true}} - the cancel handle firing");
    println!("  STDERR: \"received model response error: context canceled\"");
    println!("    - the harness aborting its upstream request; expected, not a bug");
    println!("  WS Receive: {{\"trajectoryStateUpdate\": {{\"state\": \"STATE_FULLY_IDLE\"}}}}");
    println!("    - the SAME terminal state a natural completion sends, which is");
    println!("      why the turn resolves instead of failing");
    println!("  Try LOUD_WIRE=haltRequest,trajectoryStateUpdate to see just this\n");

    println!("--- Production Considerations ---");
    println!("• A cancelled turn RESOLVES, carrying partial text. Nothing in the");
    println!("  response says it was halted — record that on your side if it matters");
    println!("• Take the handle before starting the turn: send_streaming holds");
    println!("  &mut agent, so you cannot reach the agent while consuming it");
    println!("• cancel() vs with_turn_timeout: cancel keeps partial output and");
    println!("  succeeds; the timeout discards the turn and returns Timeout");
    println!("• The handle is cheap to clone — hand copies to a UI button, a");
    println!("  shutdown signal handler, and a watchdog all at once");
    println!("• Thinking deltas are not answer text. Halting mid-thought keeps");
    println!("  nothing — trigger on TextDelta if partial output is the point");
    println!("• Cancelling before any output is a no-op worth guarding: there is");
    println!("  no turn to halt yet, and the halt is simply dropped");
    println!("• The harness's \"context canceled\" stderr is the expected trace of");
    println!("  a successful halt, not an error to alert on");

    Ok(())
}
