# Configuration Guide

This guide covers model configuration options including generation parameters, timeouts, and request customization.

## Table of Contents

- [Overview](#overview)
- [GenerationConfig](#generationconfig)
- [Temperature and Sampling](#temperature-and-sampling)
- [Repetition Penalties](#repetition-penalties)
- [Token Limits](#token-limits)
- [Seeds for Reproducibility](#seeds-for-reproducibility)
- [Stop Sequences](#stop-sequences)
- [Client Configuration](#client-configuration)
- [Request Timeouts](#request-timeouts)
- [Service Tier](#service-tier)
- [Cached Content](#cached-content)
- [Function Calling Modes](#function-calling-modes)
- [Best Practices](#best-practices)
- [Configuration Reference](#configuration-reference)

## Overview

Configuration in `genai-rs` happens at three levels:

| Level | Configured Via | Affects |
|-------|---------------|---------|
| **Client** | `Client::builder()` | All requests (timeouts) |
| **Request** | `with_*()` methods | Single interaction |
| **Generation** | `GenerationConfig` | Model behavior |

## GenerationConfig

The `GenerationConfig` struct controls model generation parameters.

### Using the Struct Directly

```rust,no_run
# use genai_rs::{Client, GenerationConfig, ThinkingLevel};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let config = GenerationConfig {
    temperature: Some(0.7),
    max_output_tokens: Some(1024),
    top_p: Some(0.9),
    seed: Some(42),
    stop_sequences: Some(vec!["END".to_string()]),
    thinking_level: Some(ThinkingLevel::Medium),
    ..Default::default()
};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Write a short story")
    .with_generation_config(config)
    .create()
    .await?;
# Ok(())
# }
```

### Using Builder Methods

Most common settings have dedicated builder methods:

```rust,no_run
# use genai_rs::{Client, ThinkingLevel};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Solve this step by step: 2x + 5 = 15")
    .with_seed(42)
    .with_stop_sequences(vec!["THE END".to_string()])
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;
# Ok(())
# }
```

## Temperature and Sampling

Control randomness in model outputs.

### Temperature

| Value | Effect | Use Case |
|-------|--------|----------|
| 0.0 | Deterministic | Factual queries, code generation |
| 0.3-0.5 | Low randomness | Balanced responses |
| 0.7-0.9 | Higher creativity | Creative writing |
| 1.0+ | Maximum randomness | Brainstorming |

```rust
# use genai_rs::GenerationConfig;
let config = GenerationConfig {
    temperature: Some(0.3),  // More focused responses
    ..Default::default()
};
```

### Top-P (Nucleus Sampling)

Limits token selection to cumulative probability:

```rust
# use genai_rs::GenerationConfig;
let config = GenerationConfig {
    top_p: Some(0.9),  // Consider tokens totaling 90% probability
    ..Default::default()
};
```

Note: The API revision 2026-05-20 removed `top_k` from `GenerationConfig`; use `temperature` and `top_p` to control sampling.

### Combining Parameters

```rust
# use genai_rs::GenerationConfig;
// Conservative, focused output
let conservative = GenerationConfig {
    temperature: Some(0.2),
    top_p: Some(0.8),
    ..Default::default()
};

// Creative, diverse output
let creative = GenerationConfig {
    temperature: Some(0.9),
    top_p: Some(0.95),
    ..Default::default()
};
```

## Repetition Penalties

Discourage the model from repeating itself. Both penalties accept values in the range [-2.0, 2.0].

| Parameter | Effect |
|-----------|--------|
| `presence_penalty` | Positive values penalize tokens that already appeared at all, encouraging new topics |
| `frequency_penalty` | Positive values penalize tokens proportionally to how often they appeared, reducing repetition |

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Brainstorm 20 startup ideas")
    .with_presence_penalty(0.5)   // Encourage new topics
    .with_frequency_penalty(0.3)  // Reduce word repetition
    .create()
    .await?;
# Ok(())
# }
```

Or via the struct:

```rust
# use genai_rs::GenerationConfig;
let config = GenerationConfig {
    presence_penalty: Some(0.5),
    frequency_penalty: Some(0.3),
    ..Default::default()
};
```

## Token Limits

Control output length with `max_output_tokens`:

```rust
# use genai_rs::GenerationConfig;
let config = GenerationConfig {
    max_output_tokens: Some(500),  // Limit response to ~500 tokens
    ..Default::default()
};
```

### Typical Limits by Model

| Model | Default Max | Absolute Max |
|-------|-------------|--------------|
| gemini-3-flash-preview | 8192 | 8192 |
| gemini-3.1-pro-preview | 8192 | 8192 |

Note: Actual limits vary by model version. Check [Google's documentation](https://ai.google.dev/models/gemini) for current values.

## Seeds for Reproducibility

Seeds enable reproducible outputs for testing and debugging.

### Basic Usage

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Generate a random number")
    .with_seed(42)
    .create()
    .await?;
# Ok(())
# }
```

### Reproducibility Guarantees

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
// Same seed + same input = same output
let response1 = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What is 2+2?")
    .with_seed(42)
    .create()
    .await?;

let response2 = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What is 2+2?")
    .with_seed(42)
    .create()
    .await?;

// response1.as_text() should equal response2.as_text()
# Ok(())
# }
```

### Use Cases for Seeds

| Use Case | Approach |
|----------|----------|
| Unit testing | Fixed seed for deterministic assertions |
| A/B testing | Same seed across variants for fair comparison |
| Debugging | Reproduce unexpected outputs |
| Demos | Consistent examples in documentation |

## Stop Sequences

Halt generation when specific strings are produced.

### Basic Usage

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Tell me a story. End with 'THE END'.")
    .with_stop_sequences(vec!["THE END".to_string()])
    .create()
    .await?;
# Ok(())
# }
```

### Multiple Stop Sequences

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Generate a list")
    .with_stop_sequences(vec![
        "---".to_string(),
        "END OF LIST".to_string(),
        "\n\n\n".to_string(),
    ])
    .create()
    .await?;
# Ok(())
# }
```

### Use Cases

| Use Case | Stop Sequence |
|----------|---------------|
| Story generation | "THE END", "---" |
| Code extraction | "```" (end of code block) |
| List generation | "\n\n" (double newline) |
| Q&A format | "Question:" (prevent next question) |

## Client Configuration

Configure the HTTP client for all requests.

### Timeouts

```rust
use genai_rs::Client;
use std::time::Duration;

let client = Client::builder("api-key".to_string())
    .with_timeout(Duration::from_secs(120))        // Request timeout
    .with_connect_timeout(Duration::from_secs(10)) // Connection timeout
    .build()?;
# Ok::<(), genai_rs::GenaiError>(())
```

### Full Configuration Example

```rust
use genai_rs::Client;
use std::time::Duration;

# let api_key = "api-key".to_string();
let client = Client::builder(api_key)
    .with_timeout(Duration::from_secs(180))        // 3 minute timeout
    .with_connect_timeout(Duration::from_secs(15)) // 15s connection timeout
    .build()?;
# Ok::<(), genai_rs::GenaiError>(())
```

## Request Timeouts

Override client-level timeout for specific requests:

```rust,no_run
# use genai_rs::Client;
# use std::time::Duration;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
// Long-running analysis task
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Analyze this large document...")
    .with_timeout(Duration::from_secs(300))  // 5 minute timeout
    .create()
    .await?;
# Ok(())
# }
```

### Timeout Guidelines

| Task Type | Suggested Timeout |
|-----------|------------------|
| Simple queries | 30-60 seconds |
| Complex analysis | 120-180 seconds |
| Large document processing | 300+ seconds |
| Streaming responses | Longer (partial results arrive) |

## Service Tier

Select a latency/priority tier for a request with `with_service_tier()`:

| Tier | Wire Value | Behavior |
|------|-----------|----------|
| `ServiceTier::Flex` | `"flex"` | Flexible latency, lower cost |
| `ServiceTier::Standard` | `"standard"` | Standard processing |
| `ServiceTier::Priority` | `"priority"` | Prioritized processing |

Unrecognized tiers deserialize into `ServiceTier::Unknown { tier_type, data }` (Evergreen pattern).

```rust,no_run
# use genai_rs::{Client, ServiceTier};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello")
    .with_service_tier(ServiceTier::Flex)
    .create()
    .await?;
# Ok(())
# }
```

## Cached Content

Reference an explicit context cache to reuse large, repeated context (e.g., a long document) across requests:

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Summarize the cached document")
    .with_cached_content("cachedContents/xyz")
    .create()
    .await?;
# Ok(())
# }
```

## Function Calling Modes

Control how the model uses declared functions. Modes serialize as lowercase strings on the wire (`"auto"`, `"any"`, `"none"`, `"validated"`).

### Available Modes

| Mode | Wire Value | Behavior |
|------|-----------|----------|
| `Auto` | `"auto"` | Model decides whether to call functions (default) |
| `Any` | `"any"` | Model must call a function |
| `None` | `"none"` | Function calling disabled |
| `Validated` | `"validated"` | Ensures schema adherence |

### Setting the Mode

```rust,no_run
# use genai_rs::{Client, FunctionCallingMode, FunctionDeclaration};
# async fn example(client: &Client, weather_declaration: FunctionDeclaration) -> Result<(), genai_rs::GenaiError> {
// Force function use
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather?")
    .add_function(weather_declaration)
    .with_function_calling_mode(FunctionCallingMode::Any)
    .create()
    .await?;
# Ok(())
# }
```

### Via GenerationConfig

`GenerationConfig.tool_choice` is a [`ToolChoice`] union: either a plain mode or an `allowed_tools` restriction object.

```rust,no_run
# use genai_rs::{Client, FunctionCallingMode, FunctionDeclaration, GenerationConfig, ToolChoice};
# async fn example(client: &Client, time_declaration: FunctionDeclaration) -> Result<(), genai_rs::GenaiError> {
let config = GenerationConfig {
    tool_choice: Some(ToolChoice::Mode(FunctionCallingMode::Any)),
    ..Default::default()
};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Get current time")
    .add_function(time_declaration)
    .with_generation_config(config)
    .create()
    .await?;
# Ok(())
# }
```

### Restricting to Named Tools

`with_allowed_tools()` restricts the model to calling only the named tools. It sets `tool_choice` to the `AllowedTools` restriction object (preserving any previously set mode):

```rust,no_run
# use genai_rs::{Client, FunctionDeclaration};
# async fn example(client: &Client, weather_declaration: FunctionDeclaration) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Get weather in Tokyo")
    .add_function(weather_declaration)
    .with_allowed_tools(vec!["get_weather".to_string()])
    .create()
    .await?;
# Ok(())
# }
```

For full control over the union, use `with_tool_choice()`:

```rust,no_run
# use genai_rs::{Client, FunctionCallingMode, ToolChoice};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather?")
    .with_tool_choice(ToolChoice::allowed_tools(
        Some(FunctionCallingMode::Any),
        vec!["get_weather".to_string()],
    ))
    .create()
    .await?;
# Ok(())
# }
```

## Best Practices

### 1. Start with Defaults

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
// Let the API use sensible defaults
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello!")
    .create()
    .await?;
# Ok(())
# }
```

### 2. Use Seeds for Testing

```rust,ignore
#[cfg(test)]
mod tests {
    const TEST_SEED: i64 = 12345;

    #[tokio::test]
    async fn test_model_output() {
        let response = client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("Generate test data")
            .with_seed(TEST_SEED)
            .create()
            .await?;

        // Now outputs are reproducible for assertions
    }
}
```

### 3. Match Temperature to Task

```rust,no_run
# use genai_rs::{Client, GenerationConfig};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
// Factual query - low temperature
let fact_response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What is the capital of France?")
    .with_generation_config(GenerationConfig {
        temperature: Some(0.0),
        ..Default::default()
    })
    .create()
    .await?;

// Creative task - higher temperature
let story_response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Write a creative poem about the moon")
    .with_generation_config(GenerationConfig {
        temperature: Some(0.9),
        ..Default::default()
    })
    .create()
    .await?;
# Ok(())
# }
```

### 4. Set Appropriate Timeouts

```rust,no_run
# use genai_rs::Client;
# use std::time::Duration;
# async fn example(client: &Client, long_document: String) -> Result<(), genai_rs::GenaiError> {
// Quick lookup
let quick = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What is 2+2?")
    .with_timeout(Duration::from_secs(30))
    .create()
    .await?;

// Complex analysis
let analysis = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text(&long_document)
    .with_timeout(Duration::from_secs(180))
    .create()
    .await?;
# Ok(())
# }
```

### 5. Reuse GenerationConfig

```rust,no_run
# use genai_rs::{Client, GenerationConfig};
# async fn example(client: &Client, creative_prompts: Vec<String>) -> Result<(), genai_rs::GenaiError> {
// Define once, use many times
let creative_config = GenerationConfig {
    temperature: Some(0.8),
    top_p: Some(0.95),
    max_output_tokens: Some(2048),
    ..Default::default()
};

for prompt in creative_prompts {
    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(prompt)
        .with_generation_config(creative_config.clone())
        .create()
        .await?;
}
# Ok(())
# }
```

## Configuration Reference

### GenerationConfig Fields

| Field | Type | Description |
|-------|------|-------------|
| `temperature` | `Option<f32>` | Randomness (0.0-2.0) |
| `top_p` | `Option<f32>` | Nucleus sampling (0.0-1.0) |
| `max_output_tokens` | `Option<i32>` | Maximum response length |
| `seed` | `Option<i64>` | Reproducibility seed |
| `stop_sequences` | `Option<Vec<String>>` | Generation stop triggers |
| `thinking_level` | `Option<ThinkingLevel>` | Chain-of-thought depth |
| `thinking_summaries` | `Option<ThinkingSummaries>` | Include reasoning summary |
| `tool_choice` | `Option<ToolChoice>` | Function calling behavior (mode or allowed-tools restriction) |
| `presence_penalty` | `Option<f32>` | Penalize already-present tokens [-2.0, 2.0] |
| `frequency_penalty` | `Option<f32>` | Penalize frequent tokens [-2.0, 2.0] |
| `speech_config` | `Option<SpeechConfig>` | TTS configuration |
| `image_config` | `Option<ImageConfig>` | Image generation aspect ratio and size |

Note: `top_k` was removed by the API revision 2026-05-20.

### InteractionBuilder Methods

| Method | Description |
|--------|-------------|
| `with_generation_config()` | Set full GenerationConfig |
| `with_seed()` | Set reproducibility seed |
| `with_stop_sequences()` | Set stop sequences |
| `with_presence_penalty()` | Set presence penalty |
| `with_frequency_penalty()` | Set frequency penalty |
| `with_thinking_level()` | Set thinking depth |
| `with_thinking_summaries()` | Set thinking summary mode |
| `with_function_calling_mode()` | Set function calling mode (plain-mode form) |
| `with_tool_choice()` | Set the full `ToolChoice` union directly |
| `with_allowed_tools()` | Restrict model to named tools (AllowedTools form) |
| `with_service_tier()` | Set latency/priority tier |
| `with_cached_content()` | Reference an explicit context cache |
| `with_response_format()` | JSON schema for structured output |
| `with_timeout()` | Set request timeout |

### ClientBuilder Methods

| Method | Description |
|--------|-------------|
| `with_timeout()` | Default request timeout |
| `with_connect_timeout()` | Connection timeout |
| `add_wire_inspector()` | Observe raw API traffic (requests, responses, SSE frames) |
