//! [`FileUpload`]: a file to upload, from disk or from memory, for
//! [`Files::upload`](crate::Files::upload) and
//! [`FileSearchStores::upload`](crate::FileSearchStores::upload).

use crate::errors::GenaiError;
use std::fmt;
use std::path::{Path, PathBuf};

/// The file name, used as the display name of path-based uploads.
pub(super) fn file_display_name(path: &Path) -> Option<String> {
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

/// A file to upload with [`Files::upload`](crate::Files::upload) or
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
    /// Nothing is read until the upload: [`Files::upload`](crate::Files::upload) streams the file
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
    /// ([`FileMetadata::display_name`](crate::FileMetadata::display_name), or
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

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
