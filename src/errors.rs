//! [`GenaiError`], the error every API and transport call returns.

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
///     Err(other) => {
///         // `Display` is this error alone: an error it wraps, such as
///         // `Http`'s transport error, is its `source()`.
///         let mut text = other.to_string();
///         let mut cause = std::error::Error::source(&other);
///         while let Some(e) = cause {
///             text = format!("{text}: {e}");
///             cause = e.source();
///         }
///         eprintln!("{text}");
///     }
///     Ok(response) => println!("{:?}", response.as_text()),
/// }
/// # }
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GenaiError {
    /// A transport failure before a response arrived: connect (including
    /// the default 30 s connect timeout), TLS, a client-level timeout, or a
    /// non-streamed body that failed while it was read.
    ///
    /// The reqwest error is the [`source`](std::error::Error::source), and
    /// `Display` does not repeat its text, so a reporter that walks the
    /// chain prints it once.
    #[error("HTTP request error")]
    Http(#[from] reqwest::Error),
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
    /// The client failed in a way that is neither the API's answer nor your
    /// input: a request body that would not serialize, or an unexpected
    /// failure reading back a resource a create already made (the message
    /// then names the resource). Not retryable.
    #[error("Internal client error: {0}")]
    Internal(String),
    /// The builder or a handle rejected the request before sending it.
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    /// API returned a successful response but with unexpected or invalid content.
    ///
    /// This indicates the API response didn't match the expected schema,
    /// possibly due to API evolution or an undocumented response format.
    /// Unlike `InvalidInput` (user's fault), this represents an issue with
    /// the API response itself. It includes a streamed SSE event that does
    /// not parse or is not valid UTF-8, and one that grows past 64 MiB
    /// without ending.
    ///
    /// A stream cut off before its terminal event is
    /// [`StreamInterrupted`](Self::StreamInterrupted), not this.
    #[error("Malformed API response: {0}")]
    MalformedResponse(String),
    /// A stream's body ended before its terminal event
    /// (`interaction.completed` or an error event): the connection was cut
    /// off, or an auto-function round's body ended on an event this crate
    /// doesn't recognize.
    ///
    /// Every way the connection can end a body early lands here, so a
    /// caller sees one error for one event:
    ///
    /// - the server closed the connection between events or in the middle
    ///   of one (`source` is `None`);
    /// - the transport failed mid-body: a reset, the client's
    ///   [read](crate::ClientBuilder::with_read_timeout) or
    ///   [total](crate::ClientBuilder::with_timeout) timeout between chunks,
    ///   a decode error (`source` is the [`reqwest::Error`]).
    ///
    /// A body that ends cleanly right after an event this crate doesn't
    /// recognize is not a cut-off: the plain stream ends there without an
    /// error, since the event may be a terminal one added to the API after
    /// this release. A round of
    /// [`create_stream_with_auto_functions`](crate::InteractionBuilder::create_stream_with_auto_functions)
    /// can't go on without its `interaction.completed`, so it fails here
    /// instead (`source` is `None`). See "An unrecognized last event" in
    /// `docs/RELIABILITY.md`.
    ///
    /// A stream stopped by the request's own per-chunk limit
    /// ([`InteractionBuilder::with_timeout`](crate::InteractionBuilder::with_timeout)
    /// on `create_stream`) ends with [`Timeout`](Self::Timeout) instead: the
    /// limit the caller set ran out, and the connection was not at fault.
    ///
    /// [`is_retryable`](Self::is_retryable) is true; see there for why. A
    /// background interaction can be resumed instead of re-sent:
    /// [`Interactions::resume_stream`](crate::Interactions::resume_stream)
    /// continues the same generation from the last `event_id` you saw.
    ///
    /// A failure before the response headers arrive (connect, TLS, the
    /// read timeout while the model thinks) is [`Http`](Self::Http): no
    /// stream had started.
    ///
    /// `Display` says what ended the body, not the transport error's text:
    /// that is the [`source`](std::error::Error::source), so a reporter
    /// that walks the chain prints it once.
    #[error("Stream interrupted: {message}")]
    StreamInterrupted {
        /// What ended the body, without the transport error's text.
        message: String,
        /// The transport error, when the transport reported one.
        #[source]
        source: Option<reqwest::Error>,
    },
    /// Request timed out after the specified duration.
    ///
    /// Returned when a request exceeds
    /// [`InteractionBuilder::with_timeout`](crate::InteractionBuilder::with_timeout).
    /// [`ClientBuilder::with_timeout`](crate::ClientBuilder::with_timeout) is
    /// enforced by the HTTP client instead and surfaces as [`GenaiError::Http`].
    #[error("Request timed out after {0:?}")]
    Timeout(std::time::Duration),
    /// A wait for a resource to become ready reached its
    /// [`PollOptions`](crate::PollOptions) timeout: a `wait_until_active`,
    /// or the wait inside a file search store import.
    ///
    /// The resource may still be processing, so this is not
    /// [`is_retryable`](Self::is_retryable): waiting again is your call, and
    /// re-sending an import would make a second document. `message` says
    /// what was awaited, how long, its last state, and what to do next.
    #[error("{message}")]
    PollTimeout {
        /// The wait's timeout.
        timeout: std::time::Duration,
        /// What was awaited and what to do next.
        message: String,
    },
    /// Server-side processing of a resource reached a terminal failure: an
    /// uploaded file that failed processing, a file search document that
    /// failed to index, or an import whose operation reported an error.
    ///
    /// Not retryable: the resource will never become ready. Upload it again.
    #[error("Processing failed: {0}")]
    ProcessingFailed(String),
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
    /// TLS backend initialization failures. The reqwest error is the
    /// [`source`](std::error::Error::source), and `Display` does not repeat
    /// its text, so a reporter that walks the chain prints it once.
    #[error("Failed to build HTTP client")]
    ClientBuild(#[source] reqwest::Error),
}

impl GenaiError {
    /// Returns `true` if this error is likely transient and the request may succeed on retry.
    ///
    /// This helper identifies errors that are typically recoverable:
    /// - **HTTP errors**: Network issues, connection resets, TLS errors
    /// - **Request timeouts (408)**: the server stopped waiting before the
    ///   request arrived whole, so it acted on nothing
    /// - **Rate limits (429)**: Temporary throttling, retry after backoff
    /// - **Server errors (5xx)**: Temporary server issues, except the six
    ///   below
    /// - **Timeouts**: Request took too long but may succeed with retry
    /// - **Cut-off streams** ([`StreamInterrupted`](Self::StreamInterrupted)):
    ///   the body ended before the terminal event
    ///
    /// Errors that return `false` are typically permanent and retrying won't help:
    /// - **Client errors (4xx except 408 and 429)**: Bad request, unauthorized, not found
    /// - **The 5xx a retry cannot fix**: 501 Not Implemented (the API's
    ///   `UNIMPLEMENTED`, for a method or feature it does not support), 505
    ///   HTTP Version Not Supported, 506 Variant Also Negotiates, 508 Loop
    ///   Detected, 510 Not Extended and 511 Network Authentication Required.
    ///   Each names a condition the same request meets on every attempt.
    ///   One kind of 501 can pass: the API also sends it when a remote MCP
    ///   server answered with an error ("MCP server returned an error").
    ///   A URL that is no MCP server gets it on every attempt, but a real
    ///   server's error can pass. To retry it, match its status and
    ///   message before calling this. A server the API cannot reach gets
    ///   a 500, which is retryable.
    ///   Every other 5xx is retryable, including 507 Insufficient Storage
    ///   (temporary, per RFC 4918) and codes no RFC registers, such as a
    ///   proxy's 520 to 599.
    /// - **A status of 600 or more**: no HTTP status class, so not a server
    ///   error
    /// - **Invalid input**: Request validation failures
    /// - **Malformed response**: API contract violations, including a
    ///   stream event that does not parse
    /// - **Processing failures**: a resource the server will never finish
    /// - **Poll timeouts** ([`PollTimeout`](Self::PollTimeout)): the
    ///   resource may still be processing; wait again rather than re-send
    ///
    /// # Cut-off streams
    ///
    /// The connection can cut a stream off two ways: the server closes it
    /// early, or the transport fails mid-body (a reset, the client's read
    /// timeout). Both are [`StreamInterrupted`](Self::StreamInterrupted) and
    /// both are retryable, for three reasons:
    ///
    /// - The failure belongs to the connection, not the request: the same
    ///   request succeeds when re-sent. A non-streaming `create()` whose body
    ///   read fails the same way has always been a retryable
    ///   [`Http`](Self::Http) error, and a cut-off stream is no different.
    /// - Calling one of them permanent would make a caller retrying
    ///   "transient" errors treat the same event two ways, depending on
    ///   whether the peer closed cleanly or reset.
    /// - A background interaction keeps running server-side, so the
    ///   recovery is often a resume rather than a re-send.
    ///
    /// A stream stopped by the request's own per-chunk limit
    /// ([`InteractionBuilder::with_timeout`](crate::InteractionBuilder::with_timeout))
    /// is [`Timeout`](Self::Timeout) instead, and retryable like any timeout.
    ///
    /// Retryable does not mean free. A foreground stream's generation is
    /// cancelled server-side when its connection drops, so a retry is a new
    /// generation: discard the partial
    /// output you showed, or the retry repeats it. Retrying an auto-function
    /// stream re-runs its functions, as any retry of that loop does.
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
            // Network-level errors are typically transient. A builder error
            // (an unparsable URL such as a scheme-less `with_base_url`, a
            // rejected header) or a redirect loop fails the same way every
            // time, so retrying it only burns the retry budget.
            GenaiError::Http(e) => !(e.is_builder() || e.is_redirect()),

            GenaiError::Api { status_code, .. } => is_retryable_status(*status_code),

            // Timeouts may succeed on retry
            GenaiError::Timeout(_) => true,

            // A cut-off stream is a connection failure (see the docs above)
            GenaiError::StreamInterrupted { .. } => true,

            // These are permanent errors - retrying won't help. A poll
            // timeout's resource may still be processing: re-sending the
            // request (an import) would duplicate it.
            GenaiError::Internal(_)
            | GenaiError::InvalidInput(_)
            | GenaiError::MalformedResponse(_)
            | GenaiError::PollTimeout { .. }
            | GenaiError::ProcessingFailed(_)
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
    /// The server picks the value, so cap it: a 429 asking for an hour
    /// would otherwise wait an hour.
    ///
    /// # Example
    ///
    /// ```rust
    /// use genai_rs::GenaiError;
    /// use std::time::Duration;
    ///
    /// /// The longest wait between attempts.
    /// const MAX_DELAY: Duration = Duration::from_secs(30);
    ///
    /// /// How long to wait before retrying after `error`.
    /// fn retry_delay(error: &GenaiError) -> Duration {
    ///     match error.retry_after() {
    ///         // The server's delay, up to our own maximum
    ///         Some(asked) => asked.min(MAX_DELAY),
    ///         // Fall back to our own backoff step
    ///         None => Duration::from_secs(1),
    ///     }
    /// }
    /// # let asked = |secs| GenaiError::Api {
    /// #     status_code: 429,
    /// #     message: String::new(),
    /// #     request_id: None,
    /// #     retry_after: Some(Duration::from_secs(secs)),
    /// # };
    /// # assert_eq!(retry_delay(&asked(3600)), MAX_DELAY);
    /// # assert_eq!(retry_delay(&asked(2)), Duration::from_secs(2));
    /// ```
    #[must_use]
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            GenaiError::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

/// An error's `Display`, then that of each [`source`](std::error::Error::source)
/// below it after `": "`, for text that carries no chain of its own: a
/// [`GenaiError::PollTimeout`]'s or [`GenaiError::Internal`]'s message, or a
/// log line. `GenaiError::Http`'s `Display` leaves the transport error to
/// its source (D-049), so the text names it only through this. A source
/// whose text is already there is not added again.
pub(crate) struct ErrorChain<'a>(pub(crate) &'a (dyn std::error::Error + 'static));

impl std::fmt::Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut text = self.0.to_string();
        let mut cause = self.0.source();
        while let Some(error) = cause {
            let line = error.to_string();
            if !text.contains(&line) {
                text.push_str(": ");
                text.push_str(&line);
            }
            cause = error.source();
        }
        f.write_str(&text)
    }
}

/// Whether an API error's HTTP status can pass on a retry of the same
/// request ([`GenaiError::is_retryable`]).
///
/// Every 5xx is the server's trouble, a proxy's unregistered ones included,
/// except the registered codes whose definition is a condition the same
/// request meets again. google-genai's Interactions client retries every
/// 5xx and 409 as well; a 409 here is the request's (`ALREADY_EXISTS` never
/// passes, and `ABORTED` wants a fresh read, not the same request again).
/// See D-053.
const fn is_retryable_status(status_code: u16) -> bool {
    match status_code {
        // Request Timeout: the server stopped waiting before the request
        // arrived whole, so it acted on nothing, and the same request can
        // arrive in time.
        408
        // Too Many Requests: throttling that passes with time.
        | 429 => true,
        // Not Implemented: the server does not support the method or
        // feature (Google's `UNIMPLEMENTED`), and that does not change
        // between attempts. The API also sends it when a remote MCP
        // server answered with an error, which can pass (a server the API
        // cannot reach gets a 500 instead); the status alone cannot tell
        // the two apart, so the caller who wants that one retried matches
        // its message (docs/RELIABILITY.md).
        501
        // HTTP Version Not Supported: every attempt uses the same version.
        | 505
        // Variant Also Negotiates: a server misconfiguration in content
        // negotiation, not a passing state.
        | 506
        // Loop Detected: the same request loops again.
        | 508
        // Not Extended: the request lacks an extension the server requires.
        | 510
        // Network Authentication Required: a captive portal wants a
        // sign-in, which no retry provides.
        | 511 => false,
        // Every other 5xx: a failure on the server's side, which can pass.
        // 507 Insufficient Storage is one: RFC 4918 calls it temporary.
        500..=599 => true,
        // Every other 4xx is the request's fault; any other status is no
        // server error at all.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `reqwest::Error` from `ClientBuilder::build`: a header value with a
    /// newline is rejected there, not when it is set.
    fn client_build_error() -> reqwest::Error {
        reqwest::Client::builder()
            .user_agent("bad\nagent")
            .build()
            .expect_err("a newline in a header value fails the build")
    }

    #[test]
    fn test_genai_error_poll_timeout_display_is_its_message() {
        let error = GenaiError::PollTimeout {
            timeout: std::time::Duration::from_secs(60),
            message: "Timeout waiting for file 'files/x' to become active".to_string(),
        };
        assert_eq!(
            error.to_string(),
            "Timeout waiting for file 'files/x' to become active"
        );
    }

    #[test]
    fn test_genai_error_processing_failed_display() {
        let error = GenaiError::ProcessingFailed("File 'files/x' failed processing".to_string());
        assert_eq!(
            error.to_string(),
            "Processing failed: File 'files/x' failed processing"
        );
    }

    #[test]
    fn test_genai_error_api_display() {
        let error = GenaiError::Api {
            status_code: 429,
            message: "Rate limited".to_string(),
            request_id: Some("req-123".to_string()),
            retry_after: None,
        };
        let display = format!("{error}");
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
        let display = format!("{error}");
        assert!(display.contains("500"));
        assert!(display.contains("Internal error"));
    }

    #[test]
    fn test_genai_error_internal_display() {
        let error = GenaiError::Internal("Max function loops exceeded".to_string());
        let display = format!("{error}");
        assert!(display.contains("Internal client error"));
        assert!(display.contains("Max function loops exceeded"));
    }

    #[test]
    fn test_genai_error_invalid_input_display() {
        let error = GenaiError::InvalidInput("Missing model or agent".to_string());
        let display = format!("{error}");
        assert!(display.contains("Invalid input"));
        assert!(display.contains("Missing model or agent"));
    }

    #[test]
    fn test_genai_error_debug_format() {
        let error = GenaiError::Api {
            status_code: 400,
            message: "Bad request".to_string(),
            request_id: Some("req-456".to_string()),
            retry_after: None,
        };
        let debug = format!("{error:?}");
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
            let display = format!("{error}");
            assert!(
                display.contains(&code.to_string()),
                "Expected {code} in display: {display}"
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
        let display = format!("{error}");
        assert!(display.contains("500"));
        // Should still display properly even with empty message
        assert!(display.contains("API error"));
    }

    #[test]
    fn test_genai_error_malformed_response_display() {
        let error = GenaiError::MalformedResponse(
            "Function call 'get_weather' is missing required call_id field".to_string(),
        );
        let display = format!("{error}");
        assert!(display.contains("Malformed API response"));
        assert!(display.contains("call_id"));
    }

    #[test]
    fn test_genai_error_malformed_response_stream() {
        let error = GenaiError::MalformedResponse(
            "an SSE event passed 67108864 bytes without ending".to_string(),
        );
        let display = format!("{error}");
        assert!(display.contains("Malformed API response"));
        assert!(display.contains("SSE event"));
    }

    #[test]
    fn test_genai_error_timeout_display() {
        let error = GenaiError::Timeout(std::time::Duration::from_secs(30));
        let display = format!("{error}");
        assert!(display.contains("Request timed out"));
        assert!(display.contains("30s"));
    }

    #[test]
    fn test_genai_error_timeout_debug() {
        let error = GenaiError::Timeout(std::time::Duration::from_millis(500));
        let debug = format!("{error:?}");
        assert!(debug.contains("Timeout"));
        assert!(debug.contains("500ms"));
    }

    #[test]
    fn test_genai_error_client_build_display_leaves_the_source_to_the_chain() {
        use std::error::Error as _;
        let error = GenaiError::ClientBuild(client_build_error());
        let source = error.source().expect("ClientBuild keeps the reqwest error");
        let display = error.to_string();
        assert_eq!(display, "Failed to build HTTP client");
        assert!(
            !display.contains(&source.to_string()),
            "{display} repeats its source"
        );
    }

    #[test]
    fn test_genai_error_client_build_debug() {
        let error = GenaiError::ClientBuild(client_build_error());
        let debug = format!("{error:?}");
        assert!(debug.contains("ClientBuild"));
    }

    #[test]
    fn test_client_build_preserves_source() {
        use std::error::Error as _;
        let error = GenaiError::ClientBuild(client_build_error());
        let source = error.source().expect("ClientBuild keeps the reqwest error");
        assert!(source.downcast_ref::<reqwest::Error>().is_some());
    }

    // =============================================================================
    // Every variant: Display and source (D-049)
    // =============================================================================

    /// A `reqwest::Error` from building a request: a scheme-less URL is a
    /// builder error, `Display`ed as `builder error`.
    fn request_build_error() -> reqwest::Error {
        reqwest::Client::new()
            .get("localhost:8080/v1beta/interactions")
            .build()
            .expect_err("a URL without a scheme fails the build")
    }

    /// One value of every variant, each holding a source where it can.
    /// [`variant_name`] names each, and
    /// `test_genai_error_every_variant_is_built` fails on a variant of the
    /// enum left out here.
    fn one_of_each_variant() -> Vec<GenaiError> {
        let duration = std::time::Duration::from_secs(30);
        vec![
            GenaiError::Http(request_build_error()),
            api_error(503),
            GenaiError::Internal("a body that would not serialize".to_string()),
            GenaiError::InvalidInput("no input".to_string()),
            GenaiError::MalformedResponse("not an interaction".to_string()),
            GenaiError::StreamInterrupted {
                message: "the connection failed mid-body".to_string(),
                source: Some(request_build_error()),
            },
            GenaiError::Timeout(duration),
            GenaiError::PollTimeout {
                timeout: duration,
                message: "waited for files/x".to_string(),
            },
            GenaiError::ProcessingFailed("files/x failed processing".to_string()),
            GenaiError::Stream {
                message: "quota exhausted mid-stream".to_string(),
                code: None,
            },
            GenaiError::ClientBuild(client_build_error()),
        ]
    }

    /// The variant's name, by a match with no wildcard arm: a variant added
    /// to `GenaiError` fails to compile here until it is named.
    fn variant_name(error: &GenaiError) -> &'static str {
        match error {
            GenaiError::Http(_) => "Http",
            GenaiError::Api { .. } => "Api",
            GenaiError::Internal(_) => "Internal",
            GenaiError::InvalidInput(_) => "InvalidInput",
            GenaiError::MalformedResponse(_) => "MalformedResponse",
            GenaiError::StreamInterrupted { .. } => "StreamInterrupted",
            GenaiError::Timeout(_) => "Timeout",
            GenaiError::PollTimeout { .. } => "PollTimeout",
            GenaiError::ProcessingFailed(_) => "ProcessingFailed",
            GenaiError::Stream { .. } => "Stream",
            GenaiError::ClientBuild(_) => "ClientBuild",
        }
    }

    /// The variants as this file declares them, each with whether it holds
    /// a source, as thiserror reads one: a field marked `#[source]` or
    /// `#[from]`, or one named `source`.
    fn declared_variants() -> std::collections::BTreeMap<String, bool> {
        let file = syn::parse_file(include_str!("errors.rs")).expect("errors.rs parses");
        let genai_error = file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Enum(e) if e.ident == "GenaiError" => Some(e),
                _ => None,
            })
            .expect("errors.rs declares GenaiError");
        genai_error
            .variants
            .iter()
            .map(|variant| {
                let has_source = variant.fields.iter().any(|field| {
                    field.ident.as_ref().is_some_and(|name| name == "source")
                        || field.attrs.iter().any(|attr| {
                            attr.path().is_ident("source") || attr.path().is_ident("from")
                        })
                });
                (variant.ident.to_string(), has_source)
            })
            .collect()
    }

    /// [`one_of_each_variant`] builds every variant of the enum, each one
    /// that can hold a source with one, so the check below reads them all.
    #[test]
    fn test_genai_error_every_variant_is_built() {
        use std::error::Error as _;
        let declared = declared_variants();
        let built: std::collections::BTreeMap<String, bool> = one_of_each_variant()
            .iter()
            .map(|error| (variant_name(error).to_string(), error.source().is_some()))
            .collect();
        assert_eq!(
            built, declared,
            "one_of_each_variant must build every GenaiError variant, each that can \
             hold a source with one"
        );
        for name in ["Http", "StreamInterrupted", "ClientBuild"] {
            assert_eq!(declared.get(name), Some(&true), "{name} holds a source");
        }
    }

    /// No variant's `Display` repeats its source's text (D-049): the source
    /// is what `source()` returns, so a reporter that walks the chain
    /// (anyhow's `{:#}`, eyre) prints it once, not twice.
    #[test]
    fn test_genai_error_display_never_repeats_its_source() {
        use std::error::Error as _;
        for error in one_of_each_variant() {
            let Some(source) = error.source() else {
                continue;
            };
            let (display, source) = (error.to_string(), source.to_string());
            assert!(!source.is_empty(), "{}", variant_name(&error));
            assert!(
                !display.contains(&source),
                "{}'s Display {display:?} repeats its source {source:?}",
                variant_name(&error)
            );
        }
    }

    /// `ErrorChain` writes the source below a variant whose `Display` leaves
    /// it out, and does not repeat one whose text is already there.
    #[test]
    fn test_error_chain_writes_each_source_once() {
        let error = GenaiError::Http(request_build_error());
        let transport = request_build_error().to_string();
        let chain = ErrorChain(&error).to_string();
        assert!(
            chain.starts_with(&format!("HTTP request error: {transport}")),
            "{chain}"
        );
        assert_eq!(chain.matches(&transport).count(), 1, "{chain}");

        /// An error whose `Display` repeats its source, as `Http`'s did.
        #[derive(Debug)]
        struct Repeats(std::io::Error);
        impl std::fmt::Display for Repeats {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "outer: {}", self.0)
            }
        }
        impl std::error::Error for Repeats {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let repeats = Repeats(std::io::Error::other("refused"));
        assert_eq!(ErrorChain(&repeats).to_string(), "outer: refused");
        let alone = GenaiError::InvalidInput("no input".to_string());
        assert_eq!(ErrorChain(&alone).to_string(), alone.to_string());
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

    fn api_error(status_code: u16) -> GenaiError {
        GenaiError::Api {
            status_code,
            message: "error".to_string(),
            request_id: None,
            retry_after: None,
        }
    }

    /// The server stopped waiting before the request arrived whole, so it
    /// acted on nothing; google-genai retries a 408 too.
    #[test]
    fn test_is_retryable_request_timeout_408() {
        assert!(api_error(408).is_retryable());
    }

    #[test]
    fn test_is_retryable_server_errors_5xx() {
        // google-genai's four, 507 (temporary per RFC 4918), and codes no RFC
        // registers: a proxy's 509, 520-529 and 598-599.
        for status_code in [500, 502, 503, 504, 507, 509, 520, 524, 529, 598, 599] {
            assert!(
                api_error(status_code).is_retryable(),
                "{status_code} errors should be retryable"
            );
        }
    }

    /// Each names a condition the same request meets on every attempt.
    #[test]
    fn test_is_retryable_5xx_a_retry_cannot_fix() {
        for (status_code, why) in [
            (501, "Not Implemented: the method or feature is unsupported"),
            (
                505,
                "HTTP Version Not Supported: the same version every time",
            ),
            (506, "Variant Also Negotiates: a server misconfiguration"),
            (508, "Loop Detected: the same request loops again"),
            (510, "Not Extended: the request lacks a required extension"),
            (511, "Network Authentication Required: a captive portal"),
        ] {
            assert!(
                !api_error(status_code).is_retryable(),
                "{status_code} {why}"
            );
        }
    }

    #[test]
    fn test_is_retryable_client_errors_4xx_not_retryable() {
        // Client errors (except 408 and 429) should NOT be retryable
        for status_code in [400, 401, 403, 404, 409, 413, 422, 425, 499] {
            assert!(
                !api_error(status_code).is_retryable(),
                "{status_code} errors should NOT be retryable"
            );
        }
    }

    /// The whole table: 408, 429, and every 5xx but the six above. A status
    /// past 599 is no server error, whatever a nonstandard server means by
    /// it.
    #[test]
    fn test_is_retryable_status_table() {
        const PERMANENT_5XX: [u16; 6] = [501, 505, 506, 508, 510, 511];
        for status_code in 0..=999u16 {
            let expected = matches!(status_code, 408 | 429)
                || ((500..=599).contains(&status_code) && !PERMANENT_5XX.contains(&status_code));
            assert_eq!(
                api_error(status_code).is_retryable(),
                expected,
                "{status_code}"
            );
        }
    }

    #[test]
    fn test_is_retryable_http_builder_error_is_permanent() {
        // What a scheme-less base URL produces at send time.
        let err = reqwest::Client::new()
            .get("localhost:8080/v1beta/interactions")
            .build()
            .unwrap_err();
        assert!(err.is_builder());
        assert!(!GenaiError::Http(err).is_retryable());
    }

    #[tokio::test]
    async fn test_is_retryable_http_connect_error_is_transient() {
        // Nothing listens on port 9 of localhost.
        let err = reqwest::Client::new()
            .get("http://127.0.0.1:9/")
            .send()
            .await
            .unwrap_err();
        assert!(err.is_connect(), "{err:?}");
        assert!(GenaiError::Http(err).is_retryable());
    }

    #[test]
    fn test_is_retryable_timeout() {
        let error = GenaiError::Timeout(std::time::Duration::from_secs(30));
        assert!(error.is_retryable(), "Timeout errors should be retryable");
    }

    #[test]
    fn test_is_retryable_poll_timeout_not_retryable() {
        // The resource may still be processing; re-sending an import would
        // make a second document.
        let error = GenaiError::PollTimeout {
            timeout: std::time::Duration::from_secs(60),
            message: "waited".to_string(),
        };
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_is_retryable_processing_failed_not_retryable() {
        let error = GenaiError::ProcessingFailed("failed to index".to_string());
        assert!(!error.is_retryable());
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
        let error = GenaiError::ClientBuild(client_build_error());
        assert!(
            !error.is_retryable(),
            "ClientBuild errors should NOT be retryable"
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
