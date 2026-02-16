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
const BASE_URL_PREFIX: &str = "https://generativelanguage.googleapis.com";

/// Header name for API key authentication.
///
/// Using header-based authentication is more secure than query parameters because:
/// - API keys don't appear in server logs, proxy logs, or browser history
/// - Keys are not leaked in error messages containing URLs
/// - Matches Google Cloud API best practices
pub const API_KEY_HEADER: &str = "X-Goog-Api-Key";

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
                format!("/{}/interactions/{}", version.as_str(), id)
            }
            Self::DeleteInteraction { id } => {
                format!("/{}/interactions/{}", version.as_str(), id)
            }
            Self::CancelInteraction { id } => {
                format!("/{}/interactions/{}/cancel", version.as_str(), id)
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
                let mut params = Vec::new();
                if *stream
                    && let Some(event_id) = last_event_id
                {
                    params.push(format!("last_event_id={}", urlencoding::encode(event_id)));
                }
                if *include_input {
                    params.push("include_input=true".to_string());
                }
                if params.is_empty() {
                    None
                } else {
                    Some(params.join("&"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_version_as_str() {
        assert_eq!(ApiVersion::V1Beta.as_str(), "v1beta");
    }

    // --- Tests for Endpoint-based URL construction ---

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
    fn test_endpoint_get_interaction_with_include_input() {
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: false,
            last_event_id: None,
            include_input: true,
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-123?include_input=true"
        );
        assert!(url.contains("include_input=true"));
        assert!(!url.contains("alt=sse"));
    }

    #[test]
    fn test_endpoint_get_interaction_streaming_with_include_input() {
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: true,
            last_event_id: None,
            include_input: true,
        };
        let url = construct_endpoint_url(endpoint);

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/interactions/interaction-123?alt=sse&include_input=true"
        );
        assert!(url.contains("alt=sse"));
        assert!(url.contains("include_input=true"));
    }

    #[test]
    fn test_endpoint_get_interaction_streaming_with_resume_and_include_input() {
        let endpoint = Endpoint::GetInteraction {
            id: "interaction-123",
            stream: true,
            last_event_id: Some("evt_abc123"),
            include_input: true,
        };
        let url = construct_endpoint_url(endpoint);

        assert!(url.contains("alt=sse"));
        assert!(url.contains("last_event_id=evt_abc123"));
        assert!(url.contains("include_input=true"));
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
