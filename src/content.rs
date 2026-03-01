//! Core content types for the Interactions API.
//!
//! This module contains `Content`, the primary enum representing all content
//! types that can appear in API requests and responses, along with its custom serialization
//! and deserialization implementations.

use serde::{Deserialize, Serialize};
use std::fmt;

// =============================================================================
// Annotation Types (for Text Content with Citations)
// =============================================================================

/// An annotation linking a text span to its source.
///
/// Annotations provide citation information for specific portions of generated text,
/// typically appearing when using grounding tools like `GoogleSearch` or `UrlContext`.
///
/// # Byte Indices
///
/// **Important:** The `start_index` and `end_index` fields are **byte positions** (not
/// character indices) in the text content. This matches the UTF-8 byte offsets used
/// by the Gemini API. For multi-byte characters (like emoji or non-ASCII text), you
/// may need to use byte-slicing rather than character indexing.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{InteractionResponse, Annotation};
/// # let response: InteractionResponse = todo!();
/// // Get all annotations from the response
/// for annotation in response.all_annotations() {
///     println!(
///         "Text at bytes {}..{} sourced from: {}",
///         annotation.start_index,
///         annotation.end_index,
///         annotation.source.as_deref().unwrap_or("<no source>")
///     );
/// }
/// ```
///
/// # Extracting Annotated Text
///
/// To extract the annotated substring from the response text:
///
/// ```no_run
/// # use genai_rs::{InteractionResponse, Annotation};
/// # let response: InteractionResponse = todo!();
/// # let annotation: &Annotation = todo!();
/// if let Some(text) = response.as_text() {
///     // Use byte slicing since indices are byte positions
///     let bytes = text.as_bytes();
///     if annotation.end_index <= bytes.len() {
///         if let Ok(span) = std::str::from_utf8(&bytes[annotation.start_index..annotation.end_index]) {
///             println!("Cited text: {}", span);
///         }
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct Annotation {
    /// Start of the segment in the text (byte position, inclusive).
    ///
    /// This is a byte offset into the UTF-8 encoded text, not a character index.
    #[serde(default)]
    pub start_index: usize,

    /// End of the segment in the text (byte position, exclusive).
    ///
    /// This is a byte offset into the UTF-8 encoded text, not a character index.
    #[serde(default)]
    pub end_index: usize,

    /// Source attributed for this portion of the text.
    ///
    /// This could be a URL, title, or other identifier depending on the grounding
    /// tool used (e.g., `GoogleSearch` or `UrlContext`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl Annotation {
    /// Creates a new annotation with the given span indices and optional source.
    ///
    /// # Arguments
    ///
    /// * `start_index` - Start of the segment in the text (byte position, inclusive)
    /// * `end_index` - End of the segment in the text (byte position, exclusive)
    /// * `source` - Optional source attribution (URL, title, or identifier)
    ///
    /// # Example
    ///
    /// ```
    /// # use genai_rs::Annotation;
    /// let annotation = Annotation::new(0, 10, Some("https://example.com".to_string()));
    /// assert_eq!(annotation.start_index, 0);
    /// assert_eq!(annotation.end_index, 10);
    /// assert!(annotation.has_source());
    /// ```
    #[must_use]
    pub fn new(start_index: usize, end_index: usize, source: Option<String>) -> Self {
        Self {
            start_index,
            end_index,
            source,
        }
    }

    /// Returns the byte length of the annotated span.
    ///
    /// This is equivalent to `end_index - start_index`.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    /// Returns `true` if this annotation has a source attribution.
    #[must_use]
    pub fn has_source(&self) -> bool {
        self.source.is_some()
    }

    /// Extracts the annotated substring from the given text.
    ///
    /// Returns `None` if the indices are out of bounds or if the byte slice
    /// doesn't form valid UTF-8.
    ///
    /// # Arguments
    ///
    /// * `text` - The full text content to extract from
    ///
    /// # Example
    ///
    /// ```
    /// # use genai_rs::Annotation;
    /// let annotation = Annotation::new(0, 5, Some("https://example.com".to_string()));
    ///
    /// let text = "Hello, world!";
    /// assert_eq!(annotation.extract_span(text), Some("Hello"));
    /// ```
    #[must_use]
    pub fn extract_span<'a>(&self, text: &'a str) -> Option<&'a str> {
        let bytes = text.as_bytes();
        if self.end_index <= bytes.len() && self.start_index <= self.end_index {
            std::str::from_utf8(&bytes[self.start_index..self.end_index]).ok()
        } else {
            None
        }
    }
}

// =============================================================================
// Google Search Result Item
// =============================================================================

/// A single result from a Google Search.
///
/// Contains the source information for a grounding chunk including the title,
/// URL, and optionally the rendered content that was used for grounding.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Content, GoogleSearchResultItem};
/// # let content: Content = todo!();
/// if let Content::GoogleSearchResult { result, .. } = content {
///     for item in result {
///         println!("Source: {} - {}", item.title, item.url);
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleSearchResultItem {
    /// Title of the search result (often the domain name)
    pub title: String,
    /// URL of the source (typically a grounding redirect URL)
    pub url: String,
    /// The rendered content from the source (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_content: Option<String>,
}

impl GoogleSearchResultItem {
    /// Creates a new GoogleSearchResultItem.
    #[must_use]
    pub fn new(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            rendered_content: None,
        }
    }

    /// Returns `true` if this result has rendered content.
    #[must_use]
    pub fn has_rendered_content(&self) -> bool {
        self.rendered_content.is_some()
    }
}

// =============================================================================
// URL Context Result Item
// =============================================================================

/// A single result from a URL Context fetch.
///
/// Contains the status of the URL fetch operation.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Content, UrlContextResultItem};
/// # let content: Content = todo!();
/// if let Content::UrlContextResult { result, .. } = content {
///     for item in result {
///         println!("URL: {} - Status: {}", item.url, item.status);
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UrlContextResultItem {
    /// The URL that was fetched
    pub url: String,
    /// Status of the fetch operation (e.g., "success", "error", "unsafe")
    pub status: String,
}

impl UrlContextResultItem {
    /// Creates a new UrlContextResultItem.
    #[must_use]
    pub fn new(url: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            status: status.into(),
        }
    }

    /// Returns `true` if the fetch was successful.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.status == "success"
    }

    /// Returns `true` if the fetch failed with an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.status == "error"
    }

    /// Returns `true` if the URL was blocked as unsafe.
    #[must_use]
    pub fn is_unsafe(&self) -> bool {
        self.status == "unsafe"
    }
}

// =============================================================================
// Partial Argument (Streaming Function Call Arguments)
// =============================================================================

/// A partial argument fragment from a streaming function call.
///
/// When `stream_function_call_arguments` is enabled in the request, the API
/// streams function call arguments incrementally. Each `PartialArg` represents
/// a single fragment, identified by its [`json_path`](PartialArg::json_path)
/// within the function's parameters object.
///
/// # Wire Format
///
/// Each partial arg has a JSONPath (RFC 9535) identifying the parameter being
/// streamed, exactly one value field (`string_value`, `number_value`,
/// `bool_value`, or `null_value`), and a `will_continue` flag indicating
/// whether more chunks are expected for this specific argument.
///
/// # Example
///
/// ```
/// use genai_rs::PartialArg;
///
/// let arg = PartialArg {
///     json_path: "$.location".to_string(),
///     string_value: Some("San Fran".to_string()),
///     number_value: None,
///     bool_value: None,
///     null_value: None,
///     will_continue: Some(true), // more chunks coming for this arg
/// };
///
/// assert_eq!(arg.json_path, "$.location");
/// assert!(arg.is_string());
/// assert!(arg.will_continue());
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct PartialArg {
    /// JSONPath (RFC 9535) identifying the parameter within the function's arguments.
    ///
    /// Examples: `"$.location"`, `"$.location.latitude"`, `"$.items[0].name"`
    pub json_path: String,

    /// String value fragment (present when the argument value is a string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string_value: Option<String>,

    /// Numeric value (present when the argument value is a number).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_value: Option<f64>,

    /// Boolean value (present when the argument value is a boolean).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bool_value: Option<bool>,

    /// Null value marker (present when the argument value is null).
    ///
    /// When this field is `Some("NULL_VALUE")`, the argument value is JSON null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub null_value: Option<String>,

    /// Whether more chunks are expected for this specific argument.
    ///
    /// Only meaningful for `string_value` fragments: when `true`, the string is
    /// being streamed in chunks and more chunks will follow for this argument.
    /// When `false` or absent, this is the final chunk for this argument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub will_continue: Option<bool>,
}

impl PartialArg {
    /// Returns `true` if more chunks are expected for this argument.
    #[must_use]
    pub fn will_continue(&self) -> bool {
        self.will_continue.unwrap_or(false)
    }

    /// Returns `true` if this is a string value fragment.
    #[must_use]
    pub fn is_string(&self) -> bool {
        self.string_value.is_some()
    }

    /// Returns `true` if this is a number value.
    #[must_use]
    pub fn is_number(&self) -> bool {
        self.number_value.is_some()
    }

    /// Returns `true` if this is a boolean value.
    #[must_use]
    pub fn is_bool(&self) -> bool {
        self.bool_value.is_some()
    }

    /// Returns `true` if this is a null value.
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.null_value.is_some()
    }

    /// Returns the value as a [`serde_json::Value`], regardless of which type it is.
    #[must_use]
    pub fn value(&self) -> serde_json::Value {
        if let Some(s) = &self.string_value {
            serde_json::Value::String(s.clone())
        } else if let Some(n) = self.number_value {
            serde_json::json!(n)
        } else if let Some(b) = self.bool_value {
            serde_json::Value::Bool(b)
        } else {
            serde_json::Value::Null
        }
    }
}

// =============================================================================
// File Search Result Item
// =============================================================================

/// A single result from a File Search.
///
/// Contains the extracted text from a semantic search match, including the title,
/// text content, and the source file search store.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Content, FileSearchResultItem};
/// # let content: Content = todo!();
/// if let Content::FileSearchResult { result, .. } = content {
///     for item in result {
///         println!("Match from '{}': {}", item.store, item.text);
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileSearchResultItem {
    /// Title of the matched document
    pub title: String,
    /// Extracted text content from the semantic match
    pub text: String,
    /// Name of the file search store containing this result
    #[serde(rename = "file_search_store")]
    pub store: String,
}

impl FileSearchResultItem {
    /// Creates a new FileSearchResultItem.
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        text: impl Into<String>,
        store: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
            store: store.into(),
        }
    }

    /// Returns `true` if this result has any text content.
    #[must_use]
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}

/// Programming language for code execution.
///
/// This enum represents the programming language used in code execution requests.
/// Currently only Python is supported by the Gemini API.
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// This enum is marked `#[non_exhaustive]`, which means:
/// - Match statements must include a wildcard arm (`_ => ...`)
/// - New variants may be added in minor version updates without breaking your code
///
/// When the API returns a language value that this library doesn't recognize,
/// it will be captured as `CodeExecutionLanguage::Unknown` rather than causing a
/// deserialization error. This follows the
/// [Evergreen spec](https://github.com/google-deepmind/evergreen-spec)
/// philosophy of graceful degradation.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Content, CodeExecutionLanguage};
/// # let content: Content = todo!();
/// if let Content::CodeExecutionCall { language, code, .. } = content {
///     match language {
///         CodeExecutionLanguage::Python => println!("Python code: {}", code),
///         CodeExecutionLanguage::Unknown { language_type, .. } => {
///             println!("Unknown language '{}': {}", language_type, code);
///         }
///         _ => println!("Other language: {}", code),
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodeExecutionLanguage {
    /// Python programming language
    #[default]
    Python,
    /// Unknown language (for forward compatibility).
    ///
    /// This variant captures any unrecognized language values from the API,
    /// allowing the library to handle new languages gracefully.
    ///
    /// The `language_type` field contains the unrecognized language string,
    /// and `data` contains the full JSON value for debugging.
    Unknown {
        /// The unrecognized language string from the API
        language_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl CodeExecutionLanguage {
    /// Check if this is an unknown language.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the language type name if this is an unknown language.
    ///
    /// Returns `None` for known languages.
    #[must_use]
    pub fn unknown_language_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { language_type, .. } => Some(language_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown language.
    ///
    /// Returns `None` for known languages.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for CodeExecutionLanguage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Python => serializer.serialize_str("PYTHON"),
            Self::Unknown { language_type, .. } => serializer.serialize_str(language_type),
        }
    }
}

impl<'de> Deserialize<'de> for CodeExecutionLanguage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.as_str() {
            Some("PYTHON") => Ok(Self::Python),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown CodeExecutionLanguage '{}'. \
                     This may indicate a new API feature. \
                     The language will be preserved in the Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    language_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                // Non-string value - preserve it in Unknown
                let language_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "CodeExecutionLanguage received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    language_type,
                    data: value,
                })
            }
        }
    }
}

impl fmt::Display for CodeExecutionLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Python => write!(f, "PYTHON"),
            Self::Unknown { language_type, .. } => write!(f, "{}", language_type),
        }
    }
}

/// Resolution level for image and video content processing.
///
/// Controls the quality vs. token cost trade-off when processing images and videos.
/// Lower resolution uses fewer tokens (lower cost), while higher resolution provides
/// more detail for the model to analyze.
///
/// # Token Cost Trade-offs
///
/// | Resolution | Token Cost | Detail Level |
/// |------------|------------|--------------|
/// | Low | Lowest | Basic shapes and colors |
/// | Medium | Moderate | Standard detail |
/// | High | Higher | Fine details visible |
/// | UltraHigh | Highest | Maximum fidelity |
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// This enum is marked `#[non_exhaustive]`, which means:
/// - Match statements must include a wildcard arm (`_ => ...`)
/// - New variants may be added in minor version updates without breaking your code
///
/// When the API returns a resolution value that this library doesn't recognize,
/// it will be captured as `Resolution::Unknown` rather than causing a
/// deserialization error. This follows the
/// [Evergreen spec](https://github.com/google-deepmind/evergreen-spec)
/// philosophy of graceful degradation.
///
/// # Example
///
/// ```
/// use genai_rs::Resolution;
///
/// // Use Low for cheap, basic analysis
/// let low_cost = Resolution::Low;
///
/// // Use High for detailed analysis
/// let detailed = Resolution::High;
///
/// // Default is Medium
/// assert_eq!(Resolution::default(), Resolution::Medium);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolution {
    /// Lowest token cost, basic shapes and colors
    Low,
    /// Moderate token cost, standard detail (default)
    #[default]
    Medium,
    /// Higher token cost, fine details visible
    High,
    /// Highest token cost, maximum fidelity
    UltraHigh,
    /// Unknown resolution (for forward compatibility).
    ///
    /// This variant captures any unrecognized resolution values from the API,
    /// allowing the library to handle new resolutions gracefully.
    ///
    /// The `resolution_type` field contains the unrecognized resolution string,
    /// and `data` contains the JSON value (typically the same string).
    Unknown {
        /// The unrecognized resolution string from the API
        resolution_type: String,
        /// The raw JSON value, preserved for debugging
        data: serde_json::Value,
    },
}

impl Resolution {
    /// Check if this is an unknown resolution.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the resolution type name if this is an unknown resolution.
    ///
    /// Returns `None` for known resolutions.
    #[must_use]
    pub fn unknown_resolution_type(&self) -> Option<&str> {
        match self {
            Self::Unknown {
                resolution_type, ..
            } => Some(resolution_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown resolution.
    ///
    /// Returns `None` for known resolutions.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for Resolution {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Low => serializer.serialize_str("low"),
            Self::Medium => serializer.serialize_str("medium"),
            Self::High => serializer.serialize_str("high"),
            Self::UltraHigh => serializer.serialize_str("ultra_high"),
            Self::Unknown {
                resolution_type, ..
            } => serializer.serialize_str(resolution_type),
        }
    }
}

impl<'de> Deserialize<'de> for Resolution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.as_str() {
            Some("low") => Ok(Self::Low),
            Some("medium") => Ok(Self::Medium),
            Some("high") => Ok(Self::High),
            Some("ultra_high") => Ok(Self::UltraHigh),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown Resolution '{}'. \
                     This may indicate a new API feature. \
                     The resolution will be preserved in the Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    resolution_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                // Non-string value - preserve it in Unknown
                let resolution_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "Resolution received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    resolution_type,
                    data: value,
                })
            }
        }
    }
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::UltraHigh => write!(f, "ultra_high"),
            Self::Unknown {
                resolution_type, ..
            } => write!(f, "{}", resolution_type),
        }
    }
}

/// Content object for Interactions API - uses flat structure with type field.
///
/// This enum represents all content types that can appear in API requests and responses.
/// It includes an `Unknown` variant for forward compatibility with new API content types.
///
/// # Forward Compatibility
///
/// This enum is marked `#[non_exhaustive]`, which means:
/// - Match statements must include a wildcard arm (`_ => ...`)
/// - New variants may be added in minor version updates without breaking your code
///
/// When the API returns a content type that this library doesn't recognize, it will be
/// captured as `Content::Unknown` rather than causing a deserialization error.
/// This allows your code to continue working even when Google adds new content types.
///
/// Use [`super::InteractionResponse::has_unknown`] and [`super::InteractionResponse::unknown_content`]
/// to detect and inspect unknown content.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Content, InteractionResponse};
/// # let response: InteractionResponse = todo!();
/// for content in &response.outputs {
///     match content {
///         Content::Text { text, annotations } => {
///             println!("Text: {:?}", text);
///             if let Some(annots) = annotations {
///                 println!("  {} annotations", annots.len());
///             }
///         }
///         Content::FunctionCall { name, .. } => println!("Function: {}", name),
///         Content::Unknown { content_type, .. } => {
///             println!("Unknown content type: {}", content_type);
///         }
///         // Required due to #[non_exhaustive] - handles future variants
///         _ => {}
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Content {
    /// Text content with optional source annotations.
    ///
    /// Annotations are present when grounding tools like `GoogleSearch` or `UrlContext`
    /// provide citation information linking text spans to their sources.
    Text {
        /// The text content.
        ///
        /// This is `Option<String>` because during streaming, `content.start` events
        /// announce the content type before any text arrives. The actual text is
        /// delivered in subsequent `content.delta` events. For non-streaming responses
        /// and delta events, this will always be `Some`.
        text: Option<String>,
        /// Source annotations for portions of the text.
        ///
        /// Present when the response includes citation information from grounding tools.
        /// Use [`annotations()`](Self::annotations) for convenient access.
        annotations: Option<Vec<Annotation>>,
    },
    /// Thought content (internal reasoning).
    ///
    /// Contains a cryptographic signature for verification of the thinking process.
    /// The actual thought text is not exposed in the API response - only the signature
    /// which can be used to validate that the response was generated through the
    /// model's reasoning process.
    ///
    /// The `signature` field is `Option<String>` because `content.start` events
    /// announce the type before the signature arrives via `content.delta`.
    Thought { signature: Option<String> },
    /// Thought signature (cryptographic signature for thought verification)
    ///
    /// This variant typically appears only during streaming responses, providing
    /// a cryptographic signature that verifies the authenticity of thought content.
    ThoughtSignature { signature: String },
    /// Image content
    Image {
        data: Option<String>,
        uri: Option<String>,
        mime_type: Option<String>,
        resolution: Option<Resolution>,
    },
    /// Audio content
    Audio {
        data: Option<String>,
        uri: Option<String>,
        mime_type: Option<String>,
    },
    /// Video content
    Video {
        data: Option<String>,
        uri: Option<String>,
        mime_type: Option<String>,
        resolution: Option<Resolution>,
    },
    /// Document content for file-based inputs.
    ///
    /// PDF (`application/pdf`) is the primary supported format with full vision capabilities
    /// for understanding text, images, charts, and tables. Other formats like TXT, Markdown,
    /// HTML, and XML are processed as plain text only, losing visual structure.
    Document {
        data: Option<String>,
        uri: Option<String>,
        mime_type: Option<String>,
    },
    /// Function call (output from model)
    ///
    /// When `stream_function_call_arguments` is enabled, streaming responses will
    /// include `partial_args` with incremental argument fragments and `will_continue`
    /// indicating whether more fragments are expected.
    FunctionCall {
        /// Unique identifier for this function call
        id: Option<String>,
        name: String,
        args: serde_json::Value,
        /// Partial argument fragments streamed incrementally.
        ///
        /// Only present when `stream_function_call_arguments` is enabled
        /// in the request and the response is being streamed.
        partial_args: Option<Vec<PartialArg>>,
        /// Whether more partial args are expected for this function call.
        ///
        /// When `true`, more `partial_args` will arrive in subsequent delta events.
        /// When `false` or absent, this is the final streamed chunk for this call.
        will_continue: Option<bool>,
    },
    /// Function result (input to model with execution result)
    FunctionResult {
        /// Function name (optional per API spec)
        name: Option<String>,
        /// The call_id from the FunctionCall being responded to
        call_id: String,
        result: serde_json::Value,
        /// Indicates if the function execution resulted in an error
        is_error: Option<bool>,
    },
    /// Code execution call (model requesting code execution)
    ///
    /// This variant appears when the model initiates code execution
    /// via the `CodeExecution` built-in tool.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Content, CodeExecutionLanguage};
    /// # let content: Content = todo!();
    /// if let Content::CodeExecutionCall { id, language, code } = content {
    ///     println!("Executing {:?} code (id: {:?}): {}", language, id, code);
    /// }
    /// ```
    CodeExecutionCall {
        /// Unique identifier for this code execution call (optional per API spec)
        id: Option<String>,
        /// Programming language (currently only Python is supported)
        language: CodeExecutionLanguage,
        /// Source code to execute
        code: String,
    },
    /// Code execution result (returned after code runs)
    ///
    /// Contains the result of executed code from the `CodeExecution` tool.
    ///
    /// # Security Note
    ///
    /// When displaying results to end users, check `is_error` first. Error
    /// results may contain stack traces or system information that shouldn't be exposed
    /// directly to users without sanitization.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::Content;
    /// # let content: Content = todo!();
    /// if let Content::CodeExecutionResult { is_error, result, .. } = content {
    ///     if is_error {
    ///         eprintln!("Error: {}", result);
    ///     } else {
    ///         println!("Result: {}", result);
    ///     }
    /// }
    /// ```
    CodeExecutionResult {
        /// The call_id matching the CodeExecutionCall this result is for (optional per API spec)
        call_id: Option<String>,
        /// Whether the code execution resulted in an error
        is_error: bool,
        /// The output of the code execution (stdout for success, error message for failure)
        result: String,
    },
    /// Google Search call (model requesting a search)
    ///
    /// Appears when the model initiates a Google Search via the `GoogleSearch` tool.
    /// The model may execute multiple queries in a single call.
    GoogleSearchCall {
        /// Unique identifier for this search call (used to match with result)
        id: String,
        /// Search queries executed by the model
        queries: Vec<String>,
    },
    /// Google Search result (grounding data from search)
    ///
    /// Contains the results returned by the `GoogleSearch` built-in tool.
    /// Each result includes a title and URL for the source.
    GoogleSearchResult {
        /// ID of the corresponding GoogleSearchCall
        call_id: String,
        /// Search results with source information
        result: Vec<GoogleSearchResultItem>,
    },
    /// URL Context call (model requesting URL content)
    ///
    /// Appears when the model requests URL content via the `UrlContext` tool.
    UrlContextCall {
        /// Unique identifier for this URL context call
        id: String,
        /// URLs to fetch (extracted from arguments.urls in wire format)
        urls: Vec<String>,
    },
    /// URL Context result (fetched content from URL)
    ///
    /// Contains the results from the `UrlContext` built-in tool.
    UrlContextResult {
        /// ID of the corresponding UrlContextCall
        call_id: String,
        /// Results for each URL that was fetched
        result: Vec<UrlContextResultItem>,
    },
    /// File Search result (semantic search results from document stores)
    ///
    /// Contains the results returned by the `FileSearch` built-in tool.
    /// Each result includes the title, extracted text, and source store name.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Content, FileSearchResultItem};
    /// # let content: Content = todo!();
    /// if let Content::FileSearchResult { call_id, result } = content {
    ///     println!("Results for call {}: {} matches", call_id, result.len());
    ///     for item in result {
    ///         println!("  {}: {}", item.title, item.text);
    ///     }
    /// }
    /// ```
    FileSearchResult {
        /// ID of the corresponding file search call
        call_id: String,
        /// Search results with extracted text and source information
        result: Vec<FileSearchResultItem>,
    },
    /// Computer use call (model requesting browser interaction)
    ///
    /// Appears when the model initiates browser automation via the `ComputerUse` tool.
    ///
    /// # Security Considerations
    ///
    /// Computer use calls allow the model to interact with web browsers on your behalf.
    /// Always review calls before execution in production environments, especially when:
    /// - Accessing sensitive websites (banking, admin panels)
    /// - Performing state-changing operations (form submissions, purchases)
    /// - Working with untrusted user input
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::Content;
    /// # let content: Content = todo!();
    /// if let Content::ComputerUseCall { id, action, parameters } = content {
    ///     println!("Browser action '{}' requested (id: {})", action, id);
    ///     println!("Parameters: {:?}", parameters);
    /// }
    /// ```
    ComputerUseCall {
        /// Unique identifier for this computer use call
        id: String,
        /// The browser action to perform (e.g., "navigate", "click", "type")
        action: String,
        /// Action-specific parameters
        parameters: serde_json::Value,
    },
    /// Computer use result (returned after browser interaction)
    ///
    /// Contains the outcome of a browser action executed via the `ComputerUse` tool.
    ///
    /// # Security Note
    ///
    /// Results may contain sensitive information like:
    /// - Screenshots of the current page
    /// - DOM content from visited pages
    /// - Cookie or session data
    ///
    /// Sanitize output before displaying to end users.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::Content;
    /// # let content: Content = todo!();
    /// if let Content::ComputerUseResult { success, output, error, .. } = content {
    ///     if success {
    ///         println!("Action succeeded: {:?}", output);
    ///     } else {
    ///         eprintln!("Action failed: {:?}", error);
    ///     }
    /// }
    /// ```
    ComputerUseResult {
        /// References the `id` field from the corresponding `ComputerUseCall` variant.
        call_id: String,
        /// Whether the action succeeded
        success: bool,
        /// Action output data (may include page content, extracted data, etc.)
        output: Option<serde_json::Value>,
        /// Error message if action failed
        error: Option<String>,
        /// Optional screenshot data (base64-encoded image)
        screenshot: Option<String>,
    },
    /// Unknown content type for forward compatibility.
    ///
    /// This variant captures content types that the library doesn't recognize yet.
    /// This can happen when Google adds new features to the API before this library
    /// is updated to support them.
    ///
    /// The `content_type` field contains the unrecognized type string from the API,
    /// and `data` contains the full JSON object for inspection or debugging.
    ///
    /// # When This Occurs
    ///
    /// - New API features not yet supported by this library
    /// - Beta features in testing
    /// - Region-specific content types
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::Content;
    /// # let content: Content = todo!();
    /// if let Content::Unknown { content_type, data } = content {
    ///     eprintln!("Encountered unknown type '{}': {:?}", content_type, data);
    /// }
    /// ```
    ///
    /// # Serialization Behavior
    ///
    /// Unknown variants can be serialized back to JSON, enabling round-trip in
    /// multi-turn conversations. The serialization follows these rules:
    ///
    /// 1. **Type field**: The `content_type` becomes the `"type"` field in output
    /// 2. **Object data**: If `data` is a JSON object, its fields are flattened
    ///    into the output (excluding any existing `"type"` field to avoid duplicates)
    /// 3. **Non-object data**: If `data` is a non-object value (array, string, etc.),
    ///    it's placed under a `"data"` key
    /// 4. **Null data**: Omitted entirely from the output
    ///
    /// ## Example: Object Data (Common Case)
    ///
    /// ```
    /// # use genai_rs::Content;
    /// # use serde_json::json;
    /// let content = Content::Unknown {
    ///     content_type: "new_feature".to_string(),
    ///     data: json!({"field1": "value1", "field2": 42}),
    /// };
    /// // Serializes to: {"type": "new_feature", "field1": "value1", "field2": 42}
    /// ```
    ///
    /// ## Example: Duplicate Type Field
    ///
    /// If `data` contains a `"type"` field, it's excluded during serialization
    /// (the `content_type` takes precedence):
    ///
    /// ```
    /// # use genai_rs::Content;
    /// # use serde_json::json;
    /// let content = Content::Unknown {
    ///     content_type: "my_type".to_string(),
    ///     data: json!({"type": "ignored", "value": 123}),
    /// };
    /// // Serializes to: {"type": "my_type", "value": 123}
    /// // Note: "type": "ignored" is not included
    /// ```
    ///
    /// ## Example: Non-Object Data
    ///
    /// ```
    /// # use genai_rs::Content;
    /// # use serde_json::json;
    /// let content = Content::Unknown {
    ///     content_type: "array_type".to_string(),
    ///     data: json!([1, 2, 3]),
    /// };
    /// // Serializes to: {"type": "array_type", "data": [1, 2, 3]}
    /// ```
    ///
    /// # Field Ordering
    ///
    /// **Note:** Field ordering is not preserved during round-trip serialization,
    /// but all field **values** are fully preserved. When serializing an `Unknown`
    /// variant, the `"type"` field is always written first, followed by the remaining
    /// fields from `data`. This means the output field order may differ from the
    /// original API response.
    ///
    /// This has **no practical impact** on API compatibility because JSON objects
    /// are inherently unordered per RFC 8259. The Gemini API does not depend on
    /// field ordering.
    ///
    /// If you need to preserve the exact original field ordering (e.g., for logging
    /// or debugging purposes), access the raw `data` field directly via
    /// [`unknown_data()`](Self::unknown_data) instead of re-serializing the variant.
    ///
    /// # Manual Construction
    ///
    /// While Unknown variants are typically created by deserialization, you can
    /// construct them manually for testing or edge cases. Note that:
    ///
    /// - The `content_type` can be any string (including empty, though not recommended)
    /// - The `data` can be any valid JSON value
    /// - For multi-turn conversations, the serialized form must match what the API expects
    Unknown {
        /// The unrecognized type name from the API
        content_type: String,
        /// The full JSON data for this content, preserved for debugging
        data: serde_json::Value,
    },
}

// Custom Serialize implementation for Content.
// This handles the Unknown variant specially by merging content_type into the data.
impl Serialize for Content {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Text { text, annotations } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "text")?;
                if let Some(t) = text {
                    map.serialize_entry("text", t)?;
                }
                if let Some(annots) = annotations
                    && !annots.is_empty()
                {
                    map.serialize_entry("annotations", annots)?;
                }
                map.end()
            }
            Self::Thought { signature } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "thought")?;
                if let Some(s) = signature {
                    map.serialize_entry("signature", s)?;
                }
                map.end()
            }
            Self::ThoughtSignature { signature } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "thought_signature")?;
                map.serialize_entry("signature", signature)?;
                map.end()
            }
            Self::Image {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "image")?;
                if let Some(d) = data {
                    map.serialize_entry("data", d)?;
                }
                if let Some(u) = uri {
                    map.serialize_entry("uri", u)?;
                }
                if let Some(m) = mime_type {
                    map.serialize_entry("mime_type", m)?;
                }
                if let Some(r) = resolution {
                    map.serialize_entry("resolution", r)?;
                }
                map.end()
            }
            Self::Audio {
                data,
                uri,
                mime_type,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "audio")?;
                if let Some(d) = data {
                    map.serialize_entry("data", d)?;
                }
                if let Some(u) = uri {
                    map.serialize_entry("uri", u)?;
                }
                if let Some(m) = mime_type {
                    map.serialize_entry("mime_type", m)?;
                }
                map.end()
            }
            Self::Video {
                data,
                uri,
                mime_type,
                resolution,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "video")?;
                if let Some(d) = data {
                    map.serialize_entry("data", d)?;
                }
                if let Some(u) = uri {
                    map.serialize_entry("uri", u)?;
                }
                if let Some(m) = mime_type {
                    map.serialize_entry("mime_type", m)?;
                }
                if let Some(r) = resolution {
                    map.serialize_entry("resolution", r)?;
                }
                map.end()
            }
            Self::Document {
                data,
                uri,
                mime_type,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "document")?;
                if let Some(d) = data {
                    map.serialize_entry("data", d)?;
                }
                if let Some(u) = uri {
                    map.serialize_entry("uri", u)?;
                }
                if let Some(m) = mime_type {
                    map.serialize_entry("mime_type", m)?;
                }
                map.end()
            }
            Self::FunctionCall {
                id,
                name,
                args,
                partial_args,
                will_continue,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "function_call")?;
                if let Some(i) = id {
                    map.serialize_entry("id", i)?;
                }
                map.serialize_entry("name", name)?;
                map.serialize_entry("arguments", args)?;
                if let Some(pa) = partial_args {
                    map.serialize_entry("partial_args", pa)?;
                }
                if let Some(wc) = will_continue {
                    map.serialize_entry("will_continue", wc)?;
                }
                map.end()
            }
            Self::FunctionResult {
                name,
                call_id,
                result,
                is_error,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "function_result")?;
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
                }
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                if let Some(err) = is_error {
                    map.serialize_entry("is_error", err)?;
                }
                map.end()
            }
            Self::CodeExecutionCall { id, language, code } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "code_execution_call")?;
                if let Some(i) = id {
                    map.serialize_entry("id", i)?;
                }
                // Wire format nests language and code inside arguments object
                let arguments = serde_json::json!({
                    "language": language,
                    "code": code
                });
                map.serialize_entry("arguments", &arguments)?;
                map.end()
            }
            Self::CodeExecutionResult {
                call_id,
                is_error,
                result,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "code_execution_result")?;
                if let Some(cid) = call_id {
                    map.serialize_entry("call_id", cid)?;
                }
                map.serialize_entry("is_error", is_error)?;
                map.serialize_entry("result", result)?;
                map.end()
            }
            Self::GoogleSearchCall { id, queries } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "google_search_call")?;
                map.serialize_entry("id", id)?;
                // Serialize as nested arguments.queries to match API format
                let arguments = serde_json::json!({ "queries": queries });
                map.serialize_entry("arguments", &arguments)?;
                map.end()
            }
            Self::GoogleSearchResult { call_id, result } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "google_search_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                map.end()
            }
            Self::UrlContextCall { id, urls } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "url_context_call")?;
                map.serialize_entry("id", id)?;
                // Wire format nests urls inside arguments object
                let arguments = serde_json::json!({ "urls": urls });
                map.serialize_entry("arguments", &arguments)?;
                map.end()
            }
            Self::UrlContextResult { call_id, result } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "url_context_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                map.end()
            }
            Self::FileSearchResult { call_id, result } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "file_search_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("result", result)?;
                map.end()
            }
            Self::ComputerUseCall {
                id,
                action,
                parameters,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "computer_use_call")?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("action", action)?;
                map.serialize_entry("parameters", parameters)?;
                map.end()
            }
            Self::ComputerUseResult {
                call_id,
                success,
                output,
                error,
                screenshot,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "computer_use_result")?;
                map.serialize_entry("call_id", call_id)?;
                map.serialize_entry("success", success)?;
                if let Some(out) = output {
                    map.serialize_entry("output", out)?;
                }
                if let Some(err) = error {
                    map.serialize_entry("error", err)?;
                }
                if let Some(ss) = screenshot {
                    map.serialize_entry("screenshot", ss)?;
                }
                map.end()
            }
            Self::Unknown { content_type, data } => {
                // For Unknown, merge the content_type into the data object
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", content_type)?;
                // Flatten the data fields into the map if it's an object
                match data {
                    serde_json::Value::Object(obj) => {
                        for (key, value) in obj {
                            if key != "type" {
                                // Don't duplicate the type field
                                map.serialize_entry(key, value)?;
                            }
                        }
                    }
                    // For non-object data (unlikely but possible), preserve under "data" key
                    other if !other.is_null() => {
                        map.serialize_entry("data", other)?;
                    }
                    _ => {} // Null data is omitted
                }
                map.end()
            }
        }
    }
}

impl Content {
    /// Extract the text content, if this is a Text variant with non-empty text.
    ///
    /// Returns `Some` only for `Text` variants with non-empty text.
    /// Returns `None` for all other variants including `Thought`.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text: Some(t), .. } if !t.is_empty() => Some(t),
            _ => None,
        }
    }

    /// Returns annotations if this is Text content with annotations.
    ///
    /// Returns `Some` with a slice of annotations only for `Text` variants that
    /// have non-empty annotations. Returns `None` for all other variants.
    ///
    /// Annotations are typically present when using grounding tools like
    /// `GoogleSearch` or `UrlContext`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Content, Annotation};
    /// # let content: Content = todo!();
    /// if let Some(annotations) = content.annotations() {
    ///     for annotation in annotations {
    ///         println!("Source: {:?} for bytes {}..{}",
    ///             annotation.source,
    ///             annotation.start_index,
    ///             annotation.end_index);
    ///     }
    /// }
    /// ```
    #[must_use]
    pub fn annotations(&self) -> Option<&[Annotation]> {
        match self {
            Self::Text {
                annotations: Some(annots),
                ..
            } if !annots.is_empty() => Some(annots),
            _ => None,
        }
    }

    /// Extract the thought signature, if this is a Thought variant with a signature.
    ///
    /// The signature is a cryptographic value used for verification of the thinking
    /// process. The actual thought text is not exposed in API responses.
    ///
    /// Returns `Some` only for `Thought` variants with a non-empty signature.
    /// Returns `None` for all other variants including `ThoughtSignature`.
    #[must_use]
    pub fn thought_signature(&self) -> Option<&str> {
        match self {
            Self::Thought { signature: Some(s) } if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    /// Check if this is a Text content type.
    #[must_use]
    pub const fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    /// Check if this is a Thought content type.
    #[must_use]
    pub const fn is_thought(&self) -> bool {
        matches!(self, Self::Thought { .. })
    }

    /// Check if this is a ThoughtSignature content type.
    #[must_use]
    pub const fn is_thought_signature(&self) -> bool {
        matches!(self, Self::ThoughtSignature { .. })
    }

    /// Check if this is a FunctionCall content type.
    #[must_use]
    pub const fn is_function_call(&self) -> bool {
        matches!(self, Self::FunctionCall { .. })
    }

    /// Returns `true` if this is an unknown content type.
    ///
    /// Use this to check for content types that the library doesn't recognize.
    /// See [`Content::Unknown`] for more details.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Check if this is a CodeExecutionCall content type.
    #[must_use]
    pub const fn is_code_execution_call(&self) -> bool {
        matches!(self, Self::CodeExecutionCall { .. })
    }

    /// Check if this is a CodeExecutionResult content type.
    #[must_use]
    pub const fn is_code_execution_result(&self) -> bool {
        matches!(self, Self::CodeExecutionResult { .. })
    }

    /// Check if this is a GoogleSearchCall content type.
    #[must_use]
    pub const fn is_google_search_call(&self) -> bool {
        matches!(self, Self::GoogleSearchCall { .. })
    }

    /// Check if this is a GoogleSearchResult content type.
    #[must_use]
    pub const fn is_google_search_result(&self) -> bool {
        matches!(self, Self::GoogleSearchResult { .. })
    }

    /// Check if this is a FileSearchResult content type.
    #[must_use]
    pub const fn is_file_search_result(&self) -> bool {
        matches!(self, Self::FileSearchResult { .. })
    }

    /// Check if this is a UrlContextCall content type.
    #[must_use]
    pub const fn is_url_context_call(&self) -> bool {
        matches!(self, Self::UrlContextCall { .. })
    }

    /// Check if this is a UrlContextResult content type.
    #[must_use]
    pub const fn is_url_context_result(&self) -> bool {
        matches!(self, Self::UrlContextResult { .. })
    }

    /// Check if this is a ComputerUseCall content type.
    #[must_use]
    pub const fn is_computer_use_call(&self) -> bool {
        matches!(self, Self::ComputerUseCall { .. })
    }

    /// Check if this is a ComputerUseResult content type.
    #[must_use]
    pub const fn is_computer_use_result(&self) -> bool {
        matches!(self, Self::ComputerUseResult { .. })
    }

    /// Returns the content type name if this is an unknown content type.
    ///
    /// Returns `None` for known content types.
    #[must_use]
    pub fn unknown_content_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { content_type, .. } => Some(content_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown content type.
    ///
    /// Returns `None` for known content types.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    // =========================================================================
    // Content Constructors
    // =========================================================================
    //
    // These associated functions create Content variants for sending
    // to the API. They replace the standalone functions in interactions_api.rs.

    /// Creates text content.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let content = Content::text("Hello, world!");
    /// assert!(content.is_text());
    /// ```
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: Some(text.into()),
            annotations: None,
        }
    }

    /// Creates thought content with a signature.
    ///
    /// **Note:** Thought content is typically OUTPUT from the model, not user input.
    /// The signature is a cryptographic value for verification. This constructor
    /// is provided for completeness but rarely needed in typical usage.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let thought = Content::thought("signature_value_here");
    /// assert!(thought.is_thought());
    /// ```
    #[must_use]
    pub fn thought(signature: impl Into<String>) -> Self {
        Self::Thought {
            signature: Some(signature.into()),
        }
    }

    /// Creates a function call content with an optional ID.
    ///
    /// Use this when you need to specify a call ID, typically when echoing function calls back
    /// in manual conversation construction.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    /// use serde_json::json;
    ///
    /// let call = Content::function_call_with_id(
    ///     Some("call_123"),
    ///     "get_weather",
    ///     json!({"location": "San Francisco"})
    /// );
    /// assert!(call.is_function_call());
    /// ```
    #[must_use]
    pub fn function_call_with_id(
        id: Option<impl Into<String>>,
        name: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self::FunctionCall {
            id: id.map(|s| s.into()),
            name: name.into(),
            args,
            partial_args: None,
            will_continue: None,
        }
    }

    /// Creates a function call content (without call ID).
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    /// use serde_json::json;
    ///
    /// let call = Content::function_call(
    ///     "get_weather",
    ///     json!({"location": "San Francisco"})
    /// );
    /// assert!(call.is_function_call());
    /// ```
    #[must_use]
    pub fn function_call(name: impl Into<String>, args: serde_json::Value) -> Self {
        Self::function_call_with_id(None::<String>, name, args)
    }

    /// Creates a function result content.
    ///
    /// This is the correct way to send function execution results back to the Interactions API.
    /// The call_id must match the id from the FunctionCall you're responding to.
    ///
    /// # Panics
    ///
    /// Will log a warning if call_id is empty or whitespace-only, as this may cause
    /// API errors when the server tries to match the result to a function call.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    /// use serde_json::json;
    ///
    /// let result = Content::function_result(
    ///     "get_weather",
    ///     "call_abc123",
    ///     json!({"temperature": "72F", "conditions": "sunny"})
    /// );
    /// ```
    #[must_use]
    pub fn function_result(
        name: impl Into<String>,
        call_id: impl Into<String>,
        result: serde_json::Value,
    ) -> Self {
        let function_name = name.into();
        let call_id_str = call_id.into();

        // Validate call_id is not empty
        if call_id_str.trim().is_empty() {
            tracing::warn!(
                "Empty call_id provided for function result '{}'. \
                 This may cause the API to fail to match the result to its function call.",
                function_name
            );
        }

        Self::FunctionResult {
            name: Some(function_name),
            call_id: call_id_str,
            result,
            is_error: None,
        }
    }

    /// Creates function result content indicating an error.
    ///
    /// Use this when function execution fails and you need to report the error
    /// back to the model.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    /// use serde_json::json;
    ///
    /// let error_result = Content::function_result_error(
    ///     "get_weather",
    ///     "call_abc123",
    ///     json!({"error": "API rate limit exceeded"})
    /// );
    /// ```
    #[must_use]
    pub fn function_result_error(
        name: impl Into<String>,
        call_id: impl Into<String>,
        result: serde_json::Value,
    ) -> Self {
        let function_name = name.into();
        let call_id_str = call_id.into();

        Self::FunctionResult {
            name: Some(function_name),
            call_id: call_id_str,
            result,
            is_error: Some(true),
        }
    }

    /// Creates image content from base64-encoded data.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let image = Content::image_data(
    ///     "base64encodeddata...",
    ///     "image/png"
    /// );
    /// ```
    #[must_use]
    pub fn image_data(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
            resolution: None,
        }
    }

    /// Creates image content from base64-encoded data with specified resolution.
    ///
    /// # Resolution Trade-offs
    ///
    /// | Level | Token Cost | Detail |
    /// |-------|-----------|--------|
    /// | Low | Lowest | Basic shapes and colors |
    /// | Medium | Moderate | Standard detail |
    /// | High | Higher | Fine details visible |
    /// | UltraHigh | Highest | Maximum fidelity |
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::{Content, Resolution};
    ///
    /// let image = Content::image_data_with_resolution(
    ///     "base64encodeddata...",
    ///     "image/png",
    ///     Resolution::High
    /// );
    /// ```
    #[must_use]
    pub fn image_data_with_resolution(
        data: impl Into<String>,
        mime_type: impl Into<String>,
        resolution: Resolution,
    ) -> Self {
        Self::Image {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
            resolution: Some(resolution),
        }
    }

    /// Creates image content from a URI.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the image
    /// * `mime_type` - The MIME type (required by the API for URI-based content)
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let image = Content::image_uri(
    ///     "https://example.com/image.png",
    ///     "image/png"
    /// );
    /// ```
    #[must_use]
    pub fn image_uri(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
            resolution: None,
        }
    }

    /// Creates image content from a URI with specified resolution.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::{Content, Resolution};
    ///
    /// let image = Content::image_uri_with_resolution(
    ///     "https://example.com/image.png",
    ///     "image/png",
    ///     Resolution::Low  // Use low resolution to reduce token cost
    /// );
    /// ```
    #[must_use]
    pub fn image_uri_with_resolution(
        uri: impl Into<String>,
        mime_type: impl Into<String>,
        resolution: Resolution,
    ) -> Self {
        Self::Image {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
            resolution: Some(resolution),
        }
    }

    /// Creates audio content from base64-encoded data.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let audio = Content::audio_data(
    ///     "base64encodeddata...",
    ///     "audio/mp3"
    /// );
    /// ```
    #[must_use]
    pub fn audio_data(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
        }
    }

    /// Creates audio content from a URI.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the audio file
    /// * `mime_type` - The MIME type (required by the API for URI-based content)
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let audio = Content::audio_uri(
    ///     "https://example.com/audio.mp3",
    ///     "audio/mp3"
    /// );
    /// ```
    #[must_use]
    pub fn audio_uri(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
        }
    }

    /// Creates video content from base64-encoded data.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let video = Content::video_data(
    ///     "base64encodeddata...",
    ///     "video/mp4"
    /// );
    /// ```
    #[must_use]
    pub fn video_data(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Video {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
            resolution: None,
        }
    }

    /// Creates video content from base64-encoded data with specified resolution.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::{Content, Resolution};
    ///
    /// let video = Content::video_data_with_resolution(
    ///     "base64encodeddata...",
    ///     "video/mp4",
    ///     Resolution::Low  // Use low resolution to reduce token cost for long videos
    /// );
    /// ```
    #[must_use]
    pub fn video_data_with_resolution(
        data: impl Into<String>,
        mime_type: impl Into<String>,
        resolution: Resolution,
    ) -> Self {
        Self::Video {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
            resolution: Some(resolution),
        }
    }

    /// Creates video content from a URI.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the video file
    /// * `mime_type` - The MIME type (required by the API for URI-based content)
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let video = Content::video_uri(
    ///     "https://example.com/video.mp4",
    ///     "video/mp4"
    /// );
    /// ```
    #[must_use]
    pub fn video_uri(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Video {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
            resolution: None,
        }
    }

    /// Creates video content from a URI with specified resolution.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::{Content, Resolution};
    ///
    /// let video = Content::video_uri_with_resolution(
    ///     "https://example.com/video.mp4",
    ///     "video/mp4",
    ///     Resolution::Medium
    /// );
    /// ```
    #[must_use]
    pub fn video_uri_with_resolution(
        uri: impl Into<String>,
        mime_type: impl Into<String>,
        resolution: Resolution,
    ) -> Self {
        Self::Video {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
            resolution: Some(resolution),
        }
    }

    /// Creates document content from base64-encoded data.
    ///
    /// Use this for PDF files and other document formats.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let document = Content::document_data(
    ///     "base64encodeddata...",
    ///     "application/pdf"
    /// );
    /// ```
    #[must_use]
    pub fn document_data(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Document {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
        }
    }

    /// Creates document content from a URI.
    ///
    /// Use this for PDF files and other document formats accessible via URI.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the document
    /// * `mime_type` - The MIME type (required by the API for URI-based content)
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let document = Content::document_uri(
    ///     "https://example.com/document.pdf",
    ///     "application/pdf"
    /// );
    /// ```
    #[must_use]
    pub fn document_uri(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Document {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
        }
    }

    /// Creates content from a URI and MIME type.
    ///
    /// The content type is inferred from the MIME type:
    ///
    /// - `image/*` → [`Content::Image`]
    /// - `audio/*` → [`Content::Audio`]
    /// - `video/*` → [`Content::Video`]
    /// - Other MIME types (including `application/*`, `text/*`) → [`Content::Document`]
    ///
    /// # Arguments
    ///
    /// * `uri` - The file URI (typically from the Files API)
    /// * `mime_type` - The MIME type of the file
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// // Creates Image variant for image MIME types
    /// let image = Content::from_uri_and_mime(
    ///     "files/abc123",
    ///     "image/png"
    /// );
    ///
    /// // Creates Document variant for PDF
    /// let doc = Content::from_uri_and_mime(
    ///     "files/def456",
    ///     "application/pdf"
    /// );
    /// ```
    #[must_use]
    pub fn from_uri_and_mime(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        let uri_str = uri.into();
        let mime_str = mime_type.into();

        // Choose the appropriate content type based on MIME type prefix
        if mime_str.starts_with("image/") {
            Self::Image {
                data: None,
                uri: Some(uri_str),
                mime_type: Some(mime_str),
                resolution: None,
            }
        } else if mime_str.starts_with("audio/") {
            Self::Audio {
                data: None,
                uri: Some(uri_str),
                mime_type: Some(mime_str),
            }
        } else if mime_str.starts_with("video/") {
            Self::Video {
                data: None,
                uri: Some(uri_str),
                mime_type: Some(mime_str),
                resolution: None,
            }
        } else {
            // Default to document for PDFs, text files, and other types
            Self::Document {
                data: None,
                uri: Some(uri_str),
                mime_type: Some(mime_str),
            }
        }
    }

    /// Creates file content from a Files API metadata object.
    ///
    /// Use this to reference files uploaded via the Files API. The content type
    /// is inferred from the file's MIME type (image, audio, video, or document).
    ///
    /// # Arguments
    ///
    /// * `file` - The uploaded file metadata from the Files API
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Content};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.upload_file("video.mp4").await?;
    /// let content = Content::from_file(&file);
    ///
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_content(vec![
    ///         Content::text("Describe this video"),
    ///         content,
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn from_file(file: &crate::http::files::FileMetadata) -> Self {
        Self::from_uri_and_mime(file.uri.clone(), file.mime_type.clone())
    }

    // =========================================================================
    // Builder Methods
    // =========================================================================

    /// Sets the resolution on image or video content.
    ///
    /// This builder method enables fluent chaining for setting resolution:
    ///
    /// ```
    /// use genai_rs::{Content, Resolution};
    ///
    /// let image = Content::image_uri("files/abc123", "image/png")
    ///     .with_resolution(Resolution::High);
    ///
    /// let video = Content::video_uri("files/def456", "video/mp4")
    ///     .with_resolution(Resolution::Low);
    /// ```
    ///
    /// # Resolution Trade-offs
    ///
    /// | Level | Token Cost | Detail Level |
    /// |-------|------------|--------------|
    /// | Low | Lowest | Basic shapes and colors |
    /// | Medium | Moderate | Standard detail (default) |
    /// | High | Higher | Fine details visible |
    /// | UltraHigh | Highest | Maximum fidelity |
    ///
    /// # Behavior on Non-Media Content
    ///
    /// For content types that don't support resolution (Text, Audio, Document,
    /// FunctionCall, etc.), this method logs a warning and returns the content
    /// unchanged.
    #[must_use]
    pub fn with_resolution(self, resolution: Resolution) -> Self {
        match self {
            Self::Image {
                data,
                uri,
                mime_type,
                ..
            } => Self::Image {
                data,
                uri,
                mime_type,
                resolution: Some(resolution),
            },
            Self::Video {
                data,
                uri,
                mime_type,
                ..
            } => Self::Video {
                data,
                uri,
                mime_type,
                resolution: Some(resolution),
            },
            other => {
                tracing::warn!(
                    "with_resolution() called on content type that doesn't support resolution. \
                     Resolution is only applicable to Image and Video content."
                );
                other
            }
        }
    }

    /// Creates a function result from this function call.
    ///
    /// This builder method enables fluent patterns for handling function calls:
    ///
    /// ```
    /// use genai_rs::Content;
    /// use serde_json::json;
    ///
    /// // Given a function call content
    /// let call = Content::function_call_with_id(
    ///     Some("call_123"),
    ///     "get_weather",
    ///     json!({"location": "San Francisco"})
    /// );
    ///
    /// // Execute the function and create the result
    /// let result = call.with_result(json!({"temperature": "72F", "conditions": "sunny"}));
    /// assert!(matches!(result, Content::FunctionResult { .. }));
    /// ```
    ///
    /// # Behavior on Non-FunctionCall Content
    ///
    /// For content types other than `FunctionCall`, this method logs a warning
    /// and returns the original content unchanged.
    ///
    /// # Error Results
    ///
    /// To indicate a function execution error, use [`with_result_error`](Self::with_result_error)
    /// instead.
    #[must_use]
    pub fn with_result(self, result: serde_json::Value) -> Self {
        match &self {
            Self::FunctionCall { id, name, .. } => Self::FunctionResult {
                name: Some(name.clone()),
                call_id: id.clone().unwrap_or_default(),
                result,
                is_error: None,
            },
            _ => {
                tracing::warn!(
                    "with_result() called on non-FunctionCall content type. \
                     This method is only valid for FunctionCall content."
                );
                self
            }
        }
    }

    /// Creates a function error result from this function call.
    ///
    /// Use this when function execution fails and you need to report the error
    /// back to the model.
    ///
    /// ```
    /// use genai_rs::Content;
    /// use serde_json::json;
    ///
    /// let call = Content::function_call_with_id(
    ///     Some("call_123"),
    ///     "get_weather",
    ///     json!({"location": "San Francisco"})
    /// );
    ///
    /// // Report execution error
    /// let error = call.with_result_error(json!({"error": "API rate limit exceeded"}));
    /// ```
    #[must_use]
    pub fn with_result_error(self, result: serde_json::Value) -> Self {
        match &self {
            Self::FunctionCall { id, name, .. } => Self::FunctionResult {
                name: Some(name.clone()),
                call_id: id.clone().unwrap_or_default(),
                result,
                is_error: Some(true),
            },
            _ => {
                tracing::warn!(
                    "with_result_error() called on non-FunctionCall content type. \
                     This method is only valid for FunctionCall content."
                );
                self
            }
        }
    }
}

// Custom Deserialize implementation to handle unknown content types gracefully.
//
// This tries to deserialize known types first, and falls back to Unknown for
// unrecognized types. This provides forward compatibility when Google adds
// new content types to the API.
impl<'de> Deserialize<'de> for Content {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[cfg(feature = "strict-unknown")]
        use serde::de::Error as _;

        // First, deserialize into a raw JSON value
        let value = serde_json::Value::deserialize(deserializer)?;

        // Helper enum for deserializing known types
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum KnownContent {
            Text {
                text: Option<String>,
                #[serde(default)]
                annotations: Option<Vec<Annotation>>,
            },
            Thought {
                signature: Option<String>,
            },
            ThoughtSignature {
                #[serde(default)]
                signature: String,
            },
            Image {
                data: Option<String>,
                uri: Option<String>,
                mime_type: Option<String>,
                resolution: Option<Resolution>,
            },
            Audio {
                data: Option<String>,
                uri: Option<String>,
                mime_type: Option<String>,
            },
            Video {
                data: Option<String>,
                uri: Option<String>,
                mime_type: Option<String>,
                resolution: Option<Resolution>,
            },
            Document {
                data: Option<String>,
                uri: Option<String>,
                mime_type: Option<String>,
            },
            FunctionCall {
                #[serde(default)]
                id: Option<String>,
                // In streaming, content.start events may not include name yet
                #[serde(default)]
                name: Option<String>,
                #[serde(rename = "arguments", default)]
                args: serde_json::Value,
                #[serde(default)]
                partial_args: Option<Vec<PartialArg>>,
                #[serde(default)]
                will_continue: Option<bool>,
            },
            FunctionResult {
                name: Option<String>,
                call_id: String,
                result: serde_json::Value,
                is_error: Option<bool>,
            },
            CodeExecutionCall {
                #[serde(default)]
                id: Option<String>,
                // API returns language/code in the arguments object
                #[serde(default)]
                language: Option<CodeExecutionLanguage>,
                #[serde(default)]
                code: Option<String>,
                // Fallback for old API format
                #[serde(default)]
                arguments: Option<serde_json::Value>,
            },
            CodeExecutionResult {
                #[serde(default)]
                call_id: Option<String>,
                #[serde(default)]
                is_error: Option<bool>,
                #[serde(default)]
                result: Option<String>,
            },
            GoogleSearchCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
            },
            GoogleSearchResult {
                call_id: String,
                #[serde(default)]
                result: Vec<GoogleSearchResultItem>,
            },
            UrlContextCall {
                id: String,
                #[serde(default)]
                arguments: Option<serde_json::Value>,
            },
            UrlContextResult {
                call_id: String,
                #[serde(default)]
                result: Vec<UrlContextResultItem>,
            },
            FileSearchResult {
                call_id: String,
                #[serde(default)]
                result: Vec<FileSearchResultItem>,
            },
            ComputerUseCall {
                id: String,
                action: String,
                #[serde(default)]
                parameters: Option<serde_json::Value>,
            },
            ComputerUseResult {
                call_id: String,
                success: bool,
                #[serde(default)]
                output: Option<serde_json::Value>,
                #[serde(default)]
                error: Option<String>,
                #[serde(default)]
                screenshot: Option<String>,
            },
        }

        // Try to deserialize as a known type
        match serde_json::from_value::<KnownContent>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownContent::Text { text, annotations } => Content::Text { text, annotations },
                KnownContent::Thought { signature } => Content::Thought { signature },
                KnownContent::ThoughtSignature { signature } => {
                    Content::ThoughtSignature { signature }
                }
                KnownContent::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                } => Content::Image {
                    data,
                    uri,
                    mime_type,
                    resolution,
                },
                KnownContent::Audio {
                    data,
                    uri,
                    mime_type,
                } => Content::Audio {
                    data,
                    uri,
                    mime_type,
                },
                KnownContent::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                } => Content::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                },
                KnownContent::Document {
                    data,
                    uri,
                    mime_type,
                } => Content::Document {
                    data,
                    uri,
                    mime_type,
                },
                KnownContent::FunctionCall {
                    id,
                    name,
                    args,
                    partial_args,
                    will_continue,
                } => {
                    // In streaming, content.start may not include name yet;
                    // deltas will fill it in. Use empty string as placeholder.
                    Content::FunctionCall {
                        id,
                        name: name.unwrap_or_default(),
                        args,
                        partial_args,
                        will_continue,
                    }
                }
                KnownContent::FunctionResult {
                    name,
                    call_id,
                    result,
                    is_error,
                } => Content::FunctionResult {
                    name,
                    call_id,
                    result,
                    is_error,
                },
                KnownContent::CodeExecutionCall {
                    id,
                    language,
                    code,
                    arguments,
                } => {
                    // Prefer direct fields, fall back to parsing arguments
                    if let (Some(lang), Some(source)) = (language, code) {
                        Content::CodeExecutionCall {
                            id,
                            language: lang,
                            code: source,
                        }
                    } else if let Some(args) = arguments {
                        // Parse old format: {"language": "PYTHON", "code": "..."}
                        // Code is required - if missing, treat as Unknown per Evergreen philosophy
                        let source = match args.get("code").and_then(|v| v.as_str()) {
                            Some(code) => code.to_string(),
                            None => {
                                tracing::warn!(
                                    "CodeExecutionCall arguments missing required 'code' field for id: {:?}. \
                                     Treating as Unknown variant to preserve data for debugging.",
                                    id
                                );
                                return Ok(Content::Unknown {
                                    content_type: "code_execution_call".to_string(),
                                    data: value.clone(),
                                });
                            }
                        };

                        // Language defaults to Python if missing or malformed (most common case)
                        let lang = match args.get("language") {
                            Some(lang_value) => {
                                match serde_json::from_value::<CodeExecutionLanguage>(
                                    lang_value.clone(),
                                ) {
                                    Ok(lang) => lang,
                                    Err(e) => {
                                        tracing::warn!(
                                            "CodeExecutionCall has invalid language value for id: {:?}, \
                                             defaulting to Python. Parse error: {}",
                                            id,
                                            e
                                        );
                                        CodeExecutionLanguage::Python
                                    }
                                }
                            }
                            None => CodeExecutionLanguage::Python,
                        };

                        Content::CodeExecutionCall {
                            id,
                            language: lang,
                            code: source,
                        }
                    } else {
                        // Malformed CodeExecutionCall - treat as Unknown to preserve data
                        // per Evergreen philosophy (see CLAUDE.md). This avoids silently
                        // degrading to an empty code string which could cause subtle bugs.
                        tracing::warn!(
                            "CodeExecutionCall missing both direct fields and arguments for id: {:?}. \
                             Treating as Unknown variant to preserve data for debugging.",
                            id
                        );
                        Content::Unknown {
                            content_type: "code_execution_call".to_string(),
                            data: value.clone(),
                        }
                    }
                }
                KnownContent::CodeExecutionResult {
                    call_id,
                    is_error,
                    result,
                } => Content::CodeExecutionResult {
                    call_id,
                    // Default to false (success) when is_error is not provided
                    is_error: is_error.unwrap_or(false),
                    result: result.unwrap_or_default(),
                },
                KnownContent::GoogleSearchCall { id, arguments } => {
                    // Extract queries from arguments.queries
                    let queries = arguments
                        .as_ref()
                        .and_then(|args| args.get("queries"))
                        .and_then(|q| q.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    Content::GoogleSearchCall { id, queries }
                }
                KnownContent::GoogleSearchResult { call_id, result } => {
                    Content::GoogleSearchResult { call_id, result }
                }
                KnownContent::UrlContextCall { id, arguments } => {
                    // Extract urls from arguments.urls
                    let urls = arguments
                        .as_ref()
                        .and_then(|args| args.get("urls"))
                        .and_then(|u| u.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();

                    Content::UrlContextCall { id, urls }
                }
                KnownContent::UrlContextResult { call_id, result } => {
                    Content::UrlContextResult { call_id, result }
                }
                KnownContent::FileSearchResult { call_id, result } => {
                    Content::FileSearchResult { call_id, result }
                }
                KnownContent::ComputerUseCall {
                    id,
                    action,
                    parameters,
                } => Content::ComputerUseCall {
                    id,
                    action,
                    parameters: parameters.unwrap_or(serde_json::Value::Null),
                },
                KnownContent::ComputerUseResult {
                    call_id,
                    success,
                    output,
                    error,
                    screenshot,
                } => Content::ComputerUseResult {
                    call_id,
                    success,
                    output,
                    error,
                    screenshot,
                },
            }),
            Err(parse_error) => {
                // Unknown type - extract type name and preserve data
                let content_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                // Log the actual parse error for debugging - this helps distinguish
                // between truly unknown types and malformed known types
                tracing::warn!(
                    "Encountered unknown Content type '{}'. \
                     Parse error: {}. \
                     This may indicate a new API feature or a malformed response. \
                     The content will be preserved in the Unknown variant.",
                    content_type,
                    parse_error
                );

                #[cfg(feature = "strict-unknown")]
                {
                    Err(D::Error::custom(format!(
                        "Unknown Content type '{}'. \
                         Strict mode is enabled via the 'strict-unknown' feature flag. \
                         Either update the library or disable strict mode.",
                        content_type
                    )))
                }

                #[cfg(not(feature = "strict-unknown"))]
                {
                    Ok(Content::Unknown {
                        content_type,
                        data: value,
                    })
                }
            }
        }
    }
}
