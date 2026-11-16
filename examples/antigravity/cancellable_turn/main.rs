//! # Cancellable Turn — stopping an agent mid-thought
//!
//! A long turn you can't interrupt is a hung UI. `cancel_handle()` gives
//! you a cheap, cloneable handle that can halt an in-flight turn from
//! anywhere — which matters because `send_streaming` borrows the agent
//! mutably for the duration, so the streaming loop itself cannot call
//! `cancel()`.
//!
//! **The part worth reading twice**: a cancelled turn does *not* fail. From
//! harness 0.1.10 through the pinned 0.1.23 a halt takes the trajectory to
//! the same terminal state a natural completion does, so the turn resolves
//! normally, carrying
//! whatever text it had produced. Treat `cancel()` as "stop early and keep
//! what you have" — and record the cancellation yourself, because nothing
//! in the response distinguishes a halted turn from a finished one.
//!
//! (`AntigravityError::Turn` of kind `TurnFailure::Cancelled` is still the
//! outcome when the *harness* cancels a turn of its own accord. That is a
//! different event.)
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
//! ## Things to know
//!
//! - The handle is cheap to clone: hand copies to a UI button, a signal
//!   handler and a watchdog at once.
//! - Thinking deltas are not answer text. Halting mid-thought keeps nothing;
//!   trigger on `TextDelta` if partial output is the point.
//! - Ask for the answer *in the reply*. On harness 0.1.23 an agent with no
//!   tools enabled can still answer a request for long writing with a tool
//!   call. Nothing streams while the model writes it, the call fails as
//!   malformed minutes later, and the harness retries the model call. So
//!   the prompt asks for the essay as the reply, not in a tool or a file.
//! - Cancelling before the turn starts is a no-op: there is nothing to halt,
//!   and the halt is dropped.
//! - After a halt the harness logs `context canceled` on stderr. That is the
//!   expected trace of a successful halt, not an error.
//! - A turn that streams too little answer text to halt on within
//!   `ANSWER_DEADLINE` fails, listing on stderr what did arrive: events by
//!   kind, thinking, tool actions and the harness's error steps.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.23   # or set ANTIGRAVITY_HARNESS_PATH
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
//! [stream] 226 chars of answer text — sending halt
//! ✓ Turn ended 0.0s after the halt (budget was 180s)
//!
//! Partial answer (226 chars kept, after 0 chars of thinking):
//!   # The Mechanical Miracle: A Comprehensive History of the Bicycle ...
//!
//! ✓ The turn resolved normally — cancellation is not an error.
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "an example may stop on a missing key or a poisoned lock; the library may not"
)]

use futures_util::StreamExt;
use genai_rs::antigravity::{AgentEvent, AntigravityAgent, Capabilities};
use std::fmt;
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

/// The halt must be sent within this long of the prompt. Answer text
/// normally starts within a minute; a turn with too little by now will not
/// show a halt mid-answer, so it fails here, with what did arrive, rather
/// than at `TURN_BUDGET`.
const ANSWER_DEADLINE: Duration = Duration::from_secs(120);

/// The essay is asked for as the reply itself: see "Things to know".
const PROMPT: &str = "Write an extremely detailed 3000-word essay on the history of the \
                      bicycle, with many sections. Write the essay itself as your reply, \
                      in plain text: do not call any tools and do not write any files.";

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
        .with_model(genai_rs::DEFAULT_MODEL)
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
    let answer_deadline = tokio::time::Instant::now() + ANSWER_DEADLINE;
    let mut seen = Seen::default();
    let (outcome, halted_at) = {
        let mut stream = agent.send_streaming(PROMPT).await?;

        let mut halt_tx = Some(halt_tx);
        let mut halted_at = None;

        let outcome: Result<String, Box<dyn std::error::Error>> = loop {
            // Until the halt is sent, the answer deadline bounds each wait.
            let next = if halt_tx.is_some() {
                match tokio::time::timeout_at(answer_deadline, stream.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        break Err(format!(
                            "{HALT_AFTER_CHARS} chars of answer text did not arrive within \
                             {}s, so nothing was halted",
                            ANSWER_DEADLINE.as_secs()
                        )
                        .into());
                    }
                }
            } else {
                stream.next().await
            };
            let event = match next {
                Some(Ok(event)) => event,
                Some(Err(err)) => break Err(err.into()),
                None => break Err("the stream ended before the turn finished".into()),
            };
            seen.record(&event);
            match event {
                AgentEvent::TextDelta(_) => {
                    if seen.text_chars >= HALT_AFTER_CHARS
                        && let Some(tx) = halt_tx.take()
                    {
                        println!(
                            "[stream] {} chars of answer text — sending halt",
                            seen.text_chars
                        );
                        let _ = tx.send(());
                        halted_at = Some(Instant::now());
                    }
                }
                // The turn RESOLVES rather than erroring — this arm is the
                // one that runs after a cancel, not an error arm.
                AgentEvent::Finished(response) => break Ok(response.text().trim().to_string()),
                // Counted in `seen`, never a halt trigger: see
                // HALT_AFTER_CHARS.
                _ => {}
            }
        };
        (outcome, halted_at)
    };
    let elapsed = started.elapsed();

    let answer = match outcome {
        Ok(answer) => answer,
        Err(err) => {
            eprintln!(
                "After {:.1}s the turn had streamed {seen}",
                elapsed.as_secs_f64()
            );
            canceller.abort();
            if let Err(shutdown) = agent.shutdown().await {
                eprintln!("shutdown also failed: {shutdown}");
            }
            return Err(match halted_at {
                Some(_) => format!("the turn did not end after the halt: {err}").into(),
                None => err,
            });
        }
    };

    match canceller.await.expect("canceller task") {
        Some(Ok(())) => {}
        Some(Err(err)) => return Err(err.into()),
        None => {
            agent.shutdown().await?;
            return Err(format!(
                "the turn finished after only {} chars of answer text ({} thinking), below \
                 the {HALT_AFTER_CHARS} needed to trigger a halt — nothing was cancelled, \
                 so this run shows nothing",
                seen.text_chars, seen.thinking_chars
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
        "\nPartial answer ({} chars kept, after {} chars of thinking):",
        answer.chars().count(),
        seen.thinking_chars
    );
    let preview: String = answer.chars().take(160).collect();
    println!("  {preview}...");

    if answer.is_empty() {
        agent.shutdown().await?;
        return Err("the halted turn kept none of the answer text it had streamed".into());
    }
    if elapsed >= TURN_BUDGET {
        agent.shutdown().await?;
        return Err("the turn ran to its budget — the halt did not stop it".into());
    }
    println!("\n✓ The turn resolved normally — cancellation is not an error.");

    agent.shutdown().await?;

    Ok(())
}

/// What the turn streamed, by kind: printed when the turn fails, so a run
/// that never reaches the halt shows what the agent did instead.
#[derive(Default)]
struct Seen {
    text_events: usize,
    text_chars: usize,
    thinking_events: usize,
    thinking_chars: usize,
    tool_actions: Vec<String>,
    tool_calls: Vec<String>,
    errors: Vec<String>,
    other: Vec<String>,
}

impl Seen {
    fn record(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::TextDelta(chunk) => {
                self.text_events += 1;
                self.text_chars += chunk.chars().count();
            }
            AgentEvent::ThinkingDelta(chunk) => {
                self.thinking_events += 1;
                self.thinking_chars += chunk.chars().count();
            }
            AgentEvent::ToolAction {
                action, decision, ..
            } => {
                let action: String = format!("{action:?}").chars().take(120).collect();
                self.tool_actions.push(format!("{decision:?} {action}"));
            }
            AgentEvent::ToolCallDispatched { name, .. } => self.tool_calls.push(name.clone()),
            AgentEvent::Error { message, severity } => {
                self.errors.push(format!("{severity:?}: {message}"));
            }
            AgentEvent::Finished(_) => {}
            AgentEvent::Unknown { event_type, .. } => self.other.push(event_type.clone()),
            _ => self.other.push("unrecognized event".to_string()),
        }
    }
}

impl fmt::Display for Seen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} text deltas ({} chars), {} thinking deltas ({} chars)",
            self.text_events, self.text_chars, self.thinking_events, self.thinking_chars
        )?;
        writeln!(f, "  tool actions: {:?}", self.tool_actions)?;
        writeln!(f, "  custom tool calls: {:?}", self.tool_calls)?;
        writeln!(f, "  error steps: {:?}", self.errors)?;
        write!(f, "  other events: {:?}", self.other)
    }
}
