//! HTTP endpoints for the `/v1beta/webhooks` resource.
//!
//! All requests send the same `Api-Revision` header as the Interactions API:
//! the webhooks resource is part of the revisioned Interactions surface
//! (the generated google-genai bindings apply the revision header globally).

use super::common::{NO_BODY, path_segment, require_id, send_and_read, with_paging, with_query};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::webhooks::{
    RevocationBehavior, RotateSigningSecretResponse, Webhook, WebhookListResponse, WebhookUpdate,
};

fn webhooks_url(ctx: &HttpContext) -> String {
    ctx.api_url("webhooks")
}

fn webhook_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("webhooks/{}", path_segment(id)))
}

/// Registers a new webhook (`POST /v1beta/webhooks`).
///
/// The response includes `new_signing_secret` — only returned on create.
pub async fn create_webhook(ctx: &HttpContext, webhook: &Webhook) -> Result<Webhook, GenaiError> {
    tracing::debug!("Creating webhook: uri={}", webhook.uri);
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &webhooks_url(ctx),
        Some(webhook),
    )
    .await?;
    deserialize_with_context(&text, "Webhook from create")
}

/// Retrieves a webhook by ID (`GET /v1beta/webhooks/{id}`).
pub async fn get_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<Webhook, GenaiError> {
    require_id(webhook_id, "webhook")?;
    tracing::debug!("Getting webhook: ID={webhook_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::GET,
        &webhook_url(ctx, webhook_id),
        NO_BODY,
    )
    .await?;
    deserialize_with_context(&text, "Webhook from get")
}

/// Lists webhooks (`GET /v1beta/webhooks`).
pub async fn list_webhooks(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<WebhookListResponse, GenaiError> {
    tracing::debug!("Listing webhooks: page_size={page_size:?}, page_token={page_token:?}");

    let url = with_paging(webhooks_url(ctx), page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
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
    require_id(webhook_id, "webhook")?;
    tracing::debug!("Updating webhook: ID={webhook_id}, update_mask={update_mask:?}");

    let url = with_query(
        webhook_url(ctx, webhook_id),
        &[("update_mask", update_mask)],
    );
    let text = send_and_read(ctx, reqwest::Method::PATCH, &url, Some(update)).await?;
    deserialize_with_context(&text, "Webhook from update")
}

/// Deletes a webhook (`DELETE /v1beta/webhooks/{id}`).
pub async fn delete_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<(), GenaiError> {
    require_id(webhook_id, "webhook")?;
    tracing::debug!("Deleting webhook: ID={webhook_id}");
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &webhook_url(ctx, webhook_id),
        NO_BODY,
    )
    .await?;
    Ok(())
}

/// Sends a test event to a webhook (`POST /v1beta/webhooks/{id}:ping`).
pub async fn ping_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<(), GenaiError> {
    require_id(webhook_id, "webhook")?;
    tracing::debug!("Pinging webhook: ID={webhook_id}");
    let url = format!("{}:ping", webhook_url(ctx, webhook_id));
    // Request and response bodies are empty per the spec.
    send_and_read(
        ctx,
        reqwest::Method::POST,
        &url,
        Some(&serde_json::json!({})),
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
    require_id(webhook_id, "webhook")?;
    tracing::debug!("Rotating signing secret: ID={webhook_id}");
    let url = format!("{}:rotateSigningSecret", webhook_url(ctx, webhook_id));

    #[derive(serde::Serialize)]
    struct RotateBody {
        #[serde(skip_serializing_if = "Option::is_none")]
        revocation_behavior: Option<RevocationBehavior>,
    }

    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &url,
        Some(&RotateBody {
            revocation_behavior,
        }),
    )
    .await?;
    deserialize_with_context(&text, "RotateSigningSecretResponse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhooks_url_construction() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        assert_eq!(
            webhooks_url(&ctx),
            "https://generativelanguage.googleapis.com/v1beta/webhooks"
        );
        assert_eq!(
            webhook_url(&ctx, "wh-123"),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/wh-123"
        );
        // A path-metacharacter ID is encoded, not interpolated raw (the
        // colon-verb suffixes below are appended outside webhook_url, so
        // they are unaffected by the encoding).
        assert_eq!(
            webhook_url(&ctx, "a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/a%2Fb%3Fc"
        );
        assert_eq!(
            format!("{}:ping", webhook_url(&ctx, "wh-123")),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/wh-123:ping"
        );
        assert_eq!(
            format!("{}:rotateSigningSecret", webhook_url(&ctx, "wh-123")),
            "https://generativelanguage.googleapis.com/v1beta/webhooks/wh-123:rotateSigningSecret"
        );
    }
}
