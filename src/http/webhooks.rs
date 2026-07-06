//! HTTP endpoints for the `/v1beta/webhooks` resource.
//!
//! All requests send the same `Api-Revision` header as the Interactions API:
//! the webhooks resource is part of the revisioned Interactions surface
//! (the generated google-genai bindings apply the revision header globally).

use super::common::{API_KEY_HEADER, API_REVISION, API_REVISION_HEADER, BASE_URL_PREFIX};
use super::context::HttpContext;
use super::error_helpers::{check_response_wire, deserialize_with_context};
use crate::errors::GenaiError;
use crate::webhooks::{
    RevocationBehavior, RotateSigningSecretResponse, Webhook, WebhookListResponse, WebhookUpdate,
};
use crate::wire::WireEvent;

const API_VERSION: &str = "v1beta";

fn webhooks_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/webhooks")
}

fn webhook_url(id: &str) -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/webhooks/{id}")
}

/// Sends a request with the standard headers, emits wire events, checks the
/// status, and returns the response body text.
async fn send_and_read(
    ctx: &HttpContext,
    method: &str,
    url: &str,
    body: Option<serde_json::Value>,
) -> Result<String, GenaiError> {
    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, method, url, body.clone());

    let builder = match method {
        "GET" => ctx.http_client.get(url),
        "POST" => ctx.http_client.post(url),
        "PATCH" => ctx.http_client.patch(url),
        "DELETE" => ctx.http_client.delete(url),
        other => {
            return Err(GenaiError::Internal(format!(
                "Unsupported HTTP method: {other}"
            )));
        }
    };

    let mut builder = builder
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION);
    if let Some(body) = &body {
        builder = builder.json(body);
    }

    let response = builder.send().await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    let response = check_response_wire(response, ctx, request_id).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;
    ctx.emit_response_body(request_id, &response_text);
    Ok(response_text)
}

/// Serializes a body for the wire, mapping serialization errors to `Internal`.
fn to_body<B: serde::Serialize>(body: &B) -> Result<serde_json::Value, GenaiError> {
    serde_json::to_value(body)
        .map_err(|e| GenaiError::Internal(format!("Failed to serialize request body: {e}")))
}

/// Registers a new webhook (`POST /v1beta/webhooks`).
///
/// The response includes `new_signing_secret` — only returned on create.
pub async fn create_webhook(ctx: &HttpContext, webhook: &Webhook) -> Result<Webhook, GenaiError> {
    tracing::debug!("Creating webhook: uri={}", webhook.uri);
    let text = send_and_read(ctx, "POST", &webhooks_url(), Some(to_body(webhook)?)).await?;
    deserialize_with_context(&text, "Webhook from create")
}

/// Retrieves a webhook by ID (`GET /v1beta/webhooks/{id}`).
pub async fn get_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<Webhook, GenaiError> {
    tracing::debug!("Getting webhook: ID={webhook_id}");
    let text = send_and_read(ctx, "GET", &webhook_url(webhook_id), None).await?;
    deserialize_with_context(&text, "Webhook from get")
}

/// Lists webhooks (`GET /v1beta/webhooks`).
pub async fn list_webhooks(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<WebhookListResponse, GenaiError> {
    tracing::debug!("Listing webhooks: page_size={page_size:?}, page_token={page_token:?}");

    let mut url = webhooks_url();
    let mut params = Vec::new();
    if let Some(size) = page_size {
        params.push(format!("page_size={size}"));
    }
    if let Some(token) = page_token {
        params.push(format!("page_token={}", urlencoding::encode(token)));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let text = send_and_read(ctx, "GET", &url, None).await?;
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

    let mut url = webhook_url(webhook_id);
    if let Some(mask) = update_mask {
        url.push_str(&format!("?update_mask={}", urlencoding::encode(mask)));
    }

    let text = send_and_read(ctx, "PATCH", &url, Some(to_body(update)?)).await?;
    deserialize_with_context(&text, "Webhook from update")
}

/// Deletes a webhook (`DELETE /v1beta/webhooks/{id}`).
pub async fn delete_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting webhook: ID={webhook_id}");
    send_and_read(ctx, "DELETE", &webhook_url(webhook_id), None).await?;
    Ok(())
}

/// Sends a test event to a webhook (`POST /v1beta/webhooks/{id}:ping`).
pub async fn ping_webhook(ctx: &HttpContext, webhook_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Pinging webhook: ID={webhook_id}");
    let url = format!("{}:ping", webhook_url(webhook_id));
    // Request and response bodies are empty per the spec.
    send_and_read(ctx, "POST", &url, Some(serde_json::json!({}))).await?;
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
        body.insert(
            "revocation_behavior".to_string(),
            serde_json::to_value(behavior)
                .map_err(|e| GenaiError::Internal(format!("Failed to serialize body: {e}")))?,
        );
    }

    let text = send_and_read(ctx, "POST", &url, Some(serde_json::Value::Object(body))).await?;
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
