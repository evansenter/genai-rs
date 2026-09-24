# Conversations

A multi-turn conversation keeps its context in one of two places:

| Approach | Where history lives | How | Needs storage |
|----------|--------------------|-----|---------------|
| **Stateful** | On the server | `with_previous_interaction(id)` | Yes (the default) |
| **Stateless** | In your code, as `Vec<Step>` | `with_history(steps)`, or `conversation()...done()` inline | No |

Choose stateless when you can't store interactions server-side, persist
conversations yourself, or need to edit history (trim, summarize, branch).
Choose stateful otherwise. It is also the only mode that supports
`create_with_auto_functions()` and `with_background(true)`, since both need
storage.

For function calls inside a conversation, see
[Function Calling](FUNCTION_CALLING.md). This page covers the conversation
state around them.

## Stateful: chaining by interaction id

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let first = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("My name is Alice")
    .create()
    .await?;

let second = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_previous_interaction(first.id.as_ref().expect("stored interactions have ids"))
    .with_text("What's my name?")
    .create()
    .await?;
# let _ = second;
# Ok(())
# }
```

Storage is on by default. `with_store_disabled()` combined with
`with_previous_interaction()` is rejected at build time.

## What carries over between turns

With `previous_interaction_id`, the server supplies the earlier turns. It
does **not** carry over the request configuration:

| Field | Carried over? | Evidence |
|-------|--------------|----------|
| Conversation history | ✅ Yes | The model answers from earlier turns (`test_multiturn_tools_not_inherited` checks both halves) |
| `system_instruction` | ❌ No | A follow-up that omits it is stored with no system instruction (`test_system_instruction_not_inherited`) |
| `tools` | ❌ No | With tools omitted, the model makes no calls (`test_multiturn_tools_not_inherited`) |

So set `with_system_instruction()` and the tools on **every** turn that should
have them. `model` (or `agent`) is required on every request anyway.

Two caveats:

- The model can still *appear* to follow an earlier system instruction,
  because the replayed turns (including its thoughts) may restate it. Don't
  treat that as inheritance.
- A turn that only sends function results can omit the tools. The API
  accepts it (`test_parallel_function_calls`). Resending them is also fine,
  and it's what the auto-function loop does: it reuses the whole request,
  system instruction and tools included, every round.

```rust,no_run
# async fn example(client: &genai_rs::Client, previous_id: &str, tools: Vec<genai_rs::FunctionDeclaration>) -> Result<(), genai_rs::GenaiError> {
const SYSTEM: &str = "You are a concise assistant.";

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_previous_interaction(previous_id)
    .with_system_instruction(SYSTEM) // not inherited: send it again
    .add_functions(tools)            // not inherited: send them again
    .with_text("And tomorrow?")
    .create()
    .await?;
# let _ = response;
# Ok(())
# }
```

## Stateless: history as steps

History is a `Vec<Step>`. Build user turns with `Step::user_text()` or
`Step::user_input(contents)`, and append the model's turn with
`response.output_steps()`:

```rust,no_run
use genai_rs::{Client, Step};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
# let client = Client::new("api-key".to_string());
let mut history = vec![Step::user_text("Hi, I'm Alice")];

let response = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_history(history.clone())
    .with_system_instruction("You are a helpful assistant")
    .with_store_disabled()
    .create()
    .await?;

// Replay the model's steps (text, thoughts, tool calls) verbatim
history.extend(response.output_steps());
history.push(Step::user_text("What's my name?"));

let response = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_history(history.clone())
    .with_system_instruction("You are a helpful assistant")
    .with_store_disabled()
    .create()
    .await?;
# let _ = response;
# Ok(())
# }
```

`with_text()` composes with `with_history()`: the text is appended as a final
user step. `with_content()` does not; for multimodal history, use
`Step::user_input(vec![Content::text(..), Content::image_data(..)])`.

### Replay model output verbatim

`output_steps()` returns every step the model produced, and each carries the
fields the API checks on replay:

- `Step::Thought { signature, .. }` carries the thought signature.
- `Step::FunctionCall { signature, .. }` carries its own signature, which the
  API **requires** when a function call is replayed statelessly (verified live
  2026-07). This is why stateless function calling should extend history
  with `output_steps()`, not rebuild calls with `Step::function_call()`.

Replaying thought signatures is accepted (`test_stateless_with_thinking_function_calling`).
These signatures are in different places from the ones the
[`generateContent` thought-signature guide](https://ai.google.dev/gemini-api/docs/thought-signatures.md.txt)
describes; this crate follows what the Interactions API returns.

### Inline conversations: `conversation()`

For fixtures and hand-written context, `ConversationBuilder` produces the same
`user_input` / `model_output` steps. `.user()` accepts text or `Vec<Content>`:

```rust,no_run
# use genai_rs::Client;
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_system_instruction("You are a helpful math tutor")
    .conversation()
    .user("What is 2+2?")
    .model("2+2 equals 4.")
    .user("And what's that times 3?")
    .done()
    .create()
    .await?;
# let _ = response;
# Ok(())
# }
```

## Managing history

### A chat loop

```rust,no_run
# use genai_rs::{Client, Step};
# fn get_user_input() -> Result<String, genai_rs::GenaiError> { Ok("quit".to_string()) }
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let mut history: Vec<Step> = Vec::new();

loop {
    let user_input = get_user_input()?;
    if user_input == "quit" {
        break;
    }
    history.push(Step::user_text(user_input));

    let response = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_history(history.clone())
        .with_store_disabled()
        .create()
        .await?;

    println!("Model: {}", response.as_text().unwrap_or("No response"));
    history.extend(response.output_steps());
}
# Ok(())
# }
```

### Trimming

Send fewer steps to bound token cost. Cut at a user-turn boundary, so each
`function_call` step stays with its `function_result`:

```rust
# use genai_rs::Step;
const MAX_STEPS: usize = 10;

fn trim(history: &mut Vec<Step>) {
    if history.len() <= MAX_STEPS {
        return;
    }
    let mut cut = history.len() - MAX_STEPS;
    // Advance to the next user turn so no call/result pair is split
    while cut < history.len() && !matches!(history[cut], Step::UserInput { .. }) {
        cut += 1;
    }
    history.drain(..cut);
}
# let mut h = vec![Step::user_text("hi")];
# trim(&mut h);
```

### Summarizing old turns

```rust,no_run
# use genai_rs::{Client, GenaiError, Step};
async fn summarize_and_trim(
    client: &Client,
    history: &mut Vec<Step>,
    keep: usize,
) -> Result<(), GenaiError> {
    if history.len() <= keep {
        return Ok(());
    }
    let old: Vec<Step> = history.drain(..history.len() - keep).collect();
    let transcript = old
        .iter()
        .map(|step| format!("{}: {}", step.step_type(), step.as_text().unwrap_or("[non-text step]")))
        .collect::<Vec<_>>()
        .join("\n");

    let summary = client
        .interaction()
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_text(format!("Summarize this conversation in 2-3 sentences:\n{transcript}"))
        .with_store_disabled()
        .create()
        .await?;

    history.insert(0, Step::user_text("Summary of the conversation so far:"));
    history.insert(1, Step::model_text(summary.as_text().unwrap_or("...")));
    Ok(())
}
```

### Branching

Clone the history to explore alternatives from a shared prefix:

```rust
# use genai_rs::Step;
let base = vec![
    Step::user_text("I want to build web applications"),
    Step::model_text("Great! Which language are you considering?"),
];

let mut rust_branch = base.clone();
rust_branch.push(Step::user_text("Tell me about Rust for web development"));

let mut ts_branch = base.clone();
ts_branch.push(Step::user_text("Tell me about TypeScript for web development"));
```

## Wire format

`with_history()`, `conversation()` and `with_content()` all send `input` as
an array of steps tagged by `type`:

```json
{
  "model": "gemini-3.8-flash",
  "input": [
    { "type": "user_input", "content": [{ "type": "text", "text": "Hello" }] },
    { "type": "model_output", "content": [{ "type": "text", "text": "Hi!" }] },
    { "type": "user_input", "content": [{ "type": "text", "text": "How are you?" }] }
  ]
}
```

`with_text()` alone sends `input` as a plain string.

## Examples

| Example | Shows |
|---------|-------|
| `stateful_interaction` | Chaining with `previous_interaction_id` |
| `explicit_turns` | Stateless history and `conversation()` |
| `system_instructions` | System instructions across turns |
| `multi_turn_agent_auto` (`examples/real_world/`) | Stateful agent with the auto-function loop |
| `multi_turn_agent_manual_stateless` (`examples/real_world/`) | Stateless agent with a manual function loop |

```bash
cargo run --example explicit_turns
```
