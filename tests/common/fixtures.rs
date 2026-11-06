//! Fixture data: sample media, minimal base64 files, and the function
//! declarations several tests share.

// =============================================================================
// Test Asset URLs
// =============================================================================

/// A `gs://` URI. The Interactions API rejects these with 400; one test pins that.
#[allow(dead_code)]
pub const SAMPLE_IMAGE_URL: &str = "gs://cloud-samples-data/generative-ai/image/scones.jpg";

/// Public YouTube video URI the Interactions API accepts directly (verified live
/// 2026-08-16), for tests that need the model to actually ingest video.
#[allow(dead_code)]
pub const SAMPLE_YOUTUBE_VIDEO_URL: &str = "https://www.youtube.com/watch?v=aqz-KE-bpKQ";

/// Small 1x1 red PNG image encoded as base64
/// This is a minimal valid PNG for testing base64 image input
#[allow(dead_code)]
pub const TINY_RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

/// Small WAV audio clip: 16-bit mono 44.1kHz, 100 frames (~2ms) of silence.
/// The data chunk must be non-empty — as of 2026-08 the API rejects
/// zero-length audio streams with 400 invalid_request.
#[allow(dead_code)]
pub const TINY_WAV_BASE64: &str = "UklGRuwAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YcgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

/// A 1-second 64x64 red H.264 clip (~1.7KB). Shorter clips yield no sampled
/// frame at ~1 fps and are rejected with 400 invalid_request.
#[allow(dead_code)]
pub const TINY_MP4_BASE64: &str = "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";

/// Small 1x1 blue PNG image encoded as base64
/// This is a minimal valid PNG for testing multi-image comparisons
#[allow(dead_code)]
pub const TINY_BLUE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// Minimal PDF document containing "Hello World" text
/// This is a complete valid PDF for testing document input
#[allow(dead_code)]
pub const TINY_PDF_BASE64: &str = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA3MiA3Ml0gL0NvbnRlbnRzIDQgMCBSIC9SZXNvdXJjZXMgPDwgPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA0NCA+PgpzdHJlYW0KQlQgL0YxIDEyIFRmIDEwIDUwIFRkIChIZWxsbyBXb3JsZCkgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgNQowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAyMjQgMDAwMDAgbiAKdHJhaWxlcgo8PCAvU2l6ZSA1IC9Sb290IDEgMCBSID4+CnN0YXJ0eHJlZgozMjAKJSVFT0Y=";

// =============================================================================
// Function Declaration Builders
// =============================================================================

/// The canonical `get_weather(city)` declaration.
#[allow(dead_code)]
pub fn get_weather_function() -> genai_rs::FunctionDeclaration {
    use serde_json::json;
    genai_rs::FunctionDeclaration::builder("get_weather")
        .with_description("Get the current weather for a city")
        .add_parameter(
            "city",
            json!({"type": "string", "description": "City name"}),
        )
        .with_required(vec!["city".to_string()])
        .build()
}

/// The canonical `get_time(timezone)` declaration.
#[allow(dead_code)]
pub fn get_time_function() -> genai_rs::FunctionDeclaration {
    use serde_json::json;
    genai_rs::FunctionDeclaration::builder("get_time")
        .with_description("Get the current time in a timezone")
        .add_parameter(
            "timezone",
            json!({"type": "string", "description": "Timezone like PST, EST, JST"}),
        )
        .with_required(vec!["timezone".to_string()])
        .build()
}
