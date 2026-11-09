use std::collections::BTreeMap;

use crate::redact::REDACTED;
use crate::antigravity::handshake::{InputConfig, OutputConfig};
use crate::antigravity::protocol::{
    GeminiApiEndpoint, HarnessConfig, InitializeConversationEvent, McpHttpTransport,
    McpServerConfig, McpStdioTransport, ModelConfig, VertexEndpoint,
};
use crate::antigravity::{AntigravityAgent, McpServer};

/// The secret every value below holds; no `Debug` output may contain it.
const SECRET: &str = "sentinel-secret-5150";
/// A secret's name, which `Debug` keeps.
const NAME: &str = "X-Sentinel-Name";

fn secrets() -> BTreeMap<String, String> {
    BTreeMap::from([(NAME.to_string(), SECRET.to_string())])
}

/// Asserts `debug` hides the secret and keeps its name.
fn assert_redacted(what: &str, debug: &str, keeps_name: bool) {
    assert!(
        !debug.contains(SECRET),
        "{what}'s Debug prints the secret: {debug}"
    );
    assert!(
        debug.contains(REDACTED),
        "{what}'s Debug marks no redaction: {debug}"
    );
    if keeps_name {
        assert!(
            debug.contains(NAME),
            "{what}'s Debug drops the name: {debug}"
        );
    }
}

fn gemini_endpoint() -> GeminiApiEndpoint {
    GeminiApiEndpoint {
        base_url: Some("http://localhost:1".to_string()),
        http_headers: secrets(),
        api_key: Some(SECRET.to_string()),
        options: None,
    }
}

fn mcp_servers() -> Vec<McpServerConfig> {
    vec![
        McpServerConfig {
            name: Some("stdio".to_string()),
            stdio: Some(McpStdioTransport {
                command: Some("server".to_string()),
                args: Vec::new(),
                env: secrets(),
            }),
            ..Default::default()
        },
        McpServerConfig {
            name: Some("http".to_string()),
            http: Some(McpHttpTransport {
                url: Some("http://localhost:2/mcp".to_string()),
                headers: secrets(),
            }),
            ..Default::default()
        },
    ]
}

#[test]
fn test_debug_never_prints_a_secret() {
    let vertex = VertexEndpoint {
        http_headers: secrets(),
        project: Some("project".to_string()),
        ..Default::default()
    };
    let config = HarnessConfig {
        models: vec![
            ModelConfig {
                gemini_api_endpoint: Some(gemini_endpoint()),
                ..Default::default()
            },
            ModelConfig {
                vertex_endpoint: Some(vertex.clone()),
                ..Default::default()
            },
        ],
        mcp_servers: mcp_servers(),
        ..Default::default()
    };
    let builder = AntigravityAgent::builder()
        .with_api_key(SECRET)
        .add_model_header(NAME, SECRET)
        .add_harness_env(NAME, SECRET)
        .add_mcp_server(McpServer::stdio("server", ["--flag"]).add_env(NAME, SECRET));
    let input = InputConfig {
        env: secrets(),
        ..Default::default()
    };
    let output = OutputConfig {
        port: 1,
        api_key: SECRET.to_string(),
    };

    let cases = [
        (
            "GeminiApiEndpoint",
            format!("{:?}", gemini_endpoint()),
            true,
        ),
        ("VertexEndpoint", format!("{vertex:?}"), true),
        (
            "McpStdioTransport",
            format!("{:?}", mcp_servers()[0].stdio),
            true,
        ),
        (
            "McpHttpTransport",
            format!("{:?}", mcp_servers()[1].http),
            true,
        ),
        ("HarnessConfig", format!("{config:?}"), true),
        (
            "InitializeConversationEvent",
            format!(
                "{:?}",
                InitializeConversationEvent {
                    config: Some(config.clone()),
                }
            ),
            true,
        ),
        (
            "McpServer (stdio)",
            format!(
                "{:?}",
                McpServer::stdio("server", ["--flag"]).add_env(NAME, SECRET)
            ),
            true,
        ),
        (
            "McpServer (http)",
            format!(
                "{:?}",
                McpServer::http("http://localhost:2/mcp").add_header(NAME, SECRET)
            ),
            true,
        ),
        ("AgentBuilder", format!("{builder:?}"), true),
        ("InputConfig", format!("{input:?}"), true),
        ("OutputConfig", format!("{output:?}"), false),
    ];
    for (what, debug, keeps_name) in cases {
        assert_redacted(what, &debug, keeps_name);
    }
}

#[test]
fn test_an_unset_secret_prints_as_unset() {
    let endpoint = GeminiApiEndpoint::default();
    let debug = format!("{endpoint:?}");
    assert!(debug.contains("api_key: None"), "{debug}");
    assert!(debug.contains("http_headers: {}"), "{debug}");
}
