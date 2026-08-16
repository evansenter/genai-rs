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
//! use genai_rs::Client;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new("api-key".to_string());
//!
//! // Provision a store and add a document.
//! let store = client.create_file_search_store(Some("my-docs")).await?;
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

use crate::serde_util::{ForFileSearchDocument, deserialize_string_i64, serialize_string_i64};
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
    /// The stores on this page (wire: `fileSearchStores`).
    #[serde(default, rename = "fileSearchStores")]
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
    /// The documents on this page.
    #[serde(default)]
    pub documents: Vec<FileSearchDocument>,

    /// Token for the next page, absent on the final page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Indexing state of a document in a file search store.
///
/// Wire values are SCREAMING_CASE with a `STATE_` prefix (`STATE_PENDING`,
/// `STATE_ACTIVE`), which differs from the Files API's [`FileState`] —
/// verified live 2026-08-16.
///
/// [`FileState`]: crate::FileState
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentState {
    /// Uploaded but not yet indexed; file search will not match it yet.
    Pending,
    /// Indexed and queryable.
    Active,
    /// Indexing failed.
    Failed,
    /// Unknown state (for forward compatibility).
    ///
    /// The `state_type` field contains the unrecognized state string, and
    /// `data` contains the JSON value preserved for round-trip.
    Unknown {
        /// The unrecognized state string from the API.
        state_type: String,
        /// The raw JSON value, preserved for debugging and round-trip.
        data: serde_json::Value,
    },
}

impl DocumentState {
    /// Returns `true` when the document is indexed and queryable.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Check if this is an unknown state.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the state type name if this is an unknown state.
    ///
    /// Returns `None` for known states.
    #[must_use]
    pub fn unknown_state_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { state_type, .. } => Some(state_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown state.
    ///
    /// Returns `None` for known states.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for DocumentState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Pending => serializer.serialize_str("STATE_PENDING"),
            Self::Active => serializer.serialize_str("STATE_ACTIVE"),
            Self::Failed => serializer.serialize_str("STATE_FAILED"),
            Self::Unknown { state_type, .. } => serializer.serialize_str(state_type),
        }
    }
}

impl<'de> Deserialize<'de> for DocumentState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.as_str() {
            Some("STATE_PENDING") => Ok(Self::Pending),
            Some("STATE_ACTIVE") => Ok(Self::Active),
            Some("STATE_FAILED") => Ok(Self::Failed),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown DocumentState '{}'. \
                     This may indicate a new API feature. \
                     The state will be preserved in the Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    state_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let state_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "DocumentState received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    state_type,
                    data: value,
                })
            }
        }
    }
}

impl std::fmt::Display for DocumentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "STATE_PENDING"),
            Self::Active => write!(f, "STATE_ACTIVE"),
            Self::Failed => write!(f, "STATE_FAILED"),
            Self::Unknown { state_type, .. } => write!(f, "{}", state_type),
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

    /// Additional fields to send that this struct does not model.
    ///
    /// Evergreen forward compatibility: lets callers reach new API fields
    /// without waiting for a crate release. A key present in both this map
    /// and a modeled field is emitted from the map.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// `extra` is the entire reason `create_file_search_store_with_request`
    /// exists, and it rides `#[serde(flatten)]` — so what needs pinning is
    /// that its keys land beside `displayName` rather than nested under an
    /// `extra` object.
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
