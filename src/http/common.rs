/// Represents the API version to target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiVersion {
    /// V1 Beta API version (current)
    V1Beta,
}

impl ApiVersion {
    const fn as_str(self) -> &'static str {
        match self {
            Self::V1Beta => "v1beta",
        }
    }
}

// --- URL Construction ---
pub(crate) const BASE_URL_PREFIX: &str = "https://generativelanguage.googleapis.com";

/// The API version path segment shared by every resource URL builder.
/// Kept in lockstep with [`ApiVersion::V1Beta`] (pinned by a unit test) so
/// a future version migration is a one-file change.
pub(crate) const API_VERSION: &str = ApiVersion::V1Beta.as_str();

/// Header name for API key authentication.
///
/// Using header-based authentication is more secure than query parameters because:
/// - API keys don't appear in server logs, proxy logs, or browser history
/// - Keys are not leaked in error messages containing URLs
/// - Matches Google Cloud API best practices
pub const API_KEY_HEADER: &str = "X-Goog-Api-Key";

/// Header name for the Interactions API wire revision.
///
/// The Interactions API is date-revisioned: the value of this header selects
/// the wire protocol (response shapes, SSE event lifecycle, enum casing).
pub(crate) const API_REVISION_HEADER: &str = "Api-Revision";

/// The Interactions API revision this crate implements.
///
/// Revision `2026-05-20` introduces the steps response model
/// (`steps: [Step...]` instead of `outputs: [Content...]`), the
/// `interaction.created` / `step.*` / `interaction.completed` SSE lifecycle,
/// lowercase enum wire formats, and the `tool_choice` union.
///
/// Sent on every Interactions API request (create/get/delete/cancel,
/// streaming included). The Files API does not take a revision header
/// (matching google-genai, whose files client is unrevisioned).
pub(crate) const API_REVISION: &str = "2026-05-20";

/// Represents different API endpoints for the Interactions API
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // "Interaction" suffix is intentional for API clarity
pub enum Endpoint<'a> {
    /// Create a new interaction
    CreateInteraction { stream: bool },
    /// Retrieve an interaction by ID
    GetInteraction {
        id: &'a str,
        /// Enable streaming mode for the GET request
        stream: bool,
        /// Resume streaming from this event ID (only valid when stream=true)
        last_event_id: Option<&'a str>,
        /// Include the original input in the response
        include_input: bool,
    },
    /// Delete an interaction by ID
    DeleteInteraction { id: &'a str },
    /// Cancel a background interaction by ID
    CancelInteraction { id: &'a str },
}

impl Endpoint<'_> {
    /// Constructs the URL path for this endpoint
    fn to_path(&self, version: ApiVersion) -> String {
        match self {
            Self::CreateInteraction { .. } => {
                format!("/{}/interactions", version.as_str())
            }
            Self::GetInteraction { id, .. } => {
                format!("/{}/interactions/{}", version.as_str(), path_segment(id))
            }
            Self::DeleteInteraction { id } => {
                format!("/{}/interactions/{}", version.as_str(), path_segment(id))
            }
            Self::CancelInteraction { id } => {
                format!(
                    "/{}/interactions/{}/cancel",
                    version.as_str(),
                    path_segment(id)
                )
            }
        }
    }

    /// Returns whether this endpoint requires SSE parameters
    const fn requires_sse(&self) -> bool {
        match self {
            Self::CreateInteraction { stream } => *stream,
            Self::GetInteraction { stream, .. } => *stream,
            Self::DeleteInteraction { .. } | Self::CancelInteraction { .. } => false,
        }
    }

    /// Returns additional query parameters for this endpoint (if any).
    ///
    /// Note: `last_event_id` is only included when `stream: true`. Passing
    /// `last_event_id` with `stream: false` will silently ignore the value,
    /// since resume is only meaningful for streaming requests.
    fn query_params(&self) -> Option<String> {
        match self {
            Self::GetInteraction {
                stream,
                last_event_id,
                include_input,
                ..
            } => {
                let mut parts = Vec::new();
                if *stream && let Some(event_id) = last_event_id {
                    parts.push(format!("last_event_id={}", urlencoding::encode(event_id)));
                }
                if *include_input {
                    parts.push("include_input=true".to_string());
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("&"))
                }
            }
            _ => None,
        }
    }
}

/// Constructs a URL for a specific endpoint.
///
/// Note: API key authentication is handled via the `X-Goog-Api-Key` header,
/// not as a query parameter. Use [`API_KEY_HEADER`] when making requests.
#[must_use]
pub fn construct_endpoint_url(endpoint: Endpoint) -> String {
    let version = ApiVersion::V1Beta; // Default version for new function
    let path = endpoint.to_path(version);

    // Build query string from SSE requirement and additional params
    let mut query_parts = Vec::new();
    if endpoint.requires_sse() {
        query_parts.push("alt=sse".to_string());
    }
    if let Some(additional) = endpoint.query_params() {
        query_parts.push(additional);
    }

    let query_string = if query_parts.is_empty() {
        String::new()
    } else {
        format!("?{}", query_parts.join("&"))
    };

    format!("{BASE_URL_PREFIX}{path}{query_string}")
}

/// Sends a resource-CRUD request with the standard headers (API key +
/// `Api-Revision`), emits wire events, checks the status, and returns the
/// response body text. Shared by the agents / webhooks / triggers /
/// environments resource modules.
pub(crate) async fn send_and_read(
    ctx: &super::context::HttpContext,
    method: reqwest::Method,
    url: &str,
    body: Option<serde_json::Value>,
) -> Result<String, crate::errors::GenaiError> {
    use crate::wire::WireEvent;

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, method.as_str(), url, body.as_ref());

    let builder = ctx.http_client.request(method, url);

    let mut builder = builder
        .header(API_KEY_HEADER, &ctx.api_key)
        .header(API_REVISION_HEADER, API_REVISION);
    if let Some(body) = &body {
        builder = builder.json(body);
    }

    let response = builder.send().await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    let response = super::error_helpers::check_response_wire(response, ctx, request_id).await?;
    let response_text = response
        .text()
        .await
        .map_err(crate::errors::GenaiError::Http)?;
    ctx.emit_response_body(request_id, &response_text);
    Ok(response_text)
}

/// Serializes a body for the wire, mapping serialization errors to `Internal`.
pub(crate) fn to_body<B: serde::Serialize>(
    body: &B,
) -> Result<serde_json::Value, crate::errors::GenaiError> {
    serde_json::to_value(body).map_err(|e| {
        crate::errors::GenaiError::Internal(format!("Failed to serialize request body: {e}"))
    })
}

/// Rejects an empty resource ID before a URL is built from it.
///
/// An empty ID slips past [`path_segment`] (there is no percent-encoding
/// of nothing) and turns an item URL into the *collection* URL with a
/// trailing slash — so `delete_x("")` would issue a DELETE against the
/// collection path instead of failing locally. The same
/// defense-in-depth argument as the dot-segment arms below.
pub(crate) fn require_id(id: &str, what: &str) -> Result<(), crate::errors::GenaiError> {
    if id.is_empty() {
        return Err(crate::errors::GenaiError::InvalidInput(format!(
            "{what} ID must not be empty (an empty ID would address the collection or API-root URL)"
        )));
    }
    Ok(())
}

/// Rejects dot segments in a full resource name that is interpolated into
/// a URL path verbatim — the `get_file`/`delete_file` exemption from
/// [`path_segment`]. The name's slash is structural, so it cannot be
/// percent-encoded wholesale, but that also means a `.` or `..` segment
/// survives to URL parsing, where dot-segment removal pops the preceding
/// path segment and rewrites the request path — exactly the threat
/// `path_segment`'s explicit dot arms close for single-segment IDs.
pub(crate) fn require_no_dot_segments(
    name: &str,
    what: &str,
) -> Result<(), crate::errors::GenaiError> {
    if name
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(crate::errors::GenaiError::InvalidInput(format!(
            "{what} resource name must not contain dot segments \
             (a `.` or `..` segment would rewrite the request path)"
        )));
    }
    Ok(())
}

/// Percent-encodes a resource ID for use as a single URL path segment, so a
/// hostile or malformed ID (containing `/`, `?`, `#`, `..`, ...) cannot
/// rewrite the request path. IDs from this API are opaque hex/base64url
/// today; this is defense-in-depth applied uniformly across all resource
/// modules.
pub(crate) fn path_segment(id: &str) -> std::borrow::Cow<'_, str> {
    // `.` is unreserved, so the encoder passes "." and ".." through
    // verbatim — and URL parsing applies dot-segment removal, which would
    // pop the preceding path segment and reach a *different* endpoint.
    // Percent-encode those two forms explicitly (valid encodings of the
    // unreserved `.`, and not re-normalized as dot segments).
    match id {
        "." => std::borrow::Cow::Borrowed("%2E"),
        ".." => std::borrow::Cow::Borrowed("%2E%2E"),
        // Cow: the IDs this API issues are opaque hex/base64url, so the
        // borrowed no-escaping arm is essentially always taken — no
        // allocation per URL build. `Cow` implements `Display`, so
        // `format!` call sites are unchanged.
        _ => urlencoding::encode(id),
    }
}

/// Appends `page_size` / `page_token` query params to a URL.
///
/// `url` must not already carry a query string — the shared paging
/// helpers append `?` unconditionally.
pub(crate) fn with_paging(url: String, page_size: Option<u32>, page_token: Option<&str>) -> String {
    with_paging_and(url, page_size, page_token, &[])
}

/// [`with_paging`] plus resource-specific query params (values
/// percent-encoded), for list endpoints with extra filters like the
/// agents resource's `parent`.
///
/// `url` must not already carry a query string — this helper appends `?`
/// unconditionally.
pub(crate) fn with_paging_and(
    mut url: String,
    page_size: Option<u32>,
    page_token: Option<&str>,
    extra: &[(&str, &str)],
) -> String {
    debug_assert!(
        !url.contains('?'),
        "paging helpers require a query-less base URL, got {url}"
    );
    let mut params = Vec::new();
    if let Some(size) = page_size {
        params.push(format!("page_size={size}"));
    }
    if let Some(token) = page_token {
        params.push(format!("page_token={}", urlencoding::encode(token)));
    }
    for (key, value) in extra {
        params.push(format!("{key}={}", urlencoding::encode(value)));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn with_paging_and_percent_encodes_extra_params() {
        // The agents resource's `parent` filter rides the same helper, so
        // its encoding is covered by the same contract.
        assert_eq!(
            with_paging_and(
                "https://x/v1beta/things".into(),
                Some(5),
                None,
                &[("parent", "projects/a b")]
            ),
            "https://x/v1beta/things?page_size=5&parent=projects%2Fa%20b"
        );
    }

    #[test]
    fn test_api_version_as_str() {
        assert_eq!(ApiVersion::V1Beta.as_str(), "v1beta");
        assert_eq!(API_VERSION, ApiVersion::V1Beta.as_str());
    }

    // --- Tests for Endpoint-based URL construction ---

    #[test]
    fn test_endpoint_interaction_ids_are_path_encoded() {
        // A path-metacharacter ID is encoded, not interpolated raw — the
        // interaction endpoints are the hot path for every user of the
        // crate, so pin the property here as well as in the resource
        // modules.
        let url = construct_endpoint_url(Endpoint::GetInteraction {
            id: "a/b?c",
            stream: false,
            last_event_id: None,
            include_input: false,
        });
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/a%2Fb%3Fc"
        );
        let url = construct_endpoint_url(Endpoint::CancelInteraction { id: "a/b?c" });
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/a%2Fb%3Fc/cancel"
        );
    }

    #[test]
    fn test_require_id_rejects_empty() {
        // An empty ID would address the collection URL (trailing slash) —
        // fail locally instead of issuing the request.
        assert!(require_id("", "trigger").is_err());
        assert!(require_id("t-1", "trigger").is_ok());
    }

    #[test]
    fn test_require_no_dot_segments_rejects_traversal() {
        // A dot segment inside a raw-interpolated resource name would be
        // popped by URL dot-segment removal and rewrite the request path.
        assert!(require_no_dot_segments("../../v1beta/files/other", "file").is_err());
        assert!(require_no_dot_segments("files/./abc", "file").is_err());
        assert!(require_no_dot_segments("files/..", "file").is_err());
        // Dots *inside* a segment are not dot segments.
        assert!(require_no_dot_segments("files/a.b.c", "file").is_ok());
        assert!(require_no_dot_segments("files/abc123", "file").is_ok());
    }

    #[test]
    fn test_path_segment_encodes_dot_segments() {
        // `.` is unreserved so the encoder alone passes dot segments
        // through — and URL parsing would then pop the preceding path
        // segment, reaching a different endpoint. Pin the explicit
        // percent-encoded forms.
        assert_eq!(path_segment("."), "%2E");
        assert_eq!(path_segment(".."), "%2E%2E");
        // A dot *inside* an ID is not a dot segment; it stays borrowed.
        assert_eq!(path_segment("a.b"), "a.b");
    }

    #[test]
    fn test_endpoint_create_interaction_non_streaming() {
        let endpoint = Endpoint::CreateInteraction { stream: false };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions"
        );
        assert!(!url.contains("alt=sse"));
        assert!(!url.contains("key=")); // API key should not be in URL
    }

    #[test]
    fn test_endpoint_create_interaction_streaming() {
        let endpoint = Endpoint::CreateInteraction { stream: true };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions?alt=sse"
        );
        assert!(url.contains("alt=sse"));
        assert!(!url.contains("key=")); // API key should not be in URL
    }

    #[test]
    fn test_endpoint_get_interaction_non_streaming() {
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: false,
            last_event_id: None,
            include_input: false,
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-123"
        );
        assert!(url.contains("/interactions/interaction-123"));
        assert!(!url.contains("alt=sse"));
        assert!(!url.contains("key=")); // API key should not be in URL
    }

    #[test]
    fn test_endpoint_get_interaction_streaming() {
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: true,
            last_event_id: None,
            include_input: false,
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-123?alt=sse"
        );
        assert!(url.contains("/interactions/interaction-123"));
        assert!(url.contains("alt=sse"));
    }

    #[test]
    fn test_endpoint_get_interaction_streaming_with_resume() {
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: true,
            last_event_id: Some("evt_abc123"),
            include_input: false,
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-123?alt=sse&last_event_id=evt_abc123"
        );
        assert!(url.contains("alt=sse"));
        assert!(url.contains("last_event_id=evt_abc123"));
    }

    #[test]
    fn test_endpoint_get_interaction_streaming_with_resume_special_chars() {
        // Event IDs with special characters should be URL-encoded
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: true,
            last_event_id: Some("evt+abc&123=test"),
            include_input: false,
        };
        let url = construct_endpoint_url(endpoint);

        // + becomes %2B, & becomes %26, = becomes %3D
        assert!(url.contains("last_event_id=evt%2Babc%26123%3Dtest"));
    }

    #[test]
    fn test_endpoint_get_interaction_non_streaming_ignores_last_event_id() {
        // When stream=false, last_event_id should be silently ignored
        // since resume is only meaningful for streaming requests
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: false,
            last_event_id: Some("evt_should_be_ignored"),
            include_input: false,
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-123"
        );
        // Verify last_event_id is NOT in the URL when stream=false
        assert!(!url.contains("last_event_id"));
        assert!(!url.contains("evt_should_be_ignored"));
        // Also verify no SSE params
        assert!(!url.contains("alt=sse"));
    }

    #[test]
    fn test_endpoint_delete_interaction() {
        let endpoint = Endpoint::DeleteInteraction {
            id: "interaction-456",
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-456"
        );
        assert!(url.contains("/interactions/interaction-456"));
        assert!(!url.contains("alt=sse"));
        assert!(!url.contains("key=")); // API key should not be in URL
    }

    #[test]
    fn test_api_key_header_constant() {
        assert_eq!(API_KEY_HEADER, "X-Goog-Api-Key");
    }

    #[test]
    fn test_endpoint_requires_sse() {
        assert!(Endpoint::CreateInteraction { stream: true }.requires_sse());
        assert!(!Endpoint::CreateInteraction { stream: false }.requires_sse());
        assert!(
            Endpoint::GetInteraction {
                id: "test",
                stream: true,
                last_event_id: None,
                include_input: false,
            }
            .requires_sse()
        );
        assert!(
            !Endpoint::GetInteraction {
                id: "test",
                stream: false,
                last_event_id: None,
                include_input: false,
            }
            .requires_sse()
        );
        assert!(!Endpoint::DeleteInteraction { id: "test" }.requires_sse());
    }

    #[test]
    fn test_endpoint_to_path() {
        let endpoint = Endpoint::CreateInteraction { stream: false };
        let path = endpoint.to_path(ApiVersion::V1Beta);
        assert_eq!(path, "/v1beta/interactions");
    }

    #[test]
    fn test_endpoint_clone_and_eq() {
        let endpoint1 = Endpoint::CreateInteraction { stream: true };
        let endpoint2 = endpoint1.clone();
        assert_eq!(endpoint1, endpoint2);

        let endpoint3 = Endpoint::GetInteraction {
            id: "test-id",
            stream: false,
            last_event_id: None,
            include_input: false,
        };
        let endpoint4 = Endpoint::GetInteraction {
            id: "test-id",
            stream: false,
            last_event_id: None,
            include_input: false,
        };
        assert_eq!(endpoint3, endpoint4);

        let endpoint5 = Endpoint::GetInteraction {
            id: "different-id",
            stream: false,
            last_event_id: None,
            include_input: false,
        };
        assert_ne!(endpoint3, endpoint5);

        // Test that stream and last_event_id affect equality
        let endpoint6 = Endpoint::GetInteraction {
            id: "test-id",
            stream: true,
            last_event_id: Some("evt_123"),
            include_input: false,
        };
        assert_ne!(endpoint3, endpoint6);
    }

    #[test]
    fn test_endpoint_cancel_interaction() {
        let endpoint = Endpoint::CancelInteraction {
            id: "interaction-789",
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-789/cancel"
        );
        assert!(url.contains("/interactions/interaction-789/cancel"));
        assert!(!url.contains("alt=sse"));
        assert!(!url.contains("key=")); // API key should not be in URL
    }

    #[test]
    fn test_cancel_interaction_requires_sse() {
        assert!(!Endpoint::CancelInteraction { id: "test" }.requires_sse());
    }
}
