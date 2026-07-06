/// Helper functions for building Interactions API steps
///
/// This module provides constructors for model-generated step types that
/// appear in API responses (code execution, Google search, URL context).
///
/// # Primary API: `Content::*()` and `Step::*()`
///
/// For user input content, use `Content` constructors directly; for steps
/// (function calls/results, history turns), use `Step` constructors:
///
/// ```rust
/// use genai_rs::{Content, Step};
/// use serde_json::json;
///
/// let text = Content::text("Hello");
/// let image = Content::image_data("base64...", "image/png");
/// let call = Step::function_call("call_1", "get_weather", json!({"city": "NYC"}));
/// let result = Step::function_result("get_weather", "call_1", json!({"temp": 21}));
/// ```
///
/// # Model Output Step Constructors
///
/// This module provides constructors for step types that the MODEL generates:
/// - **Code Execution**: `code_execution_call_step`, `code_execution_result_step`
/// - **Google Search**: `google_search_call_step`, `google_search_result_step`
/// - **URL Context**: `url_context_call_step`, `url_context_result_step`
/// - **File Search**: `file_search_result_step`
///
/// These are primarily useful for testing and response simulation.
use crate::{CodeExecutionLanguage, FileSearchResultItem, GoogleSearchResultItem, Step};

// ============================================================================
// MODEL OUTPUT STEP CONSTRUCTORS
// ============================================================================
//
// These functions create steps that represent MODEL-generated outputs.
// Useful for testing and simulating API responses.

// ----------------------------------------------------------------------------
// Code Execution (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates a code execution call step
///
/// This step appears when the model initiates code execution
/// via the `CodeExecution` built-in tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::code_execution_call_step;
/// use genai_rs::CodeExecutionLanguage;
///
/// let call = code_execution_call_step("call_123", CodeExecutionLanguage::Python, "print('Hello, World!')");
/// ```
pub fn code_execution_call_step(
    id: impl Into<String>,
    language: CodeExecutionLanguage,
    code: impl Into<String>,
) -> Step {
    Step::CodeExecutionCall {
        id: id.into(),
        language,
        code: code.into(),
        signature: None,
    }
}

/// Creates a code execution result step
///
/// Contains the result of executed code from the `CodeExecution` tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::code_execution_result_step;
///
/// let result = code_execution_result_step("call_123", false, "42");
/// ```
pub fn code_execution_result_step(
    call_id: impl Into<String>,
    is_error: bool,
    result: impl Into<String>,
) -> Step {
    Step::CodeExecutionResult {
        call_id: call_id.into(),
        is_error,
        result: result.into(),
        signature: None,
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
pub fn code_execution_success(call_id: impl Into<String>, result: impl Into<String>) -> Step {
    code_execution_result_step(call_id, false, result)
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
pub fn code_execution_error(call_id: impl Into<String>, error_result: impl Into<String>) -> Step {
    code_execution_result_step(call_id, true, error_result)
}

// ----------------------------------------------------------------------------
// Google Search (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates a Google Search call step
///
/// Appears when the model initiates a Google Search via the `GoogleSearch` tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::google_search_call_step;
///
/// let search = google_search_call_step("call-123", vec!["Rust programming language"]);
/// ```
pub fn google_search_call_step(id: impl Into<String>, queries: Vec<impl Into<String>>) -> Step {
    Step::GoogleSearchCall {
        id: id.into(),
        queries: queries.into_iter().map(|q| q.into()).collect(),
        search_type: None,
        signature: None,
    }
}

/// Creates a Google Search result step
///
/// Contains the results returned by the `GoogleSearch` built-in tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::google_search_result_step;
/// use genai_rs::GoogleSearchResultItem;
///
/// let results = google_search_result_step("call-123", vec![
///     GoogleSearchResultItem::new("Rust", "https://rust-lang.org"),
/// ]);
/// ```
pub fn google_search_result_step(
    call_id: impl Into<String>,
    result: Vec<GoogleSearchResultItem>,
) -> Step {
    Step::GoogleSearchResult {
        call_id: call_id.into(),
        result,
        is_error: None,
        signature: None,
    }
}

// ----------------------------------------------------------------------------
// File Search (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates a file search result step
///
/// Returned when the model retrieves documents from file search stores.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::file_search_result_step;
/// use genai_rs::FileSearchResultItem;
///
/// let results = file_search_result_step("call-123", vec![
///     FileSearchResultItem {
///         title: "Document".into(),
///         text: "Content".into(),
///         store: "store-1".into(),
///     },
/// ]);
/// ```
pub fn file_search_result_step(
    call_id: impl Into<String>,
    result: Vec<FileSearchResultItem>,
) -> Step {
    Step::FileSearchResult {
        call_id: call_id.into(),
        result,
        signature: None,
    }
}

// ----------------------------------------------------------------------------
// URL Context (built-in tool output)
// ----------------------------------------------------------------------------

/// Creates a URL context call step
///
/// Appears when the model requests URL content via the `UrlContext` tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::url_context_call_step;
///
/// let fetch = url_context_call_step("ctx_123", vec!["https://example.com"]);
/// ```
pub fn url_context_call_step(
    id: impl Into<String>,
    urls: impl IntoIterator<Item = impl Into<String>>,
) -> Step {
    Step::UrlContextCall {
        id: id.into(),
        urls: urls.into_iter().map(Into::into).collect(),
        signature: None,
    }
}

/// Creates a URL context result step
///
/// Contains the results retrieved by the `UrlContext` built-in tool.
///
/// # Example
/// ```
/// use genai_rs::interactions_api::url_context_result_step;
/// use genai_rs::UrlContextResultItem;
///
/// let result = url_context_result_step(
///     "ctx_123",
///     vec![UrlContextResultItem::new("https://example.com", "success")]
/// );
/// ```
pub fn url_context_result_step(
    call_id: impl Into<String>,
    result: Vec<crate::UrlContextResultItem>,
) -> Step {
    Step::UrlContextResult {
        call_id: call_id.into(),
        result,
        is_error: None,
        signature: None,
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
pub fn url_context_success(call_id: impl Into<String>, url: impl Into<String>) -> Step {
    url_context_result_step(
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
pub fn url_context_failure(call_id: impl Into<String>, url: impl Into<String>) -> Step {
    url_context_result_step(
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

    #[test]
    fn test_code_execution_call_step_wire_format() {
        let step = code_execution_call_step("call_1", CodeExecutionLanguage::Python, "print(1)");
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "code_execution_call");
        assert_eq!(json["id"], "call_1");
        assert_eq!(json["arguments"]["language"], "python");
        assert_eq!(json["arguments"]["code"], "print(1)");
    }

    #[test]
    fn test_code_execution_result_helpers() {
        let ok = code_execution_success("c1", "42");
        assert!(matches!(
            ok,
            Step::CodeExecutionResult {
                is_error: false,
                ..
            }
        ));
        let err = code_execution_error("c1", "boom");
        assert!(matches!(
            err,
            Step::CodeExecutionResult { is_error: true, .. }
        ));
    }

    #[test]
    fn test_google_search_call_step_wire_format() {
        let step = google_search_call_step("s1", vec!["rust"]);
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "google_search_call");
        assert_eq!(json["arguments"]["queries"][0], "rust");
    }

    #[test]
    fn test_url_context_helpers() {
        let ok = url_context_success("u1", "https://example.com");
        match &ok {
            Step::UrlContextResult { result, .. } => {
                assert!(result[0].is_success());
            }
            other => panic!("Expected UrlContextResult, got {other:?}"),
        }
        let err = url_context_failure("u1", "https://example.com");
        match &err {
            Step::UrlContextResult { result, .. } => {
                assert!(result[0].is_error());
            }
            other => panic!("Expected UrlContextResult, got {other:?}"),
        }
    }

    #[test]
    fn test_file_search_result_step() {
        let step = file_search_result_step(
            "f1",
            vec![FileSearchResultItem::new("Doc", "Text", "store")],
        );
        let json = serde_json::to_value(&step).unwrap();
        assert_eq!(json["type"], "file_search_result");
        assert_eq!(json["result"][0]["file_search_store"], "store");
    }
}
