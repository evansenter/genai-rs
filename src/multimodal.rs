//! Multimodal content utilities for file loading and MIME type detection.
//!
//! This module provides convenience functions for working with multimodal content
//! (images, audio, video, documents) by handling file loading, base64 encoding,
//! and MIME type detection automatically.
//!
//! # File Size Recommendations
//!
//! The `*_from_file()` functions load files entirely into memory before base64
//! encoding. This approach works well for typical use cases but has memory
//! implications for large files:
//!
//! - **Memory usage**: The file is loaded into memory, then base64 encoded
//!   (which increases size by ~33%), resulting in temporary peak memory usage
//!   of approximately 2.3× the original file size
//! - **Recommended limit**: ~20MB per file for inline data
//! - **API limits**: The Gemini API has its own limits on inline data size
//!
//! ## Handling Large Files
//!
//! For files larger than 20MB, consider these alternatives:
//!
//! 1. **URI-based methods**: Upload files to Google Cloud Storage and use
//!    `Content::from_uri_and_mime()` with `with_content()`:
//!
//!    ```no_run
//!    # use genai_rs::{Client, Content};
//!    # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!    # let client = Client::new("key".to_string());
//!    let response = client
//!        .interaction()
//!        .with_model(genai_rs::DEFAULT_MODEL)
//!        .with_content(vec![
//!            Content::text("Describe this video"),
//!            Content::from_uri_and_mime("gs://bucket/large-video.mp4", "video/mp4"),
//!        ])
//!        .create()
//!        .await?;
//!    # Ok(())
//!    # }
//!    ```
//!
//! 2. **Files API**: For files that need to be referenced across multiple
//!    interactions, the Files API allows uploading once and referencing by URI:
//!
//!    ```no_run
//!    # use genai_rs::{Client, Content, FileUpload};
//!    # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!    # let client = Client::new("key".to_string());
//!    let file = client
//!        .files()
//!        .upload(FileUpload::from_path("large-video.mp4"))
//!        .await?;
//!    let response = client
//!        .interaction()
//!        .with_model(genai_rs::DEFAULT_MODEL)
//!        .with_content(vec![
//!            Content::text("Describe this video"),
//!            Content::from_file(&file),
//!        ])
//!        .create()
//!        .await?;
//!    # Ok(())
//!    # }
//!    ```
//!
//! # Example
//!
//! ```no_run
//! use genai_rs::{Client, Content, image_from_file};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new("api-key".to_string());
//!
//! // Load image from file with automatic MIME detection
//! let image = image_from_file("photo.jpg").await?;
//!
//! let response = client
//!     .interaction()
//!     .with_model(genai_rs::DEFAULT_MODEL)
//!     .with_content(vec![
//!         Content::text("What's in this image?"),
//!         image,
//!     ])
//!     .create()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use crate::Content;
use crate::errors::GenaiError;
use base64::Engine;
use std::path::Path;

/// Detects MIME type from file extension.
///
/// Returns the MIME type as a static string if the extension is recognized,
/// or `None` for unsupported or missing extensions.
///
/// # Supported Types
///
/// ## Images
/// - `jpg`, `jpeg` → `image/jpeg`
/// - `png` → `image/png`
/// - `gif` → `image/gif`
/// - `webp` → `image/webp`
/// - `heic` → `image/heic`
/// - `heif` → `image/heif`
///
/// ## Audio
/// - `mp3` → `audio/mp3`
/// - `wav` → `audio/wav`
/// - `ogg` → `audio/ogg`
/// - `flac` → `audio/flac`
/// - `aac` → `audio/aac`
/// - `m4a` → `audio/m4a`
///
/// ## Video
/// - `mp4` → `video/mp4`
/// - `webm` → `video/webm`
/// - `mov` → `video/quicktime`
/// - `avi` → `video/x-msvideo`
/// - `mkv` → `video/x-matroska`
///
/// ## Documents
/// - `pdf` → `application/pdf`
/// - `csv` → `text/csv`
/// - `doc` → `application/msword`
/// - `docx` → `application/vnd.openxmlformats-officedocument.wordprocessingml.document`
/// - `dotx` → `application/vnd.openxmlformats-officedocument.wordprocessingml.template`
/// - `xls` → `application/vnd.ms-excel`
/// - `xlsx` → `application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`
/// - `ppt` → `application/vnd.ms-powerpoint`
/// - `pptx` → `application/vnd.openxmlformats-officedocument.presentationml.presentation`
/// - `odt` → `application/vnd.oasis.opendocument.text`
/// - `ods` → `application/vnd.oasis.opendocument.spreadsheet`
/// - `odp` → `application/vnd.oasis.opendocument.presentation`
///
/// ## Text
/// - `txt` → `text/plain`
/// - `md` → `text/markdown`
/// - `json` → `application/json`
/// - `html` → `text/html`
/// - `xml` → `application/xml`
///
/// # Example
///
/// ```
/// use std::path::Path;
/// use genai_rs::detect_mime_type;
///
/// assert_eq!(detect_mime_type(Path::new("photo.jpg")), Some("image/jpeg"));
/// assert_eq!(detect_mime_type(Path::new("audio.mp3")), Some("audio/mp3"));
/// assert_eq!(detect_mime_type(Path::new("unknown.xyz")), None);
/// ```
#[must_use]
pub fn detect_mime_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        // Images
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),
        // Audio
        "mp3" => Some("audio/mp3"),
        "wav" => Some("audio/wav"),
        "ogg" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        "aac" => Some("audio/aac"),
        "m4a" => Some("audio/m4a"),
        // Video
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        "avi" => Some("video/x-msvideo"),
        "mkv" => Some("video/x-matroska"),
        // Documents
        "pdf" => Some("application/pdf"),
        "csv" => Some("text/csv"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "dotx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.template"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        // Text
        "txt" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "json" => Some("application/json"),
        "html" => Some("text/html"),
        "xml" => Some("application/xml"),
        _ => None,
    }
}

/// Internal helper to load and encode a file.
///
/// Warns if file size exceeds 20MB (the recommended threshold for inline data).
/// For large files, consider using URI-based content or the Files API.
async fn load_and_encode_file(path: impl AsRef<Path>) -> Result<String, GenaiError> {
    const LARGE_FILE_THRESHOLD: u64 = 20 * 1024 * 1024; // 20MB

    let path = path.as_ref();

    // Check file size and warn if large (don't fail - let users proceed if they want)
    if let Ok(metadata) = tokio::fs::metadata(path).await
        && metadata.len() > LARGE_FILE_THRESHOLD
    {
        tracing::warn!(
            "File '{}' is {:.1}MB which exceeds the recommended 20MB limit for inline data. \
             Consider using URI-based content (e.g., gs:// URLs) or the Files API for large files \
             to reduce memory usage. See: https://ai.google.dev/gemini-api/docs/files",
            path.display(),
            metadata.len() as f64 / (1024.0 * 1024.0)
        );
    }

    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| file_read_error(path, &e))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// Reads a plain-text file as UTF-8, with the same read-error guidance as the
/// binary loaders.
async fn read_text_file(path: &Path) -> Result<String, GenaiError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| file_read_error(path, &e))?;
    String::from_utf8(bytes).map_err(|_| {
        GenaiError::InvalidInput(format!(
            "File '{}' is not valid UTF-8. document_from_file() sends .txt and .md files \
             as text content; convert the file to UTF-8, or send it with \
             document_from_file_with_mime() if it is really another format.",
            path.display()
        ))
    })
}

/// Maps a file read failure to `InvalidInput`, with a hint for the common causes.
fn file_read_error(path: &Path, e: &std::io::Error) -> GenaiError {
    let suggestion = match e.kind() {
        std::io::ErrorKind::NotFound => " Check that the file path is correct.",
        std::io::ErrorKind::PermissionDenied => " Check file permissions.",
        _ => "",
    };
    GenaiError::InvalidInput(format!(
        "Failed to read file '{}': {}.{}",
        path.display(),
        e,
        suggestion
    ))
}

/// Helper to get extension-specific error information
fn get_extension_info(path: &Path) -> Result<String, GenaiError> {
    let ext_osstr = path.extension().ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "File '{}' has no extension. Cannot auto-detect MIME type. \
             Use the *_with_mime() variant to specify the type explicitly.",
            path.display()
        ))
    })?;

    ext_osstr.to_str().map(str::to_lowercase).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "File extension for '{}' contains invalid UTF-8. \
             Use the *_with_mime() variant to specify the type explicitly.",
            path.display()
        ))
    })
}

/// Validates that the detected MIME type matches the expected category
fn validate_mime_category(
    mime_type: &str,
    expected_prefix: &str,
    path: &Path,
    category_name: &str,
) -> Result<(), GenaiError> {
    if !mime_type.starts_with(expected_prefix) {
        let suggestion = if mime_type.starts_with("image/") {
            "image_from_file()"
        } else if mime_type.starts_with("audio/") {
            "audio_from_file()"
        } else if mime_type.starts_with("video/") {
            "video_from_file()"
        } else if DOCUMENT_MIME_TYPES.contains(&mime_type) {
            "document_from_file()"
        } else if TEXT_FILE_MIME_TYPES.contains(&mime_type) {
            "document_from_file(), which sends .txt and .md files as text content"
        } else if mime_type.starts_with("text/")
            || matches!(mime_type, "application/json" | "application/xml")
        {
            // document_from_file() rejects these, so don't point at it.
            "Content::text() with the file contents read via std::fs::read_to_string()"
        } else {
            "the appropriate *_from_file() function"
        };

        return Err(GenaiError::InvalidInput(format!(
            "File '{}' has MIME type '{}' which is not {} type. Did you mean to use {}?",
            path.display(),
            mime_type,
            category_name,
            suggestion
        )));
    }
    Ok(())
}

/// Loads an image from a file with automatic MIME type detection.
///
/// Reads the file, encodes it as base64, and detects the MIME type from
/// the file extension.
///
/// # Supported Formats
///
/// | Extension | MIME Type |
/// |-----------|-----------|
/// | `.jpg`, `.jpeg` | `image/jpeg` |
/// | `.png` | `image/png` |
/// | `.gif` | `image/gif` |
/// | `.webp` | `image/webp` |
/// | `.heic` | `image/heic` |
/// | `.heif` | `image/heif` |
///
/// # Arguments
///
/// * `path` - Path to the image file
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if:
/// - The file cannot be read (with a suggestion based on the error type)
/// - The file has no extension
/// - The extension is not recognized as an image type
/// - The extension maps to a non-image MIME type (e.g., `.mp3`)
///
/// For unsupported extensions, use [`image_from_file_with_mime()`] to specify
/// the MIME type explicitly.
///
/// # Example
///
/// ```no_run
/// use genai_rs::image_from_file;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let image = image_from_file("photo.jpg").await?;
/// # Ok(())
/// # }
/// ```
pub async fn image_from_file(path: impl AsRef<Path>) -> Result<Content, GenaiError> {
    let path = path.as_ref();

    // Get extension with specific error handling
    let ext = get_extension_info(path)?;

    // Try to detect MIME type
    let mime_type = detect_mime_type(path).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "Unsupported image extension '.{}' for file '{}'. \
             Supported extensions: jpg, jpeg, png, gif, webp, heic, heif. \
             Use image_from_file_with_mime() to override.",
            ext,
            path.display()
        ))
    })?;

    // Validate this is actually an image MIME type
    validate_mime_category(mime_type, "image/", path, "an image")?;

    image_from_file_with_mime(path, mime_type).await
}

/// Loads an image from a file with an explicit MIME type.
///
/// Use this when you need to override the auto-detected MIME type.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if the file cannot be read (with a
/// suggestion based on the error type).
///
/// # Example
///
/// ```no_run
/// use genai_rs::image_from_file_with_mime;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let image = image_from_file_with_mime("image.raw", "image/png").await?;
/// # Ok(())
/// # }
/// ```
pub async fn image_from_file_with_mime(
    path: impl AsRef<Path>,
    mime_type: impl Into<String>,
) -> Result<Content, GenaiError> {
    let data = load_and_encode_file(&path).await?;
    Ok(Content::image_data(data, mime_type))
}

/// Loads an audio file with automatic MIME type detection.
///
/// Reads the file, encodes it as base64, and detects the MIME type from
/// the file extension.
///
/// # Supported Formats
///
/// | Extension | MIME Type |
/// |-----------|-----------|
/// | `.mp3` | `audio/mp3` |
/// | `.wav` | `audio/wav` |
/// | `.ogg` | `audio/ogg` |
/// | `.flac` | `audio/flac` |
/// | `.aac` | `audio/aac` |
/// | `.m4a` | `audio/m4a` |
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if:
/// - The file cannot be read (with a suggestion based on the error type)
/// - The file has no extension
/// - The extension is not recognized as an audio type
/// - The extension maps to a non-audio MIME type (e.g., `.jpg`)
///
/// For unsupported extensions, use [`audio_from_file_with_mime()`] to specify
/// the MIME type explicitly.
///
/// # Example
///
/// ```no_run
/// use genai_rs::audio_from_file;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let audio = audio_from_file("recording.mp3").await?;
/// # Ok(())
/// # }
/// ```
pub async fn audio_from_file(path: impl AsRef<Path>) -> Result<Content, GenaiError> {
    let path = path.as_ref();

    // Get extension with specific error handling
    let ext = get_extension_info(path)?;

    // Try to detect MIME type
    let mime_type = detect_mime_type(path).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "Unsupported audio extension '.{}' for file '{}'. \
             Supported extensions: mp3, wav, ogg, flac, aac, m4a. \
             Use audio_from_file_with_mime() to override.",
            ext,
            path.display()
        ))
    })?;

    // Validate this is actually an audio MIME type
    validate_mime_category(mime_type, "audio/", path, "an audio")?;

    audio_from_file_with_mime(path, mime_type).await
}

/// Loads an audio file with an explicit MIME type.
///
/// Use this when you need to override the auto-detected MIME type,
/// such as for files with non-standard extensions.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if the file cannot be read (with a
/// suggestion based on the error type).
///
/// # Example
///
/// ```no_run
/// use genai_rs::audio_from_file_with_mime;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Load a raw audio file with explicit MIME type
/// let audio = audio_from_file_with_mime("audio.raw", "audio/wav").await?;
/// # Ok(())
/// # }
/// ```
pub async fn audio_from_file_with_mime(
    path: impl AsRef<Path>,
    mime_type: impl Into<String>,
) -> Result<Content, GenaiError> {
    let data = load_and_encode_file(&path).await?;
    Ok(Content::audio_data(data, mime_type))
}

/// Loads a video file with automatic MIME type detection.
///
/// Reads the file, encodes it as base64, and detects the MIME type from
/// the file extension.
///
/// # Supported Formats
///
/// | Extension | MIME Type |
/// |-----------|-----------|
/// | `.mp4` | `video/mp4` |
/// | `.webm` | `video/webm` |
/// | `.mov` | `video/quicktime` |
/// | `.avi` | `video/x-msvideo` |
/// | `.mkv` | `video/x-matroska` |
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if:
/// - The file cannot be read (with a suggestion based on the error type)
/// - The file has no extension
/// - The extension is not recognized as a video type
/// - The extension maps to a non-video MIME type (e.g., `.jpg`)
///
/// For unsupported extensions, use [`video_from_file_with_mime()`] to specify
/// the MIME type explicitly.
///
/// # Example
///
/// ```no_run
/// use genai_rs::video_from_file;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let video = video_from_file("clip.mp4").await?;
/// # Ok(())
/// # }
/// ```
pub async fn video_from_file(path: impl AsRef<Path>) -> Result<Content, GenaiError> {
    let path = path.as_ref();

    // Get extension with specific error handling
    let ext = get_extension_info(path)?;

    // Try to detect MIME type
    let mime_type = detect_mime_type(path).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "Unsupported video extension '.{}' for file '{}'. \
             Supported extensions: mp4, webm, mov, avi, mkv. \
             Use video_from_file_with_mime() to override.",
            ext,
            path.display()
        ))
    })?;

    // Validate this is actually a video MIME type
    validate_mime_category(mime_type, "video/", path, "a video")?;

    video_from_file_with_mime(path, mime_type).await
}

/// Loads a video file with an explicit MIME type.
///
/// Use this when you need to override the auto-detected MIME type,
/// such as for files with non-standard extensions.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if the file cannot be read (with a
/// suggestion based on the error type).
///
/// # Example
///
/// ```no_run
/// use genai_rs::video_from_file_with_mime;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Load a video file with explicit MIME type
/// let video = video_from_file_with_mime("video.raw", "video/mp4").await?;
/// # Ok(())
/// # }
/// ```
pub async fn video_from_file_with_mime(
    path: impl AsRef<Path>,
    mime_type: impl Into<String>,
) -> Result<Content, GenaiError> {
    let data = load_and_encode_file(&path).await?;
    Ok(Content::video_data(data, mime_type))
}

/// MIME types [`document_from_file`] sends as document content: the
/// bindings' `DocumentContentMimeType` list, which the Interactions API
/// accepts inline, each read back from a real file except the legacy `.xls`
/// and `.ppt`, which only passed the API's MIME check with plain-text bytes.
/// The API rejects other inline document types with `400 Unsupported
/// mime_type`. The dated record is under Requests in
/// `docs/INTERACTIONS_API_GAP.md`.
const DOCUMENT_MIME_TYPES: &[&str] = &[
    "application/pdf",
    "text/csv",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.presentation",
];

/// MIME types [`document_from_file`] reads as UTF-8 and sends as text content.
/// The API rejects them as document content with a 400 ("Unsupported
/// mime_type ... in document content", #514).
const TEXT_FILE_MIME_TYPES: &[&str] = &["text/plain", "text/markdown"];

/// Loads a document file with automatic MIME type detection.
///
/// # Supported Formats
///
/// | Extension | Sent as |
/// |-----------|---------|
/// | `.pdf` | Document content (`application/pdf`) |
/// | `.csv` | Document content (`text/csv`) |
/// | `.docx` | Document content (`application/vnd.openxmlformats-officedocument.wordprocessingml.document`) |
/// | `.dotx` | Document content (`application/vnd.openxmlformats-officedocument.wordprocessingml.template`) |
/// | `.xlsx` | Document content (`application/vnd.openxmlformats-officedocument.spreadsheetml.sheet`) |
/// | `.pptx` | Document content (`application/vnd.openxmlformats-officedocument.presentationml.presentation`) |
/// | `.odt` | Document content (`application/vnd.oasis.opendocument.text`) |
/// | `.ods` | Document content (`application/vnd.oasis.opendocument.spreadsheet`) |
/// | `.odp` | Document content (`application/vnd.oasis.opendocument.presentation`) |
/// | `.doc` | Document content (`application/msword`) |
/// | `.xls` | Document content (`application/vnd.ms-excel`) |
/// | `.ppt` | Document content (`application/vnd.ms-powerpoint`) |
/// | `.txt`, `.md` | Text content ([`Content::text()`]) with the file's UTF-8 contents |
///
/// Document content is base64-encoded. These are the document types the
/// Interactions API accepts inline (each read back from a real file; for
/// `.xls` and `.ppt`, only that the API accepts the MIME type, not that it
/// parses a real legacy file).
///
/// The Interactions API rejects `text/plain` and `text/markdown` as document
/// content, so plain-text files are sent as text instead; the model reads them
/// the same way.
///
/// The API also rejects JSON, HTML and XML documents. Read such a file and
/// send it as [`Content::text()`].
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if:
/// - The file cannot be read (with a suggestion based on the error type)
/// - The file has no extension
/// - The file is not one of the formats above
/// - A `.txt` or `.md` file is not valid UTF-8
///
/// # Example
///
/// ```no_run
/// use genai_rs::document_from_file;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = document_from_file("report.pdf").await?;
/// # Ok(())
/// # }
/// ```
pub async fn document_from_file(path: impl AsRef<Path>) -> Result<Content, GenaiError> {
    let path = path.as_ref();

    // Get extension with specific error handling
    let ext = get_extension_info(path)?;

    // Try to detect MIME type
    let mime_type = detect_mime_type(path).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "Unsupported document extension '.{}' for file '{}'. \
             If it is a PDF, CSV or Office/OpenDocument file, use \
             document_from_file_with_mime() to name its type; the API rejects \
             other inline document types, so send text as Content::text().",
            ext,
            path.display()
        ))
    })?;

    if TEXT_FILE_MIME_TYPES.contains(&mime_type) {
        let text = read_text_file(path).await?;
        return Ok(Content::text(text));
    }

    if !DOCUMENT_MIME_TYPES.contains(&mime_type) {
        let suggestion = if mime_type.starts_with("image/") {
            "image_from_file()"
        } else if mime_type.starts_with("audio/") {
            "audio_from_file()"
        } else if mime_type.starts_with("video/") {
            "video_from_file()"
        } else if mime_type.starts_with("text/")
            || matches!(mime_type, "application/json" | "application/xml")
        {
            "Content::text() with the file contents read via std::fs::read_to_string()"
        } else {
            "the appropriate *_from_file() function"
        };

        return Err(GenaiError::InvalidInput(format!(
            "File '{}' has MIME type '{}' which is not supported for document content. \
             The Gemini API accepts PDF, CSV and Office/OpenDocument documents \
             (document_from_file() sends .txt and .md files as text). For this file, use {}.",
            path.display(),
            mime_type,
            suggestion
        )));
    }

    document_from_file_with_mime(path, mime_type).await
}

/// Loads a document file with an explicit MIME type.
///
/// Use this when you need to override the auto-detected MIME type,
/// such as for files with non-standard extensions. The MIME type is sent
/// as-is: the API accepts only the types [`document_from_file`] sends as
/// document content and rejects the rest, `text/plain` and `text/markdown`
/// included, so send text files with [`document_from_file`] or
/// [`Content::text()`].
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if the file cannot be read (with a
/// suggestion based on the error type).
///
/// # Example
///
/// ```no_run
/// use genai_rs::document_from_file_with_mime;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // A PDF saved without its extension
/// let doc = document_from_file_with_mime("report.bin", "application/pdf").await?;
/// # Ok(())
/// # }
/// ```
pub async fn document_from_file_with_mime(
    path: impl AsRef<Path>,
    mime_type: impl Into<String>,
) -> Result<Content, GenaiError> {
    let data = load_and_encode_file(&path).await?;
    Ok(Content::document_data(data, mime_type))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_detect_mime_type_images() {
        assert_eq!(detect_mime_type(Path::new("photo.jpg")), Some("image/jpeg"));
        assert_eq!(
            detect_mime_type(Path::new("photo.jpeg")),
            Some("image/jpeg")
        );
        assert_eq!(detect_mime_type(Path::new("image.png")), Some("image/png"));
        assert_eq!(detect_mime_type(Path::new("anim.gif")), Some("image/gif"));
        assert_eq!(
            detect_mime_type(Path::new("photo.webp")),
            Some("image/webp")
        );
        assert_eq!(
            detect_mime_type(Path::new("photo.heic")),
            Some("image/heic")
        );
        assert_eq!(
            detect_mime_type(Path::new("photo.heif")),
            Some("image/heif")
        );
    }

    #[test]
    fn test_detect_mime_type_audio() {
        assert_eq!(detect_mime_type(Path::new("song.mp3")), Some("audio/mp3"));
        assert_eq!(detect_mime_type(Path::new("audio.wav")), Some("audio/wav"));
        assert_eq!(detect_mime_type(Path::new("audio.ogg")), Some("audio/ogg"));
        assert_eq!(
            detect_mime_type(Path::new("audio.flac")),
            Some("audio/flac")
        );
        assert_eq!(detect_mime_type(Path::new("audio.aac")), Some("audio/aac"));
        assert_eq!(detect_mime_type(Path::new("audio.m4a")), Some("audio/m4a"));
    }

    #[test]
    fn test_detect_mime_type_video() {
        assert_eq!(detect_mime_type(Path::new("video.mp4")), Some("video/mp4"));
        assert_eq!(
            detect_mime_type(Path::new("video.webm")),
            Some("video/webm")
        );
        assert_eq!(
            detect_mime_type(Path::new("video.mov")),
            Some("video/quicktime")
        );
        assert_eq!(
            detect_mime_type(Path::new("video.avi")),
            Some("video/x-msvideo")
        );
        assert_eq!(
            detect_mime_type(Path::new("video.mkv")),
            Some("video/x-matroska")
        );
    }

    #[test]
    fn test_detect_mime_type_documents() {
        for (name, mime) in [
            ("doc.pdf", "application/pdf"),
            ("data.csv", "text/csv"),
            ("letter.doc", "application/msword"),
            (
                "letter.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                "letter.dotx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
            ),
            ("sheet.xls", "application/vnd.ms-excel"),
            (
                "sheet.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            ("deck.ppt", "application/vnd.ms-powerpoint"),
            (
                "deck.pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            ("letter.odt", "application/vnd.oasis.opendocument.text"),
            (
                "sheet.ods",
                "application/vnd.oasis.opendocument.spreadsheet",
            ),
            (
                "deck.odp",
                "application/vnd.oasis.opendocument.presentation",
            ),
        ] {
            assert_eq!(detect_mime_type(Path::new(name)), Some(mime), "{name}");
        }
    }

    #[test]
    fn test_detect_mime_type_text() {
        assert_eq!(
            detect_mime_type(Path::new("readme.txt")),
            Some("text/plain")
        );
        assert_eq!(
            detect_mime_type(Path::new("README.md")),
            Some("text/markdown")
        );
        assert_eq!(
            detect_mime_type(Path::new("config.json")),
            Some("application/json")
        );
        assert_eq!(detect_mime_type(Path::new("page.html")), Some("text/html"));
        assert_eq!(
            detect_mime_type(Path::new("config.xml")),
            Some("application/xml")
        );
    }

    #[test]
    fn test_detect_mime_type_unknown() {
        assert_eq!(detect_mime_type(Path::new("file.xyz")), None);
        assert_eq!(detect_mime_type(Path::new("file.rtf")), None);
        assert_eq!(detect_mime_type(Path::new("file.pages")), None);
        assert_eq!(detect_mime_type(Path::new("noextension")), None);
    }

    #[test]
    fn test_detect_mime_type_case_insensitive() {
        assert_eq!(detect_mime_type(Path::new("photo.JPG")), Some("image/jpeg"));
        assert_eq!(detect_mime_type(Path::new("photo.PNG")), Some("image/png"));
        assert_eq!(detect_mime_type(Path::new("photo.Mp3")), Some("audio/mp3"));
    }

    #[test]
    fn test_detect_mime_type_with_path() {
        assert_eq!(
            detect_mime_type(Path::new("/some/path/to/photo.jpg")),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_mime_type(Path::new("./relative/path.png")),
            Some("image/png")
        );
    }

    #[test]
    fn test_detect_mime_type_trailing_dot() {
        // Edge case: file ending with period
        assert_eq!(detect_mime_type(Path::new("file.")), None);
        assert_eq!(detect_mime_type(Path::new("/path/to/file.")), None);
    }

    #[test]
    fn test_get_extension_info_missing_extension() {
        let result = get_extension_info(Path::new("noextension"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("has no extension"));
        assert!(err.contains("*_with_mime()"));
    }

    #[test]
    fn test_get_extension_info_valid_extension() {
        let result = get_extension_info(Path::new("photo.jpg"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "jpg");
    }

    #[test]
    fn test_get_extension_info_uppercase() {
        let result = get_extension_info(Path::new("photo.JPG"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "jpg"); // Should be lowercase
    }

    #[test]
    fn test_validate_mime_category_correct() {
        let result =
            validate_mime_category("image/jpeg", "image/", Path::new("photo.jpg"), "an image");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_mime_category_wrong_category() {
        // Trying to use audio file as image
        let result =
            validate_mime_category("audio/mp3", "image/", Path::new("song.mp3"), "an image");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("audio/mp3"));
        assert!(err.contains("not an image type"));
        assert!(err.contains("audio_from_file()"));
    }

    #[test]
    fn test_validate_mime_category_suggests_correct_function() {
        // Video file used as audio
        let result =
            validate_mime_category("video/mp4", "audio/", Path::new("clip.mp4"), "an audio");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("video_from_file()"));

        // PDF file used as image
        let result = validate_mime_category(
            "application/pdf",
            "image/",
            Path::new("doc.pdf"),
            "an image",
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("document_from_file()"));
    }

    #[test]
    fn test_validate_mime_category_suggests_document_from_file_for_csv_and_office() {
        for (name, mime) in [
            ("data.csv", "text/csv"),
            (
                "letter.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
        ] {
            let err = validate_mime_category(mime, "image/", Path::new(name), "an image")
                .unwrap_err()
                .to_string();
            assert!(err.contains("document_from_file()"), "{name}: {err}");
        }
    }

    #[test]
    fn test_validate_mime_category_text_hints_name_a_helper_that_accepts_them() {
        // document_from_file() sends .txt and .md files as text.
        for (name, mime) in [("notes.txt", "text/plain"), ("notes.md", "text/markdown")] {
            let err = validate_mime_category(mime, "image/", Path::new(name), "an image")
                .unwrap_err()
                .to_string();
            assert!(err.contains("document_from_file()"), "{name}: {err}");
        }
        // document_from_file() rejects these, so the hint must not send the
        // caller to any *_from_file() helper.
        for (name, mime) in [
            ("data.json", "application/json"),
            ("page.html", "text/html"),
            ("feed.xml", "application/xml"),
        ] {
            let err = validate_mime_category(mime, "image/", Path::new(name), "an image")
                .unwrap_err()
                .to_string();
            assert!(err.contains("Content::text()"), "{name}: {err}");
            assert!(!err.contains("_from_file()"), "{name}: {err}");
        }
    }

    #[tokio::test]
    async fn test_document_from_file_sends_document_types_as_documents() {
        let dir = tempfile::tempdir().unwrap();
        let mut seen = std::collections::HashSet::new();
        for name in [
            "doc.pdf",
            "data.csv",
            "letter.docx",
            "letter.dotx",
            "sheet.xlsx",
            "deck.pptx",
            "letter.doc",
            "sheet.xls",
            "deck.ppt",
            "letter.odt",
            "sheet.ods",
            "deck.odp",
            "SHOUT.PDF",
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"hello").unwrap();
            let expected = detect_mime_type(&path).unwrap();
            seen.insert(expected);
            match document_from_file(&path).await.unwrap() {
                Content::Document {
                    mime_type, data, ..
                } => {
                    assert_eq!(mime_type.as_deref(), Some(expected), "{name}");
                    assert_eq!(data.as_deref(), Some("aGVsbG8="), "{name}");
                }
                other => panic!("{name}: expected a document, got {other:?}"),
            }
        }
        // The extensions cover exactly the accepted document types.
        let accepted: std::collections::HashSet<_> = DOCUMENT_MIME_TYPES.iter().copied().collect();
        assert_eq!(seen, accepted);
    }

    /// The document types sent as documents are the bindings'
    /// `DocumentContentMimeType` list, so a baseline bump that adds a type
    /// fails here until it is probed and added (or listed as rejected).
    #[test]
    fn document_mime_types_are_the_bindings_list() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/api_surface/bindings.json"
        );
        let bindings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for alias in [
            "interactions.DocumentContentMimeType",
            "interactions.DocumentDeltaMimeType",
        ] {
            let listed: std::collections::BTreeSet<&str> = bindings["aliases"][alias]["values"]
                .as_array()
                .unwrap_or_else(|| panic!("{alias} has no literal values"))
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let sent: std::collections::BTreeSet<&str> =
                DOCUMENT_MIME_TYPES.iter().copied().collect();
            assert_eq!(sent, listed, "{alias}");
        }
    }

    /// The API rejects text/plain and text/markdown document content since
    /// 2026-11-04 (#514), so these go out as text.
    #[tokio::test]
    async fn test_document_from_file_sends_txt_and_markdown_as_text() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["notes.txt", "README.md", "SHOUT.TXT"] {
            let path = dir.path().join(name);
            std::fs::write(&path, "# héllo\nworld\n").unwrap();
            match document_from_file(&path).await.unwrap() {
                Content::Text { text, .. } => {
                    assert_eq!(text.as_deref(), Some("# héllo\nworld\n"), "{name}");
                }
                other => panic!("{name}: expected text content, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn test_document_from_file_rejects_non_utf8_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.txt");
        std::fs::write(&path, [0x63, 0x61, 0x66, 0xE9]).unwrap();
        let err = document_from_file(&path).await.unwrap_err().to_string();
        assert!(err.contains("not valid UTF-8"), "{err}");
        assert!(err.contains("latin1.txt"), "{err}");
    }

    #[tokio::test]
    async fn test_document_from_file_missing_text_file_has_read_hint() {
        let err = document_from_file("/nonexistent/path/notes.md")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("Failed to read file"), "{err}");
        assert!(err.contains("Check that the file path is correct"), "{err}");
    }

    #[tokio::test]
    async fn test_document_from_file_rejects_other_text_formats() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["page.html", "data.json", "feed.xml"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"x").unwrap();
            let err = document_from_file(&path).await.unwrap_err().to_string();
            assert!(
                err.contains("not supported for document content"),
                "{name}: {err}"
            );
            assert!(err.contains("Content::text()"), "{name}: {err}");
        }
    }

    #[tokio::test]
    async fn test_image_from_file_nonexistent() {
        let result = image_from_file("/nonexistent/path/file.jpg").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read file"));
        assert!(err.contains("Check that the file path is correct"));
    }

    #[tokio::test]
    async fn test_image_from_file_missing_extension() {
        let result = image_from_file("/tmp/noextension").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("has no extension"));
    }

    #[tokio::test]
    async fn test_image_from_file_unsupported_extension() {
        let result = image_from_file("/tmp/file.xyz").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported image extension"));
        assert!(err.contains(".xyz"));
    }

    #[tokio::test]
    async fn test_audio_from_file_wrong_mime_category() {
        // audio_from_file given an image extension should fail with category error
        let result = audio_from_file("/tmp/photo.png").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not an audio type"));
        assert!(err.contains("image_from_file()"));
    }
}
