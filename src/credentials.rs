//! Credentials resource (`/v1beta/credentials`).
//!
//! Server-managed secrets that a sandboxed environment can reference by ID
//! instead of carrying them inline: from an environment variable
//! ([`EnvVar::credential`](crate::EnvVar::credential)) or an egress allowlist
//! rule ([`AllowlistEntry::with_credential`](crate::AllowlistEntry::with_credential)).
//! Secret material is write-only; reads return only metadata.
//!
//! Manage them through the [`Credentials`] handle from
//! [`Client::credentials`]: [`create`](Credentials::create),
//! [`get`](Credentials::get), [`list`](Credentials::list),
//! [`update`](Credentials::update) and [`delete`](Credentials::delete).
//!
//! Verified live 2026-09-24: create (bearer token, environment variable),
//! get, list, patch (with and without `update_mask`), delete; the ID is
//! optional on create (a UUID is assigned). OAuth2 creation validates that
//! `token_url` is reachable.
//!
//! Both references take effect at runtime (verified live 2026-09-24 in a
//! `DEFAULT_ANTIGRAVITY_AGENT` sandbox, pinned by
//! `tests/credentials_tests.rs`). An `environment_variable` credential is
//! never visible inside the sandbox: the variable holds a placeholder, and the
//! egress proxy substitutes the secret into requests to its `trusted_domains`
//! at its `injection_location`s. A bearer credential on an allowlist entry is
//! injected into requests to that domain.
//!
//! # IDs
//!
//! Methods take the bare ID ([`Credential::id`]), not a `credentials/...`
//! resource name: the ID is percent-encoded into a single path segment, and
//! the API rejects a resource name there with a 400 (verified live
//! 2026-09-27). The API allows letters, digits and hyphens, up to 63
//! characters, and checks this on create too. An empty or dot-segment ID
//! fails locally with [`GenaiError::InvalidInput`] before any request.

use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
use crate::serde_util::{ResourceName, deserialize_lenient_timestamp};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

struct ForCredential;
impl ResourceName for ForCredential {
    const NAME: &'static str = "Credential";
}

wire_enum! {
    /// Kind of credential (wire field `type`).
    pub enum CredentialType {
        /// A static bearer token injected as an HTTP header.
        BearerToken = "bearer_token",
        /// A secret value exposed as an environment variable.
        EnvironmentVariable = "environment_variable",
        /// OAuth2 client credentials with automatic token refresh.
        OAuth2 = "oauth2",
    }
    unknown(credential_type, unknown_credential_type)
}

wire_enum! {
    /// Lifecycle status of a credential.
    pub enum CredentialStatus {
        /// Usable.
        Active = "active",
        /// Revoked.
        Revoked = "revoked",
    }
    unknown(status_type, unknown_status_type)
}

wire_enum! {
    /// Where in an outgoing HTTP request an environment-variable credential
    /// may be injected.
    pub enum InjectionLocation {
        /// A request header.
        Header = "header",
        /// A query parameter.
        Query = "query",
        /// The request body.
        Body = "body",
    }
    unknown(location_type, unknown_location_type)
}

/// A stored credential's metadata. Secrets are never returned.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct Credential {
    /// The credential ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Kind of credential.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub credential_type: Option<CredentialType>,
    /// Lifecycle status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<CredentialStatus>,
    /// When the credential was created.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForCredential>"
    )]
    pub create_time: Option<DateTime<Utc>>,
    /// When the credential was last updated.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForCredential>"
    )]
    pub update_time: Option<DateTime<Utc>>,
    /// Unmodeled fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing credentials.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct CredentialListResponse {
    /// The credentials in this page.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub credentials: Vec<Credential>,
    /// Token for the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(CredentialListResponse, credentials: Credential);

/// The secret material of a new credential, tagged by `type` on the wire.
///
/// `Debug` prints the write-only secrets as `[REDACTED]`.
#[derive(Clone, Serialize, PartialEq)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum CredentialConfig {
    /// A static bearer token.
    #[serde(rename = "bearer_token")]
    BearerToken {
        /// The token (write-only).
        token: String,
        /// Header to inject into; the API defaults to `Authorization`.
        ///
        /// Accepted but not applied as of 2026-09-24: a custom header name
        /// still arrived as `Authorization` in two live probes.
        #[serde(skip_serializing_if = "Option::is_none")]
        header_name: Option<String>,
        /// Prefix before the token; the API defaults to `Bearer`. `""` for
        /// none.
        ///
        /// Accepted but not applied as of 2026-09-24, like `header_name`.
        #[serde(skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
    /// A secret exposed as an environment variable.
    #[serde(rename = "environment_variable")]
    EnvironmentVariable {
        /// The secret value (write-only).
        value: String,
        /// Where the value may be injected; at least one.
        injection_location: Vec<InjectionLocation>,
        /// Domains allowed to receive the value.
        #[serde(skip_serializing_if = "Option::is_none")]
        trusted_domains: Option<Vec<String>>,
    },
    /// OAuth2 client credentials.
    #[serde(rename = "oauth2")]
    OAuth2 {
        /// Client ID.
        client_id: String,
        /// Client secret (write-only).
        client_secret: String,
        /// Refresh token (write-only).
        refresh_token: String,
        /// Token endpoint; must be reachable at create time.
        token_url: String,
        /// Scopes to request.
        #[serde(skip_serializing_if = "Option::is_none")]
        scopes: Option<Vec<String>>,
    },
}

// Custom Debug that redacts the write-only secrets (mirrors the api_key
// redaction on `Client`), so `dbg!` or `tracing::debug!(?request)` cannot
// undo the LOUD_WIRE redaction.
impl std::fmt::Debug for CredentialConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BearerToken {
                token: _,
                header_name,
                prefix,
            } => f
                .debug_struct("BearerToken")
                .field("token", &"[REDACTED]")
                .field("header_name", header_name)
                .field("prefix", prefix)
                .finish(),
            Self::EnvironmentVariable {
                value: _,
                injection_location,
                trusted_domains,
            } => f
                .debug_struct("EnvironmentVariable")
                .field("value", &"[REDACTED]")
                .field("injection_location", injection_location)
                .field("trusted_domains", trusted_domains)
                .finish(),
            Self::OAuth2 {
                client_id,
                client_secret: _,
                refresh_token: _,
                token_url,
                scopes,
            } => f
                .debug_struct("OAuth2")
                .field("client_id", client_id)
                .field("client_secret", &"[REDACTED]")
                .field("refresh_token", &"[REDACTED]")
                .field("token_url", token_url)
                .field("scopes", scopes)
                .finish(),
        }
    }
}

/// Request body for [`Credentials::create`]. `Debug` redacts the
/// secret (see [`CredentialConfig`]).
///
/// ```
/// use genai_rs::CreateCredentialRequest;
///
/// let request = CreateCredentialRequest::bearer_token("s3cret").with_id("github-token");
/// ```
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CreateCredentialRequest {
    /// Credential ID; the API assigns a UUID when omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The secret material.
    #[serde(flatten)]
    pub config: CredentialConfig,
}

impl CreateCredentialRequest {
    /// A request for the given secret material.
    #[must_use]
    pub const fn new(config: CredentialConfig) -> Self {
        Self { id: None, config }
    }

    /// A bearer token injected as `Authorization: Bearer <token>`.
    #[must_use]
    pub fn bearer_token(token: impl Into<String>) -> Self {
        Self::new(CredentialConfig::BearerToken {
            token: token.into(),
            header_name: None,
            prefix: None,
        })
    }

    /// An environment-variable secret injectable at `locations`.
    #[must_use]
    pub fn environment_variable(
        value: impl Into<String>,
        locations: Vec<InjectionLocation>,
    ) -> Self {
        Self::new(CredentialConfig::EnvironmentVariable {
            value: value.into(),
            injection_location: locations,
            trusted_domains: None,
        })
    }

    /// Sets the credential ID.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Request body for [`Credentials::update`]. `credential_type` must
/// match the stored credential's type; set only the fields to change.
/// `Debug` prints the write-only secrets as `[REDACTED]`.
///
/// [`with_update_mask`](Self::with_update_mask) adds an `update_mask` query
/// parameter naming the fields to apply.
///
/// ```
/// use genai_rs::{CredentialType, CredentialUpdate};
///
/// let update = CredentialUpdate {
///     token: Some("rotated".into()),
///     ..CredentialUpdate::new(CredentialType::BearerToken)
/// }
/// .with_update_mask("token");
/// // The mask travels in the query string, not the body.
/// assert_eq!(
///     serde_json::to_value(&update).unwrap(),
///     serde_json::json!({"type": "bearer_token", "token": "rotated"})
/// );
/// ```
#[derive(Clone, Serialize, PartialEq)]
pub struct CredentialUpdate {
    /// Type of the credential being updated.
    #[serde(rename = "type")]
    pub credential_type: CredentialType,
    /// Bearer token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Bearer header name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_name: Option<String>,
    /// Bearer prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Environment-variable value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Environment-variable injection locations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub injection_location: Option<Vec<InjectionLocation>>,
    /// Environment-variable trusted domains.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_domains: Option<Vec<String>>,
    /// OAuth2 client ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// OAuth2 client secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// OAuth2 refresh token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// OAuth2 token endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    /// OAuth2 scopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    /// Comma-separated fields for the server to apply (e.g. `"token"`).
    /// [`Credentials::update`] sends it as the `update_mask` query
    /// parameter; it is never part of the body. `None` omits the parameter.
    ///
    /// Verified live 2026-09-24: an update is accepted with and without a
    /// mask.
    #[serde(skip)]
    pub update_mask: Option<String>,
}

// Custom Debug that redacts the write-only secrets, like `CredentialConfig`.
// Destructured with no `..`, so adding a field is a compile error here until
// someone decides whether it needs redacting.
impl std::fmt::Debug for CredentialUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            credential_type,
            token,
            header_name,
            prefix,
            value,
            injection_location,
            trusted_domains,
            client_id,
            client_secret,
            refresh_token,
            token_url,
            scopes,
            update_mask,
        } = self;
        let redact = |secret: &Option<String>| secret.as_ref().map(|_| "[REDACTED]");
        f.debug_struct("CredentialUpdate")
            .field("credential_type", credential_type)
            .field("token", &redact(token))
            .field("header_name", header_name)
            .field("prefix", prefix)
            .field("value", &redact(value))
            .field("injection_location", injection_location)
            .field("trusted_domains", trusted_domains)
            .field("client_id", client_id)
            .field("client_secret", &redact(client_secret))
            .field("refresh_token", &redact(refresh_token))
            .field("token_url", token_url)
            .field("scopes", scopes)
            .field("update_mask", update_mask)
            .finish()
    }
}

impl CredentialUpdate {
    /// An update that changes nothing yet.
    #[must_use]
    pub const fn new(credential_type: CredentialType) -> Self {
        Self {
            credential_type,
            token: None,
            header_name: None,
            prefix: None,
            value: None,
            injection_location: None,
            trusted_domains: None,
            client_id: None,
            client_secret: None,
            refresh_token: None,
            token_url: None,
            scopes: None,
            update_mask: None,
        }
    }

    /// Sets the `update_mask` query parameter: comma-separated field names
    /// such as `"token,prefix"`. See [`update_mask`](Self::update_mask).
    #[must_use]
    pub fn with_update_mask(mut self, mask: impl Into<String>) -> Self {
        self.update_mask = Some(mask.into());
        self
    }
}

impl Client {
    /// The `/v1beta/credentials` resource: create, get, list, update and
    /// delete the secrets an environment can reference.
    ///
    /// The handle borrows the client and is `Copy`; see
    /// [IDs](crate::credentials#ids) for what the methods take.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let credential = client.credentials().get("github-token").await?;
    /// # let _ = credential;
    /// # Ok(())
    /// # }
    /// ```
    pub fn credentials(&self) -> Credentials<'_> {
        Credentials { client: self }
    }
}

/// The `/v1beta/credentials` resource, from [`Client::credentials`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle: `client.credentials().get(id)` can be stored or
/// joined with others. See [IDs](crate::credentials#ids).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct Credentials<'a> {
    client: &'a Client,
}

impl<'a> Credentials<'a> {
    /// Creates a credential.
    ///
    /// The response carries only metadata: the secret is write-only and is
    /// never returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the ID already exists (409), the config is
    /// invalid, or the request fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{AllowlistEntry, CreateCredentialRequest, NetworkConfig, RemoteEnvironment};
    ///
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let credential = client
    ///     .credentials()
    ///     .create(&CreateCredentialRequest::bearer_token("s3cret").with_id("github-token"))
    ///     .await?;
    ///
    /// // Reference it by ID: requests to the domain carry the token.
    /// let env = RemoteEnvironment::new().with_network(NetworkConfig::allowlist(vec![
    ///     AllowlistEntry::new("api.github.com").with_credential("github-token"),
    /// ]));
    /// # let _ = (credential, env);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(self, request: &CreateCredentialRequest) -> Result<Credential, GenaiError> {
        crate::http::credentials::create_credential(&self.client.http, request).await
    }

    /// Retrieves a credential's metadata by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential doesn't exist or the request fails.
    pub async fn get(self, credential_id: &str) -> Result<Credential, GenaiError> {
        crate::http::credentials::get_credential(&self.client.http, credential_id).await
    }

    /// Lists credentials: configure the returned [`ListCredentials`], then
    /// call [`send`](ListCredentials::send) for one page, or
    /// [`pages`](ListCredentials::pages) / [`items`](ListCredentials::items)
    /// to stream them all.
    pub fn list(self) -> ListCredentials<'a> {
        ListCredentials {
            client: self.client,
            page_size: None,
            page_token: None,
        }
    }

    /// Updates a credential with the fields set on `update`, sending its
    /// [`update_mask`](CredentialUpdate::update_mask), if any, as the
    /// `update_mask` query parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential doesn't exist, the type doesn't
    /// match (400), or the request fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{CredentialType, CredentialUpdate};
    ///
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let update = CredentialUpdate {
    ///     token: Some("rotated".into()),
    ///     ..CredentialUpdate::new(CredentialType::BearerToken)
    /// };
    /// let updated = client
    ///     .credentials()
    ///     .update("github-token", &update.with_update_mask("token"))
    ///     .await?;
    /// # let _ = updated;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update(
        self,
        credential_id: &str,
        update: &CredentialUpdate,
    ) -> Result<Credential, GenaiError> {
        crate::http::credentials::update_credential(
            &self.client.http,
            credential_id,
            update,
            update.update_mask.as_deref(),
        )
        .await
    }

    /// Deletes a credential.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential doesn't exist or the request fails.
    pub async fn delete(self, credential_id: &str) -> Result<(), GenaiError> {
        crate::http::credentials::delete_credential(&self.client.http, credential_id).await
    }
}

/// A `GET /v1beta/credentials` request, from [`Credentials::list`].
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `next_page_token` to the end of the list. The page size is sent with
/// every page.
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client.credentials().list().with_page_size(50).send().await?;
/// println!("{} credentials, more: {}", page.credentials.len(), page.next_page_token.is_some());
///
/// // Every credential, across pages
/// let all: Vec<genai_rs::Credential> = client.credentials().list().items().try_collect().await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListCredentials<'a> {
    client: &'a Client,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListCredentials<'a> {
    /// Sets the maximum number of credentials per page. Sent with every
    /// page.
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
    /// An empty collection comes back as an empty page with no
    /// `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn send(self) -> Result<CredentialListResponse, GenaiError> {
        crate::http::credentials::list_credentials(
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
    pub fn pages(self) -> BoxStream<'static, Result<CredentialListResponse, GenaiError>> {
        let Self {
            client,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("credentials", page_token, move |token| {
            let client = client.clone();
            async move {
                crate::http::credentials::list_credentials(
                    &client.http,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every credential across pages, in server order. Same rules
    /// as [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<Credential, GenaiError>> {
        paging::items(self.pages())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn debug_redacts_every_create_secret() {
        let requests = [
            CreateCredentialRequest::bearer_token("secret-token").with_id("id-1"),
            CreateCredentialRequest::environment_variable(
                "secret-value",
                vec![InjectionLocation::Header],
            ),
            CreateCredentialRequest::new(CredentialConfig::OAuth2 {
                client_id: "visible-client".into(),
                client_secret: "secret-client".into(),
                refresh_token: "secret-refresh".into(),
                token_url: "https://oauth.example/token".into(),
                scopes: None,
            }),
        ];
        for request in &requests {
            let out = format!("{request:?}");
            assert!(!out.contains("secret-"), "secret leaked: {out}");
            assert!(out.contains("[REDACTED]"), "{out}");
        }
        // Non-secret fields stay visible.
        let oauth = format!("{:?}", requests[2]);
        assert!(oauth.contains("visible-client") && oauth.contains("oauth.example"));
        assert!(format!("{:?}", requests[0]).contains("id-1"));
    }

    #[test]
    fn debug_redacts_update_secrets_only_when_set() {
        let update = CredentialUpdate {
            token: Some("secret-token".into()),
            client_secret: Some("secret-client".into()),
            header_name: Some("X-Visible".into()),
            ..CredentialUpdate::new(CredentialType::BearerToken)
        };
        let out = format!("{update:?}");
        assert!(!out.contains("secret-"), "secret leaked: {out}");
        assert!(out.contains(r#"token: Some("[REDACTED]")"#), "{out}");
        assert!(out.contains("X-Visible"), "{out}");
        // An unset secret reads as None, so the output still says what an
        // update will change.
        assert!(out.contains("value: None"), "{out}");
    }

    /// Captured from a live `POST /v1beta/credentials` (2026-09-24).
    #[test]
    fn credential_deserializes_the_live_shape() {
        let credential: Credential = serde_json::from_value(json!({
            "id": "sweep-bearer-1",
            "status": "active",
            "create_time": "2026-09-24T01:42:57.677356880Z",
            "update_time": "2026-09-24T01:42:57.677356880Z",
            "type": "bearer_token"
        }))
        .unwrap();
        assert_eq!(
            credential.credential_type,
            Some(CredentialType::BearerToken)
        );
        assert_eq!(credential.status, Some(CredentialStatus::Active));
        assert!(credential.create_time.is_some());
        assert!(credential.extra.is_empty());
    }

    #[test]
    fn create_bodies_serialize_the_binding_shapes() {
        assert_eq!(
            serde_json::to_value(CreateCredentialRequest::bearer_token("t").with_id("gh")).unwrap(),
            json!({"id": "gh", "type": "bearer_token", "token": "t"})
        );
        assert_eq!(
            serde_json::to_value(CreateCredentialRequest::environment_variable(
                "v",
                vec![InjectionLocation::Header, InjectionLocation::Query],
            ))
            .unwrap(),
            json!({"type": "environment_variable", "value": "v", "injection_location": ["header", "query"]})
        );
        assert_eq!(
            serde_json::to_value(CreateCredentialRequest::new(CredentialConfig::OAuth2 {
                client_id: "c".into(),
                client_secret: "s".into(),
                refresh_token: "r".into(),
                token_url: "https://oauth2.example/token".into(),
                scopes: Some(vec!["openid".into()]),
            }))
            .unwrap(),
            json!({
                "type": "oauth2", "client_id": "c", "client_secret": "s",
                "refresh_token": "r", "token_url": "https://oauth2.example/token",
                "scopes": ["openid"]
            })
        );
    }

    #[test]
    fn update_serializes_only_set_fields() {
        let update = CredentialUpdate {
            prefix: Some(String::new()),
            ..CredentialUpdate::new(CredentialType::BearerToken)
        };
        assert_eq!(
            serde_json::to_value(update).unwrap(),
            json!({"type": "bearer_token", "prefix": ""})
        );
    }

    #[test]
    fn with_update_mask_sets_the_mask_but_not_the_body() {
        let update = CredentialUpdate::new(CredentialType::BearerToken);
        assert_eq!(update.update_mask, None);

        let update = CredentialUpdate {
            token: Some("rotated".into()),
            ..update
        }
        .with_update_mask("prefix")
        // `with_*` replaces.
        .with_update_mask("token,prefix");
        assert_eq!(update.update_mask.as_deref(), Some("token,prefix"));
        // The mask is a query parameter: it never reaches the PATCH body.
        assert_eq!(
            serde_json::to_value(&update).unwrap(),
            json!({"type": "bearer_token", "token": "rotated"})
        );
        assert_eq!(
            serde_json::to_value(
                CredentialUpdate::new(CredentialType::OAuth2).with_update_mask("scopes")
            )
            .unwrap(),
            json!({"type": "oauth2"})
        );
    }

    #[test]
    fn list_credentials_setters_fill_the_query() {
        let client = Client::new("k".to_string());
        let list = client.credentials().list();
        assert_eq!(list.page_size, None);
        assert_eq!(list.page_token, None);

        let list = list
            .with_page_size(5)
            .with_page_token("t1")
            // `with_*` replaces.
            .with_page_token("t2");
        assert_eq!(list.page_size, Some(5));
        assert_eq!(list.page_token.as_deref(), Some("t2"));
    }

    #[test]
    fn credentials_handle_and_list_debug_redact_the_api_key() {
        let client = Client::new("secret-api-key".to_string());
        for debug in [
            format!("{:?}", client.credentials()),
            format!("{:?}", client.credentials().list().with_page_size(1)),
        ] {
            assert!(!debug.contains("secret-api-key"), "{debug}");
            assert!(debug.contains("[REDACTED]"), "{debug}");
        }
    }

    #[test]
    fn empty_list_response_deserializes() {
        let list: CredentialListResponse = serde_json::from_value(json!({})).unwrap();
        assert!(list.credentials.is_empty());
    }
}
