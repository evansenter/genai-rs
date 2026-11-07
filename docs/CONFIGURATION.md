# Configuration Guide

Configuration happens at three levels:

| Level | Configured via | Affects |
|-------|---------------|---------|
| **Client** | `Client::builder()` | Every request: timeouts, wire inspectors |
| **Request** | `InteractionBuilder` `with_*()` methods | One interaction |
| **Generation** | `GenerationConfig`, or the builder shortcuts for its fields | Model sampling and output |

Anything you leave unset is omitted from the request, so the model's default
applies.

## GenerationConfig

Set the whole struct:

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
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Write a short story")
    .with_generation_config(config)
    .create()
    .await?;
# Ok(())
# }
```

Or use the builder shortcuts, which set individual fields:

```rust,no_run
# use genai_rs::{Client, ThinkingLevel};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Brainstorm 20 startup ideas")
    .with_seed(42)
    .with_stop_sequences(vec!["THE END".to_string()])
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;
# Ok(())
# }
```

### Fields

| Field | Type | Builder shortcut | Notes |
|-------|------|------------------|-------|
| `temperature` | `Option<f32>` | — | Sampling randomness. Deprecated upstream (google-genai 2.34.0), still accepted |
| `top_p` | `Option<f32>` | — | Nucleus sampling. Deprecated upstream like `temperature`, still accepted |
| `max_output_tokens` | `Option<i32>` | — | Output cap. Per-model ceilings come from the Models API: `GET /v1beta/models/{model}` returns `outputTokenLimit` (65,536 for `gemini-3.8-flash`). Counts across decoding continuations; see [Decoding continuation](#decoding-continuation) |
| `seed` | `Option<i64>` | `with_seed()` | See [Seeds](#seeds) |
| `stop_sequences` | `Option<Vec<String>>` | `with_stop_sequences()` | Generation halts on any of them |
| `thinking_level` | `Option<ThinkingLevel>` | `with_thinking_level()` | See [Thinking Mode](THINKING_MODE.md) |
| `thinking_summaries` | `Option<ThinkingSummaries>` | `with_thinking_summaries()` | See [Thinking Mode](THINKING_MODE.md#thinking-summaries) |
| `tool_choice` | `Option<ToolChoice>` | `with_function_calling_mode()`, `with_tool_choice()`, `with_allowed_tools()` | See [Function Calling](FUNCTION_CALLING.md#function-calling-modes) |
| `speech_config` | `Option<Vec<SpeechConfig>>` | `with_speech_config()`, `with_speech_configs()`, `add_speech_config()`, `with_voice()` | See [Output Modalities](OUTPUT_MODALITIES.md) |
| `image_config` | `Option<ImageConfig>` | `with_image_config()` | Image generation aspect ratio and size |
| `video_config` | `Option<VideoConfig>` | `with_video_config()` | Video generation settings |
| `transcription_config` | `Option<TranscriptionConfig>` | `with_transcription_config()` | Audio input transcription; see [Multimodal](MULTIMODAL.md#audio) |

The 2026-05-20 API revision removed `top_k`, so `GenerationConfig` has no such
field. (`FileSearchConfig::with_top_k()` is an unrelated retrieval setting.)

Because the shortcuts and `with_generation_config()` write the same struct,
call `with_generation_config()` first if you mix them.

### Seeds

`with_seed(n)` makes sampling repeatable for identical requests: identical
seeded requests have returned identical text (see Models in the
[gap tracker](INTERACTIONS_API_GAP.md#models)). The API doesn't document a
guarantee, though, so tests should still assert on structure rather than
exact text (see [Testing](TESTING.md)).

### Decoding continuation

A response cut off by `max_output_tokens` can be continued instead of
regenerated, on `CONTINUATION_MODEL` only (every other model answers `400
Decoding continuation is not enabled for this model`). The cut-off response
is `Incomplete` and carries a `continuation_token`. The live record of
these rules is in the [gap tracker](INTERACTIONS_API_GAP.md#interaction-residue-2026-11-05).

A stored interaction (the default) continues from its id and the token
alone, with no input. `continue_decode` builds
that request: the response's model, its id as the previous interaction,
and its token. It sends no `max_output_tokens`, and the continuation runs
on to the end; a cap you add counts across continuations, so it must
exceed what the decode already produced.

```rust,no_run
# use genai_rs::{Client, GenerationConfig};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let first = client
    .interaction()
    .with_model(genai_rs::CONTINUATION_MODEL)
    .with_text("Write a sixteen-line poem about the sea.")
    .with_generation_config(GenerationConfig {
        max_output_tokens: Some(30),
        ..Default::default()
    })
    .create()
    .await?;
if first.continuation_token.is_some() {
    let rest = client.interaction().continue_decode(&first).create().await?;
    println!("{}{}", first.all_text(), rest.all_text());
}
# Ok(())
# }
```

A `store: false` interaction has no id, so its continuation resends the
same request with the token and a larger `max_output_tokens`. Reusing the
first cap is a 400 ("has already been exhausted"), and any different input
is a 400 too; without input the API answers `contents is not specified`.

```rust,no_run
# use genai_rs::{Client, GenerationConfig};
# async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
let ask = |max_output_tokens| {
    client
        .interaction()
        .with_model(genai_rs::CONTINUATION_MODEL)
        .with_text("Write a sixteen-line poem about the sea.")
        .with_store_disabled()
        .with_generation_config(GenerationConfig {
            max_output_tokens: Some(max_output_tokens),
            ..Default::default()
        })
};
let first = ask(30).create().await?;
if let Some(token) = &first.continuation_token {
    let rest = ask(600).with_continuation_token(token).create().await?;
    println!("{}{}", first.all_text(), rest.all_text());
}
# Ok(())
# }
```

## Client configuration

```rust
use genai_rs::Client;
use std::time::Duration;

let client = Client::builder("api-key".to_string())
    .with_connect_timeout(Duration::from_secs(10))
    .build()?;
# Ok::<(), genai_rs::GenaiError>(())
```

| `ClientBuilder` method | Default | Effect |
|------------------------|---------|--------|
| `with_timeout(Duration)` | None | Total-request timeout for every request, **including streamed bodies**. A request-level timeout cannot extend it |
| `with_connect_timeout(Duration)` | 30 s (`DEFAULT_CONNECT_TIMEOUT`) | Connection timeout: DNS, TCP and TLS. `Duration::MAX` turns it off, for tests on a paused tokio clock ([why](RELIABILITY.md#tests-on-a-paused-clock)) |
| `with_read_timeout(Duration)` | None | Longest wait for the headers or the next chunk, reset by each one: bounds a stalled stream without capping a long one. Set it above the longest a call thinks before answering |
| `with_base_url(String)` | The Gemini API host | Send requests to a proxy or mock server instead of `generativelanguage.googleapis.com` |
| `with_api_version(String)` | `"v1beta"` | The version path segment. `"v1"` is the stable endpoint: it serves interactions in full (create, get, delete, cancel, streaming, continuation) and the Files and File Search Store APIs, and answers 404 for agents, environments, webhooks, credentials, voices and triggers ([gap tracker](INTERACTIONS_API_GAP.md#interaction-residue-2026-11-05)) |
| `add_wire_inspector(Arc<dyn WireInspector>)` | None (`LOUD_WIRE` adds a printer) | Observe raw API traffic; see [Logging Strategy](LOGGING_STRATEGY.md#wire-inspection-api) |

`Client::new(api_key)` is `Client::builder(api_key).build()`, panicking
instead of returning an error, so both get the same defaults: a 30 s connect
timeout and no read or total timeout. There is no default read or total
timeout because a non-streaming response's headers arrive only after
generation finishes, so any default short enough to catch a hung connection
would cut off a long legitimate generation (the reasoning is in
[Reliability](RELIABILITY.md#why-only-a-connect-timeout-by-default)).

## Request-level settings

| Method | Notes |
|--------|-------|
| `with_timeout(Duration)` | `tokio::time::timeout` around the call; returns `GenaiError::Timeout`. How it interacts with the client timeout, and what it covers for streams and auto-functions: [Reliability](RELIABILITY.md#timeouts) |
| `with_service_tier(ServiceTier)` | `Flex` / `Standard` / `Priority`, sent as `"flex"` / `"standard"` / `"priority"` (`Deferred` is Vertex-only); see [Reliability](RELIABILITY.md#service-tiers) |
| `with_system_instruction(str)` | System prompt for this interaction |
| `with_store_enabled()` / `with_store_disabled()` | Server-side storage (on by default); see [Builder API](BUILDER_API.md#validation-errors) |
| `with_background(true)` | Return immediately and run in the background; see [Agents and Background Execution](AGENTS_AND_BACKGROUND.md) |
| `add_extra(key, value)` | Adds a request field this crate does not model yet, sent flat beside the modeled ones (`InteractionRequest::extra`); a repeated key replaces its value. The server validates it like any field: an unknown name is `400 Unknown parameter`. A key naming a modeled field overrides it, since the API keeps the last of a repeated key ([gap tracker](INTERACTIONS_API_GAP.md#requests)) |

## Cached content

**Explicit context caching is not available on the Interactions API.** The
request field was removed after live probing showed the API rejects it
outright (D-005; the sweeps in the
[gap tracker](INTERACTIONS_API_GAP.md) record it):

```text
400 Unknown parameter 'cached_content'
```

`cachedContent` and `cached_content_name` are rejected too, and so are both
spellings nested inside `generation_config`. The `/v1beta/cachedContents`
resource itself works (a cache creates and reports its token count), but
nothing in the Interactions API consumes one.

**Implicit caching still applies** and needs no configuration. The
response's `usage.total_cached_tokens` shows how much of a request hit the
cache.
