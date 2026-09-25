//! Built-in tools (Google Search, code execution, URL context, Google Maps,
//! MCP, computer use), structured output, image output, and generation
//! config.
//!
//! Each built-in tool test asserts evidence that the tool ran (its call or
//! result steps), not just that the model produced text: a model that
//! ignored the tool and answered from memory must not pass.
//!
//! ```bash
//! cargo nextest run --test tools_and_config_tests --run-ignored all
//! ```

mod common;

use common::{
    assert_response_semantic, get_client, interaction_builder, is_safety_block_error,
    stateful_builder,
};
use genai_rs::{
    FunctionCallingMode, FunctionDeclaration, GenaiError, GenerationConfig, InteractionResponse,
    InteractionStatus, ThinkingLevel, ThinkingSummaries, Tool,
};
use serde_json::json;

/// Unwraps a built-in tool request, tolerating only the content-safety block
/// that fetched external pages intermittently trip (observed live 2026-07).
/// The skip is marked so CI counts it.
fn tool_response(
    result: Result<InteractionResponse, GenaiError>,
    what: &str,
) -> Option<InteractionResponse> {
    match result {
        Ok(response) => Some(response),
        Err(e) if is_safety_block_error(&e) => {
            println!(
                "LIVE_TOOL_EVIDENCE_SKIPPED: {what} blocked by the content safety filter: {e:?}"
            );
            None
        }
        Err(e) => panic!("{what} failed: {e:?}"),
    }
}

// =============================================================================
// Built-in Tools: Google Search
// =============================================================================

mod google_search {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    /// Search grounding on turn 1, and a follow-up that answers from the
    /// grounded context on turn 2.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result1 = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("What is the current weather in Tokyo, Japan today? Use search to find current data.")
                .with_google_search()
                .create()
                .await
        });
        let Some(response1) = tool_response(result1, "Google Search turn 1") else {
            return;
        };

        assert_eq!(response1.status, InteractionStatus::Completed);
        assert!(
            !response1.google_search_calls().is_empty(),
            "no search was issued: {:?}",
            response1.step_summary()
        );
        let text1 = response1.as_text().expect("turn 1 should answer in text");
        assert_response_semantic(
            &client,
            "Asked about the current weather in Tokyo with Google Search enabled",
            text1,
            "Does this response describe weather conditions or temperature in Tokyo?",
        )
        .await;

        let prev_id = response1.id.clone().expect("id should exist");
        let result2 = retry_request!([client, prev_id] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_text("Based on the weather information you just found, should I bring an umbrella if I visit Tokyo today?")
                .create()
                .await
        });
        let Some(response2) = tool_response(result2, "Google Search turn 2") else {
            return;
        };

        assert_eq!(response2.status, InteractionStatus::Completed);
        let text2 = response2.as_text().expect("turn 2 should answer in text");
        assert_response_semantic(
            &client,
            &format!("Turn 1 found this Tokyo weather: {text1}. The user then asked whether to bring an umbrella."),
            text2,
            "Does this response give umbrella advice consistent with that weather?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("What's the latest news about Rust programming language?")
            .with_google_search()
            .create_stream();

        let mut final_response = None;
        while let Some(result) = stream.next().await {
            if let StreamChunk::Completed(response) = result.expect("stream error").chunk {
                final_response = Some(response);
            }
        }

        let response = final_response.expect("Should receive complete response");
        assert!(
            !response.google_search_calls().is_empty(),
            "the accumulated response should carry the search step: {:?}",
            response.step_summary()
        );
    }

    /// Grounded text carries citation annotations pointing at sources.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_search_annotations() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Who won the most recent FIFA World Cup? Use search to verify and cite your sources.")
                .with_google_search()
                .create()
                .await
        });
        let Some(response) = tool_response(result, "Google Search") else {
            return;
        };

        let text = response.as_text().expect("Should have text");
        assert!(
            response.has_annotations(),
            "grounded text carried no annotations"
        );
        for annotation in response.all_annotations() {
            assert!(
                annotation.extract_span(text).is_some(),
                "annotation span out of range: {annotation:?}"
            );
        }
        assert!(
            response.all_annotations().any(|a| a.source().is_some()),
            "no annotation named a source"
        );
    }
}

// =============================================================================
// Built-in Tools: Code Execution
// =============================================================================

mod code_execution {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::{Step, StepDelta, StreamChunk};

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Calculate the factorial of 10 using Python code execution.")
                .with_code_execution()
                .create()
                .await
        })
        .expect("Code execution request failed");

        assert!(
            !response.code_execution_calls().is_empty(),
            "no code was executed"
        );
        let output = response
            .successful_code_output()
            .expect("code execution should succeed");
        // A computed value, so a substring check is deterministic.
        assert!(
            output.contains("3628800"),
            "factorial(10) = 3628800, got: {output}"
        );
        assert!(
            response.step_summary().unknown_types.is_empty(),
            "code execution steps should all be typed"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response1 = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Calculate the factorial of 5 using code execution. Return just the number.")
                .with_code_execution()
                .create()
                .await
        })
        .expect("Turn 1 failed");
        assert_eq!(response1.status, InteractionStatus::Completed);
        assert!(
            response1
                .successful_code_output()
                .is_some_and(|o| o.contains("120")),
            "turn 1 should compute 120 in code: {:?}",
            response1.code_execution_results()
        );

        let prev_id = response1.id.clone().expect("id should exist");
        let response2 = retry_request!([client, prev_id] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_text("Multiply the factorial result you just calculated by 2. What is the answer?")
                .with_code_execution()
                .create()
                .await
        })
        .expect("Turn 2 failed");

        assert_eq!(response2.status, InteractionStatus::Completed);
        let computed = response2
            .code_execution_results()
            .iter()
            .any(|r| r.result.contains("240"))
            || response2.as_text().is_some_and(|t| t.contains("240"));
        assert!(computed, "Turn 2 should calculate 120 * 2 = 240");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_code_execution_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("Calculate 15 factorial using Python code execution.")
            .with_code_execution()
            .create_stream();

        let mut streamed_call = false;
        let mut streamed_result = false;
        let mut final_response = None;
        while let Some(result) = stream.next().await {
            match result.expect("stream error").chunk {
                StreamChunk::StepStart { step, .. } => {
                    streamed_call |= matches!(step, Step::CodeExecutionCall { .. });
                    streamed_result |= matches!(step, Step::CodeExecutionResult { .. });
                }
                StreamChunk::StepDelta { delta, .. } => {
                    streamed_call |= matches!(delta, StepDelta::CodeExecutionCall { .. });
                    streamed_result |= matches!(delta, StepDelta::CodeExecutionResult { .. });
                }
                StreamChunk::Completed(response) => final_response = Some(response),
                _ => {}
            }
        }

        assert!(streamed_call, "no code execution call was streamed");
        assert!(streamed_result, "no code execution result was streamed");
        let response = final_response.expect("Should receive complete response");
        let summary = response.step_summary();
        assert!(
            summary.code_execution_call_count > 0 && summary.code_execution_result_count > 0,
            "the accumulated response should carry both steps: {summary:?}"
        );
    }
}

// =============================================================================
// Built-in Tools: URL Context
// =============================================================================

mod url_context {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    fn assert_fetched(response: &InteractionResponse) {
        let results = response.url_context_results();
        assert!(
            results
                .iter()
                .any(|r| r.items.iter().any(|i| i.url.contains("example.com"))),
            "no URL context result for example.com: {:?}",
            response.step_summary()
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_url_context_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result1 = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Fetch and summarize the main content from https://example.com using URL context.")
                .with_url_context()
                .create()
                .await
        });
        let Some(response1) = tool_response(result1, "URL context turn 1") else {
            return;
        };

        assert_eq!(response1.status, InteractionStatus::Completed);
        assert_fetched(&response1);
        let text1 = response1.as_text().expect("turn 1 should answer in text");
        assert_response_semantic(
            &client,
            "Asked to summarize https://example.com (IANA reserved domain for documentation)",
            text1,
            "Does this response describe example.com as a reserved/example domain or mention its illustrative/documentation purpose?",
        )
        .await;

        let prev_id = response1.id.clone().expect("id should exist");
        let result2 = retry_request!([client, prev_id] => {
            stateful_builder(&client)
                .with_previous_interaction(&prev_id)
                .with_text("Is that website a real company or an example domain?")
                .create()
                .await
        });
        let Some(response2) = tool_response(result2, "URL context turn 2") else {
            return;
        };

        let text2 = response2.as_text().expect("turn 2 should answer in text");
        assert_response_semantic(
            &client,
            "Turn 1 fetched example.com. The user asked whether it is a real company or an example domain.",
            text2,
            "Does this response say it is an example/reserved domain rather than a real company?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_url_context_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let mut stream = interaction_builder(&client)
            .with_text("Fetch https://example.com and describe the page structure.")
            .with_url_context()
            .create_stream();

        let mut final_response = None;
        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    if let StreamChunk::Completed(response) = event.chunk {
                        final_response = Some(response);
                    }
                }
                Err(e) if is_safety_block_error(&e) => {
                    println!(
                        "LIVE_TOOL_EVIDENCE_SKIPPED: URL context stream blocked by the content safety filter: {e:?}"
                    );
                    return;
                }
                Err(e) => panic!("stream error: {e:?}"),
            }
        }

        assert_fetched(&final_response.expect("Should receive complete response"));
    }
}

// =============================================================================
// Google Maps
// =============================================================================

mod google_maps {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_google_maps() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = retry_request!([client] => {
            stateful_builder(&client)
                .with_text("Find popular coffee shops near Times Square, New York City")
                .with_google_maps()
                .create()
                .await
        })
        .expect("Google Maps interaction should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(
            response.has_google_maps_results(),
            "Response should contain Google Maps results"
        );
        assert!(
            response.step_summary().google_maps_result_count > 0,
            "Step summary should count Google Maps results"
        );
        let named_places = response
            .google_maps_results()
            .iter()
            .flat_map(|r| r.items)
            .filter_map(|item| item.places.as_ref())
            .flatten()
            .filter(|place| place.name.is_some())
            .count();
        assert!(
            named_places > 0,
            "maps results should name at least one place"
        );
    }
}

// =============================================================================
// Built-in Tools: MCP Server
// =============================================================================

mod mcp_server {
    use super::*;
    use genai_rs::McpServerConfig;

    /// A public MCP server, so this is a real round trip rather than a mock.
    const PUBLIC_MCP_SERVER: &str = "https://mcp.deepwiki.com/mcp";

    /// Remote MCP works on the default model (#265 once recorded it as
    /// unsupported on Gemini 3).
    ///
    /// Loud failures are the ones that are ours: the API rejecting the tool,
    /// or a non-`Completed` status. The two ambiguous outcomes (an error this
    /// guard cannot classify, and a completed turn with no evidence the server
    /// was called) are both what a dead third-party server and a silent
    /// regression look like, so they print `LIVE_TOOL_EVIDENCE_SKIPPED` for
    /// CI to count rather than failing on deepwiki's uptime.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_mcp_server_tool_round_trip() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            .with_text(
                "Using the deepwiki tool, what is the repository structure of \
                 evansenter/genai-rs? Answer in one sentence.",
            )
            .add_tool(McpServerConfig::new("deepwiki", PUBLIC_MCP_SERVER))
            .create()
            .await;

        let response = match result {
            Ok(response) => response,
            Err(e) => {
                let msg = format!("{e:?}").to_lowercase();
                // Both halves required: an availability phrase alone matches
                // any rejection, and `mcp` alone matches the echoed URL.
                let rejected = [
                    "not supported",
                    "unsupported",
                    "not enabled",
                    "not available",
                ]
                .iter()
                .any(|phrase| msg.contains(phrase));
                let about_mcp = msg.contains("mcp_server") || msg.contains("mcp server");
                if about_mcp && rejected {
                    panic!("API rejected the MCP tool — this is a regression: {e:?}");
                }
                println!(
                    "LIVE_TOOL_EVIDENCE_SKIPPED: MCP call failed for an unrecognised \
                     reason (server down, or a rejection this guard did not match): {e:?}"
                );
                return;
            }
        };

        let step_types: Vec<&str> = response
            .steps
            .iter()
            .map(genai_rs::Step::step_type)
            .collect();
        assert_eq!(
            response.status,
            InteractionStatus::Completed,
            "MCP interaction should complete; steps were {step_types:?}"
        );

        // Tool-use tokens are non-zero only if the server was called; the
        // declaration alone costs none (measured 2026-08-16). The API emits
        // generic `tool_call` steps for MCP today.
        let tool_tokens = response.tool_use_tokens().unwrap_or(0);
        let called = tool_tokens > 0
            || step_types.contains(&"tool_call")
            || step_types.contains(&"mcp_server_tool_call");
        if !called {
            println!(
                "LIVE_TOOL_EVIDENCE_SKIPPED: the interaction completed but shows no \
                 evidence the MCP server was called (tool-use tokens 0, steps \
                 {step_types:?})"
            );
        }
    }

    #[test]
    fn test_mcp_server_config_wire_shape() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());

        let tool: Tool = McpServerConfig::new("deepwiki", PUBLIC_MCP_SERVER)
            .with_allowed_tools(vec!["read_wiki_structure".to_string()])
            .with_headers(headers)
            .into();

        let value = serde_json::to_value(&tool).expect("should serialize");
        assert_eq!(value["type"], "mcp_server");
        assert_eq!(value["name"], "deepwiki");
        assert_eq!(value["url"], PUBLIC_MCP_SERVER);
        assert_eq!(value["allowed_tools"][0]["tools"][0], "read_wiki_structure");
        assert_eq!(value["headers"]["Authorization"], "Bearer token");
    }
}

// =============================================================================
// Built-in Tools: Computer Use
// =============================================================================

mod computer_use {
    use super::*;
    use genai_rs::ComputerUseConfig;

    /// Computer use hands an action back (`requires_action` plus a
    /// `function_call` step) instead of answering on its own.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_computer_use_reaches_requires_action() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let result = stateful_builder(&client)
            // Live page state the model cannot know, so answering without the
            // tool is not a plausible outcome (example.com's heading is).
            .with_text(
                "Open https://news.ycombinator.com and tell me the exact title \
                 of the current top story.",
            )
            .add_tool(ComputerUseConfig::new())
            .create()
            .await;

        let response = match result {
            Ok(response) => response,
            Err(e) => {
                let msg = format!("{e:?}").to_lowercase();
                let unavailable = [
                    "permission",
                    "not allowed",
                    "not supported",
                    "not available",
                    "not enabled",
                ]
                .iter()
                .any(|phrase| msg.contains(phrase));
                let about_computer_use =
                    msg.contains("computer_use") || msg.contains("computer use");
                if about_computer_use && unavailable {
                    // Unmarked: a key that is not allowlisted is a stable
                    // account property, like having no key at all.
                    println!("Skipping: computer use not enabled for this key: {e:?}");
                    return;
                }
                panic!("Computer use request failed: {e:?}");
            }
        };

        let step_types: Vec<&str> = response
            .steps
            .iter()
            .map(genai_rs::Step::step_type)
            .collect();
        assert_eq!(
            response.status,
            InteractionStatus::RequiresAction,
            "computer use should hand an action back; steps were {step_types:?}"
        );
        assert!(
            step_types.contains(&"function_call"),
            "expected an action handed back as a function_call step; got {step_types:?}"
        );
    }

    /// Pins the snake_case `excluded_predefined_functions`, once emitted as
    /// camelCase.
    #[test]
    fn test_computer_use_config_wire_shape() {
        let tool: Tool = ComputerUseConfig::new()
            .with_environment("browser")
            .with_excluded_predefined_functions(vec!["submit_form".to_string()])
            .with_prompt_injection_detection(true)
            .into();

        let value = serde_json::to_value(&tool).expect("should serialize");
        assert_eq!(value["type"], "computer_use");
        assert_eq!(value["environment"], "browser");
        assert_eq!(value["excluded_predefined_functions"][0], "submit_form");
        assert_eq!(value["enable_prompt_injection_detection"], true);
    }
}

// =============================================================================
// Response Formats: Structured Output
// =============================================================================

mod structured_output {
    use super::*;
    use futures_util::StreamExt;
    use genai_rs::StreamChunk;

    fn parse_json(response: &InteractionResponse) -> serde_json::Value {
        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        serde_json::from_str(text).unwrap_or_else(|e| panic!("not valid JSON ({e}): {text}"))
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"},
                "email": {"type": "string"}
            },
            "required": ["name", "age", "email"]
        });

        let response = retry_request!([client, schema] => {
            stateful_builder(&client)
                .with_text("Generate a fake user profile with a name, age, and email address.")
                .with_response_format(schema.clone())
                .create()
                .await
        })
        .expect("Structured output request should succeed");

        let json = parse_json(&response);
        assert!(json["name"].is_string(), "{json}");
        assert!(json["age"].is_i64() || json["age"].is_u64(), "{json}");
        assert!(json["email"].is_string(), "{json}");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_enum() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "sentiment": {
                    "type": "string",
                    "enum": ["positive", "negative", "neutral"]
                },
                "confidence": {"type": "number"}
            },
            "required": ["sentiment", "confidence"]
        });

        let response = stateful_builder(&client)
            .with_text("Analyze the sentiment of: 'I love this product, it's amazing!'")
            .with_response_format(schema)
            .create()
            .await
            .expect("Structured output with enum should succeed");

        let json = parse_json(&response);
        let sentiment = json["sentiment"]
            .as_str()
            .expect("sentiment should be a string");
        assert!(
            ["positive", "negative", "neutral"].contains(&sentiment),
            "sentiment '{sentiment}' is outside the enum"
        );
        assert!(json["confidence"].is_number(), "{json}");
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_with_google_search() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "answer": {"type": "string"},
                "source_count": {"type": "integer"}
            },
            "required": ["answer"]
        });

        let response = retry_request!([client, schema] => {
            stateful_builder(&client)
                .with_text("What is the current population of Tokyo, Japan? Use search.")
                .with_google_search()
                .with_response_format(schema)
                .create()
                .await
        })
        .expect("Structured output with Google Search should succeed");

        let json = parse_json(&response);
        assert!(json["answer"].is_string(), "{json}");
        assert!(
            !response.google_search_calls().is_empty(),
            "no search was issued: {:?}",
            response.step_summary()
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_with_url_context() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "description": {"type": "string"},
                "has_navigation": {"type": "boolean"}
            },
            "required": ["title", "description"]
        });

        let result = stateful_builder(&client)
            .with_text("Analyze the page at https://example.com and extract metadata.")
            .with_url_context()
            .with_response_format(schema)
            .create()
            .await;
        let Some(response) = tool_response(result, "URL context with structured output") else {
            return;
        };

        let json = parse_json(&response);
        assert!(json["title"].is_string(), "{json}");
        assert!(json["description"].is_string(), "{json}");
        assert!(
            !response.url_context_results().is_empty(),
            "no URL context result: {:?}",
            response.step_summary()
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_nested() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "company": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "founded": {"type": "integer"}
                    },
                    "required": ["name"]
                },
                "employees": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "role": {"type": "string"}
                        },
                        "required": ["name", "role"]
                    }
                }
            },
            "required": ["company", "employees"]
        });

        let response = stateful_builder(&client)
            .with_text("Generate data for a fictional tech startup called 'CloudAI' founded in 2023 with 3 employees: a CEO, CTO, and designer.")
            .with_response_format(schema)
            .create()
            .await
            .expect("Nested schema structured output should succeed");

        let json = parse_json(&response);
        assert!(json["company"]["name"].is_string(), "{json}");
        let employees = json["employees"]
            .as_array()
            .expect("employees should be an array");
        assert_eq!(employees.len(), 3, "Should have 3 employees");
        for emp in employees {
            assert!(emp["name"].is_string() && emp["role"].is_string(), "{emp}");
        }
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_streaming() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "color": {"type": "string"},
                "hex_code": {"type": "string"},
                "rgb": {
                    "type": "object",
                    "properties": {
                        "r": {"type": "integer"},
                        "g": {"type": "integer"},
                        "b": {"type": "integer"}
                    }
                }
            },
            "required": ["color", "hex_code"]
        });

        let mut stream = interaction_builder(&client)
            .with_text("Describe the color blue with its hex code and RGB values.")
            .with_response_format(schema)
            .create_stream();

        let mut collected_text = String::new();
        let mut final_response = None;
        while let Some(result) = stream.next().await {
            match result.expect("stream error").chunk {
                StreamChunk::StepDelta { delta, .. } => {
                    if let Some(text) = delta.as_text() {
                        collected_text.push_str(text);
                    }
                }
                StreamChunk::Completed(response) => final_response = Some(response),
                _ => {}
            }
        }

        let response = final_response.expect("Should receive complete response");
        let json = parse_json(&response);
        assert_eq!(
            Some(collected_text.as_str()),
            response.as_text(),
            "Streamed chunks should match final response text"
        );
        assert!(
            json["color"].is_string() && json["hex_code"].is_string(),
            "{json}"
        );
    }

    /// A second turn with a different schema can read values from the first.
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_structured_output_multi_turn() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema1 = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            },
            "required": ["name", "age"]
        });

        let response1 = stateful_builder(&client)
            .with_text("Create a user profile for a software developer. Choose any name and age you like. Output as JSON.")
            .with_response_format(schema1)
            .create()
            .await
            .expect("Turn 1 should succeed");

        let json1 = parse_json(&response1);
        let original_name = json1["name"].as_str().expect("Should have name");
        let original_age = json1["age"].as_i64().expect("Should have age");

        let schema2 = json!({
            "type": "object",
            "properties": {
                "original_name": {"type": "string"},
                "original_age": {"type": "integer"},
                "email": {"type": "string"},
                "occupation": {"type": "string"}
            },
            "required": ["original_name", "original_age", "email", "occupation"]
        });

        let response2 = stateful_builder(&client)
            .with_previous_interaction(response1.id.as_ref().expect("id should exist"))
            .with_text("Based on the user profile you just created, output a new JSON with the original name and age, plus add an email address and occupation that fits the profile.")
            .with_response_format(schema2)
            .create()
            .await
            .expect("Turn 2 should succeed");

        let json2 = parse_json(&response2);
        assert!(
            json2["original_name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(original_name)),
            "Turn 2 should preserve name '{original_name}': {json2}"
        );
        assert_eq!(
            json2["original_age"].as_i64(),
            Some(original_age),
            "{json2}"
        );
        assert!(
            json2["email"].as_str().is_some_and(|e| e.contains('@')),
            "{json2}"
        );
        assert!(
            json2["occupation"].as_str().is_some_and(|o| !o.is_empty()),
            "{json2}"
        );
    }
}

// =============================================================================
// Response Modalities: Image Generation
// =============================================================================

mod image_generation {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_response_modalities_image() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = retry_request!([client] => {
            client
                .interaction()
                .with_model(genai_rs::DEFAULT_IMAGE_MODEL)
                .with_text("Generate a simple image of a red circle on a white background.")
                .with_response_modalities(vec!["image".to_string()])
                .with_store_enabled()
                .create()
                .await
        })
        .expect("Image generation request failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let bytes = response
            .first_image_bytes()
            .expect("image data should decode")
            .expect("response should contain an image");
        assert!(!bytes.is_empty(), "decoded image is empty");
    }
}

// =============================================================================
// Generation Config: Thinking Level
// =============================================================================

mod thinking {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_thinking_level_minimal() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            max_output_tokens: Some(2000),
            thinking_level: Some(ThinkingLevel::Minimal),
            ..Default::default()
        };

        let response = retry_request!([client, config] => {
            stateful_builder(&client)
                .with_model(genai_rs::MINIMAL_THINKING_MODEL)
                .with_text("What is 2 + 2?")
                .with_generation_config(config)
                .create()
                .await
        })
        .expect("Minimal thinking interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        // A computed value, so a substring check is deterministic.
        assert!(
            text.contains('4'),
            "Should contain the answer 4. Got: {text}"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_thinking_level_high() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            // Thinking draws from this budget; High can spend over 1000 alone.
            max_output_tokens: Some(8000),
            thinking_level: Some(ThinkingLevel::High),
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("Explain step by step how to solve: If x + 3 = 7, what is x?")
            .with_generation_config(config)
            .create()
            .await
            .expect("High thinking interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        assert_response_semantic(
            &client,
            "Asked to solve x + 3 = 7 step by step",
            text,
            "Does this response conclude that x = 4?",
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_thinking_summaries() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = stateful_builder(&client)
            .with_text("What is the capital of France?")
            .with_thinking_level(ThinkingLevel::Medium)
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .create()
            .await
            .expect("Thinking with summaries request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(
            response.has_thoughts(),
            "summaries were requested but no thought came back"
        );
        assert!(response.has_text(), "Should have text response");
    }
}

// =============================================================================
// Generation Config: Sampling
// =============================================================================

mod sampling {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_top_p() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            temperature: Some(1.0),
            // Headroom: ~100 thinking tokens even on this prompt.
            max_output_tokens: Some(2000),
            top_p: Some(0.1),
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("What is the capital of France? Answer in one word.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Top-p interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        assert_response_semantic(
            &client,
            "Asked for the capital of France in one word",
            text,
            "Does this response identify Paris as the capital of France?",
        )
        .await;
    }

    /// Several sampling knobs together are accepted. (`top_k` is not part of
    /// the Interactions API since revision 2026-05-20.)
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_combined() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let config = GenerationConfig {
            temperature: Some(0.5),
            max_output_tokens: Some(2048),
            top_p: Some(0.9),
            thinking_level: Some(ThinkingLevel::Medium),
            ..Default::default()
        };

        let response = stateful_builder(&client)
            .with_text("Write a haiku about programming.")
            .with_generation_config(config)
            .create()
            .await
            .expect("Combined config interaction failed");

        assert_eq!(response.status, InteractionStatus::Completed);
        assert!(response.has_text(), "Should have text response");
    }
}

// =============================================================================
// Generation Config: stop sequences, seed with a response format
// =============================================================================

mod config_fields {
    use super::*;

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_stop_sequences() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let response = interaction_builder(&client)
            .with_text("Count from 1 to 10, one number per line. Output only the numbers.")
            .with_stop_sequences(vec!["5".to_string()])
            .create()
            .await
            .expect("Stop sequences request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        let numbers: Vec<&str> = text.split_whitespace().collect();
        assert!(
            !numbers
                .iter()
                .any(|n| ["6", "7", "8", "9", "10"].contains(n)),
            "generation should stop at '5', got: {text}"
        );
    }

    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_generation_config_seed_with_response_format() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["items"]
        });

        let response = interaction_builder(&client)
            .with_text("List 3 colors as a JSON array.")
            .with_seed(42)
            .with_response_format(schema)
            .create()
            .await
            .expect("seed + response_format request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("Should have text response");
        let parsed: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
        let items = parsed["items"].as_array().expect("Should have items array");
        assert_eq!(items.len(), 3, "{parsed}");
    }
}

// =============================================================================
// Tool Configuration: Function Calling Modes
// =============================================================================

mod function_calling_modes {
    use super::*;

    /// Validated mode is accepted and yields either a schema-bound call or
    /// text. (`FunctionCallingMode::Any` is covered in
    /// `function_calling_tests.rs`.)
    #[tokio::test]
    #[ignore = "Requires API key"]
    async fn test_function_calling_validated_mode() {
        let Some(client) = get_client() else {
            println!("Skipping: GEMINI_API_KEY not set");
            return;
        };

        let weather_fn = FunctionDeclaration::builder("get_weather")
            .with_description("Get the current weather for a location")
            .add_parameter(
                "location",
                json!({"type": "string", "description": "The city name"}),
            )
            .with_required(vec!["location".to_string()])
            .build();

        let response = retry_request!([client, weather_fn] => {
            interaction_builder(&client)
                .with_text("What's the weather like in Tokyo?")
                .add_functions(vec![weather_fn])
                .with_function_calling_mode(FunctionCallingMode::Validated)
                .create()
                .await
        })
        .expect("validated mode should be accepted");

        match response.function_calls().first() {
            Some(call) => {
                assert_eq!(call.name, "get_weather");
                assert!(call.args["location"].is_string(), "args: {}", call.args);
            }
            None => assert!(
                response.has_text(),
                "validated mode produced neither a call nor text"
            ),
        }
    }
}
