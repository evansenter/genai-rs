//! HTTP endpoints for the `/v1beta/triggers` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); see `http/webhooks.rs`.

use super::common::{API_KEY_HEADER, API_REVISION, API_REVISION_HEADER, BASE_URL_PREFIX};
use super::context::HttpContext;
use super::error_helpers::{check_response_wire, deserialize_with_context};
use crate::errors::GenaiError;
use crate::triggers::{
    Trigger, TriggerCreateParams, TriggerExecution, TriggerExecutionListResponse,
    TriggerListResponse, TriggerUpdate,
};
use crate::wire::WireEvent;

const API_VERSION: &str = "v1beta";

fn triggers_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/triggers")
}

fn trigger_url(id: &str) -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/triggers/{id}")
}

fn trigger_executions_url(trigger_id: &str) -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/triggers/{trigger_id}/executions")
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

/// Appends `page_size` / `page_token` query params to a URL.
fn with_paging(mut url: String, page_size: Option<u32>, page_token: Option<&str>) -> String {
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
    url
}

/// Creates a trigger (`POST /v1beta/triggers`).
pub async fn create_trigger(
    ctx: &HttpContext,
    params: &TriggerCreateParams,
) -> Result<Trigger, GenaiError> {
    tracing::debug!("Creating trigger: schedule={}", params.schedule);
    let text = send_and_read(ctx, "POST", &triggers_url(), Some(to_body(params)?)).await?;
    deserialize_with_context(&text, "Trigger from create")
}

/// Retrieves a trigger by ID (`GET /v1beta/triggers/{id}`).
pub async fn get_trigger(ctx: &HttpContext, trigger_id: &str) -> Result<Trigger, GenaiError> {
    tracing::debug!("Getting trigger: ID={trigger_id}");
    let text = send_and_read(ctx, "GET", &trigger_url(trigger_id), None).await?;
    deserialize_with_context(&text, "Trigger from get")
}

/// Lists triggers (`GET /v1beta/triggers`).
pub async fn list_triggers(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<TriggerListResponse, GenaiError> {
    tracing::debug!("Listing triggers: page_size={page_size:?}, page_token={page_token:?}");
    let url = with_paging(triggers_url(), page_size, page_token);
    let text = send_and_read(ctx, "GET", &url, None).await?;
    deserialize_with_context(&text, "TriggerListResponse")
}

/// Updates a trigger (`PATCH /v1beta/triggers/{id}`).
pub async fn update_trigger(
    ctx: &HttpContext,
    trigger_id: &str,
    update: &TriggerUpdate,
) -> Result<Trigger, GenaiError> {
    tracing::debug!("Updating trigger: ID={trigger_id}");
    let text = send_and_read(
        ctx,
        "PATCH",
        &trigger_url(trigger_id),
        Some(to_body(update)?),
    )
    .await?;
    deserialize_with_context(&text, "Trigger from update")
}

/// Deletes a trigger (`DELETE /v1beta/triggers/{id}`).
pub async fn delete_trigger(ctx: &HttpContext, trigger_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting trigger: ID={trigger_id}");
    send_and_read(ctx, "DELETE", &trigger_url(trigger_id), None).await?;
    Ok(())
}

/// Fires a trigger immediately (`POST /v1beta/triggers/{id}/executions`).
pub async fn run_trigger(
    ctx: &HttpContext,
    trigger_id: &str,
) -> Result<TriggerExecution, GenaiError> {
    tracing::debug!("Running trigger: ID={trigger_id}");
    let text = send_and_read(
        ctx,
        "POST",
        &trigger_executions_url(trigger_id),
        Some(serde_json::json!({})),
    )
    .await?;
    deserialize_with_context(&text, "TriggerExecution from run")
}

/// Lists a trigger's executions
/// (`GET /v1beta/triggers/{id}/executions`).
pub async fn list_trigger_executions(
    ctx: &HttpContext,
    trigger_id: &str,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<TriggerExecutionListResponse, GenaiError> {
    tracing::debug!("Listing trigger executions: ID={trigger_id}");
    let url = with_paging(trigger_executions_url(trigger_id), page_size, page_token);
    let text = send_and_read(ctx, "GET", &url, None).await?;
    deserialize_with_context(&text, "TriggerExecutionListResponse")
}
