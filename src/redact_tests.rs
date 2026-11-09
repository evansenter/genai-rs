//! The sentinel test for D-NEXT-I4-a: every public type that holds a secret,
//! formatted with `Debug`, alone and inside the requests, responses and
//! builders that carry it, prints no secret.

use super::REDACTED;
use crate::{
    Agent, AllowlistEntry, Client, CreateCredentialRequest, CreateTriggerRequest, CredentialConfig,
    CredentialType, CredentialUpdate, EnvVar, Environment, ExaAiSearchConfig, InjectionLocation,
    InteractionResponse, McpServerConfig, NetworkConfig, ParallelAiSearchConfig, RemoteEnvironment,
    RetrievalConfig, RotateSigningSecretResponse, Tool, Trigger, Webhook,
};
use std::collections::HashMap;

/// The secret every value below holds, wherever it holds one.
const SECRET: &str = "sentinel-secret-8f3a";

fn header(name: &str) -> HashMap<String, String> {
    HashMap::from([(name.to_string(), SECRET.to_string())])
}

/// Each type holding a secret, alone and through what carries it: the
/// tool configs into `Tool`, `InteractionBuilder` and `InteractionRequest`
/// (and on to a trigger and a stored interaction's echo), the environment
/// pieces into `RemoteEnvironment`, an agent and an environment resource.
/// The types that redacted before (`Client`, the credential and webhook
/// secrets) are here too, so the list is the whole set. Every one shows
/// the placeholder where the secret was, and the names around the secret
/// stay.
#[test]
fn debug_never_prints_a_secret() {
    let exa = ExaAiSearchConfig::new(SECRET);
    let parallel = ParallelAiSearchConfig::new().with_api_key(SECRET);
    let retrieval: Tool = RetrievalConfig::new()
        .with_exa_ai_search(exa.clone())
        .with_parallel_ai_search(parallel.clone())
        .into();
    let mcp = McpServerConfig::new("server", "https://mcp.example.com")
        .with_headers(header("Authorization"));
    let mcp_tool: Tool = mcp.clone().into();
    let allowlist =
        AllowlistEntry::new("api.example.com").with_transform(vec![header("X-Api-Key")]);
    let env_var = EnvVar::value(SECRET);
    let environment = RemoteEnvironment::new()
        .add_env_var("API_KEY", env_var.clone())
        .with_network(NetworkConfig::allowlist(vec![allowlist.clone()]));

    let client = Client::new(SECRET);
    let builder = client
        .interaction()
        .with_model("test-model")
        .with_text("hello")
        .add_tool(retrieval.clone())
        .add_tool(mcp_tool.clone())
        .with_environment(environment.clone());
    let builder_debug = format!("{builder:?}");
    let request = builder.build().unwrap();

    let cases = [
        ("ExaAiSearchConfig", format!("{exa:?}")),
        ("ParallelAiSearchConfig", format!("{parallel:?}")),
        ("Tool::Retrieval", format!("{retrieval:?}")),
        ("McpServerConfig", format!("{mcp:?}")),
        ("Tool::McpServer", format!("{mcp_tool:?}")),
        ("AllowlistEntry", format!("{allowlist:?}")),
        ("EnvVar", format!("{env_var:?}")),
        ("RemoteEnvironment", format!("{environment:?}")),
        ("InteractionBuilder", builder_debug),
        ("InteractionRequest", format!("{request:?}")),
        (
            "InteractionResponse",
            format!(
                "{:?}",
                InteractionResponse {
                    tools: Some(vec![retrieval.clone(), mcp_tool.clone()]),
                    environment: Some(environment.clone().into()),
                    ..Default::default()
                }
            ),
        ),
        (
            "CreateTriggerRequest",
            format!(
                "{:?}",
                CreateTriggerRequest::new("0 * * * *", "UTC", request.clone())
            ),
        ),
        (
            "Trigger",
            format!(
                "{:?}",
                Trigger {
                    interaction: Some(request.clone()),
                    ..Default::default()
                }
            ),
        ),
        (
            "Agent",
            format!(
                "{:?}",
                Agent::new("agent", "base")
                    .add_tool(mcp_tool.clone())
                    .with_base_environment(environment.clone())
            ),
        ),
        (
            "Environment",
            format!(
                "{:?}",
                Environment {
                    network: Some(NetworkConfig::allowlist(vec![allowlist.clone()])),
                    ..Default::default()
                }
            ),
        ),
        ("Client", format!("{client:?}")),
        ("ClientBuilder", format!("{:?}", Client::builder(SECRET))),
        (
            "CreateCredentialRequest (bearer)",
            format!("{:?}", CreateCredentialRequest::bearer_token(SECRET)),
        ),
        (
            "CreateCredentialRequest (environment variable)",
            format!(
                "{:?}",
                CreateCredentialRequest::environment_variable(
                    SECRET,
                    vec![InjectionLocation::Header],
                    vec!["api.example.com".to_string()],
                )
            ),
        ),
        (
            "CredentialConfig::OAuth2",
            format!(
                "{:?}",
                CredentialConfig::OAuth2 {
                    client_id: "client".to_string(),
                    client_secret: SECRET.to_string(),
                    refresh_token: SECRET.to_string(),
                    token_url: "https://oauth.example/token".to_string(),
                    scopes: None,
                }
            ),
        ),
        (
            "CredentialUpdate",
            format!(
                "{:?}",
                CredentialUpdate {
                    token: Some(SECRET.to_string()),
                    value: Some(SECRET.to_string()),
                    client_secret: Some(SECRET.to_string()),
                    refresh_token: Some(SECRET.to_string()),
                    ..CredentialUpdate::new(CredentialType::BearerToken)
                }
            ),
        ),
        (
            "Webhook",
            format!(
                "{:?}",
                Webhook {
                    new_signing_secret: Some(SECRET.to_string()),
                    ..Default::default()
                }
            ),
        ),
        (
            "RotateSigningSecretResponse",
            format!(
                "{:?}",
                RotateSigningSecretResponse {
                    secret: Some(SECRET.to_string()),
                    ..Default::default()
                }
            ),
        ),
    ];

    for (what, debug) in &cases {
        assert!(
            !debug.contains(SECRET),
            "{what}'s Debug prints the secret: {debug}"
        );
        assert!(
            debug.contains(REDACTED),
            "{what}'s Debug does not show where the secret was: {debug}"
        );
    }
    // The names around a secret stay: the header's, the variable's.
    let (_, request_debug) = &cases[9];
    for name in ["Authorization", "X-Api-Key", "API_KEY", "api.example.com"] {
        assert!(request_debug.contains(name), "{name}: {request_debug}");
    }
}

/// A secret that is not set prints as `None`, so `Debug` still tells set
/// from unset; the other fields print as the derive would.
#[test]
fn an_unset_secret_prints_as_none() {
    assert_eq!(
        format!("{:?}", ParallelAiSearchConfig::new()),
        "ParallelAiSearchConfig { api_key: None, custom_config: None }"
    );
    assert_eq!(
        format!("{:?}", EnvVar::credential("cred-1")),
        r#"EnvVar { value: None, credential: Some("cred-1"), extra: {} }"#
    );
    assert_eq!(
        format!(
            "{:?}",
            Tool::from(McpServerConfig::new("server", "https://mcp.example.com"))
        ),
        r#"McpServer { name: "server", url: "https://mcp.example.com", allowed_tools: None, headers: None, extra: {} }"#
    );
    assert_eq!(
        format!("{:?}", Tool::code_execution()),
        "CodeExecution { extra: {} }"
    );
}
