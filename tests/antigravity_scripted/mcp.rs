//! MCP server options, each shown changing what the harness does (D-010):
//! the headers an HTTP server receives, the environment a stdio server
//! runs with, the tools the model is offered, how long the harness waits
//! for a server, and how large a result it passes on.

use super::*;
use common::http_stub::{Recorded, Reply, Stub};
use genai_rs::antigravity::McpServer;

/// The stdio MCP fixture `tests/fixtures/mcp_echo_server.py`.
fn widgets_server() -> McpServer {
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_echo_server.py");
    McpServer::stdio("python3", [fixture.to_string_lossy().to_string()]).with_name("widgets")
}

/// A stdio MCP server, run by `python3 -c`. Its tools: `env_probe` answers
/// with `AGY_MCP_PROBE` from its environment, and `alpha`, `beta` and
/// `gamma` with their own names. `AGY_MCP_SLOW_LIST` and
/// `AGY_MCP_SLOW_CALL` delay its `tools/list` answer and `beta`'s by that
/// many seconds.
const STDIO_PROBE: &str = r#"
import json, os, sys, time
TOOLS = [
    {"name": name, "description": "Answers with " + name + ".",
     "inputSchema": {"type": "object", "properties": {}}}
    for name in ("env_probe", "alpha", "beta", "gamma")
]
for line in sys.stdin:
    try:
        request = json.loads(line)
    except ValueError:
        continue
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                  "serverInfo": {"name": "probe", "version": "0"}}
    elif method == "tools/list":
        time.sleep(float(os.environ.get("AGY_MCP_SLOW_LIST", "0")))
        result = {"tools": TOOLS}
    elif method == "tools/call":
        name = request["params"]["name"]
        if name == "env_probe":
            text = "probe=[%s]" % os.environ.get("AGY_MCP_PROBE", "")
        else:
            if name == "beta":
                time.sleep(float(os.environ.get("AGY_MCP_SLOW_CALL", "0")))
            text = "called " + name
        result = {"content": [{"type": "text", "text": text}], "isError": False}
    else:
        result = {}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}) + "\n")
    sys.stdout.flush()
"#;

/// [`STDIO_PROBE`] as a server named `probe`, its tools declared to the
/// model by name (`mcp_probe_<tool>`).
fn stdio_probe() -> McpServer {
    McpServer::stdio("python3", ["-c", STDIO_PROBE])
        .with_name("probe")
        .with_force_all_tools_eager(true)
}

/// The `mcp_<server>_*` tools model request `n` declares, sorted.
fn mcp_tools(fake: &FakeGemini, n: usize, server: &str) -> Vec<String> {
    let prefix = format!("mcp_{server}_");
    let mut tools: Vec<String> = fake
        .tool_names(n)
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .collect();
    tools.sort();
    tools
}

/// The header [`http_probe`] echoes.
const PROBE_HEADER: &str = "x-probe-token";

/// A streamable-HTTP MCP server with one tool, `whoami`, which answers with
/// the [`PROBE_HEADER`] its call arrived with. Every request is recorded.
async fn http_probe() -> Stub {
    Stub::start(|request: &Recorded, _| {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        // A notification (no id), the client's GET for a server stream and
        // its OAuth discovery: nothing to answer.
        let Some(id) = body.get("id").cloned() else {
            let status = if request.method == "POST" { 202 } else { 405 };
            return Reply::text(status, "");
        };
        let result = match body["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "remote", "version": "0"}
            }),
            Some("tools/list") => json!({"tools": [{
                "name": "whoami",
                "description": "Answers with the probe header it received.",
                "inputSchema": {"type": "object", "properties": {}}
            }]}),
            Some("tools/call") => json!({
                "content": [{
                    "type": "text",
                    "text": format!("header=[{}]", request.header(PROBE_HEADER).unwrap_or(""))
                }],
                "isError": false
            }),
            _ => {
                return Reply::json(
                    200,
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no"}}),
                );
            }
        };
        Reply::json(200, json!({"jsonrpc": "2.0", "id": id, "result": result}))
    })
    .await
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_mcp_http_server_gets_its_headers_on_every_request() {
    let server = http_probe().await;
    let fake = FakeGemini::start(vec![
        ModelTurn::call("mcp_remote_whoami", json!({})),
        ModelTurn::text("done"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_mcp_server(
            McpServer::http(format!("{}/mcp", server.base_url))
                .with_name("remote")
                .add_header(PROBE_HEADER, "header-value-7731")
                .with_force_all_tools_eager(true),
        )
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("who am I").await.expect("turn");

    assert_eq!(mcp_tools(&fake, 0, "remote"), ["mcp_remote_whoami"]);
    let told = function_response_text(&fake, 1);
    assert!(
        told.contains("header=[header-value-7731]"),
        "the call reached the server with its header: {told}"
    );
    let mcp_requests: Vec<Recorded> = server
        .requests()
        .into_iter()
        .filter(|request| request.target == "/mcp")
        .collect();
    let methods: Vec<String> = mcp_requests
        .iter()
        .filter_map(|request| {
            serde_json::from_slice::<Value>(&request.body).ok()?["method"]
                .as_str()
                .map(str::to_string)
        })
        .collect();
    for method in ["initialize", "tools/list", "tools/call"] {
        assert!(methods.iter().any(|m| m == method), "{methods:?}");
    }
    for request in &mcp_requests {
        assert_eq!(
            request.header(PROBE_HEADER),
            Some("header-value-7731"),
            "{} {} went out without the header",
            request.method,
            request.target
        );
    }
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_mcp_enabled_and_disabled_tools_choose_what_the_model_sees() {
    let tools = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .map(|name| format!("mcp_probe_{name}"))
            .collect()
    };
    let cases = [
        (
            stdio_probe(),
            tools(&["alpha", "beta", "env_probe", "gamma"]),
        ),
        (
            stdio_probe().with_enabled_tools(["alpha", "gamma"]),
            tools(&["alpha", "gamma"]),
        ),
        (
            stdio_probe().with_disabled_tools(["beta"]),
            tools(&["alpha", "env_probe", "gamma"]),
        ),
    ];
    for (server, expected) in cases {
        let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
        let mut agent = scripted_agent(&fake)
            .add_mcp_server(server)
            .add_policy(policy::allow_all())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("hi").await.expect("turn");
        assert_eq!(mcp_tools(&fake, 0, "probe"), expected);
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_mcp_stdio_env_reaches_the_server() {
    for (server, expected) in [
        (
            stdio_probe().add_env("AGY_MCP_PROBE", "env-value-8840"),
            "probe=[env-value-8840]",
        ),
        (stdio_probe(), "probe=[]"),
    ] {
        let fake = FakeGemini::start(vec![
            ModelTurn::call("mcp_probe_env_probe", json!({})),
            ModelTurn::text("done"),
        ])
        .await;
        let mut agent = scripted_agent(&fake)
            .add_mcp_server(server)
            .add_policy(policy::allow_all())
            .spawn()
            .await
            .expect("spawn");
        agent.chat("probe").await.expect("turn");
        let told = function_response_text(&fake, 1);
        assert!(told.contains(expected), "expected {expected}: {told}");
        agent.shutdown().await.expect("shutdown");
    }
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_mcp_timeout_bounds_listing_tools_and_each_call() {
    // Listing: a server slower than its timeout fails the first turn, which
    // is when the harness loads it; without the timeout it loads.
    for timeout in [Some(1), None] {
        let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
        let server = stdio_probe().add_env("AGY_MCP_SLOW_LIST", "2");
        let server = match timeout {
            Some(seconds) => server.with_timeout_seconds(seconds),
            None => server,
        };
        let mut agent = scripted_agent(&fake)
            .add_mcp_server(server)
            .add_policy(policy::allow_all())
            .spawn()
            .await
            .expect("spawn");
        let outcome = agent.chat("hi").await;
        if timeout.is_some() {
            let Err(AntigravityError::Turn(message)) = &outcome else {
                panic!("a server slower than its timeout must fail the turn: {outcome:?}");
            };
            assert!(message.contains("MCP load failed for probe"), "{message}");
            assert!(fake.requests().is_empty(), "the model was never called");
        } else {
            outcome.expect("turn");
            assert_eq!(mcp_tools(&fake, 0, "probe").len(), 4);
        }
        agent.shutdown().await.expect("shutdown");
    }

    // A call slower than the timeout comes back to the model as an error.
    let fake = FakeGemini::start(vec![
        ModelTurn::call("mcp_probe_beta", json!({})),
        ModelTurn::text("done"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_mcp_server(
            stdio_probe()
                .add_env("AGY_MCP_SLOW_CALL", "3")
                .with_timeout_seconds(1),
        )
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("call beta").await.expect("turn");
    let told = function_response_text(&fake, 1);
    assert!(told.contains("timed out after 1s"), "{told}");
    assert!(!told.contains("called beta"), "{told}");
    agent.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[ignore = "Requires localharness binary"]
async fn test_scripted_mcp_eager_tools_and_output_cap() {
    // Default: the server's tools sit behind call_mcp_tool.
    let fake = FakeGemini::start(vec![ModelTurn::text("ok")]).await;
    let mut agent = scripted_agent(&fake)
        .add_mcp_server(widgets_server())
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("hi").await.expect("turn");
    let tools = fake.tool_names(0);
    assert!(tools.contains(&"call_mcp_tool".to_string()), "{tools:?}");
    assert!(!tools.contains(&"mcp_widgets_lookup_widget_code".to_string()));
    agent.shutdown().await.expect("shutdown");

    // Eager: declared by name. A tiny output cap spills the result to a
    // file.
    let fake = FakeGemini::start(vec![
        ModelTurn::call(
            "mcp_widgets_lookup_widget_code",
            json!({"widget": "flange"}),
        ),
        ModelTurn::text("done"),
    ])
    .await;
    let mut agent = scripted_agent(&fake)
        .add_mcp_server(widgets_server().with_force_all_tools_eager(true))
        .with_mcp_max_output_bytes(5)
        .add_policy(policy::allow_all())
        .spawn()
        .await
        .expect("spawn");
    agent.chat("look it up").await.expect("turn");
    let tools = fake.tool_names(0);
    assert!(
        tools.contains(&"mcp_widgets_lookup_widget_code".to_string()),
        "{tools:?}"
    );
    let told = function_response_text(&fake, 1);
    assert!(told.contains("saved to"), "{told}");
    assert!(!told.contains("wibble-3317-quux"), "{told}");
    agent.shutdown().await.expect("shutdown");
}
