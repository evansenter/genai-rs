//! HTTP endpoint for the `/v1beta/decisions` resource.

use super::common::send_and_read;
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::decisions::{CreateDecisionRequest, CreateDecisionResponse};
use crate::errors::GenaiError;

/// Evaluates decision questions (`POST /v1beta/decisions`).
pub async fn create_decision(
    ctx: &HttpContext,
    request: &CreateDecisionRequest,
) -> Result<CreateDecisionResponse, GenaiError> {
    tracing::debug!(
        "Creating decision: model={}, questions={}",
        request.model,
        request.questions.len()
    );
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &ctx.api_url("decisions"),
        Some(request),
    )
    .await?;
    deserialize_with_context(&text, "CreateDecisionResponse")
}
