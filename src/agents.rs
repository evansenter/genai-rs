//! Agent resource types for the `/v1beta/agents` API.
//!
//! Custom agents package a base agent, system instruction, tools, and a base
//! environment under a reusable ID. Once created, run them with
//! [`InteractionBuilder::with_agent()`](crate::InteractionBuilder::with_agent).
//!
//! Manage agents with the [`Client`] methods `create_agent`,
//! `get_agent`, `list_agents`, and `delete_agent`.
//!
//! See `docs/AGENTS_AND_BACKGROUND.md` for the full agents flow and the list
//! of managed agent IDs.
//!
//! # IDs
//!
//! Methods take the bare ID ([`Agent::id`]), not a `agents/...` resource name:
//! the ID is percent-encoded into a single path segment, so a resource name
//! addresses nothing and 404s. An empty or dot-segment ID fails
//! locally with [`GenaiError::InvalidInput`]
//! before any request.

use crate::client::Client;
use crate::errors::GenaiError;
use serde::{Deserialize, Serialize};

use crate::environments::EnvironmentSpec;
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
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
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
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen).
    ///
    /// Without this, a deserialize-then-serialize cycle silently drops any
    /// field the crate has not modeled yet — invisible to the caller, and
    /// unrecoverable. Agent creation is gated on a standard API key, so only the
    /// GET/LIST shapes have been observed.
    ///
    /// **Also read on serialize into a request body.** `create_agent` sends
    /// this whole struct, so `extra` is an *outbound* escape hatch too —
    /// a way to send a field the crate has not modeled yet, exactly like
    /// [`CreateEnvironmentRequest::extra`](crate::CreateEnvironmentRequest::extra).
    /// It also means a get-modify-create cycle echoes unmodeled server
    /// fields back.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value`, matching the request-side escape hatches.
    /// (`to_string` on a flattened struct emits both keys rather than
    /// deduplicating; don't hand-serialize colliding keys.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct AgentListResponse {
    /// The agents on this page. A null or malformed list degrades to
    /// empty; malformed elements drop individually.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub agents: Vec<Agent>,
    /// Token for the next page. Absent when there are no more pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Agents resource methods; see [IDs](crate::agents#ids).
impl Client {
    /// Creates a custom agent.
    ///
    /// Once created, run the agent with
    /// [`InteractionBuilder::with_agent()`](crate::InteractionBuilder::with_agent)
    /// using its ID.
    ///
    /// Live behavior notes (2026-07):
    /// - Agent creation was rejected with a generic
    ///   `400 "Request contains an invalid argument."` for every payload
    ///   tried on a standard Gemini API key (even schema-valid ones), which
    ///   suggests the resource is allowlisted/gated. Field names are still
    ///   validated first (snake_case: `id`, `base_agent`,
    ///   `system_instruction`, `description`, `tools`, `base_environment`).
    /// - `tools` on an agent only accepts `code_execution`, `google_search`,
    ///   and `url_context` (per the API's own validation error).
    /// - Managed agent IDs (e.g. `deep-research-preview-04-2026`) are not
    ///   retrievable through `GET /v1beta/agents/{id}` (404).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the API returns an error,
    /// or response parsing fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Agent, Client, Tool};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let agent = client.create_agent(
    ///     &Agent::new("customer-sentinel")
    ///         .with_system_instruction("You monitor customer feedback.")
    ///         .add_tool(Tool::CodeExecution),
    /// ).await?;
    ///
    /// // Run it
    /// let response = client.interaction()
    ///     .with_agent(agent.id.as_deref().unwrap_or("customer-sentinel"))
    ///     .with_text("Summarize this week's feedback")
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_agent(&self, agent: &crate::Agent) -> Result<crate::Agent, GenaiError> {
        crate::http::agents::create_agent(&self.http, agent).await
    }

    /// Retrieves an agent by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent doesn't exist, the HTTP request fails,
    /// or response parsing fails.
    pub async fn get_agent(&self, agent_id: &str) -> Result<crate::Agent, GenaiError> {
        crate::http::agents::get_agent(&self.http, agent_id).await
    }

    /// Lists agents.
    ///
    /// # Arguments
    ///
    /// * `page_size` - Optional maximum number of agents per page.
    /// * `page_token` - Optional token from a previous list call.
    /// * `parent` - Optional parent resource filter.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn list_agents(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
        parent: Option<&str>,
    ) -> Result<crate::AgentListResponse, GenaiError> {
        crate::http::agents::list_agents(&self.http, page_size, page_token, parent).await
    }

    /// Deletes an agent by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent doesn't exist or the HTTP request fails.
    pub async fn delete_agent(&self, agent_id: &str) -> Result<(), GenaiError> {
        crate::http::agents::delete_agent(&self.http, agent_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environments::{EnvironmentSource, RemoteEnvironment};
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
        let agent = Agent::new("my-agent").with_base_environment("env42bare0");
        let value = serde_json::to_value(&agent).unwrap();
        assert_eq!(value["base_environment"], "env42bare0");
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
            "base_environment": "env1bare0"
        });

        let agent: Agent = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(agent.id.as_deref(), Some("helper"));
        assert_eq!(agent.tools.as_ref().unwrap().len(), 2);
        assert!(matches!(
            agent.base_environment,
            Some(EnvironmentSpec::Id(ref id)) if id == "env1bare0"
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

        // Present-but-degenerate list keys degrade like the trigger and
        // environment envelopes, rather than erroring the page.
        let null: AgentListResponse = serde_json::from_value(json!({"agents": null})).unwrap();
        assert!(null.agents.is_empty());
        let bad: AgentListResponse =
            serde_json::from_value(json!({"agents": "corrupted"})).unwrap();
        assert!(bad.agents.is_empty());

        // The element-drop arm, pinned on the concrete type like its
        // webhooks counterpart: a wrong-typed modeled field or a
        // non-object element drops alone; the good siblings survive.
        let partial: AgentListResponse = serde_json::from_value(
            json!({"agents": [{"id": "a1"}, {"id": 42}, "not-an-object", {"id": "a2"}]}),
        )
        .unwrap();
        assert_eq!(partial.agents.len(), 2);
        assert_eq!(partial.agents[0].id.as_deref(), Some("a1"));
        assert_eq!(partial.agents[1].id.as_deref(), Some("a2"));
    }

    // --- Evergreen `extra` passthrough on response shapes (#406) ---

    #[test]
    fn agent_preserves_unknown_response_fields() {
        // Agent creation is API-key gated, so only GET/LIST shapes have been
        // observed — an unmodeled field is entirely plausible here.
        let wire = serde_json::json!({
            "id": "agent_1",
            "description": "test",
            "future_capability": ["a", "b"]
        });

        let agent: Agent = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(
            agent.extra.get("future_capability"),
            Some(&serde_json::json!(["a", "b"]))
        );
        assert_eq!(serde_json::to_value(&agent).unwrap(), wire);
    }

    #[test]
    fn agent_without_unknown_fields_has_empty_extra() {
        let agent: Agent = serde_json::from_value(serde_json::json!({"id": "agent_1"})).unwrap();
        assert!(agent.extra.is_empty());
        assert_eq!(
            serde_json::to_value(&agent).unwrap(),
            serde_json::json!({"id": "agent_1"})
        );
    }

    #[test]
    fn agent_extra_survives_a_list_response() {
        // The flatten must work through the list envelope too, not just on a
        // directly-deserialized agent.
        let wire = serde_json::json!({"agents": [{"id": "a1", "future": 1}]});
        let list: AgentListResponse = serde_json::from_value(wire).unwrap();
        assert_eq!(
            list.agents[0].extra.get("future"),
            Some(&serde_json::json!(1))
        );
    }
}
