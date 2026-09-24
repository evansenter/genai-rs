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
    .with_presence_penalty(0.5)
    .with_frequency_penalty(0.3)
    .with_thinking_level(ThinkingLevel::Medium)
    .create()
    .await?;
# Ok(())
# }
```

### Fields

| Field | Type | Builder shortcut | Notes |
|-------|------|------------------|-------|
| `temperature` | `Option<f32>` | — | Sampling randomness |
| `top_p` | `Option<f32>` | — | Nucleus sampling |
| `max_output_tokens` | `Option<i32>` | — | Output cap. Per-model ceilings come from the Models API: `GET /v1beta/models/{model}` returns `outputTokenLimit` (65,536 for `gemini-3.8-flash`, checked 2026-09-24) |
| `seed` | `Option<i64>` | `with_seed()` | See [Seeds](#seeds) |
| `stop_sequences` | `Option<Vec<String>>` | `with_stop_sequences()` | Generation halts on any of them |
| `presence_penalty` | `Option<f32>` | `with_presence_penalty()` | Penalizes tokens that already appeared; range [-2.0, 2.0] |
| `frequency_penalty` | `Option<f32>` | `with_frequency_penalty()` | Penalizes tokens by frequency; range [-2.0, 2.0] |
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

`with_seed(n)` makes sampling repeatable for identical requests. In a live
check (2026-09-24, `DEFAULT_MODEL`, `ThinkingLevel::Low`), three identical
seeded requests returned identical text. The API doesn't document a
guarantee, though, so tests should still assert on structure rather than
exact text (see [Testing](TESTING.md)).

## Client configuration

```rust
use genai_rs::Client;
use std::time::Duration;

let client = Client::builder("api-key".to_string())
    .with_connect_timeout(Duration::from_secs(10))
    .build()?;
# Ok::<(), genai_rs::GenaiError>(())
```

| `ClientBuilder` method | Effect |
|------------------------|--------|
| `with_timeout(Duration)` | Total-request timeout for every request, **including streamed bodies**. A request-level timeout cannot extend it |
| `with_connect_timeout(Duration)` | Connection timeout |
| `add_wire_inspector(Arc<dyn WireInspector>)` | Observe raw API traffic; see [Logging Strategy](LOGGING_STRATEGY.md#wire-inspection-api) |

`Client::new(api_key)` is `Client::builder(api_key).build()` with no
timeouts.

## Request-level settings

| Method | Notes |
|--------|-------|
| `with_timeout(Duration)` | `tokio::time::timeout` around the call; returns `GenaiError::Timeout`. How it interacts with the client timeout, and what it covers for streams and auto-functions: [Reliability](RELIABILITY.md#timeouts) |
| `with_service_tier(ServiceTier)` | `Flex` / `Standard` / `Priority`, sent as `"flex"` / `"standard"` / `"priority"`; see [Reliability](RELIABILITY.md#service-tiers) |
| `with_system_instruction(str)` | System prompt for this interaction |
| `with_store_enabled()` / `with_store_disabled()` | Server-side storage (on by default); see [Builder API](BUILDER_API.md#validation-errors) |
| `with_background(true)` | Return immediately and run in the background; see [Agents and Background Execution](AGENTS_AND_BACKGROUND.md) |

## Cached content

**Explicit context caching is not available on the Interactions API.** The
request field was removed after live probing (2026-08-16) showed the API
rejects it outright:

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
