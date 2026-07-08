# Conversation Patterns Guide

This guide covers patterns for multi-turn conversations, including stateless approaches using Step arrays and the ConversationBuilder.

## Table of Contents

- [Overview](#overview)
- [Stateful vs Stateless](#stateful-vs-stateless)
- [ConversationBuilder](#conversationbuilder)
- [Step Arrays](#step-arrays)
- [Dynamic History Management](#dynamic-history-management)
- [Advanced Patterns](#advanced-patterns)
- [Choosing an Approach](#choosing-an-approach)

## Overview

`genai-rs` supports three approaches to multi-turn conversations:

| Approach | State Storage | Best For |
|----------|--------------|----------|
| **Stateful** (`previous_interaction_id`) | Server-side | Simple apps, persistent context |
| **ConversationBuilder** | Client-side | Inline conversation construction |
| **Step Arrays** (`with_history()`) | Client-side | External history, custom management |

## Stateful vs Stateless

### Stateful (Server Storage)

The server maintains conversation history:

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
// Turn 1: Start conversation
let response1 = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("My name is Alice")
    .with_store_enabled()
    .create()
    .await?;

// Turn 2: Server remembers context
let response2 = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_text("What's my name?")
    .with_previous_interaction(response1.id.as_ref().unwrap())
    .with_store_enabled()
    .create()
    .await?;

// Model responds: "Your name is Alice"
# Ok(())
# }
```

**Pros**: Simple, no history management needed
**Cons**: Requires storage, less control over context

### Stateless (Client History)

You manage conversation history as a `Vec<Step>`:

```rust,no_run
# use genai_rs::{Client, Step};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let history = vec![
    Step::user_text("My name is Alice"),
    Step::model_text("Nice to meet you, Alice!"),
    Step::user_text("What's my name?"),
];

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .create()
    .await?;
# Ok(())
# }
```

**Pros**: Full control, no server storage needed, portable
**Cons**: Must manage history, larger requests

## ConversationBuilder

Fluent API for inline conversation construction. `.user()` and `.model()` produce `user_input`/`model_output` `Step`s under the hood.

### Basic Usage

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .conversation()
    .user("What is 2+2?")
    .model("2+2 equals 4.")
    .user("And what's that times 3?")
    .done()
    .create()
    .await?;

// Model responds about 4 * 3 = 12
# Ok(())
# }
```

### With System Instructions

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_system_instruction("You are a helpful math tutor")
    .conversation()
    .user("I need help with fractions")
    .model("I'd be happy to help! What would you like to know?")
    .user("How do I add 1/2 + 1/4?")
    .done()
    .create()
    .await?;
# Ok(())
# }
```

### With Multimodal Content

For multimodal history, build a `user_input` step from content blocks with `Step::user_input()`:

```rust,no_run
# use genai_rs::{Client, Content, Step};
# async fn example(client: &Client, base64_image: String) -> Result<(), genai_rs::GenaiError> {
// Build a multimodal user step
let multimodal_step = Step::user_input(vec![
    Content::text("What's in this image?"),
    Content::image_data(base64_image, "image/png"),
]);

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(vec![multimodal_step])
    .create()
    .await?;
# Ok(())
# }
```

## Step Arrays

Direct array of `Step` objects for external history management.

### Creating Steps

```rust,no_run
# use genai_rs::{Client, Step};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
// Simple text steps
let user_step = Step::user_text("Hello!");
let model_step = Step::model_text("Hi there! How can I help?");

// Build history
let history = vec![
    Step::user_text("I'm planning a trip to Tokyo"),
    Step::model_text("Tokyo is wonderful! What aspects interest you?"),
    Step::user_text("I love food and temples"),
    Step::model_text("Great choices! For food, try Tsukiji for sushi..."),
    Step::user_text("What's one must-see temple?"),
];

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .create()
    .await?;
# Ok(())
# }
```

### From External Sources

```rust,ignore
// Load from database
let db_history = load_conversation_from_db(conversation_id)?;

let history: Vec<Step> = db_history
    .iter()
    .map(|msg| {
        if msg.is_user {
            Step::user_text(&msg.content)
        } else {
            Step::model_text(&msg.content)
        }
    })
    .collect();

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .create()
    .await?;
```

## Dynamic History Management

Build history incrementally during a conversation.

### Chat Loop Pattern

```rust,no_run
# use genai_rs::{Client, Step};
# fn get_user_input() -> Result<String, genai_rs::GenaiError> { Ok("quit".to_string()) }
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let mut history: Vec<Step> = Vec::new();

loop {
    // Get user input
    let user_input = get_user_input()?;
    if user_input == "quit" {
        break;
    }

    // Add user message to history
    history.push(Step::user_text(user_input));

    // Send full history
    let response = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_history(history.clone())
        .create()
        .await?;

    println!("Model: {}", response.as_text().unwrap_or("No response"));

    // Add ALL output steps to history. output_steps() returns thoughts,
    // function calls, and model output, so thought signatures and tool
    // state are replayed on the next turn.
    history.extend(response.output_steps());
}
# Ok(())
# }
```

### Sliding Window

Limit context to recent steps to manage token costs:

```rust
# use genai_rs::Step;
const MAX_STEPS: usize = 10;

fn add_to_history(history: &mut Vec<Step>, step: Step) {
    history.push(step);

    // Keep only recent steps
    if history.len() > MAX_STEPS {
        history.drain(0..history.len() - MAX_STEPS);
    }
}
```

### With Summarization

Summarize old context to preserve information while reducing tokens:

```rust,no_run
# use genai_rs::{Client, GenaiError, Step};
async fn summarize_and_trim(
    client: &Client,
    history: &mut Vec<Step>,
    max_steps: usize,
) -> Result<(), GenaiError> {
    if history.len() <= max_steps {
        return Ok(());
    }

    // Extract old steps to summarize
    let old_steps: Vec<_> = history.drain(0..history.len() - max_steps + 1).collect();

    // Generate summary
    let summary_prompt = format!(
        "Summarize this conversation in 2-3 sentences:\n{}",
        old_steps
            .iter()
            .map(|step| format!(
                "{}: {}",
                step.step_type(),
                step.as_text().unwrap_or("[non-text step]")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let summary = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text(&summary_prompt)
        .create()
        .await?;

    // Insert summary as context at the beginning
    history.insert(0, Step::user_text("Previous conversation summary:"));
    history.insert(1, Step::model_text(summary.as_text().unwrap_or("...")));

    Ok(())
}
```

## Advanced Patterns

### Combining with System Instructions

System instructions are not inherited across turns by the API — set them explicitly per request:

```rust,no_run
# use genai_rs::{Client, Step};
# async fn example(client: &Client, history: Vec<Step>) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_system_instruction("You are a Python expert. Always provide code examples.")
    .with_history(history)
    .create()
    .await?;
# Ok(())
# }
```

### With Function Calling

Step arrays work with all features:

```rust,ignore
use genai_rs_macros::tool;

#[tool(description = "Get current weather")]
fn get_weather(city: String) -> String {
    format!("Weather in {}: Sunny, 72°F", city)
}

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .add_function(get_weather.declaration())
    .create_with_auto_functions()
    .await?;
```

### With Streaming

```rust,no_run
# use genai_rs::{Client, Step};
# use futures_util::StreamExt;
# async fn example(client: &Client, history: Vec<Step>) -> Result<(), genai_rs::GenaiError> {
let mut stream = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_history(history)
    .create_stream();

while let Some(result) = stream.next().await {
    // Process stream events
}
# Ok(())
# }
```

### Branching Conversations

Create conversation branches by cloning history:

```rust
# use genai_rs::Step;
let base_history = vec![
    Step::user_text("I want to learn a programming language"),
    Step::model_text("Great! What's your goal?"),
    Step::user_text("I want to build web applications"),
];

// Branch 1: Explore Rust
let mut rust_branch = base_history.clone();
rust_branch.push(Step::user_text("Tell me about Rust for web development"));

// Branch 2: Explore TypeScript
let mut ts_branch = base_history.clone();
ts_branch.push(Step::user_text("Tell me about TypeScript for web development"));

// Both branches maintain the same context up to the branching point
```

## Choosing an Approach

| Scenario | Recommended Approach |
|----------|---------------------|
| Simple chatbot | Stateful (`previous_interaction_id`) |
| Serverless/Lambda | Stateless (Step arrays) |
| Custom history storage | Step arrays with `with_history()` |
| Inline test conversations | ConversationBuilder |
| Migration from other APIs | Step arrays (convert existing format) |
| Context window management | Step arrays with sliding window |
| Conversation branching | Step arrays (clone and modify) |

### Decision Tree

```text
Need persistent server storage?
├── Yes → Use stateful with previous_interaction_id
└── No
    ├── Building conversation inline?
    │   └── Yes → Use ConversationBuilder
    └── Managing external history?
        └── Yes → Use with_history()
```

## Wire Format

Both ConversationBuilder and `with_history()` produce the same wire format — an array of steps tagged by `type`:

```json
{
  "model": "gemini-3-flash-preview",
  "input": [
    { "type": "user_input", "content": [{ "type": "text", "text": "Hello" }] },
    { "type": "model_output", "content": [{ "type": "text", "text": "Hi!" }] },
    { "type": "user_input", "content": [{ "type": "text", "text": "How are you?" }] }
  ]
}
```

## Example

See `cargo run --example explicit_turns` for a complete working example.

```bash
GEMINI_API_KEY=your-key cargo run --example explicit_turns
```
