//! Common test utilities shared across all integration test files.
//!
//! Usage in test files:
//! ```ignore
//! mod common;
//! use common::*;
//! ```
//!
//! Items carry `#[allow(dead_code)]` because each test file compiles this
//! module separately and no single file uses all of it.

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

/// Cumulative sleep budget for [`retry_on_transient`], across all attempts
/// (plain backoff and honored `Retry-After` alike). Once the next delay
/// would exceed it, the real error surfaces instead of sleeping toward a
/// harness timeout.
#[allow(dead_code)]
pub const MAX_RETRY_SLEEP: Duration = Duration::from_secs(15);

/// Known model-side flakes worth retrying: Spanner UTF-8 errors (#60), and
/// two 400s that pass on re-run ("invalid json syntax" from structured output,
/// and the bare "there was a problem processing your request" burst).
/// Transport transience (network, timeouts, 429, 5xx) is
/// [`GenaiError::is_retryable`]'s job.
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
                // Server-side processing burst: generic apology with no
                // pointer at the request. Message-pinned so a genuine
                // fixture rejection (which names its problem) still fails
                // loud on the first attempt.
                || (*status_code == 400
                    && lower.contains("there was a problem processing your request"))
        }
        _ => false,
    }
}

/// Retries on [`is_transient_error`] or [`GenaiError::is_retryable`] with
/// exponential backoff, honoring `Retry-After`, capped at [`MAX_RETRY_SLEEP`]
/// of total sleep. Anything else, such as a 400 that names its problem,
/// returns on the first attempt.
#[allow(dead_code)]
pub async fn retry_on_transient<F, Fut, T>(max_retries: u32, operation: F) -> Result<T, GenaiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, GenaiError>>,
{
    let mut last_error = None;
    let mut total_slept = Duration::ZERO;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err)
                if (is_transient_error(&err) || err.is_retryable()) && attempt < max_retries =>
            {
                // Retrying before a server-sent Retry-After just earns another
                // 429; past the sleep cap, surface the real error rather than
                // an opaque harness timeout.
                let backoff = Duration::from_secs(1 << attempt);
                let delay = match err.retry_after() {
                    Some(ra) => ra.max(backoff),
                    None => backoff,
                };
                if total_slept + delay > MAX_RETRY_SLEEP {
                    return Err(err);
                }
                total_slept += delay;
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

/// `retry_on_transient` with the per-attempt clones written for you.
///
/// ```ignore
/// let response = retry_request!([client, prev_id] => {
///     stateful_builder(&client)
///         .with_previous_interaction(&prev_id)
///         .create()
///         .await
/// })
/// .expect("Request failed");
/// ```
///
/// Every non-`Copy` variable the body captures must be listed in the brackets.
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

/// Creates a client from `GEMINI_API_KEY`, or `None` when it is unset or blank
/// (the same trimmed check the CI guard applies). A key that is set but fails
/// to build a client panics rather than reading as "no key".
#[allow(dead_code)]
pub fn get_client() -> Option<Client> {
    let api_key = env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())?;
    Some(
        Client::builder(api_key)
            .build()
            .expect("GEMINI_API_KEY is set but the client failed to build"),
    )
}

/// A client that also keeps the raw JSON of the last successful response, for
/// fields the API returns but `InteractionResponse` does not model.
#[allow(dead_code)]
pub fn get_inspecting_client() -> Option<(Client, std::sync::Arc<LastResponseBody>)> {
    let api_key = env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())?;
    let body = std::sync::Arc::new(LastResponseBody::default());
    let client = Client::builder(api_key)
        .add_wire_inspector(body.clone())
        .build()
        .expect("GEMINI_API_KEY is set but the client failed to build");
    Some((client, body))
}

/// Wire inspector holding the most recent successful response body.
#[derive(Default)]
#[allow(dead_code)]
pub struct LastResponseBody(std::sync::Mutex<Option<serde_json::Value>>);

impl LastResponseBody {
    /// Takes the captured body, panicking if none arrived since the last take.
    #[allow(dead_code)]
    pub fn take(&self) -> serde_json::Value {
        self.0
            .lock()
            .unwrap()
            .take()
            .expect("no response body was captured")
    }
}

impl genai_rs::wire::WireInspector for LastResponseBody {
    fn on_event(&self, event: &genai_rs::wire::WireEvent) {
        if let genai_rs::wire::WireEvent::ResponseBody { body, .. } = event {
            *self.0.lock().unwrap() = Some(body.clone());
        }
    }
}

// =============================================================================
// Timeout Utilities
// =============================================================================

/// Per-test budget: 60s, or `TEST_TIMEOUT_SECS`.
#[allow(dead_code)]
pub fn test_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("TEST_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60),
    )
}

/// Budget for tests making many sequential calls: 120s, or
/// `EXTENDED_TEST_TIMEOUT_SECS`.
#[allow(dead_code)]
pub fn extended_test_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("EXTENDED_TEST_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120),
    )
}

/// Runs `future`, panicking if it outlives `duration`.
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

/// Polls a stored interaction until it leaves `InProgress`, returning it in
/// whatever terminal state it reached. Unknown statuses keep polling
/// (Evergreen). Panics on an API error or when `max_wait` elapses.
#[allow(dead_code)]
pub async fn poll_until_done(
    client: &Client,
    interaction_id: &str,
    max_wait: Duration,
) -> InteractionResponse {
    const MAX_DELAY: Duration = Duration::from_secs(10);

    let start = Instant::now();
    let mut delay = Duration::from_secs(1);
    loop {
        let response = client
            .get_interaction(interaction_id)
            .await
            .unwrap_or_else(|e| panic!("polling {interaction_id} failed: {e:?}"));
        match response.status {
            InteractionStatus::InProgress => {}
            ref status if status.is_unknown() => {
                eprintln!("unknown status {status:?} for {interaction_id}; still polling");
            }
            _ => return response,
        }
        assert!(
            start.elapsed() < max_wait,
            "{interaction_id} still {:?} after {max_wait:?}",
            response.status
        );
        sleep(delay).await;
        delay = (delay * 2).min(MAX_DELAY);
    }
}

// =============================================================================
// Streaming Utilities
// =============================================================================

/// What a fully consumed stream produced.
#[derive(Debug)]
#[allow(dead_code)]
pub struct StreamResult {
    /// Number of `StepDelta` chunks received.
    pub delta_count: usize,
    /// Text concatenated from text deltas.
    pub collected_text: String,
    /// The `Completed` response, if the stream sent one.
    pub final_response: Option<InteractionResponse>,
    /// A function-call step started or an arguments delta arrived.
    pub saw_function_call: bool,
    /// A thought step started, or a thought summary/signature delta arrived.
    pub saw_thought: bool,
    /// Every `event_id` seen, in order.
    pub event_ids: Vec<String>,
}

impl StreamResult {
    /// True if the stream produced deltas or a final response.
    #[allow(dead_code)]
    pub fn has_output(&self) -> bool {
        self.delta_count > 0 || self.final_response.is_some()
    }
}

/// Drains a stream, panicking on the first error.
///
/// A stream that errors midway has usually already produced a delta, so a
/// helper that stopped quietly would let `has_output()` pass on a broken
/// stream. Tests that expect an error should iterate the stream themselves.
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
        event_ids: Vec::new(),
    };

    while let Some(item) = stream.next().await {
        let event = item.unwrap_or_else(|e| {
            panic!("stream failed after {} delta(s): {e:?}", result.delta_count)
        });
        if let Some(eid) = event.event_id {
            result.event_ids.push(eid);
        }

        match event.chunk {
            StreamChunk::StepStart { step, .. } => match step {
                genai_rs::Step::FunctionCall { .. } => result.saw_function_call = true,
                genai_rs::Step::Thought { .. } => result.saw_thought = true,
                _ => {}
            },
            StreamChunk::StepDelta { delta, .. } => {
                result.delta_count += 1;
                if let Some(text) = delta.as_text() {
                    result.collected_text.push_str(text);
                }
                if delta.as_arguments_delta().is_some() {
                    result.saw_function_call = true;
                }
                if matches!(
                    delta,
                    genai_rs::StepDelta::ThoughtSignature { .. }
                        | genai_rs::StepDelta::ThoughtSummary { .. }
                ) {
                    result.saw_thought = true;
                }
            }
            StreamChunk::Completed(response) => result.final_response = Some(response),
            _ => {}
        }
    }

    result
}

/// What a fully consumed auto-function stream produced.
#[derive(Debug)]
#[allow(dead_code)]
pub struct AutoFunctionStreamResult {
    /// Number of `Delta` chunks received.
    pub delta_count: usize,
    /// Text concatenated from text deltas.
    pub collected_text: String,
    /// Number of `ExecutingFunctions` chunks.
    pub executing_functions_count: usize,
    /// Names of every function executed, in order, without duplicates.
    pub executed_function_names: Vec<String>,
    /// Number of `FunctionResults` chunks.
    pub function_results_count: usize,
    /// The response from `Complete` or `MaxLoopsReached`.
    pub final_response: Option<InteractionResponse>,
    /// The stream ended with `MaxLoopsReached`.
    pub reached_max_loops: bool,
}

impl AutoFunctionStreamResult {
    /// True if the stream produced deltas or a final response.
    #[allow(dead_code)]
    pub fn has_output(&self) -> bool {
        self.delta_count > 0 || self.final_response.is_some()
    }
}

/// Drains an auto-function stream, panicking on the first error (see
/// [`consume_stream`]).
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
        reached_max_loops: false,
    };

    fn record(names: &mut Vec<String>, name: &str) {
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }

    while let Some(item) = stream.next().await {
        let event = item.unwrap_or_else(|e| {
            panic!(
                "auto-function stream failed after {} delta(s): {e:?}",
                result.delta_count
            )
        });
        match event.chunk {
            AutoFunctionStreamChunk::Delta(delta) => {
                result.delta_count += 1;
                if let Some(text) = delta.as_text() {
                    result.collected_text.push_str(text);
                }
            }
            AutoFunctionStreamChunk::ExecutingFunctions { pending_calls, .. } => {
                result.executing_functions_count += 1;
                for call in &pending_calls {
                    record(&mut result.executed_function_names, &call.name);
                }
            }
            AutoFunctionStreamChunk::FunctionResults(results) => {
                result.function_results_count += 1;
                for r in &results {
                    record(&mut result.executed_function_names, &r.name);
                }
            }
            AutoFunctionStreamChunk::Complete(response) => result.final_response = Some(response),
            AutoFunctionStreamChunk::MaxLoopsReached(response) => {
                result.reached_max_loops = true;
                result.final_response = Some(response);
            }
            other => panic!("unexpected auto-function chunk: {other:?}"),
        }
    }

    result
}

// =============================================================================
// Test Asset URLs
// =============================================================================

/// A `gs://` URI. The Interactions API rejects these with 400; one test pins that.
#[allow(dead_code)]
pub const SAMPLE_IMAGE_URL: &str = "gs://cloud-samples-data/generative-ai/image/scones.jpg";

/// Public YouTube video URI the Interactions API accepts directly (verified live
/// 2026-08-16), for tests that need the model to actually ingest video.
#[allow(dead_code)]
pub const SAMPLE_YOUTUBE_VIDEO_URL: &str = "https://www.youtube.com/watch?v=aqz-KE-bpKQ";

/// Small 1x1 red PNG image encoded as base64
/// This is a minimal valid PNG for testing base64 image input
#[allow(dead_code)]
pub const TINY_RED_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

/// Small WAV audio clip: 16-bit mono 44.1kHz, 100 frames (~2ms) of silence.
/// The data chunk must be non-empty — as of 2026-08 the API rejects
/// zero-length audio streams with 400 invalid_request.
#[allow(dead_code)]
pub const TINY_WAV_BASE64: &str = "UklGRuwAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YcgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

/// A 1-second 64x64 red H.264 clip (~1.7KB). Shorter clips yield no sampled
/// frame at ~1 fps and are rejected with 400 invalid_request.
#[allow(dead_code)]
pub const TINY_MP4_BASE64: &str = "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";

/// Small 1x1 blue PNG image encoded as base64
/// This is a minimal valid PNG for testing multi-image comparisons
#[allow(dead_code)]
pub const TINY_BLUE_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/// Minimal PDF document containing "Hello World" text
/// This is a complete valid PDF for testing document input
#[allow(dead_code)]
pub const TINY_PDF_BASE64: &str = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA3MiA3Ml0gL0NvbnRlbnRzIDQgMCBSIC9SZXNvdXJjZXMgPDwgPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA0NCA+PgpzdHJlYW0KQlQgL0YxIDEyIFRmIDEwIDUwIFRkIChIZWxsbyBXb3JsZCkgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgNQowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAyMjQgMDAwMDAgbiAKdHJhaWxlcgo8PCAvU2l6ZSA1IC9Sb290IDEgMCBSID4+CnN0YXJ0eHJlZgozMjAKJSVFT0Y=";

// =============================================================================
// Test Fixture Builders
// =============================================================================

/// An interaction builder on `DEFAULT_MODEL`.
#[allow(dead_code)]
pub fn interaction_builder(client: &Client) -> genai_rs::InteractionBuilder<'_> {
    client.interaction().with_model(genai_rs::DEFAULT_MODEL)
}

/// An interaction builder on `DEFAULT_MODEL` with storage enabled.
#[allow(dead_code)]
pub fn stateful_builder(client: &Client) -> genai_rs::InteractionBuilder<'_> {
    interaction_builder(client).with_store_enabled()
}

// =============================================================================
// Semantic Validation Using Structured Output
// =============================================================================

/// Asks the model, via structured output, whether `response_text` answers
/// `validation_question` given `context`. Prefer [`assert_response_semantic`];
/// call this directly only when the verdict is needed as a `Result`.
///
/// An unparseable or missing verdict counts as valid but prints the
/// `SEMANTIC_VALIDATION_SKIPPED` marker, which CI counts, so a drifted
/// validator contract cannot go quietly green.
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

    let parsed = validation
        .as_text()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    let Some(json) = parsed else {
        let preview: String = validation
            .as_text()
            .unwrap_or("(no text)")
            .chars()
            .take(100)
            .collect();
        println!(
            "SEMANTIC_VALIDATION_SKIPPED (unparseable-verdict): could not parse validator response (text: '{preview}'), assuming valid"
        );
        return Ok(true);
    };

    let Some(is_valid) = json.get("is_valid").and_then(|v| v.as_bool()) else {
        println!(
            "SEMANTIC_VALIDATION_SKIPPED (missing-verdict): is_valid absent or non-boolean, assuming valid"
        );
        return Ok(true);
    };
    let reason = json
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("(no reason provided)");
    println!(
        "Semantic validation: {} - {reason}",
        if is_valid { "VALID" } else { "INVALID" }
    );
    Ok(is_valid)
}

/// Asserts a semantic verdict (see [`validate_response_semantically`]).
///
/// The validator call gets one transient retry. A transient failure that
/// survives it is tolerated with the `SEMANTIC_VALIDATION_SKIPPED` marker,
/// because the validator is a second round-trip that can fail independently
/// of the input under test; any other validator error panics.
#[allow(dead_code)]
pub async fn assert_response_semantic(
    client: &Client,
    context: &str,
    response_text: &str,
    validation_question: &str,
) {
    // One retry only: callers often nest this inside a `with_timeout` budget
    // that the primary call's retry chain has already drawn down.
    match retry_on_transient(1, || {
        validate_response_semantically(client, context, response_text, validation_question)
    })
    .await
    {
        Ok(is_valid) => assert!(
            is_valid,
            "Semantic validation failed.\nQuestion: {}\nResponse: {}",
            validation_question, response_text
        ),
        Err(e) if e.is_retryable() || is_transient_error(&e) => eprintln!(
            "SEMANTIC_VALIDATION_SKIPPED (transient validator error): {:?}",
            e
        ),
        Err(e) => panic!("Semantic validation call failed non-transiently: {:?}", e),
    }
}

// =============================================================================
// Function Declaration Builders
// =============================================================================

/// The canonical `get_weather(city)` declaration.
#[allow(dead_code)]
pub fn get_weather_function() -> genai_rs::FunctionDeclaration {
    use serde_json::json;
    genai_rs::FunctionDeclaration::builder("get_weather")
        .with_description("Get the current weather for a city")
        .add_parameter(
            "city",
            json!({"type": "string", "description": "City name"}),
        )
        .with_required(vec!["city".to_string()])
        .build()
}

/// The canonical `get_time(timezone)` declaration.
#[allow(dead_code)]
pub fn get_time_function() -> genai_rs::FunctionDeclaration {
    use serde_json::json;
    genai_rs::FunctionDeclaration::builder("get_time")
        .with_description("Get the current time in a timezone")
        .add_parameter(
            "timezone",
            json!({"type": "string", "description": "Timezone like PST, EST, JST"}),
        )
        .with_required(vec!["timezone".to_string()])
        .build()
}

// =============================================================================
// Error Predicates
// =============================================================================

/// The API's content safety block (400 "Request blocked due to safety
/// violations"). Built-in tools that fetch external content (URL context,
/// search) can trip it intermittently (observed live 2026-07).
#[allow(dead_code)]
pub fn is_safety_block_error(error: &GenaiError) -> bool {
    match error {
        GenaiError::Api {
            status_code,
            message,
            ..
        } => *status_code == 400 && message.to_lowercase().contains("safety violation"),
        _ => false,
    }
}
