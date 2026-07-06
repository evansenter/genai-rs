//! Webhook types for the `/v1beta/webhooks` resource and the per-request
//! `webhook_config` field.
//!
//! Webhooks let the API push events (batch completion, interaction lifecycle,
//! video generation) to your HTTPS endpoint instead of requiring polling.
//!
//! - Manage registered webhooks with the [`Client`](crate::Client) methods
//!   `create_webhook`, `get_webhook`, `list_webhooks`, `update_webhook`,
//!   `delete_webhook`, `ping_webhook`, and `rotate_webhook_signing_secret`.
//! - Route a single request's events to ad-hoc URIs with
//!   [`WebhookConfig`] via
//!   [`InteractionBuilder::with_webhook_config()`](crate::InteractionBuilder::with_webhook_config).
//!
//! See `docs/AGENTS_AND_BACKGROUND.md` for the full background-execution +
//! webhook flow.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// An event type a webhook can subscribe to.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New event types may be added in future API versions.
///
/// # Wire Format
///
/// Serializes as dotted lowercase strings: `"batch.succeeded"`,
/// `"interaction.completed"`, `"video.generated"`, etc.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum WebhookEvent {
    /// Batch processing finished successfully.
    BatchSucceeded,
    /// Batch was not processed within the 48h timeframe.
    BatchExpired,
    /// Batch job failed.
    BatchFailed,
    /// Interaction requires action (e.g., function calling).
    InteractionRequiresAction,
    /// Interaction completed successfully.
    InteractionCompleted,
    /// Interaction failed.
    InteractionFailed,
    /// Video generation completed.
    VideoGenerated,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized event type from the API
        event_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl WebhookEvent {
    /// Returns true if this is an unknown webhook event.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the event type name if this is an unknown webhook event.
    #[must_use]
    pub fn unknown_event_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { event_type, .. } => Some(event_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown webhook event.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    const fn as_wire(&self) -> Option<&'static str> {
        match self {
            Self::BatchSucceeded => Some("batch.succeeded"),
            Self::BatchExpired => Some("batch.expired"),
            Self::BatchFailed => Some("batch.failed"),
            Self::InteractionRequiresAction => Some("interaction.requires_action"),
            Self::InteractionCompleted => Some("interaction.completed"),
            Self::InteractionFailed => Some("interaction.failed"),
            Self::VideoGenerated => Some("video.generated"),
            Self::Unknown { .. } => None,
        }
    }
}

impl fmt::Display for WebhookEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_wire() {
            Some(wire) => write!(f, "{}", wire),
            None => match self {
                Self::Unknown { event_type, .. } => write!(f, "{}", event_type),
                _ => unreachable!("known events always have a wire form"),
            },
        }
    }
}

impl Serialize for WebhookEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.as_wire() {
            Some(wire) => serializer.serialize_str(wire),
            None => match self {
                Self::Unknown { event_type, .. } => serializer.serialize_str(event_type),
                _ => unreachable!("known events always have a wire form"),
            },
        }
    }
}

impl<'de> Deserialize<'de> for WebhookEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("batch.succeeded") => Ok(Self::BatchSucceeded),
            Some("batch.expired") => Ok(Self::BatchExpired),
            Some("batch.failed") => Ok(Self::BatchFailed),
            Some("interaction.requires_action") => Ok(Self::InteractionRequiresAction),
            Some("interaction.completed") => Ok(Self::InteractionCompleted),
            Some("interaction.failed") => Ok(Self::InteractionFailed),
            Some("video.generated") => Ok(Self::VideoGenerated),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown WebhookEvent '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Self::Unknown {
                    event_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let event_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "WebhookEvent received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    event_type,
                    data: value,
                })
            }
        }
    }
}

/// The state of a registered webhook (output only).
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase snake_case strings: `"enabled"`, `"disabled"`,
/// `"disabled_due_to_failed_deliveries"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum WebhookState {
    /// Webhook is active and receiving events.
    Enabled,
    /// Webhook is disabled and receives no events.
    Disabled,
    /// The API disabled the webhook after repeated delivery failures.
    DisabledDueToFailedDeliveries,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized state type from the API
        state_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl WebhookState {
    /// Returns true if this is an unknown webhook state.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the state type name if this is an unknown webhook state.
    #[must_use]
    pub fn unknown_state_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { state_type, .. } => Some(state_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown webhook state.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for WebhookState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Enabled => serializer.serialize_str("enabled"),
            Self::Disabled => serializer.serialize_str("disabled"),
            Self::DisabledDueToFailedDeliveries => {
                serializer.serialize_str("disabled_due_to_failed_deliveries")
            }
            Self::Unknown { state_type, .. } => serializer.serialize_str(state_type),
        }
    }
}

impl<'de> Deserialize<'de> for WebhookState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("enabled") => Ok(Self::Enabled),
            Some("disabled") => Ok(Self::Disabled),
            Some("disabled_due_to_failed_deliveries") => Ok(Self::DisabledDueToFailedDeliveries),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown WebhookState '{}' - using Unknown variant (Evergreen)",
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
                    "WebhookState received non-string value: {}. \
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

/// A signing secret used to verify webhook payloads (output only).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SigningSecret {
    /// Truncated version of the signing secret (for identification).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated_secret: Option<String>,
    /// Expiration timestamp of the signing secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<DateTime<Utc>>,
}

/// A Webhook resource.
///
/// Create with [`Webhook::new()`] and register it via
/// [`Client::create_webhook()`](crate::Client::create_webhook). Fields marked
/// "output only" are populated by the API and ignored on create.
///
/// # Example
///
/// ```
/// use genai_rs::{Webhook, WebhookEvent};
///
/// let webhook = Webhook::new(
///     "https://example.com/hooks/genai",
///     vec![WebhookEvent::InteractionCompleted, WebhookEvent::InteractionFailed],
/// )
/// .with_name("my-hook");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Webhook {
    /// The URI to which webhook events will be sent (required).
    pub uri: String,
    /// The events that the webhook is subscribed to (required).
    pub subscribed_events: Vec<WebhookEvent>,
    /// Optional user-provided name of the webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Output only. The ID of the webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Output only. The state of the webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<WebhookState>,
    /// Output only. The signing secrets associated with this webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secrets: Option<Vec<SigningSecret>>,
    /// Output only. The new signing secret. Only populated on create —
    /// store it securely, it is not returned again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_signing_secret: Option<String>,
    /// Output only. When the webhook was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<DateTime<Utc>>,
    /// Output only. When the webhook was last updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<DateTime<Utc>>,
}

impl Webhook {
    /// Creates a new webhook definition for registration.
    #[must_use]
    pub fn new(uri: impl Into<String>, subscribed_events: Vec<WebhookEvent>) -> Self {
        Self {
            uri: uri.into(),
            subscribed_events,
            ..Default::default()
        }
    }

    /// Sets the user-provided name of the webhook.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

/// A partial update for a webhook (`PATCH /v1beta/webhooks/{id}`).
///
/// Only the set fields are updated. Pair with an `update_mask` in
/// [`Client::update_webhook()`](crate::Client::update_webhook) to control
/// which fields the server applies.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebhookUpdate {
    /// New user-provided name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New destination URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// New event subscription list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribed_events: Option<Vec<WebhookEvent>>,
    /// New state (`enabled` / `disabled`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<WebhookState>,
}

impl WebhookUpdate {
    /// Creates an empty update.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a new name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets a new destination URI.
    #[must_use]
    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    /// Sets a new event subscription list.
    #[must_use]
    pub fn with_subscribed_events(mut self, events: Vec<WebhookEvent>) -> Self {
        self.subscribed_events = Some(events);
        self
    }

    /// Sets a new state.
    #[must_use]
    pub fn with_state(mut self, state: WebhookState) -> Self {
        self.state = Some(state);
        self
    }
}

/// Revocation behavior for previous signing secrets when rotating.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as `"revoke_previous_secrets_after_h24"` or
/// `"revoke_previous_secrets_immediately"`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum RevocationBehavior {
    /// Previous secrets stay valid for 24 hours (safe rollover).
    RevokePreviousSecretsAfterH24,
    /// Previous secrets are revoked immediately.
    RevokePreviousSecretsImmediately,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized behavior type from the API
        behavior_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl RevocationBehavior {
    /// Returns true if this is an unknown revocation behavior.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the behavior type name if this is an unknown revocation behavior.
    #[must_use]
    pub fn unknown_behavior_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { behavior_type, .. } => Some(behavior_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown revocation behavior.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for RevocationBehavior {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::RevokePreviousSecretsAfterH24 => {
                serializer.serialize_str("revoke_previous_secrets_after_h24")
            }
            Self::RevokePreviousSecretsImmediately => {
                serializer.serialize_str("revoke_previous_secrets_immediately")
            }
            Self::Unknown { behavior_type, .. } => serializer.serialize_str(behavior_type),
        }
    }
}

impl<'de> Deserialize<'de> for RevocationBehavior {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("revoke_previous_secrets_after_h24") => Ok(Self::RevokePreviousSecretsAfterH24),
            Some("revoke_previous_secrets_immediately") => {
                Ok(Self::RevokePreviousSecretsImmediately)
            }
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown RevocationBehavior '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Self::Unknown {
                    behavior_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let behavior_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "RevocationBehavior received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    behavior_type,
                    data: value,
                })
            }
        }
    }
}

/// Response for `GET /v1beta/webhooks` (list).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebhookListResponse {
    /// The webhooks on this page.
    pub webhooks: Vec<Webhook>,
    /// Token for the next page. Absent when there are no more pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Response for `POST /v1beta/webhooks/{id}:rotateSigningSecret`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RotateSigningSecretResponse {
    /// The newly generated signing secret. Store it securely — it is not
    /// returned again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

/// Per-request webhook configuration (`webhook_config` on an interaction
/// request).
///
/// When set, events for this request are delivered to `uris` instead of the
/// registered webhooks, and `user_metadata` is echoed back on each event.
///
/// # Example
///
/// ```
/// use genai_rs::WebhookConfig;
///
/// let config = WebhookConfig::new()
///     .with_uris(vec!["https://example.com/hooks/genai".to_string()])
///     .with_user_metadata(serde_json::json!({"job": "nightly-report"}));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebhookConfig {
    /// If set, these webhook URIs are used for events from this request
    /// instead of the registered webhooks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uris: Option<Vec<String>>,
    /// User metadata returned on each event emission to the webhooks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_metadata: Option<serde_json::Value>,
}

impl WebhookConfig {
    /// Creates an empty webhook config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the override webhook URIs for this request.
    #[must_use]
    pub fn with_uris(mut self, uris: Vec<String>) -> Self {
        self.uris = Some(uris);
        self
    }

    /// Sets the user metadata echoed back on each event emission.
    #[must_use]
    pub fn with_user_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.user_metadata = Some(metadata);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_webhook_event_wire_roundtrip() {
        for (event, wire) in [
            (WebhookEvent::BatchSucceeded, "\"batch.succeeded\""),
            (WebhookEvent::BatchExpired, "\"batch.expired\""),
            (WebhookEvent::BatchFailed, "\"batch.failed\""),
            (
                WebhookEvent::InteractionRequiresAction,
                "\"interaction.requires_action\"",
            ),
            (
                WebhookEvent::InteractionCompleted,
                "\"interaction.completed\"",
            ),
            (WebhookEvent::InteractionFailed, "\"interaction.failed\""),
            (WebhookEvent::VideoGenerated, "\"video.generated\""),
        ] {
            assert_eq!(serde_json::to_string(&event).unwrap(), wire);
            let parsed: WebhookEvent = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, event);
        }
    }

    #[test]
    fn test_webhook_event_unknown_roundtrip() {
        let unknown: WebhookEvent = serde_json::from_str("\"file.generated\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_event_type(), Some("file.generated"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(
            serde_json::to_string(&unknown).unwrap(),
            "\"file.generated\""
        );
    }

    #[test]
    fn test_webhook_state_wire_roundtrip() {
        for (state, wire) in [
            (WebhookState::Enabled, "\"enabled\""),
            (WebhookState::Disabled, "\"disabled\""),
            (
                WebhookState::DisabledDueToFailedDeliveries,
                "\"disabled_due_to_failed_deliveries\"",
            ),
        ] {
            assert_eq!(serde_json::to_string(&state).unwrap(), wire);
            let parsed: WebhookState = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, state);
        }
    }

    #[test]
    fn test_webhook_state_unknown_roundtrip() {
        let unknown: WebhookState = serde_json::from_str("\"paused\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_state_type(), Some("paused"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"paused\"");
    }

    #[test]
    fn test_revocation_behavior_wire_roundtrip() {
        for (behavior, wire) in [
            (
                RevocationBehavior::RevokePreviousSecretsAfterH24,
                "\"revoke_previous_secrets_after_h24\"",
            ),
            (
                RevocationBehavior::RevokePreviousSecretsImmediately,
                "\"revoke_previous_secrets_immediately\"",
            ),
        ] {
            assert_eq!(serde_json::to_string(&behavior).unwrap(), wire);
            let parsed: RevocationBehavior = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, behavior);
        }
    }

    #[test]
    fn test_revocation_behavior_unknown_roundtrip() {
        let unknown: RevocationBehavior = serde_json::from_str("\"revoke_after_week\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_behavior_type(), Some("revoke_after_week"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(
            serde_json::to_string(&unknown).unwrap(),
            "\"revoke_after_week\""
        );
    }

    #[test]
    fn test_webhook_new_serializes_input_fields_only() {
        let webhook = Webhook::new(
            "https://example.com/hook",
            vec![WebhookEvent::InteractionCompleted],
        )
        .with_name("my-hook");

        let value = serde_json::to_value(&webhook).unwrap();
        assert_eq!(value["uri"], "https://example.com/hook");
        assert_eq!(value["subscribed_events"][0], "interaction.completed");
        assert_eq!(value["name"], "my-hook");
        // Output-only fields are skipped when unset
        for field in [
            "id",
            "state",
            "signing_secrets",
            "new_signing_secret",
            "create_time",
            "update_time",
        ] {
            assert!(value.get(field).is_none(), "{field} should be skipped");
        }
    }

    #[test]
    fn test_webhook_full_resource_roundtrip() {
        // Wire fixture derived from the generated google-genai bindings.
        let json = json!({
            "id": "webhooks/wh-123",
            "name": "my-hook",
            "uri": "https://example.com/hook",
            "subscribed_events": ["batch.succeeded", "interaction.failed", "video.generated"],
            "state": "enabled",
            "signing_secrets": [
                {"truncated_secret": "whsec_...abcd", "expire_time": "2026-08-01T00:00:00Z"}
            ],
            "new_signing_secret": "whsec_full_secret",
            "create_time": "2026-07-01T12:00:00Z",
            "update_time": "2026-07-02T12:00:00Z"
        });

        let webhook: Webhook = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(webhook.id.as_deref(), Some("webhooks/wh-123"));
        assert_eq!(webhook.state, Some(WebhookState::Enabled));
        assert_eq!(webhook.subscribed_events.len(), 3);
        assert_eq!(
            webhook.signing_secrets.as_ref().unwrap()[0]
                .truncated_secret
                .as_deref(),
            Some("whsec_...abcd")
        );

        let back = serde_json::to_value(&webhook).unwrap();
        assert_eq!(back, json);
    }

    #[test]
    fn test_webhook_update_partial_serialization() {
        let update = WebhookUpdate::new().with_state(WebhookState::Disabled);
        let value = serde_json::to_value(&update).unwrap();
        assert_eq!(value, json!({"state": "disabled"}));
    }

    #[test]
    fn test_webhook_list_response_deserialization() {
        let json = json!({
            "webhooks": [
                {"uri": "https://a.example.com", "subscribed_events": ["batch.failed"]}
            ],
            "next_page_token": "tok-1"
        });
        let list: WebhookListResponse = serde_json::from_value(json).unwrap();
        assert_eq!(list.webhooks.len(), 1);
        assert_eq!(list.next_page_token.as_deref(), Some("tok-1"));

        // Empty response is valid too
        let empty: WebhookListResponse = serde_json::from_str("{}").unwrap();
        assert!(empty.webhooks.is_empty());
        assert!(empty.next_page_token.is_none());
    }

    #[test]
    fn test_rotate_signing_secret_response() {
        let response: RotateSigningSecretResponse =
            serde_json::from_str(r#"{"secret": "whsec_new"}"#).unwrap();
        assert_eq!(response.secret.as_deref(), Some("whsec_new"));

        let empty: RotateSigningSecretResponse = serde_json::from_str("{}").unwrap();
        assert!(empty.secret.is_none());
    }

    #[test]
    fn test_webhook_config_serialization() {
        let config = WebhookConfig::new()
            .with_uris(vec!["https://example.com/hook".to_string()])
            .with_user_metadata(json!({"job": "nightly"}));

        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(
            value,
            json!({
                "uris": ["https://example.com/hook"],
                "user_metadata": {"job": "nightly"}
            })
        );
    }

    #[test]
    fn test_webhook_config_empty_serializes_to_empty_object() {
        let config = WebhookConfig::new();
        assert_eq!(serde_json::to_string(&config).unwrap(), "{}");
    }

    #[test]
    fn test_webhook_config_roundtrip() {
        let config = WebhookConfig::new()
            .with_uris(vec!["https://example.com/a".to_string()])
            .with_user_metadata(json!({"k": [1, 2, 3]}));
        let json = serde_json::to_string(&config).unwrap();
        let parsed: WebhookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_webhook_event_display() {
        assert_eq!(
            WebhookEvent::InteractionCompleted.to_string(),
            "interaction.completed"
        );
        let unknown = WebhookEvent::Unknown {
            event_type: "x.y".to_string(),
            data: json!("x.y"),
        };
        assert_eq!(unknown.to_string(), "x.y");
    }
}
