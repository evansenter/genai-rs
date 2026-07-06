//! Agent resource types for the `/v1beta/agents` API.
//!
//! Custom agents package a base agent, system instruction, tools, and a base
//! environment under a reusable ID. Once created, run them with
//! [`InteractionBuilder::with_agent()`](crate::InteractionBuilder::with_agent).
//!
//! Manage agents with the [`Client`](crate::Client) methods `create_agent`,
//! `get_agent`, `list_agents`, and `delete_agent`.
//!
//! See `docs/AGENTS_AND_BACKGROUND.md` for the full agents flow and the list
//! of managed agent IDs.

use serde::{Deserialize, Serialize};

use crate::environment::EnvironmentSpec;
use crate::tools::Tool;

/// An agent definition for the `/v1beta/agents` resource.
///
/// Per the API spec, agent `tools` support a subset of the tool union:
/// `code_execution`, `url_context`, `google_search`, and `mcp_server`.
/// Other tool types are rejected by the API.
///
/// # Example
///
/// ```
/// use genai_rs::{Agent, EnvironmentSource, RemoteEnvironment, Tool};
///
/// let agent = Agent::new("customer-sentinel")
///     .with_system_instruction("You monitor customer feedback.")
///     .with_description("Watches feedback channels and summarizes sentiment")
///     .add_tool(Tool::CodeExecution)
///     .with_base_environment(
///         RemoteEnvironment::new()
///             .add_source(EnvironmentSource::gcs("gs://feedback", "/data")),
///     );
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Agent {
    /// The unique identifier for the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The base agent to extend.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_agent: Option<String>,
    /// System instruction for the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,
    /// Agent description for developers to quickly read and understand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The tools available to the agent (subset: `code_execution`,
    /// `url_context`, `google_search`, `mcp_server`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// The environment configuration for the agent — a string environment ID
    /// or a typed remote environment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_environment: Option<EnvironmentSpec>,
}

impl Agent {
    /// Creates a new agent definition with the given ID.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            ..Default::default()
        }
    }

    /// Sets the base agent to extend.
    #[must_use]
    pub fn with_base_agent(mut self, base_agent: impl Into<String>) -> Self {
        self.base_agent = Some(base_agent.into());
        self
    }

    /// Sets the system instruction for the agent.
    #[must_use]
    pub fn with_system_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.system_instruction = Some(instruction.into());
        self
    }

    /// Sets the developer-facing description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a tool to the agent's tool list.
    ///
    /// The API accepts the subset `code_execution`, `url_context`,
    /// `google_search`, and `mcp_server` for agents.
    #[must_use]
    pub fn add_tool(mut self, tool: impl Into<Tool>) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool.into());
        self
    }

    /// Sets the base environment (string ID or typed remote environment).
    #[must_use]
    pub fn with_base_environment(mut self, environment: impl Into<EnvironmentSpec>) -> Self {
        self.base_environment = Some(environment.into());
        self
    }
}

/// Response for `GET /v1beta/agents` (list).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentListResponse {
    /// The agents on this page.
    pub agents: Vec<Agent>,
    /// Token for the next page. Absent when there are no more pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{EnvironmentSource, RemoteEnvironment};
    use serde_json::json;

    #[test]
    fn test_agent_serialization_matches_spec_shape() {
        // New format documented in the generated bindings:
        // {"id": ..., "base_agent": ..., "system_instruction": ...,
        //  "base_environment": {"type": "remote", "sources": [...]},
        //  "tools": [{"type": "code_execution"}]}
        let agent = Agent::new("customer-sentinel")
            .with_base_agent("")
            .with_system_instruction("You monitor customer feedback.")
            .add_tool(Tool::CodeExecution)
            .with_base_environment(
                RemoteEnvironment::new()
                    .add_source(EnvironmentSource::gcs("gs://feedback", "/data")),
            );

        let value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["id"], "customer-sentinel");
        assert_eq!(value["base_agent"], "");
        assert_eq!(
            value["system_instruction"],
            "You monitor customer feedback."
        );
        assert_eq!(value["tools"][0]["type"], "code_execution");
        assert_eq!(value["base_environment"]["type"], "remote");
        assert_eq!(
            value["base_environment"]["sources"][0]["source"],
            "gs://feedback"
        );
        assert!(value.get("description").is_none());
    }

    #[test]
    fn test_agent_base_environment_string_id() {
        let agent = Agent::new("my-agent").with_base_environment("environments/env-42");
        let value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["base_environment"], "environments/env-42");
    }

    #[test]
    fn test_agent_roundtrip() {
        let json = json!({
            "id": "helper",
            "base_agent": "gemini-base",
            "system_instruction": "Help users.",
            "description": "A helper agent",
            "tools": [
                {"type": "google_search"},
                {"type": "mcp_server", "name": "fs", "url": "https://mcp.example.com/fs"}
            ],
            "base_environment": "environments/env-1"
        });

        let agent: Agent = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(agent.id.as_deref(), Some("helper"));
        assert_eq!(agent.tools.as_ref().unwrap().len(), 2);
        assert!(matches!(
            agent.base_environment,
            Some(EnvironmentSpec::Id(ref id)) if id == "environments/env-1"
        ));

        let back = serde_json::to_value(&agent).unwrap();
        assert_eq!(back, json);
    }

    #[test]
    fn test_agent_unknown_tool_preserved() {
        // Evergreen: unknown tool types on an agent are preserved
        let json = json!({
            "id": "future",
            "tools": [{"type": "quantum_solver", "qubits": 128}]
        });
        let agent: Agent = serde_json::from_value(json.clone()).unwrap();
        let tools = agent.tools.as_ref().unwrap();
        assert!(tools[0].is_unknown());
        assert_eq!(tools[0].unknown_tool_type(), Some("quantum_solver"));
        assert_eq!(serde_json::to_value(&agent).unwrap(), json);
    }

    #[test]
    fn test_agent_list_response_deserialization() {
        let json = json!({
            "agents": [{"id": "a1"}, {"id": "a2"}],
            "next_page_token": "tok"
        });
        let list: AgentListResponse = serde_json::from_value(json).unwrap();
        assert_eq!(list.agents.len(), 2);
        assert_eq!(list.next_page_token.as_deref(), Some("tok"));

        let empty: AgentListResponse = serde_json::from_str("{}").unwrap();
        assert!(empty.agents.is_empty());
        assert!(empty.next_page_token.is_none());
    }
}
