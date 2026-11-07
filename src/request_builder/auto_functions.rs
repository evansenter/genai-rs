//! Automatic function calling logic for InteractionBuilder.
//!
//! This module contains the `create_with_auto_functions()` and
//! `create_stream_with_auto_functions()` methods that handle
//! automatic function discovery, execution, and multi-turn orchestration.

mod round;

use super::within;
use crate::{GenaiError, StreamChunk};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use tracing::debug;

use crate::streaming::{AutoFunctionResult, AutoFunctionStreamChunk, AutoFunctionStreamEvent};
pub(crate) use round::DEFAULT_MAX_FUNCTION_CALL_LOOPS;
use round::{AutoFunctionLoop, Round};

use super::InteractionBuilder;

/// Reads a round's stream to its end after its `Completed` event. The HTTP
/// layer then only reads the body's tail, bounded and with errors
/// swallowed, so nothing more is yielded.
async fn read_to_end<S: futures_util::Stream + Unpin>(stream: &mut S) {
    while stream.next().await.is_some() {}
}

impl<'a> InteractionBuilder<'a> {
    /// Creates interaction with automatic function call handling.
    ///
    /// This method implements the auto-function execution loop:
    /// 1. Send initial input to model with available tools
    /// 2. If response contains function calls, execute them
    /// 3. Send function results back to model in new interaction
    /// 4. Repeat until model returns text or max iterations reached
    ///
    /// Functions are auto-discovered from the global registry (via `#[tool]` macro)
    /// or can be explicitly provided via `.add_function()` or `.with_tools()`.
    ///
    /// The loop automatically stops when:
    /// - Model returns text without function calls
    /// - Function calls array is empty
    /// - Maximum iterations is reached (default 5, configurable via `with_max_function_call_loops()`)
    ///
    /// # Thought Signatures
    ///
    /// For Gemini 3 models, thought signatures are required to maintain reasoning context
    /// across function calling turns. This method uses `previous_interaction_id` to link
    /// turns, which allows the server to manage thought signatures automatically.
    ///
    /// See <https://ai.google.dev/gemini-api/docs/thought-signatures> for more details.
    ///
    /// # Runtime Validation
    ///
    /// This method requires storage to be enabled. If you've called `with_store_disabled()`,
    /// this method will return an error with a helpful message explaining why and how to fix it.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, FunctionDeclaration};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// // Functions are auto-discovered from registry
    /// let result = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What's the weather in Tokyo?")
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// // Access the final response
    /// println!("{}", result.response.as_text().unwrap_or("No text"));
    ///
    /// // Access execution history
    /// for exec in &result.executions {
    ///     println!("Called {} -> {}", exec.name, exec.result);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Serialization
    ///
    /// Both [`AutoFunctionResult`] and its contained
    /// [`InteractionResponse`](crate::InteractionResponse) implement
    /// `Serialize`, enabling logging, caching, and persistence of complete
    /// execution histories for debugging and evaluation workflows.
    ///
    /// # Max Loops Behavior
    ///
    /// When the maximum number of iterations is reached (default 5, configurable via
    /// `with_max_function_call_loops()`), the method returns an `Ok` result with
    /// `reached_max_loops: true` instead of an error. This preserves the execution
    /// history and the last response for debugging stuck loops.
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("key".to_string());
    /// let result = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("...")
    ///     .with_max_function_call_loops(3)
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// if result.reached_max_loops {
    ///     eprintln!("Hit max loops! Executed {} functions", result.executions.len());
    ///     // Inspect result.response.pending_function_calls() to see what's still pending
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Timeout Behavior
    ///
    /// If [`with_timeout()`](InteractionBuilder::with_timeout) was set, the timeout applies
    /// **per-API-call**, not to the total operation or function execution time. Each round
    /// of model interaction must complete within the timeout, but:
    ///
    /// - Function execution time is **not** counted against the timeout
    /// - Multiple API calls may occur (one per function-calling round)
    /// - For a total timeout, wrap the call in `tokio::time::timeout()`
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("key".to_string());
    /// // Per-API-call timeout (30s per model round)
    /// let result = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What's the weather?")
    ///     .with_timeout(Duration::from_secs(30))
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// // Total timeout (60s for entire operation including functions)
    /// let result = tokio::time::timeout(
    ///     Duration::from_secs(60),
    ///     client.interaction()
    ///         .with_model(genai_rs::DEFAULT_MODEL)
    ///         .with_text("What's the weather?")
    ///         .create_with_auto_functions()
    /// ).await??;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No input was provided
    /// - Neither model nor agent was specified
    /// - The API request fails
    /// - An API call times out (if `with_timeout()` was set). Note: on timeout, any
    ///   function calls that completed in previous iterations are preserved on the API
    ///   side via the interaction chain, but this method returns an error rather than
    ///   a partial `AutoFunctionResult`. Use `previous_interaction_id` to continue.
    /// - `max_function_call_loops` is set to 0 (invalid configuration)
    pub async fn create_with_auto_functions(self) -> Result<AutoFunctionResult, GenaiError> {
        // Auto-functions chain rounds on stored interactions.
        self.validate_for_auto_functions()?;

        let client = self.client;
        let timeout = self.timeout;
        let max_loops = self.max_function_call_loops;
        let tool_service = self.tool_service.clone();
        let mut state = AutoFunctionLoop::new(self.build()?, tool_service.as_ref());
        let mut executions = Vec::new();

        for round in 1..=max_loops {
            debug!("Auto-function round {round}/{max_loops}");
            // Per API call: function execution time is not included.
            let response = within(
                timeout,
                "Auto-function API call",
                client.execute(state.request.clone()),
            )
            .await??;
            match state.read_round(response)? {
                Round::Done(response) => {
                    return Ok(AutoFunctionResult {
                        response,
                        executions,
                        reached_max_loops: false,
                    });
                }
                Round::Calls { response, calls } => {
                    executions.extend(state.run_calls(response.id.clone(), &calls).await);
                    state.keep_last_response(response);
                }
            }
        }

        // The partial result keeps the executions for debugging a stuck loop.
        Ok(AutoFunctionResult {
            response: state.max_loops_response(max_loops)?,
            executions,
            reached_max_loops: true,
        })
    }

    /// Creates a streaming interaction with automatic function call handling.
    ///
    /// This method combines the streaming capabilities of `create_stream()` with the
    /// automatic function execution of `create_with_auto_functions()`. It yields
    /// [`AutoFunctionStreamChunk`] events that include:
    ///
    /// - `Delta`: Incremental content from the model (text, thoughts, etc.)
    /// - `ExecutingFunctions`: Notification when function calls are about to execute
    /// - `FunctionResults`: Results from executed functions
    /// - `Complete`: Final response when no more function calls are needed
    ///
    /// The stream automatically handles multiple function calling rounds, streaming
    /// content from each round and executing functions between rounds.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Client, AutoFunctionStreamChunk};
    /// # use futures_util::StreamExt;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// let mut stream = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What's the weather in Tokyo?")
    ///     .create_stream_with_auto_functions();
    ///
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     match event.chunk {
    ///         AutoFunctionStreamChunk::Delta(delta) => {
    ///             if let Some(t) = delta.as_text() {
    ///                 print!("{}", t);
    ///             }
    ///         }
    ///         AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
    ///             let names: Vec<_> = pending_calls.iter().map(|c| &c.name).collect();
    ///             println!("[Executing: {:?}]", names);
    ///         }
    ///         AutoFunctionStreamChunk::FunctionResults(results) => {
    ///             println!("[Got {} results]", results.len());
    ///         }
    ///         AutoFunctionStreamChunk::Complete(response) => {
    ///             println!("\n[Complete: {} tokens]", response.usage.as_ref()
    ///                 .and_then(|u| u.total_tokens).unwrap_or(0));
    ///         }
    ///         _ => {} // Handle unknown future variants
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Max Loops Behavior
    ///
    /// When the maximum number of iterations is reached, the stream yields a
    /// `MaxLoopsReached(response)` chunk instead of returning an error. This
    /// preserves access to prior `FunctionResults` chunks that were already yielded.
    ///
    /// The `AutoFunctionResultAccumulator` handles `MaxLoopsReached` automatically
    /// and returns an `AutoFunctionResult` with `reached_max_loops: true`.
    ///
    /// # Timeout Behavior
    ///
    /// If [`with_timeout()`](InteractionBuilder::with_timeout) was set, the timeout applies
    /// **per-chunk**, not to the total stream or function execution time. Each chunk must
    /// arrive within the timeout (detecting stalled connections), but:
    ///
    /// - Function execution time is **not** counted against the timeout
    /// - Multiple streaming rounds may occur (one per function-calling round)
    /// - For a total timeout, wrap the stream consumption in `tokio::time::timeout()`
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # use futures_util::StreamExt;
    /// # use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("key".to_string());
    /// // Per-chunk timeout (30s between chunks)
    /// let mut stream = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What's the weather?")
    ///     .with_timeout(Duration::from_secs(30))
    ///     .create_stream_with_auto_functions();
    ///
    /// // Total timeout (120s for entire stream + function execution)
    /// tokio::time::timeout(Duration::from_secs(120), async {
    ///     while let Some(chunk) = stream.next().await {
    ///         // process chunk...
    ///     }
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns errors if:
    /// - No input was provided
    /// - Neither model nor agent was specified
    /// - The API request fails
    /// - A chunk doesn't arrive within the timeout (if set). Note: on timeout, any
    ///   function calls that completed in previous iterations are preserved on the API
    ///   side via the interaction chain, but the stream yields an error rather than
    ///   a partial result. Use `previous_interaction_id` to continue.
    /// - The server sends an error event mid-stream ([`GenaiError::Stream`])
    /// - A round's stream is cut off before its `Completed` event
    ///   ([`GenaiError::StreamInterrupted`], retryable). So does a round
    ///   whose stream ends cleanly on an event this crate doesn't
    ///   recognize: [`create_stream()`](Self::create_stream) ends such a
    ///   stream without an error, since the event may be a new terminal
    ///   one, but the loop needs the round's `Completed` response to go on.
    ///   After a round's `Completed`, the rest of its body is read and
    ///   discarded, errors included and outside the per-chunk timeout, so a
    ///   late error can't fail a finished round. That read ends before the
    ///   next round's request is sent, and before this stream ends, since
    ///   the server stores an interaction only once its stream is done.
    /// - `max_function_call_loops` is set to 0 (invalid configuration)
    ///
    /// The stream owns a clone of the client, so it can be stored or
    /// `tokio::spawn`ed.
    pub fn create_stream_with_auto_functions(
        self,
    ) -> BoxStream<'static, Result<AutoFunctionStreamEvent, GenaiError>> {
        // The stream owns its client, so it can be spawned. The request is
        // validated and built now; an error is the stream's only item, and
        // nothing is sent.
        let client = self.client.clone();
        let (timeout, max_loops) = (self.timeout, self.max_function_call_loops);
        let tool_service = self.tool_service.clone();
        let request = self
            .validate_for_auto_functions()
            .and_then(|()| self.build());

        Box::pin(async_stream::try_stream! {
            let mut state = AutoFunctionLoop::new(request?, tool_service.as_ref());

            for round in 1..=max_loops {
                debug!("Auto-function streaming round {round}/{max_loops}");
                let mut stream = client.execute_stream(state.request.clone());
                let mut completed = None;
                // The last event id, for resume support.
                let mut last_event_id: Option<String> = None;

                // Per chunk: function execution time is not included.
                while let Some(event) =
                    within(timeout, "Auto-function stream chunk", stream.next()).await?
                {
                    let event = event?;
                    if event.event_id.is_some() {
                        last_event_id = event.event_id.clone();
                    }
                    match event.chunk {
                        // Every delta, including the arguments_delta
                        // fragments of a streamed function call.
                        StreamChunk::StepDelta { delta, .. } => {
                            yield AutoFunctionStreamEvent::new(
                                AutoFunctionStreamChunk::Delta(delta),
                                event.event_id,
                            );
                        }
                        // The HTTP layer assembles the steps into the
                        // Completed response, so its function calls are
                        // whole. The tail is read below, outside the
                        // per-chunk timeout, which must not fail a finished
                        // round.
                        StreamChunk::Completed(response) => {
                            completed = Some(*response);
                            break;
                        }
                        StreamChunk::Error { message, code } => {
                            Err(GenaiError::Stream { message, code })?;
                        }
                        StreamChunk::Unknown { chunk_type, .. } => {
                            tracing::warn!(
                                "Received unknown StreamChunk type '{chunk_type}' during \
                                 auto-function streaming. This may indicate a new API feature."
                            );
                        }
                        _ => {}
                    }
                }
                // A cut-off already failed above, so the round's stream
                // ended cleanly on an event this crate doesn't recognize
                // (Evergreen: it may be a new terminal event). The plain
                // stream lets that pass, but the loop can't go on without
                // the round's response. Nothing says the request was at
                // fault, so the round ends as a cut-off one does: retryable.
                let response = completed.ok_or_else(|| GenaiError::StreamInterrupted {
                    message: "the round's stream ended on an unrecognized event, with no \
                              interaction.completed to run the loop on"
                        .to_string(),
                    source: None,
                })?;

                match state.read_round(response)? {
                    Round::Done(response) => {
                        yield AutoFunctionStreamEvent::new(
                            AutoFunctionStreamChunk::Complete(Box::new(response)),
                            last_event_id,
                        );
                        // Read the round's stream to its end before this one
                        // ends, so a caller that gets the interaction or
                        // chains on it then finds it stored.
                        read_to_end(&mut stream).await;
                        return;
                    }
                    Round::Calls { response, calls } => {
                        let round_id = response.id.clone();
                        // The event takes the response. The final round's is
                        // also the MaxLoopsReached result, so only it is
                        // cloned.
                        if round == max_loops {
                            state.keep_last_response(response.clone());
                        }
                        // Client-generated events carry no API event id.
                        yield AutoFunctionStreamEvent::new(
                            AutoFunctionStreamChunk::ExecutingFunctions {
                                response: Box::new(response),
                                pending_calls: calls.clone(),
                            },
                            None,
                        );
                        let executions = state.run_calls(round_id, &calls).await;
                        yield AutoFunctionStreamEvent::new(
                            AutoFunctionStreamChunk::FunctionResults(executions),
                            None,
                        );
                        // The next round chains on this one's id, and the
                        // server stores an interaction only once its stream
                        // is done. The tail usually arrived while the
                        // functions ran.
                        read_to_end(&mut stream).await;
                    }
                }
            }

            // Earlier FunctionResults chunks already carried the executions.
            yield AutoFunctionStreamEvent::new(
                AutoFunctionStreamChunk::MaxLoopsReached(Box::new(
                    state.max_loops_response(max_loops)?,
                )),
                None,
            );
        })
    }
}
