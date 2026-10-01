//! Files API: upload files once and reference them by URI across
//! interactions.
//!
//! Files are stored for 48 hours. Limits: 2 GB per file, 20 GB per project.
//! Uploads use Google's resumable protocol, completed in a single
//! `upload, finalize` request. Path-based uploads stream the file from disk
//! (about 8 MB of buffer, whatever the file size);
//! [`Client::upload_file_bytes`] sends bytes already in memory.
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

use crate::client::Client;
use crate::errors::GenaiError;
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    /// The files on this page. A null or malformed list degrades to empty;
    /// malformed elements drop individually.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_lenient_vec"
    )]
    pub files: Vec<FileMetadata>,

    /// Token for retrieving the next page of results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Wrapper for file upload response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileUploadResponse {
    /// The uploaded file metadata
    pub file: FileMetadata,
}

/// The file name, used as the display name of path-based uploads.
fn file_display_name(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(ToString::to_string)
}

/// The MIME type inferred from `path`'s extension, or an error naming the
/// `*_with_mime` method to use instead.
pub(crate) fn mime_type_for_upload(
    path: &std::path::Path,
    explicit_alternative: &str,
) -> Result<&'static str, GenaiError> {
    crate::multimodal::detect_mime_type(path).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "Could not determine MIME type for '{}'. Please use {explicit_alternative} to specify explicitly.",
            path.display()
        ))
    })
}

/// Files API methods.
impl Client {
    /// Uploads a file from a path to the Files API, streaming it from disk.
    ///
    /// Files are stored for 48 hours and can be referenced in interactions by their URI.
    /// This is more efficient than inline base64 encoding for large files or files
    /// that will be used across multiple interactions. Memory use is bounded
    /// (about 8 MB of read buffer) whatever the file size, up to the 2 GB limit.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The MIME type cannot be determined
    /// - The upload fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Content};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload a video file
    /// let file = client.upload_file("video.mp4").await?;
    /// println!("Uploaded: {} -> {}", file.name, file.uri);
    ///
    /// // Use in interaction
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_content(vec![
    ///         Content::text("Describe this video"),
    ///         Content::from_file(&file),
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<crate::FileMetadata, GenaiError> {
        let path = path.as_ref();
        let mime_type = mime_type_for_upload(path, "upload_file_with_mime()")?;
        self.upload_file_with_mime(path, mime_type).await
    }

    /// Uploads a file with an explicit MIME type, streaming it from disk.
    ///
    /// Use this when automatic MIME type detection isn't suitable.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    /// * `mime_type` - MIME type of the file (e.g., "video/mp4")
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.upload_file_with_mime("data.bin", "application/octet-stream").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_with_mime(
        &self,
        path: impl AsRef<std::path::Path>,
        mime_type: &str,
    ) -> Result<crate::FileMetadata, GenaiError> {
        let path = path.as_ref();
        crate::http::files::upload_path(
            &self.http,
            path,
            mime_type,
            file_display_name(path).as_deref(),
        )
        .await
    }

    /// Uploads file bytes directly with a specified MIME type.
    ///
    /// Use this when you already have file contents in memory.
    ///
    /// # Arguments
    ///
    /// * `data` - File contents as bytes
    /// * `mime_type` - MIME type of the file
    /// * `display_name` - Optional display name for the file
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload bytes from memory
    /// let video_bytes = std::fs::read("video.mp4")?;
    /// let file = client.upload_file_bytes(video_bytes, "video/mp4", Some("my-video")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_bytes(
        &self,
        data: Vec<u8>,
        mime_type: &str,
        display_name: Option<&str>,
    ) -> Result<crate::FileMetadata, GenaiError> {
        tracing::debug!(
            "Uploading file bytes: size={} bytes, mime_type={}, display_name={:?}",
            data.len(),
            mime_type,
            display_name
        );

        crate::http::files::upload_bytes(&self.http, data, mime_type, display_name).await
    }

    /// Gets metadata for an uploaded file.
    ///
    /// Use this to check the processing status of a recently uploaded file.
    ///
    /// # Arguments
    ///
    /// * `file_name` - The full resource name of the file (e.g.,
    ///   "files/abc123" — the form [`FileMetadata::name`](crate::FileMetadata)
    ///   returns). Anything else — a bare ID, extra path segments — is
    ///   rejected locally as [`GenaiError::InvalidInput`] before a request
    ///   is sent.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>`, and an API or network error if the request fails or
    /// the file doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.get_file("files/abc123").await?;
    /// if file.is_active() {
    ///     println!("File is ready to use");
    /// } else if file.is_processing() {
    ///     println!("File is still processing...");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_file(&self, file_name: &str) -> Result<crate::FileMetadata, GenaiError> {
        crate::http::files::get_file(&self.http, file_name).await
    }

    /// Lists all uploaded files.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client.list_files(None, None).await?;
    /// for file in response.files {
    ///     println!("{}: {} ({})", file.name, file.display_name.as_deref().unwrap_or(""), file.mime_type);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_files(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::ListFilesResponse, GenaiError> {
        crate::http::files::list_files(&self.http, page_size, page_token).await
    }

    /// Deletes an uploaded file.
    ///
    /// # Arguments
    ///
    /// * `file_name` - The full resource name of the file to delete (e.g.,
    ///   "files/abc123" — the form [`FileMetadata::name`](crate::FileMetadata)
    ///   returns). Anything else — a bare ID, extra path segments — is
    ///   rejected locally as [`GenaiError::InvalidInput`] before a request
    ///   is sent.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>`, and an API or network error if the request fails or
    /// the file doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload, use, then delete
    /// let file = client.upload_file("video.mp4").await?;
    /// // ... use in interactions ...
    /// client.delete_file(&file.name).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete_file(&self, file_name: &str) -> Result<(), GenaiError> {
        crate::http::files::delete_file(&self.http, file_name).await
    }

    /// Waits for a file to finish processing.
    ///
    /// Some files (especially videos) require processing before they can be used.
    /// This method polls the file status until it becomes active or fails.
    ///
    /// # Arguments
    ///
    /// * `file` - The file metadata to wait for
    /// * `poll_interval` - How often to check the status
    /// * `timeout` - Maximum time to wait
    ///
    /// # Returns
    ///
    /// Returns the updated file metadata when processing completes.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::Internal`] if processing fails (terminal, so
    /// not retryable) or the timeout is exceeded, or an error from polling.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.upload_file("large_video.mp4").await?;
    ///
    /// // Wait for processing to complete
    /// let ready_file = client.wait_for_file_ready(
    ///     &file,
    ///     Duration::from_secs(2),
    ///     Duration::from_secs(120)
    /// ).await?;
    ///
    /// println!("File ready: {}", ready_file.uri);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn wait_for_file_ready(
        &self,
        file: &crate::FileMetadata,
        poll_interval: std::time::Duration,
        timeout: std::time::Duration,
    ) -> Result<crate::FileMetadata, GenaiError> {
        use std::time::Instant;

        let start = Instant::now();

        loop {
            let current = self.get_file(&file.name).await?;

            if current.is_active() {
                return Ok(current);
            }

            if current.is_failed() {
                // `Internal`, not `Api`: the file will never process, and a
                // fabricated 5xx would make `is_retryable()` say otherwise.
                // `FileError::code` is a google.rpc code, not an HTTP status.
                let detail = current
                    .error
                    .as_ref()
                    .map_or_else(|| "no details".to_string(), ToString::to_string);
                tracing::error!("File '{}' processing failed: {}", file.name, detail);
                return Err(GenaiError::Internal(format!(
                    "File '{}' failed processing ({detail}). This is terminal — \
                     re-uploading is the only recovery.",
                    file.name
                )));
            }

            // Log unknown states per Evergreen logging strategy
            if let Some(state) = &current.state
                && state.is_unknown()
            {
                tracing::warn!(
                    "File '{}' is in unknown state {:?}, continuing to poll. \
                     This may indicate API evolution - consider updating genai-rs.",
                    file.name,
                    state
                );
            }

            if start.elapsed() > timeout {
                // Use Internal error since this is an operational issue, not invalid input
                let state_info = current
                    .state
                    .as_ref()
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_else(|| "unknown".to_string());
                return Err(GenaiError::Internal(format!(
                    "Timeout waiting for file '{}' to be ready (waited {:?}, last state: {}). \
                     The file may still be processing - try again with a longer timeout.",
                    file.name,
                    start.elapsed(),
                    state_info
                )));
            }

            tracing::debug!(
                "File '{}' still processing, waiting {:?}...",
                file.name,
                poll_interval
            );
            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_list_files_response_drops_only_the_undeserializable_entry() {
        // `mimeType` is required; the second entry lacks it.
        let json = r#"{
            "files": [
                {"name": "files/a", "mimeType": "text/plain", "uri": "u"},
                {"name": "files/b", "uri": "u"}
            ],
            "nextPageToken": "p2"
        }"#;
        let list: ListFilesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(list.files.len(), 1);
        assert_eq!(list.files[0].name, "files/a");
        assert_eq!(list.next_page_token.as_deref(), Some("p2"));
    }

    #[test]
    fn test_list_files_response_explicit_null_list_is_empty() {
        let list: ListFilesResponse = serde_json::from_str(r#"{"files": null}"#).unwrap();
        assert!(list.files.is_empty());
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

    #[tokio::test]
    async fn test_upload_file_unknown_extension_error() {
        let client = Client::new("test_key".to_string());

        // Create a temp file with an unknown extension
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("data.xyz");
        std::fs::write(&file_path, b"test data").unwrap();

        // upload_file should fail with InvalidInput for unknown MIME type
        let result = client.upload_file(&file_path).await;
        assert!(result.is_err(), "Should fail for unknown extension");

        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(
            err_string.contains("Could not determine MIME type"),
            "Error should mention MIME type issue: {}",
            err_string
        );
        assert!(
            err_string.contains("data.xyz"),
            "Error should include filename: {}",
            err_string
        );
    }

    #[tokio::test]
    async fn test_upload_file_nonexistent_file_error() {
        let client = Client::new("test_key".to_string());

        // Try to upload a file that doesn't exist
        let result = client.upload_file("/nonexistent/path/to/file.txt").await;
        assert!(result.is_err(), "Should fail for nonexistent file");

        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(
            err_string.contains("Failed to read file"),
            "Error should mention file read failure: {}",
            err_string
        );
    }

    #[tokio::test]
    async fn test_upload_file_bytes_empty_file_error() {
        let client = Client::new("test_key".to_string());

        // Try to upload empty bytes
        let result = client
            .upload_file_bytes(Vec::new(), "text/plain", Some("empty.txt"))
            .await;
        assert!(result.is_err(), "Should fail for empty file");

        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(
            err_string.contains("Cannot upload empty file"),
            "Error should mention empty file: {}",
            err_string
        );
    }

    #[tokio::test]
    async fn test_upload_file_bytes_validates_before_network() {
        // This test verifies that validation happens before any network call
        // by using an invalid API key - if we reach the network, we'd get auth error
        let client = Client::new("invalid_key".to_string());

        // Empty file should fail with validation error, not auth error
        let result = client
            .upload_file_bytes(Vec::new(), "text/plain", None)
            .await;
        assert!(result.is_err());
        let err_string = result.unwrap_err().to_string();
        assert!(
            err_string.contains("Cannot upload empty file"),
            "Should fail validation before hitting network: {}",
            err_string
        );
    }
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
