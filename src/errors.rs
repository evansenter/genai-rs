use thiserror::Error;

/// Defines errors that can occur when interacting with the GenAI API.
///
/// # Example: Handling API Errors
///
/// ```no_run
/// # use genai_rs::{Client, GenaiError};
/// # async fn example(client: Client) {
/// match client.interaction().with_text("Hi").create().await {
///     Err(GenaiError::Api { status_code: 429, request_id, .. }) => {
///         eprintln!("Rate limited, request_id: {request_id:?}");
///         // Retry with backoff
///     }
///     Err(GenaiError::Api { status_code, message, request_id, .. }) => {
///         eprintln!("API error {status_code}: {message} (request: {request_id:?})");
///     }
///     Err(other) => eprintln!("{other}"),
///     Ok(response) => println!("{:?}", response.as_text()),
/// }
/// # }
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GenaiError {
    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SSE parsing error: {0}")]
    Parse(String),
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("UTF-8 decoding error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    /// API error with structured context for debugging and automated handling.
    ///
    /// Contains the HTTP status code (for retry logic), error message, and
    /// optional request ID (for correlation with Google API logs/support).
    #[error("API error (HTTP {status_code}): {message}")]
    Api {
        /// HTTP status code (e.g., 400, 429, 500)
        status_code: u16,
        /// The `message` of Google's `{"error": {...}}` envelope, prefixed with
        /// its symbolic status or code (`"INVALID_ARGUMENT: ..."`,
        /// `"invalid_request: ..."`). A body that is not an envelope is
        /// included raw, truncated.
        message: String,
        /// Request ID from `x-goog-request-id` header, if available
        request_id: Option<String>,
        /// Retry delay from `Retry-After` header (for 429 rate limit errors).
        ///
        /// When present, this indicates how long to wait before retrying.
        /// The value is parsed from the `Retry-After` header, which can be:
        /// - Seconds (e.g., "120" → 120 seconds)
        /// - HTTP date (e.g., "Wed, 21 Oct 2015 07:28:00 GMT" → duration until then)
        ///
        /// This field is typically only populated for 429 (Too Many Requests) errors.
        retry_after: Option<std::time::Duration>,
    },
    #[error("Internal client error: {0}")]
    Internal(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    /// API returned a successful response but with unexpected or invalid content.
    ///
    /// This indicates the API response didn't match the expected schema,
    /// possibly due to API evolution or an undocumented response format.
    /// Unlike `InvalidInput` (user's fault), this represents an issue with
    /// the API response itself.
    #[error("Malformed API response: {0}")]
    MalformedResponse(String),
    /// Request timed out after the specified duration.
    ///
    /// Returned when a request exceeds
    /// [`InteractionBuilder::with_timeout`](crate::InteractionBuilder::with_timeout).
    /// [`ClientBuilder::with_timeout`](crate::ClientBuilder::with_timeout) is
    /// enforced by the HTTP client instead and surfaces as [`GenaiError::Http`].
    #[error("Request timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// The server reported an error inside a stream (an SSE `error` event)
    /// after the HTTP status had already succeeded.
    ///
    /// Returned by the auto-function streaming loop, which cannot continue
    /// past it. Plain streams surface the same event as
    /// [`StreamChunk::Error`](crate::StreamChunk::Error) instead.
    #[error("Stream error: {message}")]
    Stream {
        /// Human-readable error message from the event
        message: String,
        /// Error code from the event, if provided
        code: Option<String>,
    },
    /// Failed to build the HTTP client.
    ///
    /// This typically only occurs in exceptional circumstances such as
    /// TLS backend initialization failures.
    #[error("Failed to build HTTP client: {0}")]
    ClientBuild(String),
}

impl GenaiError {
    /// Returns `true` if this error is likely transient and the request may succeed on retry.
    ///
    /// This helper identifies errors that are typically recoverable:
    /// - **HTTP errors**: Network issues, connection resets, TLS errors
    /// - **Rate limits (429)**: Temporary throttling, retry after backoff
    /// - **Server errors (5xx)**: Temporary server issues
    /// - **Timeouts**: Request took too long but may succeed with retry
    ///
    /// Errors that return `false` are typically permanent and retrying won't help:
    /// - **Client errors (4xx except 429)**: Bad request, unauthorized, not found
    /// - **Parse/JSON errors**: Response format issues
    /// - **Invalid input**: Request validation failures
    /// - **Malformed response**: API contract violations
    ///
    /// # Example
    ///
    /// ```rust
    /// use genai_rs::GenaiError;
    /// use std::time::Duration;
    ///
    /// fn should_retry(error: &GenaiError, attempt: u32, max_attempts: u32) -> bool {
    ///     attempt < max_attempts && error.is_retryable()
    /// }
    ///
    /// // Rate limit errors are retryable
    /// let rate_limited = GenaiError::Api {
    ///     status_code: 429,
    ///     message: "Resource exhausted".to_string(),
    ///     request_id: None,
    ///     retry_after: Some(std::time::Duration::from_secs(60)),
    /// };
    /// assert!(rate_limited.is_retryable());
    ///
    /// // Bad request errors are not retryable
    /// let bad_request = GenaiError::Api {
    ///     status_code: 400,
    ///     message: "Invalid model".to_string(),
    ///     request_id: None,
    ///     retry_after: None,
    /// };
    /// assert!(!bad_request.is_retryable());
    ///
    /// // Timeouts are retryable
    /// let timeout = GenaiError::Timeout(Duration::from_secs(30));
    /// assert!(timeout.is_retryable());
    /// ```
    ///
    /// # Retry Strategy
    ///
    /// When implementing retry logic, consider:
    /// - Use exponential backoff with jitter
    /// - Set a maximum number of retries
    /// - For 429 errors, use the `retry_after` field if available (extracted from `Retry-After` header)
    /// - Log retries for observability
    ///
    /// See `examples/retry_with_backoff.rs` for a complete retry implementation.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            // Network-level errors are typically transient
            GenaiError::Http(_) => true,

            // API errors: 429 (rate limit) and 5xx (server errors) are retryable
            GenaiError::Api { status_code, .. } => *status_code == 429 || *status_code >= 500,

            // Timeouts may succeed on retry
            GenaiError::Timeout(_) => true,

            // These are permanent errors - retrying won't help
            GenaiError::Parse(_)
            | GenaiError::Json(_)
            | GenaiError::Utf8(_)
            | GenaiError::Internal(_)
            | GenaiError::InvalidInput(_)
            | GenaiError::MalformedResponse(_)
            | GenaiError::Stream { .. }
            | GenaiError::ClientBuild(_) => false,
        }
    }

    /// Returns the server-suggested retry delay for rate limit (429) errors.
    ///
    /// This extracts the `Retry-After` header value that was parsed when the error
    /// was created. Only `Api` errors with status code 429 typically have this field set.
    ///
    /// # Returns
    ///
    /// - `Some(Duration)` if the error has a `retry_after` value
    /// - `None` for all other error types or if no `Retry-After` header was present
    ///
    /// # Example
    ///
    /// ```rust
    /// use genai_rs::GenaiError;
    /// use std::time::Duration;
    ///
    /// async fn with_server_suggested_delay(error: &GenaiError) {
    ///     if let Some(delay) = error.retry_after() {
    ///         // Server told us how long to wait
    ///         tokio::time::sleep(delay).await;
    ///     } else {
    ///         // Fall back to our own backoff strategy
    ///         tokio::time::sleep(Duration::from_secs(1)).await;
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            GenaiError::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genai_error_parse_display() {
        let error = GenaiError::Parse("Invalid SSE format".to_string());
        let display = format!("{}", error);
        assert!(display.contains("SSE parsing error"));
        assert!(display.contains("Invalid SSE format"));
    }

    #[test]
    fn test_genai_error_api_display() {
        let error = GenaiError::Api {
            status_code: 429,
            message: "Rate limited".to_string(),
            request_id: Some("req-123".to_string()),
            retry_after: None,
        };
        let display = format!("{}", error);
        assert!(display.contains("429"));
        assert!(display.contains("Rate limited"));
    }

    #[test]
    fn test_genai_error_api_without_request_id() {
        let error = GenaiError::Api {
            status_code: 500,
            message: "Internal error".to_string(),
            request_id: None,
            retry_after: None,
        };
        let display = format!("{}", error);
        assert!(display.contains("500"));
        assert!(display.contains("Internal error"));
    }

    #[test]
    fn test_genai_error_internal_display() {
        let error = GenaiError::Internal("Max function loops exceeded".to_string());
        let display = format!("{}", error);
        assert!(display.contains("Internal client error"));
        assert!(display.contains("Max function loops exceeded"));
    }

    #[test]
    fn test_genai_error_invalid_input_display() {
        let error = GenaiError::InvalidInput("Missing model or agent".to_string());
        let display = format!("{}", error);
        assert!(display.contains("Invalid input"));
        assert!(display.contains("Missing model or agent"));
    }

    #[test]
    fn test_genai_error_json_from() {
        let json_str = "not valid json";
        let json_err = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
        let genai_err: GenaiError = json_err.into();
        let display = format!("{}", genai_err);
        assert!(display.contains("JSON deserialization error"));
    }

    #[test]
    fn test_genai_error_utf8_from() {
        // Create an invalid UTF-8 byte sequence
        let bytes = vec![0xff, 0xfe];
        let utf8_err = std::str::from_utf8(&bytes).unwrap_err();
        let genai_err: GenaiError = utf8_err.into();
        let display = format!("{}", genai_err);
        assert!(display.contains("UTF-8 decoding error"));
    }

    #[test]
    fn test_genai_error_debug_format() {
        let error = GenaiError::Api {
            status_code: 400,
            message: "Bad request".to_string(),
            request_id: Some("req-456".to_string()),
            retry_after: None,
        };
        let debug = format!("{:?}", error);
        assert!(debug.contains("Api"));
        assert!(debug.contains("400"));
        assert!(debug.contains("req-456"));
    }

    #[test]
    fn test_genai_error_api_status_codes() {
        // Test common HTTP status codes
        let status_codes = [
            (400, "Bad Request"),
            (401, "Unauthorized"),
            (403, "Forbidden"),
            (404, "Not Found"),
            (429, "Too Many Requests"),
            (500, "Internal Server Error"),
            (503, "Service Unavailable"),
        ];

        for (code, message) in status_codes {
            let error = GenaiError::Api {
                status_code: code,
                message: message.to_string(),
                request_id: None,
                retry_after: None,
            };
            let display = format!("{}", error);
            assert!(
                display.contains(&code.to_string()),
                "Expected {} in display: {}",
                code,
                display
            );
        }
    }

    #[test]
    fn test_genai_error_api_with_empty_message() {
        // Some APIs might return empty error messages
        let error = GenaiError::Api {
            status_code: 500,
            message: "".to_string(),
            request_id: None,
            retry_after: None,
        };
        let display = format!("{}", error);
        assert!(display.contains("500"));
        // Should still display properly even with empty message
        assert!(display.contains("API error"));
    }

    #[test]
    fn test_genai_error_malformed_response_display() {
        let error = GenaiError::MalformedResponse(
            "Function call 'get_weather' is missing required call_id field".to_string(),
        );
        let display = format!("{}", error);
        assert!(display.contains("Malformed API response"));
        assert!(display.contains("call_id"));
    }

    #[test]
    fn test_genai_error_malformed_response_stream() {
        let error =
            GenaiError::MalformedResponse("Stream ended without Complete event".to_string());
        let display = format!("{}", error);
        assert!(display.contains("Malformed API response"));
        assert!(display.contains("Complete event"));
    }

    #[test]
    fn test_genai_error_timeout_display() {
        let error = GenaiError::Timeout(std::time::Duration::from_secs(30));
        let display = format!("{}", error);
        assert!(display.contains("Request timed out"));
        assert!(display.contains("30s"));
    }

    #[test]
    fn test_genai_error_timeout_debug() {
        let error = GenaiError::Timeout(std::time::Duration::from_millis(500));
        let debug = format!("{:?}", error);
        assert!(debug.contains("Timeout"));
        assert!(debug.contains("500ms"));
    }

    #[test]
    fn test_genai_error_client_build_display() {
        let error = GenaiError::ClientBuild("TLS initialization failed".to_string());
        let display = format!("{}", error);
        assert!(display.contains("Failed to build HTTP client"));
        assert!(display.contains("TLS initialization failed"));
    }

    #[test]
    fn test_genai_error_client_build_debug() {
        let error = GenaiError::ClientBuild("some error".to_string());
        let debug = format!("{:?}", error);
        assert!(debug.contains("ClientBuild"));
        assert!(debug.contains("some error"));
    }

    // =============================================================================
    // is_retryable() Tests
    // =============================================================================

    #[test]
    fn test_is_retryable_rate_limit_429() {
        let error = GenaiError::Api {
            status_code: 429,
            message: "Resource exhausted".to_string(),
            request_id: None,
            retry_after: Some(std::time::Duration::from_secs(60)),
        };
        assert!(error.is_retryable(), "429 errors should be retryable");
    }

    #[test]
    fn test_is_retryable_server_errors_5xx() {
        for status_code in [500, 502, 503, 504] {
            let error = GenaiError::Api {
                status_code,
                message: "Server error".to_string(),
                request_id: None,
                retry_after: None,
            };
            assert!(
                error.is_retryable(),
                "{} errors should be retryable",
                status_code
            );
        }
    }

    #[test]
    fn test_is_retryable_client_errors_4xx_not_retryable() {
        // Client errors (except 429) should NOT be retryable
        for status_code in [400, 401, 403, 404, 422] {
            let error = GenaiError::Api {
                status_code,
                message: "Client error".to_string(),
                request_id: None,
                retry_after: None,
            };
            assert!(
                !error.is_retryable(),
                "{} errors should NOT be retryable",
                status_code
            );
        }
    }

    #[test]
    fn test_is_retryable_timeout() {
        let error = GenaiError::Timeout(std::time::Duration::from_secs(30));
        assert!(error.is_retryable(), "Timeout errors should be retryable");
    }

    #[test]
    fn test_is_retryable_parse_error_not_retryable() {
        let error = GenaiError::Parse("Invalid SSE".to_string());
        assert!(
            !error.is_retryable(),
            "Parse errors should NOT be retryable"
        );
    }

    #[test]
    fn test_is_retryable_json_error_not_retryable() {
        let json_str = "not valid json";
        let json_err = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
        let error: GenaiError = json_err.into();
        assert!(!error.is_retryable(), "JSON errors should NOT be retryable");
    }

    #[test]
    fn test_is_retryable_invalid_input_not_retryable() {
        let error = GenaiError::InvalidInput("Missing model".to_string());
        assert!(
            !error.is_retryable(),
            "InvalidInput errors should NOT be retryable"
        );
    }

    #[test]
    fn test_is_retryable_malformed_response_not_retryable() {
        let error = GenaiError::MalformedResponse("Missing call_id".to_string());
        assert!(
            !error.is_retryable(),
            "MalformedResponse errors should NOT be retryable"
        );
    }

    #[test]
    fn test_is_retryable_internal_error_not_retryable() {
        let error = GenaiError::Internal("Max loops exceeded".to_string());
        assert!(
            !error.is_retryable(),
            "Internal errors should NOT be retryable"
        );
    }

    #[test]
    fn test_is_retryable_client_build_not_retryable() {
        let error = GenaiError::ClientBuild("TLS init failed".to_string());
        assert!(
            !error.is_retryable(),
            "ClientBuild errors should NOT be retryable"
        );
    }

    #[test]
    fn test_is_retryable_utf8_error_not_retryable() {
        let bytes = vec![0xff, 0xfe];
        let utf8_err = std::str::from_utf8(&bytes).unwrap_err();
        let error: GenaiError = utf8_err.into();
        assert!(
            !error.is_retryable(),
            "UTF-8 errors should NOT be retryable"
        );
    }

    #[test]
    fn test_stream_error_display_and_not_retryable() {
        let error = GenaiError::Stream {
            message: "quota exhausted mid-stream".to_string(),
            code: Some("resource_exhausted".to_string()),
        };
        assert_eq!(
            error.to_string(),
            "Stream error: quota exhausted mid-stream"
        );
        assert!(!error.is_retryable());
    }
}
