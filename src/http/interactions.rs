use super::common::{API_KEY_HEADER, Endpoint, construct_endpoint_url};
use super::error_helpers::{check_response, deserialize_with_context};
use super::loud_wire;
use super::sse_parser::parse_sse_stream;
use crate::errors::GenaiError;
use crate::{
    Content, InteractionRequest, InteractionResponse, InteractionStreamEvent, StreamChunk,
    StreamEvent,
};
use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
use reqwest::Client as ReqwestClient;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Creates a new interaction with the Gemini API.
///
/// This is the unified interface for interacting with both models and agents.
/// Supports function calling, structured outputs, and more.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request fails
/// - The response status is not successful
/// - The response cannot be parsed as JSON
pub async fn create_interaction(
    http_client: &ReqwestClient,
    api_key: &str,
    request: InteractionRequest,
) -> Result<InteractionResponse, GenaiError> {
    let endpoint = Endpoint::CreateInteraction { stream: false };
    let url = construct_endpoint_url(endpoint);

    // LOUD_WIRE: Log outgoing request
    let request_id = loud_wire::next_request_id();
    let request_body = match serde_json::to_string(&request) {
        Ok(body) => Some(body),
        Err(e) => {
            tracing::warn!("LOUD_WIRE: Failed to serialize request body: {}", e);
            None
        }
    };
    loud_wire::log_request(request_id, "POST", &url, request_body.as_deref());

    let response = http_client
        .post(&url)
        .header(API_KEY_HEADER, api_key)
        .json(&request)
        .send()
        .await?;

    // LOUD_WIRE: Log response status
    loud_wire::log_response_status(request_id, response.status().as_u16());

    let response = check_response(response).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;

    // LOUD_WIRE: Log response body
    loud_wire::log_response_body(request_id, &response_text);

    let interaction_response: InteractionResponse =
        deserialize_with_context(&response_text, "InteractionResponse from create")?;

    Ok(interaction_response)
}

/// Creates a new interaction with streaming responses.
///
/// Returns a stream of `StreamEvent` items as they arrive from the server.
/// Each event contains:
/// - `chunk`: The content (Start, StatusUpdate, Delta, Complete, Error, etc.)
/// - `event_id`: An identifier for stream resumption
///
/// Chunk types:
/// - `StreamChunk::Start`: Initial event with interaction ID
/// - `StreamChunk::StatusUpdate`: Status changes during processing
/// - `StreamChunk::ContentStart`: Content generation begins for an output
/// - `StreamChunk::Delta`: Incremental content (text, thought, function_call)
/// - `StreamChunk::ContentStop`: Content generation ends for an output
/// - `StreamChunk::Complete`: The final complete interaction response
/// - `StreamChunk::Error`: Error occurred during streaming
///
/// # Example
/// ```ignore
/// let mut last_event_id = None;
/// let stream = create_interaction_stream(&client, &api_key, request);
/// while let Some(event) = stream.next().await {
///     let event = event?;
///     last_event_id = event.event_id.clone();  // Track for resume
///     match event.chunk {
///         StreamChunk::Start { interaction } => {
///             println!("Started: {:?}", interaction.id);
///         }
///         StreamChunk::Delta(delta) => {
///             if let Some(text) = delta.as_text() {
///                 print!("{}", text);
///             }
///         }
///         StreamChunk::Complete(response) => {
///             println!("\nComplete: {} tokens", response.usage.map(|u| u.total_tokens).flatten().unwrap_or(0));
///         }
///         StreamChunk::Error { message, .. } => {
///             eprintln!("Error: {}", message);
///         }
///         _ => {} // Handle other event types as needed
///     }
/// }
/// ```
pub fn create_interaction_stream<'a>(
    http_client: &'a ReqwestClient,
    api_key: &'a str,
    request: InteractionRequest,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + 'a {
    let endpoint = Endpoint::CreateInteraction { stream: true };
    let url = construct_endpoint_url(endpoint);

    // LOUD_WIRE: Log outgoing request (before try_stream! to capture request_id)
    let request_id = loud_wire::next_request_id();
    let request_body = match serde_json::to_string(&request) {
        Ok(body) => Some(body),
        Err(e) => {
            tracing::warn!("LOUD_WIRE: Failed to serialize request body: {}", e);
            None
        }
    };
    loud_wire::log_request(request_id, "POST (stream)", &url, request_body.as_deref());

    try_stream! {
        // Accumulate content from deltas to include in Complete response.
        // In streaming, the API sends content via delta events, but the final
        // interaction.complete event has empty outputs. We accumulate here so
        // that response.function_calls() and response.as_text() work consistently.
        //
        // We track content by index because content can arrive in multiple chunks
        // that need to be merged (e.g., text fragments, function call arguments).
        let mut content_by_index: HashMap<usize, Content> = HashMap::new();

        let response = http_client
            .post(&url)
            .header(API_KEY_HEADER, api_key)
            .json(&request)
            .send()
            .await?;

        // LOUD_WIRE: Log response status
        loud_wire::log_response_status(request_id, response.status().as_u16());

        let response = check_response(response).await?;
        let byte_stream = response.bytes_stream();
        let parsed_stream = parse_sse_stream::<InteractionStreamEvent>(byte_stream, request_id);
        futures_util::pin_mut!(parsed_stream);

        while let Some(result) = parsed_stream.next().await {
            let event = result?;
            debug!(
                "SSE event received: event_type={:?}, has_delta={}, has_interaction={}, event_id={:?}",
                event.event_type,
                event.delta.is_some(),
                event.interaction.is_some(),
                event.event_id
            );

            // Extract event_id for the StreamEvent wrapper
            let event_id = event.event_id.clone();

            // Handle different event types from the Interactions API:
            // - interaction.start: Initial event with interaction data (yields Start)
            // - interaction.status_update: Status changes (yields StatusUpdate)
            // - content.start: Content block begins (yields ContentStart)
            // - content.delta: Incremental content updates (yields Delta)
            // - content.stop: Content block ends (yields ContentStop)
            // - interaction.complete: Final response (yields Complete)
            // - error: Error occurred (yields Error)
            match event.event_type.as_str() {
                "interaction.start" => {
                    // Interaction has started - provides early access to interaction ID
                    if let Some(interaction) = event.interaction {
                        yield StreamEvent::new(StreamChunk::Start { interaction }, event_id);
                    } else {
                        warn!("interaction.start event missing interaction field - event dropped");
                    }
                }
                "interaction.status_update" => {
                    // Status change during processing
                    match (event.interaction_id, event.status) {
                        (Some(interaction_id), Some(status)) => {
                            yield StreamEvent::new(
                                StreamChunk::StatusUpdate { interaction_id, status },
                                event_id,
                            );
                        }
                        (has_id, has_status) => {
                            warn!(
                                "interaction.status_update missing required fields: interaction_id={}, status={} - event dropped",
                                has_id.is_some(),
                                has_status.is_some()
                            );
                        }
                    }
                }
                "content.start" => {
                    // Content generation begins
                    if let Some(index) = event.index {
                        // Try to get content type from the content field if present
                        let content_type = event.content.as_ref().and_then(|c| {
                            // Get the content type name from the variant
                            match c {
                                Content::Text { .. } => Some("text".to_string()),
                                Content::Thought { .. } => Some("thought".to_string()),
                                Content::FunctionCall { .. } => Some("function_call".to_string()),
                                Content::FunctionResult { .. } => Some("function_result".to_string()),
                                Content::CodeExecutionCall { .. } => Some("code_execution_call".to_string()),
                                Content::CodeExecutionResult { .. } => Some("code_execution_result".to_string()),
                                Content::GoogleSearchCall { .. } => Some("google_search_call".to_string()),
                                Content::GoogleSearchResult { .. } => Some("google_search_result".to_string()),
                                Content::UrlContextCall { .. } => Some("url_context_call".to_string()),
                                Content::UrlContextResult { .. } => Some("url_context_result".to_string()),
                                Content::Unknown { content_type, .. } => Some(content_type.clone()),
                                _ => None,
                            }
                        });
                        yield StreamEvent::new(StreamChunk::ContentStart { index, content_type }, event_id);
                    } else {
                        warn!("content.start event missing index field - event dropped");
                    }
                }
                "content.delta" => {
                    // Incremental content update
                    if let Some(delta) = event.delta.clone() {
                        // Accumulate content for the Complete response by merging
                        // deltas at the same index. This ensures response.function_calls()
                        // and response.as_text() work in streaming mode.
                        if let Some(index) = event.index {
                            merge_content_at_index(&mut content_by_index, index, delta.clone());
                        }
                        yield StreamEvent::new(StreamChunk::Delta(delta), event_id);
                    } else {
                        warn!("content.delta event missing delta field - event dropped");
                    }
                }
                "content.stop" => {
                    // Content generation ends
                    if let Some(index) = event.index {
                        yield StreamEvent::new(StreamChunk::ContentStop { index }, event_id);
                    } else {
                        warn!("content.stop event missing index field - event dropped");
                    }
                }
                "interaction.complete" => {
                    // Final complete response
                    if let Some(mut interaction) = event.interaction {
                        // Merge accumulated delta content into outputs.
                        // The API's Complete event has empty outputs in streaming mode,
                        // but users expect response.function_calls() and as_text() to work.
                        if !content_by_index.is_empty() {
                            // Sort by index to maintain order
                            let mut outputs: Vec<_> = content_by_index.drain().collect();
                            outputs.sort_by_key(|(idx, _)| *idx);
                            interaction.outputs = outputs.into_iter().map(|(_, c)| c).collect();
                        }
                        yield StreamEvent::new(StreamChunk::Complete(interaction), event_id);
                    } else {
                        warn!("interaction.complete event missing interaction field - event dropped");
                    }
                }
                "error" => {
                    // Error occurred during streaming
                    if let Some(error) = event.error {
                        yield StreamEvent::new(
                            StreamChunk::Error { message: error.message, code: error.code },
                            event_id,
                        );
                    } else {
                        // If no error object, treat as unknown error
                        yield StreamEvent::new(
                            StreamChunk::Error { message: "Unknown streaming error".to_string(), code: None },
                            event_id,
                        );
                    }
                }
                _ => {
                    // For unknown event types, only yield if they have delta content.
                    // Do NOT yield Complete for other event types that happen to have
                    // an interaction field - only interaction.complete is the final response.
                    if let Some(delta) = event.delta {
                        yield StreamEvent::new(StreamChunk::Delta(delta), event_id);
                    } else if event.interaction.is_some() {
                        // Warn about unknown event types with interaction fields - this could
                        // indicate API version drift that needs attention
                        warn!(
                            "Unknown event type '{}' has interaction field but is not 'interaction.complete' - skipping",
                            event.event_type
                        );
                    }
                    // Skip events without useful content
                }
            }
        }
    }
}

/// Retrieves an existing interaction by its ID.
///
/// Useful for checking the status of long-running interactions or agents,
/// or for retrieving the full conversation history.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request fails
/// - The response status is not successful
/// - The response cannot be parsed as JSON
pub async fn get_interaction(
    http_client: &ReqwestClient,
    api_key: &str,
    interaction_id: &str,
    include_input: bool,
) -> Result<InteractionResponse, GenaiError> {
    let endpoint = Endpoint::GetInteraction {
        id: interaction_id,
        stream: false,
        last_event_id: None,
        include_input,
    };
    let url = construct_endpoint_url(endpoint);

    // LOUD_WIRE: Log outgoing request
    let request_id = loud_wire::next_request_id();
    loud_wire::log_request(request_id, "GET", &url, None);

    let response = http_client
        .get(&url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .await?;

    // LOUD_WIRE: Log response status
    loud_wire::log_response_status(request_id, response.status().as_u16());

    let response = check_response(response).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;

    // LOUD_WIRE: Log response body
    loud_wire::log_response_body(request_id, &response_text);

    let interaction_response: InteractionResponse =
        deserialize_with_context(&response_text, "InteractionResponse from get")?;

    Ok(interaction_response)
}

/// Retrieves an existing interaction by its ID with streaming.
///
/// Returns a stream of `StreamEvent` items as they arrive from the server.
/// This is useful for:
/// - Resuming an interrupted stream using `last_event_id`
/// - Streaming a long-running interaction's progress (e.g., deep research)
///
/// Each event includes an `event_id` that can be used to resume the stream
/// from that point if the connection is interrupted.
///
/// # Arguments
///
/// * `http_client` - The reqwest HTTP client
/// * `api_key` - The Gemini API key
/// * `interaction_id` - The ID of the interaction to stream
/// * `last_event_id` - Optional event ID to resume from (for stream resumption)
/// * `include_input` - Whether to include the original input in the response
///
/// # Example
/// ```ignore
/// // Resume a stream after interruption
/// let mut stream = get_interaction_stream(&client, &api_key, &id, Some("evt_abc123"), false);
/// while let Some(event) = stream.next().await {
///     let event = event?;
///     println!("Received chunk: {:?}", event.chunk);
///     // Track event_id for potential future resume
///     if let Some(evt_id) = &event.event_id {
///         last_event_id = Some(evt_id.clone());
///     }
/// }
/// ```
pub fn get_interaction_stream<'a>(
    http_client: &'a ReqwestClient,
    api_key: &'a str,
    interaction_id: &'a str,
    last_event_id: Option<&'a str>,
    include_input: bool,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + 'a {
    let endpoint = Endpoint::GetInteraction {
        id: interaction_id,
        stream: true,
        last_event_id,
        include_input,
    };
    let url = construct_endpoint_url(endpoint);

    // LOUD_WIRE: Log outgoing request
    let request_id = loud_wire::next_request_id();
    let resume_info = last_event_id
        .map(|id| format!(" (resuming from {})", id))
        .unwrap_or_default();
    loud_wire::log_request(
        request_id,
        &format!("GET (stream){}", resume_info),
        &url,
        None,
    );

    try_stream! {
        // Accumulate content by index (same as create_interaction_stream)
        let mut content_by_index: HashMap<usize, Content> = HashMap::new();

        let response = http_client
            .get(&url)
            .header(API_KEY_HEADER, api_key)
            .send()
            .await?;

        // LOUD_WIRE: Log response status
        loud_wire::log_response_status(request_id, response.status().as_u16());

        let response = check_response(response).await?;
        let byte_stream = response.bytes_stream();
        let parsed_stream = parse_sse_stream::<InteractionStreamEvent>(byte_stream, request_id);
        futures_util::pin_mut!(parsed_stream);

        while let Some(result) = parsed_stream.next().await {
            let event = result?;
            debug!(
                "SSE event received: event_type={:?}, has_delta={}, has_interaction={}, event_id={:?}",
                event.event_type,
                event.delta.is_some(),
                event.interaction.is_some(),
                event.event_id
            );

            // Extract event_id for the StreamEvent wrapper
            let event_id = event.event_id.clone();

            // Handle different event types (same logic as create_interaction_stream)
            match event.event_type.as_str() {
                "content.delta" => {
                    if let Some(delta) = event.delta.clone() {
                        if let Some(index) = event.index {
                            merge_content_at_index(&mut content_by_index, index, delta.clone());
                        }
                        yield StreamEvent::new(StreamChunk::Delta(delta), event_id);
                    }
                }
                "interaction.complete" => {
                    if let Some(mut interaction) = event.interaction {
                        if !content_by_index.is_empty() {
                            let mut outputs: Vec<_> = content_by_index.drain().collect();
                            outputs.sort_by_key(|(idx, _)| *idx);
                            interaction.outputs = outputs.into_iter().map(|(_, c)| c).collect();
                        }
                        yield StreamEvent::new(StreamChunk::Complete(interaction), event_id);
                    }
                }
                "interaction.start" | "interaction.status_update" | "content.start" | "content.stop" => {
                    // Known lifecycle events - skip silently
                }
                _ => {
                    if let Some(delta) = event.delta {
                        yield StreamEvent::new(StreamChunk::Delta(delta), event_id);
                    } else if event.interaction.is_some() {
                        warn!(
                            "Unknown event type '{}' has interaction field but is not 'interaction.complete' - skipping",
                            event.event_type
                        );
                    }
                }
            }
        }
    }
}

/// Deletes an interaction by its ID.
///
/// Removes the interaction from the server, freeing up storage and making it
/// unavailable for future reference via `previous_interaction_id`.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request fails
/// - The response status is not successful
pub async fn delete_interaction(
    http_client: &ReqwestClient,
    api_key: &str,
    interaction_id: &str,
) -> Result<(), GenaiError> {
    let endpoint = Endpoint::DeleteInteraction { id: interaction_id };
    let url = construct_endpoint_url(endpoint);

    // LOUD_WIRE: Log outgoing request
    let request_id = loud_wire::next_request_id();
    loud_wire::log_request(request_id, "DELETE", &url, None);

    let response = http_client
        .delete(&url)
        .header(API_KEY_HEADER, api_key)
        .send()
        .await?;

    // LOUD_WIRE: Log response status
    loud_wire::log_response_status(request_id, response.status().as_u16());

    check_response(response).await?;
    Ok(())
}

/// Cancels a background interaction by its ID.
///
/// Halts an in-progress background interaction. Only applicable to interactions
/// created with `background: true` that are still in `InProgress` status.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request fails
/// - The response status is not successful
/// - The response cannot be parsed as JSON
/// - The interaction is not in a cancellable state
pub async fn cancel_interaction(
    http_client: &ReqwestClient,
    api_key: &str,
    interaction_id: &str,
) -> Result<InteractionResponse, GenaiError> {
    let endpoint = Endpoint::CancelInteraction { id: interaction_id };
    let url = construct_endpoint_url(endpoint);

    // LOUD_WIRE: Log outgoing request
    let request_id = loud_wire::next_request_id();
    loud_wire::log_request(request_id, "POST", &url, Some("{}"));

    // Send empty JSON body - the API requires Content-Length header
    let response = http_client
        .post(&url)
        .header(API_KEY_HEADER, api_key)
        .json(&serde_json::json!({}))
        .send()
        .await?;

    // LOUD_WIRE: Log response status
    loud_wire::log_response_status(request_id, response.status().as_u16());

    let response = check_response(response).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;

    // LOUD_WIRE: Log response body
    loud_wire::log_response_body(request_id, &response_text);

    let interaction_response: InteractionResponse =
        deserialize_with_context(&response_text, "InteractionResponse from cancel")?;

    Ok(interaction_response)
}

/// Merges a delta Content into the accumulated content at a given index.
///
/// For text content, this concatenates the text. For function calls, arguments
/// are merged as JSON string chunks. For other content types, the delta replaces
/// any existing content.
fn merge_content_at_index(content_map: &mut HashMap<usize, Content>, index: usize, delta: Content) {
    match content_map.get_mut(&index) {
        Some(existing) => {
            // Merge based on content type
            match (existing, &delta) {
                // Text content: concatenate the text
                (
                    Content::Text {
                        text: Some(existing_text),
                        ..
                    },
                    Content::Text {
                        text: Some(delta_text),
                        ..
                    },
                ) => {
                    existing_text.push_str(delta_text);
                }
                // FunctionCall: merge name and arguments as they stream in
                (
                    Content::FunctionCall {
                        name: existing_name,
                        args: existing_args,
                        ..
                    },
                    Content::FunctionCall {
                        name: delta_name,
                        args: delta_args,
                        ..
                    },
                ) => {
                    // Name is a String, concatenate if delta has content
                    if !delta_name.is_empty() {
                        existing_name.push_str(delta_name);
                    }
                    // Args: if both are strings, concatenate the JSON string chunks
                    if let (serde_json::Value::String(es), serde_json::Value::String(ds)) =
                        (existing_args, delta_args)
                    {
                        es.push_str(ds);
                    }
                    // If delta args isn't a string, just keep existing
                }
                // For other types, just replace
                _ => {
                    content_map.insert(index, delta);
                }
            }
        }
        None => {
            // No existing content at this index, just insert
            content_map.insert(index, delta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Content, InteractionInput, InteractionStatus};

    #[test]
    fn test_endpoint_url_construction() {
        // Test that we can construct proper URLs for each endpoint
        // API key is now passed via header, not in URL
        let endpoint_create = Endpoint::CreateInteraction { stream: false };
        let url = construct_endpoint_url(endpoint_create);
        assert!(url.contains("/v1beta/interactions"));
        assert!(!url.contains("key=")); // API key should not be in URL

        let endpoint_get = Endpoint::GetInteraction {
            id: "test_id_123",
            stream: false,
            last_event_id: None,
            include_input: false,
        };
        let url = construct_endpoint_url(endpoint_get);
        assert!(url.contains("/v1beta/interactions/test_id_123"));
        assert!(!url.contains("key=")); // API key should not be in URL

        let endpoint_delete = Endpoint::DeleteInteraction { id: "test_id_456" };
        let url = construct_endpoint_url(endpoint_delete);
        assert!(url.contains("/v1beta/interactions/test_id_456"));
        assert!(!url.contains("key=")); // API key should not be in URL

        let endpoint_cancel = Endpoint::CancelInteraction { id: "test_id_789" };
        let url = construct_endpoint_url(endpoint_cancel);
        assert!(url.contains("/v1beta/interactions/test_id_789/cancel"));
        assert!(!url.contains("key=")); // API key should not be in URL
    }

    #[test]
    fn test_create_interaction_request_serialization() {
        // Verify request serialization works correctly
        let request = InteractionRequest {
            model: Some("gemini-3-flash-preview".to_string()),
            agent: None,
            agent_config: None,
            input: InteractionInput::Text("Hello".to_string()),
            previous_interaction_id: None,
            tools: None,
            response_modalities: None,
            response_format: None,
            response_mime_type: None,
            generation_config: None,
            stream: None,
            background: None,
            store: None,
            system_instruction: None,
        };

        let json = serde_json::to_string(&request).expect("Serialization should work");
        assert!(json.contains("gemini-3-flash-preview"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_interaction_response_deserialization() {
        // Verify we can deserialize a typical response
        let response_json = r#"{
            "id": "test_interaction_123",
            "model": "gemini-3-flash-preview",
            "input": [{"type": "text", "text": "Hello"}],
            "outputs": [{"type": "text", "text": "Hi there!"}],
            "status": "completed"
        }"#;

        let response: InteractionResponse =
            serde_json::from_str(response_json).expect("Deserialization should work");

        assert_eq!(response.id.as_deref(), Some("test_interaction_123"));
        assert_eq!(response.status, InteractionStatus::Completed);
        assert_eq!(response.outputs.len(), 1);

        // Verify we can access the text content
        match &response.outputs[0] {
            Content::Text { text, .. } => {
                assert_eq!(text.as_ref().unwrap(), "Hi there!")
            }
            _ => panic!("Expected Text content"),
        }
    }

    #[test]
    fn test_cancelled_interaction_response_deserialization() {
        // Verify we can deserialize a cancelled interaction response
        let response_json = r#"{
            "id": "cancelled_interaction_123",
            "model": "deep-research-pro-preview-12-2025",
            "input": [{"type": "text", "text": "Research topic"}],
            "outputs": [],
            "status": "cancelled"
        }"#;

        let response: InteractionResponse =
            serde_json::from_str(response_json).expect("Deserialization should work");

        assert_eq!(response.id.as_deref(), Some("cancelled_interaction_123"));
        assert_eq!(response.status, InteractionStatus::Cancelled);
        assert!(response.outputs.is_empty());
    }
}
