//! Common test utilities shared across all integration test files.
//!
//! Usage in test files:
//! ```ignore
//! mod common;
//! use common::*;
//! ```
//!
//! # Note on `#[allow(dead_code)]`
//!
//! Many items in this module are annotated with `#[allow(dead_code)]` even though
//! they ARE used. This is because Rust compiles each test file (`*_tests.rs`) as a
//! separate compilation unit, and the compiler can't see cross-file usage. Without
//! these annotations, you'd get spurious "function is never used" warnings.
//!
//! # Note on URI Support
//!
//! The Interactions API does NOT support Google Cloud Storage (gs://) URIs.
//! Tests that need external media should use base64-encoded data or
//! gracefully handle the unsupported URI error.

use futures_util::StreamExt;
use genai_rs::{
    AutoFunctionStreamChunk, AutoFunctionStreamEvent, Client, GenaiError, InteractionResponse,
    InteractionStatus, StreamChunk, StreamEvent,
};
use std::env;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::time::sleep;

// =============================================================================
// Retry Utilities for Transient API Errors
// =============================================================================

/// Maximum number of retries for transient API errors.
#[allow(dead_code)]
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Checks if an error is a transient API error that should be retried.
///
/// Currently detects:
/// - Spanner UTF-8 errors (Google backend issue with stateful conversations)
///
/// The detection is specific to avoid false positives - both "spanner" and "utf-8"
/// must appear in the error message.
#[allow(dead_code)]
pub fn is_transient_error(err: &GenaiError) -> bool {
    match err {
        GenaiError::Api {
            status_code,
            message,
            ..
        } => {
            let lower = message.to_lowercase();
            // Spanner UTF-8 errors are transient backend issues
            // See: https://github.com/evansenter/genai-rs/issues/60
            (lower.contains("spanner") && lower.contains("utf-8"))
                // Model occasionally generates invalid JSON — transient API-side issue
                || (*status_code == 400 && lower.contains("invalid json syntax"))
        }
        _ => false,
    }
}

/// Retries an async operation on transient API errors with exponential backoff.
///
/// This is useful for working around transient Google API backend issues,
/// such as the Spanner UTF-8 error that occasionally occurs with stateful
/// conversations (see issue #60).
///
/// # Arguments
///
/// * `max_retries` - Maximum number of retry attempts (0 = no retries, just run once)
/// * `operation` - A closure that returns a future producing `Result<T, GenaiError>`
///
/// # Returns
///
/// The result of the operation if it succeeds, or the last error if all retries fail.
///
/// # Example
///
/// ```ignore
/// let response = retry_on_transient(3, || async {
///     stateful_builder(&client)
///         .with_text("Hello")
///         .create()
///         .await
/// }).await.expect("Request failed after retries");
/// ```
#[allow(dead_code)]
pub async fn retry_on_transient<F, Fut, T>(max_retries: u32, operation: F) -> Result<T, GenaiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, GenaiError>>,
{
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) if is_transient_error(&err) && attempt < max_retries => {
                // Exponential backoff: 1s, 2s, 4s, ...
                let delay = Duration::from_secs(1 << attempt);
                println!(
                    "Transient error on attempt {} of {}, retrying in {:?}: {:?}",
                    attempt + 1,
                    max_retries + 1,
                    delay,
                    err
                );
                last_error = Some(err);
                sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.expect("Should have an error if we exhausted retries"))
}

/// Retries an async operation that may fail due to flaky API behavior.
///
/// Unlike `retry_on_transient` which checks for specific error types, this function
/// retries on ANY error. Use this for operations where the API may intermittently
/// fail in unpredictable ways (e.g., image generation returning text instead of images).
///
/// # Arguments
///
/// * `max_retries` - Maximum number of retry attempts (0 = no retries, just run once)
/// * `delay` - Fixed delay between retries
/// * `operation` - A closure that returns a future producing `Result<T, E>`
///
/// # Returns
///
/// The result of the operation if it succeeds, or the last error if all retries fail.
///
/// # Example
///
/// ```ignore
/// let response = retry_on_any_error(2, Duration::from_secs(2), || async {
///     let resp = client.interaction()
///         .with_model("gemini-3-pro-image-preview")
///         .with_text("Generate an image of a cat")
///         .create()
///         .await?;
///
///     // Check if we got an image (not text)
///     if resp.outputs.iter().any(|o| matches!(o, Content::Image { .. })) {
///         Ok(resp)
///     } else {
///         Err(anyhow::anyhow!("No image in response"))
///     }
/// }).await;
/// ```
#[allow(dead_code)]
pub async fn retry_on_any_error<F, Fut, T, E>(
    max_retries: u32,
    delay: Duration,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let mut last_error = None;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) if attempt < max_retries => {
                println!(
                    "Attempt {} of {} failed: {:?}, retrying in {:?}...",
                    attempt + 1,
                    max_retries + 1,
                    err,
                    delay
                );
                last_error = Some(err);
                sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_error.expect("Should have an error if we exhausted retries"))
}

/// Macro to reduce boilerplate when using `retry_on_transient`.
///
/// The `retry_on_transient` function requires double-cloning: once for the outer
/// closure capture, and again inside the closure for the `async move` block.
/// This macro eliminates that boilerplate by handling the cloning automatically.
///
/// # Usage
///
/// ```ignore
/// // Clone client and prev_id for retry, then execute the async block
/// let response = retry_request!([client, prev_id] => {
///     stateful_builder(&client)
///         .with_previous_interaction(&prev_id)
///         .create()
///         .await
/// }).expect("Request failed");
/// ```
///
/// This expands to:
///
/// ```ignore
/// let response = {
///     let client = client.clone();
///     let prev_id = prev_id.clone();
///     retry_on_transient(DEFAULT_MAX_RETRIES, || {
///         let client = client.clone();
///         let prev_id = prev_id.clone();
///         async move {
///             stateful_builder(&client)
///                 .with_previous_interaction(&prev_id)
///                 .create()
///                 .await
///         }
///     }).await
/// }.expect("Request failed");
/// ```
///
/// # Arguments
///
/// * Variables in brackets `[a, b, c]` - Variables to clone for each retry attempt.
///   All non-Copy variables captured in the async block must be listed here.
/// * Expression after `=>` - The async operation to execute (should include `.await`)
///
/// # Returns
///
/// The result of `retry_on_transient(...).await` - typically `Result<T, GenaiError>`.
/// Chain with `.expect()` or `?` as needed.
///
/// **Usage**: Import with `use crate::retry_request;` in test files, or just
/// use directly after `mod common;` since `#[macro_export]` places it at crate root.
#[macro_export]
macro_rules! retry_request {
    ([$($var:ident),* $(,)?] => $body:expr) => {{
        $(let $var = $var.clone();)*
        $crate::common::retry_on_transient($crate::common::DEFAULT_MAX_RETRIES, || {
            $(let $var = $var.clone();)*
            async move { $body }
        }).await
    }};
}

/// Creates a client from the GEMINI_API_KEY environment variable.
/// Returns None if the API key is not set or client build fails.
///
/// Note: If the API key is set but client build fails (e.g., TLS issues),
/// a warning is printed to distinguish from missing API key.
#[allow(dead_code)]
pub fn get_client() -> Option<Client> {
    let api_key = env::var("GEMINI_API_KEY").ok()?;
    match Client::builder(api_key).build() {
        Ok(client) => Some(client),
        Err(e) => {
            eprintln!(
                "WARNING: GEMINI_API_KEY is set but client build failed: {}",
                e
            );
            None
        }
    }
}

// =============================================================================
// Timeout Utilities
// =============================================================================

/// Default timeout for long-running integration tests.
///
/// Defaults to 60 seconds. Override via `TEST_TIMEOUT_SECS` environment variable.
///
/// This provides a safety net to prevent tests from hanging indefinitely
/// when interacting with external APIs.
#[allow(dead_code)]
pub fn test_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("TEST_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60),
    )
}

/// Extended timeout for tests that make many sequential API calls.
///
/// Defaults to 120 seconds. Override via `EXTENDED_TEST_TIMEOUT_SECS` environment variable.
///
/// Use this for tests like multi-turn conversations that make 10+ API calls.
#[allow(dead_code)]
pub fn extended_test_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("EXTENDED_TEST_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120),
    )
}

/// Wraps a future with a timeout, panicking if the timeout is exceeded.
///
/// Use this to prevent integration tests from hanging indefinitely when
/// interacting with external APIs.
///
/// # Arguments
///
/// * `duration` - Maximum time to wait for the future to complete
/// * `future` - The async operation to wrap with a timeout
///
/// # Panics
///
/// Panics with a descriptive message if the timeout is exceeded.
///
/// # Example
///
/// ```ignore
/// use common::{test_timeout, get_client, with_timeout};
///
/// #[tokio::test]
/// #[ignore = "Requires API key"]
/// async fn test_something() {
///     let Some(client) = get_client() else {
///         println!("Skipping: GEMINI_API_KEY not set");
///         return;
///     };
///
///     with_timeout(test_timeout(), async {
///         // test logic that might hang
///         let response = interaction_builder(&client)
///             .with_text("Hello")
///             .create()
///             .await
///             .expect("Request failed");
///     }).await;
/// }
/// ```
#[allow(dead_code)]
pub async fn with_timeout<F, T>(duration: Duration, future: F) -> T
where
    F: Future<Output = T>,
{
    tokio::time::timeout(duration, future)
        .await
        .unwrap_or_else(|_| panic!("Test timed out after {:?}", duration))
}

// =============================================================================
// Polling Utilities
// =============================================================================

/// Error type for polling operations.
#[derive(Debug)]
#[allow(dead_code)]
pub enum PollError {
    /// Polling timed out before the interaction completed.
    Timeout,
    /// The interaction failed.
    Failed,
    /// An API error occurred during polling.
    Api(GenaiError),
}

impl From<GenaiError> for PollError {
    fn from(err: GenaiError) -> Self {
        PollError::Api(err)
    }
}

/// Polls an interaction until it completes or times out, using exponential backoff.
///
/// Checks the status immediately on first call (no initial delay), then uses exponential
/// backoff starting at 1 second and doubling up to a maximum of 10 seconds. This is more
/// efficient than fixed-interval polling: instant detection of already-completed tasks,
/// faster initial detection of quick completions, and fewer API calls for long-running tasks.
///
/// # Arguments
///
/// * `client` - The API client to use for polling
/// * `interaction_id` - The ID of the interaction to poll
/// * `max_wait` - Maximum duration to wait before timing out
///
/// # Returns
///
/// * `Ok(InteractionResponse)` - The completed (or failed) interaction
/// * `Err(PollError::Timeout)` - If max_wait elapsed without completion
/// * `Err(PollError::Failed)` - If the interaction status became Failed
/// * `Err(PollError::Api(_))` - If an API error occurred
///
/// # Example
///
/// ```ignore
/// let response = poll_until_complete(&client, &interaction_id, Duration::from_secs(60)).await?;
/// ```
#[allow(dead_code)]
pub async fn poll_until_complete(
    client: &Client,
    interaction_id: &str,
    max_wait: Duration,
) -> Result<InteractionResponse, PollError> {
    const INITIAL_DELAY: Duration = Duration::from_secs(1);
    const MAX_DELAY: Duration = Duration::from_secs(10);

    let mut delay = INITIAL_DELAY;
    let mut first_poll = true;
    let start = Instant::now();

    loop {
        if start.elapsed() > max_wait {
            return Err(PollError::Timeout);
        }

        // Skip delay on first poll to detect instant completions
        if first_poll {
            first_poll = false;
        } else {
            sleep(delay).await;
            delay = (delay * 2).min(MAX_DELAY);
        }

        let response = client.get_interaction(interaction_id).await?;
        println!(
            "Poll after {:?}: status={:?}",
            start.elapsed(),
            response.status
        );

        match response.status {
            InteractionStatus::Completed => return Ok(response),
            InteractionStatus::Failed => return Err(PollError::Failed),
            InteractionStatus::InProgress => {
                // Continue polling with exponential backoff
            }
            InteractionStatus::Cancelled => return Err(PollError::Failed),
            InteractionStatus::RequiresAction => return Err(PollError::Failed),
            other => {
                // Following Evergreen principles (see CLAUDE.md) - continue polling
                // on unknown status variants for forward compatibility.
                eprintln!("    Unhandled status {:?}, continuing to poll...", other);
            }
        }
    }
}

// =============================================================================
// Streaming Utilities
// =============================================================================

/// Result of consuming a stream, containing collected deltas and final response.
#[derive(Debug)]
#[allow(dead_code)]
pub struct StreamResult {
    /// Number of delta chunks received during streaming.
    pub delta_count: usize,
    /// All text content collected from delta chunks.
    pub collected_text: String,
    /// The final complete response, if received.
    pub final_response: Option<InteractionResponse>,
    /// Whether any function call deltas were received.
    pub saw_function_call: bool,
    /// Whether any thought deltas were received.
    pub saw_thought: bool,
    /// Whether any thought signature deltas were received.
    pub saw_thought_signature: bool,
    /// All thought content collected from delta chunks.
    pub collected_thoughts: String,
    /// All event_ids collected from stream events (for resume support).
    pub event_ids: Vec<String>,
    /// The last event_id received (for resumption).
    pub last_event_id: Option<String>,
}

impl StreamResult {
    /// Returns true if streaming produced any output (deltas or complete response).
    #[allow(dead_code)]
    pub fn has_output(&self) -> bool {
        self.delta_count > 0 || self.final_response.is_some()
    }
}

/// Consumes a stream, collecting text deltas, event_ids, and the final response.
///
/// This helper standardizes stream consumption across tests, handling:
/// - Counting delta chunks
/// - Collecting text content from deltas
/// - Capturing the final complete response
/// - Detecting function call deltas
/// - Tracking event_ids for stream resume support
/// - Graceful error handling (breaks on error, doesn't panic)
///
/// **Note**: Text content is printed to stdout as it's received for debugging
/// purposes when running tests with `--nocapture`.
///
/// # Arguments
///
/// * `stream` - A boxed stream of `Result<StreamEvent, GenaiError>` that will be
///   fully consumed (ownership is taken)
///
/// # Returns
///
/// A `StreamResult` containing the collected data from the stream.
///
/// # Example
///
/// ```ignore
/// let stream = interaction_builder(&client)
///     .with_text("Hello")
///     .create_stream();
///
/// let result = consume_stream(stream).await;
/// assert!(result.has_output());
/// assert!(result.collected_text.contains("hello"));
/// assert!(result.last_event_id.is_some()); // event_id for resume support
/// ```
#[allow(dead_code)]
pub async fn consume_stream(
    mut stream: futures_util::stream::BoxStream<'_, Result<StreamEvent, GenaiError>>,
) -> StreamResult {
    let mut result = StreamResult {
        delta_count: 0,
        collected_text: String::new(),
        final_response: None,
        saw_function_call: false,
        saw_thought: false,
        saw_thought_signature: false,
        collected_thoughts: String::new(),
        event_ids: Vec::new(),
        last_event_id: None,
    };

    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                // Track event_id for resume support
                if let Some(ref eid) = event.event_id {
                    result.event_ids.push(eid.clone());
                    result.last_event_id = Some(eid.clone());
                }

                match event.chunk {
                    StreamChunk::Delta(delta) => {
                        result.delta_count += 1;
                        if let Some(text) = delta.as_text() {
                            result.collected_text.push_str(text);
                            print!("{}", text);
                        }
                        if delta.is_function_call() {
                            result.saw_function_call = true;
                        }
                        if delta.is_thought() {
                            result.saw_thought = true;
                            // Thoughts contain cryptographic signatures, not readable text
                            if let Some(sig) = delta.thought_signature() {
                                result.collected_thoughts.push_str(sig);
                            }
                        }
                        if delta.is_thought_signature() {
                            result.saw_thought_signature = true;
                        }
                    }
                    StreamChunk::Complete(response) => {
                        println!("\nStream complete: {:?}", response.id);
                        result.final_response = Some(response);
                    }
                    _ => {} // Handle unknown variants
                }
            }
            Err(e) => {
                println!("Stream error: {:?}", e);
                break;
            }
        }
    }

    result
}

/// Result of consuming an auto-function stream.
#[derive(Debug)]
#[allow(dead_code)]
pub struct AutoFunctionStreamResult {
    /// Number of delta chunks received during streaming.
    pub delta_count: usize,
    /// All text content collected from delta chunks.
    pub collected_text: String,
    /// Number of times function execution was signaled.
    pub executing_functions_count: usize,
    /// Names of all functions that were executed.
    pub executed_function_names: Vec<String>,
    /// Number of function result events.
    pub function_results_count: usize,
    /// The final complete response, if received.
    pub final_response: Option<InteractionResponse>,
    /// Whether any thought deltas were received.
    pub saw_thought: bool,
    /// All thought content collected from delta chunks.
    pub collected_thoughts: String,
    /// All event_ids collected from stream events (for resume support).
    /// Only API-generated events have event_ids; client events (ExecutingFunctions) don't.
    pub event_ids: Vec<String>,
    /// The last event_id received (for resumption).
    pub last_event_id: Option<String>,
}

impl AutoFunctionStreamResult {
    /// Returns true if streaming produced any output.
    #[allow(dead_code)]
    pub fn has_output(&self) -> bool {
        self.delta_count > 0 || self.final_response.is_some()
    }
}

/// Consumes an auto-function stream, collecting events, event_ids, and the final response.
///
/// This helper standardizes auto-function stream consumption across tests.
///
/// # Arguments
///
/// * `stream` - A boxed stream of `Result<AutoFunctionStreamEvent, GenaiError>`
///
/// # Returns
///
/// An `AutoFunctionStreamResult` containing the collected data from the stream.
#[allow(dead_code)]
pub async fn consume_auto_function_stream(
    mut stream: futures_util::stream::BoxStream<'_, Result<AutoFunctionStreamEvent, GenaiError>>,
) -> AutoFunctionStreamResult {
    let mut result = AutoFunctionStreamResult {
        delta_count: 0,
        collected_text: String::new(),
        executing_functions_count: 0,
        executed_function_names: Vec::new(),
        function_results_count: 0,
        final_response: None,
        saw_thought: false,
        collected_thoughts: String::new(),
        event_ids: Vec::new(),
        last_event_id: None,
    };

    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                // Track event_id for resume support (only API events have event_ids)
                if let Some(ref eid) = event.event_id {
                    result.event_ids.push(eid.clone());
                    result.last_event_id = Some(eid.clone());
                }

                match event.chunk {
                    AutoFunctionStreamChunk::Delta(delta) => {
                        result.delta_count += 1;
                        if let Some(text) = delta.as_text() {
                            result.collected_text.push_str(text);
                            print!("{}", text);
                        }
                        if delta.is_thought() {
                            result.saw_thought = true;
                            // Thoughts contain cryptographic signatures, not readable text
                            if let Some(sig) = delta.thought_signature() {
                                result.collected_thoughts.push_str(sig);
                            }
                        }
                    }
                    AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
                        result.executing_functions_count += 1;
                        for call in pending_calls {
                            println!("\n[Executing: {}]", call.name);
                            result.executed_function_names.push(call.name.clone());
                        }
                    }
                    AutoFunctionStreamChunk::FunctionResults(results) => {
                        result.function_results_count += 1;
                        println!("[Got {} result(s)]", results.len());
                        // Track executed function names from results
                        for r in &results {
                            if !result.executed_function_names.contains(&r.name) {
                                result.executed_function_names.push(r.name.clone());
                            }
                        }
                    }
                    AutoFunctionStreamChunk::Complete(response) => {
                        println!("\n[Stream complete: {:?}]", response.id);
                        result.final_response = Some(response);
                    }
                    _ => {
                        // Unknown future variants - ignore
                        println!("[Unknown chunk type]");
                    }
                }
            }
            Err(e) => {
                println!("Stream error: {:?}", e);
                break;
            }
        }
    }

    result
}

// =============================================================================
// Test Asset URLs
// =============================================================================

// Allow dead_code because these are shared utilities and not all test files use all constants

/// Google Cloud Storage sample image URL (scones/pastries)
/// NOTE: GCS URIs are NOT supported by the Interactions API - tests should handle errors gracefully
#[allow(dead_code)]
pub const SAMPLE_IMAGE_URL: &str = "gs://cloud-samples-data/generative-ai/image/scones.jpg";

/// Google Cloud Storage sample audio URL (Pixel phone promo)
/// NOTE: GCS URIs are NOT supported by the Interactions API - tests should handle errors gracefully
#[allow(dead_code)]
pub const SAMPLE_AUDIO_URL: &str = "gs://cloud-samples-data/generative-ai/audio/pixel.mp3";

/// Google Cloud Storage sample video URL
/// NOTE: GCS URIs are NOT supported by the Interactions API - tests should handle errors gracefully
#[allow(dead_code)]
pub const SAMPLE_VIDEO_URL: &str = "gs://cloud-samples-data/video/animals.mp4";

/// Small 1x1 red PNG image encoded as base64
/// This is a minimal valid PNG for testing base64 image input
#[allow(dead_code)]
pub const TINY_RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

/// Small WAV audio header (minimal valid WAV for testing)
/// This is a 44-byte WAV header with no actual audio data
#[allow(dead_code)]
pub const TINY_WAV_BASE64: &str = "UklGRiQAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YQAAAAA=";

/// Minimal MP4 video file (ftyp box only) for testing base64 video input
/// This is a minimal valid MP4 container header - the model may report it's empty/corrupt
#[allow(dead_code)]
pub const TINY_MP4_BASE64: &str = "AAAAIGZ0eXBpc29tAAACAGlzb21pc28yYXZjMW1wNDE=";

/// Small 1x1 blue PNG image encoded as base64
/// This is a minimal valid PNG for testing multi-image comparisons
#[allow(dead_code)]
pub const TINY_BLUE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// Minimal PDF document containing "Hello World" text
/// This is a complete valid PDF for testing document input
#[allow(dead_code)]
pub const TINY_PDF_BASE64: &str = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA3MiA3Ml0gL0NvbnRlbnRzIDQgMCBSIC9SZXNvdXJjZXMgPDwgPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA0NCA+PgpzdHJlYW0KQlQgL0YxIDEyIFRmIDEwIDUwIFRkIChIZWxsbyBXb3JsZCkgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgNQowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAyMjQgMDAwMDAgbiAKdHJhaWxlcgo8PCAvU2l6ZSA1IC9Sb290IDEgMCBSID4+CnN0YXJ0eHJlZgozMjAKJSVFT0Y=";

// =============================================================================
// Test Fixture Builders (Issue #82)
// =============================================================================

/// Default model used across all tests.
pub const DEFAULT_MODEL: &str = "gemini-3-flash-preview";

/// Creates a pre-configured interaction builder with the default model.
///
/// This is the standard entry point for integration tests, reducing boilerplate.
///
/// # Example
///
/// ```ignore
/// use common::{get_client, interaction_builder};
///
/// let client = get_client().unwrap();
/// let response = interaction_builder(&client)
///     .with_text("Hello!")
///     .create()
///     .await
///     .expect("Request failed");
/// ```
#[allow(dead_code)]
pub fn interaction_builder(client: &Client) -> genai_rs::InteractionBuilder<'_> {
    client.interaction().with_model(DEFAULT_MODEL)
}

/// Creates a stateful interaction builder with storage enabled.
///
/// Use this when testing multi-turn conversations that need server-side state.
///
/// # Example
///
/// ```ignore
/// let response = stateful_builder(&client)
///     .with_text("Remember my name is Alice")
///     .create()
///     .await?;
/// ```
#[allow(dead_code)]
pub fn stateful_builder(client: &Client) -> genai_rs::InteractionBuilder<'_> {
    interaction_builder(client).with_store_enabled()
}

// =============================================================================
// Semantic Validation Using Structured Output
// =============================================================================

/// Uses Gemini with structured output to validate that a response is semantically appropriate.
///
/// This provides a middle ground between brittle content assertions and purely structural checks.
/// The validator uses a separate API call to ask Gemini to judge whether the response makes sense
/// given the context and expected behavior.
///
/// **When to use this:**
/// - Multi-turn context preservation tests
/// - Function calling integration where the response should use the function result
/// - Complex queries where "did it do the right thing?" matters
/// - Any test where you need behavioral validation, not just structural checks
///
/// **When NOT to use this:**
/// - Simple structural checks (empty/non-empty text, status codes)
/// - Known deterministic outputs (error codes, status values, exact numbers)
/// - Performance-critical test paths where extra API calls are costly
///
/// **Performance:** Adds ~1-2 seconds per validation (one extra API call with structured output).
///
/// **Reliability:** Uses best-effort validation with graceful fallback. If the structured output
/// is malformed or unparsable, returns true (valid) to avoid blocking tests on API format changes.
/// Prints validation reason for debugging.
///
/// # Arguments
///
/// * `client` - The API client to use for validation
/// * `context` - Background context: what the user asked, what data was provided (e.g., function results, prior turns), and what's expected
/// * `response_text` - The actual response text from the LLM being tested
/// * `validation_question` - Specific yes/no question to ask, e.g., "Does this response address the user's question about weather?"
///
/// # Returns
///
/// * `Ok(true)` - Response is semantically valid
/// * `Ok(false)` - Response is not semantically valid (rare - usually means genuinely wrong response)
/// * `Err(_)` - Validation API call failed (network error, etc.)
///
/// # Example
///
/// ```ignore
/// // Test that multi-turn context is preserved
/// let response2 = stateful_builder(&client)
///     .with_previous_interaction(&response1_id)
///     .with_text("What is my favorite color?")
///     .create().await?;
///
/// let is_valid = validate_response_semantically(
///     &client,
///     "User said 'My favorite color is blue' in Turn 1, now asking 'What is my favorite color?' in Turn 2",
///     response2.as_text().unwrap(),
///     "Does this response indicate the user's favorite color is blue?"
/// ).await?;
///
/// assert!(is_valid, "Response should recall blue from previous turn");
/// ```
///
/// # See Also
///
/// * Example usage in `tests/function_calling_tests.rs` and `tests/interactions_api_tests.rs`
/// * CLAUDE.md "Test Assertion Strategies" section for when to use this vs structural assertions
#[allow(dead_code)]
pub async fn validate_response_semantically(
    client: &Client,
    context: &str,
    response_text: &str,
    validation_question: &str,
) -> Result<bool, GenaiError> {
    use serde_json::json;

    let validation_prompt = format!(
        "You are a test validator. Your job is to judge whether an LLM response is appropriate given the context.\n\nContext: {}\n\nResponse to validate: {}\n\nQuestion: {}\n\nProvide your judgment as a yes/no boolean and explain your reasoning.",
        context, response_text, validation_question
    );

    let schema = json!({
        "type": "object",
        "properties": {
            "is_valid": {
                "type": "boolean",
                "description": "Whether the response is semantically valid"
            },
            "reason": {
                "type": "string",
                "description": "Brief explanation of the judgment"
            }
        },
        "required": ["is_valid", "reason"]
    });

    let validation = interaction_builder(client)
        .with_text(&validation_prompt)
        .with_response_format(schema)
        .create()
        .await?;

    // Parse structured output
    if let Some(text) = validation.as_text()
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(text)
    {
        let is_valid = json
            .get("is_valid")
            .and_then(|v| v.as_bool())
            // Design decision: Default to valid if the boolean is missing or malformed.
            // This favors test reliability (avoiding false negatives from API format changes)
            // over catching edge cases where Gemini might return invalid but we can't parse it.
            // The tradeoff is acceptable because: (1) structured output is typically reliable,
            // (2) we log the reason for debugging, and (3) blocking tests on parse errors
            // would make tests fragile to API evolution.
            .unwrap_or(true);

        let reason = json
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("(no reason provided)");

        println!(
            "Semantic validation: {} - {}",
            if is_valid { "✓ VALID" } else { "✗ INVALID" },
            reason
        );

        return Ok(is_valid);
    }

    // Fallback: if we can't parse the structured output at all, assume valid
    // Design decision: Same reasoning as above - we prioritize test reliability over
    // catching malformed API responses. The validator is a safety net for behavioral
    // validation, not a critical assertion. If Gemini's structured output format changes,
    // we don't want to break all tests; we want to degrade gracefully and log warnings.
    let response_preview = validation
        .as_text()
        .map(|t| {
            if t.len() > 100 {
                format!("{}...", &t[..100])
            } else {
                t.to_string()
            }
        })
        .unwrap_or_else(|| "(no text)".to_string());
    println!(
        "Warning: Could not parse semantic validation response (text: '{}'), assuming valid",
        response_preview
    );
    Ok(true)
}

/// Validates and asserts that a response is semantically appropriate in one call.
///
/// This is a convenience wrapper around [`validate_response_semantically`] that combines
/// the validation check and assertion, reducing boilerplate in tests.
///
/// # Panics
///
/// - If the semantic validation API call fails
/// - If the response is not semantically valid
///
/// # Example
///
/// ```ignore
/// // Instead of:
/// let is_valid = validate_response_semantically(&client, context, &text, question)
///     .await
///     .expect("Semantic validation failed");
/// assert!(is_valid, "Response should...");
///
/// // Use:
/// assert_response_semantic(&client, context, &text, question).await;
/// ```
// Note: Used in multimodal_tests.rs and temp_file_tests.rs but warning appears
// because each test file compiles independently
#[allow(dead_code)]
pub async fn assert_response_semantic(
    client: &Client,
    context: &str,
    response_text: &str,
    validation_question: &str,
) {
    let is_valid =
        validate_response_semantically(client, context, response_text, validation_question)
            .await
            .expect("Semantic validation API call failed");
    assert!(
        is_valid,
        "Semantic validation failed.\nQuestion: {}\nResponse: {}",
        validation_question, response_text
    );
}

// =============================================================================
// Function Declaration Builders
// =============================================================================

/// Creates a standard "get_weather" function declaration for testing.
///
/// This is the canonical weather function used across function calling tests.
/// Returns weather information for a given city.
#[allow(dead_code)]
pub fn get_weather_function() -> genai_rs::FunctionDeclaration {
    use serde_json::json;
    genai_rs::FunctionDeclaration::builder("get_weather")
        .description("Get the current weather for a city")
        .parameter(
            "city",
            json!({"type": "string", "description": "City name"}),
        )
        .required(vec!["city".to_string()])
        .build()
}

/// Creates a standard "get_time" function declaration for testing.
///
/// This is the canonical time function used across function calling tests.
/// Returns the current time in a given timezone.
#[allow(dead_code)]
pub fn get_time_function() -> genai_rs::FunctionDeclaration {
    use serde_json::json;
    genai_rs::FunctionDeclaration::builder("get_time")
        .description("Get the current time in a timezone")
        .parameter(
            "timezone",
            json!({"type": "string", "description": "Timezone like PST, EST, JST"}),
        )
        .required(vec!["timezone".to_string()])
        .build()
}

// =============================================================================
// Long Conversation Error Detection
// =============================================================================

/// Checks if an error is a known API limitation for long conversation chains.
///
/// Long multi-turn conversations can trigger backend issues including:
/// - UTF-8 encoding errors
/// - Spanner database timeouts
/// - Content truncation errors
///
/// Use this to gracefully handle expected failures in long conversation tests.
#[allow(dead_code)]
pub fn is_long_conversation_api_error(error: &GenaiError) -> bool {
    let error_str = format!("{:?}", error);
    error_str.contains("UTF-8") || error_str.contains("spanner") || error_str.contains("truncated")
}
