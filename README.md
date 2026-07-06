# genai-rs

[![Crates.io](https://img.shields.io/crates/v/genai-rs.svg)](https://crates.io/crates/genai-rs)
[![Documentation](https://docs.rs/genai-rs/badge.svg)](https://docs.rs/genai-rs)
[![CI](https://github.com/evansenter/genai-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/evansenter/genai-rs/actions/workflows/rust.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue.svg)](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0.html)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A Rust client library for Google's Generative AI (Gemini) API using the [Interactions API](https://ai.google.dev/static/api/interactions-api.md.txt).

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
| **Streaming** | Real-time token streaming with resume capability |
| **Stateful Conversations** | Multi-turn context via `previous_interaction_id` |
| **Function Calling** | Auto-discovery with `#[tool]` macro or manual control |
| **Structured Output** | JSON schema enforcement with `with_response_format()` |
| **Thinking Mode** | Access model reasoning with configurable depth |

### Built-in Tools

| Tool | Method | Use Case |
|------|--------|----------|
| Google Search | `with_google_search()` | Real-time web grounding |
| Code Execution | `with_code_execution()` | Python sandbox |
| URL Context | `with_url_context()` | Web page analysis |

### Multimodal I/O

| Input | Output |
|-------|--------|
| Images, Audio, Video, PDFs | Text, Images, Audio (TTS) |

## Installation

```toml
[dependencies]
genai-rs = "0.7"
tokio = { version = "1.0", features = ["full"] }

# Optional
genai-rs-macros = "0.7"  # For #[tool] macro
futures-util = "0.3"     # For streaming
```

**Requirements:** Rust 1.88+ (edition 2024), [Gemini API key](https://ai.dev/)

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

## Documentation

### Guides

| Guide | Description |
|-------|-------------|
| [Examples Index](docs/EXAMPLES_INDEX.md) | All examples, categorized |
| [Function Calling](docs/FUNCTION_CALLING.md) | `#[tool]` macro, ToolService, manual execution |
| [Multi-Turn Patterns](docs/MULTI_TURN_FUNCTION_CALLING.md) | Stateful/stateless, inheritance rules |
| [Streaming API](docs/STREAMING_API.md) | Stream types, resume, auto-functions |
| [Multimodal](docs/MULTIMODAL.md) | Images, audio, video, PDFs |
| [Output Modalities](docs/OUTPUT_MODALITIES.md) | Image generation, text-to-speech |
| [Thinking Mode](docs/THINKING_MODE.md) | Reasoning depth, thought signatures |
| [Built-in Tools](docs/BUILT_IN_TOOLS.md) | Google Search, code execution, URL context |
| [Configuration](docs/CONFIGURATION.md) | Client options, generation config |
| [Conversation Patterns](docs/CONVERSATION_PATTERNS.md) | Multi-turn, context management |

### Reference

| Document | Description |
|----------|-------------|
| [Error Handling](docs/ERROR_HANDLING.md) | Error types, recovery patterns |
| [Reliability Patterns](docs/RELIABILITY_PATTERNS.md) | Retries, timeouts, resilience |
| [Logging Strategy](docs/LOGGING_STRATEGY.md) | Log levels, `LOUD_WIRE` debugging |
| [Testing Guide](docs/TESTING.md) | Test strategies, assertions |
| [Agents & Background](docs/AGENTS_AND_BACKGROUND.md) | Long-running tasks, polling |
| [API Reference](https://docs.rs/genai-rs) | Generated API documentation |

### External Resources

| Resource | Description |
|----------|-------------|
| [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) | Official API specification |
| [Interactions API Guide](https://ai.google.dev/static/api/interactions-api.md.txt) | Usage patterns |
| [Function Calling Guide](https://ai.google.dev/gemini-api/docs/function-calling.md.txt) | Google's function calling docs |

## Debugging

```bash
# Wire-level request/response logging
LOUD_WIRE=1 cargo run --example simple_interaction

# Library debug logs
RUST_LOG=genai_rs=debug cargo run --example simple_interaction
```

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

## License

[MIT](LICENSE)
