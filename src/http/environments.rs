//! HTTP endpoints for the `/v1beta/environments` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); shared plumbing lives in `http/common.rs`.

use super::common::{NO_BODY, path_segment, require_id, send_and_read, with_paging};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::environments::{CreateEnvironmentRequest, Environment, EnvironmentListResponse};
use crate::errors::GenaiError;

fn environments_url(ctx: &HttpContext) -> String {
    ctx.api_url("environments")
}

fn environment_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("environments/{}", path_segment(id)))
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
        &environments_url(ctx),
        Some(request),
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
        &environment_url(ctx, environment_id),
        NO_BODY,
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
    let url = with_paging(environments_url(ctx), page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "EnvironmentListResponse")
}

/// Deletes an environment (`DELETE /v1beta/environments/{id}`).
pub async fn delete_environment(ctx: &HttpContext, environment_id: &str) -> Result<(), GenaiError> {
    require_id(environment_id, "environment")?;
    tracing::debug!("Deleting environment: ID={environment_id}");
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &environment_url(ctx, environment_id),
        NO_BODY,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environments_url_construction() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        assert_eq!(
            environments_url(&ctx),
            "https://generativelanguage.googleapis.com/v1beta/environments"
        );
        assert_eq!(
            environment_url(&ctx, "env-123"),
            "https://generativelanguage.googleapis.com/v1beta/environments/env-123"
        );
        // A path-metacharacter ID is encoded, not interpolated raw.
        assert_eq!(
            environment_url(&ctx, "a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/environments/a%2Fb%3Fc"
        );
    }
}
