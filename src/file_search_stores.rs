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
//! [`upload`](FileSearchStores::upload) to add a document. Its nested
//! [`documents`](FileSearchStores::documents) handle lists, gets and deletes
//! documents, and [waits](FileSearchDocuments::wait_until_active) for one to
//! be indexed.
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
//! `sizeBytes` is returned as a **JSON string**, not a number, and goes
//! through the crate's shared protobuf-JSON int64 helpers accordingly.
//!
//! # Example
//!
//! ```no_run
//! use genai_rs::{Client, CreateFileSearchStoreRequest, FileUpload, PollOptions};
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
//!     .upload(&store.name, FileUpload::from_path("handbook.pdf").with_display_name("handbook"))
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

use crate::client::Client;
use crate::errors::GenaiError;
use crate::files::{FileUpload, PollOptions};
use crate::paging;
use crate::serde_util::{ForFileSearchDocument, deserialize_string_i64, serialize_string_i64};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A file search store.
///
/// Created by [`FileSearchStores::create`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileSearchStore {
    /// Full resource name, e.g. `fileSearchStores/my-docs-4kws71n2ybpr`.
    ///
    /// This is the value to pass to
    /// [`Tool::FileSearch`](crate::Tool::FileSearch)'s `store_names`.
    #[serde(default)]
    pub name: String,

    /// Human-readable name supplied at creation.
    ///
    /// Note the API derives [`name`](Self::name) from this by stripping
    /// non-alphanumeric characters and appending a unique suffix, so the two
    /// are related but not interchangeable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_time: Option<DateTime<Utc>>,

    /// Last-update timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_time: Option<DateTime<Utc>>,

    /// Embedding model used to index documents in this store, e.g.
    /// `models/gemini-embedding-001`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,

    /// Fields the API returned that this struct does not model.
    ///
    /// Evergreen forward compatibility: unknown fields are preserved rather
    /// than dropped, so a deserialize-then-serialize round-trip is lossless.
    /// On serialize, a key present in both this map and a modeled field is
    /// emitted from the map.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing file search stores.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileSearchStoreListResponse {
    /// The stores on this page (wire: `fileSearchStores`). A null or
    /// malformed list degrades to empty; malformed elements drop
    /// individually.
    #[serde(
        default,
        rename = "fileSearchStores",
        deserialize_with = "crate::serde_util::deserialize_lenient_vec"
    )]
    pub stores: Vec<FileSearchStore>,

    /// Token for the next page, absent on the final page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(FileSearchStoreListResponse, stores: FileSearchStore);

/// A document inside a file search store.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileSearchDocument {
    /// Full resource name, e.g.
    /// `fileSearchStores/my-docs-4kws71n2ybpr/documents/handbook-25rp7vz1euwz`.
    #[serde(default)]
    pub name: String,

    /// Human-readable name supplied at upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Indexing state.
    ///
    /// A freshly uploaded document is [`DocumentState::Pending`] and becomes
    /// [`DocumentState::Active`] once indexed — typically within a second or
    /// two, but file search will not match against it until then. See
    /// [`FileSearchDocuments::wait_until_active`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<DocumentState>,

    /// MIME type detected at upload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Size in bytes.
    ///
    /// The API returns this as a JSON *string* (e.g. `"27"`, protobuf JSON
    /// convention); a plain number is accepted too, and it serializes back
    /// as a string so captured responses round-trip faithfully.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForFileSearchDocument>"
    )]
    pub size_bytes: Option<i64>,

    /// Creation timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_time: Option<DateTime<Utc>>,

    /// Last-update timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_time: Option<DateTime<Utc>>,

    /// Fields the API returned that this struct does not model.
    ///
    /// See [`FileSearchStore::extra`].
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing documents in a store.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct DocumentListResponse {
    /// The documents on this page. A null or malformed list degrades to
    /// empty; malformed elements drop individually.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::deserialize_lenient_vec"
    )]
    pub documents: Vec<FileSearchDocument>,

    /// Token for the next page, absent on the final page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(DocumentListResponse, documents: FileSearchDocument);

wire_enum! {
    /// Indexing state of a document in a file search store.
    ///
    /// Wire values are SCREAMING_CASE with a `STATE_` prefix (`STATE_PENDING`,
    /// `STATE_ACTIVE`), which differs from the Files API's [`FileState`] —
    /// verified live 2026-08-16.
    ///
    /// [`FileState`]: crate::FileState
    pub enum DocumentState {
        /// Uploaded but not yet indexed; file search will not match it yet.
        Pending = "STATE_PENDING",
        /// Indexed and queryable.
        Active = "STATE_ACTIVE",
        /// Indexing failed.
        Failed = "STATE_FAILED",
    }
    unknown(state_type, unknown_state_type)
}

impl DocumentState {
    /// Returns `true` when the document is indexed and queryable.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Request body for creating a file search store.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CreateFileSearchStoreRequest {
    /// Optional human-readable name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Additional fields to send that this struct does not model.
    ///
    /// Evergreen forward compatibility: lets callers reach new API fields
    /// without waiting for a crate release. A key present in both this map
    /// and a modeled field is emitted from the map.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CreateFileSearchStoreRequest {
    /// Creates an empty request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the human-readable display name.
    #[must_use]
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    /// Adds an unmodeled field, sent flat alongside the modeled ones.
    ///
    /// ```
    /// use genai_rs::CreateFileSearchStoreRequest;
    ///
    /// let request = CreateFileSearchStoreRequest::new()
    ///     .with_display_name("my-docs")
    ///     .with_extra("customChunkingConfig", serde_json::json!({"maxTokensPerChunk": 200}));
    ///
    /// assert_eq!(
    ///     serde_json::to_value(&request).unwrap(),
    ///     serde_json::json!({
    ///         "displayName": "my-docs",
    ///         "customChunkingConfig": {"maxTokensPerChunk": 200}
    ///     })
    /// );
    /// ```
    #[must_use]
    pub fn with_extra(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

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
        ListFileSearchStores {
            client: self.client,
            page_size: None,
            page_token: None,
        }
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
    /// The MIME type is the one set on the [`FileUpload`], or for a path, the
    /// one its extension maps to, as for [`Files::upload`](crate::Files::upload).
    /// The document's display name is the one set with
    /// [`FileUpload::with_display_name`]; without one, a path upload is named
    /// after its file and an in-memory upload is unnamed.
    ///
    /// The document is indexed asynchronously and starts in
    /// [`DocumentState::Pending`]; file search will not match it until it
    /// reaches [`Active`](DocumentState::Active). See
    /// [`FileSearchDocuments::wait_until_active`].
    ///
    /// The upload is a single request with the whole file as its body (the
    /// `raw` protocol, with no resumable path), so a path upload reads the
    /// file fully into memory first: a 1.5 GB file costs 1.5 GB of resident
    /// memory. The API answers with an operation naming the new document,
    /// which a second request reads back.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`], before any request, for a store
    /// name that is not `fileSearchStores/<id>`, a file that cannot be read,
    /// empty contents or contents over 2 GB, no MIME type (a path whose
    /// extension doesn't map to one, without [`FileUpload::with_mime_type`]),
    /// or a MIME type that cannot be sent as a header value. MIME *syntax* is
    /// not checked: `nonsense` or `text/` reach the API and come back as an
    /// API error. Returns an API or network error if the upload fails, and
    /// [`GenaiError::MalformedResponse`] if its answer names no document. If
    /// the upload succeeds but reading the document back fails, the error
    /// names the created document.
    ///
    /// The 2 GB ceiling is borrowed from the Files API and has not been
    /// verified for this resource, so treat it as a local guard rather than
    /// the API's limit.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{FileUpload, PollOptions};
    ///
    /// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
    /// // From disk, MIME type from the extension, named after the file
    /// let handbook = client
    ///     .file_search_stores()
    ///     .upload(store_name, FileUpload::from_path("handbook.pdf"))
    ///     .await?;
    ///
    /// // From memory
    /// let memo = client
    ///     .file_search_stores()
    ///     .upload(
    ///         store_name,
    ///         FileUpload::from_bytes(b"Q4 budget: 4.2M".to_vec(), "text/plain")
    ///             .with_display_name("budget-memo"),
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
        upload: FileUpload,
    ) -> Result<FileSearchDocument, GenaiError> {
        let (source, mime_type, display_name) = upload.into_parts()?;
        crate::http::file_search_stores::upload_to_file_search_store(
            &self.client.http,
            store_name,
            source,
            display_name.as_deref(),
            &mime_type,
        )
        .await
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
        ListFileSearchDocuments {
            client: self.client,
            store_name: store_name.to_owned(),
            page_size: None,
            page_token: None,
        }
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
    /// Returns [`GenaiError::Internal`] if the document reaches
    /// [`Failed`](DocumentState::Failed) or if the wait times out (neither is
    /// retryable, and the two are told apart by their message),
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
        let timeout = poll.timeout_or(DOCUMENT_WAIT_TIMEOUT);
        let poll_interval = poll.poll_interval_or(DOCUMENT_POLL_INTERVAL);
        let start = std::time::Instant::now();

        loop {
            let current = self.get(document_name).await?;

            match &current.state {
                Some(DocumentState::Active) => return Ok(current),
                Some(DocumentState::Failed) => {
                    // `Internal`: terminal, and a 5xx `Api` would read as
                    // retryable.
                    return Err(GenaiError::Internal(format!(
                        "Document '{document_name}' failed to index. This is \
                         terminal — re-uploading is the only recovery."
                    )));
                }
                Some(state) if state.is_unknown() => {
                    tracing::warn!(
                        "Document '{}' is in unknown state {}, continuing to poll. \
                         This may indicate API evolution - consider updating genai-rs.",
                        document_name,
                        state
                    );
                }
                _ => {}
            }

            if start.elapsed() > timeout {
                let state_info = current
                    .state
                    .as_ref()
                    .map_or_else(|| "unknown".to_string(), ToString::to_string);
                return Err(GenaiError::Internal(format!(
                    "Timeout waiting for document '{document_name}' to become active \
                     (waited {:?}, last state: {state_info}). It may still be indexing - \
                     try again with a longer timeout.",
                    start.elapsed()
                )));
            }

            tracing::debug!(
                "Document '{}' not active yet, waiting {:?}...",
                document_name,
                poll_interval
            );
            tokio::time::sleep(poll_interval).await;
        }
    }
}

/// A `GET /v1beta/fileSearchStores` request, from [`FileSearchStores::list`]:
/// your file search stores, oldest first.
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `nextPageToken` to the end of the list. The page size is sent with every
/// page.
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
/// use genai_rs::FileSearchStore;
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client.file_search_stores().list().with_page_size(10).send().await?;
/// for store in &page.stores {
///     println!("{}: {:?}", store.name, store.display_name);
/// }
///
/// // Every store, across pages
/// let all: Vec<FileSearchStore> = client.file_search_stores().list().items().try_collect().await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListFileSearchStores<'a> {
    client: &'a Client,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListFileSearchStores<'a> {
    /// Sets the maximum number of stores per page, from 1 to 20 (a larger
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
    /// With no stores, the page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn send(self) -> Result<FileSearchStoreListResponse, GenaiError> {
        crate::http::file_search_stores::list_file_search_stores(
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
    pub fn pages(self) -> BoxStream<'static, Result<FileSearchStoreListResponse, GenaiError>> {
        let Self {
            client,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("file search stores", page_token, move |token| {
            let client = client.clone();
            async move {
                crate::http::file_search_stores::list_file_search_stores(
                    &client.http,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every store across pages, oldest first. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<FileSearchStore, GenaiError>> {
        paging::items(self.pages())
    }
}

/// A `GET /v1beta/fileSearchStores/{store}/documents` request, from
/// [`FileSearchDocuments::list`]: the documents in one store, oldest first.
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `nextPageToken` to the end of the list. The page size is sent with every
/// page. A store name that is not `fileSearchStores/<id>` fails with
/// [`GenaiError::InvalidInput`] before any request (from `.send()`, or as
/// the streams' only item).
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
/// use genai_rs::FileSearchDocument;
///
/// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client
///     .file_search_stores()
///     .documents()
///     .list(store_name)
///     .with_page_size(10)
///     .send()
///     .await?;
/// for document in &page.documents {
///     println!("{}: {:?}", document.name, document.state);
/// }
///
/// // Every document, across pages
/// let all: Vec<FileSearchDocument> = client
///     .file_search_stores()
///     .documents()
///     .list(store_name)
///     .items()
///     .try_collect()
///     .await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListFileSearchDocuments<'a> {
    client: &'a Client,
    store_name: String,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListFileSearchDocuments<'a> {
    /// Sets the maximum number of documents per page, from 1 to 20 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token` for the same store (another store's token lists
    /// nothing, verified live 2026-09-27).
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Sends the request and returns one page.
    ///
    /// An empty store's page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, and
    /// an API or network error if the request fails or response parsing
    /// fails.
    pub async fn send(self) -> Result<DocumentListResponse, GenaiError> {
        crate::http::file_search_stores::list_documents(
            &self.client.http,
            &self.store_name,
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
    pub fn pages(self) -> BoxStream<'static, Result<DocumentListResponse, GenaiError>> {
        let Self {
            client,
            store_name,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("file search documents", page_token, move |token| {
            let (client, store_name) = (client.clone(), store_name.clone());
            async move {
                crate::http::file_search_stores::list_documents(
                    &client.http,
                    &store_name,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every document across pages, oldest first. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<FileSearchDocument, GenaiError>> {
        paging::items(self.pages())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact create/get payload observed live 2026-08-16.
    fn store_wire() -> serde_json::Value {
        serde_json::json!({
            "name": "fileSearchStores/genairsauditprobe-4kws71n2ybpr",
            "displayName": "genai-rs-audit-probe",
            "createTime": "2026-08-16T15:13:13.783782Z",
            "updateTime": "2026-08-16T15:13:13.783782Z",
            "embeddingModel": "models/gemini-embedding-001"
        })
    }

    #[test]
    fn store_deserializes_camel_case_wire() {
        let store: FileSearchStore = serde_json::from_value(store_wire()).unwrap();

        assert_eq!(
            store.name,
            "fileSearchStores/genairsauditprobe-4kws71n2ybpr"
        );
        assert_eq!(store.display_name.as_deref(), Some("genai-rs-audit-probe"));
        assert_eq!(
            store.embedding_model.as_deref(),
            Some("models/gemini-embedding-001")
        );
        assert!(store.create_time.is_some());
        assert!(store.extra.is_empty());
    }

    #[test]
    fn store_roundtrips_exactly() {
        let wire = store_wire();
        let store: FileSearchStore = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&store).unwrap(), wire);
    }

    #[test]
    fn store_preserves_unknown_fields() {
        let mut wire = store_wire();
        wire["futureField"] = serde_json::json!({"nested": true});

        let store: FileSearchStore = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            store.extra.get("futureField"),
            Some(&serde_json::json!({"nested": true}))
        );
        assert_eq!(serde_json::to_value(&store).unwrap(), wire);
    }

    #[test]
    fn list_response_uses_file_search_stores_envelope() {
        let wire = serde_json::json!({"fileSearchStores": [store_wire()]});
        let list: FileSearchStoreListResponse = serde_json::from_value(wire).unwrap();

        assert_eq!(list.stores.len(), 1);
        assert!(list.next_page_token.is_none());
    }

    #[test]
    fn empty_list_response_deserializes() {
        // The API returns a bare `{}` for an empty store list.
        let list: FileSearchStoreListResponse =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(list.stores.is_empty());
    }

    #[test]
    fn list_responses_drop_only_the_undeserializable_entry() {
        // `name` is required on both element types; a number in its place
        // costs that element, not the page.
        let stores: FileSearchStoreListResponse = serde_json::from_value(serde_json::json!({
            "fileSearchStores": [store_wire(), {"name": 7}],
            "nextPageToken": "p2"
        }))
        .unwrap();
        assert_eq!(stores.stores.len(), 1);
        assert_eq!(stores.next_page_token.as_deref(), Some("p2"));

        let docs: DocumentListResponse = serde_json::from_value(serde_json::json!({
            "documents": [{"name": 7}, document_wire()]
        }))
        .unwrap();
        assert_eq!(docs.documents.len(), 1);
    }

    #[test]
    fn list_responses_treat_an_explicit_null_list_as_empty() {
        let stores: FileSearchStoreListResponse =
            serde_json::from_value(serde_json::json!({"fileSearchStores": null})).unwrap();
        assert!(stores.stores.is_empty());
        let docs: DocumentListResponse =
            serde_json::from_value(serde_json::json!({"documents": null})).unwrap();
        assert!(docs.documents.is_empty());
    }

    /// Exact document payload observed live 2026-08-16.
    fn document_wire() -> serde_json::Value {
        serde_json::json!({
            "name": "fileSearchStores/probe-lhba715kr8z5/documents/doc1-25rp7vz1euwz",
            "displayName": "doc1",
            "updateTime": "2026-08-16T15:53:10.494695Z",
            "createTime": "2026-08-16T15:53:10.494695Z",
            "state": "STATE_PENDING",
            "sizeBytes": "27",
            "mimeType": "text/plain"
        })
    }

    #[test]
    fn document_deserializes_with_string_size_bytes() {
        let doc: FileSearchDocument = serde_json::from_value(document_wire()).unwrap();

        // sizeBytes arrives as a JSON string, not a number.
        assert_eq!(doc.size_bytes, Some(27));
        assert_eq!(doc.state, Some(DocumentState::Pending));
        assert_eq!(doc.mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn document_state_wire_values_use_state_prefix() {
        assert_eq!(
            serde_json::to_value(DocumentState::Pending).unwrap(),
            "STATE_PENDING"
        );
        assert_eq!(
            serde_json::to_value(DocumentState::Active).unwrap(),
            "STATE_ACTIVE"
        );
        assert_eq!(
            serde_json::to_value(DocumentState::Failed).unwrap(),
            "STATE_FAILED"
        );
    }

    #[cfg(not(feature = "strict-unknown"))]
    #[test]
    fn document_state_unknown_is_preserved() {
        let state: DocumentState = serde_json::from_value(serde_json::json!("STATE_QUARANTINED"))
            .expect("unknown states must not fail deserialization");

        assert!(state.is_unknown());
        assert_eq!(state.unknown_state_type(), Some("STATE_QUARANTINED"));
        assert!(!state.is_active());
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            "STATE_QUARANTINED",
            "unknown states must round-trip to their original wire value"
        );
    }

    #[test]
    fn document_state_is_active_only_for_active() {
        assert!(DocumentState::Active.is_active());
        assert!(!DocumentState::Pending.is_active());
        assert!(!DocumentState::Failed.is_active());
    }

    #[test]
    fn document_preserves_unknown_fields() {
        let mut wire = document_wire();
        wire["chunkCount"] = serde_json::json!(3);

        let doc: FileSearchDocument = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(doc.extra.get("chunkCount"), Some(&serde_json::json!(3)));
        assert_eq!(serde_json::to_value(&doc).unwrap(), wire);
    }

    #[test]
    fn create_request_omits_display_name_when_unset() {
        let request = CreateFileSearchStoreRequest::default();
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn create_request_serializes_camel_case() {
        let request = CreateFileSearchStoreRequest {
            display_name: Some("my-docs".to_string()),
            extra: serde_json::Map::new(),
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({"displayName": "my-docs"})
        );
    }

    /// `extra` rides `#[serde(flatten)]`, so its keys must land beside
    /// `displayName` rather than nested under an `extra` object.
    #[test]
    fn create_request_flattens_extra_beside_modeled_fields() {
        let mut extra = serde_json::Map::new();
        extra.insert(
            "customChunkingConfig".to_string(),
            serde_json::json!({"maxTokensPerChunk": 200}),
        );
        let request = CreateFileSearchStoreRequest {
            display_name: Some("my-docs".to_string()),
            extra,
        };

        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "displayName": "my-docs",
                "customChunkingConfig": {"maxTokensPerChunk": 200}
            })
        );
    }

    /// The builders have to agree with the struct-literal form the tests
    /// around them use. They exist for downstream crates, which cannot use
    /// that form at all on a `#[non_exhaustive]` type, so nothing outside
    /// the crate would notice them drifting.
    ///
    /// Doctests cover `with_extra`'s serialization, but doctests run only in
    /// CI — not under `make test` / `make test-all` — so this is what the
    /// repo's own default test commands exercise.
    #[test]
    fn builders_match_the_struct_literal_form() {
        let mut extra = serde_json::Map::new();
        extra.insert("someNewField".to_string(), serde_json::json!(7));
        let literal = CreateFileSearchStoreRequest {
            display_name: Some("my-docs".to_string()),
            extra,
        };

        let built = CreateFileSearchStoreRequest::new()
            .with_display_name("my-docs")
            .with_extra("someNewField", 7);

        assert_eq!(built, literal);
        assert_eq!(
            serde_json::to_value(&built).unwrap(),
            serde_json::to_value(&literal).unwrap()
        );
    }

    #[test]
    fn new_matches_default() {
        assert_eq!(
            CreateFileSearchStoreRequest::new(),
            CreateFileSearchStoreRequest::default()
        );
    }

    /// The other half of the flatten contract: an unmodeled key on the way
    /// in has to survive into `extra` rather than being dropped, or a
    /// round-trip through this type would silently discard it.
    #[test]
    fn create_request_captures_unknown_fields_into_extra() {
        let wire = serde_json::json!({
            "displayName": "my-docs",
            "somethingNewUpstream": "value"
        });
        let request: CreateFileSearchStoreRequest = serde_json::from_value(wire.clone()).unwrap();

        assert_eq!(request.display_name.as_deref(), Some("my-docs"));
        assert_eq!(
            request.extra.get("somethingNewUpstream"),
            Some(&serde_json::json!("value"))
        );
        assert_eq!(serde_json::to_value(&request).unwrap(), wire);
    }

    #[test]
    fn document_wait_defaults_are_60_s_and_500_ms() {
        assert_eq!(DOCUMENT_WAIT_TIMEOUT, Duration::from_secs(60));
        assert_eq!(DOCUMENT_POLL_INTERVAL, Duration::from_millis(500));
    }

    #[test]
    fn store_list_builder_setters_replace() {
        let client = Client::new("k".to_string());
        let list = client.file_search_stores().list();
        assert_eq!(list.page_size, None);
        assert_eq!(list.page_token, None);

        let list = list
            .with_page_size(20)
            .with_page_token("t1")
            .with_page_size(1)
            .with_page_token(String::from("t2"));
        assert_eq!(list.page_size, Some(1));
        assert_eq!(list.page_token.as_deref(), Some("t2"));
    }

    #[test]
    fn document_list_builder_owns_its_store_name_and_query() {
        let client = Client::new("k".to_string());
        let store_name = String::from("fileSearchStores/abc");
        let list = client.file_search_stores().documents().list(&store_name);
        // The builder owns the store name, so the caller's string can go.
        drop(store_name);
        assert_eq!(list.store_name, "fileSearchStores/abc");
        assert_eq!(list.page_size, None);
        assert_eq!(list.page_token, None);

        let list = list
            .with_page_size(20)
            .with_page_token("t1")
            // `with_*` replaces.
            .with_page_size(2)
            .with_page_token("t2");
        assert_eq!(list.page_size, Some(2));
        assert_eq!(list.page_token.as_deref(), Some("t2"));
    }

    #[test]
    fn file_search_handles_and_lists_debug_redact_the_api_key() {
        let client = Client::new("secret-api-key".to_string());
        let stores = client.file_search_stores();
        for debug in [
            format!("{stores:?}"),
            format!("{:?}", stores.documents()),
            format!("{:?}", stores.list().with_page_size(5)),
            format!("{:?}", stores.documents().list("fileSearchStores/abc")),
        ] {
            assert!(!debug.contains("secret-api-key"), "{debug}");
            assert!(debug.contains("[REDACTED]"), "{debug}");
        }
    }
}
