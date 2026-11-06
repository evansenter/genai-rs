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

mod upload;

pub use upload::FileUpload;
pub(crate) use upload::UploadSource;
use upload::file_display_name;

use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
pub use crate::poll::PollOptions;
use crate::poll::PollStep;
use crate::serde_util::{ResourceName, deserialize_string_i64, serialize_string_i64};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

struct ForFile;
impl ResourceName for ForFile {
    const NAME: &'static str = "FileMetadata";
}

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

    /// Size of the file in bytes.
    ///
    /// The API sends this protobuf int64 as a JSON string (`"17"`, live
    /// 2026-11-05); a plain number is accepted too, and it serializes back as
    /// a string.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForFile>"
    )]
    pub size_bytes: Option<i64>,

    /// Timestamp when the file was created (ISO 8601 UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<DateTime<Utc>>,

    /// Timestamp of the file's last change (ISO 8601 UTC): its upload, or the
    /// end of its processing (live 2026-11-05).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<DateTime<Utc>>,

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

    /// Where the file came from: [`FileSource::Uploaded`] for every file
    /// [`Files::upload`] makes (live 2026-11-05).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<FileSource>,

    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen), as on the other resource types.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

wire_enum! {
    /// Where a file came from ([`FileMetadata::source`]).
    pub enum FileSource {
        /// Uploaded with [`Files::upload`] (live 2026-11-05).
        Uploaded = "UPLOADED",
        /// Generated by the API, such as a generated video (google-genai
        /// 2.34.0; not seen live).
        Generated = "GENERATED",
        /// Registered from an existing storage URI (google-genai 2.34.0;
        /// not seen live, and the crate has no register call).
        Registered = "REGISTERED",
    }
    unknown(source_type, unknown_source_type)
}

/// Why a file failed processing: a `google.rpc.Status`.
///
/// A video that cannot be decoded fails with `code` 3 and the message "The
/// file failed to be processed.", and no `details` (live 2026-11-05).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileError {
    /// The `google.rpc.Code`, such as 3 (`INVALID_ARGUMENT`); not an HTTP
    /// status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    /// Error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Typed detail messages, each an object with an `@type` key, as in an
    /// API error envelope's `details`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.code, &self.message) {
            (Some(code), Some(msg)) => write!(f, "error {code}: {msg}"),
            (Some(code), None) => write!(f, "error {code}"),
            (None, Some(msg)) => write!(f, "{msg}"),
            (None, None) => write!(f, "unknown error"),
        }
    }
}

/// Metadata for video files.
///
/// The bindings type it `dict[str, Any]`; the API serves `videoDuration`
/// (live 2026-11-05: `{"videoDuration": "1s"}` for a one-second clip).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct VideoMetadata {
    /// Duration of the video
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_duration: Option<String>,
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen). The bindings type the whole object as a
    /// free-form map.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing files.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileListResponse {
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

paging::impl_list_page!(FileListResponse, files: FileMetadata);

/// Wrapper for file upload response.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileUploadResponse {
    /// The uploaded file metadata
    pub file: FileMetadata,
    /// Fields the crate does not model yet, preserved for roundtrip.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
        ListFiles::new(self.client)
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
    /// Returns [`GenaiError::ProcessingFailed`] if processing fails
    /// (terminal: only a new upload recovers), [`GenaiError::PollTimeout`] if
    /// the timeout passes (the file may still be processing; neither is
    /// retryable), [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>`, or the error of a failed status request.
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
        let what = format!("file '{file_name}'");
        poll.wait(
            &what,
            (FILE_WAIT_TIMEOUT, FILE_POLL_INTERVAL),
            || self.get(file_name),
            |current| {
                if current.is_active() {
                    return Ok(PollStep::Ready(current));
                }
                if current.is_failed() {
                    // Not `Api`: the file will never process, and a fabricated
                    // 5xx would make `is_retryable()` say otherwise.
                    // `FileError::code` is a google.rpc code, not an HTTP status.
                    let detail = current
                        .error
                        .as_ref()
                        .map_or_else(|| "no details".to_string(), ToString::to_string);
                    tracing::error!("File '{file_name}' processing failed: {detail}");
                    return Err(GenaiError::ProcessingFailed(format!(
                        "File '{file_name}' failed processing ({detail}). This is terminal — \
                         re-uploading is the only recovery."
                    )));
                }
                // Log unknown states per Evergreen logging strategy
                if let Some(state) = &current.state
                    && state.is_unknown()
                {
                    tracing::warn!(
                        "File '{file_name}' is in unknown state {state:?}, continuing to poll. \
                         This may indicate API evolution - consider updating genai-rs."
                    );
                }
                Ok(PollStep::Pending(current.state.as_ref().map_or_else(
                    || "unknown".to_string(),
                    |s| format!("{s:?}"),
                )))
            },
        )
        .await
    }
}

paging::list_builder! {
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
    pub struct ListFiles<'a> {
    }
    page: FileListResponse => FileMetadata;
    label: "files";

    /// Sets the maximum number of files per page, from 1 to 100 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    fn with_page_size;

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    fn with_page_token;

    /// Sends the request and returns one page.
    ///
    /// With no files, the page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    fn send;

    /// Streams every file across pages, newest first. Same rules as
    /// [`pages`](Self::pages).
    fn items;

    fetch: |http, page_size, token| crate::http::files::list_files(
        http,
        page_size,
        token.as_deref(),
    ),
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
