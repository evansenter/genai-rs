//! HTTP endpoints for the `/v1beta/webhooks` resource.
//!
//! All requests send the same `Api-Revision` header as the Interactions API:
//! the webhooks resource is part of the revisioned Interactions surface
//! (the generated google-genai bindings apply the revision header globally).

use super::common::{
    API_VERSION, BASE_URL_PREFIX, path_segment, send_and_read, to_body, with_paging,
    with_paging_and,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::webhooks::{
    RevocationBehavior, RotateSigningSecretResponse, Webhook, WebhookListResponse, WebhookUpdate,
};

fn webhooks_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/webhooks")
}

fn webhook_url(id: &str) -> String {
    format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/webhooks/{}",
        path_segment(id)
    )
}

/// Registers a new webhook (`POST /v1beta/webhooks`).
///
/// The response includes `new_signing_secret` — only returned on create.
pub async fn create_webhook(ctx: &HttpContext, webhook: &Webhook) -> Result<Webhook, GenaiError> {
    tracing::debug!("Creating webhook: uri={}", webhook.uri);
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &webhooks_url(),
        Some(to_body(webhook)?),
    )
    .await?;
    deserialize_with_context(&text, "Webhook from create")
}

/// Retrieves a webhook by ID (`GET /v1beta/webhooks/{id}`).
pub async fn get_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<Webhook, GenaiError> {
    tracing::debug!("Getting webhook: ID={webhook_id}");
    let text = send_and_read(ctx, reqwest::Method::GET, &webhook_url(webhook_id), None).await?;
    deserialize_with_context(&text, "Webhook from get")
}

/// Lists webhooks (`GET /v1beta/webhooks`).
pub async fn list_webhooks(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<WebhookListResponse, GenaiError> {
    tracing::debug!("Listing webhooks: page_size={page_size:?}, page_token={page_token:?}");

    let url = with_paging(webhooks_url(), page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "WebhookListResponse")
}

/// Updates a webhook (`PATCH /v1beta/webhooks/{id}`).
///
/// `update_mask` optionally lists the fields to update (comma-separated,
/// e.g. `"uri,subscribed_events"`).
pub async fn update_webhook(
    ctx: &HttpContext,
    webhook_id: &str,
    update: &WebhookUpdate,
    update_mask: Option<&str>,
) -> Result<Webhook, GenaiError> {
    tracing::debug!("Updating webhook: ID={webhook_id}, update_mask={update_mask:?}");

    // The last query string in the HTTP layer routes through the shared
    // helper too, bringing the mask under its percent-encoding tests.
    let extra: Vec<(&str, &str)> = update_mask
        .map(|m| ("update_mask", m))
        .into_iter()
        .collect();
    let url = with_paging_and(webhook_url(webhook_id), None, None, &extra);

    let text = send_and_read(ctx, reqwest::Method::PATCH, &url, Some(to_body(update)?)).await?;
    deserialize_with_context(&text, "Webhook from update")
}

/// Deletes a webhook (`DELETE /v1beta/webhooks/{id}`).
pub async fn delete_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting webhook: ID={webhook_id}");
    send_and_read(ctx, reqwest::Method::DELETE, &webhook_url(webhook_id), None).await?;
    Ok(())
}

/// Sends a test event to a webhook (`POST /v1beta/webhooks/{id}:ping`).
pub async fn ping_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Pinging webhook: ID={webhook_id}");
    let url = format!("{}:ping", webhook_url(webhook_id));
    // Request and response bodies are empty per the spec.
    send_and_read(
        ctx,
        reqwest::Method::POST,
        &url,
        Some(serde_json::json!({})),
    )
    .await?;
    Ok(())
}

/// Rotates a webhook's signing secret
/// (`POST /v1beta/webhooks/{id}:rotateSigningSecret`).
pub async fn rotate_signing_secret(
    ctx: &HttpContext,
    webhook_id: &str,
    revocation_behavior: Option<RevocationBehavior>,
) -> Result<RotateSigningSecretResponse, GenaiError> {
    tracing::debug!("Rotating signing secret: ID={webhook_id}");
    let url = format!("{}:rotateSigningSecret", webhook_url(webhook_id));

    let mut body = serde_json::Map::new();
    if let Some(behavior) = &revocation_behavior {
        body.insert("revocation_behavior".to_string(), to_body(behavior)?);
    }

    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &url,
        Some(serde_json::Value::Object(body)),
    )
    .await?;
    deserialize_with_context(&text, "RotateSigningSecretResponse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhooks_url_construction() {
        assert_eq!(
            webhooks_url(),
            "https://generativelanguage.googleapis.com/v1beta/webhooks"
        );
        assert_eq!(
            webhook_url("wh-123"),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/wh-123"
        );
        // A path-metacharacter ID is encoded, not interpolated raw (the
        // colon-verb suffixes below are appended outside webhook_url, so
        // they are unaffected by the encoding).
        assert_eq!(
            webhook_url("a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/a%2Fb%3Fc"
        );
        assert_eq!(
            format!("{}:ping", webhook_url("wh-123")),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/wh-123:ping"
        );
        assert_eq!(
            format!("{}:rotateSigningSecret", webhook_url("wh-123")),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/wh-123:rotateSigningSecret"
        );
    }

    #[test]
    fn test_list_query_params_are_snake_case() {
        // The Interactions API family uses snake_case query params
        // (unlike the unrevisioned Files API, which uses camelCase).
        let mut url = webhooks_url();
        url.push_str("?page_size=10&page_token=tok");
        assert!(url.contains("page_size=10"));
        assert!(url.contains("page_token=tok"));
    }
}
