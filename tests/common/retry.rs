//! Retrying the transport errors and model-side flakes a live test can hit.

use genai_rs::{AutoFunctionResult, GenaiError, InteractionResponse, InteractionStatus};
use std::any::Any;
use std::future::Future;
use std::time::Duration;
use tokio::time::sleep;

// =============================================================================
// Retry Utilities for Transient API Errors
// =============================================================================

/// Maximum number of retries for transient API errors.
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Cumulative sleep budget for [`retry_on_transient`], across all attempts
/// (plain backoff and honored `Retry-After` alike). Once the next delay
/// would exceed it, the real error surfaces instead of sleeping toward a
/// harness timeout.
pub const MAX_RETRY_SLEEP: Duration = Duration::from_secs(15);

/// Known model-side flakes worth retrying: Spanner UTF-8 errors (#60), and
/// 400s that pass on re-run: "invalid json syntax" from structured output,
/// the bare "there was a problem processing your request" burst, and any 400
/// that tells the caller to retry, which is how the API used to report
/// `malformed_tool_call`. It now reports that on a `200` instead, as a
/// failed interaction; [`is_transient_failure`] matches that form.
///
/// `too_many_tool_calls` is never retried, whatever its message says: it is
/// the url_context loop under a response schema, a known server bug, and a
/// live test does not retry a known flake inside itself (D-052). Its
/// message asked to be retried on 2026-11-01 and did not on 2026-11-07, so
/// the match is on the code the crate puts at the front of the message.
/// Transport transience (network, timeouts, 408, 429, most 5xx) is
/// [`GenaiError::is_retryable`]'s job.
pub fn is_transient_error(err: &GenaiError) -> bool {
    transient_flake(err).is_some()
}

/// The model-side flake [`is_transient_error`] matches, by the code its
/// retry marker carries (see [`transient_retry_marker`]).
fn transient_flake(err: &GenaiError) -> Option<&'static str> {
    match err {
        GenaiError::Api {
            status_code,
            message,
            ..
        } => {
            let lower = message.to_lowercase();
            // A tool-call loop is a known server bug, not a transient one;
            // see the doc comment.
            if lower.contains("too_many_tool_calls") {
                return None;
            }
            // Spanner UTF-8 errors are transient backend issues
            // See: https://github.com/evansenter/genai-rs/issues/60
            if lower.contains("spanner") && lower.contains("utf-8") {
                return Some("spanner_utf8");
            }
            if *status_code != 400 {
                return None;
            }
            // Model occasionally generates invalid JSON — transient API-side issue
            if lower.contains("invalid json syntax") {
                Some("invalid_json")
            // Server-side processing burst: generic apology with no
            // pointer at the request. Message-pinned so a genuine
            // fixture rejection (which names its problem) still fails
            // loud on the first attempt.
            } else if lower.contains("there was a problem processing your request") {
                Some("processing_error")
            // The model produced a broken tool call, and the API said
            // so in a 400: "...Please retry the request." It now says it
            // in a failed interaction instead (`is_transient_failure`).
            // (A loop says it too, and is turned away above.)
            } else if lower.contains("please retry the request") {
                Some("please_retry")
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The word that starts the line each retry below prints, which the
/// flakiness report counts per test (`count_retried_flakes.sh`; D-052).
pub const TRANSIENT_RETRY_MARKER: &str = "TRANSIENT_RETRIED";

/// The line [`retry_on_transient`] prints before each retry, alone on its
/// line: `TRANSIENT_RETRIED <test> <code>`. The test is the running thread's
/// name, which libtest sets to the test's path in its binary (a tokio
/// test's body runs on that thread), or `-` off a test thread. The code is
/// the model-side flake's name (`malformed_tool_call`, or what
/// [`is_transient_error`] matched: `invalid_json`, `processing_error`,
/// `please_retry`, `spanner_utf8`), or the transport failure's: the HTTP
/// status, `timeout`, `network` or `stream_interrupted`. The report counts
/// a run with a model-side retry toward the test's chronic-flake share, and
/// lists transport retries without a target.
pub fn transient_retry_marker(code: &str) -> String {
    let thread = std::thread::current();
    let test = thread
        .name()
        .filter(|name| *name != "main" && !name.starts_with("tokio-runtime"))
        .unwrap_or("-");
    format!("{TRANSIENT_RETRY_MARKER} {test} {code}")
}

/// The code a retried error's marker carries (see
/// [`transient_retry_marker`]).
fn retry_code(err: &GenaiError) -> String {
    if let Some(flake) = transient_flake(err) {
        return flake.to_string();
    }
    match err {
        GenaiError::Api { status_code, .. } => status_code.to_string(),
        GenaiError::Timeout(_) => "timeout".to_string(),
        GenaiError::Http(_) => "network".to_string(),
        GenaiError::StreamInterrupted { .. } => "stream_interrupted".to_string(),
        _ => "other".to_string(),
    }
}

/// Whether an interaction that came back still failed transiently:
/// `status: failed` with a `malformed_tool_call` error, which asks to be
/// retried. That is how the API reports a broken tool call now, in a `200`
/// rather than the `400` [`is_transient_error`] matches (#545; the gap
/// tracker's Tools section has the record), so it is retried as the `400`
/// was. A failure that also names `too_many_tool_calls` is not: that is the
/// url_context loop, a known server bug (D-052).
pub fn is_transient_failure(response: &InteractionResponse) -> bool {
    if response.status != InteractionStatus::Failed {
        return false;
    }
    let errors = response.errors.as_deref().unwrap_or_default();
    let names = |needle: &str| {
        errors.iter().any(|e| {
            [e.code.as_deref(), e.message.as_deref()]
                .into_iter()
                .flatten()
                .any(|text| text.to_lowercase().contains(needle))
        })
    };
    errors
        .iter()
        .any(|e| e.code.as_deref() == Some("malformed_tool_call"))
        && !names("too_many_tool_calls")
}

/// The interaction a successful operation returned, when it returned one:
/// an [`InteractionResponse`], or the final response of an
/// [`AutoFunctionResult`].
fn returned_interaction(result: &dyn Any) -> Option<&InteractionResponse> {
    result.downcast_ref::<InteractionResponse>().or_else(|| {
        result
            .downcast_ref::<AutoFunctionResult>()
            .map(|auto| &auto.response)
    })
}

/// Retries on [`is_transient_error`] or [`GenaiError::is_retryable`] with
/// exponential backoff, honoring `Retry-After`, capped at [`MAX_RETRY_SLEEP`]
/// of total sleep. An operation that returns an interaction (directly, or
/// as an auto-function loop's final response) that failed transiently
/// ([`is_transient_failure`]) is retried the same way, and its last attempt
/// is returned as it came, for the test's own status check to report.
/// Anything else, such as a 400 that names its problem, returns on the
/// first attempt.
pub async fn retry_on_transient<F, Fut, T>(max_retries: u32, operation: F) -> Result<T, GenaiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, GenaiError>>,
    T: 'static,
{
    let mut last_error = None;
    let mut total_slept = Duration::ZERO;

    for attempt in 0..=max_retries {
        match operation().await {
            Ok(result) => {
                let failed = returned_interaction(&result).filter(|r| is_transient_failure(r));
                let delay = Duration::from_secs(1 << attempt);
                let Some(failed) = failed else {
                    return Ok(result);
                };
                if attempt == max_retries || total_slept + delay > MAX_RETRY_SLEEP {
                    return Ok(result);
                }
                total_slept += delay;
                println!("{}", transient_retry_marker("malformed_tool_call"));
                println!(
                    "Transient failed interaction on attempt {} of {}, retrying in {:?}: {:?}",
                    attempt + 1,
                    max_retries + 1,
                    delay,
                    failed.errors
                );
                sleep(delay).await;
            }
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
                println!("{}", transient_retry_marker(&retry_code(&err)));
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
/// <!-- ignore: rustdoc does not doctest a test crate's modules, and the call needs a live client; the live suites compile it -->
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
