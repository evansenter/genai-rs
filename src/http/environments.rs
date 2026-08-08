//! HTTP endpoints for the `/v1beta/environments` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); see `http/webhooks.rs`.

use super::common::{API_KEY_HEADER, API_REVISION, API_REVISION_HEADER, BASE_URL_PREFIX};
use super::context::HttpContext;
use super::error_helpers::{check_response_wire, deserialize_with_context};
use crate::environments::{CreateEnvironmentRequest, Environment, EnvironmentListResponse};
use crate::errors::GenaiError;
use crate::wire::WireEvent;

const API_VERSION: &str = "v1beta";

fn environments_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/environments")
}

fn environment_url(id: &str) -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/environments/{id}")
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

/// Creates an environment (`POST /v1beta/environments`).
pub async fn create_environment(
    ctx: &HttpContext,
    request: &CreateEnvironmentRequest,
) -> Result<Environment, GenaiError> {
    tracing::debug!("Creating environment");
    let body = serde_json::to_value(request)
        .map_err(|e| GenaiError::Internal(format!("Failed to serialize request body: {e}")))?;
    let text = send_and_read(ctx, "POST", &environments_url(), Some(body)).await?;
    deserialize_with_context(&text, "Environment from create")
}

/// Retrieves an environment by ID (`GET /v1beta/environments/{id}`).
pub async fn get_environment(
    ctx: &HttpContext,
    environment_id: &str,
) -> Result<Environment, GenaiError> {
    tracing::debug!("Getting environment: ID={environment_id}");
    let text = send_and_read(ctx, "GET", &environment_url(environment_id), None).await?;
    deserialize_with_context(&text, "Environment from get")
}

/// Lists environments (`GET /v1beta/environments`).
pub async fn list_environments(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<EnvironmentListResponse, GenaiError> {
    tracing::debug!("Listing environments: page_size={page_size:?}, page_token={page_token:?}");

    let mut url = environments_url();
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
    deserialize_with_context(&text, "EnvironmentListResponse")
}

/// Deletes an environment (`DELETE /v1beta/environments/{id}`).
pub async fn delete_environment(ctx: &HttpContext, environment_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting environment: ID={environment_id}");
    send_and_read(ctx, "DELETE", &environment_url(environment_id), None).await?;
    Ok(())
}
