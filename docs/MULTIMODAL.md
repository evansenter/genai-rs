# Multimodal Content Guide

This guide covers working with images, audio, video, and documents in `genai-rs`.

## Table of Contents

- [Overview](#overview)
- [Images](#images)
- [Audio](#audio)
- [Video](#video)
- [Documents](#documents)
- [Files API](#files-api)
- [Resolution Control](#resolution-control)
- [Content Constructors](#content-constructors)

## Overview

Gemini supports multimodal inputs through three methods:

| Method | Best For | Size Limit |
|--------|----------|------------|
| Inline base64 | Small files (<20MB) | ~20MB |
| URI reference | Files API uploads | Large files |
| File helpers | Ergonomic file loading | Varies |

Under API revision 2026-05-20, `Content` is purely *data* content: `Text`, `Image`, `Audio`, `Video`, `Document` (plus an `Unknown` fallback). Tool activity (function calls, code execution, search, etc.) and thoughts are `Step` variants in `response.steps`, not `Content`. Use `content.is_image()`, `is_audio()`, `is_video()`, and `is_document()` to check content kinds.

## Images

### Method 1: Content Constructors with with_content() (Recommended)

```rust,ignore
use genai_rs::{Client, Content};

// From base64 data
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("What's in this image?"),
        Content::image_data(base64_string, "image/png"),
    ])
    .create()
    .await?;

// From URI (Files API or public URL)
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Describe the uploaded image"),
        Content::image_uri(&file_metadata.uri, "image/png"),
    ])
    .create()
    .await?;
```

### Method 2: File Helper Functions

```rust,ignore
use genai_rs::{Client, Content, image_from_file};

// Load and encode from filesystem
let image_content = image_from_file("photo.jpg").await?;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("What do you see?"),
        image_content,
    ])
    .create()
    .await?;
```

### Method 3: Multiple Images

```rust,ignore
use genai_rs::{Client, Content};

// Compare multiple images
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Compare these images:"),
        Content::image_data(base64_image1, "image/png"),
        Content::image_data(base64_image2, "image/png"),
    ])
    .create()
    .await?;
```

### Supported Image Formats

| Format | MIME Type |
|--------|-----------|
| PNG | `image/png` |
| JPEG | `image/jpeg` |
| GIF | `image/gif` |
| WebP | `image/webp` |

## Audio

### Input

```rust,ignore
use genai_rs::{Client, Content, audio_from_file};

// From file helper
let audio_content = audio_from_file("recording.mp3").await?;
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Transcribe this audio"),
        audio_content,
    ])
    .create()
    .await?;

// From base64
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("What's being said?"),
        Content::audio_data(base64_audio, "audio/mp3"),
    ])
    .create()
    .await?;
```

### Output (Text-to-Speech)

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-2.5-pro-preview-tts")  // TTS-specific model
    .with_text("Hello, welcome to genai-rs!")
    .with_audio_output()
    .with_voice("Kore")  // Optional voice selection
    .create()
    .await?;

// Get audio data
if let Some(audio) = response.first_audio() {
    let bytes = audio.bytes()?;
    std::fs::write("output.wav", &bytes)?;
    println!("Saved audio: {} bytes", bytes.len());

    // Playback metadata, if reported by the API
    if let Some(rate) = audio.sample_rate() {
        println!("Sample rate: {} Hz", rate);
    }
    if let Some(channels) = audio.channels() {
        println!("Channels: {}", channels);
    }
}

// Iterate multiple audio outputs
for (i, audio) in response.audios().enumerate() {
    let bytes = audio.bytes()?;
    let filename = format!("audio_{}.{}", i, audio.extension());
    std::fs::write(&filename, bytes)?;
}
```

### Supported Audio Formats

| Format | MIME Type |
|--------|-----------|
| MP3 | `audio/mp3` or `audio/mpeg` |
| WAV | `audio/wav` |
| FLAC | `audio/flac` |
| OGG | `audio/ogg` |

## Video

```rust,ignore
use genai_rs::{Client, Content, video_from_file};

// From file helper
let video_content = video_from_file("clip.mp4").await?;
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Describe what happens in this video"),
        video_content,
    ])
    .create()
    .await?;

// From base64
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Summarize this video"),
        Content::video_data(base64_video, "video/mp4"),
    ])
    .create()
    .await?;

// From Files API URI (for large videos)
let file = client.upload_file("large_video.mp4").await?;
let file = client
    .wait_for_file_ready(&file, Duration::from_secs(2), Duration::from_secs(120))
    .await?;

let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("What's in this video?"),
        Content::video_uri(&file.uri, "video/mp4"),
    ])
    .create()
    .await?;
```

### Supported Video Formats

| Format | MIME Type |
|--------|-----------|
| MP4 | `video/mp4` |
| MPEG | `video/mpeg` |
| MOV | `video/quicktime` |
| AVI | `video/x-msvideo` |
| WebM | `video/webm` |

## Documents

### PDF Files

```rust,ignore
use genai_rs::{Client, Content, document_from_file};

// From file helper
let doc_content = document_from_file("report.pdf").await?;
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Summarize this document"),
        doc_content,
    ])
    .create()
    .await?;

// From base64
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Extract key points from this PDF"),
        Content::document_data(base64_pdf, "application/pdf"),
    ])
    .create()
    .await?;
```

### Plain Text

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Analyze this code"),
        Content::document_data(base64_text, "text/plain"),
    ])
    .create()
    .await?;
```

### Supported Document Formats

| Format | MIME Type |
|--------|-----------|
| PDF | `application/pdf` |
| Plain Text | `text/plain` |
| HTML | `text/html` |
| CSV | `text/csv` |
| Markdown | `text/markdown` |

**Example**: `cargo run --example pdf_input`

## Files API

For files >20MB or when you need to reuse content across requests.

### Upload

```rust,ignore
// Simple upload
let file = client.upload_file("large_video.mp4").await?;
println!("Uploaded: {}", file.name);
println!("URI: {}", file.uri);

// With explicit MIME type (when extension-based detection isn't suitable)
let file = client
    .upload_file_with_mime("data.bin", "application/octet-stream")
    .await?;

// From bytes in memory, with an optional display name
let file = client
    .upload_file_bytes(csv_bytes, "text/csv", Some("Q4 Sales Data"))
    .await?;

// Chunked (streaming) upload for very large files.
// Returns the metadata plus a resume handle for interrupted uploads.
let (file, _resume_handle) = client.upload_file_chunked("huge_video.mp4").await?;

// Custom chunk size (default: 8MB)
let (file, _resume_handle) = client
    .upload_file_chunked_with_options("huge_video.mp4", "video/mp4", 16 * 1024 * 1024)
    .await?;
```

### Wait for Processing

Videos and some documents require processing time:

```rust,ignore
// Poll every 2 seconds, waiting up to 2 minutes for the file to be ready
let file = client
    .wait_for_file_ready(&file, Duration::from_secs(2), Duration::from_secs(120))
    .await?;

// Or check state manually
let metadata = client.get_file(&file.name).await?;
if metadata.is_active() {
    println!("Ready to use");
} else if metadata.is_processing() {
    println!("Still processing...");
} else if metadata.is_failed() {
    println!("Processing failed");
}
```

### Use in Requests

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("Analyze this file"),
        Content::from_file(&file),
    ])
    .create()
    .await?;
```

### List and Delete

```rust,ignore
// List all uploaded files (page_size, page_token)
let response = client.list_files(None, None).await?;
for file in response.files {
    println!(
        "{}: {} ({})",
        file.name,
        file.display_name.as_deref().unwrap_or(""),
        file.mime_type
    );
}

// Delete a file
client.delete_file(&file.name).await?;
```

**Example**: `cargo run --example files_api`

## Resolution Control

Control the trade-off between image quality and token cost.

### Resolution Levels

| Level | Use Case | Token Cost |
|-------|----------|------------|
| `Low` | Simple detection (colors, shapes) | Lowest |
| `Medium` | General analysis | Moderate |
| `High` | Detailed inspection | Higher |
| `UltraHigh` | Maximum detail | Highest |

### Usage

```rust,ignore
use genai_rs::{Client, Content, Resolution};

// With resolution using builder method
let response = client
    .interaction()
    .with_model("gemini-3-flash-preview")
    .with_content(vec![
        Content::text("What color is this?"),
        Content::image_data(base64, "image/png").with_resolution(Resolution::Low),
    ])
    .create()
    .await?;

// Using constructor with resolution
let content = Content::image_data_with_resolution(
    base64,
    "image/png",
    Resolution::High
);
```

### When to Use Each

| Resolution | Scenario |
|------------|----------|
| Low | Color detection, presence/absence checks |
| Medium | General image description (default) |
| High | Text reading, fine details |
| UltraHigh | Medical imaging, technical diagrams |

## Content Constructors

All constructors are static methods on `Content`, re-exported from the crate root.

### Text

```rust,ignore
use genai_rs::Content;

let content = Content::text("Analyze the following:");
```

### Images

```rust,ignore
use genai_rs::{Content, Resolution};

// Inline base64
let content = Content::image_data(base64, "image/png");
let content = Content::image_data_with_resolution(base64, "image/png", Resolution::High);

// URI reference
let content = Content::image_uri(uri, "image/png");
let content = Content::image_uri_with_resolution(uri, "image/png", Resolution::High);
```

### Audio

```rust,ignore
use genai_rs::Content;

let content = Content::audio_data(base64, "audio/mp3");
let content = Content::audio_uri(uri, "audio/mp3");
```

`Content::Audio` also carries optional `sample_rate` and `channels` fields. The constructors leave them unset; the API populates them on audio it returns (see `AudioInfo::sample_rate()` / `channels()` on responses).

### Video

```rust,ignore
use genai_rs::{Content, Resolution};

let content = Content::video_data(base64, "video/mp4");
let content = Content::video_data_with_resolution(base64, "video/mp4", Resolution::High);
let content = Content::video_uri(uri, "video/mp4");
let content = Content::video_uri_with_resolution(uri, "video/mp4", Resolution::High);
```

### Documents

```rust,ignore
use genai_rs::Content;

let content = Content::document_data(base64, "application/pdf");
let content = Content::document_uri(uri, "application/pdf");
```

### From File Metadata

```rust,ignore
use genai_rs::Content;

let file = client.upload_file("document.pdf").await?;
let content = Content::from_file(&file);
```

### From Any URI

```rust,ignore
use genai_rs::Content;

// Generic URI + MIME type
let content = Content::from_uri_and_mime(uri, "video/mp4");
```

## Image Generation

Generate images from text prompts.

```rust,ignore
let response = client
    .interaction()
    .with_model("gemini-3-pro-image-preview")  // Image generation model
    .with_text("A sunset over mountains, digital art style")
    .with_image_output()
    .create()
    .await?;

// Get the first generated image
if let Some(bytes) = response.first_image_bytes()? {
    std::fs::write("generated.png", &bytes)?;
}

// Check for multiple images
if response.has_images() {
    for (i, image) in response.images().enumerate() {
        let bytes = image.bytes()?;
        let filename = format!("image_{}.{}", i, image.extension());
        std::fs::write(&filename, bytes)?;
    }
}
```

**Example**: `cargo run --example image_generation`

## Examples

| Example | Features |
|---------|----------|
| `multimodal_image` | Image input, comparison, resolution control |
| `audio_input` | Audio transcription and analysis |
| `pdf_input` | PDF document processing |
| `files_api` | Upload, list, delete files |
| `image_generation` | Text-to-image generation |

Run with:
```bash
cargo run --example <name>
```
