//! Environments: the sandbox an agent runs in, the `/v1beta/environments`
//! resource that stores one, and the files inside it.
//!
//! # Describing an environment
//!
//! An environment describes which sources are mounted (GCS buckets, inline
//! files, repositories, skill registries) and what outbound network access
//! is allowed. The `environment` request field and the Agents resource's
//! `base_environment` accept either a string environment ID — one created
//! explicitly via [`Client::create_environment()`](crate::Client::create_environment)
//! or by a previous interaction, echoed as
//! [`InteractionResponse::environment_id`](crate::InteractionResponse) — or
//! a typed remote environment object. [`EnvironmentSpec`] models that union.
//!
//! # The Environments resource
//!
//! An [`Environment`] is a server-side container of files (and network
//! configuration) that agent interactions can execute against. Requests
//! reference one via
//! [`InteractionRequest::environment`](crate::request::InteractionRequest::environment)
//! — either inline (the API creates one implicitly) or by ID. The CRUD
//! surface creates an environment once, references it from many
//! interactions, lists what exists, and deletes what's stale.
//!
//! Wire format verified live 2026-08-08: the resource uses `created` /
//! `updated` / `last_accessed` ISO-8601 timestamps, and `file_count` /
//! `size_bytes` are int64s serialized as JSON *strings* (protobuf JSON
//! convention); both are accepted here as numbers too.
//!
//! # Files inside an environment
//!
//! [`Client::list_environment_files()`](crate::Client::list_environment_files)
//! lists what an agent left in an environment and
//! [`Client::upload_environment_file()`](crate::Client::upload_environment_file)
//! uploads a file into one before an interaction runs. Paths are relative to
//! the environment root; `""` lists the root.
//!
//! Verified live 2026-09-24: listing (root, a directory, one file,
//! recursive) and a resumable single-shot upload. The API reports entry
//! types in uppercase (`FILE`, `DIRECTORY`) although the bindings spell them
//! lowercase; both are accepted. Environments are forked with
//! [`CreateEnvironmentRequest::from_environment`].
//!
//! # IDs
//!
//! Methods take the bare ID (the form [`Environment::id`] returns), not an
//! `environments/...` resource name: the ID is percent-encoded into a single
//! path segment, so a resource name addresses nothing and 404s (verified
//! live). The one unobserved source is
//! [`InteractionResponse::environment_id`](crate::InteractionResponse::environment_id):
//! strip a leading `environments/` prefix before passing it. An empty or
//! dot-segment ID fails locally with
//! [`GenaiError::InvalidInput`] before any
//! request.

mod files;
mod spec;

pub use files::{EnvironmentFile, EnvironmentFileList, EnvironmentFileType, EnvironmentFileUpload};
pub use spec::{
    AllowlistEntry, EnvVar, EnvironmentSource, EnvironmentSpec, NetworkConfig, RemoteEnvironment,
    SourceType,
};

use crate::client::Client;
use crate::errors::GenaiError;
use crate::serde_util::{
    ForEnvironment, deserialize_lenient_timestamp, deserialize_string_i64, serialize_string_i64,
};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

wire_enum! {
    /// Status of an environment container.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"active"`, `"expired"`.
    pub enum EnvironmentStatus {
        /// The environment is available for use.
        Active = "active",
        /// The environment has expired and can no longer be used.
        Expired = "expired",
    }
    unknown(status_type, unknown_status_type)
}

/// An execution environment for an agent, as returned by the
/// `/v1beta/environments` resource.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
#[non_exhaustive]
pub struct Environment {
    /// Output only. The ID of the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The file sources materialized into the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<EnvironmentSource>>,
    /// Network configuration for the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkConfig>,
    /// Output only. The status of the environment container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<EnvironmentStatus>,
    /// Output only. When the environment was created.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForEnvironment>"
    )]
    pub created: Option<DateTime<Utc>>,
    /// Output only. When the environment was last updated.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForEnvironment>"
    )]
    pub updated: Option<DateTime<Utc>>,
    /// Output only. When the environment was last accessed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForEnvironment>"
    )]
    pub last_accessed: Option<DateTime<Utc>>,
    /// Output only. The number of files in the environment.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForEnvironment>"
    )]
    pub file_count: Option<i64>,
    /// Output only. The total size of the environment's files in bytes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_string_i64",
        deserialize_with = "deserialize_string_i64::<_, ForEnvironment>"
    )]
    pub size_bytes: Option<i64>,
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen).
    ///
    /// Without this, a deserialize-then-serialize cycle silently drops any
    /// field the crate has not modeled yet — invisible to the caller, and
    /// unrecoverable. The resource was live-verified 2026-08-08, but verification is a
    /// point-in-time snapshot, not a guarantee the shape stays fixed.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value`, matching the request-side escape hatches.
    /// (`to_string` on a flattened struct emits both keys rather than
    /// deduplicating; don't hand-serialize colliding keys.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Request body for creating an environment explicitly.
///
/// For the *inline* per-request form (the `environment` field on an
/// interaction), use [`RemoteEnvironment`] / [`EnvironmentSpec`] instead —
/// same fields, but carrying the `remote` type discriminator the inline union
/// requires.
///
/// # Example
///
/// ```
/// use genai_rs::{CreateEnvironmentRequest, EnvironmentSource};
///
/// let request = CreateEnvironmentRequest::new()
///     .add_source(EnvironmentSource::inline("/etc/motd", "hello"));
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CreateEnvironmentRequest {
    /// An environment to copy. Must be a bare ID: the documented
    /// `environments/{id}` form returned 404 (2026-09-24). Excludes
    /// `sources`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_environment: Option<String>,
    /// The file sources to materialize into the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<EnvironmentSource>>,
    /// Network configuration for the environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkConfig>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — lets an
    /// unmodeled create-request field be set without a crate release.
    ///
    /// The inherent cost (same as
    /// [`TriggerCreateParams::extra`](crate::TriggerCreateParams)): a
    /// *typo'd optional* key in a deserialized config (`netwrok`, ...) is
    /// silently absorbed here and forwarded to the server verbatim rather
    /// than rejected.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value` — the form the request path uses — so the
    /// escape hatch can also override a modeled field whose wire shape
    /// turns out wrong. (`to_string` on a flattened struct emits both keys
    /// rather than deduplicating; don't hand-serialize colliding params.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CreateEnvironmentRequest {
    /// Creates an empty environment request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A request that forks an existing environment, files included.
    ///
    /// ```
    /// use genai_rs::CreateEnvironmentRequest;
    ///
    /// let fork = CreateEnvironmentRequest::from_environment("38aac1ae7f30fe9bd67afe42382ea041");
    /// ```
    #[must_use]
    pub fn from_environment(environment_id: impl Into<String>) -> Self {
        Self {
            from_environment: Some(environment_id.into()),
            ..Self::default()
        }
    }

    /// Adds a file source, accumulating with any added earlier.
    #[must_use]
    pub fn add_source(mut self, source: EnvironmentSource) -> Self {
        self.sources.get_or_insert_with(Vec::new).push(source);
        self
    }

    /// Sets the network configuration.
    #[must_use]
    pub fn with_network(mut self, network: NetworkConfig) -> Self {
        self.network = Some(network);
        self
    }
}

/// Response from listing environments.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct EnvironmentListResponse {
    /// The environments in this page. A null or malformed list degrades to
    /// empty; malformed elements drop individually.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub environments: Vec<Environment>,
    /// Token for fetching the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Environments resource methods; see [IDs](crate::environments#ids).
impl Client {
    /// Creates an environment explicitly, for reuse across interactions.
    ///
    /// Requests can also create environments implicitly by passing a typed
    /// [`EnvironmentSpec`] inline; explicit creation
    /// returns the ID so many interactions can share one container.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the definition is
    /// rejected.
    pub async fn create_environment(
        &self,
        request: &crate::CreateEnvironmentRequest,
    ) -> Result<crate::Environment, GenaiError> {
        crate::http::environments::create_environment(&self.http, request).await
    }

    /// Retrieves an environment by ID.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the environment doesn't
    /// exist.
    pub async fn get_environment(
        &self,
        environment_id: &str,
    ) -> Result<crate::Environment, GenaiError> {
        crate::http::environments::get_environment(&self.http, environment_id).await
    }

    /// Lists environments, paged.
    ///
    /// # Arguments
    ///
    /// * `page_size` - Optional maximum number of environments per page.
    /// * `page_token` - Optional token from a previous list call.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or an invalid page token.
    pub async fn list_environments(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::EnvironmentListResponse, GenaiError> {
        crate::http::environments::list_environments(&self.http, page_size, page_token).await
    }

    /// Deletes an environment.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the environment doesn't
    /// exist.
    pub async fn delete_environment(&self, environment_id: &str) -> Result<(), GenaiError> {
        crate::http::environments::delete_environment(&self.http, environment_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_deserializes_live_wire_shape() {
        // Captured from a live GET /v1beta/environments on 2026-08-08:
        // int64s arrive as strings, timestamps as ISO 8601 with offset.
        let json = serde_json::json!({
            "id": "38aac1ae7f30fe9bd67afe42382ea041",
            "sources": [
                {"type": "inline", "target": "/etc/motd", "content": "hello from genai-rs"}
            ],
            "created": "2026-08-08T13:24:10.64798+00:00",
            "updated": "2026-08-08T13:24:10.64798+00:00",
            "status": "active",
            "file_count": "2",
            "size_bytes": "19"
        });
        let env: Environment = serde_json::from_value(json).unwrap();
        assert_eq!(env.id.as_deref(), Some("38aac1ae7f30fe9bd67afe42382ea041"));
        assert_eq!(env.status, Some(EnvironmentStatus::Active));
        assert_eq!(env.file_count, Some(2));
        assert_eq!(env.size_bytes, Some(19));
        assert!(env.created.is_some());

        // Roundtrip preserves the protobuf-JSON string form the API sent.
        let back = serde_json::to_value(&env).unwrap();
        assert_eq!(back["file_count"], serde_json::json!("2"));
        assert_eq!(back["size_bytes"], serde_json::json!("19"));
        // Timestamps are deliberately NOT byte-faithful: chrono re-emits
        // the same instant in its own RFC 3339 spelling (Z suffix, padded
        // fraction) rather than the captured `+00:00` form — see the
        // serde_util module doc for the scoped fidelity claim.
        assert_eq!(
            back["created"],
            serde_json::json!("2026-08-08T13:24:10.647980Z")
        );
    }

    #[test]
    fn environment_timestamps_degrade_per_field() {
        // Uniform with the trigger family: a timestamp arriving in an
        // unexpected encoding drops that field to None instead of failing
        // the whole list response, even though this resource's encoding is
        // live-verified — the int64s next door got the same tolerance.
        let json = serde_json::json!({
            "id": "env-1",
            "created": "2026-08-08T13:24:10.64798+00:00",
            "updated": "not-a-time",
            "last_accessed": 1754656200
        });
        let env: Environment = serde_json::from_value(json).unwrap();
        assert_eq!(env.id.as_deref(), Some("env-1"));
        assert!(env.created.is_some());
        assert_eq!(env.updated, None);
        assert_eq!(env.last_accessed, None);
    }

    #[test]
    fn numeric_counts_also_accepted() {
        let json = serde_json::json!({"id": "x", "file_count": 3, "size_bytes": 42});
        let env: Environment = serde_json::from_value(json).unwrap();
        assert_eq!(env.file_count, Some(3));
        assert_eq!(env.size_bytes, Some(42));
    }

    #[cfg(not(feature = "strict-unknown"))]
    #[test]
    fn unknown_status_roundtrips() {
        let json = serde_json::json!({"id": "x", "status": "hibernating"});
        let env: Environment = serde_json::from_value(json).unwrap();
        assert!(env.status.as_ref().unwrap().is_unknown());
        assert_eq!(
            env.status.as_ref().unwrap().unknown_status_type(),
            Some("hibernating")
        );
    }

    #[test]
    fn display_agrees_with_wire_value() {
        for (status, wire) in [
            (EnvironmentStatus::Active, "active"),
            (EnvironmentStatus::Expired, "expired"),
        ] {
            assert_eq!(serde_json::to_value(&status).unwrap(), wire);
            assert_eq!(status.to_string(), wire);
        }
    }

    #[test]
    fn add_source_accumulates() {
        let request = CreateEnvironmentRequest::new()
            .add_source(EnvironmentSource::inline("/a", "one"))
            .add_source(EnvironmentSource::inline("/b", "two"));
        assert_eq!(request.sources.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn extra_passes_through_and_wins_on_collision() {
        // Same contract as the trigger bodies: novel keys pass through,
        // and a colliding key wins on serialize (the flattened map is
        // emitted last) — pinned so the precedence reads as a decision.
        let mut request =
            CreateEnvironmentRequest::new().add_source(EnvironmentSource::inline("/a", "one"));
        request
            .extra
            .insert("future_field".into(), serde_json::json!(true));
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["future_field"], true);

        request
            .extra
            .insert("sources".into(), serde_json::json!([]));
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["sources"], serde_json::json!([]));

        // The deserialize direction: an unmodeled key on the way in lands
        // in `extra` — pinned because flatten reroutes every sibling
        // through serde's buffering, where a regression would be silent.
        let request: CreateEnvironmentRequest = serde_json::from_value(serde_json::json!({
            "sources": [{"type": "inline", "target": "/a", "content": "one"}],
            "future_field": true
        }))
        .unwrap();
        assert_eq!(request.extra["future_field"], true);
    }

    #[test]
    fn fork_request_serializes_from_environment() {
        assert_eq!(
            serde_json::to_value(CreateEnvironmentRequest::from_environment("env-1")).unwrap(),
            serde_json::json!({"from_environment": "env-1"})
        );
    }

    #[test]
    fn create_request_network_serializes_without_discriminator() {
        use super::NetworkConfig;

        let request = CreateEnvironmentRequest::new().with_network(NetworkConfig::Disabled);
        let json = serde_json::to_value(&request).unwrap();
        assert!(json.get("network").is_some(), "network key present");
        assert!(
            json.get("type").is_none(),
            "the standalone create body must not carry the inline union's \
             `remote` discriminator: {json}"
        );
    }

    #[test]
    fn empty_list_response_deserializes() {
        // GET /v1beta/environments returns `{}` when nothing exists.
        let list: EnvironmentListResponse = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(list.environments.is_empty());
        assert!(list.next_page_token.is_none());

        // Present-but-degenerate list keys — the shapes the struct-level
        // serde default does not reach — degrade rather than zeroing the
        // page with an error: null and non-array values read as empty.
        let list: EnvironmentListResponse =
            serde_json::from_value(serde_json::json!({"environments": null})).unwrap();
        assert!(list.environments.is_empty());
        let list: EnvironmentListResponse =
            serde_json::from_value(serde_json::json!({"environments": 7})).unwrap();
        assert!(list.environments.is_empty());
    }

    // --- Evergreen `extra` passthrough on response shapes (#406) ---

    #[test]
    fn environment_preserves_unknown_response_fields() {
        let wire = serde_json::json!({
            "id": "env_123",
            "future_field": "value"
        });

        let environment: Environment = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            environment.extra.get("future_field"),
            Some(&serde_json::json!("value"))
        );
        assert_eq!(serde_json::to_value(&environment).unwrap(), wire);
    }

    #[test]
    fn environment_without_unknown_fields_has_empty_extra() {
        let environment: Environment =
            serde_json::from_value(serde_json::json!({"id": "env_123"})).unwrap();
        assert!(environment.extra.is_empty());
        assert_eq!(
            serde_json::to_value(&environment).unwrap(),
            serde_json::json!({"id": "env_123"})
        );
    }
}
