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

pub mod fake_gemini;
mod fixtures;
pub mod http_stub;
mod retry;
mod semantic;
mod streams;

// Re-exported so every helper stays at `common::<name>`; a file that uses
// none of a submodule's helpers would otherwise warn on its re-export.
#[allow(unused_imports)]
pub use fixtures::*;
#[allow(unused_imports)]
pub use retry::*;
#[allow(unused_imports)]
pub use semantic::*;
#[allow(unused_imports)]
pub use streams::*;

use genai_rs::{Client, GenaiError, InteractionResponse, InteractionStatus};
use std::env;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::time::sleep;

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

// =============================================================================
// Names for the resources live tests create
// =============================================================================

/// The marker on every resource a live test creates: an id, display name or
/// fixture content starting with it. The daily janitor
/// (`.github/scripts/cleanup_test_resources.py`) deletes only resources
/// that carry it, so a test that creates a resource without it leaks that
/// resource for good when its own cleanup does not run (#492).
#[allow(dead_code)]
pub const TEST_RESOURCE_PREFIX: &str = "genai-rs-test";

/// `genai-rs-test-{label}-{unix seconds}-{nanos}`: unique per call, marked
/// for the janitor, and carrying its creation time. Agents come back with
/// no timestamp at all (verified 2026-11-05), so the janitor reads their age
/// from this name rather than deleting one a running test still uses. Use
/// it for every id or display name a live test chooses.
#[allow(dead_code)]
pub fn test_resource_name(label: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch");
    format!(
        "{TEST_RESOURCE_PREFIX}-{label}-{}-{}",
        now.as_secs(),
        now.subsec_nanos()
    )
}

/// Content for an inline environment source, marked for the janitor: an
/// environment whose inline sources all start with the prefix is a test's.
#[allow(dead_code)]
pub fn test_source_content(text: &str) -> String {
    format!("{TEST_RESOURCE_PREFIX} {text}")
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
        .unwrap_or_else(|_| panic!("Test timed out after {duration:?}"))
}

// =============================================================================
// Polling Utilities
// =============================================================================

/// Polls a stored interaction until it leaves `InProgress` or `Queued`,
/// returning it in whatever terminal state it reached. Unknown statuses keep
/// polling (Evergreen). Panics on an API error or when `max_wait` elapses.
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
            .interactions()
            .get(interaction_id)
            .await
            .unwrap_or_else(|e| panic!("polling {interaction_id} failed: {e:?}"));
        match response.status {
            InteractionStatus::InProgress | InteractionStatus::Queued => {}
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
// Stored read-backs
// =============================================================================

/// Reads a stored interaction back and returns the value at `pointer`, a
/// JSON pointer into the interaction as the API stored it
/// (`/generation_config/transcription_config`), or `None` when nothing is
/// there. Transient errors are retried; a read that still fails is an
/// `Err`, not a panic, so the test cleans up before [`assert_stored`]
/// reports it.
///
/// A 200 on create shows a field was accepted, not that it was applied:
/// create responses drop fields the stored record keeps (#476), so a live
/// test of a request field compares this with what it sent (#492). A
/// pointer under `/input` reads with `include_input=true`, which returns
/// the input as a list of steps (a single `user_input` step for one turn,
/// live 2026-11-04). The record is the typed response serialized again,
/// which keeps every field the crate does not model as it arrived (D-001).
#[allow(dead_code)]
pub async fn stored_echo(
    client: &Client,
    interaction_id: &str,
    pointer: &str,
) -> Result<Option<serde_json::Value>, GenaiError> {
    let stored = retry_on_transient(DEFAULT_MAX_RETRIES, || async move {
        let interactions = client.interactions();
        if pointer.starts_with("/input") {
            interactions.get_with_input(interaction_id).await
        } else {
            interactions.get(interaction_id).await
        }
    })
    .await?;
    let record = serde_json::to_value(&stored).expect("an interaction serializes");
    Ok(record.pointer(pointer).cloned())
}

/// How long [`stored_echo_when_present`] waits for a field to appear.
#[allow(dead_code)]
pub const STORED_ECHO_WAIT: Duration = Duration::from_secs(10);

/// [`stored_echo`], read again until `pointer` holds a value, for up to
/// [`STORED_ECHO_WAIT`]. A background run's record can lack a field for a
/// moment after its create returned: while an Antigravity or Deep Research
/// run starts, a read returns a sparse record without `agent_config`, and
/// one read after the cancel still met it in three runs of one day (#504).
/// The last read is returned as it came: still `None` after the wait, or an
/// `Err` (not retried here; `stored_echo` already retries transients).
#[allow(dead_code)]
pub async fn stored_echo_when_present(
    client: &Client,
    interaction_id: &str,
    pointer: &str,
) -> Result<Option<serde_json::Value>, GenaiError> {
    // tokio's clock, so a test on a paused one sees the wait in virtual time.
    let deadline = tokio::time::Instant::now() + STORED_ECHO_WAIT;
    let mut delay = Duration::from_millis(500);
    loop {
        let read = stored_echo(client, interaction_id, pointer).await;
        if !matches!(read, Ok(None)) || tokio::time::Instant::now() + delay > deadline {
            return read;
        }
        println!("{interaction_id}: nothing stored at {pointer} yet; reading again in {delay:?}");
        sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(2));
    }
}

/// Asserts that a stored read-back holds everything that was sent: every
/// key of `sent`, at every depth, with an equal value. The stored record may
/// hold more (defaults the server fills in), which is not a mismatch. An
/// integer comes back as its decimal string where the API stores an int64
/// (protobuf JSON: `max_total_tokens: "200000"`, 2026-11-05), which counts
/// as equal. `stored` is [`stored_echo`]'s result; a failed read fails here.
#[allow(dead_code)]
#[track_caller]
pub fn assert_stored(
    what: &str,
    sent: &serde_json::Value,
    stored: Result<Option<serde_json::Value>, GenaiError>,
) {
    fn missing(
        sent: &serde_json::Value,
        stored: &serde_json::Value,
        at: &str,
        out: &mut Vec<String>,
    ) {
        match (sent, stored) {
            (serde_json::Value::Object(sent), serde_json::Value::Object(stored)) => {
                for (key, value) in sent {
                    match stored.get(key) {
                        Some(found) => missing(value, found, &format!("{at}/{key}"), out),
                        None => out.push(format!("{at}/{key}: not stored")),
                    }
                }
            }
            (serde_json::Value::Number(n), serde_json::Value::String(s))
                if (n.is_i64() || n.is_u64()) && *s == n.to_string() => {}
            _ if sent == stored => {}
            _ => out.push(format!("{at}: sent {sent}, stored {stored}")),
        }
    }
    let stored = stored
        .unwrap_or_else(|e| panic!("{what}: reading the interaction back failed: {e:?}"))
        .unwrap_or_else(|| panic!("{what}: nothing stored; sent {sent}"));
    let mut problems = Vec::new();
    missing(sent, &stored, "", &mut problems);
    assert!(
        problems.is_empty(),
        "{what}: the stored record differs from what was sent:\n  {}\nstored: {stored}",
        problems.join("\n  ")
    );
}

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
