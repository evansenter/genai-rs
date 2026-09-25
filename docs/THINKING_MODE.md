# Thinking Mode Guide

Gemini models can reason before they answer. Under the 2026-05-20 API
revision, that reasoning appears in `response.steps` as
`Step::Thought { signature, summary }`:

- `signature`: an opaque signature over the reasoning. It is not readable
  text; pass it back unchanged when replaying history.
- `summary`: human-readable summary content, populated only when you ask for
  [thinking summaries](#thinking-summaries).

## Thinking levels

`with_thinking_level(ThinkingLevel::…)` sets `generation_config.thinking_level`.

| Level | Wire value | On `DEFAULT_MODEL` |
|-------|-----------|--------------------|
| `Minimal` | `"minimal"` | **Rejected**: `'minimal' is not a supported thinking level for this model. Allowed values are: high, low, medium.` Use `genai_rs::MINIMAL_THINKING_MODEL`, a model that accepts it |
| `Low` | `"low"` | Accepted; the cheapest level this model allows |
| `Medium` | `"medium"` | Accepted |
| `High` | `"high"` | Accepted |

**Omitting `with_thinking_level()` does not turn thinking off.** It sends no
level, and the model's default applies. `DEFAULT_MODEL` (`gemini-3.8-flash`)
still thinks. One live check, 2026-09-24, on a short arithmetic puzzle:

| Level sent | Thought tokens |
|------------|----------------|
| *(none)* | 103, then 222 on a second run |
| `low` | 56 |
| `medium` | 142 |
| `high` | 260 |

Counts vary run to run and with the prompt. Treat these as the shape of the
behavior, not a price list. To keep reasoning cost down, set
`ThinkingLevel::Low` explicitly, and read the actual cost from
`response.thought_tokens()`.

## Basic usage

```rust,no_run
use genai_rs::ThinkingLevel;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("If a train travels 120 miles in 2 hours, what's its speed?")
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;

println!("answer: {}", response.as_text().unwrap_or_default());
println!("thought tokens: {:?}", response.thought_tokens());
println!("thought steps: {}", response.step_summary().thought_count);
# Ok(())
# }
```

Helpers on `InteractionResponse`:

| Method | Returns |
|--------|---------|
| `has_thoughts()` | `true` if any `Thought` step is present (a step can be present even when `thought_tokens()` is 0) |
| `thought_signatures()` | Iterator over the populated signatures (`&str`) |
| `thought_summaries()` | Iterator over summary `Content` blocks |
| `thought_tokens()` | `Option<u32>`, the same as `usage.total_thought_tokens` |
| `step_summary().thought_count` | Number of `Thought` steps |

## Thinking summaries

Summaries are returned only when requested:

| `with_thinking_summaries(...)` | Result (live check, 2026-09-24) |
|-------------------------------|------------------------------|
| `ThinkingSummaries::Auto` | `Thought` steps carry a `summary` |
| `ThinkingSummaries::None`, or not set | No summary |

```rust,no_run
use genai_rs::{ThinkingLevel, ThinkingSummaries};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Explain photosynthesis step by step")
    .with_thinking_level(ThinkingLevel::High)
    .with_thinking_summaries(ThinkingSummaries::Auto)
    .create()
    .await?;

for content in response.thought_summaries() {
    if let Some(text) = content.as_text() {
        println!("reasoning summary: {text}");
    }
}
# Ok(())
# }
```

On the wire both `generation_config.thinking_summaries` and the Deep Research
`agent_config.thinking_summaries` take `"auto"` / `"none"`.

## Streaming with thinking

Summaries and signatures stream as their own `StepDelta` variants, before the
answer text:

```rust,no_run
use futures_util::StreamExt;
use genai_rs::{StepDelta, StreamChunk, ThinkingLevel, ThinkingSummaries};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let mut stream = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Solve: What is 15% of 240?")
    .with_thinking_level(ThinkingLevel::Medium)
    .with_thinking_summaries(ThinkingSummaries::Auto)
    .create_stream();

while let Some(event) = stream.next().await {
    if let StreamChunk::StepDelta { delta, .. } = event?.chunk {
        match delta {
            StepDelta::ThoughtSummary { content: Some(c) } => {
                if let Some(text) = c.as_text() {
                    eprint!("[thinking] {text}");
                }
            }
            StepDelta::ThoughtSignature { .. } => {} // opaque; the Completed response carries it
            StepDelta::Text { text } => print!("{text}"),
            _ => {}
        }
    }
}
# Ok(())
# }
```

## Thought signatures

Signatures live on `Step::Thought { signature, .. }`. `Step::FunctionCall`
steps also carry a `signature`, which the API **requires** when a function
call is replayed in stateless history (verified live 2026-07), and server-tool
call and result steps carry an optional one.

When you manage history yourself, replay the model's steps unchanged.
`response.output_steps()` returns them ready for `with_history()`, signatures
included:

```rust,no_run
use genai_rs::{Step, ThinkingLevel};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
let first = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Think about the fastest route from A to B")
    .with_thinking_level(ThinkingLevel::Medium)
    .with_store_disabled()
    .create()
    .await?;

let mut history = vec![Step::user_text("Think about the fastest route from A to B")];
history.extend(first.output_steps());

let followup = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_history(history)
    .with_text("Now assume road B2 is closed")
    .with_store_disabled()
    .create()
    .await?;
# let _ = followup;
# Ok(())
# }
```

See Google's [thought signatures guide](https://ai.google.dev/gemini-api/docs/thought-signatures.md.txt).
It describes `generateContent`, where the field sits in a different place;
this crate follows what the Interactions API actually returns.

## Example

```bash
GEMINI_API_KEY=your-key cargo run --example thinking
```
