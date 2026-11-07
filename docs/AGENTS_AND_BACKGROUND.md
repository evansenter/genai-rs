# Agents and Background Execution

An interaction targets either a **model** (`with_model(...)`), which answers
synchronously, or an **agent** (`with_agent(...)`), which runs a multi-step
task and must run in the background. This page covers agents, the resources
around them (custom agents, environments, credentials, triggers), and how to
get results from background work: webhooks, polling, streaming, and
cancellation.

## Managed agents

Google-managed agents known to this crate:

| Agent ID | Constant | Notes |
|----------|----------|-------|
| `deep-research-preview-04-2026` | `genai_rs::DEFAULT_DEEP_RESEARCH_AGENT` | Deep Research |
| `deep-research-max-preview-04-2026` | — | Deep Research Max |
| `deep-research-pro-preview-12-2025` | — | The Deep Research launch preview |
| `antigravity-preview-09-2026` | `genai_rs::DEFAULT_ANTIGRAVITY_AGENT` | Multi-step tasks with file operations and tool use. Requires an [environment](#environments) |

Prefer the constants. Unknown agent ids pass through unchanged, so newer
agents work without a crate update. Availability varies by account.

Agent interactions **require `with_background(true)`**. They also need
storage, which is on by default; `with_store_disabled()` combined with
background is rejected at build time.

## Deep Research

```rust,no_run
use genai_rs::{DeepResearchConfig, ThinkingSummaries, Visualization};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
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
# let _ = interaction_id;
# Ok(())
# }
```

| Option | Wire field (`agent_config`) | Values | Effect |
|--------|-----------------------------|--------|--------|
| `with_thinking_summaries` | `thinking_summaries` | `"auto"` / `"none"` (the `THINKING_SUMMARIES_*` spelling is rejected) | Reasoning summaries in the output |
| `with_visualization` | `visualization` | `"off"` / `"auto"` | Visualizations in the report |
| `with_collaborative_planning` | `collaborative_planning` | bool | The agent returns a plan and proceeds only after you confirm in the next turn |
| `with_bigquery_tool` | `enable_bigquery_tool` | bool | **Vertex-only**: the Gemini API rejects the field |

Runs take minutes, and the duration varies widely with the question. Get the
result through [webhooks](#webhooks) or [polling](#polling), not a long
request timeout. `examples/deep_research.rs` is runnable.

## Custom agents (`/v1beta/agents`)

A custom agent bundles an id, a system instruction, tools (a subset:
`code_execution`, `function`, `google_search`, `mcp_server`, `url_context`)
and a base environment. `Agent::new(id, base_agent)` takes both required
fields; the only accepted base agent is `DEFAULT_ANTIGRAVITY_AGENT`. A
standard API key can create custom agents. The dated record is under
[Agents and environments](INTERACTIONS_API_GAP.md#agents-and-environments)
in the gap tracker.

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::{Agent, EnvironmentSource, RemoteEnvironment, Tool};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let agent = client.agents().create(
    &Agent::new("customer-sentinel", genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
        .with_system_instruction("You monitor customer feedback.")
        .with_description("Watches feedback channels and summarizes sentiment")
        .add_tool(Tool::code_execution())
        .with_base_environment(
            RemoteEnvironment::new().add_source(EnvironmentSource::gcs("gs://feedback", "/data")),
        ),
).await?;

// Run it like any agent: .with_agent("customer-sentinel").with_background(true)

let fetched = client.agents().get("customer-sentinel").await?;
let page = client.agents().list().with_page_size(50).send().await?; // one page
let all: Vec<Agent> = client.agents().list().items().try_collect().await?; // every page
client.agents().delete("customer-sentinel").await?;
# Ok(())
# }
```

`list()` returns a builder: `.with_page_size()`, `.with_page_token()` and
`.with_parent()` configure it, and `.send()`, `.pages()` or `.items()` run it
(see [Resource handles and list builders](BUILDER_API.md#resource-handles-and-list-builders)).

`Agent::with_agent_config` stores the base agent's configuration, an
`AntigravityConfig` (its model and token cap), with the agent. The API echoes
it back on `Agent::agent_config`, `max_total_tokens` as a string, and keeps
only an Antigravity config: a config of another type is stored as a bare
`{"type": "antigravity"}`.

```rust
use genai_rs::{Agent, AntigravityConfig};

let agent = Agent::new("reviewer", genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
    .with_agent_config(AntigravityConfig::new().with_max_total_tokens(50_000));
# let _ = agent;
```

## Environments

An agent interaction can run in a sandboxed **environment**: mounted sources
(GCS, inline files, repositories, skill registries), environment variables,
and an outbound network policy. `with_environment(...)` takes either a typed
`RemoteEnvironment` or an environment id string.

```rust,no_run
use genai_rs::{AllowlistEntry, AntigravityConfig, EnvVar, EnvironmentSource, NetworkConfig, RemoteEnvironment};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
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
# let _ = env_id;
# Ok(())
# }
```

- **Network policy**: omit `with_network` to allow all outbound traffic, use
  `NetworkConfig::Disabled` to turn networking off, or pass an allowlist
  (wildcards supported; `with_transform` injects headers on matching
  requests).
- **`AntigravityConfig`**: `max_total_tokens` and `model` are accepted on
  `DEFAULT_ANTIGRAVITY_AGENT`. The agent defaults to `gemini-3.8-flash`;
  Google's guide lists the 3.8, 3.7, 3.6 and 3.5 Flash models and
  `gemini-3.5-flash-lite` for `with_model()`, and a model the agent doesn't
  offer fails the interaction with 404 `not_found`. Which of those were
  probed is under AntigravityConfig in
  [Enum Wire Formats](ENUM_WIRE_FORMATS.md#antigravityconfig).
- **An existing environment**: `RemoteEnvironment::new().with_environment_id(id)`
  runs the interaction in that environment, with its files (an ID from
  `environments().create()` or a previous interaction's `environment_id`).
  The interaction reports the ID back, and an ID that does not exist is a
  404.
- A custom agent can carry a default environment via
  `Agent::with_base_environment`. An `environment_id` there is accepted and
  silently dropped, so a custom agent can't be pinned to an existing
  environment that way; pass the ID on each interaction instead.
- **`CodeMenderConfig`** (`agent_config.type: "code-mender"`, with
  `CodeMenderFindRequest`, `CodeMenderFixRequest` and `CodeMenderFile`)
  models the bindings' CodeMender agent config. It is Vertex-only: the
  Gemini API answers `400 The value 'code-mender' is not supported for
  'agent_config.type' on the Gemini API, it is allowed on the Gemini
  Enterprise Agent Platform`.

### Managing environments

Environments are also first-class resources. Create one up front, reference
its id from many interactions, and delete it when done. The full lifecycle
works on a standard API key.

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::{
    CreateEnvironmentRequest, Environment, EnvironmentFile, EnvironmentFileUpload,
    EnvironmentSource,
};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let env = client
    .environments()
    .create(
        &CreateEnvironmentRequest::new()
            .add_source(EnvironmentSource::inline("/workspace/.env", "MODE=ci")),
    )
    .await?;
let env_id = env.id.clone().expect("create returns an id");

// Put a file in before a run, list what the agent left afterwards, and
// read a file back (a directory downloads as a tar archive)
let upload = EnvironmentFileUpload::new(b"a,b\n1,2\n".to_vec(), "text/csv").with_overwrite(true);
client.environments().files().upload(&env_id, "data/input.csv", upload).await?;
let files: Vec<EnvironmentFile> = client
    .environments()
    .files()
    .list(&env_id, "") // "" is the root
    .with_recursive(true)
    .items()
    .try_collect()
    .await?;
let input = client.environments().files().download(&env_id, "data/input.csv").await?;
assert_eq!(input.data, b"a,b\n1,2\n");

// Fork it, files included
let fork = client
    .environments()
    .create(&CreateEnvironmentRequest::from_environment(&env_id))
    .await?;

let fetched = client.environments().get(&env_id).await?;
println!("status={:?} files={:?}", fetched.status, fetched.file_count);
let page = client.environments().list().with_page_size(10).send().await?;
let all: Vec<Environment> = client.environments().list().items().try_collect().await?;
client.environments().delete(&env_id).await?;
# let _ = (files, fork, page, all);
# Ok(())
# }
```

Environment file paths are relative to the root (`""` lists the root), and
`.`/`..` segments are rejected. **Delete what you create.** The spec has an
`expired` status, but don't count on it: environments have stayed `active`
for months after their last use (see the
[gap tracker](INTERACTIONS_API_GAP.md#agents-and-environments)).

### Credentials (`/v1beta/credentials`)

A credential is a server-held secret that an environment references by id,
instead of carrying it inline. Reference it with `EnvVar::credential(id)`
(an environment variable) or `AllowlistEntry::with_credential(id)` (a header
on matching egress). Secret material is write-only: reads return metadata.

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::{
    AllowlistEntry, CreateCredentialRequest, Credential, CredentialType, CredentialUpdate,
    NetworkConfig, RemoteEnvironment,
};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let cred = client
    .credentials()
    .create(&CreateCredentialRequest::bearer_token("s3cret").with_id("github-token"))
    .await?;

// Requests from the sandbox to api.github.com carry the token.
let env = RemoteEnvironment::new().with_network(NetworkConfig::allowlist(vec![
    AllowlistEntry::new("api.github.com").with_credential("github-token"),
]));

// Rotate the secret; the mask travels as the `update_mask` query parameter.
let rotated = CredentialUpdate {
    token: Some("n3w-s3cret".into()),
    ..CredentialUpdate::new(CredentialType::BearerToken)
};
client
    .credentials()
    .update("github-token", &rotated.with_update_mask("token"))
    .await?;

let all: Vec<Credential> = client.credentials().list().items().try_collect().await?;
client.credentials().delete("github-token").await?;
# let _ = (cred, env, all);
# Ok(())
# }
```

Both references take effect at runtime: a bearer credential on an
allowlist entry is injected into requests to that domain, and an
`environment_variable` credential behind `EnvVar::credential` shows the
sandbox only a placeholder, which the egress proxy replaces with the secret
in requests to its `trusted_domains`. An `environment_variable` credential
needs at least one `trusted_domains` entry and an `injection_location`.
`tests/credentials_tests.rs` pins both references; the dated record is
under [Credentials](INTERACTIONS_API_GAP.md#credentials) in the gap
tracker.

## Scheduled triggers (`/v1beta/triggers`)

A trigger runs a stored interaction on a cron schedule, server-side. The
nested interaction must target a **custom agent**: a model-only interaction
is rejected. Create one with `client.agents()` (see
[Custom agents](#custom-agents-v1betaagents)) first.

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::{
    InteractionInput, InteractionRequest, CreateTriggerRequest, TriggerExecution, TriggerStatus,
    TriggerUpdate,
};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let interaction = InteractionRequest {
    agent: Some("my-custom-agent".to_string()),
    input: Some(InteractionInput::Text("Summarize yesterday's alerts".to_string())),
    ..Default::default()
};

// Cron schedule, IANA time zone, the interaction to run
let params = CreateTriggerRequest::new("0 9 * * 1-5", "America/Los_Angeles", interaction)
    .with_display_name("weekday-briefing")
    .with_environment_id("env-id"); // optional
let trigger = client.triggers().create(&params).await?;

let id = trigger.id.clone().expect("created trigger has an id");
let execution = client.triggers().run(&id).await?; // fire now
let recent = client.triggers().list_executions(&id).with_page_size(10).send().await?;
let all: Vec<TriggerExecution> = client.triggers().list_executions(&id).items().try_collect().await?;
client.triggers().update(&id, &TriggerUpdate::new().with_status(TriggerStatus::Paused)).await?;
client.triggers().delete(&id).await?;
# let _ = (execution, recent, all);
# Ok(())
# }
```

The nested request must target a custom agent (an `agents` resource ID, not
a base agent like `DEFAULT_ANTIGRAVITY_AGENT` itself) and must not set
`store`; the API rejects both a model-only request and `store`.
`TriggerUpdate` omits unset fields from the PATCH body, and the server applies
exactly those fields; only `display_name` and `status` are updatable. `run`
fires asynchronously and the API returns `{}`, so the `TriggerExecution` it
returns is empty: find the firing with `list_executions`, whose entries carry
the status and the fired interaction's ID. `list_executions` on a trigger ID
that does not exist returns an empty list, not a 404. The dated record of
these rules is under [Triggers](INTERACTIONS_API_GAP.md#triggers) in the
gap tracker.

## Background execution

`with_background(true)` returns as soon as the interaction is created. It is
required for agents and optional for models.

```rust,no_run
use genai_rs::InteractionStatus;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
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
# Ok(())
# }
```

### Webhooks

Webhooks push lifecycle events to your HTTPS endpoint so you don't have to
poll. `WebhookEvent` covers `interaction.requires_action`,
`interaction.completed` and `interaction.failed`, plus the `batch.*` events
and `video.generated`.

Register once for all matching events:

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::{Webhook, WebhookEvent, WebhookState, WebhookUpdate};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let webhook = client.webhooks().create(
    &Webhook::new(
        "https://example.com/hooks/genai",
        vec![WebhookEvent::InteractionCompleted, WebhookEvent::InteractionFailed],
    )
    .with_name("prod-hook"),
).await?;

// Returned only at creation: store it now
let signing_secret = webhook.new_signing_secret.clone().expect("returned on create");
let id = webhook.id.clone().expect("created webhook has an id");

client.webhooks().ping(&id).await?;                                   // test delivery
let rotated = client.webhooks().rotate_signing_secret(&id, None).await?;
client.webhooks().update(&id, &WebhookUpdate::new().with_state(WebhookState::Disabled)).await?;
let all: Vec<Webhook> = client.webhooks().list().items().try_collect().await?; // every page
client.webhooks().delete(&id).await?;
# let _ = (signing_secret, rotated, all);
# Ok(())
# }
```

`update()` applies the fields set on the `WebhookUpdate`;
`.with_update_mask("state")` adds the optional `update_mask` query parameter,
which the API ignores (see Webhooks in
[Enum Wire Formats](ENUM_WIRE_FORMATS.md#webhooks)).

Or route one request, which overrides the registered webhooks for it and
echoes `user_metadata` on every event:

```rust,no_run
use genai_rs::WebhookConfig;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
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
// On interaction.completed: client.interactions().get(&id_from_event).await?
# let _ = response;
# Ok(())
# }
```

Verify delivery signatures with the signing secret before trusting a
payload. The API disables a webhook after repeated delivery failures
(`WebhookState::DisabledDueToFailedDeliveries`), so monitor its state.
`examples/webhooks_and_background.rs` shows the whole flow.

### Polling

```rust,no_run
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
        let response = client.interactions().get(id).await?;
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
| `Incomplete` | Ended before completion (for example a token limit); inspect partial results. On `CONTINUATION_MODEL` it carries a `continuation_token` to resume the decode |
| `RequiresContinuation` | In the spec, never observed: continuable responses report `Incomplete` |
| `BudgetExceeded` | The configured budget ran out; inspect partial results |
| `Queued` | Accepted, not started. In the bindings, never observed: background interactions report `InProgress` from their create response on |

Persist the interaction id as soon as `create()` returns, so a restart can
resume with `client.interactions().get(&id)`. `response.steps` can be non-empty while
the run is still `InProgress`, and `response.output_steps()` folds those
partial results into a history.

### Streaming a background interaction

`client.interactions().stream(&id)` streams a running interaction from
the start; `resume_stream(&id, last_event_id)` resumes after an event. See
[Streaming API](STREAMING_API.md#stream-resume).

```rust,no_run
use futures_util::StreamExt;

# async fn run(client: genai_rs::Client, interaction_id: String) -> Result<(), genai_rs::GenaiError> {
let mut stream = client.interactions().stream(&interaction_id);
while let Some(event) = stream.next().await {
    let event = event?;
    if let Some(text) = event.chunk.delta_text() {
        print!("{text}");
    }
}
// The stream ends after its terminal event and the server's tail, once
// the interaction is stored (see STREAMING_API.md#read-to-the-end).
# Ok(())
# }
```

### Cancellation

`client.interactions().cancel(&id)` stops a background interaction that is still
`InProgress`, and returns it with status `Cancelled`. It errors if the
interaction is not background or has already finished. On
`DEFAULT_ANTIGRAVITY_AGENT` it fails with `400 There was a problem
processing your request` (a server-side bug; see
[Server-side changes](INTERACTIONS_API_GAP.md#server-side-changes-live-2026-10-01)).
