//! # Proactive Agent — work that starts without a user turn
//!
//! Every other example is request/response: you call `chat`, the agent
//! answers. Triggers invert that. `add_trigger` injects a message on a
//! fixed interval with no user turn behind it, which is the shape you want
//! for a watcher — poll a queue, re-check a build, summarize what changed.
//!
//! The sharp edge, and the reason this example exists: **a trigger's turn
//! runs unobserved.** The next `chat`/`send_streaming` halts it and
//! discards its events, so a trigger can never surface as (or desync) your
//! turn's response. Its effects on conversation history do persist — which
//! is what makes triggers useful *and* what makes them easy to misread.
//!
//! What this example demonstrates that the others don't:
//!
//! - **`add_trigger` / `TriggerConfig`** with a real interval.
//! - **Observing a trigger** via a wire inspector, which is the only way
//!   to see one today — there is no event on the agent's own stream.
//! - **The discard boundary**: a `chat` after a trigger returns an answer
//!   to *your* message, never the trigger's.
//! - **Idle-only delivery**: a firing due mid-turn is deferred, and missed
//!   intervals collapse rather than queueing a backlog.
//! - **Why the opening turn is mandatory**: on harness 0.1.10 a trigger
//!   that fires into a conversation with no history crashes the harness
//!   outright. See the comment on the opening `chat` below.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example proactive_agent --features antigravity
//! LOUD_WIRE=automatedTrigger,summary cargo run --example proactive_agent --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === Proactive Agent ===
//!
//! Opening turn: "Ready."
//!
//! Trigger registered: every 2s, "Check the sensor log..."
//! Waiting for the first firing (fires after one interval, not immediately)...
//! ✓ Trigger delivered after 0.1s
//!
//! --- A user turn now interrupts the trigger's turn ---
//! Agent: The sensor log is a stand-in; nothing was actually read.
//! ✓ The answer addresses the user's message, not the trigger's.
//!
//! Triggers delivered during this run: 2
//! ```

use genai_rs::antigravity::{AntigravityAgent, Capabilities, TriggerConfig};
use genai_rs::wire::{WireEvent, WireInspector};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The trigger's message. Distinct from anything the user says, so the
/// two turns can be told apart in the transcript.
const TRIGGER_MESSAGE: &str = "Check the sensor log for anomalies and note anything unusual.";

/// How often the trigger fires. Short so the example finishes quickly; a
/// real watcher would use minutes, not seconds.
const TRIGGER_INTERVAL: Duration = Duration::from_secs(2);

/// Records every `automatedTrigger` the crate sends to the harness.
///
/// A trigger's turn produces no event on the agent's own stream, so a wire
/// inspector is the supported way to confirm one fired at all. This is
/// also the shape you'd use to export trigger deliveries to metrics.
#[derive(Debug, Default)]
struct TriggerWatcher {
    deliveries: Mutex<Vec<String>>,
}

impl WireInspector for TriggerWatcher {
    fn on_event(&self, event: &WireEvent) {
        if let WireEvent::WsSend { payload, .. } = event
            && let Some(message) = payload.get("automatedTrigger").and_then(|v| v.as_str())
        {
            self.deliveries.lock().unwrap().push(message.to_string());
        }
    }
}

impl TriggerWatcher {
    fn count(&self) -> usize {
        self.deliveries.lock().unwrap().len()
    }
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

    println!("=== Proactive Agent ===\n");

    let watcher = Arc::new(TriggerWatcher::default());

    let mut agent = AntigravityAgent::builder()
        // Tighter than the 300s DEFAULT_TURN_TIMEOUT an unset budget now
        // resolves to: this example should fail fast, not sit for five
        // minutes. `without_turn_timeout()` is what opts out entirely.
        .with_turn_timeout(Duration::from_secs(120))
        .with_api_key(api_key)
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_system_instructions(
            "You are a terse monitoring assistant. There is no real sensor \
             log — say so plainly rather than inventing readings.",
        )
        .with_capabilities(Capabilities::none())
        .add_trigger(TriggerConfig::new(TRIGGER_MESSAGE, TRIGGER_INTERVAL))
        .add_wire_inspector(Arc::clone(&watcher) as Arc<dyn WireInspector>)
        .spawn()
        .await?;

    // An opening turn BEFORE any trigger can fire — this is required, not
    // stylistic. Harness 0.1.10 crashes if a trigger is the first activity
    // in a conversation: its pre-invocation hook asks for "tokens since
    // the last checkpoint", finds no steps, and dies with
    //
    //     hook_utils.go:94] error getting tokens since last checkpoint:
    //       earliest step index is out of bounds: 0 vs 0
    //     Agent run failed: executor run failed
    //
    // which kills the harness process and takes the session with it (the
    // next send fails with a closed socket or a broken pipe). One real
    // turn first is enough to give the hook something to measure.
    let opening = agent
        .chat("You are now monitoring. Reply with just: ready.")
        .await?;
    println!("Opening turn: {:?}\n", opening.text().trim());

    println!(
        "Trigger registered: every {}s, {:?}",
        TRIGGER_INTERVAL.as_secs(),
        TRIGGER_MESSAGE
    );
    println!("Waiting for the first firing (fires after one interval, not immediately)...");

    // Poll rather than sleeping a fixed amount: delivery is "after the
    // interval, once idle", which is a lower bound and not a promise.
    let started = Instant::now();
    let deadline = started + Duration::from_secs(30);
    while watcher.count() == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if watcher.count() == 0 {
        agent.shutdown().await?;
        return Err("no trigger delivered within 30s — check the interval and \
                    that the agent stayed idle"
            .into());
    }
    println!(
        "✓ Trigger delivered after {:.1}s",
        started.elapsed().as_secs_f64()
    );

    // ---------------------------------------------------------------
    // The discard boundary. The trigger's turn may still be running; this
    // `chat` halts it and throws its events away before sending. The
    // answer below is to *this* message — a trigger's output can never
    // arrive here.
    // ---------------------------------------------------------------
    println!("\n--- A user turn now interrupts the trigger's turn ---");
    let response = agent
        .chat("Ignore the sensor log. In one sentence: what are you for?")
        .await?;
    let answer = response.text().trim().to_string();
    println!("Agent: {answer}");

    if answer.is_empty() {
        agent.shutdown().await?;
        return Err("the user turn returned no text — a trigger turn may have desynced it".into());
    }
    println!("✓ The answer addresses the user's message, not the trigger's.");

    println!("\nTriggers delivered during this run: {}", watcher.count());

    // shutdown() stops the trigger tasks; so does dropping the agent. No
    // timer outlives the session either way.
    agent.shutdown().await?;

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  WS Send: {{\"automatedTrigger\": \"Check the sensor log...\"}}");
    println!("    - one per firing, sent by the crate, not by a user turn");
    println!("  WS Receive: {{\"stepUpdate\": ...}} - the trigger's turn, unobserved");
    println!("  WS Send: {{\"haltRequest\": true}} then {{\"userInput\": ...}}");
    println!("    - the next chat() halting the trigger's turn before sending");
    println!("  Try LOUD_WIRE=automatedTrigger,summary to see only deliveries\n");

    println!("--- Production Considerations ---");
    println!("• Give the conversation one real turn before any trigger can");
    println!("  fire. On harness 0.1.10 a trigger into an empty conversation");
    println!("  crashes the harness process, not just the turn");
    println!("• A trigger's turn is NOT surfaced: its text never reaches your");
    println!("  stream. Use triggers for side effects (tool calls, history), and");
    println!("  read the results with a normal turn afterwards");
    println!("• Delivery is idle-only. A firing due mid-turn is deferred, and");
    println!("  missed intervals collapse into one — there is no backlog");
    println!("• The first firing is after one interval, not at spawn. Do the");
    println!("  first pass yourself if you need work done immediately");
    println!("• Intervals must be non-zero; spawn() validates this");
    println!("• A wire inspector is the only way to observe deliveries today —");
    println!("  worth wiring to metrics if a silent trigger would matter");
    println!("• Trigger tasks stop on shutdown() and on drop, so no timer leaks");

    Ok(())
}
