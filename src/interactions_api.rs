/// Helper functions for building Interactions API content
///
/// This module provides constructors for model-generated content types that appear
/// in API responses (code execution, Google search, URL context results).
///
/// # Primary API: `Content::*()`
///
/// For user input content, use `Content` constructors directly:
///
/// ```rust
/// use genai_rs::Content;
/// use serde_json::json;
///
/// let text = Content::text("Hello");
/// let image = Content::image_data("base64...", "image/png");
/// let func = Content::function_call("get_weather", json!({"city": "NYC"}));
/// ```
///
/// # Model Output Constructors
///
/// This module provides constructors for content types that the MODEL generates:
/// - **Code Execution**: `code_execution_call_content`, `code_execution_result_content`
/// - **Google Search**: `google_search_call_content`, `google_search_result_content`
/// - **URL Context**: `url_context_call_content`, `url_context_result_content`
/// - **File Search**: `file_search_result_content`
///
/// These are primarily useful for testing and response simulation.
use crate::{CodeExecutionLanguage, Content, FileSearchResultItem, GoogleSearchResultItem};

// ============================================================================
// MODEL OUTPUT CONSTRUCTORS
// ============================================================================
//
// These functions create content that represents MODEL-generated outputs.
// Useful for testing and simulating API responses.

// ----------------------------------------------------------------------------
// Code Execution (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates code execution call content
///
/// This variant appears when the model initiates code execution
/// via the `CodeExecution` built-in tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::code_execution_call_content;
/// use genai_rs::CodeExecutionLanguage;
///
/// let call = code_execution_call_content("call_123", CodeExecutionLanguage::Python, "print('Hello, World!')");
/// ```
pub fn code_execution_call_content(
    id: impl Into<String>,
    language: CodeExecutionLanguage,
    code: impl Into<String>,
) -> Content {
    Content::CodeExecutionCall {
        id: Some(id.into()),
        language,
        code: code.into(),
    }
}

/// Creates code execution result content
///
/// Contains the result of executed code from the `CodeExecution` tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::code_execution_result_content;
///
/// let result = code_execution_result_content("call_123", false, "42");
/// ```
pub fn code_execution_result_content(
    call_id: impl Into<String>,
    is_error: bool,
    result: impl Into<String>,
) -> Content {
    Content::CodeExecutionResult {
        call_id: Some(call_id.into()),
        is_error,
        result: result.into(),
    }
}

/// Creates a successful code execution result (convenience helper)
///
/// Shorthand for creating a successful (is_error=false) result.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::code_execution_success;
///
/// let result = code_execution_success("call_123", "42\n");
/// ```
pub fn code_execution_success(call_id: impl Into<String>, result: impl Into<String>) -> Content {
    code_execution_result_content(call_id, false, result)
}

/// Creates a failed code execution result (convenience helper)
///
/// Shorthand for creating a failed (is_error=true) result.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::code_execution_error;
///
/// let result = code_execution_error("call_123", "NameError: name 'x' is not defined");
/// ```
pub fn code_execution_error(
    call_id: impl Into<String>,
    error_result: impl Into<String>,
) -> Content {
    code_execution_result_content(call_id, true, error_result)
}

// ----------------------------------------------------------------------------
// Google Search (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates Google Search call content
///
/// Appears when the model initiates a Google Search via the `GoogleSearch` tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::google_search_call_content;
///
/// let search = google_search_call_content("call-123", vec!["Rust programming language"]);
/// ```
pub fn google_search_call_content(
    id: impl Into<String>,
    queries: Vec<impl Into<String>>,
) -> Content {
    Content::GoogleSearchCall {
        id: id.into(),
        queries: queries.into_iter().map(|q| q.into()).collect(),
    }
}

/// Creates Google Search result content
///
/// Contains the results returned by the `GoogleSearch` built-in tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::google_search_result_content;
/// use genai_rs::GoogleSearchResultItem;
///
/// let results = google_search_result_content("call-123", vec![
///     GoogleSearchResultItem::new("Rust", "https://rust-lang.org"),
/// ]);
/// ```
pub fn google_search_result_content(
    call_id: impl Into<String>,
    result: Vec<GoogleSearchResultItem>,
) -> Content {
    Content::GoogleSearchResult {
        call_id: call_id.into(),
        result,
    }
}

// ----------------------------------------------------------------------------
// File Search (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates file search result content
///
/// Returned when the model retrieves documents from file search stores.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::file_search_result_content;
/// use genai_rs::FileSearchResultItem;
///
/// let results = file_search_result_content("call-123", vec![
///     FileSearchResultItem {
///         title: "Document".into(),
///         text: "Content".into(),
///         store: "store-1".into(),
///     },
/// ]);
/// ```
pub fn file_search_result_content(
    call_id: impl Into<String>,
    result: Vec<FileSearchResultItem>,
) -> Content {
    Content::FileSearchResult {
        call_id: call_id.into(),
        result,
    }
}

// ----------------------------------------------------------------------------
// URL Context (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates URL context call content
///
/// Appears when the model requests URL content via the `UrlContext` tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::url_context_call_content;
///
/// let fetch = url_context_call_content("ctx_123", vec!["https://example.com"]);
/// ```
pub fn url_context_call_content(
    id: impl Into<String>,
    urls: impl IntoIterator<Item = impl Into<String>>,
) -> Content {
    Content::UrlContextCall {
        id: id.into(),
        urls: urls.into_iter().map(Into::into).collect(),
    }
}

/// Creates URL context result content
///
/// Contains the results retrieved by the `UrlContext` built-in tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::url_context_result_content;
/// use genai_rs::UrlContextResultItem;
///
/// let result = url_context_result_content(
///     "ctx_123",
///     vec![UrlContextResultItem::new("https://example.com", "success")]
/// );
/// ```
pub fn url_context_result_content(
    call_id: impl Into<String>,
    result: Vec<crate::UrlContextResultItem>,
) -> Content {
    Content::UrlContextResult {
        call_id: call_id.into(),
        result,
    }
}

/// Creates a successful URL context result for a single URL (convenience helper)
///
/// Shorthand for creating a result where a single URL was successfully fetched.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::url_context_success;
///
/// let result = url_context_success("ctx_123", "https://example.com");
/// ```
pub fn url_context_success(call_id: impl Into<String>, url: impl Into<String>) -> Content {
    url_context_result_content(
        call_id,
        vec![crate::UrlContextResultItem::new(url, "success")],
    )
}

/// Creates a failed URL context result for a single URL (convenience helper)
///
/// Shorthand for creating a result where a single URL fetch failed
/// (e.g., network errors, blocked URLs, timeouts, or access restrictions).
///
/// # Example
/// ```
/// use genai_rs::interactions_api::url_context_failure;
///
/// let result = url_context_failure("ctx_123", "https://example.com/blocked");
/// ```
pub fn url_context_failure(call_id: impl Into<String>, url: impl Into<String>) -> Content {
    url_context_result_content(
        call_id,
        vec![crate::UrlContextResultItem::new(url, "error")],
    )
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Resolution;
    use serde_json::json;

    // Tests for Content::*() constructors (the primary API)

    #[test]
    fn test_content_text() {
        let content = Content::text("Hello");
        match content {
            Content::Text { text, .. } => assert_eq!(text, Some("Hello".to_string())),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_content_thought() {
        let content = Content::thought("EosFCogFAXLI2...");
        match content {
            Content::Thought { signature } => {
                assert_eq!(signature, Some("EosFCogFAXLI2...".to_string()))
            }
            _ => panic!("Expected Thought variant"),
        }
    }

    #[test]
    fn test_content_function_call() {
        let content = Content::function_call("test", json!({"key": "value"}));
        match content {
            Content::FunctionCall { name, args, .. } => {
                assert_eq!(name, "test");
                assert_eq!(args, json!({"key": "value"}));
            }
            _ => panic!("Expected FunctionCall variant"),
        }
    }

    #[test]
    fn test_content_function_result() {
        let content = Content::function_result("test", "call_123", json!({"result": "ok"}));
        match content {
            Content::FunctionResult {
                name,
                call_id,
                result,
                is_error,
            } => {
                assert_eq!(name, Some("test".to_string()));
                assert_eq!(call_id, "call_123");
                assert_eq!(result, json!({"result": "ok"}));
                assert_eq!(is_error, None);
            }
            _ => panic!("Expected FunctionResult variant"),
        }
    }

    #[test]
    fn test_content_image_data() {
        let content = Content::image_data("data123", "image/png");
        match content {
            Content::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, Some("data123".to_string()));
                assert_eq!(uri, None);
                assert_eq!(mime_type, Some("image/png".to_string()));
                assert_eq!(resolution, None);
            }
            _ => panic!("Expected Image variant"),
        }
    }

    #[test]
    fn test_content_image_data_with_resolution() {
        let content = Content::image_data_with_resolution("data123", "image/png", Resolution::High);
        match content {
            Content::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, Some("data123".to_string()));
                assert_eq!(uri, None);
                assert_eq!(mime_type, Some("image/png".to_string()));
                assert_eq!(resolution, Some(Resolution::High));
            }
            _ => panic!("Expected Image variant"),
        }
    }

    #[test]
    fn test_content_image_uri() {
        let content = Content::image_uri("http://example.com/img.png", "image/png");
        match content {
            Content::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, None);
                assert_eq!(uri, Some("http://example.com/img.png".to_string()));
                assert_eq!(mime_type, Some("image/png".to_string()));
                assert_eq!(resolution, None);
            }
            _ => panic!("Expected Image variant"),
        }
    }

    #[test]
    fn test_content_image_uri_with_resolution() {
        let content = Content::image_uri_with_resolution(
            "http://example.com/img.png",
            "image/png",
            Resolution::Low,
        );
        match content {
            Content::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, None);
                assert_eq!(uri, Some("http://example.com/img.png".to_string()));
                assert_eq!(mime_type, Some("image/png".to_string()));
                assert_eq!(resolution, Some(Resolution::Low));
            }
            _ => panic!("Expected Image variant"),
        }
    }

    #[test]
    fn test_content_function_call_with_id() {
        let content = Content::function_call_with_id(
            Some("call_abc"),
            "get_weather",
            json!({"city": "Tokyo"}),
        );
        match content {
            Content::FunctionCall { id, name, args, .. } => {
                assert_eq!(id, Some("call_abc".to_string()));
                assert_eq!(name, "get_weather");
                assert_eq!(args, json!({"city": "Tokyo"}));
            }
            _ => panic!("Expected FunctionCall variant"),
        }
    }

    #[test]
    fn test_content_function_call_without_id() {
        let content = Content::function_call_with_id(None::<String>, "test_fn", json!({}));
        match content {
            Content::FunctionCall { id, .. } => {
                assert_eq!(id, None);
            }
            _ => panic!("Expected FunctionCall variant"),
        }
    }

    #[test]
    fn test_content_audio_data() {
        let content = Content::audio_data("audio_base64_data", "audio/mp3");
        match content {
            Content::Audio {
                data,
                uri,
                mime_type,
            } => {
                assert_eq!(data, Some("audio_base64_data".to_string()));
                assert_eq!(uri, None);
                assert_eq!(mime_type, Some("audio/mp3".to_string()));
            }
            _ => panic!("Expected Audio variant"),
        }
    }

    #[test]
    fn test_content_audio_uri() {
        let content = Content::audio_uri("https://example.com/audio.mp3", "audio/mp3");
        match content {
            Content::Audio {
                data,
                uri,
                mime_type,
            } => {
                assert_eq!(data, None);
                assert_eq!(uri, Some("https://example.com/audio.mp3".to_string()));
                assert_eq!(mime_type, Some("audio/mp3".to_string()));
            }
            _ => panic!("Expected Audio variant"),
        }
    }

    #[test]
    fn test_content_video_data() {
        let content = Content::video_data("video_base64_data", "video/mp4");
        match content {
            Content::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, Some("video_base64_data".to_string()));
                assert_eq!(uri, None);
                assert_eq!(mime_type, Some("video/mp4".to_string()));
                assert_eq!(resolution, None);
            }
            _ => panic!("Expected Video variant"),
        }
    }

    #[test]
    fn test_content_video_data_with_resolution() {
        let content =
            Content::video_data_with_resolution("video_base64_data", "video/mp4", Resolution::Low);
        match content {
            Content::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, Some("video_base64_data".to_string()));
                assert_eq!(uri, None);
                assert_eq!(mime_type, Some("video/mp4".to_string()));
                assert_eq!(resolution, Some(Resolution::Low));
            }
            _ => panic!("Expected Video variant"),
        }
    }

    #[test]
    fn test_content_video_uri() {
        let content = Content::video_uri("https://example.com/video.mp4", "video/mp4");
        match content {
            Content::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, None);
                assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
                assert_eq!(mime_type, Some("video/mp4".to_string()));
                assert_eq!(resolution, None);
            }
            _ => panic!("Expected Video variant"),
        }
    }

    #[test]
    fn test_content_video_uri_with_resolution() {
        let content = Content::video_uri_with_resolution(
            "https://example.com/video.mp4",
            "video/mp4",
            Resolution::UltraHigh,
        );
        match content {
            Content::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                assert_eq!(data, None);
                assert_eq!(uri, Some("https://example.com/video.mp4".to_string()));
                assert_eq!(mime_type, Some("video/mp4".to_string()));
                assert_eq!(resolution, Some(Resolution::UltraHigh));
            }
            _ => panic!("Expected Video variant"),
        }
    }

    #[test]
    fn test_content_document_data() {
        let content = Content::document_data("pdf_base64_data", "application/pdf");
        match content {
            Content::Document {
                data,
                uri,
                mime_type,
            } => {
                assert_eq!(data, Some("pdf_base64_data".to_string()));
                assert_eq!(uri, None);
                assert_eq!(mime_type, Some("application/pdf".to_string()));
            }
            _ => panic!("Expected Document variant"),
        }
    }

    #[test]
    fn test_content_document_uri() {
        let content = Content::document_uri("https://example.com/doc.pdf", "application/pdf");
        match content {
            Content::Document {
                data,
                uri,
                mime_type,
            } => {
                assert_eq!(data, None);
                assert_eq!(uri, Some("https://example.com/doc.pdf".to_string()));
                assert_eq!(mime_type, Some("application/pdf".to_string()));
            }
            _ => panic!("Expected Document variant"),
        }
    }

    // Tests for model output constructors (kept in this module)

    #[test]
    fn test_code_execution_call_content() {
        let content =
            code_execution_call_content("call_123", CodeExecutionLanguage::Python, "print(42)");
        match content {
            Content::CodeExecutionCall { id, language, code } => {
                assert_eq!(id, Some("call_123".to_string()));
                assert_eq!(language, CodeExecutionLanguage::Python);
                assert_eq!(code, "print(42)");
            }
            _ => panic!("Expected CodeExecutionCall variant"),
        }
    }

    #[test]
    fn test_code_execution_result_content() {
        let content = code_execution_result_content("call_123", false, "42");
        match content {
            Content::CodeExecutionResult {
                call_id,
                is_error,
                result,
            } => {
                assert_eq!(call_id, Some("call_123".to_string()));
                assert!(!is_error);
                assert_eq!(result, "42");
            }
            _ => panic!("Expected CodeExecutionResult variant"),
        }
    }

    #[test]
    fn test_code_execution_success() {
        let content = code_execution_success("call_123", "42\n");
        match content {
            Content::CodeExecutionResult { is_error, .. } => {
                assert!(!is_error);
            }
            _ => panic!("Expected CodeExecutionResult variant"),
        }
    }

    #[test]
    fn test_code_execution_error() {
        let content = code_execution_error("call_123", "NameError");
        match content {
            Content::CodeExecutionResult { is_error, .. } => {
                assert!(is_error);
            }
            _ => panic!("Expected CodeExecutionResult variant"),
        }
    }

    #[test]
    fn test_google_search_call_content() {
        let content = google_search_call_content("call_123", vec!["query1", "query2"]);
        match content {
            Content::GoogleSearchCall { id, queries } => {
                assert_eq!(id, "call_123");
                assert_eq!(queries, vec!["query1", "query2"]);
            }
            _ => panic!("Expected GoogleSearchCall variant"),
        }
    }

    #[test]
    fn test_google_search_result_content() {
        let results = vec![GoogleSearchResultItem::new("Title", "https://example.com")];
        let content = google_search_result_content("call_123", results);
        match content {
            Content::GoogleSearchResult { call_id, result } => {
                assert_eq!(call_id, "call_123");
                assert_eq!(result.len(), 1);
            }
            _ => panic!("Expected GoogleSearchResult variant"),
        }
    }

    #[test]
    fn test_file_search_result_content() {
        let results = vec![FileSearchResultItem {
            title: "Doc".into(),
            text: "Content".into(),
            store: "store-1".into(),
        }];
        let content = file_search_result_content("call_123", results);
        match content {
            Content::FileSearchResult { call_id, result } => {
                assert_eq!(call_id, "call_123");
                assert_eq!(result.len(), 1);
            }
            _ => panic!("Expected FileSearchResult variant"),
        }
    }

    #[test]
    fn test_url_context_call_content() {
        let content = url_context_call_content("ctx_123", vec!["https://example.com"]);
        match content {
            Content::UrlContextCall { id, urls } => {
                assert_eq!(id, "ctx_123");
                assert_eq!(urls, vec!["https://example.com"]);
            }
            _ => panic!("Expected UrlContextCall variant"),
        }
    }

    #[test]
    fn test_url_context_result_content() {
        let results = vec![crate::UrlContextResultItem::new(
            "https://example.com",
            "success",
        )];
        let content = url_context_result_content("ctx_123", results);
        match content {
            Content::UrlContextResult { call_id, result } => {
                assert_eq!(call_id, "ctx_123");
                assert_eq!(result.len(), 1);
            }
            _ => panic!("Expected UrlContextResult variant"),
        }
    }

    #[test]
    fn test_url_context_success() {
        let content = url_context_success("ctx_123", "https://example.com");
        match content {
            Content::UrlContextResult { result, .. } => {
                assert_eq!(result.len(), 1);
                assert_eq!(result[0].status, "success");
            }
            _ => panic!("Expected UrlContextResult variant"),
        }
    }

    #[test]
    fn test_url_context_failure() {
        let content = url_context_failure("ctx_123", "https://example.com");
        match content {
            Content::UrlContextResult { result, .. } => {
                assert_eq!(result.len(), 1);
                assert_eq!(result[0].status, "error");
            }
            _ => panic!("Expected UrlContextResult variant"),
        }
    }
}
