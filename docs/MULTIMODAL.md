# Multimodal Content Guide

This page covers sending images, audio, video and documents to the model. For
generating images and speech, see [Output Modalities](OUTPUT_MODALITIES.md).

## Overview

There are three ways to attach media:

| Method | How | Notes |
|--------|-----|-------|
| Inline base64 | `Content::image_data(base64, mime)` and friends | The file helpers warn above 20 MB (the crate's recommended inline ceiling; the API enforces its own limits) |
| URI reference | `Content::image_uri(uri, mime)`, `Content::from_file(&file_metadata)` | Files API uploads or other URIs the API can read |
| File helpers | `image_from_file(path).await?` and friends | Read, base64-encode and detect the MIME type from the extension |

Under API revision 2026-05-20, `Content` is purely *data*: `Text`, `Image`,
`Audio`, `Video`, `Document`, plus an `Unknown` fallback. Tool activity and
thoughts are `Step` variants in `response.steps`, not `Content`. Check kinds
with `content.is_image()`, `is_audio()`, `is_video()` and `is_document()`.

`with_content(vec![...])` sends one multimodal turn. To combine media with
conversation history, put the content in a `Step::user_input(...)` and use
`with_history()`; see [Builder API](BUILDER_API.md#input-methods).

## Images

```rust,ignore
use genai_rs::{Content, image_from_file};

// Inline base64
let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_content(vec![
        Content::text("What's in this image?"),
        Content::image_data(base64_string, "image/png"),
    ])
    .create()
    .await?;

// From the filesystem (MIME type detected from the extension)
let photo = image_from_file("photo.jpg").await?;

// By URI (for example a Files API upload)
let uploaded = Content::image_uri(&file_metadata.uri, "image/png");
```

Pass several `Content::image_*` blocks in one `with_content()` call to compare
images.

## Audio

```rust,ignore
use genai_rs::{Content, audio_from_file};

let response = client
    .interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_content(vec![
        Content::text("Transcribe this audio"),
        audio_from_file("recording.mp3").await?,
    ])
    .create()
    .await?;

// Or inline
let clip = Content::audio_data(base64_audio, "audio/mp3");
```

Speech recognition is tunable via
`with_transcription_config(TranscriptionConfig::new()...)`:

- BCP-47 `with_language_codes` hints (omit for auto-detect)
- `with_adaptation_phrases` / `with_custom_vocabulary` biasing
- `with_diarization_mode("speaker")`
- `with_timestamp_granularities(["word"])`

These are the SDK-documented value sets, kept as open strings for forward
compatibility. `examples/audio_input.rs` is a runnable demo.

## Video

```rust,ignore
use genai_rs::{Content, video_from_file};
use std::time::Duration;

// From the filesystem
let clip = video_from_file("clip.mp4").await?;

// Inline base64. A clip must yield at least one sampled frame at the
// effective fps (default ~1): a sub-second clip is rejected with a generic
// `400 Request contains an invalid argument` unless you raise the sampling
// rate with `VideoProcessing` (see below).
let inline = Content::video_data(base64_video, "video/mp4");

// Files API, for large videos
let file = client.upload_file("large_video.mp4").await?;
let file = client
    .wait_for_file_ready(&file, Duration::from_secs(2), Duration::from_secs(120))
    .await?;
let by_uri = Content::video_uri(&file.uri, "video/mp4");
```

### Video processing

`Content::with_processing(VideoProcessing)` controls how the model samples a
video (the `processing` field on video content). It is the main lever on
video token cost.

| Value | Wire form | Effect |
|-------|-----------|--------|
| *(not set)* / `VideoProcessing::Static` | omitted / `"static"` | Default frame sampling |
| `VideoProcessing::segment()…build()` | `{"type": "static", "start_offset": "5s", "end_offset": "10s", "fps": 1.0}` | A time window and/or frame rate |
| `VideoProcessing::Agentic` | `"agentic"` | Model-driven exploration of the video |

```rust
use genai_rs::{Content, VideoProcessing};

// Clip a 5-second window and sample one frame per second
let clipped = VideoProcessing::segment()
    .start_offset("5s")
    .end_offset("10s")
    .fps(1.0)
    .build();

let video = Content::video_uri("files/abc123", "video/mp4").with_processing(clipped);
# let _ = video;
```

- **Offsets** are decimal seconds with an `s` suffix (`"10.5s"`). `end_offset`
  must be greater than `start_offset`.
- **Cost.** Measured 2026-08-18 on one source video (`gemini-3.7-flash`),
  the window was what moved video input tokens among the `static` forms
  (57,778 → 16,198). `fps` alone did not move them. `Agentic` reported no
  video tokens at all and billed a varying `image` count instead. The figures
  have changed between measurements, so treat them as dated observations.
  The token table in the `VideoProcessing` rustdoc has the details.
- **Sub-second clips** need a higher `fps` to yield a frame at all (see the
  note above).
- **Input shape.** The API accepts `processing` only when the video sits
  inside a `user_input` step. Both `with_content()` (which the crate sends as
  a single `user_input` step) and `with_history(vec![Step::user_input(..)])`
  satisfy that.

## Documents

```rust,ignore
use genai_rs::{Content, document_from_file};

let pdf = document_from_file("report.pdf").await?;
let notes = document_from_file("notes.md").await?; // text/markdown
let inline_pdf = Content::document_data(base64_pdf, "application/pdf");
let plain = Content::document_data(base64_text, "text/plain");
```

The API accepts `application/pdf`, `text/plain` and `text/markdown` document
content, and `document_from_file` sends those three (`.pdf`, `.txt`, `.md`).
It rejects other text formats (CSV, JSON, HTML, XML) with a pointer to
`Content::text()`; `document_from_file_with_mime` sends any MIME type as-is.

`examples/pdf_input.rs` is a runnable demo.

## Files API

Upload once, then reference by URI across requests.

Path uploads stream from disk with about 8 MB of buffer, so file size (up to
the 2 GB limit) does not drive memory use.

```rust,ignore
// Upload (MIME type from the extension)
let file = client.upload_file("large_video.mp4").await?;

// Explicit MIME type
let file = client.upload_file_with_mime("data.bin", "application/octet-stream").await?;

// From bytes, with an optional display name
let file = client.upload_file_bytes(csv_bytes, "text/csv", Some("Q4 Sales Data")).await?;

// Wait until processing finishes (poll every 2 s, give up after 2 min)
let file = client
    .wait_for_file_ready(&file, Duration::from_secs(2), Duration::from_secs(120))
    .await?;

// Use it
let content = Content::from_file(&file); // URI + MIME type from the metadata

// Inspect, list, delete
let metadata = client.get_file(&file.name).await?;
println!("active={} processing={} failed={}",
    metadata.is_active(), metadata.is_processing(), metadata.is_failed());
for f in client.list_files(None, None).await?.files {
    println!("{} {}", f.name, f.mime_type);
}
client.delete_file(&file.name).await?;
```

`examples/files_api.rs` is a runnable demo.

## Resolution control

`Resolution` trades image and video detail against token cost:
`Low` (lowest cost), `Medium` (the default), `High`, `UltraHigh`.

```rust
use genai_rs::{Content, Resolution};

let quick = Content::image_data("base64...", "image/png").with_resolution(Resolution::Low);
let detailed = Content::image_data("base64...", "image/png").with_resolution(Resolution::High);
# let _ = (quick, detailed);
```

## Content constructors

All are associated functions on `Content`, re-exported from the crate root.
Chain `.with_resolution(..)` onto an image or video to set its resolution.

| Kind | Inline | By URI |
|------|--------|--------|
| Text | `Content::text(s)` | — |
| Image | `image_data(b64, mime)` | `image_uri(uri, mime)` |
| Audio | `audio_data(b64, mime)` | `audio_uri(uri, mime)` |
| Video | `video_data(b64, mime)` | `video_uri(uri, mime)` |
| Document | `document_data(b64, mime)` | `document_uri(uri, mime)` |
| Any | — | `from_file(&FileMetadata)`, `from_uri_and_mime(uri, mime)` |

`Content::Audio` also carries optional `sample_rate` and `channels` fields.
The constructors leave them unset; the API fills them in on audio it returns.

## MIME types the file helpers detect

`image_from_file`, `audio_from_file`, `video_from_file` and
`document_from_file` pick the MIME type from the file extension (use the
`*_with_mime` variants for anything else):

| Kind | Extensions → MIME type |
|------|------------------------|
| Image | `jpg`/`jpeg` → `image/jpeg`, `png`, `gif`, `webp`, `heic`, `heif` |
| Audio | `mp3` → `audio/mp3`, `wav`, `ogg`, `flac`, `aac`, `m4a` |
| Video | `mp4`, `webm`, `mov` → `video/quicktime`, `avi` → `video/x-msvideo`, `mkv` → `video/x-matroska` |
| Document | `pdf` → `application/pdf`, `txt` → `text/plain`, `md` → `text/markdown`, `json`, `csv`, `html`, `xml` |

Detection doesn't guarantee the model accepts a format: `document_from_file`
only sends the PDF, plain-text and Markdown document types (above). Always pass
full MIME types (`"image/png"`, not `"png"`).

## Examples

| Example | Features |
|---------|----------|
| `multimodal_image` | Image input, comparison, resolution control |
| `audio_input` | Audio transcription and analysis |
| `video_input` | Video input |
| `pdf_input` | PDF document processing |
| `files_api` | Upload, list, delete files |

```bash
cargo run --example <name>
```
