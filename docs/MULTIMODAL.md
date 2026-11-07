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

`with_content(vec![...])` sends one multimodal turn. With `with_history()`
it is the turn after the history; earlier multimodal turns go in the history
as `Step::user_input(...)`. See [Builder API](BUILDER_API.md#input-methods).

## Images

```rust,no_run
use genai_rs::{Content, image_from_file};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
# let base64_string = String::new();
# let file_metadata: genai_rs::FileMetadata = unimplemented!();
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
# let _ = (response, photo, uploaded);
# Ok(())
# }
```

Pass several `Content::image_*` blocks in one `with_content()` call to compare
images.

## Audio

```rust,no_run
use genai_rs::{Content, audio_from_file};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
# let base64_audio = String::new();
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
# let _ = (response, clip);
# Ok(())
# }
```

### Transcription

`DEFAULT_TRANSCRIPTION_MODEL` is the speech-to-text model: it answers audio
with its transcript, ignores a text prompt beside the audio, answers
text-only input with nothing, and rejects function tools. Other models
transcribe when asked in the prompt, like any other task.

Speech recognition is tunable via
`with_transcription_config(TranscriptionConfig::new()...)`, which takes
effect on the transcription model (general models accept it without
effect):

- BCP-47 `with_language_codes` hints (omit for auto-detect)
- `with_custom_vocabulary` biasing
- `with_mode(TranscriptionMode::Verbatim { diarization_mode, timestamp_granularities })`
  for speaker diarization (`"speaker"`) and word timestamps (`"word"`): the
  transcript's text block carries an `Annotation::WordInfo` per word, with
  its byte span, `start_offset` / `end_offset` and speaker
- `with_mode(TranscriptionMode::Smart { source_text })`, where
  `source_text` is the script the speaker read: the transcript follows its
  wording and spelling where the audio agrees

`with_adaptation_phrases`, `with_diarization_mode` and
`with_timestamp_granularities` still work but are deprecated in the
google-genai bindings in favor of `with_mode`.

These are the SDK-documented value sets, kept as open strings for forward
compatibility. `examples/audio_input.rs` is a runnable demo; the live record
is under Models in
[INTERACTIONS_API_GAP.md](INTERACTIONS_API_GAP.md#models).

## Video

```rust,no_run
use genai_rs::{Content, FileUpload, PollOptions, video_from_file};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
# let base64_video = String::new();
// From the filesystem
let clip = video_from_file("clip.mp4").await?;

// Inline base64. A clip must yield at least one sampled frame at the
// effective fps (default ~1): a sub-second clip is rejected with a generic
// `400 Request contains an invalid argument` unless you raise the sampling
// rate with `VideoProcessing` (see below).
let inline = Content::video_data(base64_video, "video/mp4");

// Files API, for large videos
let file = client.files().upload(FileUpload::from_path("large_video.mp4")).await?;
let file = client.files().wait_until_active(&file.name, PollOptions::new()).await?;
let by_uri = Content::video_uri(&file.uri, "video/mp4");
# let _ = (clip, inline, by_uri);
# Ok(())
# }
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
    .with_start_offset("5s")
    .with_end_offset("10s")
    .with_fps(1.0)
    .build();

let video = Content::video_uri("files/abc123", "video/mp4").with_processing(clipped);
# let _ = video;
```

- **Offsets** are decimal seconds with an `s` suffix (`"10.5s"`). `end_offset`
  must be greater than `start_offset`.
- **Cost.** The window is what moves video input tokens among the
  `static` forms; `fps` alone does not. `Agentic` reports no video tokens at
  all and bills a varying `image` count instead. The figures have changed
  between measurements; the measured token counts, with their dates, are
  under VideoProcessing in [Enum Wire Formats](ENUM_WIRE_FORMATS.md#videoprocessing).
- **Sub-second clips** need a higher `fps` to yield a frame at all (see the
  note above).
- **Input shape.** The API accepts `processing` only when the video sits
  inside a `user_input` step. Both `with_content()` (which the crate sends as
  a single `user_input` step) and `with_history(vec![Step::user_input(..)])`
  satisfy that.

## Documents

```rust,no_run
use genai_rs::{Content, document_from_file};

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
# let base64_pdf = String::new();
let pdf = document_from_file("report.pdf").await?;
let sheet = document_from_file("inventory.csv").await?; // text/csv document
let notes = document_from_file("notes.md").await?; // sent as Content::text
let inline_pdf = Content::document_data(base64_pdf, "application/pdf");
# let _ = (pdf, sheet, notes, inline_pdf);
# Ok(())
# }
```

`document_from_file` sends `.pdf` files as `application/pdf` document
content, and `.txt` / `.md` files as text content with the file's UTF-8
contents, because the API rejects `text/plain` and `text/markdown`
document content with a 400 (#514).

It also sends CSV (`.csv`) and Office/OpenDocument files (`.docx`, `.xlsx`,
`.pptx`, `.odt`, `.ods`, `.odp`, and the legacy `.doc`, `.xls`, `.ppt`) as
document content: with PDF, these are the types the API accepts inline (for
the legacy `.doc`, `.xls` and `.ppt` only the MIME type was checked, not
that a real legacy file parses). Their MIME types are in the
[table below](#mime-types-the-file-helpers-detect).

The API rejects every other inline document type with a 400
(`Unsupported mime_type '...' in document content`), `text/html`,
`application/json`, `application/xml` and `application/rtf` included.
`document_from_file` rejects `.json`, `.html` and `.xml` locally with a
pointer to `Content::text()`; `document_from_file_with_mime` sends any MIME
type as-is.

The restriction is on inline document content only: a `.txt` or `.md` file
uploaded to the [Files API](#files-api) still works as a `text/plain` or
`text/markdown` document by URI (`Content::from_file`). The live record of
these rules is under Requests in the
[gap tracker](INTERACTIONS_API_GAP.md#requests).

`examples/pdf_input.rs` is a runnable demo.

## Files API

Upload once, then reference by URI across requests.

Path uploads stream from disk with about 8 MB of buffer, so file size (up to
the 2 GB limit) does not drive memory use.

```rust,no_run
use futures_util::TryStreamExt;
use genai_rs::{Content, FileMetadata, FileUpload, PollOptions};
use std::time::Duration;

# async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
# let csv_bytes = b"region,total\nnorth,12\n".to_vec();
// Upload (MIME type from the extension, display name from the file name)
let file = client.files().upload(FileUpload::from_path("large_video.mp4")).await?;

// Explicit MIME type
let data = client
    .files()
    .upload(FileUpload::from_path("data.bin").with_mime_type("application/octet-stream"))
    .await?;

// From bytes, with an optional display name
let sales = client
    .files()
    .upload(FileUpload::from_bytes(csv_bytes, "text/csv").with_display_name("Q4 Sales Data"))
    .await?;

// Wait until processing finishes. By default it polls every 2 s and gives
// up after 2 min; `PollOptions::new()` alone keeps both defaults.
let poll = PollOptions::new().with_timeout(Duration::from_secs(300));
let file = client.files().wait_until_active(&file.name, poll).await?;

// Use it
let content = Content::from_file(&file); // URI + MIME type from the metadata

// Inspect, list, delete
let metadata = client.files().get(&file.name).await?;
println!("active={} processing={} failed={}",
    metadata.is_active(), metadata.is_processing(), metadata.is_failed());
for f in client.files().list().send().await?.files {
    println!("{} {}", f.name, f.mime_type);
}
let every_file: Vec<FileMetadata> = client.files().list().items().try_collect().await?;
client.files().delete(&file.name).await?;
# let _ = (data, sales, content, every_file);
# Ok(())
# }
```

The list is newest first. `send()` returns one page; `items()` streams every
file across pages (`with_page_size` takes 1 to 100).

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
| Document | `pdf` → `application/pdf`, `csv` → `text/csv`, `doc` → `application/msword`, `docx` → `application/vnd.openxmlformats-officedocument.wordprocessingml.document`, `dotx` → `application/vnd.openxmlformats-officedocument.wordprocessingml.template`, `xls` → `application/vnd.ms-excel`, `xlsx` → `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`, `ppt` → `application/vnd.ms-powerpoint`, `pptx` → `application/vnd.openxmlformats-officedocument.presentationml.presentation`, `odt` / `ods` / `odp` → `application/vnd.oasis.opendocument.text` / `.spreadsheet` / `.presentation` |
| Text | `txt` → `text/plain`, `md` → `text/markdown`, `json`, `html`, `xml` |

Detection doesn't guarantee the API accepts a format inline:
`document_from_file` sends the Document row as document content, sends
`.txt` / `.md` files as text (above), and rejects `json`, `html` and
`xml`. `FileUpload::from_path` uses
the same detection for every row. Always pass full MIME types (`"image/png"`,
not `"png"`).

## Examples

| Example | Features |
|---------|----------|
| `multimodal_image` | Image input, comparison, resolution control |
| `audio_input` | Transcription on `DEFAULT_TRANSCRIPTION_MODEL`: word timings and `source_text` |
| `video_input` | Video input |
| `pdf_input` | PDF document processing |
| `files_api` | Upload, list, delete files |

```bash
cargo run --example <name>
```
