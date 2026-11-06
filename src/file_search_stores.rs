//! File Search Store resource (`/v1beta/fileSearchStores`) and the
//! documents in each store.
//!
//! A file search store holds documents that [`Tool::FileSearch`](crate::Tool::FileSearch)
//! retrieves over. The tool takes store names, so without these endpoints a
//! caller has to provision stores outside the crate before file search is
//! usable at all.
//!
//! Manage stores through the [`FileSearchStores`] handle from
//! [`Client::file_search_stores`]: [`create`](FileSearchStores::create),
//! [`get`](FileSearchStores::get), [`list`](FileSearchStores::list),
//! [`delete`](FileSearchStores::delete) /
//! [`force_delete`](FileSearchStores::force_delete), and
//! [`upload`](FileSearchStores::upload) or
//! [`import_file`](FileSearchStores::import_file) to add a document, with
//! [custom metadata](CustomMetadata) and [chunking](ChunkingConfig) if
//! wanted. Its nested [`documents`](FileSearchStores::documents) handle
//! lists, gets and deletes documents, and
//! [waits](FileSearchDocuments::wait_until_active) for one to be indexed.
//!
//! # Names
//!
//! Methods take full resource names, the form [`FileSearchStore::name`] and
//! [`FileSearchDocument::name`] hold: `fileSearchStores/<id>` for a store and
//! `fileSearchStores/<store>/documents/<id>` for a document. Anything else (a
//! bare ID, a missing or extra segment, an empty or dot-segment ID) fails
//! locally with [`GenaiError::InvalidInput`] before any request; each ID is
//! percent-encoded into one path segment.
//!
//! # Wire format note
//!
//! Unlike the Interactions API, which uses snake_case throughout, this
//! resource returns **camelCase** (`displayName`, `createTime`,
//! `embeddingModel`, `sizeBytes`). Verified live 2026-08-16. The types here
//! therefore carry explicit `rename_all = "camelCase"`, deliberately
//! diverging from the rest of the crate — see `docs/ENUM_WIRE_FORMATS.md`.
//!
//! `sizeBytes` and the document counts are returned as **JSON strings**, not
//! numbers, and go through the crate's shared protobuf-JSON int64 helpers
//! accordingly.
//!
//! # Example
//!
//! ```no_run
//! use genai_rs::{
//!     Client, CreateFileSearchStoreRequest, CustomMetadata, DocumentUpload, FileUpload,
//!     PollOptions,
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new("api-key".to_string());
//! let stores = client.file_search_stores();
//!
//! // Provision a store and add a document.
//! let store = stores
//!     .create(&CreateFileSearchStoreRequest::new().with_display_name("my-docs"))
//!     .await?;
//! let document = stores
//!     .upload(
//!         &store.name,
//!         DocumentUpload::new(FileUpload::from_path("handbook.pdf").with_display_name("handbook"))
//!             .add_custom_metadata(CustomMetadata::string("team", "search")),
//!     )
//!     .await?;
//!
//! // Documents are indexed asynchronously; wait before querying.
//! stores
//!     .documents()
//!     .wait_until_active(&document.name, PollOptions::new())
//!     .await?;
//!
//! // ... query it via Tool::FileSearch with store.name ...
//!
//! // A store that still holds documents needs the forced delete.
//! stores.force_delete(&store.name).await?;
//! # Ok(())
//! # }
//! ```

mod document_input;
mod list;
mod types;

pub(crate) use document_input::DocumentOptions;
pub use document_input::{DocumentImport, DocumentUpload};
pub use list::{ListFileSearchDocuments, ListFileSearchStores};
pub use types::{
    ChunkingConfig, CreateFileSearchStoreRequest, CustomMetadata, DocumentState,
    FileSearchDocument, FileSearchDocumentListResponse, FileSearchStore,
    FileSearchStoreListResponse, StringList, WhiteSpaceConfig,
};

use crate::client::Client;
use crate::errors::GenaiError;
use crate::poll::{PollOptions, PollStep};
use std::time::Duration;

/// [`FileSearchDocuments::wait_until_active`] gives up after this long by
/// default.
const DOCUMENT_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
/// [`FileSearchDocuments::wait_until_active`] polls this often by default.
const DOCUMENT_POLL_INTERVAL: Duration = Duration::from_millis(500);

impl Client {
    /// The `/v1beta/fileSearchStores` resource: create, get, list and delete
    /// file search stores, upload documents into them, and reach their
    /// documents through [`documents`](FileSearchStores::documents).
    ///
    /// The handle borrows the client and is `Copy`; see
    /// [names](crate::file_search_stores#names) for what the methods take.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let store = client
    ///     .file_search_stores()
    ///     .get("fileSearchStores/my-docs-4kws71n2ybpr")
    ///     .await?;
    /// # let _ = store;
    /// # Ok(())
    /// # }
    /// ```
    pub fn file_search_stores(&self) -> FileSearchStores<'_> {
        FileSearchStores { client: self }
    }
}

/// The `/v1beta/fileSearchStores` resource, from
/// [`Client::file_search_stores`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle. See [names](crate::file_search_stores#names).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct FileSearchStores<'a> {
    client: &'a Client,
}

impl<'a> FileSearchStores<'a> {
    /// Creates a file search store.
    ///
    /// The returned [`FileSearchStore::name`] is what
    /// [`Tool::FileSearch`](crate::Tool::FileSearch) takes in `store_names`.
    ///
    /// The request's display name is a human-readable label; the API derives
    /// the resource name from it by stripping non-alphanumeric characters and
    /// appending a unique suffix, so the two are related but not
    /// interchangeable. Fields the crate does not model yet go in
    /// [`CreateFileSearchStoreRequest::extra`].
    ///
    /// # Errors
    ///
    /// Returns an API or network error if the request fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, CreateFileSearchStoreRequest};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let store = client
    ///     .file_search_stores()
    ///     .create(&CreateFileSearchStoreRequest::new().with_display_name("my-docs"))
    ///     .await?;
    /// println!("created {}", store.name);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(
        self,
        request: &CreateFileSearchStoreRequest,
    ) -> Result<FileSearchStore, GenaiError> {
        crate::http::file_search_stores::create_file_search_store(&self.client.http, request).await
    }

    /// Gets a file search store.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `fileSearchStores/<id>` (see [names](crate::file_search_stores#names)),
    /// and an API or network error if the request fails or the store doesn't
    /// exist.
    pub async fn get(self, store_name: &str) -> Result<FileSearchStore, GenaiError> {
        crate::http::file_search_stores::get_file_search_store(&self.client.http, store_name).await
    }

    /// Lists your file search stores, oldest first: configure the returned
    /// [`ListFileSearchStores`], then call
    /// [`send`](ListFileSearchStores::send) for one page, or
    /// [`pages`](ListFileSearchStores::pages) /
    /// [`items`](ListFileSearchStores::items) to stream them all.
    pub fn list(self) -> ListFileSearchStores<'a> {
        ListFileSearchStores::new(self.client)
    }

    /// Deletes an empty file search store.
    ///
    /// A store that still holds documents is rejected with a 400
    /// `FAILED_PRECONDITION` ("Cannot delete non-empty FileSearchStore");
    /// [`force_delete`](Self::force_delete) deletes it with its documents.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `fileSearchStores/<id>`, and an API or network error if the request
    /// fails, the store doesn't exist, or it still holds documents.
    pub async fn delete(self, store_name: &str) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_file_search_store(
            &self.client.http,
            store_name,
            false,
        )
        .await
    }

    /// Deletes a file search store and every document in it (sent as
    /// `force=true`).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `fileSearchStores/<id>`, and an API or network error if the request
    /// fails or the store doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
    /// client.file_search_stores().force_delete(store_name).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn force_delete(self, store_name: &str) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_file_search_store(
            &self.client.http,
            store_name,
            true,
        )
        .await
    }

    /// Uploads a document into a file search store, from disk or from
    /// memory, and returns it.
    ///
    /// Takes a [`FileUpload`](crate::FileUpload), or a [`DocumentUpload`] wrapping one with the
    /// [custom metadata](DocumentUpload::add_custom_metadata) to attach and
    /// the [chunking](DocumentUpload::with_chunking_config) to index it with.
    /// The MIME type is the one set on the [`FileUpload`](crate::FileUpload), or for a path, the
    /// one its extension maps to, as for [`Files::upload`](crate::Files::upload).
    /// The document's display name is the one set with
    /// [`FileUpload::with_display_name`](crate::FileUpload::with_display_name); without one, a path upload is named
    /// after its file and an in-memory upload is unnamed (live 2026-11-05).
    ///
    /// The document is indexed asynchronously and starts in
    /// [`DocumentState::Pending`]; file search will not match it until it
    /// reaches [`Active`](DocumentState::Active). See
    /// [`FileSearchDocuments::wait_until_active`].
    ///
    /// The upload uses the resumable protocol, as the Files API does: one
    /// request sends the document's settings as JSON and opens a session, a
    /// second sends the file and finishes it, and the API answers with an
    /// operation naming the new document, which a third request reads back.
    /// A path upload streams the file from disk, so memory use does not grow
    /// with it; an interrupted upload starts over.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`], before any request, for a store
    /// name that is not `fileSearchStores/<id>`, a file that cannot be read,
    /// empty contents or contents over 2 GB, no MIME type (a path whose
    /// extension doesn't map to one, without [`FileUpload::with_mime_type`](crate::FileUpload::with_mime_type)),
    /// a MIME type that cannot be sent as a header value, or a white-space
    /// chunking overlap not smaller than its chunk (which the API fails with
    /// a 500, live 2026-11-05). MIME *syntax* is not checked: `nonsense` or
    /// `text/` reach the API and come back as an API error. Returns an API or
    /// network error if the upload fails, and
    /// [`GenaiError::MalformedResponse`] if its answer names no document. If the upload succeeds but reading the document back
    /// fails, the error names the created document.
    ///
    /// The 2 GB ceiling is borrowed from the Files API and has not been
    /// verified for this resource, so treat it as a local guard rather than
    /// the API's limit.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{ChunkingConfig, CustomMetadata, DocumentUpload, FileUpload, PollOptions};
    ///
    /// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
    /// // From disk, MIME type from the extension, named after the file
    /// let handbook = client
    ///     .file_search_stores()
    ///     .upload(store_name, FileUpload::from_path("handbook.pdf"))
    ///     .await?;
    ///
    /// // From memory, with custom metadata and smaller chunks
    /// let memo = client
    ///     .file_search_stores()
    ///     .upload(
    ///         store_name,
    ///         DocumentUpload::new(
    ///             FileUpload::from_bytes(b"Q4 budget: 4.2M".to_vec(), "text/plain")
    ///                 .with_display_name("budget-memo"),
    ///         )
    ///         .add_custom_metadata(CustomMetadata::numeric("quarter", 4.0))
    ///         .with_chunking_config(ChunkingConfig::white_space(100, 10)),
    ///     )
    ///     .await?;
    ///
    /// for document in [&handbook, &memo] {
    ///     client
    ///         .file_search_stores()
    ///         .documents()
    ///         .wait_until_active(&document.name, PollOptions::new())
    ///         .await?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload(
        self,
        store_name: &str,
        upload: impl Into<DocumentUpload>,
    ) -> Result<FileSearchDocument, GenaiError> {
        let DocumentUpload { file, options } = upload.into();
        let (source, mime_type, display_name) = file.into_parts()?;
        crate::http::file_search_stores::upload_to_file_search_store(
            &self.client.http,
            store_name,
            source,
            display_name.as_deref(),
            &mime_type,
            &options,
        )
        .await
    }

    /// Imports a file from the Files API into a file search store, waits for
    /// the import to finish, and returns the new document.
    ///
    /// Takes the file's full name (`files/<id>`), its
    /// [`FileMetadata`](crate::FileMetadata), or a [`DocumentImport`] that adds
    /// the [custom metadata](DocumentImport::add_custom_metadata) to attach,
    /// the [chunking](DocumentImport::with_chunking_config) to index it with,
    /// and how long to [wait](DocumentImport::with_poll_options). The
    /// document is named after the file's ID, and is a copy: deleting the
    /// file afterwards leaves it in place (live 2026-11-05).
    ///
    /// The API answers with an operation, which this polls (`GET` on the
    /// operation, every 500 ms for up to 60 s by default) until it names the
    /// document, then reads the document back. Like an upload, the document
    /// may still be [`Pending`](DocumentState::Pending): see
    /// [`FileSearchDocuments::wait_until_active`].
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`], before any request, for a store
    /// name that is not `fileSearchStores/<id>` or a white-space chunking
    /// overlap not smaller than its chunk. Returns an API or network
    /// error if the import is refused (a file that does not exist, for
    /// instance), [`GenaiError::ProcessingFailed`] if the operation reports an
    /// error, [`GenaiError::PollTimeout`] if the wait times out, and
    /// [`GenaiError::MalformedResponse`] if the
    /// operation finishes without naming a document. If reading the document
    /// back fails, the error names the imported document.
    ///
    /// Do not retry an import that timed out or whose polling failed: the
    /// import goes on on the server, and its document already exists,
    /// pending, under the operation's ID, so a second import makes a second
    /// document (live 2026-11-05). The error names the operation and that
    /// document; wait for it with
    /// [`FileSearchDocuments::wait_until_active`] instead.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{CustomMetadata, DocumentImport, FileUpload};
    ///
    /// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
    /// let file = client.files().upload(FileUpload::from_path("faq.md")).await?;
    ///
    /// // The file as it is
    /// let faq = client.file_search_stores().import_file(store_name, &file).await?;
    ///
    /// // With custom metadata
    /// let tagged = client
    ///     .file_search_stores()
    ///     .import_file(
    ///         store_name,
    ///         DocumentImport::new(&file.name)
    ///             .add_custom_metadata(CustomMetadata::string("source", "faq")),
    ///     )
    ///     .await?;
    /// # let _ = (faq, tagged);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn import_file(
        self,
        store_name: &str,
        import: impl Into<DocumentImport>,
    ) -> Result<FileSearchDocument, GenaiError> {
        let import = import.into();
        crate::http::file_search_stores::import_file(&self.client.http, store_name, &import).await
    }

    /// The documents inside file search stores: list, get and delete them,
    /// and wait for one to be indexed.
    ///
    /// Like [`FileSearchStores`], the handle binds no store:
    /// [`list`](FileSearchDocuments::list) takes the store name, and the
    /// other methods take a document's full name, which already names its
    /// store (see [names](crate::file_search_stores#names)).
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
    /// let page = client
    ///     .file_search_stores()
    ///     .documents()
    ///     .list(store_name)
    ///     .send()
    ///     .await?;
    /// # let _ = page;
    /// # Ok(())
    /// # }
    /// ```
    pub fn documents(self) -> FileSearchDocuments<'a> {
        FileSearchDocuments {
            client: self.client,
        }
    }
}

/// The documents inside file search stores
/// (`/v1beta/fileSearchStores/{store}/documents`), from
/// [`FileSearchStores::documents`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle. See [names](crate::file_search_stores#names).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct FileSearchDocuments<'a> {
    client: &'a Client,
}

impl<'a> FileSearchDocuments<'a> {
    /// Lists the documents in a store, oldest first: configure the returned
    /// [`ListFileSearchDocuments`], then call
    /// [`send`](ListFileSearchDocuments::send) for one page, or
    /// [`pages`](ListFileSearchDocuments::pages) /
    /// [`items`](ListFileSearchDocuments::items) to stream them all.
    pub fn list(self, store_name: &str) -> ListFileSearchDocuments<'a> {
        ListFileSearchDocuments::new(self.client, store_name.to_owned())
    }

    /// Gets a document, for example to check its indexing
    /// [`state`](FileSearchDocument::state).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `fileSearchStores/<store>/documents/<id>` (see
    /// [names](crate::file_search_stores#names)), and an API or network
    /// error if the request fails or the document doesn't exist.
    pub async fn get(self, document_name: &str) -> Result<FileSearchDocument, GenaiError> {
        crate::http::file_search_stores::get_document(&self.client.http, document_name).await
    }

    /// Deletes a document that holds no chunks.
    ///
    /// Indexing splits a document into chunks, so the API rejects this for
    /// every successfully indexed document with a 400 ("Cannot delete
    /// non-empty Document"); [`force_delete`](Self::force_delete) deletes it
    /// with its chunks.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `fileSearchStores/<store>/documents/<id>`, and an API or network error
    /// if the request fails, the document doesn't exist, or it holds chunks.
    pub async fn delete(self, document_name: &str) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_document(&self.client.http, document_name, false)
            .await
    }

    /// Deletes a document and its chunks (sent as `force=true`), the delete
    /// an indexed document needs.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `fileSearchStores/<store>/documents/<id>`, and an API or network error
    /// if the request fails or the document doesn't exist.
    pub async fn force_delete(self, document_name: &str) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_document(&self.client.http, document_name, true)
            .await
    }

    /// Polls a document until it is indexed and queryable
    /// ([`Active`](DocumentState::Active)), then returns it.
    ///
    /// Uploading is not enough: file search silently returns no matches for a
    /// document still in [`Pending`](DocumentState::Pending), so anything
    /// that uploads and immediately queries needs this in between. Indexing
    /// is typically fast (observed ~1-2 s for a small text file), but it is
    /// not synchronous.
    ///
    /// By default the wait gives up after 60 s and polls every 500 ms;
    /// [`PollOptions`] changes either. The interval is worth raising when
    /// waiting on many documents at once, since the default issues a GET
    /// every half second per document. A state the crate doesn't know yet is
    /// logged at `warn` and polled through, per the Evergreen principle; the
    /// timeout is what bounds the wait.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::ProcessingFailed`] if the document reaches
    /// [`Failed`](DocumentState::Failed) and [`GenaiError::PollTimeout`] if
    /// the wait times out (neither is retryable),
    /// [`GenaiError::InvalidInput`] for a malformed name, or the error of a
    /// failed status request.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::PollOptions;
    /// use std::time::Duration;
    ///
    /// # async fn example(client: genai_rs::Client, document_name: &str) -> Result<(), genai_rs::GenaiError> {
    /// // Poll every 2 s for up to 5 minutes
    /// let poll = PollOptions::new()
    ///     .with_poll_interval(Duration::from_secs(2))
    ///     .with_timeout(Duration::from_secs(300));
    /// let document = client
    ///     .file_search_stores()
    ///     .documents()
    ///     .wait_until_active(document_name, poll)
    ///     .await?;
    /// println!("indexed: {:?}", document.state);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn wait_until_active(
        self,
        document_name: &str,
        poll: PollOptions,
    ) -> Result<FileSearchDocument, GenaiError> {
        let what = format!("document '{document_name}'");
        poll.wait(
            &what,
            (DOCUMENT_WAIT_TIMEOUT, DOCUMENT_POLL_INTERVAL),
            || self.get(document_name),
            |current| {
                match &current.state {
                    Some(DocumentState::Active) => return Ok(PollStep::Ready(current)),
                    Some(DocumentState::Failed) => {
                        // Terminal, and a 5xx `Api` would read as retryable.
                        return Err(GenaiError::ProcessingFailed(format!(
                            "Document '{document_name}' failed to index. This is \
                             terminal — re-uploading is the only recovery."
                        )));
                    }
                    Some(state) if state.is_unknown() => {
                        tracing::warn!(
                            "Document '{document_name}' is in unknown state {state}, continuing \
                             to poll. This may indicate API evolution - consider updating \
                             genai-rs."
                        );
                    }
                    _ => {}
                }
                Ok(PollStep::Pending(current.state.as_ref().map_or_else(
                    || "unknown".to_string(),
                    ToString::to_string,
                )))
            },
        )
        .await
    }
}

#[cfg(test)]
#[path = "file_search_stores_tests.rs"]
mod tests;
