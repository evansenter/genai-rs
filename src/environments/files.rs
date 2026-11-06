//! Files inside an environment (`/v1beta/environments/{id}/files/...`).
//! See the [parent module](super).

use std::fmt;

use super::Environments;
use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
use crate::serde_util::{
    ResourceName, deserialize_lenient_timestamp, deserialize_string_i64, serialize_string_i64,
};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

struct ForEnvironmentFile;
impl ResourceName for ForEnvironmentFile {
    const NAME: &'static str = "EnvironmentFile";
}

wire_enum! {
    /// Whether an environment entry is a file or a directory. Serializes in
    /// the uppercase form the API sends.
    pub enum EnvironmentFileType {
        /// A regular file.
        File = "FILE" | "file",
        /// A directory.
        Directory = "DIRECTORY" | "directory",
    }
    unknown(file_type, unknown_file_type)
}

/// A file or directory inside an environment.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct EnvironmentFile {
    /// Entry name, e.g. `main.py`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Path relative to the environment root, e.g. `src/main.py`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// File or directory.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub file_type: Option<EnvironmentFileType>,
    /// Size in bytes (an int64 string on the wire). Reported for files
    /// from environment sources; files written with
    /// [`upload`](EnvironmentFiles::upload) carry none.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForEnvironmentFile>"
    )]
    pub size_bytes: Option<i64>,
    /// MIME type, e.g. `text/plain; charset=utf-8`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// When the entry was created.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForEnvironmentFile>"
    )]
    pub created: Option<DateTime<Utc>>,
    /// When the entry was last modified.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForEnvironmentFile>"
    )]
    pub modified: Option<DateTime<Utc>>,
    /// Unmodeled fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A page of environment entries, from [`ListEnvironmentFiles`]. Also the
/// response to [`EnvironmentFiles::upload`], listing the entry written.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct EnvironmentFileListResponse {
    /// The entries in this page.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub files: Vec<EnvironmentFile>,
    /// Token for the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them (the other ten list envelopes have one).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(EnvironmentFileListResponse, files: EnvironmentFile);

/// A file to write into an environment, for [`EnvironmentFiles::upload`]:
/// its bytes, its MIME type, and whether to overwrite or unpack it.
///
/// ```
/// use genai_rs::EnvironmentFileUpload;
///
/// let upload = EnvironmentFileUpload::new(b"a,b\n1,2\n".to_vec(), "text/csv")
///     .with_overwrite(true);
/// ```
///
/// `Debug` prints the data's length, not its bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct EnvironmentFileUpload {
    pub(crate) data: Vec<u8>,
    pub(crate) mime_type: String,
    pub(crate) overwrite: bool,
    pub(crate) extract: bool,
}

impl EnvironmentFileUpload {
    /// An upload of `data`, stored with `mime_type` as its content type,
    /// neither overwriting nor extracting.
    ///
    /// Empty data, or a MIME type that cannot be sent as a header value,
    /// fails with [`GenaiError::InvalidInput`] when uploaded, before any
    /// request.
    #[must_use]
    pub fn new(data: Vec<u8>, mime_type: impl Into<String>) -> Self {
        Self {
            data,
            mime_type: mime_type.into(),
            overwrite: false,
            extract: false,
        }
    }

    /// Whether to replace a file already at the destination. Without it,
    /// uploading over an existing file fails with a `409` from the
    /// finalizing request, after the bytes are sent.
    #[must_use]
    pub fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }

    /// Whether to treat the data as a tar or tar.gz archive and unpack it
    /// into the destination path.
    #[must_use]
    pub fn with_extract(mut self, extract: bool) -> Self {
        self.extract = extract;
        self
    }
}

impl fmt::Debug for EnvironmentFileUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentFileUpload")
            .field("data_len", &self.data.len())
            .field("mime_type", &self.mime_type)
            .field("overwrite", &self.overwrite)
            .field("extract", &self.extract)
            .finish()
    }
}

/// A file or directory read out of an environment with
/// [`EnvironmentFiles::download`].
///
/// `Debug` prints the data's length, not its bytes.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EnvironmentFileDownload {
    /// The file's bytes, or for a directory, a tar archive of everything
    /// below it.
    pub data: Vec<u8>,
    /// The `Content-Type` the API served: the file's own (`text/csv;
    /// charset=utf-8` for a file uploaded as `text/csv`,
    /// `application/octet-stream` for a source file), or
    /// `application/x-tar` for a directory.
    pub mime_type: Option<String>,
}

impl EnvironmentFileDownload {
    /// Whether the API served a tar archive, as it does for a directory.
    #[must_use]
    pub fn is_tar_archive(&self) -> bool {
        self.mime_type
            .as_deref()
            .is_some_and(|m| m.split(';').next().map(str::trim) == Some("application/x-tar"))
    }
}

impl fmt::Debug for EnvironmentFileDownload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvironmentFileDownload")
            .field("data_len", &self.data.len())
            .field("mime_type", &self.mime_type)
            .finish()
    }
}

impl<'a> Environments<'a> {
    /// The files inside environments: list, download and upload them.
    ///
    /// Like [`Environments`], the handle binds no environment: each method
    /// takes the environment ID (see [IDs](crate::environments#ids)).
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let root = client.environments().files().list("env-123", "").send().await?;
    /// # let _ = root;
    /// # Ok(())
    /// # }
    /// ```
    pub fn files(self) -> EnvironmentFiles<'a> {
        EnvironmentFiles {
            client: self.client,
        }
    }
}

/// The files inside environments
/// (`/v1beta/environments/{id}/files/{path}`), from [`Environments::files`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle. Paths are relative to the environment root
/// (`""` is the root), and a `.` or `..` segment fails with
/// [`GenaiError::InvalidInput`] before any request. See
/// [IDs](crate::environments#ids).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct EnvironmentFiles<'a> {
    client: &'a Client,
}

impl<'a> EnvironmentFiles<'a> {
    /// Lists the entries at `path` in an environment (`""` for the root):
    /// configure the returned [`ListEnvironmentFiles`], then call
    /// [`send`](ListEnvironmentFiles::send) for one page, or
    /// [`pages`](ListEnvironmentFiles::pages) /
    /// [`items`](ListEnvironmentFiles::items) to stream them all.
    ///
    /// A directory lists its entries; a file path lists that file's own
    /// entry.
    pub fn list(self, environment_id: &str, path: &str) -> ListEnvironmentFiles<'a> {
        ListEnvironmentFiles::new(
            self.client,
            environment_id.to_owned(),
            path.to_owned(),
            false,
        )
    }

    /// Downloads the file at `path` in an environment: its bytes, or for a
    /// directory, a tar archive of everything below it
    /// ([`is_tar_archive`](EnvironmentFileDownload::is_tar_archive)).
    ///
    /// This is `GET` on the listing's URL with `alt=media`. The whole body is
    /// read into memory.
    ///
    /// ```no_run
    /// # async fn run(client: genai_rs::Client, env_id: &str) -> Result<(), genai_rs::GenaiError> {
    /// let report = client
    ///     .environments()
    ///     .files()
    ///     .download(env_id, "out/report.md")
    ///     .await?;
    /// println!("{}", String::from_utf8_lossy(&report.data));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for an invalid environment ID or
    /// a `.`/`..` path segment, before any request, and an API error when
    /// nothing is at `path`: a `404 Directory not found or is empty`, which
    /// the root (`""`) gets too.
    pub async fn download(
        self,
        environment_id: &str,
        path: &str,
    ) -> Result<EnvironmentFileDownload, GenaiError> {
        crate::http::environment_files::download_file(&self.client.http, environment_id, path).await
    }

    /// Uploads a file to `path` in an environment, returning the entry
    /// written.
    ///
    /// It uses the resumable upload protocol in one shot: one request starts
    /// the session, and a second sends the bytes and finalizes.
    ///
    /// ```no_run
    /// # async fn run(client: genai_rs::Client, env_id: &str) -> Result<(), genai_rs::GenaiError> {
    /// use genai_rs::EnvironmentFileUpload;
    ///
    /// let written = client
    ///     .environments()
    ///     .files()
    ///     .upload(
    ///         env_id,
    ///         "data/input.csv",
    ///         EnvironmentFileUpload::new(b"a,b\n1,2\n".to_vec(), "text/csv").with_overwrite(true),
    ///     )
    ///     .await?;
    /// println!("{:?}", written.files[0].path);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for an invalid environment ID,
    /// empty data, a `.`/`..` path segment, or a MIME type that cannot be
    /// sent as a header value, or an error if either upload request fails
    /// (a file already at `path` without
    /// [`with_overwrite`](EnvironmentFileUpload::with_overwrite) is a `409`).
    pub async fn upload(
        self,
        environment_id: &str,
        path: &str,
        upload: EnvironmentFileUpload,
    ) -> Result<EnvironmentFileListResponse, GenaiError> {
        crate::http::environment_files::upload_file(&self.client.http, environment_id, path, upload)
            .await
    }
}

paging::list_builder! {
    /// A `GET /v1beta/environments/{id}/files/{path}` request, from
    /// [`EnvironmentFiles::list`].
    ///
    /// End it with [`send`](Self::send) for one page, or
    /// [`pages`](Self::pages) / [`items`](Self::items) to follow
    /// `next_page_token` to the end of the listing. The page size and
    /// [`with_recursive`](Self::with_recursive) are sent with every page (a page
    /// token without `recursive` lists something else). An
    /// invalid environment ID or a `.`/`..` path segment fails with
    /// [`GenaiError::InvalidInput`] before any request (from `.send()`, or as
    /// the streams' only item).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use futures_util::TryStreamExt;
    ///
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// // One page of the root directory
    /// let page = client.environments().files().list("env-123", "").send().await?;
    /// for entry in &page.files {
    ///     println!("{:?} {:?}", entry.file_type, entry.path);
    /// }
    ///
    /// // Everything under `src/`, across pages
    /// let all: Vec<genai_rs::EnvironmentFile> = client
    ///     .environments()
    ///     .files()
    ///     .list("env-123", "src")
    ///     .with_recursive(true)
    ///     .items()
    ///     .try_collect()
    ///     .await?;
    /// # let _ = all;
    /// # Ok(())
    /// # }
    /// ```
    pub struct ListEnvironmentFiles<'a> {
        environment_id: String,
        path: String,
        recursive: bool,
    }
    page: EnvironmentFileListResponse => EnvironmentFile;
    label: "environment files";

    /// Sets the maximum number of entries per page. Sent with every page.
    fn with_page_size;

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    fn with_page_token;

    /// Sends the request and returns one page.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid environment ID, path or page token,
    /// when the environment or path doesn't exist (404), on network
    /// failure, or when the response fails to parse.
    fn send;

    /// Streams every entry across pages, in server order. Same rules as
    /// [`pages`](Self::pages).
    fn items;

    fetch: |http, page_size, token| crate::http::environment_files::list_files(
        http,
        &environment_id,
        &path,
        recursive,
        page_size,
        token.as_deref(),
    ),
}

impl ListEnvironmentFiles<'_> {
    /// Whether to list everything below a directory, not only its direct
    /// entries. Sent (as `recursive=true`) with every page, and only when
    /// true.
    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Captured from a live listing and upload finalize (2026-09-24).
    #[test]
    fn listing_deserializes_the_live_shape() {
        let list: EnvironmentFileListResponse = serde_json::from_value(json!({
            "files": [
                {"name": ".agents", "path": ".agents", "type": "DIRECTORY",
                 "created": "2026-09-23T14:47:11Z", "modified": "2026-09-23T14:47:11Z"},
                {"name": "hello.txt", "path": "sweep/hello.txt", "type": "FILE",
                 "size_bytes": "17", "mime_type": "text/plain; charset=utf-8",
                 "created": "2026-09-24T01:46:18Z", "modified": "2026-09-24T01:46:18Z"}
            ]
        }))
        .unwrap();
        assert_eq!(
            list.files[0].file_type,
            Some(EnvironmentFileType::Directory)
        );
        let file = &list.files[1];
        assert_eq!(file.file_type, Some(EnvironmentFileType::File));
        assert_eq!(file.size_bytes, Some(17));
        assert!(file.modified.is_some());

        let back = serde_json::to_value(file).unwrap();
        assert_eq!(back["type"], "FILE");
        assert_eq!(back["size_bytes"], "17");
    }

    #[test]
    fn lowercase_binding_spelling_is_accepted() {
        let file: EnvironmentFile = serde_json::from_value(json!({"type": "directory"})).unwrap();
        assert_eq!(file.file_type, Some(EnvironmentFileType::Directory));
    }

    #[test]
    fn upload_new_carries_the_payload_and_writes_nothing_over() {
        let upload = EnvironmentFileUpload::new(b"a,b\n".to_vec(), "text/csv");
        assert_eq!(upload.data, b"a,b\n");
        assert_eq!(upload.mime_type, "text/csv");
        assert!(!upload.overwrite);
        assert!(!upload.extract);
        // `impl Into<String>` takes an owned string too.
        assert_eq!(
            EnvironmentFileUpload::new(Vec::new(), String::from("application/gzip")).mime_type,
            "application/gzip"
        );
    }

    #[test]
    fn upload_setters_replace() {
        let upload = EnvironmentFileUpload::new(b"x".to_vec(), "application/x-tar")
            .with_overwrite(true)
            .with_extract(true);
        assert!(upload.overwrite);
        assert!(upload.extract);

        let upload = upload.with_overwrite(false).with_extract(false);
        assert_eq!(
            upload,
            EnvironmentFileUpload::new(b"x".to_vec(), "application/x-tar")
        );
    }

    #[test]
    fn upload_debug_shows_the_length_not_the_bytes() {
        let upload =
            EnvironmentFileUpload::new(b"top-secret".to_vec(), "text/plain").with_overwrite(true);
        let debug = format!("{upload:?}");
        assert!(!debug.contains("top-secret"), "{debug}");
        assert!(!debug.contains("116, 111"), "no byte values: {debug}");
        assert!(debug.contains("data_len: 10"), "{debug}");
        assert!(debug.contains("text/plain"), "{debug}");
        assert!(debug.contains("overwrite: true"), "{debug}");
    }

    #[test]
    fn list_environment_files_owns_its_ids_and_query() {
        let client = Client::new("k".to_string());
        let (environment_id, path) = (String::from("env-1"), String::from("src"));
        let list = client.environments().files().list(&environment_id, &path);
        // The builder owns its IDs, so the caller's strings can go.
        drop((environment_id, path));
        assert_eq!(list.environment_id, "env-1");
        assert_eq!(list.path, "src");
        assert!(!list.recursive);
        assert_eq!(list.page_size, None);
        assert_eq!(list.page_token, None);

        let list = list
            .with_recursive(true)
            .with_page_size(10)
            .with_page_token("t1")
            // `with_*` replaces.
            .with_page_size(2)
            .with_page_token("t2");
        assert!(list.recursive);
        assert_eq!(list.page_size, Some(2));
        assert_eq!(list.page_token.as_deref(), Some("t2"));
        assert!(!list.with_recursive(false).recursive);
    }

    #[test]
    fn environment_files_handle_and_list_debug_redact_the_api_key() {
        let client = Client::new("secret-api-key".to_string());
        for debug in [
            format!("{:?}", client.environments().files()),
            format!(
                "{:?}",
                client
                    .environments()
                    .files()
                    .list("env-1", "")
                    .with_recursive(true)
            ),
        ] {
            assert!(!debug.contains("secret-api-key"), "{debug}");
            assert!(debug.contains("[REDACTED]"), "{debug}");
        }
    }
}
