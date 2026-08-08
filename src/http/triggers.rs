//! HTTP endpoints for the `/v1beta/triggers` resource.
//!
//! Same header conventions as the other Interactions API resources
//! (API key + `Api-Revision`); shared plumbing lives in `http/common.rs`.

use super::common::{BASE_URL_PREFIX, send_and_read, with_paging};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::triggers::{
    Trigger, TriggerCreateParams, TriggerExecution, TriggerExecutionListResponse,
    TriggerListResponse, TriggerUpdate,
};

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

/// Serializes a body for the wire, mapping serialization errors to `Internal`.
fn to_body<B: serde::Serialize>(body: &B) -> Result<serde_json::Value, GenaiError> {
    serde_json::to_value(body)
        .map_err(|e| GenaiError::Internal(format!("Failed to serialize request body: {e}")))
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
