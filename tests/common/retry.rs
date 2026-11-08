//! Retrying the transport errors and model-side flakes a live test can hit.

use genai_rs::GenaiError;
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
/// that tells the caller to retry (`malformed_tool_call`).
///
/// `too_many_tool_calls` is never retried, whatever its message says: it is
/// the url_context loop under a response schema, a known server bug, and a
/// live test does not retry a known flake inside itself (D-052). Its
/// message asked to be retried on 2026-11-01 and did not on 2026-11-07, so
/// the match is on the code the crate puts at the front of the message.
/// Transport transience (network, timeouts, 408, 429, most 5xx) is
/// [`GenaiError::is_retryable`]'s job.
pub fn is_transient_error(err: &GenaiError) -> bool {
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
                return false;
            }
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
                // The model produced a broken tool call, and the API says
                // so: "...Please retry the request." (A loop says it too,
                // and is turned away above.)
                || (*status_code == 400 && lower.contains("please retry the request"))
        }
        _ => false,
    }
}

/// Retries on [`is_transient_error`] or [`GenaiError::is_retryable`] with
/// exponential backoff, honoring `Retry-After`, capped at [`MAX_RETRY_SLEEP`]
/// of total sleep. Anything else, such as a 400 that names its problem,
/// returns on the first attempt.
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
