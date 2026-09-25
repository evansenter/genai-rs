# Output Modalities Guide

A response's content is text by default. Other outputs need the right model and
a modality shortcut:

| Output | Builder | Model |
|--------|---------|-------|
| Text | (default) | Any, e.g. `genai_rs::DEFAULT_MODEL` |
| Structured JSON | `with_response_format(schema)` | Any |
| Image | `with_image_output()` | `genai_rs::DEFAULT_IMAGE_MODEL` |
| Speech | `with_audio_output()` | `genai_rs::DEFAULT_TTS_MODEL` (`gemini-3.8-flash-tts`) |
| Video | `with_video_output()` | None available through the Interactions API today (see [Video](#video)) |

The shortcuts set `response_modalities` (`["image"]`, `["audio"]`,
`["video"]`). A [typed `ResponseFormat`](#typed-response-formats) can refine
each modality.

## Text and structured JSON

```rust,ignore
let text = response.as_text(); // Option<&str>: the model_output text
```

Pass a JSON schema to `with_response_format()` to constrain the output. A raw
`serde_json::Value` schema becomes the typed
`{"type": "text", "mime_type": "application/json", "schema": ...}` form, and the
API validates the output against it (verified live 2026-07).

```rust,no_run
use serde_json::json;

# async fn run(client: genai_rs::Client) -> Result<(), Box<dyn std::error::Error>> {
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text("Generate a user profile for John Doe, age 30")
    .with_response_format(json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"},
            "sentiment": {"type": "string", "enum": ["positive", "negative", "neutral"]}
        },
        "required": ["name", "age"]
    }))
    .create()
    .await?;

let data: serde_json::Value = serde_json::from_str(response.as_text().unwrap_or("{}"))?;
println!("{} is {}", data["name"], data["age"]);
# Ok(())
# }
```

There is no `with_response_mime_type()`. The API rejects any request that
sets `response_mime_type`, even alongside `response_format` (live 2026-07:
400 "responseFormat must be set when responseMimeType is set"), so the field
was removed. `examples/structured_output.rs` is runnable.

## Images

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
    .with_text("A sunset over mountains, digital art style")
    .with_image_output()
    .create()
    .await?;

if let Some(bytes) = response.first_image_bytes()? {
    std::fs::write("sunset.png", &bytes)?;
}
for (i, image) in response.images().enumerate() {
    // mime_type() e.g. Some("image/png"); extension() "png", "jpeg", ...
    std::fs::write(format!("image_{i}.{}", image.extension()), image.bytes()?)?;
}
```

Image output is a separate model family: `DEFAULT_MODEL` does not produce
images. `examples/image_generation.rs` is runnable.

## Speech (text-to-speech)

```rust,ignore
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_TTS_MODEL)
    .with_text("Hello, welcome to genai-rs!")
    .with_audio_output()
    .with_voice("Kore")
    .create()
    .await?;

// DEFAULT_TTS_MODEL returns audio/wav, so the bytes are a playable file
if let Some(audio) = response.first_audio() {
    std::fs::write(format!("output.{}", audio.extension()), audio.bytes()?)?;
}
```

The output format depends on the model (verified live 2026-09-24):

| Model | `mime_type` | Notes |
|-------|-------------|-------|
| `gemini-3.8-flash-tts`, `gemini-3.8-flash-lite-tts` | `audio/wav` | RIFF container, 24 kHz mono; no `sample_rate` / `channels` fields |
| `gemini-2.5-pro-preview-tts`, `gemini-3.1-flash-tts-preview` | `audio/L16;...;rate=24000` | Raw PCM, so `extension()` returns `pcm`. Add a WAV header yourself to play it |

`with_voice(id)` is shorthand for
`with_speech_config(SpeechConfig::with_voice_and_language(id, "en-US"))`. Pass
a `SpeechConfig` yourself for another language. `audio.sample_rate()` and
`audio.channels()` are reported by the L16 models only.
`examples/text_to_speech.rs` is runnable.

### Voices

The Voices API (`/v1beta/voices`) serves the catalog and stores custom
voices. Prebuilt ids are lowercase (`kore`, `puck`, and locale voices like
`ar-001-advisor-2`); the capitalized spellings are accepted too.

```rust,ignore
use genai_rs::{CreateVoiceRequest, ListVoicesParams};

let page = client.list_voices(&ListVoicesParams::new().with_page_size(10)).await?;
for voice in &page.voices {
    println!("{:?}: {:?}", voice.id, voice.description);
}

// Design a voice from a prompt; its id works anywhere a prebuilt name does
let voice = client
    .create_voice(&CreateVoiceRequest::prompted("A calm, low-pitched narrator."))
    .await?;
let id = voice.id.clone().expect("stored voices have an id");
// ... .with_voice(&id) ...
client.delete_voice(&id).await?;
```

Prompted voices must be stored (`store: true`, the default for
`CreateVoiceRequest::prompted`) and expire after a year. A custom voice
worked on the 3.8 and 2.5-pro TTS models; `gemini-3.1-flash-tts-preview`
returned 500.

### Multi-speaker

`speech_config` is a list with one entry per speaker. On `DEFAULT_TTS_MODEL`
each text turn must name its speaker with a `speech_metadata` annotation, which
`Content::speaker_text()` builds:

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

The API returns one combined audio stream. Rules observed live 2026-09-24:

| Request | Result |
|---------|--------|
| `Alice: ...` / `Bob: ...` transcript text, no annotations | `400 Multi-speaker interactions must specify a speaker for each text turn` on 3.8 TTS models; accepted by the 2.5-pro and 3.1-flash TTS models |
| Annotations on the 2.5-pro or 3.1-flash TTS models | `400 Speech annotations are not supported for model ...` |
| A speaker not in `speech_config` | `400 ... must specify a speaker matching a speaker defined in speech_config` |
| Spanned annotations (`start_index` / `end_index`) with a gap | `400 ... must cover the entire text string without gaps` |

An annotation can also carry a `style` (for example `"whisper"`). For a single
voice, leave `speaker` unset:
`Annotation::speech_metadata(None, Some("whisper".into()))`.
`add_speech_config()` appends one entry; `with_speech_configs()` replaces the
list. The accepted `speech_config` wire forms are in
[Enum Wire Formats](ENUM_WIRE_FORMATS.md#speech_config-wire-forms).

## Video

`with_video_output()` sets `response_modalities: ["video"]`, and
`with_video_config(VideoConfig::new().with_task(VideoTask::TextToVideo))` sets
`generation_config.video_config.task`. The API validates the task (`TextToVideo`,
`ImageToVideo`, `ReferenceToVideo`, `Edit`, `Extend`).

**No model generates video through the Interactions API today** (live
2026-07). Veo models return 404 (they list only the legacy
`predictLongRunning` method), and Gemini models reject
`response_modalities: ["video"]`. The video `ResponseFormat`'s `gcs_uri` is
Vertex-only. The types are modeled for when this opens up.

## Typed response formats

`response_format` is a union tagged by `type` (`text`, `audio`, `image`,
`video`). `with_response_formats(vec![...])` sends the list form, one entry per
modality. On the Gemini API only some options are accepted (live 2026-07):

| Variant | Accepted | Rejected |
|---------|----------|----------|
| `ResponseFormat::Text { mime_type, schema }` (`text_plain()`, `json_schema(..)`) | Both MIME types; schemas enforced | — |
| `ResponseFormat::Image { mime_type, delivery, aspect_ratio, image_size }` | `mime_type: "image/jpeg"` only, `aspect_ratio`, `image_size` | Any `delivery` ("Image delivery mode is not supported.") |
| `ResponseFormat::Audio { mime_type, delivery, sample_rate, bit_rate }` | `sample_rate` | Any `mime_type`, any `delivery` |
| `ResponseFormat::Video { .. }` | — | `gcs_uri` (Vertex-only); no model serves video |

```rust,ignore
use genai_rs::{ImageAspectRatio, ImageSize, ResponseFormat};

let image = ResponseFormat::Image {
    mime_type: Some("image/jpeg".to_string()),
    delivery: None,
    aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
    image_size: Some(ImageSize::Hd2k),
};

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
    .with_text("A labeled diagram of a volcano")
    .with_response_formats(vec![ResponseFormat::text_plain(), image])
    .create()
    .await?;
```

Unknown format types and delivery modes are preserved in `Unknown` variants.

## Response helpers

| Helper | Returns |
|--------|---------|
| `as_text()`, `has_text()` | First text, and whether there is any |
| `first_image_bytes()`, `images()`, `has_images()` | Decoded image bytes / `ImageInfo` (`bytes()`, `mime_type()`, `extension()`) |
| `first_audio()`, `audios()`, `has_audio()` | `AudioInfo` (`bytes()`, `mime_type()`, `extension()`, `sample_rate()`, `channels()`) |
| `has_thoughts()`, `thought_summaries()` | See [Thinking Mode](THINKING_MODE.md) |
| `step_summary()` | Per-type counts (`text_count`, `image_count`, `audio_count`, `thought_count`, ...) |

`first_image_bytes()` and `bytes()` return `Result`: they decode base64, which
can fail.
