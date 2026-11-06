use super::*;

#[test]
fn test_mcp_stdio_defaults_name_from_command() {
    let server = McpServer::stdio("/usr/bin/uvx", ["mcp-server-git"]);
    assert_eq!(server.name(), "uvx");
    let wire = server.to_wire();
    let stdio = wire.stdio.unwrap();
    assert_eq!(stdio.command.as_deref(), Some("/usr/bin/uvx"));
    assert_eq!(stdio.args, vec!["mcp-server-git"]);
    assert!(wire.http.is_none());
}

#[test]
fn test_mcp_http_defaults_name_from_host() {
    let server = McpServer::http("http://localhost:8931/mcp");
    assert_eq!(server.name(), "localhost");
    let wire = server.to_wire();
    assert_eq!(
        wire.http.unwrap().url.as_deref(),
        Some("http://localhost:8931/mcp")
    );
    assert!(wire.stdio.is_none());
}

#[test]
fn test_mcp_builders_accumulate() {
    let server = McpServer::stdio("uvx", ["x"])
        .with_name("git")
        .add_env("A", "1")
        .add_env("B", "2")
        .with_enabled_tools(["status"])
        .with_timeout_seconds(30);
    assert_eq!(server.name(), "git");
    let wire = server.to_wire();
    assert_eq!(wire.stdio.unwrap().env.len(), 2);
    assert_eq!(wire.enabled_tools, vec!["status"]);
    assert_eq!(wire.timeout_seconds, Some(30));
}
