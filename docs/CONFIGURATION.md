# Configuration Guide

This guide covers model configuration options including generation parameters, timeouts, and request customization.

## Table of Contents

- [Overview](#overview)
- [GenerationConfig](#generationconfig)
- [Temperature and Sampling](#temperature-and-sampling)
- [Token Limits](#token-limits)
- [Seeds for Reproducibility](#seeds-for-reproducibility)
- [Stop Sequences](#stop-sequences)
- [Client Configuration](#client-configuration)
- [Request Timeouts](#request-timeouts)
- [Function Calling Modes](#function-calling-modes)
- [Best Practices](#best-practices)

## Overview

Configuration in `genai-rs` happens at three levels:

| Level | Configured Via | Affects |
|-------|---------------|---------|
| **Client** | `Client::builder()` | All requests (timeouts, base URL) |
| **Request** | `with_*()` methods | Single interaction |
| **Generation** | `GenerationConfig` | Model behavior |

## GenerationConfig

The `GenerationConfig` struct controls model generation parameters.

### Using the Struct Directly

```rust,ignore
use genai_rs::{Client, GenerationConfig, ThinkingLevel};

let config = GenerationConfig {
    temperature: Some(0.7),
    max_output_tokens: Some(1024),
    top_p: Some(0.9),
    top_k: Some(40),
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
```

### Using Builder Methods

Most common settings have dedicated builder methods:

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Solve this step by step: 2x + 5 = 15")
    .with_seed(42)
    .with_stop_sequences(vec!["THE END".to_string()])
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;
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

```rust,ignore
let config = GenerationConfig {
    temperature: Some(0.3),  // More focused responses
    ..Default::default()
};
```

### Top-P (Nucleus Sampling)

Limits token selection to cumulative probability:

```rust,ignore
let config = GenerationConfig {
    top_p: Some(0.9),  // Consider tokens totaling 90% probability
    ..Default::default()
};
```

### Top-K

Limits consideration to top K most likely tokens:

```rust,ignore
let config = GenerationConfig {
    top_k: Some(40),  // Only consider top 40 tokens
    ..Default::default()
};
```

### Combining Parameters

```rust,ignore
// Conservative, focused output
let conservative = GenerationConfig {
    temperature: Some(0.2),
    top_p: Some(0.8),
    top_k: Some(20),
    ..Default::default()
};

// Creative, diverse output
let creative = GenerationConfig {
    temperature: Some(0.9),
    top_p: Some(0.95),
    top_k: Some(100),
    ..Default::default()
};
```

## Token Limits

Control output length with `max_output_tokens`:

```rust,ignore
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

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Generate a random number")
    .with_seed(42)
    .create()
    .await?;
```

### Reproducibility Guarantees

```rust,ignore
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

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Tell me a story. End with 'THE END'.")
    .with_stop_sequences(vec!["THE END".to_string()])
    .create()
    .await?;
```

### Multiple Stop Sequences

```rust,ignore
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

```rust,ignore
use std::time::Duration;

let client = Client::builder("api-key".to_string())
    .with_timeout(Duration::from_secs(120))      // Request timeout
    .with_connect_timeout(Duration::from_secs(10)) // Connection timeout
    .build()?;
```

### Custom Base URL

```rust,ignore
let client = Client::builder("api-key".to_string())
    .with_base_url("https://custom-endpoint.example.com")
    .build()?;
```

### Full Configuration Example

```rust,ignore
use std::time::Duration;

let client = Client::builder(api_key)
    .with_timeout(Duration::from_secs(180))       // 3 minute timeout
    .with_connect_timeout(Duration::from_secs(15)) // 15s connection timeout
    .build()?;
```

## Request Timeouts

Override client-level timeout for specific requests:

```rust,ignore
use std::time::Duration;

// Long-running analysis task
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Analyze this large document...")
    .with_timeout(Duration::from_secs(300))  // 5 minute timeout
    .create()
    .await?;
```

### Timeout Guidelines

| Task Type | Suggested Timeout |
|-----------|------------------|
| Simple queries | 30-60 seconds |
| Complex analysis | 120-180 seconds |
| Large document processing | 300+ seconds |
| Streaming responses | Longer (partial results arrive) |

## Function Calling Modes

Control how the model uses declared functions.

### Available Modes

| Mode | Behavior |
|------|----------|
| `Auto` | Model decides whether to call functions (default) |
| `Any` | Model must call a function |
| `None` | Function calling disabled |
| `Validated` | Ensures schema adherence |

### Setting the Mode

```rust,ignore
use genai_rs::FunctionCallingMode;

// Force function use
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather?")
    .add_function(GetWeatherCallable.declaration())
    .with_function_calling_mode(FunctionCallingMode::Any)
    .create()
    .await?;
```

### Via GenerationConfig

```rust,ignore
let config = GenerationConfig {
    tool_choice: Some(FunctionCallingMode::Any),
    ..Default::default()
};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Get current time")
    .add_function(GetTimeCallable.declaration())
    .with_generation_config(config)
    .create()
    .await?;
```

## Best Practices

### 1. Start with Defaults

```rust,ignore
// Let the API use sensible defaults
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Hello!")
    .create()
    .await?;
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

```rust,ignore
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
```

### 4. Set Appropriate Timeouts

```rust,ignore
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
```

### 5. Reuse GenerationConfig

```rust,ignore
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
```

## Configuration Reference

### GenerationConfig Fields

| Field | Type | Description |
|-------|------|-------------|
| `temperature` | `Option<f32>` | Randomness (0.0-2.0) |
| `top_p` | `Option<f32>` | Nucleus sampling (0.0-1.0) |
| `top_k` | `Option<i32>` | Top-K sampling |
| `max_output_tokens` | `Option<i32>` | Maximum response length |
| `seed` | `Option<i64>` | Reproducibility seed |
| `stop_sequences` | `Option<Vec<String>>` | Generation stop triggers |
| `thinking_level` | `Option<ThinkingLevel>` | Chain-of-thought depth |
| `thinking_summaries` | `Option<ThinkingSummaries>` | Include reasoning summary |
| `tool_choice` | `Option<FunctionCallingMode>` | Function calling behavior |
| `speech_config` | `Option<SpeechConfig>` | TTS configuration |

### InteractionBuilder Methods

| Method | Description |
|--------|-------------|
| `with_generation_config()` | Set full GenerationConfig |
| `with_seed()` | Set reproducibility seed |
| `with_stop_sequences()` | Set stop sequences |
| `with_thinking_level()` | Set thinking depth |
| `with_thinking_summaries()` | Set thinking summary mode |
| `with_function_calling_mode()` | Set function calling behavior |
| `with_timeout()` | Set request timeout |

### ClientBuilder Methods

| Method | Description |
|--------|-------------|
| `with_timeout()` | Default request timeout |
| `with_connect_timeout()` | Connection timeout |
| `with_base_url()` | Custom API endpoint |
