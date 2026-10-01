# InteractionBuilder API Guide

`client.interaction()` returns an `InteractionBuilder`. Its methods can be
chained in any order. The request is validated and assembled when you call
`create()`, `create_stream()`, `create_with_auto_functions()`,
`create_stream_with_auto_functions()` or `build()`.

This page covers the conventions, how inputs compose, and the validation
rules. For every method and its arguments, see the
[API reference](https://docs.rs/genai-rs/latest/genai_rs/struct.InteractionBuilder.html).

## Method naming conventions

| Prefix | Behavior | Example |
|--------|----------|---------|
| `with_*` | **Configures** a setting (replaces if called twice) | `with_model()`, `with_text()`, `with_content()` |
| `add_*` | **Accumulates** items to a collection | `add_function()`, `add_tool()` |

The option-free built-in tool setters (`with_google_search()`,
`with_google_maps()`, `with_code_execution()`, `with_url_context()`) follow the
`with_*` rule: each replaces any earlier tool of its kind, so calling one
twice sends it once. `with_tools(Vec<Tool>)` replaces the whole list.

Tools with options take a config struct through `add_tool()`:
`GoogleSearchConfig`, `GoogleMapsConfig`, `McpServerConfig`,
`ComputerUseConfig`, `FileSearchConfig` and `RetrievalConfig`. See
[Built-in Tools](BUILT_IN_TOOLS.md).

The other builders follow the same convention, e.g.
`FunctionDeclaration::builder(name).with_description(..).add_parameter(..).with_required(..)`
and `ComputerUseConfig::new().with_excluded_predefined_functions(..)`.

## Input methods

| Method | Purpose | Composes with |
|--------|---------|---------------|
| `with_text(str)` | A text message | `with_history()`, `with_content()` |
| `with_history(Vec<Step>)` | Conversation history as steps | `with_text()` |
| `with_content(Vec<Content>)` | Multimodal content for one turn | `with_text()` |
| `conversation()...done()` | Fluent history builder (`.user()` / `.model()` steps) | — |
| `with_input(InteractionInput)` | Maps an `InteractionInput` onto the three methods above | — |

### How inputs compose at build time

```text
with_content() set?
├── Yes ── with_history() also set? ── Yes → ERROR (InvalidInput)
│                                     No  → Content([text?, ...content])   (text prepended if set)
└── No ─── with_history() set?
           ├── Yes → Steps([...history, user_text(text)?])                 (text appended if set)
           └── No  → with_text() set?  Yes → Text(text)
                                       No  → ERROR ("Input is required for interaction")
```

A `Content` input is sent on the wire as a single `user_input` step.

For multimodal content *and* history, put the content in a step yourself:

```rust,no_run
use genai_rs::{Content, Step};

# async fn run(client: genai_rs::Client, mut history: Vec<Step>, base64_png: String) -> Result<(), genai_rs::GenaiError> {
history.push(Step::user_input(vec![
    Content::text("What's in this image?"),
    Content::image_data(base64_png, "image/png"),
]));

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_history(history)
    .create()
    .await?;
# let _ = response;
# Ok(())
# }
```

## Validation errors

Each of these returns `GenaiError::InvalidInput` from `build()` (and so from
every `create*()`), before any request is sent:

| Combination | Why |
|-------------|-----|
| Neither or both of `with_model()` and `with_agent()` | Exactly one target is required |
| `with_agent_config()` without `with_agent()` | Agent config only applies to agents |
| `with_content()` together with `with_history()` | Wrap the content in `Step::user_input` instead (above) |
| No input at all | Nothing to send |
| `with_store_disabled()` + `with_previous_interaction()` | Chaining needs the previous interaction stored |
| `with_store_disabled()` + `with_background(true)` | Background results are fetched by id, so they must be stored |
| `with_store_disabled()` + `create_with_auto_functions()` | The loop chains rounds by `previous_interaction_id` |

Storage is on by default (the API's default), so `with_store_enabled()` is
only needed for clarity.

Because validation happens at build time, conditional chaining is safe:

```rust,no_run
# async fn run(client: genai_rs::Client, previous_id: Option<String>, privacy_mode: bool) -> Result<(), genai_rs::GenaiError> {
let mut builder = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Hello");

if let Some(id) = &previous_id {
    builder = builder.with_previous_interaction(id);
} else if privacy_mode {
    // Only valid when not chaining
    builder = builder.with_store_disabled();
}

let response = builder.create().await?;
# let _ = response;
# Ok(())
# }
```

## Building without sending

`build()` returns an `InteractionRequest` (`Clone + Serialize`) that
`client.execute()` sends. That makes it the basis for retries (see
[Reliability](RELIABILITY.md)), and handy for inspecting the exact JSON:

```rust,no_run
# fn run(client: genai_rs::Client) -> Result<(), Box<dyn std::error::Error>> {
let request = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Hello")
    .build()?;

println!("{}", serde_json::to_string_pretty(&request)?);
# Ok(())
# }
```

## Retrieving stored interactions

| Method | Behavior |
|--------|----------|
| `client.interactions().get(id)` | Fetches the interaction; the response's `input` is `None` |
| `client.interactions().get_with_input(id)` | Fetches with `include_input=true`, so `input` is populated |
| `client.interactions().stream(id)` | Streams a background interaction from the start |
| `client.interactions().resume_stream(id, last_event_id)` | Resumes its stream after an event id (see [Streaming API](STREAMING_API.md#stream-resume)) |
| `client.interactions().cancel(id)` | Cancels a background interaction that is still in progress |
| `client.interactions().delete(id)` | Deletes the stored interaction |

## Resource handles and list builders

A `/v1beta` resource is reached through a handle: `client.agents()` returns a
`Copy` value that borrows the client, and its methods are the resource's
verbs. [Resources](RESOURCES.md) lists every handle and the `Client` method
each one replaced.

| Shape | Rule |
|-------|------|
| `client.agents().get(id)` | Required arguments are positional. The future holds only the client borrow, so it can be stored or joined with others |
| `client.agents().list()` | Returns a list builder. `with_page_size()`, `with_page_token()` and the list's own filters (`with_parent()`) configure it |
| `.send()` | Sends one request and returns one page (`AgentListResponse`) |
| `.pages()` | Streams every page |
| `.items()` | Streams every item across pages, in server order |

The streams need `futures_util::{StreamExt, TryStreamExt}` and follow these
rules:

- Nothing is sent until the stream is first polled.
- The page size and filters are sent with every page; only the token changes.
- The stream ends after a page whose `next_page_token` is absent or empty. An
  empty page that has a token is followed.
- If the server returns a token it already returned (or the starting token),
  the stream yields that page, then `GenaiError::MalformedResponse`, then ends.
- An error is yielded once and ends the stream. Nothing is retried.
- A stream owns a clone of the client, so it can be stored or spawned.

```rust,no_run
use futures_util::{StreamExt, TryStreamExt};
use genai_rs::Agent;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
// One page
let page = client.agents().list().with_page_size(50).send().await?;
if let Some(token) = page.next_page_token {
    // Resume from where that page left off
    let _rest = client.agents().list().with_page_token(token).send().await?;
}

// Every agent, across pages
let agents: Vec<Agent> = client.agents().list().items().try_collect().await?;

// Page by page, stopping early
let mut pages = client.agents().list().with_page_size(10).pages();
while let Some(page) = pages.next().await {
    if page?.agents.iter().any(|a| a.id.as_deref() == Some("customer-sentinel")) {
        break;
    }
}

// On another task
let listing = tokio::spawn(client.agents().list().items().try_collect::<Vec<Agent>>());
let spawned = listing.await.expect("list task panicked")?;
# let _ = (agents, spawned);
# Ok(())
# }
```

## Related

- [Function Calling](FUNCTION_CALLING.md): `add_function()`, `with_tool_service()`, the auto-function loop
- [Multimodal](MULTIMODAL.md): `Content` constructors
- [Configuration](CONFIGURATION.md): generation settings
