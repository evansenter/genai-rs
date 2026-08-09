//! Error handling tests for the genai-rs library.
//!
//! These tests verify that error types, error messages, and retry behavior
//! work correctly. Most tests don't require an API key.
//!
//! # Running Tests
//!
//! ```bash
//! cargo test --test error_handling_tests -- --nocapture
//! ```

mod common;

use common::{is_transient_error, retry_on_transient};
use genai_rs::GenaiError;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

// =============================================================================
// GenaiError Display Tests
// =============================================================================

#[test]
fn test_genai_error_parse_display() {
    // We can't easily create a reqwest::Error, so we test Parse error instead
    let error = GenaiError::Parse("Connection reset".to_string());
    let display = format!("{}", error);
    assert!(display.contains("SSE parsing error"));
}

#[test]
fn test_genai_error_api_429_rate_limit() {
    let error = GenaiError::Api {
        status_code: 429,
        message: "Resource exhausted".to_string(),
        request_id: Some("req-abc123".to_string()),
        retry_after: Some(std::time::Duration::from_secs(60)),
    };
    let display = format!("{}", error);
    assert!(display.contains("429"));
    assert!(display.contains("Resource exhausted"));

    // Verify we can pattern match on 429 for retry logic
    match &error {
        GenaiError::Api {
            status_code: 429, ..
        } => {
            // This is the pattern for rate limit handling
        }
        _ => panic!("Should match 429 status code"),
    }
}

#[test]
fn test_genai_error_api_503_service_unavailable() {
    let error = GenaiError::Api {
        status_code: 503,
        message: "Service temporarily unavailable".to_string(),
        request_id: None,
        retry_after: None,
    };
    let display = format!("{}", error);
    assert!(display.contains("503"));
    assert!(display.contains("Service temporarily unavailable"));
}

#[test]
fn test_genai_error_api_400_bad_request() {
    let error = GenaiError::Api {
        status_code: 400,
        message: "Invalid model name".to_string(),
        request_id: Some("req-xyz789".to_string()),
        retry_after: None,
    };
    let display = format!("{}", error);
    assert!(display.contains("400"));
    assert!(display.contains("Invalid model name"));
}

#[test]
fn test_genai_error_api_request_id_in_debug() {
    // request_id is available in Debug output (for logging) and pattern matching,
    // but intentionally excluded from Display to keep error messages concise.
    let error = GenaiError::Api {
        status_code: 500,
        message: "Server error".to_string(),
        request_id: Some("req-debug-12345".to_string()),
        retry_after: None,
    };

    // Display is concise - no request_id
    let display = format!("{}", error);
    assert!(display.contains("500"));
    assert!(display.contains("Server error"));

    // Debug includes request_id for logging/diagnostics
    let debug = format!("{:?}", error);
    assert!(
        debug.contains("req-debug-12345"),
        "Debug output should include request_id: {}",
        debug
    );
}

#[test]
fn test_genai_error_malformed_response_patterns() {
    // Test various MalformedResponse scenarios documented in the codebase

    // Missing call_id in function call
    let error1 = GenaiError::MalformedResponse(
        "Function call 'get_weather' is missing required call_id field".to_string(),
    );
    let display1 = format!("{}", error1);
    assert!(display1.contains("Malformed API response"));
    assert!(display1.contains("call_id"));

    // Stream ended without Complete event
    let error2 = GenaiError::MalformedResponse("Stream ended without Complete event".to_string());
    let display2 = format!("{}", error2);
    assert!(display2.contains("Complete event"));
}

#[test]
fn test_genai_error_invalid_input() {
    let error = GenaiError::InvalidInput("Model or agent must be specified".to_string());
    let display = format!("{}", error);
    assert!(display.contains("Invalid input"));
    assert!(display.contains("Model or agent"));
}

#[test]
fn test_genai_error_internal() {
    let error = GenaiError::Internal("Exceeded maximum function call loops".to_string());
    let display = format!("{}", error);
    assert!(display.contains("Internal client error"));
    assert!(display.contains("function call loops"));
}

// =============================================================================
// Transient Error Detection Tests
// =============================================================================

#[test]
fn test_is_transient_error_spanner_utf8() {
    // This is the known transient error pattern from issue #60
    let error = GenaiError::Api {
        status_code: 500,
        message: "Spanner UTF-8 encoding error in backend".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        is_transient_error(&error),
        "Spanner UTF-8 error should be transient"
    );
}

#[test]
fn test_is_transient_error_case_insensitive() {
    // Test case insensitivity
    let error = GenaiError::Api {
        status_code: 500,
        message: "SPANNER UTF-8 error".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        is_transient_error(&error),
        "Spanner detection should be case insensitive"
    );
}

#[test]
fn test_is_transient_error_requires_both_keywords() {
    // Only "spanner" is not enough
    let error1 = GenaiError::Api {
        status_code: 500,
        message: "Spanner database error".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        !is_transient_error(&error1),
        "Just 'spanner' should not be transient"
    );

    // Only "utf-8" is not enough
    let error2 = GenaiError::Api {
        status_code: 500,
        message: "UTF-8 encoding error".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        !is_transient_error(&error2),
        "Just 'utf-8' should not be transient"
    );
}

#[test]
fn test_is_transient_error_non_api_errors() {
    // Non-API errors are never transient
    let parse_error = GenaiError::Parse("Invalid SSE".to_string());
    assert!(
        !is_transient_error(&parse_error),
        "Parse errors are not transient"
    );

    let internal_error = GenaiError::Internal("Max loops".to_string());
    assert!(
        !is_transient_error(&internal_error),
        "Internal errors are not transient"
    );

    let invalid_input = GenaiError::InvalidInput("Missing model".to_string());
    assert!(
        !is_transient_error(&invalid_input),
        "Invalid input errors are not transient"
    );

    let malformed = GenaiError::MalformedResponse("Missing call_id".to_string());
    assert!(
        !is_transient_error(&malformed),
        "MalformedResponse errors are not transient"
    );
}

#[test]
fn test_is_transient_error_regular_500() {
    // Regular 500 errors without spanner/utf-8 are not transient
    let error = GenaiError::Api {
        status_code: 500,
        message: "Internal server error".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        !is_transient_error(&error),
        "Generic 500 should not be transient"
    );
}

#[test]
fn test_is_transient_error_message_pinned_400_arms() {
    // The two message-pinned 400 classes retry: the model's occasional
    // invalid structured-output JSON, and the server-side processing
    // burst (generic apology, observed failing in under a second on
    // requests that pass on re-run).
    let invalid_json = GenaiError::Api {
        status_code: 400,
        message: "Invalid JSON syntax at line 3".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        is_transient_error(&invalid_json),
        "Invalid-JSON 400 should be transient"
    );

    let processing_burst = GenaiError::Api {
        status_code: 400,
        message: r#"{"error":{"message":"There was a problem processing your request. You will not be charged.","code":"invalid_request"}}"#.to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        is_transient_error(&processing_burst),
        "Processing-burst 400 should be transient"
    );

    // The fail-loud contract: a 400 that names its actual problem matches
    // neither phrase and fails on the first attempt.
    let fixture_rejection = GenaiError::Api {
        status_code: 400,
        message:
            r#"{"error":{"message":"Unknown field 'speech_configs'","code":"invalid_request"}}"#
                .to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        !is_transient_error(&fixture_rejection),
        "A 400 naming its problem should fail loud"
    );

    // Transport classes (429, plain 5xx bursts) are is_retryable's job,
    // not this predicate's.
    let rate_limited = GenaiError::Api {
        status_code: 429,
        message: "rate limited".to_string(),
        request_id: None,
        retry_after: None,
    };
    assert!(
        !is_transient_error(&rate_limited),
        "429 belongs to is_retryable, not is_transient_error"
    );
}

// =============================================================================
// Retry Logic Tests
// =============================================================================

#[tokio::test]
async fn test_retry_on_transient_success_first_try() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(3, || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok::<_, GenaiError>("success".to_string())
        }
    })
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Should only call once on success"
    );
}

#[tokio::test]
async fn test_retry_on_transient_non_transient_error_no_retry() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(3, || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(GenaiError::InvalidInput("bad input".to_string()))
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Should not retry non-transient errors"
    );
}

#[tokio::test]
async fn test_retry_on_transient_success_after_retry() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(3, || {
        let count = count.clone();
        async move {
            let attempt = count.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                // First attempt fails with transient error
                Err(GenaiError::Api {
                    status_code: 500,
                    message: "Spanner UTF-8 error".to_string(),
                    request_id: None,
                    retry_after: None,
                })
            } else {
                // Second attempt succeeds
                Ok("recovered".to_string())
            }
        }
    })
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "recovered");
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "Should retry once and then succeed"
    );
}

#[tokio::test]
async fn test_retry_on_transient_exhausted_retries() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(2, || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(GenaiError::Api {
                status_code: 500,
                message: "Spanner UTF-8 error".to_string(),
                request_id: None,
                retry_after: None,
            })
        }
    })
    .await;

    assert!(result.is_err());
    // max_retries=2 means: initial attempt + 2 retries = 3 total attempts
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        3,
        "Should attempt initial + max_retries times"
    );
}

#[tokio::test]
async fn test_retry_on_transient_zero_retries() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(0, || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(GenaiError::Api {
                status_code: 500,
                message: "Spanner UTF-8 error".to_string(),
                request_id: None,
                retry_after: None,
            })
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "max_retries=0 should only run once"
    );
}

/// A plain 5xx with no model-side flake marker is retried via
/// `GenaiError::is_retryable` — the predicate widening the retry sweep
/// relies on. Before that widening, only `is_transient_error` cases
/// retried, so this test pins the new behavior.
#[tokio::test]
async fn test_retry_on_transient_retries_plain_5xx() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(1, || {
        let count = count.clone();
        async move {
            let attempt = count.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                Err(GenaiError::Api {
                    status_code: 503,
                    message: "The service is currently unavailable.".to_string(),
                    request_id: None,
                    retry_after: None,
                })
            } else {
                Ok("recovered".to_string())
            }
        }
    })
    .await;

    assert!(result.is_ok());
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        2,
        "A plain 503 should be retried"
    );
}

/// A 400 invalid_request (e.g. a rejected fixture) carries neither
/// transient marker, so it must fail on the first attempt — the property
/// the retry-then-assert-strictly sweep depends on.
#[tokio::test]
async fn test_retry_on_transient_invalid_request_fails_first_attempt() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let result = retry_on_transient(3, || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(GenaiError::Api {
                status_code: 400,
                message: "invalid_request: Provided content is invalid.".to_string(),
                request_id: None,
                retry_after: None,
            })
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "A 400 invalid_request must not be retried"
    );
}

/// A retryable error whose server-sent Retry-After exceeds what a harness
/// timeout budget can afford aborts the retry immediately, surfacing the
/// real error instead of sleeping toward an opaque timeout. No wall clock
/// is spent, so this also pins that the abort happens before the sleep.
#[tokio::test]
async fn test_retry_on_transient_aborts_on_long_retry_after() {
    let call_count = Arc::new(AtomicU32::new(0));
    let count = call_count.clone();

    let start = std::time::Instant::now();
    let result = retry_on_transient(3, || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            Err::<String, _>(GenaiError::Api {
                status_code: 429,
                message: "Rate limited".to_string(),
                request_id: None,
                retry_after: Some(std::time::Duration::from_secs(30)),
            })
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "A Retry-After beyond the cap must abort instead of retrying"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "The abort must happen without sleeping out the requested delay"
    );
}

// =============================================================================
// Error Matching Pattern Tests
// =============================================================================

#[test]
fn test_error_matching_for_retry_logic() {
    // Test the pattern users would use for implementing their own retry logic
    let errors = vec![
        (
            GenaiError::Api {
                status_code: 429,
                message: "Rate limited".to_string(),
                request_id: None,
                retry_after: None,
            },
            "rate_limit",
        ),
        (
            GenaiError::Api {
                status_code: 500,
                message: "Server error".to_string(),
                request_id: None,
                retry_after: None,
            },
            "server_error",
        ),
        (
            GenaiError::Api {
                status_code: 503,
                message: "Unavailable".to_string(),
                request_id: None,
                retry_after: None,
            },
            "unavailable",
        ),
    ];

    for (error, expected_category) in errors {
        let category = match &error {
            GenaiError::Api {
                status_code: 429, ..
            } => "rate_limit",
            GenaiError::Api {
                status_code: 500, ..
            } => "server_error",
            GenaiError::Api {
                status_code: 503, ..
            } => "unavailable",
            _ => "other",
        };
        assert_eq!(
            category, expected_category,
            "Error matching should work for {:?}",
            error
        );
    }
}

#[test]
fn test_error_request_id_extraction() {
    let error = GenaiError::Api {
        status_code: 500,
        message: "Error".to_string(),
        request_id: Some("req-abc123".to_string()),
        retry_after: None,
    };

    // Test pattern for extracting request_id for logging/debugging
    if let GenaiError::Api {
        request_id: Some(id),
        ..
    } = &error
    {
        assert_eq!(id, "req-abc123");
    } else {
        panic!("Should extract request_id");
    }
}

// =============================================================================
// Client Configuration Error Tests
// =============================================================================

#[test]
fn test_client_with_empty_api_key() {
    // Empty API key should still create a client (validation happens at request time)
    let client = genai_rs::Client::builder("".to_string()).build().unwrap();
    // Client creation succeeds - the API will return 401 when used
    assert!(
        client
            .interaction()
            .with_model("gemini-3-flash-preview")
            .with_text("test")
            .build()
            .is_ok()
    );
}

#[test]
fn test_interaction_builder_missing_model() {
    let client = genai_rs::Client::builder("fake-key".to_string())
        .build()
        .unwrap();

    // Building a request without a model should fail
    let result = client.interaction().with_text("test").build();

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("model") || err.contains("agent"),
        "Error should mention missing model: {}",
        err
    );
}

#[test]
fn test_interaction_builder_missing_content() {
    let client = genai_rs::Client::builder("fake-key".to_string())
        .build()
        .unwrap();

    // Building a request without content should fail
    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .build();

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("content") || err.contains("input"),
        "Error should mention missing content: {}",
        err
    );
}

// =============================================================================
// API Error Integration Tests
// =============================================================================

/// Test that invalid API keys return appropriate 401/403 errors.
///
/// This test intentionally uses an invalid API key to verify that:
/// - The library correctly propagates authentication errors
/// - Error messages indicate the authentication failure
/// - Status codes are captured correctly (400/401/403)
#[tokio::test]
async fn test_invalid_api_key_returns_auth_error() {
    // Create client with intentionally invalid API key
    let client = genai_rs::Client::builder("invalid-api-key-12345".to_string())
        .build()
        .unwrap();

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .create()
        .await;

    // Should fail with an API error
    assert!(result.is_err(), "Request with invalid API key should fail");

    let error = result.unwrap_err();
    println!("Error received: {:?}", error);

    // Verify it's an API error with auth-related status code
    match &error {
        GenaiError::Api {
            status_code,
            message,
            ..
        } => {
            // Google API returns 400 for invalid API key format
            // or 401/403 for invalid but properly formatted keys
            assert!(
                *status_code == 400 || *status_code == 401 || *status_code == 403,
                "Expected auth error (400/401/403), got {}: {}",
                status_code,
                message
            );
            println!(
                "✓ Correctly received {} error for invalid API key",
                status_code
            );
        }
        _ => panic!(
            "Expected GenaiError::Api for auth failure, got: {:?}",
            error
        ),
    }
}

/// Test that malformed API keys are rejected appropriately.
#[tokio::test]
async fn test_malformed_api_key() {
    // Create client with empty API key
    let client = genai_rs::Client::builder("".to_string()).build().unwrap();

    let result = client
        .interaction()
        .with_model("gemini-3-flash-preview")
        .with_text("Hello")
        .create()
        .await;

    // Should fail
    assert!(result.is_err(), "Request with empty API key should fail");
    println!("Empty key error: {:?}", result.unwrap_err());
}
