# genai-rs

[![Crates.io](https://img.shields.io/crates/v/genai-rs.svg)](https://crates.io/crates/genai-rs)
[![Documentation](https://docs.rs/genai-rs/badge.svg)](https://docs.rs/genai-rs)
[![CI](https://github.com/evansenter/genai-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/evansenter/genai-rs/actions/workflows/rust.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0.html)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A Rust client library for Google's Generative AI (Gemini) API, built on the
[Interactions API](https://ai.google.dev/static/api/interactions-api.md.txt)
(wire revision **2026-05-20**) — plus an optional native client for Google's
Antigravity local agent runtime.

## Quick Start

```rust,no_run
use genai_rs::Client;

#[tokio::main]
async fn main() -> Result<(), genai_rs::GenaiError> {
    let client = Client::new(std::env::var("GEMINI_API_KEY").unwrap());

    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Explain Rust's ownership model in one sentence.")
        .create()
        .await?;

    println!("{}", response.as_text().unwrap_or_default());
    Ok(())
}
```

## Features

### Core Capabilities

| Feature | Description |
|---------|-------------|
| **Steps Model** | Interactions API revision 2026-05-20: responses and history as typed `Step`s, per-step usage, typed citation annotations |
| **Streaming** | Real-time step deltas with resume capability; function-call arguments stream incrementally |
| **Stateful Conversations** | Multi-turn context via `previous_interaction_id`, or stateless step replay with `with_history()` |
| **Function Calling** | Auto-discovery with `#[tool]` macro or manual control |
| **Typed Response Formats** | JSON schema, audio, image, and video output via `with_response_format()` |
| **Thinking Mode** | Model reasoning with configurable depth, thought summaries and signatures |
| **Background + Webhooks** | Background execution, per-request webhook routing, full `/webhooks` resource (CRUD, ping, secret rotation) |
| **Environments & Agents** | `environment` request field and the `/agents` resource client |
| **Multi-Speaker TTS** | `speech_config` list form for multi-voice dialogue |
| **Wire Inspection** | Structured `WireEvent` stream via the `WireInspector` trait; `LOUD_WIRE=1` and `tracing` built-ins |
| **Local Agents** | `antigravity` feature: native client for the Antigravity harness (workspaces, policies, subagents) |

### Built-in Tools

| Tool | Method | Use Case |
|------|--------|----------|
| Google Search | `with_google_search()` | Real-time web grounding |
| Code Execution | `with_code_execution()` | Python sandbox |
| URL Context | `with_url_context()` | Web page analysis |
| Google Maps | `with_google_maps()` | Places and geographic grounding |
| File Search | `add_tool(FileSearchConfig::new(stores))` | Semantic retrieval from vector stores |
| Computer Use | `add_tool(ComputerUseConfig::new())` | Browser/desktop automation (allowlisted keys) |
| MCP Servers | `add_tool(McpServerConfig::new(name, url))` | Model Context Protocol tools |
| Retrieval | `add_tool(RetrievalConfig::new()...)` | Vertex AI Search / RAG stores (Vertex-only, see below) |

### Multimodal I/O

| Input | Output |
|-------|--------|
| Images, Audio, Video, PDFs | Text, Images, Audio (TTS, incl. multi-speaker), Video config |

### API coverage and Vertex-gated features

The crate models the full 2026-05-20 Interactions API surface, with wire
shapes verified live against the Gemini API. A few knobs are **modeled but
gated to Vertex AI** and rejected by the Gemini API today: the Retrieval
tool, `DeepResearchConfig::with_bigquery_tool()`, and video `gcs_uri`
delivery. Details and per-feature live-verification notes are in
[docs/INTERACTIONS_API_GAP.md](docs/INTERACTIONS_API_GAP.md).

## Installation

```toml
[dependencies]
genai-rs = "0.8"
tokio = { version = "1.0", features = ["full"] }

# Optional
genai-rs-macros = "0.8"  # For #[tool] macro
futures-util = "0.3"     # For streaming
```

**Requirements:** Rust 1.88+ (edition 2024), [Gemini API key](https://ai.dev/)

**TLS note:** the crate uses rustls and (as of reqwest 0.13) verifies
certificates against the **OS trust store**. Minimal containers
(scratch/distroless) need a CA bundle installed.

## Examples

Runnable examples covering all features:

```bash
export GEMINI_API_KEY=your-key
cargo run --example simple_interaction
```

**Quick Reference:**

| I want to... | Example |
|--------------|---------|
| Make my first API call | `simple_interaction` |
| Stream responses | `streaming` |
| Use function calling | `auto_function_calling` |
| Multi-turn conversations | `stateful_interaction` |
| Generate images | `image_generation` |
| Text to speech | `text_to_speech` |
| Get structured JSON | `structured_output` |
| Route results to webhooks | `webhooks_and_background` |
| Ground answers in my documents | `retrieval_grounding` |
| Run a local agent on my repo | `repo_auditor` (requires `--features antigravity`) |
| Implement retry logic | `retry_with_backoff` |

See [Examples Index](docs/EXAMPLES_INDEX.md) for the complete categorized list.

## Usage Highlights

### Streaming

```rust,ignore
use futures_util::StreamExt;

let mut stream = client.interaction()
    .with_text("Write a haiku about Rust.")
    .create_stream();

while let Some(Ok(event)) = stream.next().await {
    // delta_text() extracts text from StreamChunk::StepDelta events
    if let Some(text) = event.chunk.delta_text() {
        print!("{}", text);
    }
}
```

### Function Calling with `#[tool]`

```rust,ignore
use genai_rs_macros::tool;

#[tool(location(description = "City name, e.g. Tokyo"))]
fn get_weather(location: String) -> String {
    format!(r#"{{"temp": 72, "conditions": "sunny"}}"#)
}

let result = client.interaction()
    .with_text("What's the weather in Tokyo?")
    .add_function(GetWeatherCallable.declaration())
    .create_with_auto_functions()
    .await?;
```

### Stateful Conversations

```rust,ignore
// First turn (enable storage for multi-turn)
let r1 = client.interaction()
    .with_system_instruction("You are a helpful assistant.")
    .with_text("My name is Alice.")
    .with_store_enabled()
    .create().await?;

// Continue conversation (r1.id is Option<String>)
let r2 = client.interaction()
    .with_previous_interaction(r1.id.as_ref().expect("stored interactions have IDs"))
    .with_text("What's my name?")  // Remembers: Alice
    .create().await?;
```

For stateless deployments, replay history as steps — `output_steps()`
preserves the `signature` fields the API requires on replay:

```rust,ignore
use genai_rs::Step;

let mut history = vec![Step::user_text("My name is Alice.")];
history.extend(r1.output_steps());  // replay model output verbatim

let r2 = client.interaction()
    .with_history(history)
    .with_text("What's my name?")
    .create().await?;
```

### Thinking Mode

```rust,ignore
use genai_rs::ThinkingLevel;

let response = client.interaction()
    .with_thinking_level(ThinkingLevel::High)
    .with_text("What's 15% of 847?")
    .create().await?;

// Check if model used reasoning (thoughts contain cryptographic signatures)
if response.has_thoughts() {
    println!("Model used {} thought blocks", response.thought_signatures().count());
}
```

### Background Execution + Webhook Routing

```rust,ignore
use genai_rs::{Webhook, WebhookConfig, WebhookEvent};

// Register a managed webhook once...
let webhook = client.create_webhook(&Webhook::new(
    "https://example.com/hooks/genai",
    vec![WebhookEvent::InteractionCompleted, WebhookEvent::InteractionFailed],
)).await?;

// ...then route long-running interactions to it (background required)
let response = client.interaction()
    .with_agent("deep-research-preview-04-2026")
    .with_text("Research the history of the Rust programming language.")
    .with_background(true)
    .with_webhook_config(WebhookConfig::new().with_uris(vec![webhook.uri.clone()]))
    .create().await?;
```

### Build & Execute (for Retries)

```rust,ignore
use genai_rs::InteractionRequest;

// Build request without executing (Clone + Serialize)
let request: InteractionRequest = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello!")
    .build()?;

// Execute separately - enables retry loops
let response = client.execute(request.clone()).await?;

// On error, check if retryable: error.is_retryable()
```

See [`retry_with_backoff`](examples/retry_with_backoff.rs) for a complete retry example using the `backon` crate.

## Local Agents (Antigravity)

The `antigravity` feature (off by default) adds a **native Rust client for
Google's Antigravity `localharness` agent runtime** — the same harness behind
the hosted `antigravity-preview-05-2026` agent, running locally with your
workspaces, your tools, and Rust-side policy enforcement. The harness binary
executes the agent loop (shell, file edits, search, MCP, subagents); this
crate speaks its protocol directly, with no Python in the loop:

```rust,ignore
use genai_rs::antigravity::{AntigravityAgent, policy};

let mut agent = AntigravityAgent::builder()
    .with_api_key(std::env::var("GEMINI_API_KEY")?)
    .with_model("gemini-3-flash-preview")
    .add_workspace("/path/to/repo")
    .add_policy(policy::deny_all())          // policies evaluated in Rust,
    .add_policy(policy::allow("view_file"))  // before every tool dispatch
    .spawn()
    .await?;

let response = agent.chat("Summarize the layout of this repo.").await?;
println!("{}", response.text());

agent.shutdown().await?;
```

The same `#[tool]` functions work in both modes, and `LOUD_WIRE=1` covers
harness sessions too. Setup (`pip install google-antigravity==0.1.5`),
capabilities, policies/hooks, MCP servers, subagents, triggers, and session
resume are covered in [docs/ANTIGRAVITY.md](docs/ANTIGRAVITY.md); see
[`repo_auditor`](examples/real_world/repo_auditor/main.rs) for a complete
agentic code-review application.

## Documentation

### Guides

| Guide | Description |
|-------|-------------|
| [Examples Index](docs/EXAMPLES_INDEX.md) | All examples, categorized |
| [Function Calling](docs/FUNCTION_CALLING.md) | `#[tool]` macro, ToolService, manual execution |
| [Multi-Turn Patterns](docs/MULTI_TURN_FUNCTION_CALLING.md) | Stateful/stateless, signature replay, inheritance rules |
| [Streaming API](docs/STREAMING_API.md) | Stream types, resume, auto-functions |
| [Multimodal](docs/MULTIMODAL.md) | Images, audio, video, PDFs |
| [Output Modalities](docs/OUTPUT_MODALITIES.md) | Image generation, text-to-speech |
| [Thinking Mode](docs/THINKING_MODE.md) | Reasoning depth, thought signatures |
| [Built-in Tools](docs/BUILT_IN_TOOLS.md) | Google Search, code execution, URL context, Maps |
| [Configuration](docs/CONFIGURATION.md) | Client options, generation config |
| [Conversation Patterns](docs/CONVERSATION_PATTERNS.md) | Multi-turn, context management |
| [Antigravity](docs/ANTIGRAVITY.md) | Local agent harness: setup, policies, subagents |
| [Agents & Background](docs/AGENTS_AND_BACKGROUND.md) | Hosted agents, long-running tasks, polling |

### Reference

| Document | Description |
|----------|-------------|
| [Builder API](docs/BUILDER_API.md) | Method naming conventions, validation |
| [Error Handling](docs/ERROR_HANDLING.md) | Error types, recovery patterns |
| [Reliability Patterns](docs/RELIABILITY_PATTERNS.md) | Retries, timeouts, resilience |
| [Logging Strategy](docs/LOGGING_STRATEGY.md) | Log levels, `LOUD_WIRE` debugging |
| [Enum Wire Formats](docs/ENUM_WIRE_FORMATS.md) | Verified wire formats, Unknown variants |
| [API Gap Analysis](docs/INTERACTIONS_API_GAP.md) | Coverage tracker, Vertex-only findings |
| [Testing Guide](docs/TESTING.md) | Test strategies, assertions |
| [API Reference](https://docs.rs/genai-rs) | Generated API documentation |

### External Resources

| Resource | Description |
|----------|-------------|
| [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) | Official API specification |
| [Interactions API Guide](https://ai.google.dev/static/api/interactions-api.md.txt) | Usage patterns |
| [Function Calling Guide](https://ai.google.dev/gemini-api/docs/function-calling.md.txt) | Google's function calling docs |

## Debugging & Wire Inspection

```bash
# Wire-level request/response logging (colored, secrets redacted)
LOUD_WIRE=1 cargo run --example simple_interaction

# Wire events via tracing
RUST_LOG=genai_rs::wire=debug cargo run --example simple_interaction

# Library debug logs
RUST_LOG=genai_rs=debug cargo run --example simple_interaction
```

For programmatic capture (snapshot tests, bug reports), implement the
`WireInspector` trait and register it with
`ClientBuilder::add_wire_inspector()` — inspectors receive structured
`WireEvent`s (requests, response bodies, SSE frames, harness WebSocket
traffic) with per-client correlation ids and secret redaction applied.
See [Logging Strategy](docs/LOGGING_STRATEGY.md) for details.

## Forward Compatibility

This library follows the [Evergreen philosophy](https://github.com/google-deepmind/evergreen-spec): unknown API types deserialize into `Unknown` variants instead of failing. Always include wildcard arms:

```rust,ignore
match step {
    Step::ModelOutput { content, .. } => { /* text, images, ... */ }
    Step::FunctionCall { name, .. } => println!("call: {name}"),
    _ => {}  // Handles future variants gracefully
}
```

## Testing

```bash
make test      # Unit tests (uses cargo-nextest)
make test-all  # Full integration suite (requires GEMINI_API_KEY)
```

## Project Structure

```text
genai-rs/           # Main crate: Client, InteractionBuilder, types
genai-rs-macros/    # Procedural macro for #[tool]
docs/               # Comprehensive guides
examples/           # Runnable examples
```

## Contributing

Contributions welcome! Please read:

- [CLAUDE.md](CLAUDE.md) - Development guidelines and architecture
- [CHANGELOG.md](CHANGELOG.md) - Version history and migration guides
- [SECURITY.md](SECURITY.md) - Security policy and reporting

## Troubleshooting

Common issues and solutions are documented in [TROUBLESHOOTING.md](TROUBLESHOOTING.md).

**Quick fixes:**
- **"API key not valid"** - Check `GEMINI_API_KEY` is set
- **"Model not found"** - Use `gemini-3-flash-preview`
- **Functions not executing** - Use `create_with_auto_functions()`
- **TLS errors in minimal containers** - Install a CA bundle (OS trust store is used since reqwest 0.13)

## License

[MIT](LICENSE)
