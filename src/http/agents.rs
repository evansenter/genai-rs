//! HTTP endpoints for the `/v1beta/agents` resource.
//!
//! All requests send the same `Api-Revision` header as the Interactions API:
//! the agents resource is part of the revisioned Interactions surface
//! (the generated google-genai bindings apply the revision header globally).

use super::common::{API_KEY_HEADER, API_REVISION, API_REVISION_HEADER, BASE_URL_PREFIX};
use super::context::HttpContext;
use super::error_helpers::{check_response_wire, deserialize_with_context};
use crate::agents::{Agent, AgentListResponse};
use crate::errors::GenaiError;
use crate::wire::WireEvent;

const API_VERSION: &str = "v1beta";

fn agents_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/agents")
}

fn agent_url(id: &str) -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/agents/{id}")
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

/// Creates an agent (`POST /v1beta/agents`).
pub async fn create_agent(ctx: &HttpContext, agent: &Agent) -> Result<Agent, GenaiError> {
    tracing::debug!("Creating agent: id={:?}", agent.id);
    let body = serde_json::to_value(agent)
        .map_err(|e| GenaiError::Internal(format!("Failed to serialize agent: {e}")))?;
    let text = send_and_read(ctx, "POST", &agents_url(), Some(body)).await?;
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

    let mut url = agents_url();
    let mut params = Vec::new();
    if let Some(size) = page_size {
        params.push(format!("page_size={size}"));
    }
    if let Some(token) = page_token {
        params.push(format!("page_token={}", urlencoding::encode(token)));
    }
    if let Some(parent) = parent {
        params.push(format!("parent={}", urlencoding::encode(parent)));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

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
