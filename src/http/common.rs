//! Shared request plumbing for every endpoint module: URL helpers, the
//! standard headers, and the send → check → read sequence.

use super::context::HttpContext;
use super::error_helpers::check_response_wire;
use crate::errors::GenaiError;
use crate::wire::WireEvent;

/// The production API host. Overridable per client via
/// [`ClientBuilder::with_base_url`](crate::ClientBuilder::with_base_url).
pub(crate) const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

/// The API version path segment shared by every resource URL.
pub(crate) const API_VERSION: &str = "v1beta";

/// Header name for API key authentication.
///
/// A header rather than a `key=` query parameter keeps the key out of
/// server logs, proxy logs, and error messages that echo the URL.
pub const API_KEY_HEADER: &str = "X-Goog-Api-Key";

/// Header name for the Interactions API wire revision.
pub(crate) const API_REVISION_HEADER: &str = "Api-Revision";

/// The Interactions API revision this crate implements.
///
/// Revision `2026-05-20` is the steps response model (`steps: [Step...]`),
/// the `interaction.created` / `step.*` / `interaction.completed` SSE
/// lifecycle, lowercase enum wire formats, and the `tool_choice` union. The
/// server currently serves this protocol whatever the header says; it is
/// still sent so a future server that honors it gets the shape this crate
/// parses.
pub(crate) const API_REVISION: &str = "2026-05-20";

/// Passed as the `body` of a request that has none.
pub(crate) const NO_BODY: Option<&()> = None;

/// Starts a request with the standard headers (API key + `Api-Revision`).
pub(crate) fn api_request(
    ctx: &HttpContext,
    method: reqwest::Method,
    url: &str,
) -> reqwest::RequestBuilder {
    ctx.http_client
        .request(method, url)
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION)
}

/// Sends `builder`, reports the status to wire inspectors, and maps a
/// non-success status to [`GenaiError::Api`].
pub(crate) async fn send_checked(
    ctx: &HttpContext,
    request_id: u64,
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response, GenaiError> {
    let response = builder.send().await?;
    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });
    check_response_wire(response, ctx, request_id).await
}

/// Sends a JSON request with the standard headers, emits wire events,
/// checks the status, and returns the response body text.
///
/// The body is serialized once for the wire; inspectors get a separate
/// `Value` only when any are installed.
pub(crate) async fn send_and_read<B: serde::Serialize>(
    ctx: &HttpContext,
    method: reqwest::Method,
    url: &str,
    body: Option<&B>,
) -> Result<String, GenaiError> {
    let request_id = ctx.next_request_id();
    let wire_body = body.and_then(|b| ctx.serialize_wire_body(b));
    ctx.emit_request(request_id, method.as_str(), url, wire_body.as_ref());

    let mut builder = api_request(ctx, method, url);
    if let Some(body) = body {
        let bytes = serde_json::to_vec(body)
            .map_err(|e| GenaiError::Internal(format!("Failed to serialize request body: {e}")))?;
        builder = builder
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes);
    }

    let response = send_checked(ctx, request_id, builder).await?;
    let response_text = response.text().await?;
    ctx.emit_response_body(request_id, &response_text);
    Ok(response_text)
}

/// Rejects an empty or dot-segment resource ID before a URL is built
/// from it.
///
/// An empty ID turns an item URL into the *collection* URL, so
/// `delete_x("")` would target the collection. A dot segment (`.`, `..`,
/// or a percent-encoded spelling, matched case-insensitively) is popped by
/// the URL parser under reqwest and addresses a different endpoint. Neither
/// is ever a real ID.
pub(crate) fn require_id(id: &str, what: &str) -> Result<(), GenaiError> {
    if id.is_empty() {
        return Err(GenaiError::InvalidInput(format!(
            "{what} ID must not be empty (an empty ID would address the collection or API-root URL)"
        )));
    }
    // The longest spelling is six bytes, so real IDs skip the comparisons.
    if id.len() <= 6
        && [".", "%2e", "..", ".%2e", "%2e.", "%2e%2e"]
            .iter()
            .any(|dot| id.eq_ignore_ascii_case(dot))
    {
        return Err(GenaiError::InvalidInput(format!(
            "{what} ID must not be a dot segment (URL parsing would pop \
             the preceding path segment and address a different endpoint)"
        )));
    }
    Ok(())
}

/// Percent-encodes a resource ID for use as a single URL path segment, so
/// an ID containing `/`, `?` or `#` cannot rewrite the request path.
///
/// Dot segments pass through unencoded (`.` is unreserved), which is why
/// every caller runs [`require_id`] first.
pub(crate) fn path_segment(id: &str) -> std::borrow::Cow<'_, str> {
    urlencoding::encode(id)
}

/// A caller-supplied MIME type as a header value, for an upload's
/// `Content-Type` or `X-Goog-Upload-Header-Content-Type`.
///
/// Fails up front: `RequestBuilder::header` would otherwise defer the
/// rejection to `.send()` as a `GenaiError::Http`, which `is_retryable()`
/// reports as transient. MIME *syntax* is not checked.
pub(crate) fn mime_type_header(
    mime_type: &str,
) -> Result<reqwest::header::HeaderValue, GenaiError> {
    reqwest::header::HeaderValue::try_from(mime_type).map_err(|_| {
        GenaiError::InvalidInput(format!(
            "MIME type {mime_type:?} cannot be sent as a header value"
        ))
    })
}

/// Appends percent-encoded query params to `url`, skipping `None` values.
///
/// Joins with `&` when `url` already carries a query string.
pub(crate) fn with_query(mut url: String, params: &[(&str, Option<&str>)]) -> String {
    let mut pairs = params
        .iter()
        .filter_map(|(key, value)| value.map(|v| (key, v)))
        .map(|(key, value)| {
            format!(
                "{}={}",
                urlencoding::encode(key),
                urlencoding::encode(value)
            )
        })
        .peekable();
    if pairs.peek().is_some() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&pairs.collect::<Vec<_>>().join("&"));
    }
    url
}

/// Appends the Interactions-family `page_size` / `page_token` params.
pub(crate) fn with_paging(url: String, page_size: Option<u32>, page_token: Option<&str>) -> String {
    let page_size = page_size.map(|size| size.to_string());
    with_query(
        url,
        &[
            ("page_size", page_size.as_deref()),
            ("page_token", page_token),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unheaderable_mime_type_is_rejected_as_invalid_input() {
        // Must be `InvalidInput`, not the deferred builder error `.header()`
        // would surface at `.send()`.
        let err = mime_type_header("text/plain\nX-Injected: 1").unwrap_err();
        assert!(
            matches!(err, GenaiError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );
        // Names the offending value, so the error is actionable.
        assert!(err.to_string().contains("X-Injected"), "got {err}");
    }

    #[test]
    fn valid_mime_types_pass_through_unchanged() {
        assert_eq!(
            mime_type_header("text/plain").unwrap().as_bytes(),
            b"text/plain"
        );
        // Syntax is deliberately not checked; the API rejects these.
        assert!(mime_type_header("nonsense").is_ok());
        assert!(mime_type_header("text/").is_ok());
    }

    #[test]
    fn with_paging_no_params_leaves_url_unchanged() {
        assert_eq!(
            with_paging("https://x/v1beta/things".into(), None, None),
            "https://x/v1beta/things"
        );
    }

    #[test]
    fn with_paging_page_size_only() {
        assert_eq!(
            with_paging("https://x/v1beta/things".into(), Some(10), None),
            "https://x/v1beta/things?page_size=10"
        );
    }

    #[test]
    fn with_paging_percent_encodes_token() {
        // A token with reserved characters must arrive percent-encoded, or
        // the server sees a truncated token and silently restarts paging.
        assert_eq!(
            with_paging("https://x/v1beta/things".into(), Some(5), Some("a/b&c=d")),
            "https://x/v1beta/things?page_size=5&page_token=a%2Fb%26c%3Dd"
        );
    }

    #[test]
    fn with_query_encodes_values_and_skips_none() {
        assert_eq!(
            with_query(
                "https://x/v1beta/things".into(),
                &[("parent", Some("projects/a b")), ("unset", None)]
            ),
            "https://x/v1beta/things?parent=projects%2Fa%20b"
        );
    }

    #[test]
    fn with_query_joins_an_existing_query_string() {
        let url = with_paging("https://x/v1beta/things".into(), Some(5), None);
        assert_eq!(
            with_query(url, &[("parent", Some("p"))]),
            "https://x/v1beta/things?page_size=5&parent=p"
        );
    }

    #[test]
    fn test_require_id_rejects_empty_and_dot_segments() {
        assert!(require_id("", "trigger").is_err());
        assert!(require_id("t-1", "trigger").is_ok());
        // The URL parser normalizes the percent-encoded forms
        // case-insensitively too, so all of these would pop the preceding
        // path segment.
        for hostile in [".", "..", "%2e", "%2E", ".%2e", "%2e.", "%2E%2E", "%2e%2e"] {
            assert!(
                require_id(hostile, "trigger").is_err(),
                "dot-segment spelling {hostile:?} must be rejected"
            );
        }
        // Dots inside an ID are not dot segments.
        assert!(require_id("a.b", "trigger").is_ok());
    }

    #[test]
    fn test_path_segment_keeps_ids_inside_one_segment() {
        assert_eq!(path_segment("a.b"), "a.b");
        assert_eq!(path_segment("a/b?c"), "a%2Fb%3Fc");

        // Every ID `require_id` accepts survives the parser reqwest uses
        // with its path structure intact.
        for hostile in ["%2e%2e%2f", "a/b", "x?alt=media", "x#frag", "..a"] {
            require_id(hostile, "thing").unwrap();
            let url = format!("https://h.test/v1beta/things/{}", path_segment(hostile));
            let parsed = reqwest::Url::parse(&url).expect("built URL must parse");
            assert!(
                parsed.path().starts_with("/v1beta/things/")
                    && parsed.path().len() > "/v1beta/things/".len(),
                "ID {hostile:?} must stay inside the item segment; \
                 parser saw path {:?}",
                parsed.path()
            );
            assert!(
                parsed.query().is_none() && parsed.fragment().is_none(),
                "ID {hostile:?} must not split a query or fragment"
            );
        }
    }

    #[test]
    fn test_api_key_header_constant() {
        assert_eq!(API_KEY_HEADER, "X-Goog-Api-Key");
    }
}
