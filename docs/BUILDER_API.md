# InteractionBuilder API Guide

`client.interaction()` returns an `InteractionBuilder`. Its methods can be
chained in any order. The request is validated and assembled when you call
`create()`, `create_stream()`, `create_with_auto_functions()`,
`create_stream_with_auto_functions()` or `build()`.

This page covers the conventions, how inputs compose, and the validation
rules. For every method and its arguments, see the
[API reference](https://docs.rs/genai-rs/latest/genai_rs/request_builder/struct.InteractionBuilder.html).

## Method naming conventions

| Prefix | Behavior | Example |
|--------|----------|---------|
| `with_*` | **Configures** a setting (replaces if called twice) | `with_model()`, `with_text()`, `with_content()` |
| `add_*` | **Accumulates** items to a collection | `add_function()`, `add_tool()` |

`add_label(key, value)` and `add_extra(key, value)` add one entry to a keyed
map: entries added earlier are kept, and a repeated key replaces its value.

Types follow the same pattern across resources: a list page is
`<Resource>ListResponse` (`FileListResponse`), a create body is
`Create<Resource>Request` (`CreateTriggerRequest`), and every collection
accessor on `InteractionResponse` (`function_calls()`, `images()`,
`all_annotations()`, ...) returns a `Vec` (D-028).

`continue_decode(&response)` sets several settings at once from a response
whose decode stopped early: its model, its id as the previous interaction,
and its continuation token (see
[Decoding continuation](CONFIGURATION.md#decoding-continuation)).

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
| `with_history(Vec<Step>)` | Conversation history as steps | `with_text()`, `with_content()` |
| `with_content(Vec<Content>)` | Multimodal content for one turn | `with_text()`, `with_history()` |
| `conversation()...done()` | Fluent history builder (`.user()` / `.model()` steps) | — |
| `with_input(InteractionInput)` | Maps an `InteractionInput` onto the three methods above | — |

### How inputs compose at build time

```text
with_content() set?
├── Yes → this turn = [text?, ...content]           (text prepended if set; empty → ERROR)
│         with_history() also set?  Yes → Steps([...history, user_input(this turn)])
│                                   No  → Content(this turn)
└── No ─── with_history() set?
           ├── Yes → Steps([...history, user_text(text)?])                 (text appended if set)
           └── No  → with_text() set?  Yes → Text(text)
                                       No  → no input: only a continuation
                                             (continue_decode()) may send none,
                                             else ERROR ("Input is required")
```

A `Content` input is sent on the wire as a single `user_input` step, so
content after history is the same request as a history whose last step is
`Step::user_input(content)`, which the API accepts (D-031):

```rust,no_run
use genai_rs::{Content, Step};

# async fn run(client: genai_rs::Client, history: Vec<Step>, base64_png: String) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_history(history)
    .with_content(vec![
        Content::text("What's in this image?"),
        Content::image_data(base64_png, "image/png"),
    ])
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
| `with_content(vec![])` with no `with_text()` | An empty turn; the API would refuse it too (`400 Request has empty input.`) |
| No input at all, outside a continuation | Nothing to send; without input the API answers a misleading `404 Model ... not found` |
| A continuation token with no input and no `with_previous_interaction()` | A continuation without input continues a stored interaction by its id (a `store: false` one resends its input) |
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
| `client.agents().list()` | Returns a list builder. `with_page_size()`, `with_page_token()` and the list's own options (`with_parent()`) configure it |
| `.send()` | Sends one request and returns one page (`AgentListResponse`) |
| `.pages()` | Streams every page |
| `.items()` | Streams every item across pages, in server order |

A handle struct carries
`#[must_use = "a resource handle does nothing until you call one of its methods"]`,
word for word: `tests/ci_coverage.rs` finds handles by it and requires a
live test of every public method on each (`docs/TESTING.md`, "Coverage
oracle"). It fails when a `Client` accessor, or a handle method returning
anything but a list builder, returns a struct without the marker.

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
