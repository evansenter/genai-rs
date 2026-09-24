//! Files API: upload files once and reference them by URI across
//! interactions.
//!
//! Files are stored for 48 hours. Limits: 2 GB per file, 20 GB per project.
//! Uploads use Google's resumable protocol, completed in a single
//! `upload, finalize` request; the `chunked` variants stream the body from
//! disk instead of reading it into memory.
//!
//! # Example
//!
//! ```no_run
//! use genai_rs::{Client, Content};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new("api-key".to_string());
//!
//! let file = client.upload_file("video.mp4").await?;
//! let response = client
//!     .interaction()
//!     .with_model(genai_rs::DEFAULT_MODEL)
//!     .with_content(vec![
//!         Content::text("Describe this video"),
//!         Content::from_file(&file),
//!     ])
//!     .create()
//!     .await?;
//!
//! client.delete_file(&file.name).await?;
//! # Ok(())
//! # }
//! ```

use super::common::{
    API_KEY_HEADER, NO_BODY, path_segment, require_id, send_and_read, send_checked, with_query,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::wire::WireEvent;
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio_util::io::ReaderStream;

/// Represents an uploaded file in the Files API.
///
/// Files are stored on Google's servers for 48 hours and can be referenced
/// in interactions by their URI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileMetadata {
    /// The resource name of the file (e.g., "files/abc123")
    pub name: String,

    /// User-provided display name for the file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// MIME type of the file
    pub mime_type: String,

    /// Size of the file in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<String>,

    /// Timestamp when the file was created (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<DateTime<Utc>>,

    /// Timestamp when the file will be automatically deleted (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiration_time: Option<DateTime<Utc>>,

    /// SHA256 hash of the file contents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256_hash: Option<String>,

    /// URI to reference this file in API calls
    #[serde(default)]
    pub uri: String,

    /// Processing state of the file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<FileState>,

    /// Error information if processing failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,

    /// Video metadata (if this is a video file)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_metadata: Option<VideoMetadata>,
}

impl FileMetadata {
    /// Returns true if the file is still being processed.
    #[must_use]
    pub fn is_processing(&self) -> bool {
        matches!(self.state, Some(FileState::Processing))
    }

    /// Returns true if the file is ready to use.
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self.state, Some(FileState::Active))
    }

    /// Returns true if file processing failed.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self.state, Some(FileState::Failed))
    }

    /// Parses the size_bytes field as a u64, if present and valid.
    ///
    /// The API returns file sizes as strings in the JSON response.
    /// This helper parses that string into a numeric type for convenience.
    ///
    /// # Returns
    ///
    /// - `Some(size)` if size_bytes is present and can be parsed as u64
    /// - `None` if size_bytes is absent or cannot be parsed
    ///
    /// # Example
    ///
    /// ```
    /// # use genai_rs::FileMetadata;
    /// # let file: FileMetadata = serde_json::from_str(r#"{"name":"files/abc","mimeType":"video/mp4","uri":"","sizeBytes":"1234567"}"#).unwrap();
    /// if let Some(size) = file.size_bytes_as_u64() {
    ///     println!("File size: {} bytes", size);
    /// }
    /// ```
    #[must_use]
    pub fn size_bytes_as_u64(&self) -> Option<u64> {
        self.size_bytes.as_ref().and_then(|s| s.parse().ok())
    }
}

wire_enum! {
    /// Processing state of an uploaded file.
    pub enum FileState {
        /// File is being processed
        Processing = "PROCESSING",
        /// File is ready to use
        Active = "ACTIVE",
        /// File processing failed
        Failed = "FAILED",
    }
    unknown(state_type, unknown_state_type)
}

/// Error information for failed file operations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileError {
    /// Error code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    /// Error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.code, &self.message) {
            (Some(code), Some(msg)) => write!(f, "error {}: {}", code, msg),
            (Some(code), None) => write!(f, "error {}", code),
            (None, Some(msg)) => write!(f, "{}", msg),
            (None, None) => write!(f, "unknown error"),
        }
    }
}

/// Metadata for video files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VideoMetadata {
    /// Duration of the video
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_duration: Option<String>,
}

/// Response from listing files.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ListFilesResponse {
    /// List of files. Strict, unlike the Interactions list envelopes: a
    /// malformed element here is a real protocol break worth failing on.
    #[serde(default)]
    pub files: Vec<FileMetadata>,

    /// Token for retrieving the next page of results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Wrapper for file upload response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileUploadResponse {
    /// The uploaded file metadata
    pub file: FileMetadata,
}

// --- API Functions ---

/// Maximum file size for uploads (2 GB)
const MAX_FILE_SIZE: u64 = 2_147_483_648;

/// Rejects an empty or oversized upload before any request is made.
fn check_upload_size(file_size: u64) -> Result<(), GenaiError> {
    if file_size == 0 {
        return Err(GenaiError::InvalidInput(
            "Cannot upload empty file".to_string(),
        ));
    }
    if file_size > MAX_FILE_SIZE {
        return Err(GenaiError::InvalidInput(format!(
            "File size {} bytes exceeds maximum allowed size of {} bytes (2 GB)",
            file_size, MAX_FILE_SIZE
        )));
    }
    Ok(())
}

/// Starts a resumable upload session and returns its upload URL.
///
/// Emits [`WireEvent::UploadStart`] under `request_id`.
async fn start_upload_session(
    ctx: &HttpContext,
    request_id: u64,
    file_label: &str,
    file_size: u64,
    mime_type: &str,
    display_name: Option<&str>,
) -> Result<String, GenaiError> {
    if ctx.has_inspectors() {
        ctx.emit(WireEvent::UploadStart {
            id: request_id,
            file_name: file_label.to_string(),
            mime_type: mime_type.to_string(),
            size_bytes: file_size,
        });
    }

    let metadata = match display_name {
        Some(name) => serde_json::json!({ "file": { "displayName": name } }),
        None => serde_json::json!({ "file": {} }),
    };
    let builder = ctx
        .http_client
        .post(ctx.upload_url("files"))
        .header(API_KEY_HEADER, &ctx.api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", file_size.to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .json(&metadata);
    let response = send_checked(ctx, request_id, builder).await?;

    response
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| {
            GenaiError::MalformedResponse(
                "Upload session response is missing the x-goog-upload-url header".to_string(),
            )
        })
}

/// Sends the whole body to an upload session and finalizes it.
///
/// Emits [`WireEvent::UploadComplete`] under `request_id`.
async fn finish_upload(
    ctx: &HttpContext,
    request_id: u64,
    upload_url: &str,
    file_size: u64,
    body: reqwest::Body,
) -> Result<FileMetadata, GenaiError> {
    let builder = ctx
        .http_client
        .post(upload_url)
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .header("Content-Length", file_size.to_string())
        .body(body);
    let response = send_checked(ctx, request_id, builder).await?;
    let response_text = response.text().await?;
    let file =
        deserialize_with_context::<FileUploadResponse>(&response_text, "FileUploadResponse")?.file;

    tracing::debug!("File uploaded: name={}, uri={}", file.name, file.uri);
    if ctx.has_inspectors() {
        ctx.emit(WireEvent::UploadComplete {
            id: request_id,
            uri: file.uri.clone(),
        });
    }
    Ok(file)
}

/// Uploads in-memory bytes to the Files API.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] for an empty or oversized file, or an
/// error if either request of the upload fails.
pub async fn upload_file(
    ctx: &HttpContext,
    file_data: Vec<u8>,
    mime_type: &str,
    display_name: Option<&str>,
) -> Result<FileMetadata, GenaiError> {
    let file_size = file_data.len() as u64;
    check_upload_size(file_size)?;
    tracing::debug!(
        "Uploading file: size={} bytes, mime_type={}, display_name={:?}",
        file_size,
        mime_type,
        display_name
    );

    let request_id = ctx.next_request_id();
    let upload_url = start_upload_session(
        ctx,
        request_id,
        display_name.unwrap_or("(unnamed)"),
        file_size,
        mime_type,
        display_name,
    )
    .await?;
    finish_upload(ctx, request_id, &upload_url, file_size, file_data.into()).await
}

/// Metadata of the resumable-upload session a streaming upload used.
#[derive(Clone, Debug)]
pub struct ResumableUpload {
    upload_url: String,
    file_size: u64,
    mime_type: String,
}

impl ResumableUpload {
    /// Returns the upload URL for this session.
    #[must_use]
    pub fn upload_url(&self) -> &str {
        &self.upload_url
    }

    /// Returns the total file size.
    #[must_use]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Returns the MIME type.
    #[must_use]
    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }
}

/// Default read-buffer size for streaming uploads (8 MB).
pub const DEFAULT_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Uploads a file from disk, streaming it rather than reading it into memory.
///
/// `chunk_size` is the read-buffer size, which bounds memory use; the body
/// still goes up as one `upload, finalize` request.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if the file cannot be read or is
/// empty or oversized, or an error if the upload fails.
pub async fn upload_file_chunked(
    ctx: &HttpContext,
    path: &Path,
    mime_type: &str,
    display_name: Option<&str>,
    chunk_size: usize,
) -> Result<(FileMetadata, ResumableUpload), GenaiError> {
    let file_size = tokio::fs::metadata(path)
        .await
        .map_err(|e| {
            GenaiError::InvalidInput(format!("Failed to access file '{}': {}", path.display(), e))
        })?
        .len();
    check_upload_size(file_size)?;
    tracing::debug!(
        "Streaming upload: path={}, size={} bytes, mime_type={}, chunk_size={} bytes",
        path.display(),
        file_size,
        mime_type,
        chunk_size
    );

    let request_id = ctx.next_request_id();
    let file_label = display_name
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let upload_url = start_upload_session(
        ctx,
        request_id,
        &file_label,
        file_size,
        mime_type,
        display_name,
    )
    .await?;

    let file = tokio::fs::File::open(path).await.map_err(|e| {
        GenaiError::InvalidInput(format!("Failed to open file '{}': {}", path.display(), e))
    })?;
    let body = reqwest::Body::wrap_stream(ReaderStream::with_capacity(file, chunk_size));
    let metadata = finish_upload(ctx, request_id, &upload_url, file_size, body).await?;

    Ok((
        metadata,
        ResumableUpload {
            upload_url,
            file_size,
            mime_type: mime_type.to_string(),
        },
    ))
}

/// Validates a `files/<id>` resource name and rebuilds it with the ID
/// percent-encoded.
fn file_resource_path(file_name: &str) -> Result<String, GenaiError> {
    let Some(id) = file_name.strip_prefix("files/") else {
        return Err(GenaiError::InvalidInput(format!(
            "file name must be a full `files/<id>` resource name \
             (the form upload responses return); got {file_name:?}"
        )));
    };
    require_id(id, "file")?;
    if id.contains('/') {
        return Err(GenaiError::InvalidInput(format!(
            "file name must contain exactly one segment after `files/`; \
             got {file_name:?}"
        )));
    }
    Ok(format!("files/{}", path_segment(id)))
}

/// Gets metadata for a file by its full resource name (`files/abc123`).
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] for a name that is not
/// `files/<id>`, or an error if the request fails or the file doesn't exist.
pub async fn get_file(ctx: &HttpContext, file_name: &str) -> Result<FileMetadata, GenaiError> {
    let url = ctx.api_url(&file_resource_path(file_name)?);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    let file: FileMetadata = deserialize_with_context(&text, "FileMetadata")?;
    tracing::debug!("Got file {}: state={:?}", file.name, file.state);
    Ok(file)
}

/// The list URL. The Files API spells its paging params in camelCase.
fn list_files_url(ctx: &HttpContext, page_size: Option<u32>, page_token: Option<&str>) -> String {
    let page_size = page_size.map(|size| size.to_string());
    with_query(
        ctx.api_url("files"),
        &[
            ("pageSize", page_size.as_deref()),
            ("pageToken", page_token),
        ],
    )
}

/// Lists uploaded files.
///
/// # Errors
///
/// Returns an error if the request fails.
pub async fn list_files(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<ListFilesResponse, GenaiError> {
    let url = list_files_url(ctx, page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    let list: ListFilesResponse = deserialize_with_context(&text, "ListFilesResponse")?;
    tracing::debug!("Listed {} files", list.files.len());
    Ok(list)
}

/// Deletes a file by its full resource name (`files/abc123`).
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] for a name that is not
/// `files/<id>`, or an error if the request fails or the file doesn't exist.
pub async fn delete_file(ctx: &HttpContext, file_name: &str) -> Result<(), GenaiError> {
    let url = ctx.api_url(&file_resource_path(file_name)?);
    send_and_read(ctx, reqwest::Method::DELETE, &url, NO_BODY).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_files_url_encodes_paging() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        let list_files_url = |size, token| super::list_files_url(&ctx, size, token);
        let base = "https://generativelanguage.googleapis.com/v1beta/files";
        assert_eq!(list_files_url(None, None), base);
        assert_eq!(
            list_files_url(Some(10), None),
            format!("{base}?pageSize=10")
        );
        // Token alone takes the leading `?`; with a size it rides `&`.
        assert_eq!(
            list_files_url(None, Some("tok")),
            format!("{base}?pageToken=tok")
        );
        assert_eq!(
            list_files_url(Some(10), Some("tok")),
            format!("{base}?pageSize=10&pageToken=tok")
        );
        // `+` is what a standard-base64 token would hit (it would otherwise
        // decode to a space server-side).
        assert_eq!(
            list_files_url(None, Some("a/b&c=d+e")),
            format!("{base}?pageToken=a%2Fb%26c%3Dd%2Be")
        );
    }

    #[test]
    fn file_resource_path_validates_shape_and_encodes() {
        // The happy path: the exact shape upload responses return, with
        // the opaque ID passing through byte-identical.
        assert_eq!(file_resource_path("files/abc123").unwrap(), "files/abc123");
        // Dots inside an ID are not dot segments and stay verbatim.
        assert_eq!(file_resource_path("files/a.b.c").unwrap(), "files/a.b.c");

        // Shape violations reject locally: missing prefix, empty ID,
        // extra segments (which also covers `files/../x`), and every
        // dot-segment spelling — WHATWG dot-segment removal matches the
        // percent-encoded forms case-insensitively at parse time, so
        // `require_id` rejects them all rather than trusting encoding.
        assert!(file_resource_path("abc123").is_err());
        assert!(file_resource_path("").is_err());
        assert!(file_resource_path("files/").is_err());
        assert!(file_resource_path("files/a/b").is_err());
        assert!(file_resource_path("../../v1beta/files/other").is_err());
        assert!(file_resource_path("files/..").is_err());
        assert!(file_resource_path("files/%2e%2e").is_err());
        assert!(file_resource_path("files/%2E.").is_err());

        // Threats that pass the shape check are defused by encoding, so
        // URL parsing can never treat them structurally: the
        // query/fragment splitters that would silently retarget the
        // request.
        assert_eq!(
            file_resource_path("files/abc?alt=media").unwrap(),
            "files/abc%3Falt%3Dmedia"
        );
        assert_eq!(
            file_resource_path("files/abc#frag").unwrap(),
            "files/abc%23frag"
        );
    }

    #[test]
    fn test_file_metadata_deserialization() {
        let json = r#"{
            "name": "files/abc123",
            "displayName": "test.mp4",
            "mimeType": "video/mp4",
            "sizeBytes": "1234567",
            "createTime": "2024-01-01T00:00:00Z",
            "expirationTime": "2024-01-03T00:00:00Z",
            "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc123",
            "state": "ACTIVE"
        }"#;

        let file: FileMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(file.name, "files/abc123");
        assert_eq!(file.display_name.as_deref(), Some("test.mp4"));
        assert_eq!(file.mime_type, "video/mp4");
        assert!(file.is_active());
        assert!(!file.is_processing());
    }

    #[test]
    fn test_file_state_processing() {
        let json =
            r#"{"name": "files/test", "mimeType": "video/mp4", "state": "PROCESSING", "uri": ""}"#;
        let file: FileMetadata = serde_json::from_str(json).unwrap();
        assert!(file.is_processing());
        assert!(!file.is_active());
    }

    #[test]
    fn test_file_state_failed() {
        let json =
            r#"{"name": "files/test", "mimeType": "video/mp4", "state": "FAILED", "uri": ""}"#;
        let file: FileMetadata = serde_json::from_str(json).unwrap();
        assert!(file.is_failed());
        assert!(!file.is_active());
    }

    #[test]
    fn test_list_files_response_deserialization() {
        let json = r#"{
            "files": [
                {"name": "files/a", "mimeType": "video/mp4", "uri": ""},
                {"name": "files/b", "mimeType": "image/png", "uri": ""}
            ],
            "nextPageToken": "token123"
        }"#;

        let response: ListFilesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.files.len(), 2);
        assert_eq!(response.next_page_token.as_deref(), Some("token123"));
    }

    #[test]
    fn test_empty_list_files_response() {
        let json = r#"{}"#;
        let response: ListFilesResponse = serde_json::from_str(json).unwrap();
        assert!(response.files.is_empty());
        assert!(response.next_page_token.is_none());
    }

    #[cfg(not(feature = "strict-unknown"))]
    #[test]
    fn test_file_state_unknown_preserves_data() {
        // Test that unknown states preserve the original value
        let json =
            r#"{"name": "files/test", "mimeType": "video/mp4", "state": "UPLOADING", "uri": ""}"#;
        let file: FileMetadata = serde_json::from_str(json).unwrap();

        assert!(!file.is_active());
        assert!(!file.is_processing());
        assert!(!file.is_failed());

        // Check the Unknown variant captured the state
        if let Some(FileState::Unknown { state_type, data }) = &file.state {
            assert_eq!(state_type, "UPLOADING");
            assert_eq!(data.as_str(), Some("UPLOADING"));
        } else {
            panic!("Expected FileState::Unknown variant, got {:?}", file.state);
        }
    }

    #[test]
    fn test_file_state_unknown_helper_methods() {
        let unknown = FileState::Unknown {
            state_type: "NEW_STATE".to_string(),
            data: serde_json::json!("NEW_STATE"),
        };

        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_state_type(), Some("NEW_STATE"));
        assert_eq!(
            unknown.unknown_data(),
            Some(&serde_json::json!("NEW_STATE"))
        );

        // Known states should return None for unknown helpers
        let active = FileState::Active;
        assert!(!active.is_unknown());
        assert_eq!(active.unknown_state_type(), None);
        assert_eq!(active.unknown_data(), None);
    }

    #[test]
    fn test_file_state_roundtrip_serialization() {
        // Known state roundtrips
        let active = FileState::Active;
        let json = serde_json::to_string(&active).unwrap();
        assert_eq!(json, r#""ACTIVE""#);
        let deserialized: FileState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, FileState::Active);

        // Unknown state roundtrips
        let unknown = FileState::Unknown {
            state_type: "QUEUED".to_string(),
            data: serde_json::json!("QUEUED"),
        };
        let json = serde_json::to_string(&unknown).unwrap();
        assert_eq!(json, r#""QUEUED""#);
    }

    #[test]
    fn test_file_metadata_failed_state_with_error() {
        let json = r#"{
            "name": "files/failed123",
            "mimeType": "video/mp4",
            "state": "FAILED",
            "uri": "",
            "error": {
                "code": 400,
                "message": "Unsupported video codec"
            }
        }"#;
        let file: FileMetadata = serde_json::from_str(json).unwrap();
        assert!(file.is_failed());
        assert!(file.error.is_some());

        let error = file.error.unwrap();
        assert_eq!(error.code, Some(400));
        assert_eq!(error.message.as_deref(), Some("Unsupported video codec"));
    }

    #[test]
    fn test_file_error_partial_fields() {
        // Error with only code
        let json = r#"{"code": 500}"#;
        let error: FileError = serde_json::from_str(json).unwrap();
        assert_eq!(error.code, Some(500));
        assert_eq!(error.message, None);

        // Error with only message
        let json = r#"{"message": "Something went wrong"}"#;
        let error: FileError = serde_json::from_str(json).unwrap();
        assert_eq!(error.code, None);
        assert_eq!(error.message.as_deref(), Some("Something went wrong"));

        // Empty error (edge case)
        let json = r#"{}"#;
        let error: FileError = serde_json::from_str(json).unwrap();
        assert_eq!(error.code, None);
        assert_eq!(error.message, None);
    }

    #[test]
    fn test_file_error_display() {
        // Both code and message
        let error = FileError {
            code: Some(400),
            message: Some("Invalid file format".to_string()),
        };
        assert_eq!(error.to_string(), "error 400: Invalid file format");

        // Only code
        let error = FileError {
            code: Some(500),
            message: None,
        };
        assert_eq!(error.to_string(), "error 500");

        // Only message
        let error = FileError {
            code: None,
            message: Some("Something went wrong".to_string()),
        };
        assert_eq!(error.to_string(), "Something went wrong");

        // Neither code nor message
        let error = FileError {
            code: None,
            message: None,
        };
        assert_eq!(error.to_string(), "unknown error");
    }

    #[test]
    fn test_size_bytes_as_u64() {
        // Valid size_bytes parses correctly
        let file = FileMetadata {
            name: "files/test".to_string(),
            display_name: None,
            mime_type: "video/mp4".to_string(),
            size_bytes: Some("1234567890".to_string()),
            create_time: None,
            expiration_time: None,
            sha256_hash: None,
            uri: "".to_string(),
            state: None,
            error: None,
            video_metadata: None,
        };
        assert_eq!(file.size_bytes_as_u64(), Some(1234567890));

        // None size_bytes returns None
        let file = FileMetadata {
            name: "files/test".to_string(),
            display_name: None,
            mime_type: "video/mp4".to_string(),
            size_bytes: None,
            create_time: None,
            expiration_time: None,
            sha256_hash: None,
            uri: "".to_string(),
            state: None,
            error: None,
            video_metadata: None,
        };
        assert_eq!(file.size_bytes_as_u64(), None);

        // Invalid size_bytes (non-numeric) returns None
        let file = FileMetadata {
            name: "files/test".to_string(),
            display_name: None,
            mime_type: "video/mp4".to_string(),
            size_bytes: Some("not a number".to_string()),
            create_time: None,
            expiration_time: None,
            sha256_hash: None,
            uri: "".to_string(),
            state: None,
            error: None,
            video_metadata: None,
        };
        assert_eq!(file.size_bytes_as_u64(), None);

        // Large file size (2GB+) parses correctly
        let file = FileMetadata {
            name: "files/test".to_string(),
            display_name: None,
            mime_type: "video/mp4".to_string(),
            size_bytes: Some("2147483648".to_string()), // 2GB
            create_time: None,
            expiration_time: None,
            sha256_hash: None,
            uri: "".to_string(),
            state: None,
            error: None,
            video_metadata: None,
        };
        assert_eq!(file.size_bytes_as_u64(), Some(2147483648));
    }

    // Note: Tests for upload_file validation (empty file, max size) are in
    // tests/files_api_tests.rs as integration tests since they require mocking
    // the HTTP client or hitting the real API.
}

/// Property-based tests for serialization roundtrips using proptest.
#[cfg(test)]
mod proptest_tests {
    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    /// Strategy for generating DateTime<Utc> values.
    /// Uses second precision to ensure reliable roundtrip.
    fn arb_datetime() -> impl Strategy<Value = DateTime<Utc>> {
        // Generate timestamps between 2020-01-01 and 2030-01-01
        (0i64..315_360_000).prop_map(|offset_secs| {
            Utc.timestamp_opt(1_577_836_800 + offset_secs, 0)
                .single()
                .expect("valid timestamp")
        })
    }

    /// Strategy for generating FileState variants.
    #[cfg(not(feature = "strict-unknown"))]
    fn arb_file_state() -> impl Strategy<Value = FileState> {
        prop_oneof![
            Just(FileState::Processing),
            Just(FileState::Active),
            Just(FileState::Failed),
            // Include Unknown variant for graceful handling
            ("[A-Z_]{4,20}".prop_map(|state_type| FileState::Unknown {
                state_type,
                data: serde_json::Value::Null,
            })),
        ]
    }

    /// Strategy for FileState - no Unknown in strict mode.
    #[cfg(feature = "strict-unknown")]
    fn arb_file_state() -> impl Strategy<Value = FileState> {
        prop_oneof![
            Just(FileState::Processing),
            Just(FileState::Active),
            Just(FileState::Failed),
        ]
    }

    /// Strategy for generating FileError.
    fn arb_file_error() -> impl Strategy<Value = FileError> {
        (
            prop::option::of(any::<i32>()),
            prop::option::of(".{0,100}".prop_map(String::from)),
        )
            .prop_map(|(code, message)| FileError { code, message })
    }

    /// Strategy for generating VideoMetadata.
    fn arb_video_metadata() -> impl Strategy<Value = VideoMetadata> {
        prop::option::of("[0-9]+s".prop_map(String::from))
            .prop_map(|video_duration| VideoMetadata { video_duration })
    }

    /// Strategy for generating FileMetadata.
    fn arb_file_metadata() -> impl Strategy<Value = FileMetadata> {
        (
            "files/[a-zA-Z0-9_]+",              // name
            prop::option::of(".{1,50}"),        // display_name
            "[a-z]+/[a-z0-9+-]+",               // mime_type
            prop::option::of("[0-9]+"),         // size_bytes
            prop::option::of(arb_datetime()),   // create_time
            prop::option::of(arb_datetime()),   // expiration_time
            prop::option::of("[a-f0-9]{64}"),   // sha256_hash (API returns raw hash, no prefix)
            "https?://[a-z]+\\.[a-z]+/[a-z]+",  // uri
            prop::option::of(arb_file_state()), // state is Option<FileState>
            prop::option::of(arb_file_error()),
            prop::option::of(arb_video_metadata()),
        )
            .prop_map(
                |(
                    name,
                    display_name,
                    mime_type,
                    size_bytes,
                    create_time,
                    expiration_time,
                    sha256_hash,
                    uri,
                    state,
                    error,
                    video_metadata,
                )| {
                    FileMetadata {
                        name,
                        display_name,
                        mime_type,
                        size_bytes,
                        create_time,
                        expiration_time,
                        sha256_hash,
                        uri,
                        state,
                        error,
                        video_metadata,
                    }
                },
            )
    }

    proptest! {
        /// Verify FileState roundtrips through JSON serialization.
        #[test]
        fn file_state_roundtrip(state in arb_file_state()) {
            let json = serde_json::to_string(&state).expect("serialize");
            let parsed: FileState = serde_json::from_str(&json).expect("deserialize");
            // For Unknown variants, we can't do exact equality since data may be different
            // Just verify it roundtrips to a valid state
            match (&state, &parsed) {
                (FileState::Processing, FileState::Processing) => {}
                (FileState::Active, FileState::Active) => {}
                (FileState::Failed, FileState::Failed) => {}
                (FileState::Unknown { .. }, FileState::Unknown { .. }) => {}
                _ => panic!("State changed during roundtrip: {:?} -> {:?}", state, parsed),
            }
        }

        /// Verify FileError roundtrips through JSON serialization.
        #[test]
        fn file_error_roundtrip(error in arb_file_error()) {
            let json = serde_json::to_string(&error).expect("serialize");
            let parsed: FileError = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(error.code, parsed.code);
            prop_assert_eq!(error.message, parsed.message);
        }

        /// Verify VideoMetadata roundtrips through JSON serialization.
        #[test]
        fn video_metadata_roundtrip(metadata in arb_video_metadata()) {
            let json = serde_json::to_string(&metadata).expect("serialize");
            let parsed: VideoMetadata = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(metadata.video_duration, parsed.video_duration);
        }

        /// Verify FileMetadata roundtrips through JSON serialization.
        #[test]
        fn file_metadata_roundtrip(metadata in arb_file_metadata()) {
            let json = serde_json::to_string(&metadata).expect("serialize");
            let parsed: FileMetadata = serde_json::from_str(&json).expect("deserialize");

            prop_assert_eq!(&metadata.name, &parsed.name);
            prop_assert_eq!(&metadata.display_name, &parsed.display_name);
            prop_assert_eq!(&metadata.mime_type, &parsed.mime_type);
            prop_assert_eq!(&metadata.size_bytes, &parsed.size_bytes);
            prop_assert_eq!(&metadata.uri, &parsed.uri);
            // Note: state comparison is relaxed for Unknown variants
        }
    }
}
