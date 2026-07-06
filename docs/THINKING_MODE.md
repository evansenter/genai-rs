# Thinking Mode Guide

This guide covers Gemini's thinking capabilities, which expose the model's chain-of-thought reasoning process.

## Table of Contents

- [Overview](#overview)
- [Thinking Levels](#thinking-levels)
- [Basic Usage](#basic-usage)
- [Accessing Thoughts](#accessing-thoughts)
- [Thinking Summaries](#thinking-summaries)
- [Streaming with Thinking](#streaming-with-thinking)
- [Cost and Performance](#cost-and-performance)
- [Best Practices](#best-practices)
- [Thought Signatures](#thought-signatures)

## Overview

Thinking mode enables the model to "think out loud" before responding, showing its reasoning process. This is useful for:

- Complex problem solving
- Mathematical calculations
- Multi-step reasoning
- Debugging model behavior
- Understanding how the model reaches conclusions

Under API revision 2026-05-20, thinking appears in `response.steps` as `Step::Thought { signature, summary }`:

- `signature`: an opaque cryptographic signature validating the reasoning (not readable text)
- `summary`: human-readable summary content blocks, populated when thinking summaries are enabled

## Thinking Levels

| Level | Description | Token Cost | Use Case |
|-------|-------------|------------|----------|
| `Minimal` | Minimal reasoning | Low | Quick checks |
| `Low` | Light reasoning | Moderate | Simple problems |
| `Medium` | Balanced reasoning | Higher | Moderate complexity |
| `High` | Extensive reasoning | Highest | Complex problems |

Higher levels produce more detailed reasoning but consume more tokens. To skip thinking entirely, simply omit `with_thinking_level()`.

## Basic Usage

### Enable Thinking

```rust,ignore
use genai_rs::{Client, ThinkingLevel};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Solve step by step: If a train travels 120 miles in 2 hours, what's its speed?")
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;
```

### Check for Thoughts

```rust,ignore
if response.has_thoughts() {
    println!("Model used reasoning!");
}
```

## Accessing Thoughts

> **Note**: Thought steps carry cryptographic signatures for verification, not human-readable reasoning text. Enable [thinking summaries](#thinking-summaries) to get readable summaries. See [Thought Signatures](#thought-signatures) for details.

### Check for Thoughts

```rust,ignore
// Check if model used reasoning
if response.has_thoughts() {
    println!("Model used {} thought steps", response.thought_signatures().count());
}
```

### Get Thought Signatures

```rust,ignore
// Iterate over thought signatures (cryptographic proofs, not readable text)
for signature in response.thought_signatures() {
    // Signatures are for verification/replay, not display
    println!("Thought signature present");
}

// Get final answer
if let Some(text) = response.as_text() {
    println!("Final Answer: {}", text);
}
```

### Step Summary

```rust,ignore
let summary = response.step_summary();
println!("Thought steps: {}", summary.thought_count);
println!("Text blocks: {}", summary.text_count);
```

### Thought Token Usage

```rust,ignore
if let Some(thought_tokens) = response.thought_tokens() {
    println!("Tokens used for reasoning: {}", thought_tokens);
}

// Equivalent, via the usage struct:
if let Some(thought_tokens) = response.usage.as_ref().and_then(|u| u.total_thought_tokens) {
    println!("Tokens used for reasoning: {}", thought_tokens);
}
```

## Thinking Summaries

Request human-readable summaries of the reasoning process:

```rust,ignore
use genai_rs::{ThinkingLevel, ThinkingSummaries};

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Explain photosynthesis step by step")
    .with_thinking_level(ThinkingLevel::High)
    .with_thinking_summaries(ThinkingSummaries::Auto)
    .create()
    .await?;

// Read the summaries (Content blocks inside Step::Thought)
for content in response.thought_summaries() {
    if let Some(text) = content.as_text() {
        println!("Reasoning summary: {}", text);
    }
}
```

### ThinkingSummaries Options

| Option | Behavior |
|--------|----------|
| `Auto` | Include thinking summaries (default when thinking is enabled) |
| `None` | No summary included |

## Streaming with Thinking

Thought summaries and signatures stream before the final response as dedicated `StepDelta` variants:

```rust,ignore
use futures_util::StreamExt;
use genai_rs::{StepDelta, StreamChunk, ThinkingLevel, ThinkingSummaries};

let mut stream = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Solve: What is 15% of 240?")
    .with_thinking_level(ThinkingLevel::Medium)
    .with_thinking_summaries(ThinkingSummaries::Auto)
    .create_stream();

let mut in_thought = false;

while let Some(Ok(event)) = stream.next().await {
    if let StreamChunk::StepDelta { delta, .. } = event.chunk {
        match delta {
            StepDelta::ThoughtSummary { content } => {
                if !in_thought {
                    println!("=== Thinking ===");
                    in_thought = true;
                }
                if let Some(text) = content.as_ref().and_then(|c| c.as_text()) {
                    print!("{}", text);
                }
            }
            StepDelta::ThoughtSignature { .. } => {
                // Opaque signature fragment; keep for replay, nothing to display
            }
            StepDelta::Text { text } => {
                if in_thought {
                    println!("\n=== Response ===");
                    in_thought = false;
                }
                print!("{}", text);
            }
            _ => {}
        }
    }
}
```

## Cost and Performance

### Token Costs

Thinking increases token usage significantly:

| Level | Typical Overhead |
|-------|------------------|
| (none) | Baseline |
| Minimal | +10-20% |
| Low | +20-50% |
| Medium | +50-100% |
| High | +100-300% |

Actual overhead varies based on query complexity.

### When to Use Each Level

```rust,ignore
// Simple factual query - no thinking needed
client.interaction()
    .with_text("What is the capital of France?")
    // No with_thinking_level() - thinking not requested

// Math problem - medium thinking
client.interaction()
    .with_text("Calculate compound interest...")
    .with_thinking_level(ThinkingLevel::Medium)

// Complex reasoning - high thinking
client.interaction()
    .with_text("Analyze this philosophical argument...")
    .with_thinking_level(ThinkingLevel::High)
```

### Monitoring Costs

```rust,ignore
if let Some(usage) = &response.usage {
    println!("Input tokens: {:?}", usage.total_input_tokens);
    println!("Output tokens: {:?}", usage.total_output_tokens);
    println!("Thought tokens: {:?}", usage.total_thought_tokens);

    if let Some(total) = usage.total_tokens {
        println!("Total tokens: {}", total);
    }
}
```

## Best Practices

### 1. Match Level to Task Complexity

```rust,ignore
// DON'T: Use high thinking for simple queries
let response = client.interaction()
    .with_text("What color is the sky?")
    .with_thinking_level(ThinkingLevel::High)  // Wasteful!
    .create().await?;

// DO: Use appropriate level
let response = client.interaction()
    .with_text("What color is the sky?")
    // No thinking needed for simple facts
    .create().await?;
```

### 2. Request Thinking for Problem-Solving Prompts

```rust,ignore
// Good prompts for thinking mode
let prompts = [
    "Solve step by step: ...",
    "Analyze this code for bugs: ...",
    "Compare and contrast: ...",
    "Explain your reasoning: ...",
    "Debug this issue: ...",
];
```

### 3. Handle Missing Thoughts Gracefully

```rust,ignore
// Check for thought presence (signatures are cryptographic, not readable)
if response.has_thoughts() {
    let sig_count = response.thought_signatures().count();
    println!("Model used {} thought steps for reasoning", sig_count);
} else {
    println!("No thought steps in response");
}
```

### 4. Use for Debugging Model Behavior

```rust,ignore
// Enable thinking + summaries to understand why the model gave an unexpected answer
let response = client.interaction()
    .with_text(&problematic_prompt)
    .with_thinking_level(ThinkingLevel::High)
    .with_thinking_summaries(ThinkingSummaries::Auto)
    .create().await?;

for content in response.thought_summaries() {
    if let Some(text) = content.as_text() {
        println!("DEBUG - Reasoning: {}", text);
    }
}

if let Some(text) = response.as_text() {
    println!("DEBUG - Model response: {}", text);
}
```

### 5. Combine with Function Calling

Thinking works with function calling to show reasoning about tool use:

```rust,ignore
let response = client.interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's the weather in Tokyo and should I bring an umbrella?")
    .with_thinking_level(ThinkingLevel::Medium)
    .add_function(get_weather.declaration())
    .create_with_auto_functions()
    .await?;

// Thoughts may include reasoning about:
// - Whether to call the function
// - How to interpret function results
// - What recommendation to make
```

## Thought Signatures

Thought signatures provide cryptographic verification of model reasoning. See [Google's documentation](https://ai.google.dev/gemini-api/docs/thought-signatures.md.txt) for details.

Signatures live on `Step::Thought { signature, .. }`; server-tool call/result steps (`Step::CodeExecutionCall`, `Step::GoogleSearchCall`, ...) also carry an optional `signature` field.

### Replaying Signatures in Stateless Multi-Turn

When managing conversation history yourself (stateless mode), include the model's output steps unchanged in the next request so thought signatures are replayed. `response.output_steps()` returns the steps ready for `with_history()`:

```rust,ignore
// Turn 1
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("Think about the fastest route from A to B")
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;

// Build history: prior turns + this response's steps (thought signatures included)
let mut history = vec![genai_rs::Step::user_text("Think about the fastest route from A to B")];
history.extend(response.output_steps());

// Turn 2 - signatures replay automatically via the history
let followup = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .with_text("Now assume road B2 is closed")
    .create()
    .await?;
```

### Streaming Signatures

```rust,ignore
use genai_rs::{StepDelta, StreamChunk};

// Thought signatures arrive as dedicated deltas in streaming
if let StreamChunk::StepDelta { delta, .. } = event.chunk {
    if let StepDelta::ThoughtSignature { signature } = delta {
        // Opaque fragment; the accumulated final response carries the full signature
    }
}
```

## Example

See `cargo run --example thinking` for a complete working example.

```bash
GEMINI_API_KEY=your-key cargo run --example thinking
```
