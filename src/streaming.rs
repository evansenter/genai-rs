//! Streaming types for automatic function calling.
//!
//! This module contains types for streaming responses with automatic function execution.
//! The [`AutoFunctionStreamChunk`] enum provides events for tracking the progress of
//! an interaction that may involve multiple function call rounds. Events are wrapped
//! in [`AutoFunctionStreamEvent`] to include `event_id` for stream resume support.
//!
//! # Example
//!
//! ```no_run
//! use futures_util::StreamExt;
//! use genai_rs::{Client, AutoFunctionStreamChunk};
//!
//! # async fn example() -> Result<(), genai_rs::GenaiError> {
//! let client = Client::new("your-api-key".to_string());
//!
//! let mut stream = client
//!     .interaction()
//!     .with_model("gemini-3-flash-preview")
//!     .with_text("What's the weather in London?")
//!     .create_stream_with_auto_functions();
//!
//! let mut last_event_id = None;
//! while let Some(result) = stream.next().await {
//!     let event = result?;
//!     // Track event_id for potential resume
//!     if event.event_id.is_some() {
//!         last_event_id = event.event_id.clone();
//!     }
//!
//!     match &event.chunk {
//!         AutoFunctionStreamChunk::Delta(content) => {
//!             if let Some(t) = content.as_text() {
//!                 print!("{}", t);
//!             }
//!         }
//!         AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
//!             println!("[Executing: {:?}]", pending_calls.iter().map(|c| &c.name).collect::<Vec<_>>());
//!         }
//!         AutoFunctionStreamChunk::FunctionResults(results) => {
//!             println!("[Got {} results]", results.len());
//!         }
//!         AutoFunctionStreamChunk::Complete(_response) => {
//!             println!("[Done]");
//!         }
//!         _ => {} // Handle unknown future variants
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use crate::{Content, InteractionResponse};
use serde::{Deserialize, Serialize};

/// A function call that is about to be executed.
///
/// This represents a function call detected during streaming but not yet executed.
/// It contains the call metadata (name, ID, args) but not the result, which will
/// be available in [`FunctionExecutionResult`] after execution completes.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::PendingFunctionCall;
/// # let call: PendingFunctionCall = todo!();
/// println!("About to execute: {}({})", call.name, call.args);
/// println!("  Call ID: {}", call.call_id);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PendingFunctionCall {
    /// Name of the function to be called
    pub name: String,
    /// The call_id from the API (used to match results)
    pub call_id: String,
    /// The arguments to pass to the function
    pub args: serde_json::Value,
}

impl PendingFunctionCall {
    /// Creates a new pending function call.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        call_id: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            call_id: call_id.into(),
            args,
        }
    }
}

/// A chunk from streaming with automatic function calling.
///
/// This enum represents the different events that can occur during a streaming
/// interaction with automatic function execution. The stream yields deltas as
/// content arrives, signals when functions are being executed, and completes
/// when the model returns a response without function calls.
///
/// # Forward Compatibility
///
/// This enum uses `#[non_exhaustive]` to allow adding new event types in future
/// versions without breaking existing code. Always include a wildcard arm in
/// match statements. Unknown variants are preserved with their data for debugging.
///
/// # Serialization
///
/// This enum implements `Serialize` and `Deserialize` for logging, persistence,
/// and replay of streaming events.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum AutoFunctionStreamChunk {
    /// Incremental content from the model (text, thoughts, etc.)
    Delta(Content),

    /// Function calls detected, about to execute.
    ///
    /// This event is yielded when the model requests function calls and
    /// before the functions are executed. The `pending_calls` field contains
    /// the function calls that are about to be executed.
    ///
    /// **Note**: In streaming mode, function calls arrive incrementally via
    /// `Delta` chunks. The `pending_calls` list is built from accumulated deltas
    /// and will always be populated, even though `response.function_calls()`
    /// may be empty.
    ExecutingFunctions {
        /// The response from the API (may have empty `function_calls()` in streaming mode)
        response: InteractionResponse,
        /// The function calls that are about to be executed (always populated)
        pending_calls: Vec<PendingFunctionCall>,
    },

    /// Function execution completed with results.
    ///
    /// This event is yielded after all functions in a batch have been executed,
    /// before sending results back to the model for the next iteration.
    FunctionResults(Vec<FunctionExecutionResult>),

    /// Final complete response (no more function calls).
    ///
    /// This is the last event in the stream, yielded when the model returns
    /// a response that doesn't request any function calls.
    Complete(InteractionResponse),

    /// Maximum function call loops reached.
    ///
    /// This event is yielded when the auto-function loop has reached the maximum
    /// number of iterations (set via `with_max_function_call_loops()`) without
    /// the model returning a response without function calls.
    ///
    /// The response contains the last response from the model, which likely still
    /// contains pending function calls. Use [`AutoFunctionResultAccumulator`] to
    /// collect all function execution results from prior `FunctionResults` chunks.
    ///
    /// This allows debugging why the model is stuck in a loop while preserving
    /// all partial results.
    MaxLoopsReached(InteractionResponse),

    /// Unknown event type (for forward compatibility).
    ///
    /// This variant is used when deserializing JSON that contains an unrecognized
    /// `chunk_type`. This allows the library to gracefully handle new event types
    /// added by the API in future versions without failing deserialization.
    ///
    /// The `chunk_type` field contains the unrecognized type string, and `data`
    /// contains the full JSON data for inspection or debugging.
    Unknown {
        /// The unrecognized chunk type from the API
        chunk_type: String,
        /// The raw JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl AutoFunctionStreamChunk {
    /// Check if this is an unknown chunk type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Check if this chunk is a Delta variant.
    #[must_use]
    pub const fn is_delta(&self) -> bool {
        matches!(self, Self::Delta(_))
    }

    /// Check if this chunk is a Complete variant.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }

    /// Returns the chunk type name if this is an unknown chunk type.
    ///
    /// Returns `None` for known chunk types.
    #[must_use]
    pub fn unknown_chunk_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { chunk_type, .. } => Some(chunk_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown chunk type.
    ///
    /// Returns `None` for known chunk types.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for AutoFunctionStreamChunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Delta(content) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "delta")?;
                map.serialize_entry("data", content)?;
                map.end()
            }
            Self::ExecutingFunctions {
                response,
                pending_calls,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "executing_functions")?;
                // Serialize as nested object with both fields
                let data = serde_json::json!({
                    "response": response,
                    "pending_calls": pending_calls,
                });
                map.serialize_entry("data", &data)?;
                map.end()
            }
            Self::FunctionResults(results) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "function_results")?;
                map.serialize_entry("data", results)?;
                map.end()
            }
            Self::Complete(response) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "complete")?;
                map.serialize_entry("data", response)?;
                map.end()
            }
            Self::MaxLoopsReached(response) => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", "max_loops_reached")?;
                map.serialize_entry("data", response)?;
                map.end()
            }
            Self::Unknown { chunk_type, data } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("chunk_type", chunk_type)?;
                if !data.is_null() {
                    map.serialize_entry("data", data)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for AutoFunctionStreamChunk {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        // Helper to extract "data" field with consistent warning for missing data
        fn extract_data_field(value: &serde_json::Value, variant_name: &str) -> serde_json::Value {
            match value.get("data").cloned() {
                Some(d) => d,
                None => {
                    tracing::warn!(
                        "AutoFunctionStreamChunk::{} is missing the 'data' field. \
                         This may indicate a malformed API response.",
                        variant_name
                    );
                    serde_json::Value::Null
                }
            }
        }

        let chunk_type = match value.get("chunk_type") {
            Some(serde_json::Value::String(s)) => s.as_str(),
            Some(other) => {
                tracing::warn!(
                    "AutoFunctionStreamChunk received non-string chunk_type: {}. \
                     This may indicate a malformed API response.",
                    other
                );
                "<non-string chunk_type>"
            }
            None => {
                tracing::warn!(
                    "AutoFunctionStreamChunk is missing required chunk_type field. \
                     This may indicate a malformed API response."
                );
                "<missing chunk_type>"
            }
        };

        match chunk_type {
            "delta" => {
                let data = extract_data_field(&value, "Delta");
                let content: Content = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize AutoFunctionStreamChunk::Delta data: {}",
                        e
                    ))
                })?;
                Ok(Self::Delta(content))
            }
            "executing_functions" => {
                let data = extract_data_field(&value, "ExecutingFunctions");

                let response = serde_json::from_value(
                    data.get("response")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                )
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize ExecutingFunctions response: {}",
                        e
                    ))
                })?;
                let pending_calls = serde_json::from_value(
                    data.get("pending_calls")
                        .cloned()
                        .unwrap_or(serde_json::json!([])),
                )
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize ExecutingFunctions pending_calls: {}",
                        e
                    ))
                })?;

                Ok(Self::ExecutingFunctions {
                    response,
                    pending_calls,
                })
            }
            "function_results" => {
                let data = extract_data_field(&value, "FunctionResults");
                let results: Vec<FunctionExecutionResult> =
                    serde_json::from_value(data).map_err(|e| {
                        serde::de::Error::custom(format!(
                            "Failed to deserialize AutoFunctionStreamChunk::FunctionResults data: {}",
                            e
                        ))
                    })?;
                Ok(Self::FunctionResults(results))
            }
            "complete" => {
                let data = extract_data_field(&value, "Complete");
                let response: InteractionResponse = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize AutoFunctionStreamChunk::Complete data: {}",
                        e
                    ))
                })?;
                Ok(Self::Complete(response))
            }
            "max_loops_reached" => {
                let data = extract_data_field(&value, "MaxLoopsReached");
                let response: InteractionResponse = serde_json::from_value(data).map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize AutoFunctionStreamChunk::MaxLoopsReached data: {}",
                        e
                    ))
                })?;
                Ok(Self::MaxLoopsReached(response))
            }
            other => {
                tracing::warn!(
                    "Encountered unknown AutoFunctionStreamChunk type '{}'. \
                     This may indicate a new API feature. \
                     The chunk will be preserved in the Unknown variant.",
                    other
                );
                let data = value
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                Ok(Self::Unknown {
                    chunk_type: other.to_string(),
                    data,
                })
            }
        }
    }
}

/// Streaming event with position metadata for auto-function stream resumption.
///
/// This wrapper pairs an [`AutoFunctionStreamChunk`] with its `event_id`,
/// enabling stream resumption after network interruptions or reconnects.
///
/// # Stream Resumption
///
/// Save the `event_id` from each event. If the connection drops, you can resume
/// the stream from the last received event by calling `get_interaction_stream()`
/// with the saved `event_id`.
///
/// **Note**: The auto-function streaming loop is client-side. If interrupted during
/// function execution, you may need to restart the full loop rather than resuming.
/// However, the underlying API stream can be resumed.
///
/// # Example
///
/// ```no_run
/// use futures_util::StreamExt;
/// use genai_rs::{Client, AutoFunctionStreamEvent, AutoFunctionStreamChunk};
///
/// # async fn example() -> Result<(), genai_rs::GenaiError> {
/// let client = Client::new("your-api-key".to_string());
///
/// let mut stream = client
///     .interaction()
///     .with_model("gemini-3-flash-preview")
///     .with_text("What's the weather in London?")
///     .create_stream_with_auto_functions();
///
/// let mut last_event_id: Option<String> = None;
///
/// while let Some(event) = stream.next().await {
///     let event = event?;
///
///     // Save event_id for potential resume
///     if let Some(id) = &event.event_id {
///         last_event_id = Some(id.clone());
///     }
///
///     match &event.chunk {
///         AutoFunctionStreamChunk::Delta(content) => {
///             if let Some(text) = content.as_text() {
///                 print!("{}", text);
///             }
///         }
///         AutoFunctionStreamChunk::Complete(_) => {
///             println!("\n[Done]");
///         }
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AutoFunctionStreamEvent {
    /// The auto-function stream chunk content
    pub chunk: AutoFunctionStreamChunk,
    /// Event ID for stream resumption.
    ///
    /// Pass this to `last_event_id` when resuming a stream to continue from
    /// this position. This is the `event_id` from the underlying API stream.
    ///
    /// May be `None` for client-generated events (like `ExecutingFunctions`
    /// and `FunctionResults`) that don't come from the API stream.
    pub event_id: Option<String>,
}

impl AutoFunctionStreamEvent {
    /// Creates a new auto-function stream event.
    #[must_use]
    pub fn new(chunk: AutoFunctionStreamChunk, event_id: Option<String>) -> Self {
        Self { chunk, event_id }
    }

    /// Check if the inner chunk is a Delta variant.
    #[must_use]
    pub const fn is_delta(&self) -> bool {
        self.chunk.is_delta()
    }

    /// Check if the inner chunk is a Complete variant.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.chunk.is_complete()
    }

    /// Check if the inner chunk is an Unknown variant.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        self.chunk.is_unknown()
    }

    /// Returns the unrecognized chunk type if this is an Unknown variant.
    #[must_use]
    pub fn unknown_chunk_type(&self) -> Option<&str> {
        self.chunk.unknown_chunk_type()
    }

    /// Returns the preserved JSON data if this is an Unknown variant.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        self.chunk.unknown_data()
    }
}

impl Serialize for AutoFunctionStreamEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("chunk", &self.chunk)?;
        if let Some(id) = &self.event_id {
            map.serialize_entry("event_id", id)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for AutoFunctionStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        let chunk = match value.get("chunk") {
            Some(chunk_value) => {
                serde_json::from_value(chunk_value.clone()).map_err(serde::de::Error::custom)?
            }
            None => {
                return Err(serde::de::Error::missing_field("chunk"));
            }
        };

        let event_id = value
            .get("event_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(Self { chunk, event_id })
    }
}

/// Result of executing a function locally.
///
/// This represents the output from a function that was executed by the library
/// during automatic function calling. It contains the function name, the call ID
/// (used to match with the original request), the arguments that were passed,
/// and the result value.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::FunctionExecutionResult;
/// # let result: FunctionExecutionResult = todo!();
/// println!("Function {} called with: {}", result.name, result.args);
/// println!("  Returned: {}", result.result);
/// println!("  Call ID: {}, Duration: {:?}", result.call_id, result.duration);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FunctionExecutionResult {
    /// Name of the function that was called
    pub name: String,
    /// The call_id from the FunctionCall this result responds to
    pub call_id: String,
    /// The arguments passed to the function
    pub args: serde_json::Value,
    /// The result returned by the function
    pub result: serde_json::Value,
    /// How long the function took to execute
    #[serde(with = "duration_millis")]
    pub duration: Duration,
}

impl FunctionExecutionResult {
    /// Creates a new function execution result.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        call_id: impl Into<String>,
        args: serde_json::Value,
        result: serde_json::Value,
        duration: Duration,
    ) -> Self {
        Self {
            name: name.into(),
            call_id: call_id.into(),
            args,
            result,
            duration,
        }
    }

    /// Returns true if this execution resulted in an error.
    ///
    /// Errors occur when:
    /// - The function was not found in the registry or tool service
    /// - The function execution failed (panicked or returned an error)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = client.interaction()
    ///     .with_text("What's the weather?")
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// for execution in &result.executions {
    ///     if execution.is_error() {
    ///         eprintln!("Function {} failed: {:?}", execution.name, execution.result);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.result.get("error").is_some()
    }

    /// Returns true if this execution succeeded (no error).
    #[must_use]
    pub fn is_success(&self) -> bool {
        !self.is_error()
    }

    /// Returns the error message if this execution failed, None otherwise.
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.result.get("error").and_then(|v| v.as_str())
    }
}

/// Serialize Duration as milliseconds for JSON compatibility
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_millis().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

/// Result from `create_with_auto_functions()` containing the final response
/// and a history of all function executions.
///
/// This type provides visibility into which functions were called during
/// automatic function execution, useful for debugging, logging, and evaluation.
///
/// # Memory Considerations
///
/// The `executions` vector accumulates all [`FunctionExecutionResult`] records
/// from each iteration of the auto-function loop. For typical use cases (1-5
/// iterations with 1-3 functions each), this is negligible.
///
/// For edge cases with many function calls or large result payloads, consider:
///
/// - **Limit iterations**: Use [`crate::InteractionBuilder::with_max_function_call_loops()`]
///   to cap the maximum number of iterations (default: 5)
/// - **Extract and drop**: Extract only the data you need, then drop the result
/// - **Manual control**: For fine-grained memory management, implement function
///   calling manually using [`crate::InteractionBuilder::add_functions()`] and
///   [`crate::InteractionBuilder::create()`] instead of the auto-function helpers
///
/// Each `FunctionExecutionResult` contains the function name, call ID, result
/// value (as `serde_json::Value`), and execution duration. Memory usage scales
/// primarily with the size of function result payloads.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Client, AutoFunctionResult};
/// # async fn example() -> Result<(), genai_rs::GenaiError> {
/// # let client = Client::new("key".to_string());
/// let result = client
///     .interaction()
///     .with_model("gemini-3-flash-preview")
///     .with_text("What's the weather in London?")
///     .create_with_auto_functions()
///     .await?;
///
/// // Access the final response
/// if let Some(text) = result.response.as_text() {
///     println!("Answer: {}", text);
/// }
///
/// // Access execution history
/// for exec in &result.executions {
///     println!("Called {} -> {}", exec.name, exec.result);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AutoFunctionResult {
    /// The final response from the model (after all function calls completed)
    pub response: InteractionResponse,
    /// All functions that were executed during the auto-function loop
    pub executions: Vec<FunctionExecutionResult>,
    /// Whether the auto-function loop was terminated due to reaching the maximum
    /// number of iterations (set via `with_max_function_call_loops()`).
    ///
    /// When `true`, the `response` contains the last response from the model before
    /// hitting the limit, which likely still contains pending function calls.
    /// The `executions` vector contains all functions that were successfully executed
    /// before the limit was reached.
    ///
    /// This allows debugging why the model is stuck in a loop and preserves
    /// partial results that may still be useful.
    #[serde(default)]
    pub reached_max_loops: bool,
}

impl AutoFunctionResult {
    /// Returns true if all function executions succeeded (no errors).
    ///
    /// This is useful for detecting missing function implementations or
    /// function execution failures that would otherwise be silently
    /// sent to the model as error results.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let result = client.interaction()
    ///     .with_text("What's the weather?")
    ///     .add_functions(vec![get_weather_function()])
    ///     .create_with_auto_functions()
    ///     .await?;
    ///
    /// assert!(result.all_executions_succeeded(),
    ///     "Function executions failed: {:?}",
    ///     result.failed_executions());
    /// ```
    #[must_use]
    pub fn all_executions_succeeded(&self) -> bool {
        self.executions.iter().all(|e| e.is_success())
    }

    /// Returns all executions that failed (had errors).
    #[must_use]
    pub fn failed_executions(&self) -> Vec<&FunctionExecutionResult> {
        self.executions.iter().filter(|e| e.is_error()).collect()
    }
}

/// Accumulator for building [`AutoFunctionResult`] from a stream of [`AutoFunctionStreamChunk`].
///
/// This helper collects all function execution results and the final response from
/// a streaming auto-function interaction, producing the same result type as the
/// non-streaming `create_with_auto_functions()` method.
///
/// # Example
///
/// ```no_run
/// use futures_util::StreamExt;
/// use genai_rs::{Client, AutoFunctionStreamChunk, AutoFunctionResultAccumulator};
///
/// # async fn example() -> Result<(), genai_rs::GenaiError> {
/// let client = Client::new("your-api-key".to_string());
///
/// let mut stream = client
///     .interaction()
///     .with_model("gemini-3-flash-preview")
///     .with_text("What's the weather in London?")
///     .create_stream_with_auto_functions();
///
/// let mut accumulator = AutoFunctionResultAccumulator::new();
///
/// while let Some(event) = stream.next().await {
///     let event = event?;
///
///     // Process deltas for UI updates
///     if let AutoFunctionStreamChunk::Delta(content) = &event.chunk {
///         if let Some(text) = content.as_text() {
///             print!("{}", text);
///         }
///     }
///
///     // Feed all chunks to the accumulator
///     if let Some(result) = accumulator.push(event.chunk) {
///         // Stream is complete, we have the full result
///         println!("\n\nExecuted {} functions", result.executions.len());
///         for exec in &result.executions {
///             println!("  {} took {:?}", exec.name, exec.duration);
///         }
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct AutoFunctionResultAccumulator {
    executions: Vec<FunctionExecutionResult>,
}

impl AutoFunctionResultAccumulator {
    /// Creates a new empty accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a chunk to the accumulator.
    ///
    /// Returns `Some(AutoFunctionResult)` when the stream ends, either:
    /// - With a `Complete` chunk (normal completion, `reached_max_loops: false`)
    /// - With a `MaxLoopsReached` chunk (limit hit, `reached_max_loops: true`)
    ///
    /// Returns `None` for all other chunk types.
    ///
    /// The accumulator collects all `FunctionResults` chunks and combines them
    /// with the final response.
    #[must_use]
    #[allow(unreachable_patterns)] // Handle future variants from #[non_exhaustive] enum
    pub fn push(&mut self, chunk: AutoFunctionStreamChunk) -> Option<AutoFunctionResult> {
        match chunk {
            AutoFunctionStreamChunk::FunctionResults(results) => {
                self.executions.extend(results);
                None
            }
            AutoFunctionStreamChunk::Complete(response) => Some(AutoFunctionResult {
                response,
                executions: std::mem::take(&mut self.executions),
                reached_max_loops: false,
            }),
            AutoFunctionStreamChunk::MaxLoopsReached(response) => Some(AutoFunctionResult {
                response,
                executions: std::mem::take(&mut self.executions),
                reached_max_loops: true,
            }),
            AutoFunctionStreamChunk::Delta(_)
            | AutoFunctionStreamChunk::ExecutingFunctions { .. } => None,
            // Handle future variants gracefully
            _ => None,
        }
    }

    /// Returns the accumulated executions so far.
    ///
    /// Useful for checking progress without consuming the accumulator.
    #[must_use]
    pub fn executions(&self) -> &[FunctionExecutionResult] {
        &self.executions
    }

    /// Resets the accumulator to its initial empty state.
    pub fn reset(&mut self) {
        self.executions.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_function_execution_result() {
        let result = FunctionExecutionResult::new(
            "get_weather",
            "call-123",
            json!({"city": "Seattle"}),
            json!({"temp": 20, "unit": "celsius"}),
            Duration::from_millis(42),
        );

        assert_eq!(result.name, "get_weather");
        assert_eq!(result.call_id, "call-123");
        assert_eq!(result.args, json!({"city": "Seattle"}));
        assert_eq!(result.result, json!({"temp": 20, "unit": "celsius"}));
        assert_eq!(result.duration, Duration::from_millis(42));
    }

    #[test]
    fn test_auto_function_stream_chunk_variants() {
        // Test that Delta and FunctionResults variants can be created
        let _delta = AutoFunctionStreamChunk::Delta(Content::Text {
            text: Some("Hello".to_string()),
            annotations: None,
        });

        let _results = AutoFunctionStreamChunk::FunctionResults(vec![FunctionExecutionResult {
            name: "test".to_string(),
            call_id: "1".to_string(),
            args: json!({}),
            result: json!({"ok": true}),
            duration: Duration::from_millis(10),
        }]);

        // Note: ExecutingFunctions and Complete require InteractionResponse which is harder to construct in tests
    }

    #[test]
    fn test_function_execution_result_serialization() {
        let result = FunctionExecutionResult::new(
            "get_weather",
            "call-456",
            json!({"city": "Miami"}),
            json!({"temp": 22, "conditions": "sunny"}),
            Duration::from_millis(150),
        );

        let json_str = serde_json::to_string(&result).expect("Serialization should succeed");

        // Verify key fields are present in serialized output
        assert!(
            json_str.contains("get_weather"),
            "Should contain function name"
        );
        assert!(json_str.contains("call-456"), "Should contain call_id");
        assert!(json_str.contains("sunny"), "Should contain result data");

        // Verify full roundtrip
        let deserialized: FunctionExecutionResult =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");
        assert_eq!(deserialized, result);
    }

    #[test]
    fn test_auto_function_stream_chunk_serialization_roundtrip() {
        // Test Delta variant roundtrip
        let delta = AutoFunctionStreamChunk::Delta(Content::Text {
            text: Some("Hello, world!".to_string()),
            annotations: None,
        });

        let json_str = serde_json::to_string(&delta).expect("Serialization should succeed");
        assert!(json_str.contains("chunk_type"), "Should contain tag field");
        assert!(json_str.contains("delta"), "Should contain variant name");
        assert!(json_str.contains("Hello, world!"), "Should contain text");

        let deserialized: AutoFunctionStreamChunk =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");

        match deserialized {
            AutoFunctionStreamChunk::Delta(content) => {
                assert_eq!(content.as_text(), Some("Hello, world!"));
            }
            _ => panic!("Expected Delta variant"),
        }

        // Test FunctionResults variant roundtrip
        let results = AutoFunctionStreamChunk::FunctionResults(vec![
            FunctionExecutionResult::new(
                "get_weather",
                "call-1",
                json!({"city": "Tokyo"}),
                json!({"temp": 20}),
                Duration::from_millis(50),
            ),
            FunctionExecutionResult::new(
                "get_time",
                "call-2",
                json!({"timezone": "UTC"}),
                json!({"time": "14:30"}),
                Duration::from_millis(30),
            ),
        ]);

        let json_str = serde_json::to_string(&results).expect("Serialization should succeed");
        let deserialized: AutoFunctionStreamChunk =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");

        match deserialized {
            AutoFunctionStreamChunk::FunctionResults(execs) => {
                assert_eq!(execs.len(), 2);
                assert_eq!(execs[0].name, "get_weather");
                assert_eq!(execs[1].name, "get_time");
            }
            _ => panic!("Expected FunctionResults variant"),
        }

        // Test Unknown variant handling (forward compatibility)
        let unknown_json = r#"{"chunk_type": "future_event_type", "data": {"key": "value"}}"#;
        let deserialized: AutoFunctionStreamChunk =
            serde_json::from_str(unknown_json).expect("Should deserialize unknown variant");

        // Verify it's an Unknown variant with data preserved
        assert!(deserialized.is_unknown());
        assert_eq!(deserialized.unknown_chunk_type(), Some("future_event_type"));
        let data = deserialized.unknown_data().expect("Should have data");
        assert_eq!(data["key"], "value");

        // Verify roundtrip serialization
        let reserialized = serde_json::to_string(&deserialized).expect("Should serialize");
        assert!(reserialized.contains("future_event_type"));
        assert!(reserialized.contains("value"));
    }

    #[test]
    fn test_auto_function_stream_chunk_unknown_without_data() {
        // Test unknown chunk type without data field
        let unknown_json = r#"{"chunk_type": "no_data_chunk"}"#;
        let deserialized: AutoFunctionStreamChunk =
            serde_json::from_str(unknown_json).expect("Should deserialize unknown variant");

        assert!(deserialized.is_unknown());
        assert_eq!(deserialized.unknown_chunk_type(), Some("no_data_chunk"));

        // Data should be null when not provided
        let data = deserialized.unknown_data().expect("Should have data field");
        assert!(data.is_null());
    }

    #[test]
    fn test_auto_function_result_roundtrip() {
        use crate::InteractionStatus;

        // Create a realistic AutoFunctionResult with multiple executions
        let result = AutoFunctionResult {
            response: crate::InteractionResponse {
                id: Some("interaction-abc123".to_string()),
                model: Some("gemini-3-flash-preview".to_string()),
                agent: None,
                input: vec![Content::Text {
                    text: Some("What's the weather in Paris and London?".to_string()),
                    annotations: None,
                }],
                outputs: vec![
                    Content::Text {
                        text: Some("Based on the weather data:".to_string()),
                        annotations: None,
                    },
                    Content::Text {
                        text: Some("Paris is 18°C and London is 15°C.".to_string()),
                        annotations: None,
                    },
                ],
                status: InteractionStatus::Completed,
                usage: Some(crate::UsageMetadata {
                    total_input_tokens: Some(50),
                    total_output_tokens: Some(30),
                    total_tokens: Some(80),
                    ..Default::default()
                }),
                tools: None,
                grounding_metadata: None,
                url_context_metadata: None,
                previous_interaction_id: Some("prev-interaction-xyz".to_string()),
                created: None,
                updated: None,
            },
            executions: vec![
                FunctionExecutionResult::new(
                    "get_weather",
                    "call-001",
                    json!({"city": "Paris"}),
                    json!({"city": "Paris", "temp": 18, "unit": "celsius"}),
                    Duration::from_millis(120),
                ),
                FunctionExecutionResult::new(
                    "get_weather",
                    "call-002",
                    json!({"city": "London"}),
                    json!({"city": "London", "temp": 15, "unit": "celsius"}),
                    Duration::from_millis(95),
                ),
            ],
            reached_max_loops: false,
        };

        // Serialize
        let json_str = serde_json::to_string(&result).expect("Serialization should succeed");

        // Verify key data is present in JSON
        assert!(
            json_str.contains("interaction-abc123"),
            "Should contain interaction ID"
        );
        assert!(
            json_str.contains("gemini-3-flash-preview"),
            "Should contain model name"
        );
        assert!(
            json_str.contains("get_weather"),
            "Should contain function name"
        );
        assert!(
            json_str.contains("call-001"),
            "Should contain first call_id"
        );
        assert!(
            json_str.contains("call-002"),
            "Should contain second call_id"
        );
        assert!(json_str.contains("Paris"), "Should contain Paris");
        assert!(json_str.contains("London"), "Should contain London");
        assert!(
            json_str.contains("prev-interaction-xyz"),
            "Should contain previous interaction ID"
        );

        // Deserialize
        let deserialized: AutoFunctionResult =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");

        // Verify response fields
        assert_eq!(
            deserialized.response.id.as_deref(),
            Some("interaction-abc123")
        );
        assert_eq!(
            deserialized.response.model,
            Some("gemini-3-flash-preview".to_string())
        );
        assert_eq!(deserialized.response.status, InteractionStatus::Completed);
        assert_eq!(
            deserialized.response.previous_interaction_id,
            Some("prev-interaction-xyz".to_string())
        );

        // Verify usage metadata
        let usage = deserialized.response.usage.expect("Should have usage");
        assert_eq!(usage.total_input_tokens, Some(50));
        assert_eq!(usage.total_output_tokens, Some(30));
        assert_eq!(usage.total_tokens, Some(80));

        // Verify executions
        assert_eq!(deserialized.executions.len(), 2);
        assert_eq!(deserialized.executions[0].name, "get_weather");
        assert_eq!(deserialized.executions[0].call_id, "call-001");
        assert_eq!(deserialized.executions[0].result["city"], "Paris");
        assert_eq!(deserialized.executions[1].name, "get_weather");
        assert_eq!(deserialized.executions[1].call_id, "call-002");
        assert_eq!(deserialized.executions[1].result["city"], "London");

        // Verify reached_max_loops
        assert!(!deserialized.reached_max_loops);
    }

    #[test]
    fn test_auto_function_result_reached_max_loops() {
        use crate::InteractionStatus;

        // Create an AutoFunctionResult with reached_max_loops: true
        let result = AutoFunctionResult {
            response: crate::InteractionResponse {
                id: Some("interaction-stuck".to_string()),
                model: Some("gemini-3-flash-preview".to_string()),
                agent: None,
                input: vec![Content::Text {
                    text: Some("What's the weather?".to_string()),
                    annotations: None,
                }],
                outputs: vec![Content::FunctionCall {
                    id: Some("call-stuck".to_string()),
                    name: "get_weather".to_string(),
                    args: json!({"city": "Tokyo"}),
                    partial_args: None,
                    will_continue: None,
                }],
                status: InteractionStatus::Completed,
                usage: None,
                tools: None,
                grounding_metadata: None,
                url_context_metadata: None,
                previous_interaction_id: None,
                created: None,
                updated: None,
            },
            executions: vec![FunctionExecutionResult::new(
                "get_weather",
                "call-1",
                json!({"city": "Berlin"}),
                json!({"temp": 25}),
                Duration::from_millis(50),
            )],
            reached_max_loops: true,
        };

        // Serialize
        let json_str = serde_json::to_string(&result).expect("Serialization should succeed");
        assert!(
            json_str.contains("reached_max_loops"),
            "Should contain reached_max_loops field"
        );
        assert!(json_str.contains("true"), "Should contain true value");

        // Deserialize
        let deserialized: AutoFunctionResult =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");
        assert!(deserialized.reached_max_loops);
        assert_eq!(deserialized.executions.len(), 1);
    }

    #[test]
    fn test_auto_function_result_backwards_compatibility() {
        // Test that JSON without reached_max_loops (from older versions) still deserializes
        let legacy_json = r#"{
            "response": {
                "id": "interaction-old",
                "model": "gemini-3-flash-preview",
                "agent": null,
                "input": [],
                "outputs": [],
                "status": "COMPLETED",
                "usage": null,
                "tools": null,
                "grounding_metadata": null,
                "url_context_metadata": null,
                "previous_interaction_id": null
            },
            "executions": []
        }"#;

        let deserialized: AutoFunctionResult =
            serde_json::from_str(legacy_json).expect("Should deserialize legacy JSON");

        // reached_max_loops should default to false
        assert!(
            !deserialized.reached_max_loops,
            "Missing field should default to false"
        );
    }

    #[test]
    fn test_max_loops_reached_chunk_roundtrip() {
        use crate::InteractionStatus;

        // Create a MaxLoopsReached chunk
        let response = crate::InteractionResponse {
            id: Some("interaction-max-loops".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            agent: None,
            input: vec![],
            outputs: vec![Content::FunctionCall {
                id: Some("call-pending".to_string()),
                name: "stuck_function".to_string(),
                args: json!({}),
                partial_args: None,
                will_continue: None,
            }],
            status: InteractionStatus::Completed,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        };

        let chunk = AutoFunctionStreamChunk::MaxLoopsReached(response);

        // Serialize
        let json_str = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(
            json_str.contains("max_loops_reached"),
            "Should contain chunk_type"
        );
        assert!(
            json_str.contains("interaction-max-loops"),
            "Should contain response data"
        );

        // Deserialize
        let deserialized: AutoFunctionStreamChunk =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");

        match deserialized {
            AutoFunctionStreamChunk::MaxLoopsReached(resp) => {
                assert_eq!(resp.id.as_deref(), Some("interaction-max-loops"));
                assert_eq!(resp.function_calls().len(), 1);
                assert_eq!(resp.function_calls()[0].name, "stuck_function");
            }
            other => panic!("Expected MaxLoopsReached, got {:?}", other),
        }
    }

    #[test]
    fn test_accumulator_handles_max_loops_reached() {
        use crate::InteractionStatus;

        let mut accumulator = AutoFunctionResultAccumulator::new();

        // Simulate function results being yielded
        let results = AutoFunctionStreamChunk::FunctionResults(vec![FunctionExecutionResult::new(
            "test_func",
            "call-1",
            json!({}),
            json!({"ok": true}),
            Duration::from_millis(10),
        )]);

        assert!(
            accumulator.push(results).is_none(),
            "Should not complete yet"
        );
        assert_eq!(accumulator.executions().len(), 1);

        // Simulate MaxLoopsReached being yielded
        let response = crate::InteractionResponse {
            id: Some("max-loops-response".to_string()),
            model: Some("gemini-3-flash-preview".to_string()),
            agent: None,
            input: vec![],
            outputs: vec![],
            status: InteractionStatus::Completed,
            usage: None,
            tools: None,
            grounding_metadata: None,
            url_context_metadata: None,
            previous_interaction_id: None,
            created: None,
            updated: None,
        };

        let max_loops_chunk = AutoFunctionStreamChunk::MaxLoopsReached(response);
        let result = accumulator.push(max_loops_chunk);

        assert!(result.is_some(), "Should complete on MaxLoopsReached");
        let result = result.unwrap();
        assert!(
            result.reached_max_loops,
            "Should have reached_max_loops: true"
        );
        assert_eq!(result.executions.len(), 1);
        assert_eq!(result.response.id.as_deref(), Some("max-loops-response"));
    }

    #[test]
    fn test_auto_function_stream_event_with_event_id_roundtrip() {
        let event = AutoFunctionStreamEvent::new(
            AutoFunctionStreamChunk::Delta(Content::Text {
                text: Some("Hello from auto-function".to_string()),
                annotations: None,
            }),
            Some("evt_auto_abc123".to_string()),
        );

        // Test helper methods
        assert!(event.is_delta());
        assert!(!event.is_complete());
        assert!(!event.is_unknown());

        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        assert!(json.contains("evt_auto_abc123"), "Should have event_id");
        assert!(
            json.contains("Hello from auto-function"),
            "Should have content"
        );

        let deserialized: AutoFunctionStreamEvent =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        assert_eq!(deserialized.event_id.as_deref(), Some("evt_auto_abc123"));
        assert!(deserialized.is_delta());
    }

    #[test]
    fn test_auto_function_stream_event_without_event_id() {
        // Client-generated events like FunctionResults don't have event_id
        let event = AutoFunctionStreamEvent::new(
            AutoFunctionStreamChunk::FunctionResults(vec![FunctionExecutionResult::new(
                "weather",
                "call-123",
                json!({"city": "Denver"}),
                json!({"temp": 72}),
                Duration::from_millis(50),
            )]),
            None,
        );

        assert!(!event.is_delta());
        assert!(!event.is_complete());
        assert!(event.event_id.is_none());

        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        assert!(!json.contains("event_id"), "Should not have event_id field");
        assert!(json.contains("weather"), "Should have function name");

        let deserialized: AutoFunctionStreamEvent =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        assert!(deserialized.event_id.is_none());
    }

    #[test]
    fn test_auto_function_stream_event_with_empty_event_id() {
        // Edge case: empty string event_id should still serialize/deserialize
        let event = AutoFunctionStreamEvent::new(
            AutoFunctionStreamChunk::Delta(Content::Text {
                text: Some("Test".to_string()),
                annotations: None,
            }),
            Some(String::new()),
        );

        let json = serde_json::to_string(&event).expect("Serialization should succeed");
        assert!(
            json.contains(r#""event_id":"""#),
            "Should have empty event_id"
        );

        let deserialized: AutoFunctionStreamEvent =
            serde_json::from_str(&json).expect("Deserialization should succeed");
        assert_eq!(deserialized.event_id.as_deref(), Some(""));
    }

    #[test]
    fn test_pending_function_call() {
        let call = PendingFunctionCall::new("get_weather", "call-123", json!({"city": "Seattle"}));

        assert_eq!(call.name, "get_weather");
        assert_eq!(call.call_id, "call-123");
        assert_eq!(call.args, json!({"city": "Seattle"}));
    }

    #[test]
    fn test_pending_function_call_serialization_roundtrip() {
        let call = PendingFunctionCall::new("test_func", "id-456", json!({"key": "value"}));

        let json_str = serde_json::to_string(&call).expect("Serialization should succeed");
        assert!(json_str.contains("test_func"));
        assert!(json_str.contains("id-456"));

        let deserialized: PendingFunctionCall =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");
        assert_eq!(deserialized, call);
    }

    #[test]
    fn test_executing_functions_new_format_roundtrip() {
        use crate::InteractionStatus;

        let chunk = AutoFunctionStreamChunk::ExecutingFunctions {
            response: crate::InteractionResponse {
                id: Some("interaction-new".to_string()),
                model: Some("gemini-3-flash-preview".to_string()),
                agent: None,
                input: vec![],
                outputs: vec![],
                status: InteractionStatus::Completed,
                usage: None,
                tools: None,
                grounding_metadata: None,
                url_context_metadata: None,
                previous_interaction_id: None,
                created: None,
                updated: None,
            },
            pending_calls: vec![
                PendingFunctionCall::new("func1", "call-1", json!({"a": 1})),
                PendingFunctionCall::new("func2", "call-2", json!({"b": 2})),
            ],
        };

        let json_str = serde_json::to_string(&chunk).expect("Serialization should succeed");
        assert!(json_str.contains("pending_calls"));
        assert!(json_str.contains("func1"));
        assert!(json_str.contains("func2"));

        let deserialized: AutoFunctionStreamChunk =
            serde_json::from_str(&json_str).expect("Deserialization should succeed");

        match deserialized {
            AutoFunctionStreamChunk::ExecutingFunctions {
                response,
                pending_calls,
            } => {
                assert_eq!(response.id.as_deref(), Some("interaction-new"));
                assert_eq!(pending_calls.len(), 2);
                assert_eq!(pending_calls[0].name, "func1");
                assert_eq!(pending_calls[1].name, "func2");
            }
            _ => panic!("Expected ExecutingFunctions variant"),
        }
    }
}
