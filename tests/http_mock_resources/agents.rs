//! Agents: every endpoint's request and the responses.

use super::*;

#[tokio::test]
async fn agent_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/agents",
                Some(json!({
                    "id": "my-agent",
                    "base_agent": "agents/base",
                    "system_instruction": "Be brief.",
                    "description": "A test agent",
                    "tools": [{"type": "code_execution"}],
                    "base_environment": "env-1"
                })),
                async move {
                    let agent = Agent::new("my-agent", "agents/base")
                        .with_system_instruction("Be brief.")
                        .with_description("A test agent")
                        .add_tool(Tool::code_execution())
                        .with_base_environment("env-1");
                    c.agents().create(&agent).await
                },
            ),
            wire(
                "GET",
                "/v1beta/agents/my-agent",
                None,
                c.agents().get("my-agent"),
            ),
            wire("GET", "/v1beta/agents", None, c.agents().list().send()),
            wire(
                "GET",
                "/v1beta/agents?page_size=5&page_token=t&parent=projects%2Fp%201",
                None,
                c.agents()
                    .list()
                    .with_page_size(5)
                    .with_page_token("t")
                    .with_parent("projects/p 1")
                    .send(),
            ),
            // With no paging, the filter takes the leading `?`.
            wire(
                "GET",
                "/v1beta/agents?parent=p",
                None,
                c.agents().list().with_parent("p").send(),
            ),
            wire(
                "DELETE",
                "/v1beta/agents/my-agent",
                None,
                c.agents().delete("my-agent"),
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn agent_responses_parse_tools_and_preserve_unknown_environments() {
    let agent = json!({
        "id": "my-agent",
        "base_agent": "agents/base",
        "tools": [{"type": "code_execution"}, {"type": "url_context"}],
        "base_environment": {"type": "gpu_sandbox", "accelerator": "l4"},
        "state": "ready"
    });
    let stub = Stub::replying(vec![
        Reply::json(200, agent.clone()),
        Reply::json(
            200,
            json!({"agents": [agent, {"id": "other", "base_environment": "env-9"}], "next_page_token": "p2"}),
        ),
    ])
    .await;
    let client = stub.client();

    let fetched = client.agents().get("my-agent").await.unwrap();
    assert!(matches!(
        fetched.tools.as_deref(),
        Some([Tool::CodeExecution { .. }, Tool::UrlContext { .. }])
    ));
    let env = fetched.base_environment.as_ref().unwrap();
    assert_eq!(env.unknown_environment_type(), Some("gpu_sandbox"));
    assert_eq!(fetched.extra["state"], "ready");
    let back = serde_json::to_value(&fetched).unwrap();
    assert_eq!(
        back["base_environment"],
        json!({"type": "gpu_sandbox", "accelerator": "l4"})
    );

    let list = client.agents().list().send().await.unwrap();
    assert_eq!(list.agents.len(), 2);
    assert_eq!(
        list.agents[1].base_environment,
        Some(EnvironmentSpec::Id("env-9".to_string()))
    );
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}
