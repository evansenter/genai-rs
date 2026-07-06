//! Error handling utilities for HTTP responses and error context formatting.

use super::context::HttpContext;
use crate::errors::GenaiError;
use crate::wire::WireEvent;
use reqwest::Response;
use serde::de::DeserializeOwned;

/// Maximum characters to include from error body in context messages
const ERROR_BODY_PREVIEW_LENGTH: usize = 200;

/// Checks if an HTTP response is successful, returning it if so or an error otherwise.
///
/// This helper consolidates the common pattern of checking response status and
/// extracting error details on failure.
///
/// # Errors
///
/// Returns an error with status code and body preview on non-success status.
pub async fn check_response(response: Response) -> Result<Response, GenaiError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(read_error_with_context(response).await)
    }
}

/// Like [`check_response`], but also surfaces error response bodies to the
/// installed wire inspectors as [`WireEvent::ErrorBody`] before constructing
/// the error.
///
/// # Errors
///
/// Returns an error with status code and body preview on non-success status.
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

/// Google's request ID header name.
///
/// This is a standard Google Cloud API header that uniquely identifies each request.
/// The value can be used when contacting Google support or correlating with server logs.
/// See: <https://cloud.google.com/apis/docs/system-parameters>
const REQUEST_ID_HEADER: &str = "x-goog-request-id";

/// Standard HTTP header indicating how long to wait before retrying.
///
/// Can contain either:
/// - Seconds as integer (e.g., "120")
/// - HTTP date (e.g., "Wed, 21 Oct 2015 07:28:00 GMT")
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
    /// Builds the structured API error, truncating the body for the message.
    fn into_api_error(self) -> GenaiError {
        GenaiError::Api {
            status_code: self.status_code,
            message: truncate_for_context(&self.body, ERROR_BODY_PREVIEW_LENGTH),
            request_id: self.request_id,
            retry_after: self.retry_after,
        }
    }
}

/// Reads the status code, diagnostic headers, and full body of an error response.
async fn read_error_parts(response: Response) -> ErrorParts {
    let status_code = response.status().as_u16();

    // Extract request ID from response headers before consuming the body
    let request_id = response
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    // Extract Retry-After header (primarily for 429 rate limit responses)
    let retry_after = response
        .headers()
        .get(RETRY_AFTER_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_retry_after);

    let body = response
        .text()
        .await
        .unwrap_or_else(|e| format!("Failed to read error body: {}", e));

    ErrorParts {
        status_code,
        request_id,
        retry_after,
        body,
    }
}

/// Reads error response body and creates a detailed GenaiError::Api with context.
///
/// Extracts:
/// - HTTP status code for programmatic error handling
/// - Truncated response body (first 200 chars)
/// - Request ID from `x-goog-request-id` header for debugging/support
/// - Retry delay from `Retry-After` header (typically for 429 errors)
///
/// # Returns
///
/// A structured `GenaiError::Api` with status code, message, optional request ID,
/// and optional retry delay. If body cannot be read, the message describes the read failure.
pub async fn read_error_with_context(response: Response) -> GenaiError {
    read_error_parts(response).await.into_api_error()
}

/// Parses the Retry-After header value into a Duration.
///
/// Supports two formats per HTTP spec:
/// - Seconds as integer (e.g., "120" → 120 seconds)
/// - HTTP date (e.g., "Wed, 21 Oct 2015 07:28:00 GMT" → duration until then)
fn parse_retry_after(value: &str) -> Option<std::time::Duration> {
    // Try parsing as seconds first (most common for rate limiting)
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(std::time::Duration::from_secs(seconds));
    }

    // Try parsing as HTTP date using chrono
    // HTTP date format (RFC 7231): "Wed, 21 Oct 2015 07:28:00 GMT"
    // RFC 2822 (email) format requires numeric offset, so convert "GMT" → "+0000"
    let normalized = value.replace(" GMT", " +0000");
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(&normalized) {
        let now = chrono::Utc::now();
        let target = date.with_timezone(&chrono::Utc);
        if target > now {
            let duration = target - now;
            return duration.to_std().ok();
        }
        // If target is in the past, return zero duration (retry immediately)
        return Some(std::time::Duration::ZERO);
    }

    // Unable to parse - return None
    None
}

/// Formats JSON parsing context by including a preview of the raw JSON.
///
/// # Arguments
///
/// * `json_str` - The JSON string that failed to parse
/// * `error` - The original serde_json error
///
/// # Returns
///
/// A formatted error message with JSON preview (first 200 chars)
pub fn format_json_parse_error(json_str: &str, error: serde_json::Error) -> String {
    let preview = truncate_for_context(json_str, ERROR_BODY_PREVIEW_LENGTH);
    format!("JSON parse error: {} | Context: {}", error, preview)
}

/// Deserializes JSON with context-rich error messages.
///
/// This function wraps `serde_json::from_str` and converts deserialization errors
/// to `GenaiError::Json` with additional context about what type failed to parse
/// and a preview of the JSON that caused the error.
///
/// # Arguments
///
/// * `json_str` - The JSON string to deserialize
/// * `type_context` - A human-readable description of what's being deserialized
///   (e.g., "InteractionResponse", "create interaction response")
///
/// # Returns
///
/// The deserialized value on success, or a context-rich `GenaiError` on failure.
///
/// # Example
///
/// ```ignore
/// // This module is pub(crate) - example shown for documentation only
/// use genai_rs::http::error_helpers::deserialize_with_context;
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Debug)]
/// struct Response { id: String }
///
/// let json = r#"{"id": "test123"}"#;
/// let result: Result<Response, _> = deserialize_with_context(json, "API response");
/// assert!(result.is_ok());
///
/// let bad_json = r#"{"missing_id": true}"#;
/// let result: Result<Response, _> = deserialize_with_context(bad_json, "API response");
/// let err = result.unwrap_err();
/// assert!(err.to_string().contains("API response"));
/// ```
pub fn deserialize_with_context<T: DeserializeOwned>(
    json_str: &str,
    type_context: &str,
) -> Result<T, GenaiError> {
    serde_json::from_str(json_str).map_err(|e| {
        let preview = truncate_for_context(json_str, ERROR_BODY_PREVIEW_LENGTH);
        let message = format!(
            "Failed to parse {}: {} | JSON: {}",
            type_context, e, preview
        );
        GenaiError::Json(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )))
    })
}

/// Truncates a string to specified length, adding "..." if truncated.
///
/// Uses character-boundary-aware slicing to prevent panics on multi-byte UTF-8 characters.
fn truncate_for_context(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find a valid UTF-8 character boundary at or before max_len
        // We need to ensure the character END position is <= max_len
        let truncate_at = s
            .char_indices()
            .take_while(|(i, c)| i + c.len_utf8() <= max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..truncate_at])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_for_context_short_string() {
        let result = truncate_for_context("Short", 100);
        assert_eq!(result, "Short");
    }

    #[test]
    fn test_truncate_for_context_long_string() {
        let long_str = "a".repeat(300);
        let result = truncate_for_context(&long_str, 200);
        assert_eq!(result.len(), 203); // 200 + "..."
        assert!(result.ends_with("..."));
    }

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
    fn test_truncate_for_context_utf8_boundary() {
        // Test with multi-byte UTF-8 characters (emojis are 4 bytes each)
        let emoji_str = "x".repeat(198) + "🎉"; // 198 + 4 = 202 bytes total
        let result = truncate_for_context(&emoji_str, 200);

        // Should truncate before the emoji to avoid splitting it
        // Result should be 198 x's + "..." = 201 bytes
        assert_eq!(result.len(), 201); // 198 + 3 for "..."
        assert!(result.ends_with("..."));
        assert!(result.starts_with("xxx")); // Should start with x's
        assert!(!result.contains("🎉")); // Should not include emoji
        // Verify result is valid UTF-8 (this would panic if we sliced mid-character)
        assert!(result.is_char_boundary(result.len() - 3)); // before "..."
    }

    #[test]
    fn test_truncate_for_context_exactly_at_boundary() {
        // String is exactly max_len bytes
        let exact = "a".repeat(200);
        let result = truncate_for_context(&exact, 200);
        assert_eq!(result, exact); // No truncation needed
    }

    #[test]
    fn test_truncate_for_context_multibyte_characters() {
        // Test with various multi-byte UTF-8: emoji (4 bytes), Chinese (3 bytes), accented (2 bytes)
        let mixed = "Hello 世界 🌍 Café"; // Mix of 1-byte, 2-byte, 3-byte, and 4-byte chars
        let result = truncate_for_context(mixed, 15);

        // Should produce valid UTF-8 without panicking
        assert!(result.ends_with("..."));
        // Verify all characters in result are valid
        for ch in result.chars() {
            assert!(ch.is_ascii() || !ch.is_ascii()); // Tautology, but ensures no panic
        }
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
        #[allow(dead_code)]
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
            "Error should mention the type: {}",
            err_str
        );
        // Should include JSON preview
        assert!(
            err_str.contains("wrong_field"),
            "Error should include JSON preview: {}",
            err_str
        );
    }

    #[test]
    fn test_deserialize_with_context_truncates_long_json() {
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct TestData {
            id: String,
        }

        // Create JSON longer than 200 chars
        let long_value = "x".repeat(300);
        let json = format!(r#"{{"long_field": "{}"}}"#, long_value);

        let result: Result<TestData, _> = deserialize_with_context(&json, "test");
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_str = err.to_string();

        // Should be truncated (contains "...")
        assert!(
            err_str.contains("..."),
            "Long JSON should be truncated: {}",
            err_str
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
