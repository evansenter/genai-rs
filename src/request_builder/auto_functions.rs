//! Automatic function calling logic for InteractionBuilder.
//!
//! This module contains the `create_with_auto_functions()` and
//! `create_stream_with_auto_functions()` methods that handle
//! automatic function discovery, execution, and multi-turn orchestration.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::{InteractionInput, InteractionResponse, StreamChunk, UsageMetadata};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::GenaiError;
use crate::Step;
use crate::ToolService;
use crate::function_calling::{CallableFunction, FunctionRegistry, get_global_function_registry};
use crate::streaming::{
    AutoFunctionResult, AutoFunctionStreamChunk, AutoFunctionStreamEvent, FunctionExecutionResult,
    PendingFunctionCall,
};

use super::InteractionBuilder;

/// Default maximum iterations for auto function calling.
pub(crate) const DEFAULT_MAX_FUNCTION_CALL_LOOPS: usize = 5;

/// Builds a map of callable functions from a ToolService for efficient lookup.
fn build_service_function_map(
    tool_service: &Option<Arc<dyn ToolService>>,
) -> HashMap<String, Arc<dyn CallableFunction>> {
    tool_service
        .as_ref()
        .map(|svc| {
            svc.tools()
                .into_iter()
                .map(|f| (f.declaration().name().to_string(), f))
                .collect()
        })
        .unwrap_or_default()
}

/// Auto-discovers functions from the global registry and tool service.
///
/// If `request.tools` is already set, this is a no-op. Otherwise, it:
/// 1. Collects all functions from the global registry (`#[tool]` macro functions)
/// 2. Filters out any that would be shadowed by service functions (with warning)
/// 3. Adds declarations from the tool service
/// 4. Sets `request.tools` if any functions were found
fn auto_discover_tools(
    request: &mut crate::request::InteractionRequest,
    service_functions: &HashMap<String, Arc<dyn CallableFunction>>,
) {
    if request.tools.is_some() {
        return;
    }

    let function_registry = get_global_function_registry();
    let mut all_declarations = function_registry.all_declarations();

    // Service functions take precedence over global registry
    // Filter out global declarations that would be shadowed by service functions
    let service_names: std::collections::HashSet<&str> =
        service_functions.keys().map(|s| s.as_str()).collect();

    // Log warnings for shadowed functions and filter them out
    all_declarations.retain(|decl| {
        if service_names.contains(decl.name()) {
            warn!(
                "Tool service function '{}' shadows global registry function with same name",
                decl.name()
            );
            false
        } else {
            true
        }
    });

    // Add declarations from tool service
    for func in service_functions.values() {
        all_declarations.push(func.declaration());
    }

    if !all_declarations.is_empty() {
        request.tools = Some(
            all_declarations
                .into_iter()
                .map(|decl| decl.into_tool())
                .collect(),
        );
    }
}

/// Executes a function by looking it up in the service map first, then the global registry.
///
/// Returns the function result as JSON. Errors are converted to JSON error objects
/// rather than failing the entire operation, allowing the model to recover gracefully.
async fn execute_function(
    name: &str,
    args: Value,
    service_functions: &HashMap<String, Arc<dyn CallableFunction>>,
    function_registry: &FunctionRegistry,
) -> Value {
    // Function lookup order: tool service first (for dependency-injected functions),
    // then global registry (for #[tool] macro functions).
    if let Some(function) = service_functions.get(name) {
        // Found in tool service (dependency-injected)
        match function.call(args).await {
            Ok(result) => result,
            Err(e) => {
                warn!(
                    "Function execution failed (recoverable): function='{}', error='{}'. \
                     The error will be sent to the model, which may retry or adapt.",
                    name, e
                );
                json!({ "error": e.to_string() })
            }
        }
    } else if let Some(function) = function_registry.get(name) {
        // Found in global registry (#[tool] macro)
        match function.call(args).await {
            Ok(result) => result,
            Err(e) => {
                warn!(
                    "Function execution failed (recoverable): function='{}', error='{}'. \
                     The error will be sent to the model, which may retry or adapt.",
                    name, e
                );
                json!({ "error": e.to_string() })
            }
        }
    } else {
        // Function not found anywhere - could be a typo in declarations or missing #[tool] macro.
        // We inform the model rather than failing, allowing it to adapt or use other functions.
        warn!(
            "Function not found in registry or tool service: function='{}'. Informing model.",
            name
        );
        json!({ "error": format!("Function '{}' is not available or not found.", name) })
    }
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
    /// or can be explicitly provided via `.add_function()` or `.set_tools()`.
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
    ///     .with_model("gemini-3-flash-preview")
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
    /// Both [`AutoFunctionResult`] and its contained [`InteractionResponse`]
    /// implement `Serialize`, enabling logging, caching, and persistence of complete
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
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("...")
    ///     .with_max_function_call_loops(3)
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// if result.reached_max_loops {
    ///     eprintln!("Hit max loops! Executed {} functions", result.executions.len());
    ///     // Inspect result.response.function_calls() to see what's still pending
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
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What's the weather?")
    ///     .with_timeout(Duration::from_secs(30))
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// // Total timeout (60s for entire operation including functions)
    /// let result = tokio::time::timeout(
    ///     Duration::from_secs(60),
    ///     client.interaction()
    ///         .with_model("gemini-3-flash-preview")
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
        // Runtime validation: auto-functions require storage
        self.validate_for_auto_functions()?;

        let client = self.client;
        let timeout = self.timeout;
        let max_loops = self.max_function_call_loops;
        let tool_service = self.tool_service.clone();
        let mut request = self.build()?;

        // Track all function executions for the result
        let mut all_executions: Vec<FunctionExecutionResult> = Vec::new();

        // Build a map of service-provided functions for lookup during execution
        let service_functions = build_service_function_map(&tool_service);

        // Auto-discover functions from registry and tool service if not explicitly provided
        auto_discover_tools(&mut request, &service_functions);
        let function_registry = get_global_function_registry();

        // Track the last response for returning partial results if max loops is reached
        let mut last_response: Option<InteractionResponse> = None;

        // Accumulate usage across all loop iterations.
        // The API may report 0 input tokens on the final response, so we track
        // total usage ourselves for accurate reporting.
        let mut accumulated_usage = UsageMetadata::default();

        // Main auto-function loop (configurable iterations to prevent infinite loops)
        for loop_count in 0..max_loops {
            debug!(
                "Auto-function loop iteration {}/{}",
                loop_count + 1,
                max_loops
            );

            // Apply per-API-call timeout if set (function execution time not included)
            let response = match timeout {
                Some(duration) => {
                    let future = client.execute(request.clone());
                    tokio::time::timeout(duration, future).await.map_err(|_| {
                        warn!("Auto-function API call timed out after {:?}", duration);
                        GenaiError::Timeout(duration)
                    })??
                }
                None => client.execute(request.clone()).await?,
            };

            // When store != false (validated at function entry), the API should always
            // return an interaction ID. Return an error if the API violates this contract,
            // as continuing would silently lose conversation context.
            if response.id.is_none() {
                return Err(GenaiError::MalformedResponse(
                    "Response missing interaction ID. Auto-function calling requires stored \
                     interactions (store != false) to maintain conversation context."
                        .to_string(),
                ));
            }

            // Accumulate usage from this response
            if let Some(ref usage) = response.usage {
                accumulated_usage.accumulate(usage);
            }

            // Extract function calls using convenience method
            let function_calls = response.function_calls();

            // If no function calls, we're done!
            if function_calls.is_empty() {
                debug!("No function calls in response, completing auto-function loop");
                // Create final response with accumulated usage across all API calls
                let final_response = InteractionResponse {
                    usage: Some(accumulated_usage),
                    ..response
                };
                return Ok(AutoFunctionResult {
                    response: final_response,
                    executions: all_executions,
                    reached_max_loops: false,
                });
            }

            // Build function results for next iteration
            let mut function_results = Vec::new();
            debug!("Executing {} function call(s)", function_calls.len());

            for call in function_calls {
                let call_id = call.id.to_string();

                // Execute the function with timing
                let start = Instant::now();
                let result = execute_function(
                    call.name,
                    call.args.clone(),
                    &service_functions,
                    function_registry,
                )
                .await;
                let duration = start.elapsed();
                debug!("Function '{}' executed in {:?}", call.name, duration);

                // Track execution for the result
                all_executions.push(FunctionExecutionResult::new(
                    call.name,
                    &call_id,
                    call.args.clone(),
                    result.clone(),
                    duration,
                ));

                // Add function result step (only the result, not the call -
                // the server has the call via previous_interaction_id)
                function_results.push(Step::function_result(
                    call.name.to_string(),
                    call_id,
                    result,
                ));
            }

            // Save this response before moving to next iteration
            // (in case we hit max loops, we want to return the last response)
            last_response = Some(response.clone());

            // Create new request with function results
            // The server maintains function call context via previous_interaction_id
            request.previous_interaction_id = response.id;
            request.input = InteractionInput::Steps(function_results);
        }

        // Max loops reached - return partial result with whatever we have
        // This preserves execution history for debugging instead of discarding it
        warn!(
            "Reached maximum function call loops ({max_loops}). \
             Returning partial result with {} executions. \
             The model may be stuck in a loop.",
            all_executions.len()
        );

        // If we never made it through even one iteration (shouldn't happen with max_loops > 0),
        // return an error since we have no response to return
        let response = last_response.ok_or_else(|| {
            GenaiError::InvalidInput(format!(
                "max_function_call_loops ({max_loops}) must be at least 1"
            ))
        })?;

        // Create final response with accumulated usage across all API calls
        let final_response = InteractionResponse {
            usage: Some(accumulated_usage),
            ..response
        };

        Ok(AutoFunctionResult {
            response: final_response,
            executions: all_executions,
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
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What's the weather in Tokyo?")
    ///     .create_stream_with_auto_functions();
    ///
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     // event.event_id can be saved for stream resume support
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
    ///     .with_model("gemini-3-flash-preview")
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
    /// - A function call is missing its required `call_id` field
    /// - `max_function_call_loops` is set to 0 (invalid configuration)
    pub fn create_stream_with_auto_functions(
        self,
    ) -> BoxStream<'a, Result<AutoFunctionStreamEvent, GenaiError>> {
        // Runtime validation: auto-functions require storage
        // We do this early so errors are returned immediately, not mid-stream
        if let Err(e) = self.validate_for_auto_functions() {
            return Box::pin(futures_util::stream::once(async move { Err(e) }));
        }

        let client = self.client;
        let max_loops = self.max_function_call_loops;
        let tool_service = self.tool_service.clone();
        let timeout = self.timeout;

        Box::pin(async_stream::try_stream! {
            let mut request = self.build()?;

            // Build a map of service-provided functions for lookup during execution
            let service_functions = build_service_function_map(&tool_service);

            // Auto-discover functions from registry and tool service if not explicitly provided
            auto_discover_tools(&mut request, &service_functions);
            let function_registry = get_global_function_registry();

            // Track the last response for returning partial results if max loops is reached
            let mut last_response: Option<InteractionResponse> = None;

            // Accumulate usage across all loop iterations.
            // The API may report 0 input tokens on the final response (especially in
            // streaming), so we track total usage ourselves for accurate reporting.
            let mut accumulated_usage = UsageMetadata::default();

            // Main auto-function streaming loop
            for loop_count in 0..max_loops {
                debug!("Auto-function streaming loop iteration {}/{}", loop_count + 1, max_loops);
                // Enable streaming for this request
                request.stream = Some(true);

                // Stream this iteration's response
                let mut stream = client.execute_stream(request.clone());
                let mut complete_response: Option<InteractionResponse> = None;
                // Track last event_id for resume support
                let mut last_event_id: Option<String> = None;

                // Apply per-chunk timeout if set (function execution time not included)
                loop {
                    let next_chunk = stream.next();
                    let result = match timeout {
                        Some(duration) => {
                            match tokio::time::timeout(duration, next_chunk).await {
                                Ok(Some(result)) => Some(result),
                                Ok(None) => None,
                                Err(_) => {
                                    warn!("Auto-function stream chunk timed out after {:?}", duration);
                                    Err(GenaiError::Timeout(duration))?;
                                    unreachable!()
                                }
                            }
                        }
                        None => next_chunk.await,
                    };

                    let Some(result) = result else { break };
                    let event = result?;
                    // Track event_id for resume support
                    if event.event_id.is_some() {
                        last_event_id = event.event_id.clone();
                    }
                    match event.chunk {
                        StreamChunk::StepDelta { delta, .. } => {
                            // Forward all step deltas, including arguments_delta
                            // fragments for streaming function-call arguments.
                            yield AutoFunctionStreamEvent::new(
                                AutoFunctionStreamChunk::Delta(delta),
                                event.event_id,
                            );
                        }
                        StreamChunk::Completed(response) => {
                            // The HTTP layer accumulates step.start/delta/stop
                            // events into the Completed response's steps, so
                            // response.function_calls() is fully populated here.
                            complete_response = Some(response);
                        }
                        StreamChunk::Error { message, code } => {
                            tracing::warn!(
                                "Streaming error during auto-function loop: {} (code: {:?})",
                                message, code
                            );
                        }
                        // Log unknown chunk types for observability, but continue for forward compatibility
                        StreamChunk::Unknown { chunk_type, .. } => {
                            tracing::warn!(
                                "Received unknown StreamChunk type '{}' during auto-function streaming. \
                                 This may indicate a new API feature.",
                                chunk_type
                            );
                        }
                        // Wildcard for future non-exhaustive variants
                        _ => {}
                    }
                }

                // Get the complete response (should always be present after stream ends)
                let response = complete_response.ok_or_else(|| {
                    GenaiError::MalformedResponse(
                        "Stream ended without Complete event".to_string()
                    )
                })?;

                // Accumulate usage from this response
                if let Some(ref usage) = response.usage {
                    accumulated_usage.accumulate(usage);
                }

                // When store != false (validated at function entry), the API should always
                // return an interaction ID. Return an error if the API violates this contract,
                // as continuing would silently lose conversation context.
                if response.id.is_none() {
                    Err(GenaiError::MalformedResponse(
                        "Response missing interaction ID. Auto-function calling requires stored \
                         interactions (store != false) to maintain conversation context."
                            .to_string(),
                    ))?;
                }

                // Function calls come from the Completed response's steps
                // (assembled by the HTTP layer from step.start/step.delta
                // events, including streamed arguments_delta fragments).
                let response_function_calls = response.function_calls();
                let has_function_calls = !response_function_calls.is_empty();

                // If no function calls, we're done!
                if !has_function_calls {
                    debug!("No function calls in response, completing auto-function streaming loop");
                    // Create final response with accumulated usage across all API calls
                    let final_response = InteractionResponse {
                        usage: Some(accumulated_usage),
                        ..response
                    };
                    yield AutoFunctionStreamEvent::new(
                        AutoFunctionStreamChunk::Complete(final_response),
                        last_event_id.clone(),
                    );
                    return;
                }

                let calls_to_execute: Vec<(String, String, serde_json::Value)> = response_function_calls
                    .iter()
                    .map(|call| (call.id.to_string(), call.name.to_string(), call.args.clone()))
                    .collect();

                // Signal that we're executing functions with pending call info
                debug!("Executing {} function call(s)", calls_to_execute.len());
                let pending_calls: Vec<PendingFunctionCall> = calls_to_execute
                    .iter()
                    .map(|(call_id, name, args)| PendingFunctionCall::new(name, call_id, args.clone()))
                    .collect();
                // ExecutingFunctions is client-generated, no API event_id
                yield AutoFunctionStreamEvent::new(
                    AutoFunctionStreamChunk::ExecutingFunctions {
                        response: response.clone(),
                        pending_calls,
                    },
                    None,
                );

                // Build function result steps for next iteration
                let mut function_result_steps = Vec::new();
                let mut execution_results = Vec::new();

                for (call_id, name, args) in &calls_to_execute {
                    // Execute the function with timing
                    let start = Instant::now();
                    let result = execute_function(
                        name,
                        args.clone(),
                        &service_functions,
                        function_registry,
                    )
                    .await;
                    let duration = start.elapsed();
                    debug!(
                        "Function '{}' executed in {:?}",
                        name, duration
                    );

                    // Track result for yielding
                    execution_results.push(FunctionExecutionResult::new(
                        name.clone(),
                        call_id.clone(),
                        args.clone(),
                        result.clone(),
                        duration,
                    ));

                    // Add function result step for the API
                    function_result_steps.push(Step::function_result(
                        name.clone(),
                        call_id.clone(),
                        result,
                    ));
                }

                // Yield function results (client-generated, no API event_id)
                yield AutoFunctionStreamEvent::new(
                    AutoFunctionStreamChunk::FunctionResults(execution_results),
                    None,
                );

                // Save this response before moving to next iteration
                // (in case we hit max loops, we want to return the last response)
                last_response = Some(response.clone());

                // Create new request with function results
                request.previous_interaction_id = response.id;
                request.input = InteractionInput::Steps(function_result_steps);
            }

            // Max loops reached - yield partial result with the last response
            // This preserves all prior FunctionResults chunks that were already yielded
            warn!(
                "Reached maximum function call loops ({max_loops}). \
                 Yielding MaxLoopsReached with last response. \
                 The model may be stuck in a loop."
            );

            // If we never made it through even one iteration (shouldn't happen with max_loops > 0),
            // return an error since we have no response to return
            let response = last_response.ok_or_else(|| {
                GenaiError::InvalidInput(format!(
                    "max_function_call_loops ({max_loops}) must be at least 1"
                ))
            })?;

            // Create final response with accumulated usage across all API calls
            let final_response = InteractionResponse {
                usage: Some(accumulated_usage),
                ..response
            };

            // MaxLoopsReached is client-generated (loop limit hit), no API event_id
            yield AutoFunctionStreamEvent::new(
                AutoFunctionStreamChunk::MaxLoopsReached(final_response),
                None,
            );
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: The old validate_call_id() helper was removed in the 2026-05-20
    // revision: function-call ids are now required on the wire (Step::FunctionCall
    // carries `id: String`), so there is no Option to validate. These tests cover
    // the remaining pure helpers in this module instead.

    #[test]
    fn test_build_service_function_map_without_service() {
        let map = build_service_function_map(&None);
        assert!(map.is_empty(), "No tool service should yield an empty map");
    }

    #[tokio::test]
    async fn test_execute_function_not_found_returns_error_json() {
        // A function missing from both the service map and the global registry
        // must produce a recoverable JSON error (sent back to the model),
        // not a hard failure.
        let service_functions: HashMap<String, Arc<dyn CallableFunction>> = HashMap::new();
        let registry = get_global_function_registry();

        let result = execute_function(
            "__genai_rs_test_nonexistent_function__",
            json!({"arg": 1}),
            &service_functions,
            registry,
        )
        .await;

        let error = result
            .get("error")
            .and_then(|v| v.as_str())
            .expect("Missing function should produce an error object");
        assert!(
            error.contains("__genai_rs_test_nonexistent_function__"),
            "Error should name the missing function: {}",
            error
        );
    }

    #[test]
    fn test_default_max_function_call_loops() {
        // Documented default used by with_max_function_call_loops() tests
        assert_eq!(DEFAULT_MAX_FUNCTION_CALL_LOOPS, 5);
    }
}
