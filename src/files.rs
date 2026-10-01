//! Files API (`/v1beta/files`): upload files once and reference them by URI
//! across interactions.
//!
//! Files are stored for 48 hours. Limits: 2 GB per file, 20 GB per project.
//! Manage them through the [`Files`] handle from [`Client::files`]:
//! [`upload`](Files::upload), [`get`](Files::get), [`list`](Files::list),
//! [`delete`](Files::delete) and
//! [`wait_until_active`](Files::wait_until_active).
//!
//! Uploads use Google's resumable protocol, completed in a single
//! `upload, finalize` request. A [`FileUpload::from_path`] upload streams the
//! file from disk (about 8 MB of buffer, whatever the file size);
//! [`FileUpload::from_bytes`] sends bytes already in memory.
//!
//! # Names
//!
//! Methods take a file's full resource name, `files/<id>`, the form
//! [`FileMetadata::name`] holds. Anything else (a bare ID, extra path
//! segments, an empty or dot-segment ID) fails locally with
//! [`GenaiError::InvalidInput`] before any request; the ID is
//! percent-encoded into one path segment. A file that doesn't exist is a
//! 403 ending in "or it may not exist", not a 404 (verified live
//! 2026-09-24).
//!
//! # Example
//!
//! ```no_run
//! use genai_rs::{Client, Content, FileUpload, PollOptions};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new("api-key".to_string());
//!
//! let file = client.files().upload(FileUpload::from_path("video.mp4")).await?;
//! // Videos are processed after upload; wait until the file is usable.
//! let file = client
//!     .files()
//!     .wait_until_active(&file.name, PollOptions::new())
//!     .await?;
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
//! client.files().delete(&file.name).await?;
//! # let _ = response;
//! # Ok(())
//! # }
//! ```

use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

paging::impl_list_page!(ListFilesResponse, files: FileMetadata);

/// Wrapper for file upload response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileUploadResponse {
    /// The uploaded file metadata
    pub file: FileMetadata,
}

/// The file name, used as the display name of path-based uploads.
fn file_display_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(ToString::to_string)
}

/// The MIME type inferred from `path`'s extension, or an error naming the
/// way to set it explicitly instead.
pub(crate) fn mime_type_for_upload(
    path: &Path,
    explicit_alternative: &str,
) -> Result<&'static str, GenaiError> {
    crate::multimodal::detect_mime_type(path).ok_or_else(|| {
        GenaiError::InvalidInput(format!(
            "Could not determine MIME type for '{}'. Please use {explicit_alternative} to specify explicitly.",
            path.display()
        ))
    })
}

/// Where a [`FileUpload`] reads its contents from.
#[derive(Clone)]
pub(crate) enum UploadSource {
    /// A file on disk, streamed when the upload is sent.
    Path(PathBuf),
    /// Bytes already in memory.
    Bytes(Vec<u8>),
}

/// A file to upload with [`Files::upload`] or
/// [`FileSearchStores::upload`](crate::FileSearchStores::upload): a path or
/// bytes in memory, with an optional MIME type and display name.
///
/// ```
/// use genai_rs::FileUpload;
///
/// // MIME type from the extension, display name from the file name
/// let from_disk = FileUpload::from_path("notes.md");
///
/// // Bytes take their MIME type up front
/// let from_memory = FileUpload::from_bytes(b"a,b\n1,2\n".to_vec(), "text/csv")
///     .with_display_name("Q4 sales");
/// # let _ = (from_disk, from_memory);
/// ```
///
/// `Debug` prints the length of in-memory data, not its bytes.
#[derive(Clone)]
pub struct FileUpload {
    source: UploadSource,
    mime_type: Option<String>,
    display_name: Option<String>,
}

impl FileUpload {
    /// An upload of the file at `path`, read when it is sent.
    ///
    /// Nothing is read until the upload: [`Files::upload`] streams the file
    /// from disk, and
    /// [`FileSearchStores::upload`](crate::FileSearchStores::upload) reads it
    /// into memory. A missing, unreadable or empty file, or one over 2 GB,
    /// fails there with [`GenaiError::InvalidInput`] before any request. The
    /// MIME type comes from the file extension unless
    /// [`with_mime_type`](Self::with_mime_type) sets it (an extension it
    /// can't map fails the same way), and the display name is the file name
    /// unless [`with_display_name`](Self::with_display_name) sets one.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            source: UploadSource::Path(path.into()),
            mime_type: None,
            display_name: None,
        }
    }

    /// An upload of `data`, stored with `mime_type` as its content type and
    /// no display name.
    ///
    /// Empty data, data over 2 GB, or a MIME type that cannot be sent as a
    /// header value fails with [`GenaiError::InvalidInput`] when uploaded,
    /// before any request.
    #[must_use]
    pub fn from_bytes(data: Vec<u8>, mime_type: impl Into<String>) -> Self {
        Self {
            source: UploadSource::Bytes(data),
            mime_type: Some(mime_type.into()),
            display_name: None,
        }
    }

    /// Sets the MIME type, e.g. `video/mp4`, instead of the one inferred
    /// from a path's extension (or given to
    /// [`from_bytes`](Self::from_bytes)).
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Sets the display name the file is listed under
    /// ([`FileMetadata::display_name`], or
    /// [`FileSearchDocument::display_name`](crate::FileSearchDocument::display_name)
    /// in a store). A path upload defaults to the file name.
    #[must_use]
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    /// Splits the upload into its source, the MIME type to send and the
    /// display name set, if any.
    pub(crate) fn into_parts(self) -> Result<(UploadSource, String, Option<String>), GenaiError> {
        let mime_type = self.resolved_mime_type()?;
        Ok((self.source, mime_type, self.display_name))
    }

    /// The MIME type to send: the one set, or for a path, the one its
    /// extension maps to.
    fn resolved_mime_type(&self) -> Result<String, GenaiError> {
        match (&self.mime_type, &self.source) {
            (Some(mime_type), _) => Ok(mime_type.clone()),
            (None, UploadSource::Path(path)) => {
                mime_type_for_upload(path, "FileUpload::with_mime_type()").map(str::to_string)
            }
            // `from_bytes` takes a MIME type, and nothing unsets it.
            (None, UploadSource::Bytes(_)) => Err(GenaiError::InvalidInput(
                "an in-memory upload needs a MIME type".to_string(),
            )),
        }
    }
}

impl fmt::Debug for FileUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("FileUpload");
        match &self.source {
            UploadSource::Path(path) => debug.field("path", path),
            UploadSource::Bytes(data) => debug.field("data_len", &data.len()),
        };
        debug
            .field("mime_type", &self.mime_type)
            .field("display_name", &self.display_name)
            .finish()
    }
}

/// How long a wait such as [`Files::wait_until_active`] lasts, and how
/// often it polls. A value left unset takes that method's default.
///
/// ```
/// use genai_rs::PollOptions;
/// use std::time::Duration;
///
/// let poll = PollOptions::new()
///     .with_timeout(Duration::from_secs(300))
///     .with_poll_interval(Duration::from_secs(5));
/// # let _ = poll;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollOptions {
    timeout: Option<Duration>,
    poll_interval: Option<Duration>,
}

impl PollOptions {
    /// The waiting method's defaults for both values.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            timeout: None,
            poll_interval: None,
        }
    }

    /// Gives up once `timeout` has passed since the wait started. It is
    /// checked after each status request, so a wait can run over by up to
    /// one poll interval and one request.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sleeps `poll_interval` between status requests.
    #[must_use]
    pub const fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = Some(poll_interval);
        self
    }

    /// The timeout, or `default` when unset.
    pub(crate) fn timeout_or(self, default: Duration) -> Duration {
        self.timeout.unwrap_or(default)
    }

    /// The poll interval, or `default` when unset.
    pub(crate) fn poll_interval_or(self, default: Duration) -> Duration {
        self.poll_interval.unwrap_or(default)
    }
}

/// [`Files::wait_until_active`] gives up after this long by default.
const FILE_WAIT_TIMEOUT: Duration = Duration::from_secs(120);
/// [`Files::wait_until_active`] polls this often by default.
const FILE_POLL_INTERVAL: Duration = Duration::from_secs(2);

impl Client {
    /// The `/v1beta/files` resource (the Files API): upload, get, list and
    /// delete files, and wait for one to finish processing.
    ///
    /// The handle borrows the client and is `Copy`; see
    /// [names](crate::files#names) for what the methods take.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let file = client.files().get("files/abc123").await?;
    /// # let _ = file;
    /// # Ok(())
    /// # }
    /// ```
    pub fn files(&self) -> Files<'_> {
        Files { client: self }
    }
}

/// The `/v1beta/files` resource, from [`Client::files`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle: `client.files().get(name)` can be stored or
/// joined with others. See [names](crate::files#names).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct Files<'a> {
    client: &'a Client,
}

impl<'a> Files<'a> {
    /// Uploads a file, from disk or from memory, and returns its metadata.
    ///
    /// The file is stored for 48 hours and referenced in interactions by its
    /// URI ([`Content::from_file`](crate::Content::from_file)). That beats
    /// inline base64 for a large file or one used in several interactions.
    /// A path upload streams from disk with bounded memory (about 8 MB of
    /// read buffer) whatever the file size, up to the 2 GB limit. Some files,
    /// videos especially, need processing before use: see
    /// [`wait_until_active`](Self::wait_until_active).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`], before any request, if the file
    /// cannot be read, is empty or over 2 GB, or has no MIME type (a path
    /// whose extension doesn't map to one, without
    /// [`FileUpload::with_mime_type`]), or if the MIME type cannot be sent as
    /// a header value. Returns an API or network error if either request of
    /// the upload fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Content, FileUpload};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // From disk, MIME type from the extension
    /// let video = client.files().upload(FileUpload::from_path("video.mp4")).await?;
    /// println!("Uploaded: {} -> {}", video.name, video.uri);
    ///
    /// // From disk, with an explicit MIME type
    /// let data = client
    ///     .files()
    ///     .upload(FileUpload::from_path("data.bin").with_mime_type("application/octet-stream"))
    ///     .await?;
    ///
    /// // From memory
    /// let csv = client
    ///     .files()
    ///     .upload(FileUpload::from_bytes(b"a,b\n1,2\n".to_vec(), "text/csv").with_display_name("sales"))
    ///     .await?;
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_content(vec![
    ///         Content::text("Summarize this table"),
    ///         Content::from_file(&csv),
    ///     ])
    ///     .create()
    ///     .await?;
    /// # let _ = (data, response);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload(self, upload: FileUpload) -> Result<FileMetadata, GenaiError> {
        let (source, mime_type, display_name) = upload.into_parts()?;
        match source {
            UploadSource::Path(path) => {
                let display_name = display_name.or_else(|| file_display_name(&path));
                crate::http::files::upload_path(
                    &self.client.http,
                    &path,
                    &mime_type,
                    display_name.as_deref(),
                )
                .await
            }
            UploadSource::Bytes(data) => {
                crate::http::files::upload_bytes(
                    &self.client.http,
                    data,
                    &mime_type,
                    display_name.as_deref(),
                )
                .await
            }
        }
    }

    /// Gets a file's metadata, for example to check whether it has finished
    /// processing.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>` (see [names](crate::files#names)), and an API or network
    /// error if the request fails or the file doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let file = client.files().get("files/abc123").await?;
    /// if file.is_active() {
    ///     println!("File is ready to use");
    /// } else if file.is_processing() {
    ///     println!("File is still processing...");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get(self, file_name: &str) -> Result<FileMetadata, GenaiError> {
        crate::http::files::get_file(&self.client.http, file_name).await
    }

    /// Lists your uploaded files, newest first: configure the returned
    /// [`ListFiles`], then call [`send`](ListFiles::send) for one page, or
    /// [`pages`](ListFiles::pages) / [`items`](ListFiles::items) to stream
    /// them all.
    pub fn list(self) -> ListFiles<'a> {
        ListFiles {
            client: self.client,
            page_size: None,
            page_token: None,
        }
    }

    /// Deletes a file before its 48 hours are up.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>` (see [names](crate::files#names)), and an API or network
    /// error if the request fails or the file doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::FileUpload;
    ///
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let file = client.files().upload(FileUpload::from_path("video.mp4")).await?;
    /// // ... use in interactions ...
    /// client.files().delete(&file.name).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete(self, file_name: &str) -> Result<(), GenaiError> {
        crate::http::files::delete_file(&self.client.http, file_name).await
    }

    /// Polls a file until it is [`Active`](FileState::Active), then returns
    /// its metadata.
    ///
    /// Some files, videos especially, are processed after upload and can't
    /// be used until they are active. By default the wait gives up after
    /// 120 s and polls every 2 s; [`PollOptions`] changes either. A state
    /// the crate doesn't know yet is logged at `warn` and polled through.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::Internal`] if processing fails (terminal, so not
    /// retryable: only a new upload recovers) or the timeout passes,
    /// [`GenaiError::InvalidInput`] for a name that is not `files/<id>`, or
    /// the error of a failed status request.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{FileUpload, PollOptions};
    /// use std::time::Duration;
    ///
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let file = client.files().upload(FileUpload::from_path("large_video.mp4")).await?;
    ///
    /// // Poll every 5 s for up to 5 minutes
    /// let poll = PollOptions::new()
    ///     .with_poll_interval(Duration::from_secs(5))
    ///     .with_timeout(Duration::from_secs(300));
    /// let ready = client.files().wait_until_active(&file.name, poll).await?;
    /// println!("File ready: {}", ready.uri);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn wait_until_active(
        self,
        file_name: &str,
        poll: PollOptions,
    ) -> Result<FileMetadata, GenaiError> {
        let timeout = poll.timeout_or(FILE_WAIT_TIMEOUT);
        let poll_interval = poll.poll_interval_or(FILE_POLL_INTERVAL);
        let start = std::time::Instant::now();

        loop {
            let current = self.get(file_name).await?;

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
                tracing::error!("File '{}' processing failed: {}", file_name, detail);
                return Err(GenaiError::Internal(format!(
                    "File '{file_name}' failed processing ({detail}). This is terminal — \
                     re-uploading is the only recovery."
                )));
            }

            // Log unknown states per Evergreen logging strategy
            if let Some(state) = &current.state
                && state.is_unknown()
            {
                tracing::warn!(
                    "File '{}' is in unknown state {:?}, continuing to poll. \
                     This may indicate API evolution - consider updating genai-rs.",
                    file_name,
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
                    file_name,
                    start.elapsed(),
                    state_info
                )));
            }

            tracing::debug!(
                "File '{}' still processing, waiting {:?}...",
                file_name,
                poll_interval
            );
            tokio::time::sleep(poll_interval).await;
        }
    }
}

/// A `GET /v1beta/files` request, from [`Files::list`]: your uploaded
/// files, newest first.
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `nextPageToken` to the end of the list. The page size is sent with every
/// page. (The Files API spells its paging parameters `pageSize` and
/// `pageToken`; the builder sends them that way.)
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
/// use genai_rs::FileMetadata;
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client.files().list().with_page_size(10).send().await?;
/// for file in &page.files {
///     println!("{}: {} ({})", file.name, file.display_name.as_deref().unwrap_or(""), file.mime_type);
/// }
///
/// // Every file, across pages
/// let all: Vec<FileMetadata> = client.files().list().items().try_collect().await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListFiles<'a> {
    client: &'a Client,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListFiles<'a> {
    /// Sets the maximum number of files per page, from 1 to 100 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Sends the request and returns one page.
    ///
    /// With no files, the page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn send(self) -> Result<ListFilesResponse, GenaiError> {
        crate::http::files::list_files(
            &self.client.http,
            self.page_size,
            self.page_token.as_deref(),
        )
        .await
    }

    /// Streams every page, starting at [`with_page_token`](Self::with_page_token)
    /// or the first page.
    ///
    /// Nothing is sent until the stream is polled. It ends after a page
    /// without a `next_page_token`; an error is yielded once and ends it. A
    /// page whose token was already requested (the starting token included)
    /// is yielded, then [`GenaiError::MalformedResponse`]. The stream owns a
    /// clone of the client, so it can be stored or spawned.
    #[must_use = "streams do nothing unless polled"]
    pub fn pages(self) -> BoxStream<'static, Result<ListFilesResponse, GenaiError>> {
        let Self {
            client,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("files", page_token, move |token| {
            let client = client.clone();
            async move {
                crate::http::files::list_files(&client.http, page_size, token.as_deref()).await
            }
        })
    }

    /// Streams every file across pages, newest first. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<FileMetadata, GenaiError>> {
        paging::items(self.pages())
    }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
