//! File Search Store resource (`/v1beta/fileSearchStores`).
//!
//! A file search store holds documents that [`Tool::FileSearch`](crate::Tool::FileSearch)
//! retrieves over. The tool takes store names, so without these endpoints a
//! caller has to provision stores outside the crate before file search is
//! usable at all.
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
//! use genai_rs::{Client, CreateFileSearchStoreRequest};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new("api-key".to_string());
//!
//! // Provision a store and add a document.
//! let store = client
//!     .create_file_search_store(&CreateFileSearchStoreRequest::new().with_display_name("my-docs"))
//!     .await?;
//! let document = client
//!     .upload_to_file_search_store(&store.name, "handbook.pdf", Some("handbook"))
//!     .await?;
//!
//! // Documents are indexed asynchronously; wait before querying.
//! client.wait_for_document_active(&document.name, None, None).await?;
//!
//! // ... query it via Tool::FileSearch with store.name ...
//!
//! client.delete_file_search_store(&store.name, true).await?;
//! # Ok(())
//! # }
//! ```

use crate::client::Client;
use crate::errors::GenaiError;
use crate::serde_util::{ForFileSearchDocument, deserialize_string_i64, serialize_string_i64};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A file search store.
///
/// Created by [`Client::create_file_search_store`](crate::Client::create_file_search_store).
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
}

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
    /// [`Client::wait_for_document_active`](crate::Client::wait_for_document_active).
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
}

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

/// File search store and document methods.
impl Client {
    /// Creates a file search store.
    ///
    /// The returned [`FileSearchStore::name`](crate::FileSearchStore::name) is
    /// what [`Tool::FileSearch`](crate::Tool::FileSearch) takes in
    /// `store_names`.
    ///
    /// The request's display name is a human-readable label; the API derives
    /// the resource name from it by stripping non-alphanumeric characters and
    /// appending a unique suffix, so the two are related but not
    /// interchangeable. Fields the crate does not model yet go in
    /// [`CreateFileSearchStoreRequest::extra`](crate::CreateFileSearchStoreRequest::extra).
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
    ///     .create_file_search_store(&CreateFileSearchStoreRequest::new().with_display_name("my-docs"))
    ///     .await?;
    /// println!("created {}", store.name);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_file_search_store(
        &self,
        request: &crate::CreateFileSearchStoreRequest,
    ) -> Result<crate::FileSearchStore, GenaiError> {
        crate::http::file_search_stores::create_file_search_store(&self.http, request).await
    }

    /// Retrieves a file search store by resource name.
    ///
    /// # Arguments
    ///
    /// * `store_name` - Full resource name (e.g. `fileSearchStores/abc123`).
    ///   A bare ID is rejected locally as [`GenaiError::InvalidInput`].
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn get_file_search_store(
        &self,
        store_name: &str,
    ) -> Result<crate::FileSearchStore, GenaiError> {
        crate::http::file_search_stores::get_file_search_store(&self.http, store_name).await
    }

    /// Lists file search stores.
    ///
    /// # Errors
    ///
    /// Returns an API or network error if the request fails.
    pub async fn list_file_search_stores(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::FileSearchStoreListResponse, GenaiError> {
        crate::http::file_search_stores::list_file_search_stores(&self.http, page_size, page_token)
            .await
    }

    /// Deletes a file search store.
    ///
    /// # Arguments
    ///
    /// * `store_name` - Full resource name (e.g. `fileSearchStores/abc123`).
    /// * `force` - Delete even when the store still holds documents. Without
    ///   it, a non-empty store is rejected with `FAILED_PRECONDITION`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn delete_file_search_store(
        &self,
        store_name: &str,
        force: bool,
    ) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_file_search_store(&self.http, store_name, force)
            .await
    }

    /// Uploads a local file into a file search store.
    ///
    /// MIME type is inferred from the file extension, matching
    /// [`upload_file`](Self::upload_file). Use
    /// [`upload_to_file_search_store_with_mime`](Self::upload_to_file_search_store_with_mime)
    /// to set it explicitly.
    ///
    /// The document is indexed asynchronously and starts in
    /// [`DocumentState::Pending`]; file search
    /// will not match it until it reaches
    /// [`Active`](crate::DocumentState::Active). See
    /// [`wait_for_document_active`](Self::wait_for_document_active).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, a file
    /// that is unreadable, empty, or over the local 2 GB ceiling, or a file
    /// whose MIME type cannot be inferred from its extension — use
    /// [`upload_to_file_search_store_with_mime`](Self::upload_to_file_search_store_with_mime)
    /// in that case. Returns an API or network error if the request fails.
    ///
    /// Note that "unreadable" and "empty" are separate cases: a zero-byte file
    /// reads fine and is rejected on its own guard.
    ///
    /// The 2 GB ceiling is borrowed from the Files API and has not been
    /// verified for this resource, so treat it as a local guard rather than
    /// the API's limit — the API may reject a smaller file with its own.
    /// Note also that the raw upload protocol sends the file as a single
    /// body with no resumable path, so it is read fully into memory before
    /// the request is issued: a 1.5 GB upload costs 1.5 GB of resident
    /// memory.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, CreateFileSearchStoreRequest};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    /// let store = client
    ///     .create_file_search_store(&CreateFileSearchStoreRequest::new().with_display_name("my-docs"))
    ///     .await?;
    ///
    /// let doc = client
    ///     .upload_to_file_search_store(&store.name, "handbook.pdf", Some("handbook"))
    ///     .await?;
    /// client.wait_for_document_active(&doc.name, None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_to_file_search_store(
        &self,
        store_name: &str,
        file_path: impl AsRef<std::path::Path>,
        display_name: Option<&str>,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        let path = file_path.as_ref();
        let mime_type =
            crate::files::mime_type_for_upload(path, "upload_to_file_search_store_with_mime()")?;
        crate::http::file_search_stores::upload_to_file_search_store(
            &self.http,
            store_name,
            path,
            display_name,
            mime_type,
        )
        .await
    }

    /// Uploads a local file into a store with an explicit MIME type.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, a file
    /// that is unreadable, empty, or over the local 2 GB ceiling, or a MIME
    /// type that cannot be sent as a header value (one containing control
    /// characters). Returns an API or network error if the request fails.
    ///
    /// As with
    /// [`upload_to_file_search_store`](Self::upload_to_file_search_store), the
    /// 2 GB ceiling is borrowed from the Files API and unverified for this
    /// resource, and the file is read fully into memory before the request is
    /// issued.
    ///
    /// MIME *syntax* is not checked here: `nonsense` or `text/` are valid
    /// header values, so they reach the API and come back as an API error
    /// rather than `InvalidInput`.
    pub async fn upload_to_file_search_store_with_mime(
        &self,
        store_name: &str,
        file_path: impl AsRef<std::path::Path>,
        display_name: Option<&str>,
        mime_type: &str,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        crate::http::file_search_stores::upload_to_file_search_store(
            &self.http,
            store_name,
            file_path.as_ref(),
            display_name,
            mime_type,
        )
        .await
    }

    /// Lists documents in a file search store.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, and
    /// an API or network error if the request fails.
    pub async fn list_file_search_documents(
        &self,
        store_name: &str,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::DocumentListResponse, GenaiError> {
        crate::http::file_search_stores::list_documents(
            &self.http, store_name, page_size, page_token,
        )
        .await
    }

    /// Retrieves a document from a file search store.
    ///
    /// # Arguments
    ///
    /// * `document_name` - Full resource name (e.g.
    ///   `fileSearchStores/abc/documents/doc1`).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn get_file_search_document(
        &self,
        document_name: &str,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        crate::http::file_search_stores::get_document(&self.http, document_name).await
    }

    /// Deletes a document from a file search store.
    ///
    /// # Arguments
    ///
    /// * `document_name` - Full resource name.
    /// * `force` - Required for a document that has been chunked, which is
    ///   every successfully indexed one. Without it the API responds
    ///   `400 Cannot delete non-empty Document`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn delete_file_search_document(
        &self,
        document_name: &str,
        force: bool,
    ) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_document(&self.http, document_name, force).await
    }

    /// Polls a document until it is indexed and queryable.
    ///
    /// Uploading is not enough: file search silently returns no matches for a
    /// document still in [`Pending`](crate::DocumentState::Pending), so
    /// anything that uploads and immediately queries needs this in between.
    /// Indexing is typically fast (observed ~1-2s for a small text file), but
    /// it is not synchronous.
    ///
    /// Unknown states are polled through rather than treated as terminal, per
    /// the Evergreen principle — the `timeout` is what bounds the wait.
    ///
    /// # Arguments
    ///
    /// * `document_name` - Full resource name.
    /// * `timeout` - Maximum time to wait; defaults to 60s when `None`.
    /// * `poll_interval` - Delay between polls; defaults to 500ms when `None`.
    ///   Worth raising when uploading many documents at once, since the
    ///   default issues a GET every half second per document.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::Internal`] if the document reaches
    /// [`Failed`](crate::DocumentState::Failed) or if the wait times out —
    /// neither is retryable, and the two are distinguished by their message —
    /// and [`GenaiError::InvalidInput`] for a malformed name. Errors from the
    /// underlying polling GET propagate as they are.
    pub async fn wait_for_document_active(
        &self,
        document_name: &str,
        timeout: Option<std::time::Duration>,
        poll_interval: Option<std::time::Duration>,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        use std::time::{Duration, Instant};

        let timeout = timeout.unwrap_or(Duration::from_secs(60));
        let poll_interval = poll_interval.unwrap_or(Duration::from_millis(500));
        let start = Instant::now();

        loop {
            let current = self.get_file_search_document(document_name).await?;

            match &current.state {
                Some(crate::DocumentState::Active) => return Ok(current),
                Some(crate::DocumentState::Failed) => {
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

            tokio::time::sleep(poll_interval).await;
        }
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
}
