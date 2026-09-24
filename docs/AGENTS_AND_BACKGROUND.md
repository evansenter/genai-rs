# Agents and Background Execution

An interaction targets either a **model** (`with_model(...)`), which answers
synchronously, or an **agent** (`with_agent(...)`), which runs a multi-step
task and must run in the background. This page covers agents, the resources
around them (custom agents, environments, credentials, triggers), and how to
get results from background work: webhooks, polling, streaming, and
cancellation.

## Managed agents

Google-managed agents known to this crate (from the 2026-05-20 spec):

| Agent ID | Constant | Notes |
|----------|----------|-------|
| `deep-research-preview-04-2026` | `genai_rs::DEFAULT_DEEP_RESEARCH_AGENT` | Deep Research |
| `deep-research-max-preview-04-2026` | — | Deep Research Max |
| `deep-research-pro-preview-12-2025` | — | The Deep Research launch preview |
| `antigravity-preview-05-2026` | `genai_rs::DEFAULT_ANTIGRAVITY_AGENT` | Multi-step tasks with file operations and tool use. Requires an [environment](#environments) |

Prefer the constants. Unknown agent ids pass through unchanged, so newer
agents work without a crate update. Availability varies by account.

Agent interactions **require `with_background(true)`**. They also need
storage, which is on by default; `with_store_disabled()` combined with
background is rejected at build time.

## Deep Research

```rust,ignore
use genai_rs::{DeepResearchConfig, ThinkingSummaries, Visualization};

let response = client
    .interaction()
    .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
    .with_text("What are the current best practices for production REST APIs in Rust?")
    .with_agent_config(
        DeepResearchConfig::new()
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .with_visualization(Visualization::Auto)
            .with_collaborative_planning(true),
    )
    .with_background(true)
    .create()
    .await?;

let interaction_id = response.id.expect("stored interaction has an id");
```

| Option | Wire field (`agent_config`) | Values | Effect |
|--------|-----------------------------|--------|--------|
| `with_thinking_summaries` | `thinking_summaries` | `"auto"` / `"none"` (`THINKING_SUMMARIES_*` is rejected since 2026-08-10) | Reasoning summaries in the output |
| `with_visualization` | `visualization` | `"off"` / `"auto"` | Visualizations in the report |
| `with_collaborative_planning` | `collaborative_planning` | bool | The agent returns a plan and proceeds only after you confirm in the next turn |
| `with_bigquery_tool` | `enable_bigquery_tool` | bool | **Vertex-only**: the Gemini API rejects the field |

Runs take minutes, and the duration varies widely with the question. Get the
result through [webhooks](#webhooks) or [polling](#polling), not a long
request timeout. `examples/deep_research.rs` is runnable.

## Custom agents (`/v1beta/agents`)

A custom agent bundles an id, a system instruction, tools (a subset:
`code_execution`, `url_context`, `google_search`, `mcp_server`) and a base
environment. Creating custom agents is gated on standard API keys.

```rust,ignore
use genai_rs::{Agent, EnvironmentSource, RemoteEnvironment, Tool};

let agent = client.create_agent(
    &Agent::new("customer-sentinel")
        .with_system_instruction("You monitor customer feedback.")
        .with_description("Watches feedback channels and summarizes sentiment")
        .add_tool(Tool::CodeExecution)
        .with_base_environment(
            RemoteEnvironment::new().add_source(EnvironmentSource::gcs("gs://feedback", "/data")),
        ),
).await?;

// Run it like any agent: .with_agent("customer-sentinel").with_background(true)

let fetched = client.get_agent("customer-sentinel").await?;
let page = client.list_agents(Some(50), None, None).await?; // page_size, page_token, parent
client.delete_agent("customer-sentinel").await?;
```

## Environments

An agent interaction can run in a sandboxed **environment**: mounted sources
(GCS, inline files, repositories, skill registries), environment variables,
and an outbound network policy. `with_environment(...)` takes either a typed
`RemoteEnvironment` or an environment id string.

```rust,ignore
use genai_rs::{AllowlistEntry, AntigravityConfig, EnvVar, EnvironmentSource, NetworkConfig, RemoteEnvironment};

let response = client
    .interaction()
    .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
    .with_text("Run the test suite and report failures")
    .with_environment(
        RemoteEnvironment::new()
            .add_source(EnvironmentSource::repository("github.com/org/repo", "/workspace"))
            .add_source(EnvironmentSource::inline("/workspace/.env", "MODE=ci"))
            .add_env_var("CI", EnvVar::value("true"))
            .with_network(NetworkConfig::allowlist(vec![AllowlistEntry::new("*.crates.io")])),
    )
    .with_agent_config(AntigravityConfig::new().with_max_total_tokens(200_000))
    .with_background(true)
    .create()
    .await?;

// The server assigns an environment id; pass it to reuse the environment
let env_id = response.environment_id.clone().expect("assigned environment");
```

- **Network policy**: omit `with_network` to allow all outbound traffic, use
  `NetworkConfig::Disabled` to turn networking off, or pass an allowlist
  (wildcards supported; `with_transform` injects headers on matching
  requests).
- **`AntigravityConfig`**: the Antigravity agent config was verified end to
  end on 2026-08-09. Leave its `model` unset: an unavailable value returns 404,
  as `gemini-3.6-flash` did then, even though it worked for ordinary
  interactions.
- A custom agent can carry a default environment via
  `Agent::with_base_environment`.

### Managing environments

Environments are also first-class resources. Create one up front, reference
its id from many interactions, and delete it when done. The full lifecycle
works on a standard API key.

```rust,ignore
use genai_rs::{CreateEnvironmentRequest, EnvironmentFileUpload, EnvironmentSource};

let env = client
    .create_environment(
        &CreateEnvironmentRequest::new()
            .add_source(EnvironmentSource::inline("/workspace/.env", "MODE=ci")),
    )
    .await?;
let env_id = env.id.clone().expect("create returns an id");

// Put a file in before a run, and list what the agent left afterwards
client
    .upload_environment_file(&env_id, "data/input.csv", b"a,b\n1,2\n".to_vec(), "text/csv",
        EnvironmentFileUpload { overwrite: true, ..Default::default() })
    .await?;
let files = client.list_environment_files(&env_id, "", true, None, None).await?; // path, recursive

// Fork it, files included
let fork = client
    .create_environment(&CreateEnvironmentRequest::from_environment(&env_id))
    .await?;

let fetched = client.get_environment(&env_id).await?;
println!("status={:?} files={:?}", fetched.status, fetched.file_count);
let page = client.list_environments(Some(10), None).await?;
client.delete_environment(&env_id).await?;
```

Environment file paths are relative to the root (`""` lists the root), and
`.`/`..` segments are rejected. Environments expire on their own (`status`
moves from `active` to `expired`), but delete what you create: repeated runs
otherwise accumulate containers until they expire.

### Credentials (`/v1beta/credentials`)

A credential is a server-held secret that an environment references by id,
instead of carrying it inline. Reference it with `EnvVar::credential(id)`
(an environment variable) or `AllowlistEntry::with_credential(id)` (a header
on matching egress). Secret material is write-only: reads return metadata.

```rust,ignore
use genai_rs::{CreateCredentialRequest, EnvVar, RemoteEnvironment};

let cred = client
    .create_credential(&CreateCredentialRequest::bearer_token("s3cret").with_id("github-token"))
    .await?;

let env = RemoteEnvironment::new().add_env_var("GITHUB_TOKEN", EnvVar::credential("github-token"));
```

CRUD was verified live (2026-09-24): create, get, list, update with and
without `update_mask`, and delete. **The references have no observed effect
yet.** The API accepts and echoes them, but an Antigravity sandbox saw
neither the variable nor an injected header.

## Scheduled triggers (`/v1beta/triggers`)

A trigger runs a stored interaction on a cron schedule, server-side. The
nested interaction must target a **custom agent**: a model-only interaction
is rejected. Since custom-agent creation is gated, most accounts can list
triggers but not create them.

```rust,ignore
use genai_rs::{InteractionInput, InteractionRequest, TriggerCreateParams, TriggerStatus, TriggerUpdate};

let interaction = InteractionRequest {
    agent: Some("my-custom-agent".to_string()),
    input: InteractionInput::Text("Summarize yesterday's alerts".to_string()),
    ..Default::default()
};

// Cron schedule, IANA time zone, the interaction to run
let params = TriggerCreateParams::new("0 9 * * 1-5", "America/Los_Angeles", interaction)
    .with_display_name("weekday-briefing")
    .with_environment_id("env-id"); // optional
let trigger = client.create_trigger(&params).await?;

let id = trigger.id.clone().expect("created trigger has an id");
let execution = client.run_trigger(&id).await?; // fire now
let runs = client.list_trigger_executions(&id, Some(10), None).await?;
client.update_trigger(&id, &TriggerUpdate::new().with_status(TriggerStatus::Paused)).await?;
client.delete_trigger(&id).await?;
```

`TriggerUpdate` omits unset fields from the PATCH body. The endpoint takes no
`update_mask`, so partial-update behavior rests on that omission, and it is
unverified until trigger updates can be live-tested. The `genai_rs::triggers`
docs cover the execution-status lifecycle.

## Background execution

`with_background(true)` returns as soon as the interaction is created. It is
required for agents and optional for models.

```rust,ignore
use genai_rs::InteractionStatus;

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL) // or .with_agent(...)
    .with_text("Complex analysis task...")
    .with_background(true)
    .create()
    .await?;

match response.status {
    InteractionStatus::InProgress => println!("running: {:?}", response.id),
    InteractionStatus::Completed => println!("done: {}", response.as_text().unwrap_or_default()),
    _ => {}
}
```

### Webhooks

Webhooks push lifecycle events to your HTTPS endpoint so you don't have to
poll. `WebhookEvent` covers `interaction.requires_action`,
`interaction.completed` and `interaction.failed`, plus the `batch.*` events
and `video.generated`.

Register once for all matching events:

```rust,ignore
use genai_rs::{Webhook, WebhookEvent, WebhookState, WebhookUpdate};

let webhook = client.create_webhook(
    &Webhook::new(
        "https://example.com/hooks/genai",
        vec![WebhookEvent::InteractionCompleted, WebhookEvent::InteractionFailed],
    )
    .with_name("prod-hook"),
).await?;

// Returned only at creation: store it now
let signing_secret = webhook.new_signing_secret.clone().expect("returned on create");
let id = webhook.id.clone().expect("created webhook has an id");

client.ping_webhook(&id).await?;                                  // test delivery
let rotated = client.rotate_webhook_signing_secret(&id, None).await?;
client.update_webhook(&id, &WebhookUpdate::new().with_state(WebhookState::Disabled), Some("state")).await?;
client.delete_webhook(&id).await?;
```

Or route one request, which overrides the registered webhooks for it and
echoes `user_metadata` on every event:

```rust,ignore
use genai_rs::WebhookConfig;

let response = client
    .interaction()
    .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
    .with_text("Research the history of quantum computing")
    .with_background(true)
    .with_webhook_config(
        WebhookConfig::new()
            .with_uris(vec!["https://example.com/hooks/genai".to_string()])
            .with_user_metadata(serde_json::json!({"job_id": "job-42"})),
    )
    .create()
    .await?;
// On interaction.completed: client.get_interaction(&id_from_event).await?
```

Verify delivery signatures with the signing secret before trusting a
payload. The API disables a webhook after repeated delivery failures
(`WebhookState::DisabledDueToFailedDeliveries`), so monitor its state.
`examples/webhooks_and_background.rs` shows the whole flow.

### Polling

```rust,ignore
use genai_rs::{Client, InteractionResponse, InteractionStatus};
use std::time::{Duration, Instant};

async fn poll_until_done(
    client: &Client,
    id: &str,
    deadline: Duration,
) -> Result<InteractionResponse, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut delay = Duration::from_secs(2);
    loop {
        let response = client.get_interaction(id).await?;
        match response.status {
            InteractionStatus::Completed => return Ok(response),
            InteractionStatus::Failed
            | InteractionStatus::Cancelled
            | InteractionStatus::Incomplete
            | InteractionStatus::BudgetExceeded => {
                return Err(format!("ended as {:?}", response.status).into());
            }
            // InProgress, RequiresAction, and statuses this crate doesn't
            // know yet (InteractionStatus::Unknown): keep polling
            _ => {}
        }
        if start.elapsed() > deadline {
            return Err("polling deadline exceeded".into());
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(10));
    }
}
```

| Status | Meaning |
|--------|---------|
| `InProgress` | Still running |
| `Completed` | Finished; read the result |
| `Failed` | Failed |
| `Cancelled` | Cancelled |
| `RequiresAction` | Waiting for input |
| `Incomplete` | Ended before completion (for example a token limit); inspect partial results |
| `BudgetExceeded` | The configured budget ran out; inspect partial results |

Persist the interaction id as soon as `create()` returns, so a restart can
resume with `get_interaction(&id)`. `response.steps` can be non-empty while
the run is still `InProgress`, and `response.output_steps()` folds those
partial results into a history.

### Streaming a background interaction

`client.get_interaction_stream(&id, None)` streams a running interaction from
the start. Pass a `last_event_id` to resume; see
[Streaming API](STREAMING_API.md#stream-resume).

```rust,ignore
use futures_util::StreamExt;

let mut stream = client.get_interaction_stream(&interaction_id, None);
while let Some(event) = stream.next().await {
    let event = event?;
    if let Some(text) = event.chunk.delta_text() {
        print!("{text}");
    }
    if event.is_terminal() {
        break;
    }
}
```

### Cancellation

`client.cancel_interaction(&id)` stops a background interaction that is still
`InProgress`, and returns it with status `Cancelled`. It errors if the
interaction is not background or has already finished.
