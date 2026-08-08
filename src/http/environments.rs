//! HTTP endpoints for the `/v1beta/environments` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); shared plumbing lives in `http/common.rs`.

use super::common::{BASE_URL_PREFIX, send_and_read, with_paging};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::environments::{CreateEnvironmentRequest, Environment, EnvironmentListResponse};
use crate::errors::GenaiError;

const API_VERSION: &str = "v1beta";

fn environments_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/environments")
}

fn environment_url(id: &str) -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/environments/{id}")
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
    let url = with_paging(environments_url(), page_size, page_token);
    let text = send_and_read(ctx, "GET", &url, None).await?;
    deserialize_with_context(&text, "EnvironmentListResponse")
}

/// Deletes an environment (`DELETE /v1beta/environments/{id}`).
pub async fn delete_environment(ctx: &HttpContext, environment_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting environment: ID={environment_id}");
    send_and_read(ctx, "DELETE", &environment_url(environment_id), None).await?;
    Ok(())
}
