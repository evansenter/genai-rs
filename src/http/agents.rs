//! HTTP endpoints for the `/v1beta/agents` resource.
//!
//! All requests send the same `Api-Revision` header as the Interactions API:
//! the agents resource is part of the revisioned Interactions surface
//! (the generated google-genai bindings apply the revision header globally).

use super::common::{BASE_URL_PREFIX, path_segment, send_and_read, to_body, with_paging_and};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::agents::{Agent, AgentListResponse};
use crate::errors::GenaiError;

const API_VERSION: &str = "v1beta";

fn agents_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/agents")
}

fn agent_url(id: &str) -> String {
    format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/agents/{}",
        path_segment(id)
    )
}

/// Creates an agent (`POST /v1beta/agents`).
pub async fn create_agent(ctx: &HttpContext, agent: &Agent) -> Result<Agent, GenaiError> {
    tracing::debug!("Creating agent: id={:?}", agent.id);
    let text = send_and_read(ctx, "POST", &agents_url(), Some(to_body(agent)?)).await?;
    deserialize_with_context(&text, "Agent from create")
}

/// Retrieves an agent by ID (`GET /v1beta/agents/{id}`).
pub async fn get_agent(ctx: &HttpContext, agent_id: &str) -> Result<Agent, GenaiError> {
    tracing::debug!("Getting agent: ID={agent_id}");
    let text = send_and_read(ctx, "GET", &agent_url(agent_id), None).await?;
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

    let extra: Vec<(&str, &str)> = parent.map(|p| ("parent", p)).into_iter().collect();
    let url = with_paging_and(agents_url(), page_size, page_token, &extra);

    let text = send_and_read(ctx, "GET", &url, None).await?;
    deserialize_with_context(&text, "AgentListResponse")
}

/// Deletes an agent (`DELETE /v1beta/agents/{id}`).
pub async fn delete_agent(ctx: &HttpContext, agent_id: &str) -> Result<(), GenaiError> {
    tracing::debug!("Deleting agent: ID={agent_id}");
    send_and_read(ctx, "DELETE", &agent_url(agent_id), None).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agents_url_construction() {
        assert_eq!(
            agents_url(),
            "https://generativelanguage.googleapis.com/v1beta/agents"
        );
        assert_eq!(
            agent_url("my-agent"),
            "https://generativelanguage.googleapis.com/v1beta/agents/my-agent"
        );
    }
}
