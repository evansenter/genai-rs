//! HTTP endpoints for the `/v1beta/credentials` resource.

use super::common::{NO_BODY, path_segment, require_id, send_and_read, with_paging, with_query};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::credentials::{
    CreateCredentialRequest, Credential, CredentialListResponse, CredentialUpdate,
};
use crate::errors::GenaiError;

fn credentials_url(ctx: &HttpContext) -> String {
    ctx.api_url("credentials")
}

fn credential_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("credentials/{}", path_segment(id)))
}

/// Creates a credential (`POST /v1beta/credentials`).
pub async fn create_credential(
    ctx: &HttpContext,
    request: &CreateCredentialRequest,
) -> Result<Credential, GenaiError> {
    // Never log the request: it carries the secret.
    tracing::debug!("Creating credential: id={:?}", request.id);
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &credentials_url(ctx),
        Some(request),
    )
    .await?;
    deserialize_with_context(&text, "Credential from create")
}

/// Retrieves a credential (`GET /v1beta/credentials/{id}`).
pub async fn get_credential(
    ctx: &HttpContext,
    credential_id: &str,
) -> Result<Credential, GenaiError> {
    require_id(credential_id, "credential")?;
    tracing::debug!("Getting credential: ID={credential_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::GET,
        &credential_url(ctx, credential_id),
        NO_BODY,
    )
    .await?;
    deserialize_with_context(&text, "Credential from get")
}

/// Lists credentials (`GET /v1beta/credentials`).
pub async fn list_credentials(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<CredentialListResponse, GenaiError> {
    tracing::debug!("Listing credentials: page_size={page_size:?}, page_token={page_token:?}");
    let url = with_paging(credentials_url(ctx), page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "CredentialListResponse")
}

/// Updates a credential (`PATCH /v1beta/credentials/{id}`).
pub async fn update_credential(
    ctx: &HttpContext,
    credential_id: &str,
    update: &CredentialUpdate,
    update_mask: Option<&str>,
) -> Result<Credential, GenaiError> {
    require_id(credential_id, "credential")?;
    tracing::debug!("Updating credential: ID={credential_id}, update_mask={update_mask:?}");
    let url = with_query(
        credential_url(ctx, credential_id),
        &[("update_mask", update_mask)],
    );
    let text = send_and_read(ctx, reqwest::Method::PATCH, &url, Some(update)).await?;
    deserialize_with_context(&text, "Credential from update")
}

/// Deletes a credential (`DELETE /v1beta/credentials/{id}`).
pub async fn delete_credential(ctx: &HttpContext, credential_id: &str) -> Result<(), GenaiError> {
    require_id(credential_id, "credential")?;
    tracing::debug!("Deleting credential: ID={credential_id}");
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &credential_url(ctx, credential_id),
        NO_BODY,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_urls() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        assert_eq!(
            credentials_url(&ctx),
            "https://generativelanguage.googleapis.com/v1beta/credentials"
        );
        assert_eq!(
            credential_url(&ctx, "gh-token"),
            "https://generativelanguage.googleapis.com/v1beta/credentials/gh-token"
        );
        assert_eq!(
            credential_url(&ctx, "a?b"),
            "https://generativelanguage.googleapis.com/v1beta/credentials/a%3Fb"
        );
    }
}
