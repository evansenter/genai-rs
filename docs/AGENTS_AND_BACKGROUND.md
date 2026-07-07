# Agents and Background Execution Guide

This guide covers agent-based interactions and background execution patterns for long-running tasks.

## Table of Contents

- [Overview](#overview)
- [Agents vs Models](#agents-vs-models)
- [Managed Agent IDs](#managed-agent-ids)
- [Deep Research Agent](#deep-research-agent)
- [Custom Agents (Agents Resource)](#custom-agents-agents-resource)
- [Environments](#environments)
- [Background Execution](#background-execution)
- [Webhooks Instead of Polling](#webhooks-instead-of-polling)
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

## Managed Agent IDs

Google-managed agents known to this crate (from the 2026-05-20 spec):

| Agent ID | Description |
|----------|-------------|
| `deep-research-pro-preview-12-2025` | Gemini Deep Research agent (launch preview) |
| `deep-research-preview-04-2026` | Gemini Deep Research agent |
| `deep-research-max-preview-04-2026` | Gemini Deep Research Max agent |
| `antigravity-preview-05-2026` | Antigravity managed agent for multi-step tasks with reasoning, file operations, and tool use (pairs well with [Environments](#environments)) |

Availability varies by account and region; unknown agent IDs pass through
unchanged (Evergreen), so newer agents work without a crate update.

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
use genai_rs::{DeepResearchConfig, ThinkingSummaries, Visualization};

let config = DeepResearchConfig::new()
    .with_thinking_summaries(ThinkingSummaries::Auto) // Include reasoning summary
    .with_visualization(Visualization::Auto)          // Let the agent add visualizations
    .with_collaborative_planning(true)                // Return a plan first; proceed after confirmation
    .with_bigquery_tool(true);                        // Enable the BigQuery tool

client
    .interaction()
    .with_agent("deep-research-preview-04-2026")
    .with_agent_config(config)
    // ...
```

| Option | Wire field | Values | Effect |
|--------|-----------|--------|--------|
| `with_thinking_summaries` | `thinking_summaries` | `THINKING_SUMMARIES_AUTO`/`_NONE` | Reasoning summaries in output |
| `with_visualization` | `visualization` | `"off"` / `"auto"` | Visualizations in the report |
| `with_collaborative_planning` | `collaborative_planning` | bool | Human-in-the-loop planning: the agent returns a research plan and proceeds only after you confirm in the next turn |
| `with_bigquery_tool` | `enable_bigquery_tool` | bool | BigQuery access for the agent |

## Custom Agents (Agents Resource)

Beyond the managed agents, `/v1beta/agents` lets you define reusable custom
agents: an ID, system instruction, tools (subset: `code_execution`,
`url_context`, `google_search`, `mcp_server`), and a base environment.

```rust,ignore
use genai_rs::{Agent, EnvironmentSource, RemoteEnvironment, Tool};

// Create
let agent = client.create_agent(
    &Agent::new("customer-sentinel")
        .with_system_instruction("You monitor customer feedback.")
        .with_description("Watches feedback channels and summarizes sentiment")
        .add_tool(Tool::CodeExecution)
        .with_base_environment(
            RemoteEnvironment::new()
                .add_source(EnvironmentSource::gcs("gs://feedback", "/data")),
        ),
).await?;

// Run it like any agent
let response = client
    .interaction()
    .with_agent("customer-sentinel")
    .with_text("Summarize this week's feedback")
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;

// Manage
let fetched = client.get_agent("customer-sentinel").await?;
let list = client.list_agents(Some(50), None, None).await?; // page_size, page_token, parent
client.delete_agent("customer-sentinel").await?;
```

## Environments

Agent interactions can run inside a sandboxed *environment*: mounted sources
(GCS, inline files, repositories, skill registries) plus an outbound network
policy. Set it per request with `with_environment(...)`, which accepts either
a typed `RemoteEnvironment` or a string environment ID:

```rust,ignore
use genai_rs::{AllowlistEntry, EnvironmentSource, NetworkConfig, RemoteEnvironment};

let response = client
    .interaction()
    .with_agent("antigravity-preview-05-2026")
    .with_text("Run the test suite and report failures")
    .with_environment(
        RemoteEnvironment::new()
            .add_source(EnvironmentSource::repository("github.com/org/repo", "/workspace"))
            .add_source(EnvironmentSource::inline("/workspace/.env", "MODE=ci"))
            .with_network(NetworkConfig::allowlist(vec![
                AllowlistEntry::new("*.crates.io"),
            ])),
    )
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;

// The server assigns an environment ID; reuse it on later turns
let env_id = response.environment_id.clone().expect("assigned environment");
let follow_up = client
    .interaction()
    .with_agent("antigravity-preview-05-2026")
    .with_previous_interaction(response.id.clone().unwrap())
    .with_text("Now fix the failing test")
    .with_environment(env_id)
    .create()
    .await?;
```

Network policy is a union: omit `with_network` to allow all outbound
traffic, use `NetworkConfig::Disabled` to turn networking off, or an
allowlist of domains (wildcards supported; `transform` injects headers on
matching requests). Custom agents can also carry a default environment via
`Agent::with_base_environment`.

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

## Webhooks Instead of Polling

For long-running background work, webhooks push lifecycle events
(`interaction.requires_action`, `interaction.completed`,
`interaction.failed`, plus `batch.*` and `video.generated`) to your HTTPS
endpoint so you don't have to poll.

**Option 1 - register a webhook once** (applies to all matching events):

```rust,ignore
use genai_rs::{Webhook, WebhookEvent, WebhookUpdate, WebhookState};

let webhook = client.create_webhook(
    &Webhook::new(
        "https://example.com/hooks/genai",
        vec![WebhookEvent::InteractionCompleted, WebhookEvent::InteractionFailed],
    )
    .with_name("prod-hook"),
).await?;

// Store this at create time - it is never returned again.
let signing_secret = webhook.new_signing_secret.clone().expect("returned on create");
let id = webhook.id.clone().unwrap();

client.ping_webhook(&id).await?;                       // test delivery
let rotated = client.rotate_webhook_signing_secret(&id, None).await?; // zero-downtime rotation
client.update_webhook(&id, &WebhookUpdate::new().with_state(WebhookState::Disabled), Some("state")).await?;
client.delete_webhook(&id).await?;
```

**Option 2 - per-request routing** with `webhook_config` (overrides the
registered webhooks for one request and echoes `user_metadata` on every
event):

```rust,ignore
use genai_rs::WebhookConfig;

let response = client
    .interaction()
    .with_agent("deep-research-preview-04-2026")
    .with_text("Research the history of quantum computing")
    .with_background(true)
    .with_store_enabled()
    .with_webhook_config(
        WebhookConfig::new()
            .with_uris(vec!["https://example.com/hooks/genai".to_string()])
            .with_user_metadata(serde_json::json!({"job_id": "job-42"})),
    )
    .create()
    .await?;

// On interaction.completed, fetch the result:
// let done = client.get_interaction(&interaction_id_from_event).await?;
```

Operational notes:

- Verify delivery signatures with the signing secret before trusting
  payloads.
- The API disables webhooks after repeated delivery failures
  (`WebhookState::DisabledDueToFailedDeliveries`) - monitor state and
  re-enable after fixing your endpoint.
- See `cargo run --example webhooks_and_background` for the full flow.

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
