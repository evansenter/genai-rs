use super::*;

use serde_json::json;

fn response(steps: serde_json::Value) -> InteractionResponse {
    serde_json::from_value(json!({"id": "i1", "status": "completed", "steps": steps}))
        .expect("response should deserialize")
}

/// The steps of a live MCP interaction (2026-11-04, `gemini-3.8-flash`, an
/// `mcp_server` tool named `wikiprobe`), signatures shortened.
fn live_mcp_steps() -> serde_json::Value {
    json!([
        {"type": "function_call", "id": "call_472212", "signature": "s1",
         "name": "wikiprobe:read_wiki_structure",
         "arguments": {"repoName": "facebook/react"}},
        {"type": "function_result", "call_id": "call_472212", "signature": "s2",
         "name": "wikiprobe:read_wiki_structure", "is_error": false,
         "result": {"result": "Available pages for facebook/react: ...",
                    "structured_content": {"result": "Available pages for facebook/react: ..."},
                    "call_tool_result_json": "{\"content\":[],\"isError\":false}"}},
        {"type": "thought", "signature": "s3"},
        {"type": "model_output", "content": [{"type": "text", "text": "Overview."}]}
    ])
}

#[test]
fn mcp_calls_reads_the_live_function_call_shape() {
    let response = response(live_mcp_steps());
    let calls = response.mcp_calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.id, "call_472212");
    assert_eq!(call.server, "wikiprobe");
    assert_eq!(call.tool, "read_wiki_structure");
    assert_eq!(call.arguments, &json!({"repoName": "facebook/react"}));
    assert_eq!(call.is_error, Some(false));
    let result = call
        .result
        .and_then(FunctionResultPayload::as_json)
        .unwrap();
    assert!(result["call_tool_result_json"].is_string());
    assert!(result["structured_content"].is_object());
    // Answered by the server, so nothing is left for the client.
    assert!(response.pending_function_calls().is_empty());
}

/// A failed MCP call as the live API returned it (2026-11-04,
/// `gemini-3.8-flash`, deepwiki asked about a repository it had not
/// indexed): the step's `is_error` is `false`, and only the result object's
/// own `is_error` (and the `isError` in `call_tool_result_json`) says the
/// tool failed.
#[test]
fn mcp_calls_reports_a_failure_flagged_only_inside_the_result() {
    let response = response(json!([
        {"type": "function_call", "id": "call_221719", "signature": "s1",
         "name": "deepwiki:read_wiki_structure",
         "arguments": {"repoName": "evansenter/genai-rs"}},
        {"type": "function_result", "call_id": "call_221719", "signature": "s2",
         "name": "deepwiki:read_wiki_structure", "is_error": false,
         "result": {"result": "Error fetching wiki for evansenter/genai-rs: Repository not found.",
                    "call_tool_result_json": "{\"content\":[{\"type\":\"text\",\"text\":\"Error fetching wiki\"}],\"isError\":true}",
                    "is_error": true}}
    ]));
    let calls = response.mcp_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].is_error, Some(true));
    // The step itself still says `false`; only the helper combines the two.
    assert!(matches!(
        &response.steps[1],
        Step::FunctionResult {
            is_error: Some(false),
            ..
        }
    ));
}

#[test]
fn mcp_calls_is_error_reads_the_step_flag_and_is_none_without_either() {
    let response = response(json!([
        {"type": "function_call", "id": "c1", "name": "docs:search", "arguments": {}},
        {"type": "function_result", "call_id": "c1", "is_error": true, "result": "boom"},
        {"type": "function_call", "id": "c2", "name": "docs:search", "arguments": {}},
        {"type": "function_result", "call_id": "c2", "result": {"result": "ok"}},
        {"type": "mcp_server_tool_call", "id": "m1", "name": "search",
         "server_name": "docs", "arguments": {}},
        {"type": "mcp_server_tool_result", "call_id": "m1",
         "result": {"result": "no", "is_error": true}}
    ]));
    let flags: Vec<Option<bool>> = response.mcp_calls().iter().map(|c| c.is_error).collect();
    assert_eq!(flags, [Some(true), None, Some(true)]);
}

/// A client function may carry a colon too (the API accepted `weather:get`
/// on 2026-11-04). Unanswered in the response, it is the client's call.
#[test]
fn mcp_calls_skips_an_unanswered_colon_named_client_call() {
    let response = response(json!([
        {"type": "function_call", "id": "c1", "name": "weather:get",
         "arguments": {"city": "Paris"}}
    ]));
    assert!(response.mcp_calls().is_empty());
    assert_eq!(response.pending_function_calls().len(), 1);
}

#[test]
fn mcp_calls_skips_answered_calls_without_a_server_prefix() {
    let response = response(json!([
        {"type": "function_call", "id": "c1", "name": "plain", "arguments": {}},
        {"type": "function_result", "call_id": "c1", "result": "ok"},
        {"type": "function_call", "id": "c2", "name": ":tool", "arguments": {}},
        {"type": "function_result", "call_id": "c2", "result": "ok"}
    ]));
    assert!(response.mcp_calls().is_empty());
}

#[test]
fn mcp_calls_strips_the_server_prefix_from_the_live_spec_shape() {
    // Live 2026-11-05: `name` carries the `<server>:` prefix next to an
    // explicit `server_name`, so the tool must come back bare, and a colon
    // inside the server name must not split it.
    let response = response(json!([
        {"type": "mcp_server_tool_call", "id": "c1",
         "name": "deepwiki:read_wiki_structure", "server_name": "deepwiki",
         "arguments": {"repoName": "facebook/react"}, "signature": "s"},
        {"type": "mcp_server_tool_result", "call_id": "c1",
         "name": "deepwiki:read_wiki_structure", "server_name": "deepwiki",
         "result": {"result": "pages", "call_tool_result_json": "{}",
                    "structured_content": {"result": "pages"}}},
        {"type": "mcp_server_tool_call", "id": "c2", "name": "corp:search:lookup",
         "server_name": "corp:search", "arguments": {}},
    ]));
    let calls = response.mcp_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        (calls[0].server, calls[0].tool),
        ("deepwiki", "read_wiki_structure")
    );
    assert_eq!(calls[0].is_error, None);
    assert!(calls[0].result.is_some());
    assert_eq!((calls[1].server, calls[1].tool), ("corp:search", "lookup"));
    assert!(calls[1].result.is_none());
}

#[test]
fn mcp_calls_reads_the_spec_mcp_server_tool_steps() {
    let response = response(json!([
        {"type": "mcp_server_tool_call", "id": "m1", "name": "search",
         "server_name": "docs", "arguments": {"q": "x"}},
        {"type": "mcp_server_tool_result", "call_id": "m1", "result": "found"}
    ]));
    let calls = response.mcp_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].server, calls[0].tool), ("docs", "search"));
    assert_eq!(
        calls[0].result.and_then(FunctionResultPayload::as_text),
        Some("found")
    );
}

#[test]
fn mcp_calls_ignores_the_unidentified_tool_call_step() {
    let response = response(json!([{"type": "tool_call", "id": "t1", "signature": "s"}]));
    assert!(response.mcp_calls().is_empty());
    assert_eq!(response.tool_calls().len(), 1);
}
