//! HTTP endpoints for the `/v1beta/environments` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); shared plumbing lives in `http/common.rs`.

use super::common::{
    API_VERSION, BASE_URL_PREFIX, path_segment, require_id, send_and_read, to_body, with_paging,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::environments::{CreateEnvironmentRequest, Environment, EnvironmentListResponse};
use crate::errors::GenaiError;

fn environments_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/environments")
}

fn environment_url(id: &str) -> String {
    format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/environments/{}",
        path_segment(id)
    )
}

/// Creates an environment (`POST /v1beta/environments`).
pub async fn create_environment(
    ctx: &HttpContext,
    request: &CreateEnvironmentRequest,
) -> Result<Environment, GenaiError> {
    tracing::debug!("Creating environment");
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &environments_url(),
        Some(to_body(request)?),
    )
    .await?;
    deserialize_with_context(&text, "Environment from create")
}

/// Retrieves an environment by ID (`GET /v1beta/environments/{id}`).
pub async fn get_environment(
    ctx: &HttpContext,
    environment_id: &str,
) -> Result<Environment, GenaiError> {
    require_id(environment_id, "environment")?;
    tracing::debug!("Getting environment: ID={environment_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::GET,
        &environment_url(environment_id),
        None,
    )
    .await?;
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
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "EnvironmentListResponse")
}

/// Deletes an environment (`DELETE /v1beta/environments/{id}`).
pub async fn delete_environment(ctx: &HttpContext, environment_id: &str) -> Result<(), GenaiError> {
    require_id(environment_id, "environment")?;
    tracing::debug!("Deleting environment: ID={environment_id}");
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &environment_url(environment_id),
        None,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environments_url_construction() {
        assert_eq!(
            environments_url(),
            "https://generativelanguage.googleapis.com/v1beta/environments"
        );
        assert_eq!(
            environment_url("env-123"),
            "https://generativelanguage.googleapis.com/v1beta/environments/env-123"
        );
        // A path-metacharacter ID is encoded, not interpolated raw.
        assert_eq!(
            environment_url("a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/environments/a%2Fb%3Fc"
        );
    }
}
