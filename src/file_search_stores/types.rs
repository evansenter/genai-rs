//! The file search store resource types: stores, documents, their list
//! pages, and the create, custom metadata and chunking types.

use crate::paging;
use crate::serde_util::{
    ForFileSearchDocument, ResourceName, deserialize_string_i64, serialize_string_i64,
};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

struct ForFileSearchStore;
impl ResourceName for ForFileSearchStore {
    const NAME: &'static str = "FileSearchStore";
}

/// A file search store.
///
/// Created by [`FileSearchStores::create`](super::FileSearchStores::create).
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
    /// `models/gemini-embedding-001` (the default, live 2026-11-05); set at
    /// creation with
    /// [`CreateFileSearchStoreRequest::with_embedding_model`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,

    /// How many documents are indexed and queryable. Absent on an empty
    /// store.
    ///
    /// The four counts here are protobuf int64s, sent as JSON strings
    /// (`"1"`, live 2026-11-05); a plain number is accepted too, and each
    /// serializes back as a string.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForFileSearchStore>"
    )]
    pub active_documents_count: Option<i64>,

    /// How many documents are still being indexed. Served only while one is
    /// (live 2026-11-05).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForFileSearchStore>"
    )]
    pub pending_documents_count: Option<i64>,

    /// How many documents failed to index. Not seen live: a document that
    /// cannot be indexed failed its upload instead (2026-11-05).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForFileSearchStore>"
    )]
    pub failed_documents_count: Option<i64>,

    /// The total size of the store's documents, in bytes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForFileSearchStore>"
    )]
    pub size_bytes: Option<i64>,

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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// [`FileSearchDocuments::wait_until_active`](super::FileSearchDocuments::wait_until_active).
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

    /// The key-value pairs attached when the document was added
    /// ([`DocumentUpload::add_custom_metadata`](super::DocumentUpload::add_custom_metadata),
    /// [`DocumentImport::add_custom_metadata`](super::DocumentImport::add_custom_metadata)),
    /// served back in the order sent (live 2026-11-05). Empty when none were.
    /// A malformed entry is dropped on its own.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::serde_util::deserialize_lenient_vec"
    )]
    pub custom_metadata: Vec<CustomMetadata>,

    /// Fields the API returned that this struct does not model.
    ///
    /// See [`FileSearchStore::extra`].
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing documents in a store.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct FileSearchDocumentListResponse {
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

paging::impl_list_page!(FileSearchDocumentListResponse, documents: FileSearchDocument);

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

/// A key-value pair on a document in a file search store: one of a string,
/// a number or a list of strings under a key.
///
/// Attach it when adding the document
/// ([`DocumentUpload::add_custom_metadata`](super::DocumentUpload::add_custom_metadata),
/// [`DocumentImport::add_custom_metadata`](super::DocumentImport::add_custom_metadata));
/// it is served back on [`FileSearchDocument::custom_metadata`] (all three
/// kinds verified live 2026-11-05).
///
/// ```
/// use genai_rs::CustomMetadata;
///
/// let team = CustomMetadata::string("team", "search");
/// let year = CustomMetadata::numeric("year", 2026.0);
/// let tags = CustomMetadata::string_list("tags", ["rust", "docs"]);
/// assert_eq!(
///     serde_json::to_value(&tags).unwrap(),
///     serde_json::json!({"key": "tags", "stringListValue": {"values": ["rust", "docs"]}})
/// );
/// # let _ = (team, year);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct CustomMetadata {
    /// The key.
    #[serde(default)]
    pub key: String,
    /// A string value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,
    /// A numeric value. The API serves a whole number without a fraction
    /// (`2026`); it serializes back as `2026.0`, the same JSON number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
    /// A list of strings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_list_value: Option<StringList>,
    /// Fields the API returned that this struct does not model (a value
    /// kind added later), preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CustomMetadata {
    /// A string value under `key`.
    #[must_use]
    pub fn string(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            string_value: Some(value.into()),
            ..Default::default()
        }
    }

    /// A numeric value under `key`.
    #[must_use]
    pub fn numeric(key: impl Into<String>, value: f64) -> Self {
        Self {
            key: key.into(),
            numeric_value: Some(value),
            ..Default::default()
        }
    }

    /// A list of strings under `key`.
    #[must_use]
    pub fn string_list<S: Into<String>>(
        key: impl Into<String>,
        values: impl IntoIterator<Item = S>,
    ) -> Self {
        Self {
            key: key.into(),
            string_list_value: Some(StringList {
                values: values.into_iter().map(Into::into).collect(),
                extra: serde_json::Map::new(),
            }),
            ..Default::default()
        }
    }
}

/// The value of a list-of-strings [`CustomMetadata`] (wire:
/// `{"values": [...]}`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StringList {
    /// The strings.
    #[serde(default)]
    pub values: Vec<String>,
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// How a store splits a document into chunks when it indexes it.
///
/// The only strategy is white space ([`ChunkingConfig::white_space`]). The
/// API reads it: an unknown key is a `400 Invalid JSON payload`, and an
/// overlap as large as the chunk or larger fails the upload or import with
/// a 500 (live 2026-11-05), so
/// [`FileSearchStores::upload`](crate::FileSearchStores::upload) and
/// [`import_file`](crate::FileSearchStores::import_file) reject one with
/// [`GenaiError::InvalidInput`](crate::GenaiError::InvalidInput) before any
/// request. A chunking strategy the crate does not model yet can be sent
/// through [`extra`](Self::extra).
///
/// ```
/// use genai_rs::ChunkingConfig;
///
/// let chunking = ChunkingConfig::white_space(200, 20);
/// assert_eq!(
///     serde_json::to_value(&chunking).unwrap(),
///     serde_json::json!({"whiteSpaceConfig": {"maxTokensPerChunk": 200, "maxOverlapTokens": 20}})
/// );
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ChunkingConfig {
    /// Chunking on white space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_space_config: Option<WhiteSpaceConfig>,
    /// Additional fields to send that this struct does not model, sent flat
    /// beside the modeled ones.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ChunkingConfig {
    /// Chunks on white space: at most `max_tokens_per_chunk` tokens per
    /// chunk, with up to `max_overlap_tokens` shared by neighboring chunks.
    /// The overlap must be smaller than the chunk: the upload or import
    /// checks it before any request.
    #[must_use]
    pub fn white_space(max_tokens_per_chunk: u32, max_overlap_tokens: u32) -> Self {
        Self {
            white_space_config: Some(WhiteSpaceConfig::new(
                max_tokens_per_chunk,
                max_overlap_tokens,
            )),
            extra: serde_json::Map::new(),
        }
    }
}

/// White-space chunking limits, in tokens ([`ChunkingConfig::white_space`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct WhiteSpaceConfig {
    /// The most tokens in one chunk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_chunk: Option<u32>,
    /// The most tokens two neighboring chunks share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_overlap_tokens: Option<u32>,
}

impl WhiteSpaceConfig {
    /// Both limits.
    #[must_use]
    pub const fn new(max_tokens_per_chunk: u32, max_overlap_tokens: u32) -> Self {
        Self {
            max_tokens_per_chunk: Some(max_tokens_per_chunk),
            max_overlap_tokens: Some(max_overlap_tokens),
        }
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

    /// The embedding model that indexes the store's documents, e.g.
    /// `models/gemini-embedding-2`. Unset, the API uses
    /// `models/gemini-embedding-001`; a model that does not exist is a
    /// `400 INVALID_ARGUMENT` (live 2026-11-05).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model: Option<String>,

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

    /// Sets the embedding model, as a `models/<id>` name.
    #[must_use]
    pub fn with_embedding_model(mut self, embedding_model: impl Into<String>) -> Self {
        self.embedding_model = Some(embedding_model.into());
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

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
