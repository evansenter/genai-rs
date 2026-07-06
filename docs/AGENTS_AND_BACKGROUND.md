# Agents and Background Execution Guide

This guide covers agent-based interactions and background execution patterns for long-running tasks.

## Table of Contents

- [Overview](#overview)
- [Agents vs Models](#agents-vs-models)
- [Deep Research Agent](#deep-research-agent)
- [Background Execution](#background-execution)
- [Polling Patterns](#polling-patterns)
- [Cancellation](#cancellation)
- [Best Practices](#best-practices)

## Overview

Gemini supports two types of interactions:

| Type | Entry Point | Execution | Use Case |
|------|-------------|-----------|----------|
| **Model** | `with_model("gemini-3-flash-preview")` | Synchronous | Quick responses, streaming |
| **Agent** | `with_agent("deep-research-pro-preview")` | Background | Long-running tasks, research |

Agents are specialized systems that perform multi-step tasks autonomously.

## Agents vs Models

### Models

Direct interaction with a language model:

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Explain quantum computing")
    .create()
    .await?;

// Response available immediately
println!("{}", response.as_text().unwrap());
```

### Agents

Autonomous systems that execute complex workflows:

```rust,ignore
let response = client
    .interaction()
    .with_agent("deep-research-pro-preview-12-2025")
    .with_text("Research best practices for Rust REST APIs")
    .with_background(true)      // Required for agents
    .with_store_enabled()       // Required to retrieve results
    .create()
    .await?;

// Returns immediately with interaction ID
// Must poll for completion
```

## Deep Research Agent

The Deep Research agent conducts multi-step research by:
1. Executing iterative web searches
2. Synthesizing information across sources
3. Generating comprehensive reports

### Basic Usage

```rust,ignore
use genai_rs::{Client, DeepResearchConfig, ThinkingSummaries};

let response = client
    .interaction()
    .with_agent("deep-research-pro-preview-12-2025")
    .with_text("What are the current best practices for building production REST APIs in Rust?")
    .with_agent_config(
        DeepResearchConfig::new()
            .with_thinking_summaries(ThinkingSummaries::Auto)
    )
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;

let interaction_id = response.id.expect("stored interaction has ID");
```

### Expected Runtime

- Simple queries: 30-60 seconds
- Complex research: 60-120+ seconds
- Very comprehensive queries: 2+ minutes

### Configuration Options

```rust,ignore
use genai_rs::{DeepResearchConfig, ThinkingSummaries};

let config = DeepResearchConfig::new()
    .with_thinking_summaries(ThinkingSummaries::Auto);  // Include reasoning summary

client
    .interaction()
    .with_agent("deep-research-pro-preview-12-2025")
    .with_agent_config(config)
    // ...
```

## Background Execution

Background mode allows requests to return immediately while processing continues.

### When to Use Background Mode

| Scenario | Background? | Why |
|----------|-------------|-----|
| Agent interactions | **Required** | Agents don't support synchronous execution |
| Long model requests | Optional | Avoid timeout, handle asynchronously |
| Batch processing | Recommended | Submit many, poll results |

### Starting a Background Task

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")  // or with_agent()
    .with_text("Complex analysis task...")
    .with_background(true)
    .with_store_enabled()  // Must enable storage to retrieve results
    .create()
    .await?;

// Response returns immediately
match response.status {
    InteractionStatus::InProgress => {
        println!("Task running, ID: {:?}", response.id);
    }
    InteractionStatus::Completed => {
        println!("Completed immediately: {}", response.as_text().unwrap());
    }
    _ => {}
}
```

### Requirements

- `with_store_enabled()` - Required to retrieve results by ID
- `with_background(true)` - Required for agent interactions

## Polling Patterns

### Basic Polling

```rust,ignore
use std::time::{Duration, Instant};
use tokio::time::sleep;

async fn poll_for_completion(
    client: &Client,
    interaction_id: &str,
    max_wait: Duration,
) -> Result<InteractionResponse, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut delay = Duration::from_secs(2);
    let max_delay = Duration::from_secs(10);

    loop {
        if start.elapsed() > max_wait {
            return Err("Polling timed out".into());
        }

        let response = client.get_interaction(interaction_id).await?;

        match response.status {
            InteractionStatus::Completed => return Ok(response),
            InteractionStatus::Failed => return Err("Task failed".into()),
            InteractionStatus::Cancelled => return Err("Task cancelled".into()),
            InteractionStatus::BudgetExceeded => return Err("Budget exceeded".into()),
            InteractionStatus::InProgress => {
                // Exponential backoff
                sleep(delay).await;
                delay = (delay * 2).min(max_delay);
            }
            _ => {
                // Unknown status - continue polling (Evergreen pattern)
                sleep(delay).await;
            }
        }
    }
}
```

### Usage

```rust,ignore
// Start background task
let initial = client
    .interaction()
    .with_agent("deep-research-pro-preview-12-2025")
    .with_text("Research topic")
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;

// Poll for completion
let result = poll_for_completion(
    &client,
    initial.id.as_ref().unwrap(),
    Duration::from_secs(120),
).await?;

println!("Research complete: {}", result.as_text().unwrap());
```

### Streaming During Polling

You can also stream results as they become available:

```rust,ignore
use futures_util::StreamExt;
use genai_rs::StreamChunk;

// Second argument is an optional last_event_id for resuming a prior stream
let mut stream = client.get_interaction_stream(interaction_id, None);

while let Some(result) = stream.next().await {
    match result {
        Ok(event) => match event.chunk {
            StreamChunk::StepDelta { delta, .. } => {
                if let Some(text) = delta.as_text() {
                    print!("{}", text);
                }
            }
            StreamChunk::Completed(_response) => {
                println!("\nComplete!");
                break;
            }
            _ => {}
        },
        Err(e) => {
            eprintln!("Stream error: {}", e);
            break;
        }
    }
}
```

## Cancellation

Long-running tasks can be cancelled:

```rust,ignore
// Start a background task
let response = client
    .interaction()
    .with_agent("deep-research-pro-preview-12-2025")
    .with_text("Very long research query")
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;

let interaction_id = response.id.unwrap();

// Later, cancel if needed
client.cancel_interaction(&interaction_id).await?;

// Check status
let cancelled = client.get_interaction(&interaction_id).await?;
assert_eq!(cancelled.status, InteractionStatus::Cancelled);
```

### Cancellation Behavior

- Already completed tasks cannot be cancelled
- Cancelled tasks may have partial results
- Cancellation is not instantaneous

## Best Practices

### 1. Always Use Exponential Backoff

```rust,ignore
let mut delay = Duration::from_secs(2);
let max_delay = Duration::from_secs(10);

// After each poll
delay = (delay * 2).min(max_delay);  // 2s, 4s, 8s, 10s, 10s...
```

### 2. Set Reasonable Timeouts

```rust,ignore
const MAX_POLL_DURATION: Duration = Duration::from_secs(120);  // 2 minutes

// For Deep Research, consider longer timeouts
const RESEARCH_TIMEOUT: Duration = Duration::from_secs(300);  // 5 minutes
```

### 3. Handle All Status Values

```rust,ignore
match response.status {
    InteractionStatus::Completed => { /* success */ }
    InteractionStatus::Failed => { /* handle failure */ }
    InteractionStatus::Cancelled => { /* handle cancellation */ }
    InteractionStatus::InProgress => { /* keep polling */ }
    InteractionStatus::RequiresAction => { /* handle required action */ }
    InteractionStatus::Incomplete => { /* ended early, e.g. token limit */ }
    InteractionStatus::BudgetExceeded => { /* configured budget exhausted */ }
    _ => {
        // Unknown status - log and continue (Evergreen pattern)
        log::warn!("Unknown status: {:?}", response.status);
    }
}
```

### 4. Store Interaction IDs

For recovery after crashes or restarts:

```rust,ignore
// Save interaction ID to persistent storage
save_to_database(&interaction_id);

// Later, resume polling
let interaction_id = load_from_database();
let result = client.get_interaction(&interaction_id).await?;
```

### 5. Handle Partial Results

Background tasks may have intermediate outputs:

```rust,ignore
let response = client.get_interaction(&interaction_id).await?;

// Check for steps even if still in progress
if !response.steps.is_empty() {
    println!("Partial results available");
}
```

To fold partial or final results into a conversation history, use
`response.output_steps()`:

```rust,ignore
let mut history: Vec<Step> = vec![Step::user_text("Research topic")];
history.extend(response.output_steps());
```

## Status Reference

| Status | Meaning | Action |
|--------|---------|--------|
| `InProgress` | Task running | Continue polling |
| `Completed` | Task finished successfully | Retrieve results |
| `Failed` | Task failed | Check error, possibly retry |
| `Cancelled` | Task was cancelled | Handle cancellation |
| `RequiresAction` | Task needs input | Rare for agents, check response |
| `Incomplete` | Ended before completion (e.g., token limit) | Inspect partial results |
| `BudgetExceeded` | Configured budget was exceeded | Inspect partial results, adjust budget |

Unrecognized statuses deserialize to `InteractionStatus::Unknown` — keep
polling on unknown statuses and rely on your own timeout (Evergreen pattern).

## Example

See `cargo run --example deep_research` for a complete working example.

```bash
GEMINI_API_KEY=your-key cargo run --example deep_research
```
