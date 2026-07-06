//! Content types for the Interactions API.
//!
//! Under API revision 2026-05-20, [`Content`] models the media block union
//! used inside `user_input` / `model_output` steps: text, image, audio,
//! video, and document. Tool calls, tool results, and thoughts are NOT
//! content — they are typed [`Step`](crate::Step) variants.

use serde::{Deserialize, Serialize};
use std::fmt;

// =============================================================================
// Annotations (typed citations)
// =============================================================================

/// A review snippet attached to a place citation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewSnippet {
    /// Title of the review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// URL of the review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Identifier of the review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_id: Option<String>,
}

/// A citation annotation attached to text content.
///
/// Annotations link byte ranges of the text (`start_index..end_index`,
/// UTF-8 byte offsets) to their sources. Revision 2026-05-20 uses a
/// discriminated union over `type`: `url_citation`, `file_citation`, and
/// `place_citation`.
///
/// # Forward Compatibility
///
/// `#[non_exhaustive]`; unrecognized annotation types deserialize into
/// [`Annotation::Unknown`] with the full JSON preserved.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Annotation, InteractionResponse};
/// # let response: InteractionResponse = todo!();
/// let text = response.all_text();
/// for annotation in response.all_annotations() {
///     match annotation {
///         Annotation::UrlCitation { url, title, .. } => {
///             println!("Source: {:?} ({:?})", title, url);
///         }
///         Annotation::Unknown { annotation_type, .. } => {
///             println!("Unknown annotation type: {}", annotation_type);
///         }
///         _ => {}
///     }
///     if let Some(span) = annotation.extract_span(&text) {
///         println!("  Cited text: {}", span);
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Annotation {
    /// A citation of a web source (`type: "url_citation"`).
    UrlCitation {
        /// The cited URL.
        url: Option<String>,
        /// Title of the cited page.
        title: Option<String>,
        /// Start of the cited span (UTF-8 byte offset, inclusive).
        start_index: usize,
        /// End of the cited span (UTF-8 byte offset, exclusive).
        end_index: usize,
    },
    /// A citation of an uploaded/retrieved document (`type: "file_citation"`).
    FileCitation {
        /// URI of the cited document.
        document_uri: Option<String>,
        /// Name of the cited file.
        file_name: Option<String>,
        /// Source store or origin of the file.
        source: Option<String>,
        /// Custom metadata attached to the document.
        custom_metadata: Option<serde_json::Value>,
        /// Page number of the citation, if applicable.
        page_number: Option<u32>,
        /// Media identifier within the document.
        media_id: Option<String>,
        /// Start of the cited span (UTF-8 byte offset, inclusive).
        start_index: usize,
        /// End of the cited span (UTF-8 byte offset, exclusive).
        end_index: usize,
    },
    /// A citation of a Google Maps place (`type: "place_citation"`).
    PlaceCitation {
        /// Google Maps place identifier.
        place_id: Option<String>,
        /// Name of the place.
        name: Option<String>,
        /// URL of the place.
        url: Option<String>,
        /// Review snippets supporting the citation.
        review_snippets: Vec<ReviewSnippet>,
        /// Start of the cited span (UTF-8 byte offset, inclusive).
        start_index: usize,
        /// End of the cited span (UTF-8 byte offset, exclusive).
        end_index: usize,
    },
    /// Unknown annotation type for forward compatibility.
    Unknown {
        /// The unrecognized type name from the API.
        annotation_type: String,
        /// The full JSON data, preserved for debugging and roundtrip.
        data: serde_json::Value,
    },
}

impl Annotation {
    /// Creates a URL citation annotation.
    #[must_use]
    pub fn url_citation(
        url: impl Into<String>,
        title: Option<String>,
        start_index: usize,
        end_index: usize,
    ) -> Self {
        Self::UrlCitation {
            url: Some(url.into()),
            title,
            start_index,
            end_index,
        }
    }

    /// Check if this is an unknown annotation type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the annotation type name if this is an unknown annotation.
    #[must_use]
    pub fn unknown_annotation_type(&self) -> Option<&str> {
        match self {
            Self::Unknown {
                annotation_type, ..
            } => Some(annotation_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown annotation.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Start of the annotated span (UTF-8 byte offset, inclusive).
    ///
    /// Returns `None` for [`Annotation::Unknown`] without a numeric
    /// `start_index` field.
    #[must_use]
    pub fn start_index(&self) -> Option<usize> {
        match self {
            Self::UrlCitation { start_index, .. }
            | Self::FileCitation { start_index, .. }
            | Self::PlaceCitation { start_index, .. } => Some(*start_index),
            Self::Unknown { data, .. } => data
                .get("start_index")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize),
        }
    }

    /// End of the annotated span (UTF-8 byte offset, exclusive).
    #[must_use]
    pub fn end_index(&self) -> Option<usize> {
        match self {
            Self::UrlCitation { end_index, .. }
            | Self::FileCitation { end_index, .. }
            | Self::PlaceCitation { end_index, .. } => Some(*end_index),
            Self::Unknown { data, .. } => data
                .get("end_index")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize),
        }
    }

    /// Returns the primary source identifier for this annotation, if any.
    ///
    /// - `url_citation` → the URL
    /// - `file_citation` → the document URI (or file name)
    /// - `place_citation` → the place URL (or place ID)
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::UrlCitation { url, .. } => url.as_deref(),
            Self::FileCitation {
                document_uri,
                file_name,
                ..
            } => document_uri.as_deref().or(file_name.as_deref()),
            Self::PlaceCitation { url, place_id, .. } => url.as_deref().or(place_id.as_deref()),
            Self::Unknown { .. } => None,
        }
    }

    /// Extracts the annotated substring from the given text.
    ///
    /// Returns `None` if the indices are missing, out of bounds, or don't
    /// fall on valid UTF-8 boundaries.
    ///
    /// # Example
    ///
    /// ```
    /// # use genai_rs::Annotation;
    /// let annotation = Annotation::url_citation("https://example.com", None, 0, 5);
    /// assert_eq!(annotation.extract_span("Hello, world!"), Some("Hello"));
    /// ```
    #[must_use]
    pub fn extract_span<'a>(&self, text: &'a str) -> Option<&'a str> {
        let start = self.start_index()?;
        let end = self.end_index()?;
        let bytes = text.as_bytes();
        if end <= bytes.len() && start <= end {
            std::str::from_utf8(&bytes[start..end]).ok()
        } else {
            None
        }
    }
}

impl Serialize for Annotation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        match self {
            Self::UrlCitation {
                url,
                title,
                start_index,
                end_index,
            } => {
                map.serialize_entry("type", "url_citation")?;
                if let Some(u) = url {
                    map.serialize_entry("url", u)?;
                }
                if let Some(t) = title {
                    map.serialize_entry("title", t)?;
                }
                map.serialize_entry("start_index", start_index)?;
                map.serialize_entry("end_index", end_index)?;
            }
            Self::FileCitation {
                document_uri,
                file_name,
                source,
                custom_metadata,
                page_number,
                media_id,
                start_index,
                end_index,
            } => {
                map.serialize_entry("type", "file_citation")?;
                if let Some(d) = document_uri {
                    map.serialize_entry("document_uri", d)?;
                }
                if let Some(f) = file_name {
                    map.serialize_entry("file_name", f)?;
                }
                if let Some(s) = source {
                    map.serialize_entry("source", s)?;
                }
                if let Some(c) = custom_metadata {
                    map.serialize_entry("custom_metadata", c)?;
                }
                if let Some(p) = page_number {
                    map.serialize_entry("page_number", p)?;
                }
                if let Some(m) = media_id {
                    map.serialize_entry("media_id", m)?;
                }
                map.serialize_entry("start_index", start_index)?;
                map.serialize_entry("end_index", end_index)?;
            }
            Self::PlaceCitation {
                place_id,
                name,
                url,
                review_snippets,
                start_index,
                end_index,
            } => {
                map.serialize_entry("type", "place_citation")?;
                if let Some(p) = place_id {
                    map.serialize_entry("place_id", p)?;
                }
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
                }
                if let Some(u) = url {
                    map.serialize_entry("url", u)?;
                }
                if !review_snippets.is_empty() {
                    map.serialize_entry("review_snippets", review_snippets)?;
                }
                map.serialize_entry("start_index", start_index)?;
                map.serialize_entry("end_index", end_index)?;
            }
            Self::Unknown {
                annotation_type,
                data,
            } => {
                map.serialize_entry("type", annotation_type)?;
                match data {
                    serde_json::Value::Object(obj) => {
                        for (key, value) in obj {
                            if key != "type" {
                                map.serialize_entry(key, value)?;
                            }
                        }
                    }
                    other if !other.is_null() => {
                        map.serialize_entry("data", other)?;
                    }
                    _ => {}
                }
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Annotation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        #[allow(clippy::enum_variant_names)]
        enum KnownAnnotation {
            UrlCitation {
                #[serde(default)]
                url: Option<String>,
                #[serde(default)]
                title: Option<String>,
                #[serde(default)]
                start_index: usize,
                #[serde(default)]
                end_index: usize,
            },
            FileCitation {
                #[serde(default)]
                document_uri: Option<String>,
                #[serde(default)]
                file_name: Option<String>,
                #[serde(default)]
                source: Option<String>,
                #[serde(default)]
                custom_metadata: Option<serde_json::Value>,
                #[serde(default)]
                page_number: Option<u32>,
                #[serde(default)]
                media_id: Option<String>,
                #[serde(default)]
                start_index: usize,
                #[serde(default)]
                end_index: usize,
            },
            PlaceCitation {
                #[serde(default)]
                place_id: Option<String>,
                #[serde(default)]
                name: Option<String>,
                #[serde(default)]
                url: Option<String>,
                #[serde(default)]
                review_snippets: Vec<ReviewSnippet>,
                #[serde(default)]
                start_index: usize,
                #[serde(default)]
                end_index: usize,
            },
        }

        match serde_json::from_value::<KnownAnnotation>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownAnnotation::UrlCitation {
                    url,
                    title,
                    start_index,
                    end_index,
                } => Annotation::UrlCitation {
                    url,
                    title,
                    start_index,
                    end_index,
                },
                KnownAnnotation::FileCitation {
                    document_uri,
                    file_name,
                    source,
                    custom_metadata,
                    page_number,
                    media_id,
                    start_index,
                    end_index,
                } => Annotation::FileCitation {
                    document_uri,
                    file_name,
                    source,
                    custom_metadata,
                    page_number,
                    media_id,
                    start_index,
                    end_index,
                },
                KnownAnnotation::PlaceCitation {
                    place_id,
                    name,
                    url,
                    review_snippets,
                    start_index,
                    end_index,
                } => Annotation::PlaceCitation {
                    place_id,
                    name,
                    url,
                    review_snippets,
                    start_index,
                    end_index,
                },
            }),
            Err(parse_error) => {
                let annotation_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Encountered unknown Annotation type '{}'. Parse error: {}. \
                     The annotation will be preserved in the Unknown variant.",
                    annotation_type,
                    parse_error
                );

                Ok(Annotation::Unknown {
                    annotation_type,
                    data: value,
                })
            }
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
/// # use genai_rs::{Step, GoogleSearchResultItem};
/// # let step: Step = todo!();
/// if let Step::GoogleSearchResult { result, .. } = step {
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
    /// Search suggestions rendering payload (if provided by the API)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_suggestions: Option<String>,
}

impl GoogleSearchResultItem {
    /// Creates a new GoogleSearchResultItem.
    #[must_use]
    pub fn new(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            rendered_content: None,
            search_suggestions: None,
        }
    }

    /// Returns `true` if this result has rendered content.
    #[must_use]
    pub fn has_rendered_content(&self) -> bool {
        self.rendered_content.is_some()
    }
}

// =============================================================================
// Google Maps Result Types
// =============================================================================

/// Place data returned by the Google Maps tool.
///
/// Contains location details like name, address, coordinates, and other
/// place metadata. Unknown fields from future API additions are preserved
/// via the `extra` field for Evergreen forward compatibility.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Place {
    /// Name of the place
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Formatted address of the place
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formatted_address: Option<String>,
    /// Unique identifier for this place
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<String>,
    /// URL of the place
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Latitude coordinate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    /// Longitude coordinate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lng: Option<f64>,
    /// Place type categories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
    /// Average user rating
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    /// Total number of user ratings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_ratings_total: Option<u32>,
    /// Website URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// Phone number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    /// Review snippets supporting this place
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_snippets: Option<Vec<ReviewSnippet>>,
    /// Additional fields not yet modeled (Evergreen forward compatibility)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single result item from a Google Maps tool response.
///
/// Contains place data and an optional widget context token for rendering
/// interactive map widgets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleMapsResultItem {
    /// Place data returned by the Maps tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub places: Option<Vec<Place>>,
    /// Widget context token for rendering interactive map widgets
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widget_context_token: Option<String>,
}

// =============================================================================
// URL Context Result Item
// =============================================================================

/// A single result from a URL Context fetch.
///
/// Contains the status of the URL fetch operation. Known status values under
/// revision 2026-05-20 are `success`, `error`, `paywall`, and `unsafe`.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Step, UrlContextResultItem};
/// # let step: Step = todo!();
/// if let Step::UrlContextResult { result, .. } = step {
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
    /// Status of the fetch operation (e.g., "success", "error", "paywall", "unsafe")
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

    /// Returns `true` if the URL was behind a paywall.
    #[must_use]
    pub fn is_paywall(&self) -> bool {
        self.status == "paywall"
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
/// # use genai_rs::{Step, FileSearchResultItem};
/// # let step: Step = todo!();
/// if let Step::FileSearchResult { result, .. } = step {
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
/// Currently only Python is supported by the Gemini API.
///
/// # Wire Format
///
/// Revision 2026-05-20 uses lowercase: `"python"`. The legacy uppercase
/// `"PYTHON"` is still accepted on deserialization for robustness.
///
/// # Forward Compatibility (Evergreen Philosophy)
///
/// This enum is marked `#[non_exhaustive]`; unknown languages are captured as
/// `CodeExecutionLanguage::Unknown` rather than causing a deserialization
/// error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodeExecutionLanguage {
    /// Python programming language
    #[default]
    Python,
    /// Unknown language (for forward compatibility).
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
            Self::Python => serializer.serialize_str("python"),
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
            // Spec wire format is lowercase; accept legacy uppercase too.
            Some("python") | Some("PYTHON") => Ok(Self::Python),
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
            Self::Python => write!(f, "python"),
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
/// This enum is marked `#[non_exhaustive]`; unknown values are captured as
/// `Resolution::Unknown` rather than causing a deserialization error.
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

/// Content block for the Interactions API (revision 2026-05-20).
///
/// Content is the media union used inside `user_input` and `model_output`
/// steps: `text`, `image`, `audio`, `video`, and `document`. Tool calls, tool
/// results, and thoughts are represented as [`Step`](crate::Step) variants,
/// not content.
///
/// # Forward Compatibility
///
/// This enum is marked `#[non_exhaustive]`; unrecognized content types are
/// captured as [`Content::Unknown`] rather than causing a deserialization
/// error, and roundtrip losslessly.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Content, InteractionResponse};
/// # let response: InteractionResponse = todo!();
/// for content in response.output_contents() {
///     match content {
///         Content::Text { text, .. } => println!("Text: {:?}", text),
///         Content::Image { mime_type, .. } => println!("Image: {:?}", mime_type),
///         Content::Unknown { content_type, .. } => {
///             println!("Unknown content type: {}", content_type);
///         }
///         _ => {}
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Content {
    /// Text content with optional source annotations.
    ///
    /// Annotations are present when grounding tools like `GoogleSearch` or
    /// `UrlContext` provide citation information linking text spans to their
    /// sources.
    Text {
        /// The text content.
        ///
        /// `Option<String>` because streaming may announce a text block
        /// before any text arrives. For non-streaming responses this is
        /// always `Some`.
        text: Option<String>,
        /// Source annotations for portions of the text.
        annotations: Option<Vec<Annotation>>,
    },
    /// Image content
    Image {
        /// Base64-encoded image data.
        data: Option<String>,
        /// URI reference (e.g., Files API URI).
        uri: Option<String>,
        /// MIME type (e.g., `image/png`).
        mime_type: Option<String>,
        /// Processing resolution.
        resolution: Option<Resolution>,
    },
    /// Audio content
    Audio {
        /// Base64-encoded audio data.
        data: Option<String>,
        /// URI reference (e.g., Files API URI).
        uri: Option<String>,
        /// MIME type (e.g., `audio/wav`).
        mime_type: Option<String>,
        /// Sample rate in Hz (e.g., 24000 for TTS output).
        sample_rate: Option<u32>,
        /// Number of audio channels (e.g., 1 for mono).
        channels: Option<u32>,
    },
    /// Video content
    Video {
        /// Base64-encoded video data.
        data: Option<String>,
        /// URI reference (e.g., Files API URI).
        uri: Option<String>,
        /// MIME type (e.g., `video/mp4`).
        mime_type: Option<String>,
        /// Processing resolution.
        resolution: Option<Resolution>,
    },
    /// Document content for file-based inputs.
    ///
    /// PDF (`application/pdf`) is the primary supported format with full vision capabilities
    /// for understanding text, images, charts, and tables. Other formats like TXT, Markdown,
    /// HTML, and XML are processed as plain text only, losing visual structure.
    Document {
        /// Base64-encoded document data.
        data: Option<String>,
        /// URI reference (e.g., Files API URI).
        uri: Option<String>,
        /// MIME type (e.g., `application/pdf`).
        mime_type: Option<String>,
    },
    /// Unknown content type for forward compatibility.
    ///
    /// This variant captures content types that the library doesn't recognize yet.
    /// The `content_type` field contains the unrecognized type string from the API,
    /// and `data` contains the full JSON object for inspection or debugging.
    ///
    /// # Serialization Behavior
    ///
    /// Unknown variants serialize back to JSON with `content_type` as the
    /// `"type"` field and the remaining `data` fields flattened alongside it,
    /// enabling lossless roundtrip in multi-turn conversations. Non-object
    /// `data` is placed under a `"data"` key; null data is omitted.
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
                sample_rate,
                channels,
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
                if let Some(sr) = sample_rate {
                    map.serialize_entry("sample_rate", sr)?;
                }
                if let Some(c) = channels {
                    map.serialize_entry("channels", c)?;
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

    /// Check if this is a Text content type.
    #[must_use]
    pub const fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    /// Check if this is an Image content type.
    #[must_use]
    pub const fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }

    /// Check if this is an Audio content type.
    #[must_use]
    pub const fn is_audio(&self) -> bool {
        matches!(self, Self::Audio { .. })
    }

    /// Check if this is a Video content type.
    #[must_use]
    pub const fn is_video(&self) -> bool {
        matches!(self, Self::Video { .. })
    }

    /// Check if this is a Document content type.
    #[must_use]
    pub const fn is_document(&self) -> bool {
        matches!(self, Self::Document { .. })
    }

    /// Returns `true` if this is an unknown content type.
    ///
    /// Use this to check for content types that the library doesn't recognize.
    /// See [`Content::Unknown`] for more details.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
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
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let image = Content::image_uri("files/abc123", "image/png");
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
    /// let audio = Content::audio_data("base64encodeddata...", "audio/wav");
    /// ```
    #[must_use]
    pub fn audio_data(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio {
            data: Some(data.into()),
            uri: None,
            mime_type: Some(mime_type.into()),
            sample_rate: None,
            channels: None,
        }
    }

    /// Creates audio content from a URI.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let audio = Content::audio_uri("files/abc123", "audio/mp3");
    /// ```
    #[must_use]
    pub fn audio_uri(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio {
            data: None,
            uri: Some(uri.into()),
            mime_type: Some(mime_type.into()),
            sample_rate: None,
            channels: None,
        }
    }

    /// Creates video content from base64-encoded data.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let video = Content::video_data("base64encodeddata...", "video/mp4");
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
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let video = Content::video_uri("files/abc123", "video/mp4");
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
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let doc = Content::document_data("base64encodeddata...", "application/pdf");
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
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let doc = Content::document_uri("files/abc123", "application/pdf");
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
                sample_rate: None,
                channels: None,
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
    /// # Behavior on Non-Media Content
    ///
    /// For content types that don't support resolution (Text, Audio, Document),
    /// this method logs a warning and returns the content unchanged.
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
                #[serde(default)]
                sample_rate: Option<u32>,
                #[serde(default)]
                channels: Option<u32>,
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
        }

        // Try to deserialize as a known type
        match serde_json::from_value::<KnownContent>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownContent::Text { text, annotations } => Content::Text { text, annotations },
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
                    sample_rate,
                    channels,
                } => Content::Audio {
                    data,
                    uri,
                    mime_type,
                    sample_rate,
                    channels,
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
