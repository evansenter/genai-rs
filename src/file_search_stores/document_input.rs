//! What [`FileSearchStores::upload`](super::FileSearchStores::upload) and
//! [`FileSearchStores::import_file`](super::FileSearchStores::import_file)
//! take: the file, and how the store indexes it.

use super::types::{ChunkingConfig, CustomMetadata};
use crate::errors::GenaiError;
use crate::files::{FileMetadata, FileUpload};
use crate::poll::PollOptions;
use serde::Serialize;

/// The indexing options an upload and an import share, in their wire form
/// (`customMetadata`, `chunkingConfig`): the bindings'
/// `UploadToFileSearchStoreConfig` and `ImportFileConfig` both carry them.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentOptions {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) custom_metadata: Vec<CustomMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) chunking_config: Option<ChunkingConfig>,
}

impl DocumentOptions {
    /// Rejects a white-space overlap that is not smaller than its chunk. The
    /// API answers one with a `500 Internal error` (live 2026-11-05: an
    /// overlap equal to the chunk and one larger), which reads as
    /// retryable, so it is caught before any request.
    pub(crate) fn validate(&self) -> Result<(), GenaiError> {
        let Some(white_space) = self
            .chunking_config
            .as_ref()
            .and_then(|c| c.white_space_config.as_ref())
        else {
            return Ok(());
        };
        if let (Some(chunk), Some(overlap)) = (
            white_space.max_tokens_per_chunk,
            white_space.max_overlap_tokens,
        ) && overlap >= chunk
        {
            return Err(GenaiError::InvalidInput(format!(
                "white-space chunking needs max_overlap_tokens ({overlap}) smaller than \
                 max_tokens_per_chunk ({chunk}); the API fails the request with a 500 otherwise"
            )));
        }
        Ok(())
    }
}

/// A document to upload into a file search store with
/// [`FileSearchStores::upload`](super::FileSearchStores::upload): a
/// [`FileUpload`], plus the custom metadata to attach to it and how to chunk
/// it.
///
/// A [`FileUpload`] converts into one with neither, so `upload` takes either.
///
/// ```
/// use genai_rs::{ChunkingConfig, CustomMetadata, DocumentUpload, FileUpload};
///
/// let document = DocumentUpload::new(FileUpload::from_path("handbook.pdf"))
///     .add_custom_metadata(CustomMetadata::string("team", "search"))
///     .add_custom_metadata(CustomMetadata::numeric("year", 2026.0))
///     .with_chunking_config(ChunkingConfig::white_space(200, 20));
/// # let _ = document;
/// ```
#[derive(Clone, Debug)]
pub struct DocumentUpload {
    pub(crate) file: FileUpload,
    pub(crate) options: DocumentOptions,
}

impl DocumentUpload {
    /// An upload of `file`, with no custom metadata and the store's default
    /// chunking.
    #[must_use]
    pub fn new(file: FileUpload) -> Self {
        Self {
            file,
            options: DocumentOptions::default(),
        }
    }

    /// Adds a key-value pair to the document's
    /// [`custom_metadata`](super::FileSearchDocument::custom_metadata).
    #[must_use]
    pub fn add_custom_metadata(mut self, metadata: CustomMetadata) -> Self {
        self.options.custom_metadata.push(metadata);
        self
    }

    /// Sets how the store chunks the document.
    #[must_use]
    pub fn with_chunking_config(mut self, chunking: ChunkingConfig) -> Self {
        self.options.chunking_config = Some(chunking);
        self
    }
}

impl From<FileUpload> for DocumentUpload {
    fn from(file: FileUpload) -> Self {
        Self::new(file)
    }
}

/// A file already in the Files API to import into a file search store with
/// [`FileSearchStores::import_file`](super::FileSearchStores::import_file),
/// with the custom metadata to attach and how to chunk it.
///
/// It names the file by its full resource name, `files/<id>`
/// ([`FileMetadata::name`]); a name, a `String` or a `&FileMetadata`
/// converts into an import with no options. Importing copies the file into
/// the store, so deleting the file afterwards leaves the document in place.
///
/// ```
/// use genai_rs::{CustomMetadata, DocumentImport, PollOptions};
/// use std::time::Duration;
///
/// let import = DocumentImport::new("files/abc123")
///     .add_custom_metadata(CustomMetadata::string_list("tags", ["faq", "billing"]))
///     .with_poll_options(PollOptions::new().with_timeout(Duration::from_secs(300)));
/// # let _ = import;
/// ```
#[derive(Clone, Debug)]
pub struct DocumentImport {
    pub(crate) file_name: String,
    pub(crate) options: DocumentOptions,
    pub(crate) poll: PollOptions,
}

impl DocumentImport {
    /// An import of the file named `file_name` (`files/<id>`).
    #[must_use]
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            options: DocumentOptions::default(),
            poll: PollOptions::new(),
        }
    }

    /// Adds a key-value pair to the document's
    /// [`custom_metadata`](super::FileSearchDocument::custom_metadata).
    #[must_use]
    pub fn add_custom_metadata(mut self, metadata: CustomMetadata) -> Self {
        self.options.custom_metadata.push(metadata);
        self
    }

    /// Sets how the store chunks the document.
    #[must_use]
    pub fn with_chunking_config(mut self, chunking: ChunkingConfig) -> Self {
        self.options.chunking_config = Some(chunking);
        self
    }

    /// How long to wait for the import operation to finish, and how often to
    /// check it: by default up to 60 s, every 500 ms.
    #[must_use]
    pub fn with_poll_options(mut self, poll: PollOptions) -> Self {
        self.poll = poll;
        self
    }
}

impl From<&str> for DocumentImport {
    fn from(file_name: &str) -> Self {
        Self::new(file_name)
    }
}

impl From<String> for DocumentImport {
    fn from(file_name: String) -> Self {
        Self::new(file_name)
    }
}

impl From<&FileMetadata> for DocumentImport {
    fn from(file: &FileMetadata) -> Self {
        Self::new(file.name.clone())
    }
}

#[cfg(test)]
#[path = "document_input_tests.rs"]
mod tests;
