# Output Modalities Guide

This guide covers configuring response output types including text, images, audio, and structured JSON.

## Table of Contents

- [Overview](#overview)
- [Text Output](#text-output)
- [Image Generation](#image-generation)
- [Audio Output (Text-to-Speech)](#audio-output-text-to-speech)
- [Multi-Speaker TTS](#multi-speaker-tts)
- [Video Generation](#video-generation)
- [Structured Output (JSON)](#structured-output-json)
- [Typed Response Formats](#typed-response-formats)
- [Combining Modalities](#combining-modalities)
- [Response Helpers](#response-helpers)
- [Best Practices](#best-practices)

## Overview

Gemini models can generate different types of output content:

| Modality | Method | Model Required | Use Case |
|----------|--------|----------------|----------|
| **Text** | Default | Any | Conversations, analysis |
| **Image** | `with_image_output()` | `gemini-3.1-flash-image` | Image generation |
| **Audio** | `with_audio_output()` | `DEFAULT_TTS_MODEL` (`gemini-3.8-flash-tts`) | Text-to-speech |
| **Video** | `with_video_output()` | Video-capable model (e.g., Veo previews) | Video generation (background) |
| **JSON** | `with_response_format()` | Any | Structured data extraction |

Each modality can additionally carry a typed [`ResponseFormat`](#typed-response-formats)
controlling MIME type, delivery (`inline` vs `uri`), and per-modality options.

## Text Output

Text is the default output modality - no special configuration needed.

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Explain quantum computing")
    .create()
    .await?;

// Extract text
if let Some(text) = response.as_text() {
    println!("{}", text);
}
```

### Checking for Text

```rust,ignore
if response.has_text() {
    let text = response.as_text().unwrap();
    // Process text...
}
```

## Image Generation

Generate images from text prompts.

### Basic Image Generation

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_IMAGE_MODEL)  // Image generation model required
    .with_text("A sunset over mountains, digital art style")
    .with_image_output()
    .create()
    .await?;

// Save the first image
if let Some(bytes) = response.first_image_bytes()? {
    std::fs::write("sunset.png", &bytes)?;
}
```

### Iterating Multiple Images

```rust,ignore
if response.has_images() {
    for (i, image) in response.images().enumerate() {
        let bytes = image.bytes()?;
        let filename = format!("image_{}.{}", i, image.extension());
        std::fs::write(&filename, bytes)?;

        println!("Saved {} ({} bytes, {:?})",
                 filename, bytes.len(), image.mime_type());
    }
}
```

### Image Metadata

```rust,ignore
for image in response.images() {
    // Get MIME type (e.g., Some("image/png"))
    let mime = image.mime_type();

    // Get appropriate file extension
    let ext = image.extension();  // "png", "jpeg", etc.

    // Get raw bytes
    let bytes = image.bytes()?;
}
```

### Model Requirements

Image generation requires specific models:

| Model | Capability |
|-------|------------|
| `gemini-3.1-flash-image` | Text-to-image generation |

Note: Image generation may not be available in all regions.

## Audio Output (Text-to-Speech)

Convert text to spoken audio.

### Basic TTS

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_TTS_MODEL)  // TTS model required
    .with_text("Hello, welcome to genai-rs!")
    .with_audio_output()
    .with_voice("Kore")
    .create()
    .await?;

// `DEFAULT_TTS_MODEL` returns `audio/wav`, so the bytes are a playable file.
if let Some(audio) = response.first_audio() {
    std::fs::write(format!("output.{}", audio.extension()), audio.bytes()?)?;
}
```

Output format depends on the model (verified live 2026-09-24):

| Model | `mime_type` | Notes |
|-------|-------------|-------|
| `gemini-3.8-flash-tts`, `gemini-3.8-flash-lite-tts` | `audio/wav` | RIFF container, 24 kHz mono; no `sample_rate`/`channels` metadata |
| `gemini-2.5-pro-preview-tts`, `gemini-3.1-flash-tts-preview` | `audio/L16;...;rate=24000` | Raw PCM; `extension()` returns `pcm`, so wrap it in a WAV header yourself |

### Voices

`with_voice()` takes a voice ID. The catalog, with descriptions, is served by
the Voices API (see [Custom voices](#custom-voices)):

```rust,ignore
let page = client.list_voices(&genai_rs::ListVoicesParams::new().with_page_size(10)).await?;
for voice in &page.voices {
    println!("{:?}: {:?}", voice.id, voice.description);
}
```

Prebuilt IDs are lowercase (`kore`, `puck`, locale voices like
`ar-001-advisor-2`); the capitalized spellings are accepted too.

### Full Speech Configuration

```rust,ignore
use genai_rs::SpeechConfig;

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_TTS_MODEL)
    .with_text("Good morning, everyone!")
    .with_audio_output()
    .with_speech_config(SpeechConfig::with_voice_and_language("Charon", "en-GB"))
    .create()
    .await?;
```

### Audio Metadata

```rust,ignore
for audio in response.audios() {
    let mime = audio.mime_type();        // e.g. Some("audio/wav")
    let ext = audio.extension();         // "wav", "pcm", "mp3", ...
    let rate = audio.sample_rate();      // reported by the L16 models only
    let bytes = audio.bytes()?;
}
```

## Multi-Speaker TTS

`generation_config.speech_config` is a list with one entry per speaker. On
`DEFAULT_TTS_MODEL`, each text turn must name its speaker with a
`speech_metadata` annotation — `Content::speaker_text()` builds one:

```rust,ignore
use genai_rs::{Content, InteractionInput, SpeechConfig};

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_TTS_MODEL)
    .with_input(InteractionInput::Content(vec![
        Content::speaker_text("Alice", "Hi Bob!"),
        Content::speaker_text("Bob", "Hey Alice, lovely day!"),
    ]))
    .with_audio_output()
    .with_speech_configs(vec![
        SpeechConfig::for_speaker("Alice", "Kore", "en-US"),
        SpeechConfig::for_speaker("Bob", "Puck", "en-US"),
    ])
    .create()
    .await?;
```

The API returns one combined audio stream. Rules observed live (2026-09-24):

| Request | Result |
|---------|--------|
| `Alice: ...` / `Bob: ...` transcript text, no annotations | `400 Multi-speaker interactions must specify a speaker for each text turn` on 3.8 TTS models; accepted by 2.5-pro and 3.1-flash TTS |
| Annotations on 2.5-pro or 3.1-flash TTS | `400 Speech annotations are not supported for model ...` |
| A speaker not in `speech_config` | `400 ... must specify a speaker matching a speaker defined in speech_config` |
| Spanned annotations (`start_index`/`end_index`) with a gap | `400 ... must cover the entire text string without gaps` |

An annotation may also carry a `style` (e.g. `"whisper"`); for a single voice,
leave `speaker` unset: `Annotation::speech_metadata(None, Some("whisper".into()))`.

`add_speech_config()` accumulates entries one at a time;
`with_speech_configs()` replaces the whole list.

### Custom voices

`/v1beta/voices` designs a voice from a text prompt and stores it; the returned
ID works anywhere a prebuilt voice name does.

```rust,ignore
use genai_rs::CreateVoiceRequest;

let voice = client
    .create_voice(&CreateVoiceRequest::prompted("A calm, low-pitched narrator."))
    .await?;
let id = voice.id.clone().expect("stored voices have an id");

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_TTS_MODEL)
    .with_text("Chapter one.")
    .with_audio_output()
    .with_voice(&id)
    .create()
    .await?;

client.delete_voice(&id).await?;
```

Prompted voices must be stored (`store: true`, the default for
`CreateVoiceRequest::prompted`) and expire after a year. A custom voice worked
on the 3.8 and 2.5-pro TTS models; `gemini-3.1-flash-tts-preview` returned
500.

## Video Generation

Request video output with `with_video_output()` (sets
`response_modalities: ["video"]`), optionally steering the generation task
via `generation_config.video_config`:

```rust,ignore
use genai_rs::{ResponseDelivery, ResponseFormat, VideoConfig, VideoTask};

let response = client
    .interaction()
    .with_model("veo-3.1-generate-preview")
    .with_text("A hummingbird hovering over a red flower, slow motion")
    .with_video_output()
    .with_video_config(VideoConfig::new().with_task(VideoTask::TextToVideo))
    .with_response_format(ResponseFormat::Video {
        delivery: Some(ResponseDelivery::Uri),
        gcs_uri: Some("gs://my-bucket/videos/out".to_string()),
        aspect_ratio: None,
        duration: Some("8s".to_string()),
    })
    .with_background(true)
    .with_store_enabled()
    .create()
    .await?;
```

Notes:

- `VideoTask`: `TextToVideo`, `ImageToVideo`, `ReferenceToVideo`, `Edit`.
  Omit the task to let the model pick based on the prompt and input media.
- Video generation is long-running: run it in the background and poll, or
  subscribe a webhook to the `video.generated` event
  (see `docs/AGENTS_AND_BACKGROUND.md`).
- Video *input* content blocks and streaming video deltas are already part of
  the content model; this section is about generated video output.
- Pending live verification against the 2026-05-20 revision.

## Structured Output (JSON)

Force the model to return valid JSON matching a schema.

> **Note**: `with_response_format(schema)` is all you need — passing a JSON schema implies JSON output. The old `with_response_mime_type()` method was removed: the API rejects any request that sets `response_mime_type` — even alongside `response_format` (verified live 2026-07: 400 "responseFormat must be set when responseMimeType is set" in all combinations).

### Basic JSON Schema

```rust,ignore
use serde_json::json;

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Generate a user profile for John Doe, age 30")
    .with_response_format(json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"},
            "email": {"type": "string"}
        },
        "required": ["name", "age"]
    }))
    .create()
    .await?;

// Parse the structured response
let text = response.as_text().unwrap();
let data: serde_json::Value = serde_json::from_str(text)?;
println!("Name: {}", data["name"]);
println!("Age: {}", data["age"]);
```

### Complex Schemas

```rust,ignore
let schema = json!({
    "type": "object",
    "properties": {
        "products": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "price": {"type": "number"},
                    "in_stock": {"type": "boolean"}
                },
                "required": ["name", "price"]
            }
        },
        "total_count": {"type": "integer"}
    },
    "required": ["products", "total_count"]
});

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("List 3 popular smartphones with prices")
    .with_response_format(schema)
    .create()
    .await?;
```

### With Enums

```rust,ignore
let schema = json!({
    "type": "object",
    "properties": {
        "sentiment": {
            "type": "string",
            "enum": ["positive", "negative", "neutral"]
        },
        "confidence": {
            "type": "number",
            "minimum": 0,
            "maximum": 1
        }
    },
    "required": ["sentiment", "confidence"]
});
```

### Combining with Tools

Structured output works with built-in tools:

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("What's the weather in Tokyo?")
    .with_google_search()
    .with_response_format(json!({
        "type": "object",
        "properties": {
            "temperature": {"type": "string"},
            "conditions": {"type": "string"},
            "humidity": {"type": "string"}
        },
        "required": ["temperature", "conditions"]
    }))
    .create()
    .await?;
```

## Typed Response Formats

`response_format` is a typed union tagged by `type` — `text`, `audio`,
`image`, `video` — plus a list form for multi-modality requests.
`with_response_format()` accepts any `ResponseFormat` *or* a raw
`serde_json::Value` schema (which converts to the `text` +
`application/json` form shown above), so existing JSON-schema code keeps
working unchanged.

```rust,ignore
use genai_rs::{ImageAspectRatio, ImageSize, ResponseDelivery, ResponseFormat};

// Audio: MP3 at 24kHz, delivered inline
let audio = ResponseFormat::Audio {
    mime_type: Some("audio/mp3".to_string()),
    delivery: Some(ResponseDelivery::Inline),
    sample_rate: Some(24000),
    bit_rate: Some(128_000),
};

// Image: JPEG, 16:9, 2K, delivered by URI
let image = ResponseFormat::Image {
    mime_type: Some("image/jpeg".to_string()),
    delivery: Some(ResponseDelivery::Uri),
    aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
    image_size: Some(ImageSize::Hd2k),
};

// List form: one format per requested modality
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
    .with_text("A labeled diagram of a volcano")
    .with_response_formats(vec![ResponseFormat::text_plain(), image])
    .create()
    .await?;
```

Delivery modes (`ResponseDelivery`): `Inline` returns base64 bytes in the
response; `Uri` delivers by URI (for video on Vertex, set `gcs_uri`).
Unknown format types and delivery modes are preserved via `Unknown` variants
(Evergreen).

## Combining Modalities

### Text + Structured Output

The structured output still uses text modality internally:

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Analyze this sentiment")
    .with_response_format(sentiment_schema)
    .create()
    .await?;

// Access via text()
let json_str = response.as_text().unwrap();
```

### Multiple Content Types in Response

Responses can contain multiple content types (spread across `response.steps`):

```rust,ignore
// Check what content types are present
println!("Has text: {}", response.has_text());
println!("Has images: {}", response.has_images());
println!("Has audio: {}", response.has_audio());
println!("Has thoughts: {}", response.has_thoughts());

// Step summary (per-step-type and per-content-type counts)
let summary = response.step_summary();
println!("Text blocks: {}", summary.text_count);
println!("Image blocks: {}", summary.image_count);
println!("Audio blocks: {}", summary.audio_count);
println!("Thought steps: {}", summary.thought_count);
```

## Response Helpers

### Text Helpers

```rust,ignore
// Get first text content
let text: Option<&str> = response.as_text();

// Check for text
let has_text: bool = response.has_text();
```

### Image Helpers

```rust,ignore
// Get first image bytes (convenience)
let bytes: Option<Vec<u8>> = response.first_image_bytes()?;

// Check for images
let has_images: bool = response.has_images();

// Iterate images
for image in response.images() {
    let bytes = image.bytes()?;
    let mime = image.mime_type();
    let ext = image.extension();
}
```

### Audio Helpers

```rust,ignore
// Get first audio
let audio: Option<AudioInfo> = response.first_audio();

// Check for audio
let has_audio: bool = response.has_audio();

// Iterate audio outputs
for audio in response.audios() {
    let bytes = audio.bytes()?;
    let mime = audio.mime_type();
    let ext = audio.extension();
    let rate = audio.sample_rate();   // Option<u32>
    let channels = audio.channels();  // Option<u32>
}
```

### Thought Helpers (Thinking Mode)

```rust,ignore
// Check for thought steps (Step::Thought)
let has_thoughts: bool = response.has_thoughts();

// Iterate thought signatures (cryptographic proofs, not readable reasoning)
for signature in response.thought_signatures() {
    println!("Thought signature: {}", signature);
}

// Iterate readable thought summaries (requires with_thinking_summaries(ThinkingSummaries::Auto))
for content in response.thought_summaries() {
    if let Some(text) = content.as_text() {
        println!("Reasoning summary: {}", text);
    }
}
```

## Best Practices

### 1. Use Correct Models for Modalities

```rust,ignore
// Image generation - requires image model
client.interaction()
    .with_model(genai_rs::DEFAULT_IMAGE_MODEL)  // Correct
    .with_image_output()

// TTS - requires TTS model
client.interaction()
    .with_model(genai_rs::DEFAULT_TTS_MODEL)  // Correct
    .with_audio_output()

// Text/JSON - any model works
client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)  // Standard model fine
    .with_response_format(schema)
```

### 2. Check Response Status Before Extraction

```rust,ignore
if response.status == InteractionStatus::Completed {
    if let Some(bytes) = response.first_image_bytes()? {
        // Safe to use
    }
}
```

### 3. Handle Missing Content Gracefully

```rust,ignore
// Check before accessing
if response.has_images() {
    let bytes = response.first_image_bytes()?.unwrap();
} else {
    println!("No image generated - try rephrasing prompt");
}
```

### 4. Use Appropriate File Extensions

```rust,ignore
// Use the extension helper for correct file format
for image in response.images() {
    let filename = format!("output.{}", image.extension());
    std::fs::write(&filename, image.bytes()?)?;
}
```

### 5. Validate Structured Output

```rust,ignore
let text = response.as_text().unwrap();

// Parse and validate
match serde_json::from_str::<MyStruct>(text) {
    Ok(data) => { /* use data */ }
    Err(e) => {
        // Schema enforcement should prevent this, but handle gracefully
        log::warn!("Unexpected JSON format: {}", e);
    }
}
```

### 6. Regional Availability

Image generation may not be available in all regions:

```rust,ignore
match result {
    Ok(response) => { /* success */ }
    Err(GenaiError::Api { message, .. })
        if message.contains("not found") || message.contains("not supported") => {
        eprintln!("Image generation not available in your region");
    }
    Err(e) => return Err(e.into()),
}
```

## Examples

| Example | Features |
|---------|----------|
| `image_generation` | Image output, saving files |
| `text_to_speech` | Audio output, voice selection |
| `structured_output` | JSON schemas, validation |

Run with:
```bash
cargo run --example <name>
```
