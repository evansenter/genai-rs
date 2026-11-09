//! Error handling utilities for HTTP responses and error context formatting.

use super::context::HttpContext;
use crate::errors::GenaiError;
use crate::wire::{WireEvent, truncate_utf8};
use reqwest::Response;
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Maximum characters to include from a raw body in error messages.
const ERROR_BODY_PREVIEW_LENGTH: usize = 200;

/// Returns `response` if its status is a success; otherwise reads the error
/// body, surfaces it to wire inspectors as [`WireEvent::ErrorBody`], and
/// builds a [`GenaiError::Api`].
///
/// # Errors
///
/// Returns [`GenaiError::Api`] on a non-success status.
pub async fn check_response_wire(
    response: Response,
    ctx: &HttpContext,
    request_id: u64,
) -> Result<Response, GenaiError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let parts = read_error_parts(response).await;
    if ctx.has_inspectors() {
        ctx.emit(WireEvent::ErrorBody {
            id: request_id,
            status: parts.status_code,
            body: parts.body.clone(),
        });
    }
    Err(parts.into_api_error())
}

/// Google's request ID header, for correlating with server logs or support.
const REQUEST_ID_HEADER: &str = "x-goog-request-id";

/// Standard HTTP header: seconds, or an HTTP date, to wait before retrying.
const RETRY_AFTER_HEADER: &str = "retry-after";

/// Raw pieces of an error response, extracted before the body is consumed.
struct ErrorParts {
    status_code: u16,
    request_id: Option<String>,
    retry_after: Option<std::time::Duration>,
    /// Full (untruncated) error body.
    body: String,
}

impl ErrorParts {
    fn into_api_error(self) -> GenaiError {
        GenaiError::Api {
            status_code: self.status_code,
            message: api_error_message(&self.body),
            request_id: self.request_id,
            retry_after: self.retry_after,
        }
    }
}

/// Google's JSON error envelope, `{"error": {...}}`.
///
/// The Interactions API sends a string `code` (`"invalid_request"`); the
/// standard Google endpoints send the HTTP status as an integer `code` plus
/// a string `status` (`"INVALID_ARGUMENT"`).
#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: Option<String>,
    code: Option<serde_json::Value>,
    status: Option<String>,
}

/// The human-readable message for an error body: the envelope's `message`,
/// prefixed with its symbolic status or code, or a truncated preview of the
/// raw body when it is not an envelope (an HTML proxy page, say).
fn api_error_message(body: &str) -> String {
    let Ok(ErrorEnvelope { error }) = serde_json::from_str::<ErrorEnvelope>(body) else {
        return truncate_utf8(body, ERROR_BODY_PREVIEW_LENGTH).into_owned();
    };
    let Some(message) = error.message else {
        return truncate_utf8(body, ERROR_BODY_PREVIEW_LENGTH).into_owned();
    };
    // The integer `code` only repeats the HTTP status, so it is not a label.
    let string_code = match error.code {
        Some(serde_json::Value::String(code)) => Some(code),
        _ => None,
    };
    let label = error.status.or(string_code);
    match label {
        Some(label) if !label.is_empty() => format!("{label}: {message}"),
        _ => message,
    }
}

/// Reads the status code, diagnostic headers, and full body of an error response.
async fn read_error_parts(response: Response) -> ErrorParts {
    let status_code = response.status().as_u16();

    let request_id = response
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let retry_after = response
        .headers()
        .get(RETRY_AFTER_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_retry_after);

    let body = response
        .text()
        .await
        .unwrap_or_else(|e| format!("Failed to read error body: {e}"));

    ErrorParts {
        status_code,
        request_id,
        retry_after,
        body,
    }
}

/// Parses the Retry-After header value into a Duration.
///
/// Supports both formats of the HTTP spec: integer seconds (`"120"`) and an
/// HTTP date (`"Wed, 21 Oct 2015 07:28:00 GMT"`), the latter as the time
/// remaining until then (zero if already past).
fn parse_retry_after(value: &str) -> Option<std::time::Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(std::time::Duration::from_secs(seconds));
    }

    // RFC 2822 requires a numeric offset, so convert "GMT" → "+0000".
    let normalized = value.replace(" GMT", " +0000");
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(&normalized) {
        let now = chrono::Utc::now();
        let target = date.with_timezone(&chrono::Utc);
        if target > now {
            return (target - now).to_std().ok();
        }
        return Some(std::time::Duration::ZERO);
    }

    None
}

/// Formats a JSON parse error with a preview of the raw JSON.
pub fn format_json_parse_error(json_str: &str, error: serde_json::Error) -> String {
    let preview = truncate_utf8(json_str, ERROR_BODY_PREVIEW_LENGTH);
    format!("JSON parse error: {error} | Context: {preview}")
}

/// Deserializes a successful response body.
///
/// A body that does not match the expected type is the server's contract
/// breaking, not the caller's input, so it maps to
/// [`GenaiError::MalformedResponse`] with the type and a preview of the JSON.
pub fn deserialize_with_context<T: DeserializeOwned>(
    json_str: &str,
    type_context: &str,
) -> Result<T, GenaiError> {
    serde_json::from_str(json_str).map_err(|e| {
        let preview = truncate_utf8(json_str, ERROR_BODY_PREVIEW_LENGTH);
        GenaiError::MalformedResponse(format!(
            "Failed to parse {type_context}: {e} | JSON: {preview}"
        ))
    })
}

/// The error for a successful streaming response whose body is not SSE (a
/// captive portal's HTML page, a plain JSON interaction): a
/// [`GenaiError::MalformedResponse`] naming the content type, with a preview
/// of the body. Not a cut-off stream: no stream ever started.
pub(crate) fn not_an_event_stream(content_type: &str, body: &str) -> GenaiError {
    let preview = truncate_utf8(body, ERROR_BODY_PREVIEW_LENGTH);
    GenaiError::MalformedResponse(format!(
        "expected an SSE stream (text/event-stream), got {content_type}: {preview}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json_parse_error() {
        let json = r#"{"invalid": }"#;
        let err = serde_json::from_str::<serde_json::Value>(json).unwrap_err();
        let result = format_json_parse_error(json, err);

        assert!(result.contains("JSON parse error"));
        assert!(result.contains("Context:"));
        assert!(result.contains(r#"{"invalid": }"#));
    }

    #[test]
    fn test_deserialize_with_context_success() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct TestData {
            id: String,
            value: i32,
        }

        let json = r#"{"id": "test123", "value": 42}"#;
        let result: Result<TestData, _> = deserialize_with_context(json, "test data");
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.id, "test123");
        assert_eq!(data.value, 42);
    }

    #[test]
    fn test_deserialize_with_context_error_includes_context() {
        #[derive(serde::Deserialize, Debug)]
        #[expect(dead_code, reason = "only parsed; the field is never read")]
        struct TestData {
            required_field: String,
        }

        let json = r#"{"wrong_field": "value"}"#;
        let result: Result<TestData, _> = deserialize_with_context(json, "InteractionResponse");
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_str = err.to_string();

        // Should include the type context
        assert!(
            err_str.contains("InteractionResponse"),
            "Error should mention the type: {err_str}"
        );
        // Should include JSON preview
        assert!(
            err_str.contains("wrong_field"),
            "Error should include JSON preview: {err_str}"
        );
    }

    #[test]
    fn test_deserialize_with_context_truncates_long_json() {
        #[derive(serde::Deserialize, Debug)]
        #[expect(dead_code, reason = "only parsed; the field is never read")]
        struct TestData {
            id: String,
        }

        // Create JSON longer than 200 chars
        let long_value = "x".repeat(300);
        let json = format!(r#"{{"long_field": "{long_value}"}}"#);

        let result: Result<TestData, _> = deserialize_with_context(&json, "test");
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_str = err.to_string();

        // Should be truncated (contains "...")
        assert!(
            err_str.contains("..."),
            "Long JSON should be truncated: {err_str}"
        );
    }

    #[test]
    fn test_deserialize_with_context_is_malformed_response() {
        let err = deserialize_with_context::<Vec<u32>>("{}", "numbers").unwrap_err();
        assert!(matches!(err, GenaiError::MalformedResponse(_)), "{err:?}");
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_api_error_message_interactions_envelope() {
        let body = r#"{"error":{"message":"'minimal' is not supported","code":"invalid_request"}}"#;
        assert_eq!(
            api_error_message(body),
            "invalid_request: 'minimal' is not supported"
        );
    }

    #[test]
    fn test_api_error_message_standard_envelope() {
        let body = r#"{"error":{"code":400,"message":"Request contains an invalid argument.","status":"INVALID_ARGUMENT","details":[]}}"#;
        assert_eq!(
            api_error_message(body),
            "INVALID_ARGUMENT: Request contains an invalid argument."
        );
    }

    #[test]
    fn test_api_error_message_keeps_long_messages_whole() {
        let long = "x".repeat(500);
        let body = format!(r#"{{"error":{{"message":"{long}","code":404}}}}"#);
        assert_eq!(api_error_message(&body), long);
    }

    #[test]
    fn test_api_error_message_falls_back_to_raw_preview() {
        assert_eq!(api_error_message("Bad Gateway"), "Bad Gateway");
        let html = format!("<html>{}</html>", "y".repeat(400));
        assert!(api_error_message(&html).ends_with("..."));
        // An envelope without a message is not worth unwrapping.
        assert_eq!(
            api_error_message(r#"{"error":{"code":500}}"#),
            r#"{"error":{"code":500}}"#
        );
    }

    // =============================================================================
    // parse_retry_after() Tests
    // =============================================================================

    #[test]
    fn test_parse_retry_after_seconds() {
        // Integer seconds format (most common for rate limiting)
        assert_eq!(
            parse_retry_after("60"),
            Some(std::time::Duration::from_secs(60))
        );
        assert_eq!(
            parse_retry_after("0"),
            Some(std::time::Duration::from_secs(0))
        );
        assert_eq!(
            parse_retry_after("3600"),
            Some(std::time::Duration::from_secs(3600))
        );
    }

    #[test]
    fn test_parse_retry_after_invalid() {
        // Invalid formats should return None
        assert_eq!(parse_retry_after("not-a-number"), None);
        assert_eq!(parse_retry_after(""), None);
        assert_eq!(parse_retry_after("60.5"), None); // Floats not supported
        assert_eq!(parse_retry_after("-1"), None); // Negative not valid for u64
    }

    #[test]
    fn test_parse_retry_after_http_date_future() {
        // HTTP date format in the future (RFC 7231)
        // Note: Day-of-week must be correct! Dec 31, 2030 is a Tuesday.
        let future_date = "Tue, 31 Dec 2030 23:59:59 GMT";
        let result = parse_retry_after(future_date);
        assert!(result.is_some(), "Should parse future HTTP date");
        // Duration should be positive (time until that date)
        assert!(result.unwrap() > std::time::Duration::ZERO);
    }

    #[test]
    fn test_parse_retry_after_http_date_past() {
        // HTTP date in the past should return zero duration (retry immediately)
        // Note: Day-of-week must be correct! Jan 1, 2020 was a Wednesday.
        let past_date = "Wed, 01 Jan 2020 00:00:00 GMT";
        let result = parse_retry_after(past_date);
        assert_eq!(result, Some(std::time::Duration::ZERO));
    }

    #[test]
    fn test_parse_retry_after_invalid_http_date() {
        // Malformed HTTP dates should return None
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015"), None); // Missing time
        assert_eq!(parse_retry_after("2015-10-21T07:28:00Z"), None); // ISO format not supported
    }
}
