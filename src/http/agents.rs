//! HTTP endpoints for the `/v1beta/agents` resource.
//!
//! All requests send the same `Api-Revision` header as the Interactions API:
//! the agents resource is part of the revisioned Interactions surface
//! (the generated google-genai bindings apply the revision header globally).

use super::common::{NO_BODY, path_segment, require_id, send_and_read, with_paging, with_query};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::agents::{Agent, AgentListResponse};
use crate::errors::GenaiError;

fn agents_url(ctx: &HttpContext) -> String {
    ctx.api_url("agents")
}

fn agent_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("agents/{}", path_segment(id)))
}

/// Creates an agent (`POST /v1beta/agents`).
pub async fn create_agent(ctx: &HttpContext, agent: &Agent) -> Result<Agent, GenaiError> {
    tracing::debug!("Creating agent: id={:?}", agent.id);
    let text = send_and_read(ctx, reqwest::Method::POST, &agents_url(ctx), Some(agent)).await?;
    deserialize_with_context(&text, "Agent from create")
}

/// Retrieves an agent by ID (`GET /v1beta/agents/{id}`).
pub async fn get_agent(ctx: &HttpContext, agent_id: &str) -> Result<Agent, GenaiError> {
    require_id(agent_id, "agent")?;
    tracing::debug!("Getting agent: ID={agent_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::GET,
        &agent_url(ctx, agent_id),
        NO_BODY,
    )
    .await?;
    deserialize_with_context(&text, "Agent from get")
}

/// Lists agents (`GET /v1beta/agents`).
pub async fn list_agents(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
    parent: Option<&str>,
) -> Result<AgentListResponse, GenaiError> {
    tracing::debug!(
        "Listing agents: page_size={page_size:?}, page_token={page_token:?}, parent={parent:?}"
    );

    let url = with_query(
        with_paging(agents_url(ctx), page_size, page_token),
        &[("parent", parent)],
    );
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "AgentListResponse")
}

/// Deletes an agent (`DELETE /v1beta/agents/{id}`).
pub async fn delete_agent(ctx: &HttpContext, agent_id: &str) -> Result<(), GenaiError> {
    require_id(agent_id, "agent")?;
    tracing::debug!("Deleting agent: ID={agent_id}");
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &agent_url(ctx, agent_id),
        NO_BODY,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agents_url_construction() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        assert_eq!(
            agents_url(&ctx),
            "https://generativelanguage.googleapis.com/v1beta/agents"
        );
        assert_eq!(
            agent_url(&ctx, "my-agent"),
            "https://generativelanguage.googleapis.com/v1beta/agents/my-agent"
        );
        // A path-metacharacter ID is encoded, not interpolated raw.
        assert_eq!(
            agent_url(&ctx, "a/b?c"),
            "https://generativelanguage.googleapis.com/v1beta/agents/a%2Fb%3Fc"
        );
    }
}
