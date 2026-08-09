//! HTTP endpoints for the `/v1beta/triggers` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); shared plumbing lives in `http/common.rs`.

use super::common::{
    API_VERSION, BASE_URL_PREFIX, path_segment, send_and_read, to_body, with_paging,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::triggers::{
    Trigger, TriggerCreateParams, TriggerExecution, TriggerExecutionListResponse,
    TriggerListResponse, TriggerUpdate,
};

fn triggers_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/triggers")
}

fn trigger_url(id: &str) -> String {
    format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/triggers/{}",
        path_segment(id)
    )
}

fn trigger_executions_url(trigger_id: &str) -> String {
    format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/triggers/{}/executions",
        path_segment(trigger_id)
    )
}

/// Creates a trigger (`POST /v1beta/triggers`).
pub async fn create_trigger(
    ctx: &HttpContext,
    params: &TriggerCreateParams,
) -> Result<Trigger, GenaiError> {
    tracing::debug!("Creating trigger: schedule={}", params.schedule);
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &triggers_url(),
        Some(to_body(params)?),
    )
    .await?;
    deserialize_with_context(&text, "Trigger from create")
}

/// Retrieves a trigger by ID (`GET /v1beta/triggers/{id}`).
pub async fn get_trigger(ctx: &HttpContext, trigger_id: &str) -> Result<Trigger, GenaiError> {
    tracing::debug!("Getting trigger: ID={trigger_id}");
    let text = send_and_read(ctx, reqwest::Method::GET, &trigger_url(trigger_id), None).await?;
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
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "TriggerListResponse")
}

/// Updates a trigger (`PATCH /v1beta/triggers/{id}`).
///
/// No `update_mask` query parameter: the SDK spec doesn't define one for
/// triggers (unlike webhooks), so unset-field omission in the body is the
/// only update-scoping mechanism. See [`TriggerUpdate`] for the caveat.
pub async fn update_trigger(
    ctx: &HttpContext,
    trigger_id: &str,
    update: &TriggerUpdate,
) -> Result<Trigger, GenaiError> {
    tracing::debug!("Updating trigger: ID={trigger_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::PATCH,
        &trigger_url(trigger_id),
        Some(to_body(update)?),
    )
    .await?;
    deserialize_with_context(&text, "Trigger from update")
}

/// Deletes a trigger (`DELETE /v1beta/triggers/{id}`).
pub async fn delete_trigger(ctx: &HttpContext, trigger_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting trigger: ID={trigger_id}");
    send_and_read(ctx, reqwest::Method::DELETE, &trigger_url(trigger_id), None).await?;
    Ok(())
}

/// Fires a trigger immediately (`POST /v1beta/triggers/{id}/executions`).
///
/// Note the path shape: unlike the colon custom-method verbs on webhooks
/// (`:ping`, `:rotateSigningSecret`), run-now is spec'd as a POST to the
/// `executions` sub-collection — verified against the google-genai 2.17.0
/// generated bindings (`path="/{api_version}/triggers/{trigger_id}/executions"`).
/// Not live-verifiable here: reaching it requires an existing trigger, and
/// trigger creation is agent-gated on standard keys.
pub async fn run_trigger(
    ctx: &HttpContext,
    trigger_id: &str,
) -> Result<TriggerExecution, GenaiError> {
    tracing::debug!("Running trigger: ID={trigger_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
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
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "TriggerExecutionListResponse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_triggers_url_construction() {
        assert_eq!(
            triggers_url(),
            "https://generativelanguage.googleapis.com/v1beta/triggers"
        );
        assert_eq!(
            trigger_url("trig-123"),
            "https://generativelanguage.googleapis.com/v1beta/triggers/trig-123"
        );
        // The sub-collection form the SDK spec mandates for run/list
        // executions (see the run_trigger doc comment) — this path has no
        // live probe, so the unit test is its only coverage.
        assert_eq!(
            trigger_executions_url("trig-123"),
            "https://generativelanguage.googleapis.com/v1beta/triggers/trig-123/executions"
        );
        // A path-metacharacter ID is encoded, not interpolated raw — on
        // both the item URL and the sub-collection URL.
        assert_eq!(
            trigger_url("a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/triggers/a%2Fb%3Fc"
        );
        assert_eq!(
            trigger_executions_url("a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/triggers/a%2Fb%3Fc/executions"
        );
    }
}
