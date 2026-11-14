//! The sentinel test for D-067: every public type that holds a secret,
//! formatted with `Debug`, alone and inside the requests, responses and
//! builders that carry it, prints no secret.

use super::REDACTED;
use crate::wire::WireEvent;
use crate::{
    Agent, AllowlistEntry, Client, CreateCredentialRequest, CreateTriggerRequest, CredentialConfig,
    CredentialType, CredentialUpdate, EnvVar, Environment, EnvironmentSpec, ExaAiSearchConfig,
    InjectionLocation, InteractionResponse, McpServerConfig, NetworkConfig, ParallelAiSearchConfig,
    RemoteEnvironment, RetrievalConfig, RotateSigningSecretResponse, Tool, Trigger, Webhook,
};
use serde_json::{Map, Value, json};
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
/// secrets) are here too, and so are the JSON values that carry these
/// secrets: a retrieval config's `custom_config`, an `env` the crate could
/// not read, the `extra` of every `Tool` arm ([`every_tool_arm`]), of a
/// webhook and of the environment pieces, an unknown tool's payload, and a
/// `WireEvent`'s bodies and harness messages. Every one shows the
/// placeholder where the secret was, and the names around the secret stay.
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
    let request_body = serde_json::to_value(&request).unwrap();
    // One entry is not an `EnvVar`, so the whole map is kept in `extra`.
    // Every string beneath it prints redacted, not only what the wire's walk
    // reads as a value: `NOTED`'s `note` (#504, L-3).
    let unread_env: RemoteEnvironment = serde_json::from_value(json!({
        "type": "remote",
        "env": {
            "API_KEY": {"value": SECRET},
            "OTHER": SECRET,
            "NOTED": {"value": SECRET, "note": SECRET},
        },
    }))
    .unwrap();
    assert!(unread_env.env.is_empty() && unread_env.extra.contains_key("env"));

    // JSON the crate does not model, where a secret travels under a name
    // the wire redacts (#555 item 4).
    let unknown_tool = Tool::Unknown {
        tool_type: "future_tool".to_string(),
        data: json!({"type": "future_tool", "config": {"api_key": SECRET}}),
    };
    // An environment whose `type` is not a string reads as unknown, its
    // `env` unread (D-071).
    let unknown_environment: EnvironmentSpec = serde_json::from_value(json!({
        "type": 5,
        "env": {"API_KEY": {"value": SECRET}, "OTHER": SECRET},
    }))
    .unwrap();
    assert!(unknown_environment.is_unknown());
    // Every arm of `Tool` holds this as its `extra` (`every_tool_arm`).
    let tool_extra = json!({"auth": {"headers": {"Authorization": SECRET}}, "token": SECRET})
        .as_object()
        .unwrap()
        .clone();
    let webhook_extra = json!({"secret": SECRET, "delivery": {"token": SECRET}})
        .as_object()
        .unwrap()
        .clone();
    // The environment pieces' own `extra`, beside the secrets they model.
    let unmodeled = json!({"auth": {"api_key": SECRET}})
        .as_object()
        .unwrap()
        .clone();
    let mut allowlist_extra = AllowlistEntry::new("api.example.com");
    allowlist_extra.extra = unmodeled.clone();
    let mut env_var_extra = EnvVar::credential("cred-1");
    env_var_extra.extra = unmodeled.clone();
    let mut environment_extra = RemoteEnvironment::new();
    environment_extra.extra = unmodeled.clone();

    let mut cases: Vec<(String, String)> = [
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
        (
            "ExaAiSearchConfig::custom_config",
            format!(
                "{:?}",
                ExaAiSearchConfig::new("key").with_custom_config(json!({"api_key": SECRET}))
            ),
        ),
        (
            "RemoteEnvironment (an env it could not read)",
            format!("{unread_env:?}"),
        ),
        (
            "WireEvent::Request",
            format!(
                "{:?}",
                WireEvent::Request {
                    id: 1,
                    method: "POST".to_string(),
                    url: "https://example.com/v1beta/interactions".to_string(),
                    body: Some(request_body),
                }
            ),
        ),
        (
            "WireEvent::ResponseBody",
            format!(
                "{:?}",
                WireEvent::ResponseBody {
                    id: 1,
                    body: json!({"secret": SECRET, "truncated_secret": "abc"}),
                }
            ),
        ),
        (
            "WireEvent::ErrorBody",
            format!(
                "{:?}",
                WireEvent::ErrorBody {
                    id: 1,
                    status: 400,
                    body: json!({"token": SECRET}).to_string(),
                }
            ),
        ),
        (
            "WireEvent::SseFrame",
            format!(
                "{:?}",
                WireEvent::SseFrame {
                    id: 1,
                    event_type: None,
                    data: json!({"interaction": {"tools": [
                        {"type": "mcp_server", "headers": {"Authorization": SECRET}}
                    ]}})
                    .to_string(),
                }
            ),
        ),
        (
            "WireEvent::WsSend",
            format!(
                "{:?}",
                WireEvent::WsSend {
                    id: 1,
                    payload: json!({"config": {"api_key": SECRET}}),
                }
            ),
        ),
        // The harness's own camelCase keys, in a message it sends back.
        (
            "WireEvent::WsReceive",
            format!(
                "{:?}",
                WireEvent::WsReceive {
                    id: 1,
                    payload: json!({"models": [{"geminiApiEndpoint": {
                        "apiKey": SECRET,
                        "httpHeaders": {"Authorization": SECRET}
                    }}], "mcpServers": [{"stdio": {"env": {"API_KEY": SECRET}}}]}),
                }
            ),
        ),
        ("Tool::Unknown", format!("{unknown_tool:?}")),
        (
            "EnvironmentSpec::Unknown",
            format!("{unknown_environment:?}"),
        ),
        ("AllowlistEntry's extra", format!("{allowlist_extra:?}")),
        ("EnvVar's extra", format!("{env_var_extra:?}")),
        (
            "RemoteEnvironment's extra",
            format!("{environment_extra:?}"),
        ),
        (
            "Webhook's extra",
            format!(
                "{:?}",
                Webhook {
                    extra: webhook_extra.clone(),
                    ..Default::default()
                }
            ),
        ),
        (
            "RotateSigningSecretResponse's extra",
            format!(
                "{:?}",
                RotateSigningSecretResponse {
                    extra: webhook_extra.clone(),
                    ..Default::default()
                }
            ),
        ),
    ]
    .into_iter()
    .map(|(what, debug)| (what.to_string(), debug))
    .collect();
    let tool_arms = every_tool_arm(&tool_extra);
    cases.extend(tool_arms.iter().map(|tool| {
        (
            format!("Tool::{}'s extra", tool_arm(tool)),
            format!("{tool:?}"),
        )
    }));

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
    let request_names = ["Authorization", "X-Api-Key", "API_KEY", "api.example.com"];
    let kept: [(&str, &[&str]); 8] = [
        ("InteractionRequest", &request_names),
        ("WireEvent::Request", &request_names),
        ("WireEvent::WsReceive", &["Authorization", "API_KEY"]),
        ("AllowlistEntry's extra", &["auth"]),
        ("EnvVar's extra", &["auth", "cred-1"]),
        ("RemoteEnvironment's extra", &["auth"]),
        ("Webhook's extra", &["delivery"]),
        ("RotateSigningSecretResponse's extra", &["delivery"]),
    ];
    let arm_names: Vec<String> = tool_arms
        .iter()
        .map(|tool| format!("Tool::{}'s extra", tool_arm(tool)))
        .collect();
    let kept = kept.into_iter().chain(
        arm_names
            .iter()
            .map(|what| (what.as_str(), &["auth", "Authorization"] as &[&str])),
    );
    for (what, names) in kept {
        let (_, debug) = cases.iter().find(|(w, _)| *w == what).unwrap();
        for name in names {
            assert!(debug.contains(name), "{what}, {name}: {debug}");
        }
    }
    let (_, unread_debug) = cases
        .iter()
        .find(|(w, _)| w.starts_with("RemoteEnvironment ("))
        .unwrap();
    for name in ["API_KEY", "OTHER", "NOTED", "note"] {
        assert!(unread_debug.contains(name), "{name}: {unread_debug}");
    }
}

/// One value of every `Tool` arm, each holding `extra` as its `extra` map
/// (an unknown tool in its payload, beside its `type`) and no other secret,
/// so the arm's own `extra` is what the sentinel reads. [`tool_arm`] names
/// each, and
/// `the_sentinel_builds_every_tool_arm` fails on an arm of the enum left
/// out here.
fn every_tool_arm(extra: &Map<String, Value>) -> Vec<Tool> {
    let mut data = extra.clone();
    data.insert("type".to_string(), json!("future_tool"));
    vec![
        Tool::Function {
            name: "get_weather".to_string(),
            description: None,
            parameters: None,
            extra: extra.clone(),
        },
        Tool::GoogleSearch {
            search_types: None,
            extra: extra.clone(),
        },
        Tool::GoogleMaps {
            enable_widget: None,
            latitude: None,
            longitude: None,
            extra: extra.clone(),
        },
        Tool::CodeExecution {
            extra: extra.clone(),
        },
        Tool::UrlContext {
            extra: extra.clone(),
        },
        Tool::ComputerUse {
            environment: "browser".to_string(),
            excluded_predefined_functions: Vec::new(),
            enable_prompt_injection_detection: None,
            disabled_safety_policies: Vec::new(),
            extra: extra.clone(),
        },
        Tool::McpServer {
            name: "server".to_string(),
            url: "https://mcp.example.com".to_string(),
            allowed_tools: None,
            headers: None,
            extra: extra.clone(),
        },
        Tool::FileSearch {
            store_names: Vec::new(),
            top_k: None,
            metadata_filter: None,
            extra: extra.clone(),
        },
        Tool::Retrieval {
            retrieval_types: None,
            vertex_ai_search_config: None,
            exa_ai_search_config: None,
            parallel_ai_search_config: None,
            rag_store_config: None,
            extra: extra.clone(),
        },
        Tool::Unknown {
            tool_type: "future_tool".to_string(),
            data: Value::Object(data),
        },
    ]
}

/// The arm's name, by a match with no wildcard arm: an arm added to `Tool`
/// fails to compile here until it is named.
fn tool_arm(tool: &Tool) -> &'static str {
    match tool {
        Tool::Function { .. } => "Function",
        Tool::GoogleSearch { .. } => "GoogleSearch",
        Tool::GoogleMaps { .. } => "GoogleMaps",
        Tool::CodeExecution { .. } => "CodeExecution",
        Tool::UrlContext { .. } => "UrlContext",
        Tool::ComputerUse { .. } => "ComputerUse",
        Tool::McpServer { .. } => "McpServer",
        Tool::FileSearch { .. } => "FileSearch",
        Tool::Retrieval { .. } => "Retrieval",
        Tool::Unknown { .. } => "Unknown",
    }
}

/// The sentinel formats every arm of `Tool` with a secret in its `extra`:
/// [`every_tool_arm`] builds each arm `src/tools/mod.rs` declares, once, so
/// a new arm fails here until it is built there (#555 item 4).
#[test]
fn the_sentinel_builds_every_tool_arm() {
    let file = syn::parse_file(include_str!("tools/mod.rs")).expect("src/tools/mod.rs parses");
    let declared: Vec<String> = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(e) if e.ident == "Tool" => Some(e),
            _ => None,
        })
        .expect("src/tools/mod.rs declares Tool")
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect();
    let built: Vec<String> = every_tool_arm(&Map::new())
        .iter()
        .map(|tool| tool_arm(tool).to_string())
        .collect();
    assert_eq!(
        built, declared,
        "every_tool_arm builds each arm of Tool once"
    );
}

/// A body with no secret prints as the derive printed it: the walk
/// redacts, and truncates nothing.
#[test]
fn a_wire_event_without_a_secret_prints_whole() {
    let data = "x".repeat(300);
    let frame = json!({"event_type": "step.delta", "delta": {"data": data}}).to_string();
    assert_eq!(
        format!(
            "{:?}",
            WireEvent::SseFrame {
                id: 7,
                event_type: Some("message".to_string()),
                data: frame.clone(),
            }
        ),
        format!("SseFrame {{ id: 7, event_type: Some(\"message\"), data: {frame:?} }}")
    );
    let body = json!({"signature": data, "text": "hi"});
    assert_eq!(
        format!(
            "{:?}",
            WireEvent::ResponseBody {
                id: 7,
                body: body.clone(),
            }
        ),
        format!("ResponseBody {{ id: 7, body: {body:?} }}")
    );
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
