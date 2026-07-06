use super::common::{
    API_KEY_HEADER, API_REVISION, API_REVISION_HEADER, Endpoint, construct_endpoint_url,
};
use super::context::HttpContext;
use super::error_helpers::{check_response_wire, deserialize_with_context};
use super::sse_parser::parse_sse_stream;
use crate::errors::GenaiError;
use crate::steps::StepAccumulator;
use crate::wire::WireEvent;
use crate::{
    InteractionRequest, InteractionResponse, InteractionStreamEvent, StreamChunk, StreamEvent,
};
use async_stream::try_stream;
use futures_util::{Stream, StreamExt};
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
    ctx: &HttpContext,
    request: InteractionRequest,
) -> Result<InteractionResponse, GenaiError> {
    let endpoint = Endpoint::CreateInteraction { stream: false };
    let url = construct_endpoint_url(endpoint);

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, "POST", &url, ctx.serialize_wire_body(&request));

    let response = ctx
        .http_client
        .post(&url)
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION)
        .json(&request)
        .send()
        .await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    let response = check_response_wire(response, ctx, request_id).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;

    ctx.emit_response_body(request_id, &response_text);

    let interaction_response: InteractionResponse =
        deserialize_with_context(&response_text, "InteractionResponse from create")?;

    Ok(interaction_response)
}

/// Dispatches one parsed SSE event to `StreamChunk`s, updating the step
/// accumulator. Shared by the create and get streaming paths.
///
/// Returns `Some(chunk)` for events that should be surfaced to the consumer;
/// `None` for events that were dropped (with a warning).
fn dispatch_stream_event(
    event: InteractionStreamEvent,
    accumulator: &mut StepAccumulator,
) -> Option<StreamChunk> {
    match event.event_type.as_str() {
        "interaction.created" => {
            // Interaction accepted - provides early access to interaction ID
            if let Some(interaction) = event.interaction {
                Some(StreamChunk::Created { interaction })
            } else {
                warn!("interaction.created event missing interaction field - event dropped");
                None
            }
        }
        "interaction.status_update" => match (event.interaction_id, event.status) {
            (Some(interaction_id), Some(status)) => Some(StreamChunk::StatusUpdate {
                interaction_id,
                status,
            }),
            (has_id, has_status) => {
                warn!(
                    "interaction.status_update missing required fields: interaction_id={}, status={} - event dropped",
                    has_id.is_some(),
                    has_status.is_some()
                );
                None
            }
        },
        "step.start" => match (event.index, event.step) {
            (Some(index), Some(step)) => {
                accumulator.start(index, step.clone());
                Some(StreamChunk::StepStart { index, step })
            }
            (index, step) => {
                warn!(
                    "step.start missing required fields: index={}, step={} - event dropped",
                    index.is_some(),
                    step.is_some()
                );
                None
            }
        },
        "step.delta" => match (event.index, event.delta) {
            (Some(index), Some(delta)) => {
                accumulator.apply_delta(index, &delta);
                Some(StreamChunk::StepDelta { index, delta })
            }
            (index, delta) => {
                warn!(
                    "step.delta missing required fields: index={}, delta={} - event dropped",
                    index.is_some(),
                    delta.is_some()
                );
                None
            }
        },
        "step.stop" => {
            if let Some(index) = event.index {
                accumulator.stop(index);
                Some(StreamChunk::StepStop {
                    index,
                    usage: event.usage,
                    step_usage: event.step_usage,
                })
            } else {
                warn!("step.stop event missing index field - event dropped");
                None
            }
        }
        "interaction.completed" => {
            if let Some(mut interaction) = event.interaction {
                // The lifecycle payload may omit steps (streaming already
                // delivered them incrementally). Fill them in from the
                // accumulator so response.function_calls() / as_text() work.
                if interaction.steps.is_empty() && !accumulator.is_empty() {
                    interaction.steps = std::mem::take(accumulator).finish();
                }
                // Total usage may arrive via event metadata instead of the
                // partial interaction payload.
                if interaction.usage.is_none()
                    && let Some(metadata) = event.metadata
                {
                    interaction.usage = metadata.total_usage;
                }
                Some(StreamChunk::Completed(interaction))
            } else {
                warn!("interaction.completed event missing interaction field - event dropped");
                None
            }
        }
        "error" => {
            // Error occurred during streaming
            if let Some(error) = event.error {
                Some(StreamChunk::Error {
                    message: error.message,
                    code: error.code,
                })
            } else {
                // If no error object, treat as unknown error
                Some(StreamChunk::Error {
                    message: "Unknown streaming error".to_string(),
                    code: None,
                })
            }
        }
        other => {
            debug!(
                "Unknown SSE event type '{}' - preserving as StreamChunk::Unknown",
                other
            );
            Some(StreamChunk::Unknown {
                chunk_type: other.to_string(),
                data: event.raw,
            })
        }
    }
}

/// Creates a new interaction with streaming responses.
///
/// Returns a stream of `StreamEvent` items as they arrive from the server.
/// Each event contains:
/// - `chunk`: The content (Created, StatusUpdate, StepStart, StepDelta, StepStop, Completed, Error, ...)
/// - `event_id`: An identifier for stream resumption
///
/// Chunk types (API revision 2026-05-20 lifecycle):
/// - `StreamChunk::Created`: Initial event (`interaction.created`) with interaction ID
/// - `StreamChunk::StatusUpdate`: Status changes during processing
/// - `StreamChunk::StepStart`: A step begins at an index
/// - `StreamChunk::StepDelta`: Incremental step payload (text, arguments_delta, thought_signature, ...)
/// - `StreamChunk::StepStop`: A step finished (carries per-step usage)
/// - `StreamChunk::Completed`: The final complete interaction response
/// - `StreamChunk::Error`: Error occurred during streaming
///
/// # Example
/// ```ignore
/// let mut last_event_id = None;
/// let stream = create_interaction_stream(&ctx, request);
/// while let Some(event) = stream.next().await {
///     let event = event?;
///     last_event_id = event.event_id.clone();  // Track for resume
///     match event.chunk {
///         StreamChunk::Created { interaction } => {
///             println!("Started: {:?}", interaction.id);
///         }
///         StreamChunk::StepDelta { delta, .. } => {
///             if let Some(text) = delta.as_text() {
///                 print!("{}", text);
///             }
///         }
///         StreamChunk::Completed(response) => {
///             println!("\nComplete: {:?} tokens", response.total_tokens());
///         }
///         StreamChunk::Error { message, .. } => {
///             eprintln!("Error: {}", message);
///         }
///         _ => {} // Handle other event types as needed
///     }
/// }
/// ```
pub fn create_interaction_stream<'a>(
    ctx: &'a HttpContext,
    request: InteractionRequest,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + 'a {
    let endpoint = Endpoint::CreateInteraction { stream: true };
    let url = construct_endpoint_url(endpoint);

    // Emit the request event before try_stream! so the request id is
    // captured even if the stream is never polled.
    let request_id = ctx.next_request_id();
    ctx.emit_request(
        request_id,
        "POST (stream)",
        &url,
        ctx.serialize_wire_body(&request),
    );

    try_stream! {
        // Accumulate steps from step.start/step.delta/step.stop events so the
        // final Completed response carries a fully-populated steps array even
        // when the server's interaction.completed payload omits it.
        let mut accumulator = StepAccumulator::new();

        let response = ctx
            .http_client
            .post(&url)
            .header(API_KEY_HEADER, &ctx.api_key)
            .header(API_REVISION_HEADER, API_REVISION)
            .json(&request)
            .send()
            .await?;

        ctx.emit(WireEvent::ResponseStatus {
            id: request_id,
            status: response.status().as_u16(),
        });

        let response = check_response_wire(response, ctx, request_id).await?;
        let byte_stream = response.bytes_stream();
        let parsed_stream = parse_sse_stream::<InteractionStreamEvent>(byte_stream, ctx, request_id);
        futures_util::pin_mut!(parsed_stream);

        while let Some(result) = parsed_stream.next().await {
            let event = result?;
            debug!(
                "SSE event received: event_type={:?}, index={:?}, event_id={:?}",
                event.event_type,
                event.index,
                event.event_id
            );

            let event_id = event.event_id.clone();
            if let Some(chunk) = dispatch_stream_event(event, &mut accumulator) {
                yield StreamEvent::new(chunk, event_id);
            }
        }
    }
}

/// Retrieves an existing interaction by its ID.
///
/// Useful for checking the status of long-running interactions or agents,
/// or for retrieving the full conversation history.
///
/// Set `include_input` to also receive the original `input` in the response.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request fails
/// - The response status is not successful
/// - The response cannot be parsed as JSON
pub async fn get_interaction(
    ctx: &HttpContext,
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

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, "GET", &url, None);

    let response = ctx
        .http_client
        .get(&url)
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION)
        .send()
        .await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    let response = check_response_wire(response, ctx, request_id).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;

    ctx.emit_response_body(request_id, &response_text);

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
/// # Example
/// ```ignore
/// // Resume a stream after interruption
/// let mut stream = get_interaction_stream(&ctx, &id, Some("evt_abc123"));
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
    ctx: &'a HttpContext,
    interaction_id: &'a str,
    last_event_id: Option<&'a str>,
) -> impl Stream<Item = Result<StreamEvent, GenaiError>> + Send + 'a {
    let endpoint = Endpoint::GetInteraction {
        id: interaction_id,
        stream: true,
        last_event_id,
        include_input: false,
    };
    let url = construct_endpoint_url(endpoint);

    let request_id = ctx.next_request_id();
    let resume_info = last_event_id
        .map(|id| format!(" (resuming from {})", id))
        .unwrap_or_default();
    ctx.emit_request(
        request_id,
        &format!("GET (stream){}", resume_info),
        &url,
        None,
    );

    try_stream! {
        // Accumulate steps (same as create_interaction_stream)
        let mut accumulator = StepAccumulator::new();

        let response = ctx
            .http_client
            .get(&url)
            .header(API_KEY_HEADER, &ctx.api_key)
            .header(API_REVISION_HEADER, API_REVISION)
            .send()
            .await?;

        ctx.emit(WireEvent::ResponseStatus {
            id: request_id,
            status: response.status().as_u16(),
        });

        let response = check_response_wire(response, ctx, request_id).await?;
        let byte_stream = response.bytes_stream();
        let parsed_stream = parse_sse_stream::<InteractionStreamEvent>(byte_stream, ctx, request_id);
        futures_util::pin_mut!(parsed_stream);

        while let Some(result) = parsed_stream.next().await {
            let event = result?;
            debug!(
                "SSE event received: event_type={:?}, index={:?}, event_id={:?}",
                event.event_type,
                event.index,
                event.event_id
            );

            let event_id = event.event_id.clone();
            if let Some(chunk) = dispatch_stream_event(event, &mut accumulator) {
                yield StreamEvent::new(chunk, event_id);
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
pub async fn delete_interaction(ctx: &HttpContext, interaction_id: &str) -> Result<(), GenaiError> {
    let endpoint = Endpoint::DeleteInteraction { id: interaction_id };
    let url = construct_endpoint_url(endpoint);

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, "DELETE", &url, None);

    let response = ctx
        .http_client
        .delete(&url)
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION)
        .send()
        .await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    check_response_wire(response, ctx, request_id).await?;
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
    ctx: &HttpContext,
    interaction_id: &str,
) -> Result<InteractionResponse, GenaiError> {
    let endpoint = Endpoint::CancelInteraction { id: interaction_id };
    let url = construct_endpoint_url(endpoint);

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, "POST", &url, Some(serde_json::json!({})));

    // Send empty JSON body - the API requires Content-Length header
    let response = ctx
        .http_client
        .post(&url)
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION)
        .json(&serde_json::json!({}))
        .send()
        .await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    let response = check_response_wire(response, ctx, request_id).await?;
    let response_text = response.text().await.map_err(GenaiError::Http)?;

    ctx.emit_response_body(request_id, &response_text);

    let interaction_response: InteractionResponse =
        deserialize_with_context(&response_text, "InteractionResponse from cancel")?;

    Ok(interaction_response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InteractionInput, InteractionStatus, Step, StepDelta};

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

        let endpoint_get_with_input = Endpoint::GetInteraction {
            id: "test_id_123",
            stream: false,
            last_event_id: None,
            include_input: true,
        };
        let url = construct_endpoint_url(endpoint_get_with_input);
        assert!(url.contains("include_input=true"));

        let endpoint_delete = Endpoint::DeleteInteraction { id: "test_id_456" };
        let url = construct_endpoint_url(endpoint_delete);
        assert!(url.contains("/v1beta/interactions/test_id_456"));

        let endpoint_cancel = Endpoint::CancelInteraction { id: "test_id_789" };
        let url = construct_endpoint_url(endpoint_cancel);
        assert!(url.contains("/v1beta/interactions/test_id_789/cancel"));
    }

    #[test]
    fn test_api_revision_constants() {
        assert_eq!(API_REVISION_HEADER, "Api-Revision");
        assert_eq!(API_REVISION, "2026-05-20");
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
            generation_config: None,
            stream: None,
            background: None,
            store: None,
            system_instruction: None,
            service_tier: None,
            cached_content: None,
            webhook_config: None,
            environment: None,
        };

        let json = serde_json::to_string(&request).expect("Serialization should work");
        assert!(json.contains("gemini-3-flash-preview"));
        assert!(json.contains("Hello"));
    }

    #[test]
    fn test_interaction_response_deserialization() {
        // Verify we can deserialize a typical revision 2026-05-20 response
        let response_json = r#"{
            "id": "test_interaction_123",
            "model": "gemini-3-flash-preview",
            "steps": [{"type": "model_output", "content": [{"type": "text", "text": "Hi there!"}]}],
            "status": "completed"
        }"#;

        let response: InteractionResponse =
            serde_json::from_str(response_json).expect("Deserialization should work");

        assert_eq!(response.id.as_deref(), Some("test_interaction_123"));
        assert_eq!(response.status, InteractionStatus::Completed);
        assert_eq!(response.steps.len(), 1);
        assert_eq!(response.as_text(), Some("Hi there!"));
    }

    #[test]
    fn test_cancelled_interaction_response_deserialization() {
        // Verify we can deserialize a cancelled interaction response
        let response_json = r#"{
            "id": "cancelled_interaction_123",
            "model": "deep-research-pro-preview-12-2025",
            "steps": [],
            "status": "cancelled"
        }"#;

        let response: InteractionResponse =
            serde_json::from_str(response_json).expect("Deserialization should work");

        assert_eq!(response.id.as_deref(), Some("cancelled_interaction_123"));
        assert_eq!(response.status, InteractionStatus::Cancelled);
        assert!(response.steps.is_empty());
    }

    // =========================================================================
    // SSE dispatch tests (event shapes per revision 2026-05-20)
    // =========================================================================

    fn parse_event(json: &str) -> InteractionStreamEvent {
        serde_json::from_str(json).expect("event should parse")
    }

    #[test]
    fn test_dispatch_full_text_lifecycle() {
        let mut acc = StepAccumulator::new();

        let created = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.created","interaction":{"id":"i1","status":"in_progress"},"event_id":"e0"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(created, StreamChunk::Created { .. }));

        let start = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.start","index":0,"step":{"type":"model_output","content":[]},"event_id":"e1"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(start, StreamChunk::StepStart { index: 0, .. }));

        let delta = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.delta","index":0,"delta":{"type":"text","text":"Hello"},"event_id":"e2"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match &delta {
            StreamChunk::StepDelta { index: 0, delta } => {
                assert_eq!(delta.as_text(), Some("Hello"));
            }
            other => panic!("Expected StepDelta, got {other:?}"),
        }

        let stop = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.stop","index":0,"step_usage":{"total_output_tokens":5},"event_id":"e3"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match &stop {
            StreamChunk::StepStop { step_usage, .. } => {
                assert_eq!(step_usage.as_ref().unwrap().total_output_tokens, Some(5));
            }
            other => panic!("Expected StepStop, got {other:?}"),
        }

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"completed"},"metadata":{"total_usage":{"total_tokens":12}},"event_id":"e4"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                // Steps were filled from the accumulator.
                assert_eq!(response.as_text(), Some("Hello"));
                // Usage was taken from event metadata.
                assert_eq!(response.total_tokens(), Some(12));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_function_call_arguments_delta() {
        let mut acc = StepAccumulator::new();

        dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.start","index":0,"step":{"type":"function_call","id":"c1","name":"get_weather","arguments":{}}}"#,
            ),
            &mut acc,
        );
        let delta = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"step.delta","index":0,"delta":{"type":"arguments_delta","arguments":"{\"city\":\"Tokyo\"}"}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(
            delta,
            StreamChunk::StepDelta {
                delta: StepDelta::ArgumentsDelta { .. },
                ..
            }
        ));
        dispatch_stream_event(
            parse_event(r#"{"event_type":"step.stop","index":0}"#),
            &mut acc,
        );

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{"id":"i1","status":"requires_action"}}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                let calls = response.function_calls();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "get_weather");
                assert_eq!(calls[0].args["city"], "Tokyo");
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }

    #[test]
    fn test_dispatch_status_update_and_error() {
        let mut acc = StepAccumulator::new();

        let update = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.status_update","interaction_id":"i1","status":"budget_exceeded"}"#,
            ),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(
            update,
            StreamChunk::StatusUpdate {
                status: InteractionStatus::BudgetExceeded,
                ..
            }
        ));

        let error = dispatch_stream_event(
            parse_event(r#"{"event_type":"error","error":{"message":"boom","code":"internal"}}"#),
            &mut acc,
        )
        .unwrap();
        assert!(matches!(error, StreamChunk::Error { message, .. } if message == "boom"));
    }

    #[test]
    fn test_dispatch_unknown_event_preserved() {
        let mut acc = StepAccumulator::new();
        let chunk = dispatch_stream_event(
            parse_event(r#"{"event_type":"interaction.paused","reason":"maintenance"}"#),
            &mut acc,
        )
        .unwrap();
        assert!(chunk.is_unknown());
        assert_eq!(chunk.unknown_chunk_type(), Some("interaction.paused"));
        assert_eq!(chunk.unknown_data().unwrap()["reason"], "maintenance");
    }

    #[test]
    fn test_dispatch_completed_prefers_server_steps() {
        let mut acc = StepAccumulator::new();
        acc.start(0, Step::model_text("accumulated"));

        let completed = dispatch_stream_event(
            parse_event(
                r#"{"event_type":"interaction.completed","interaction":{
                    "id":"i1","status":"completed",
                    "steps":[{"type":"model_output","content":[{"type":"text","text":"authoritative"}]}]
                }}"#,
            ),
            &mut acc,
        )
        .unwrap();
        match completed {
            StreamChunk::Completed(response) => {
                assert_eq!(response.as_text(), Some("authoritative"));
            }
            other => panic!("Expected Completed, got {other:?}"),
        }
    }
}
