//! Files inside an environment (`/v1beta/environments/{id}/files/...`).
//!
//! List what an agent left in an environment, and upload files into one
//! before an interaction runs. Paths are relative to the environment root;
//! `""` lists the root.
//!
//! Verified live 2026-09-24: listing (root, a directory, one file,
//! recursive) and a resumable single-shot upload. The API reports entry
//! types in uppercase (`FILE`, `DIRECTORY`) although the bindings spell them
//! lowercase; both are accepted. Environments are forked with
//! [`CreateEnvironmentRequest::from_environment`](crate::CreateEnvironmentRequest::from_environment).

use crate::client::Client;
use crate::errors::GenaiError;
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
    /// Size in bytes (an int64 string on the wire).
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

/// A page of environment entries. Also the response to an upload, listing
/// the entry written.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct EnvironmentFileList {
    /// The entries in this page.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub files: Vec<EnvironmentFile>,
    /// Token for the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Options for [`Client::upload_environment_file`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentFileUpload {
    /// Replace an existing file at the destination.
    pub overwrite: bool,
    /// Treat the upload as a tar/tar.gz archive and unpack it into the
    /// destination path.
    pub extract: bool,
}

impl Client {
    /// Lists the entries at `path` in an environment (`""` for the root).
    /// A file path returns that file's own entry.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment or path doesn't exist (404), the
    /// path contains a `.`/`..` segment, or the request fails.
    pub async fn list_environment_files(
        &self,
        environment_id: &str,
        path: &str,
        recursive: bool,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<EnvironmentFileList, GenaiError> {
        crate::http::environment_files::list_files(
            &self.http,
            environment_id,
            path,
            recursive,
            page_size,
            page_token,
        )
        .await
    }

    /// Uploads `data` to `path` in an environment.
    ///
    /// ```no_run
    /// # async fn run(client: genai_rs::Client, env_id: &str) -> Result<(), genai_rs::GenaiError> {
    /// use genai_rs::EnvironmentFileUpload;
    ///
    /// let written = client
    ///     .upload_environment_file(
    ///         env_id,
    ///         "data/input.csv",
    ///         b"a,b\n1,2\n".to_vec(),
    ///         "text/csv",
    ///         EnvironmentFileUpload { overwrite: true, ..Default::default() },
    ///     )
    ///     .await?;
    /// println!("{:?}", written.files[0].path);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for empty data or a `.`/`..`
    /// path segment, or an error if either upload request fails.
    pub async fn upload_environment_file(
        &self,
        environment_id: &str,
        path: &str,
        data: Vec<u8>,
        mime_type: &str,
        options: EnvironmentFileUpload,
    ) -> Result<EnvironmentFileList, GenaiError> {
        crate::http::environment_files::upload_file(
            &self.http,
            environment_id,
            path,
            data,
            mime_type,
            options,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Captured from a live listing and upload finalize (2026-09-24).
    #[test]
    fn listing_deserializes_the_live_shape() {
        let list: EnvironmentFileList = serde_json::from_value(json!({
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
}
