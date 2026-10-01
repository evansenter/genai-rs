use super::ReviewSnippet;
use serde::{Deserialize, Serialize};

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
#[non_exhaustive]
pub struct GoogleSearchResultItem {
    /// Title of the search result (often the domain name).
    ///
    /// Empty when the wire item omits it — the live API returns items that
    /// carry only `search_suggestions` (verified 2026-07). Empty values are
    /// skipped on serialize so replayed history matches the wire.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// URL of the source (typically a grounding redirect URL).
    ///
    /// Empty when the wire item omits it; skipped on serialize when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
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
/// # The Gemini API never populates this
///
/// Verified live 2026-08-16 against a store with indexed, `STATE_ACTIVE`
/// documents that demonstrably grounded the answer:
/// [`has_file_search_results`](crate::InteractionResponse::has_file_search_results)
/// is `true` while
/// [`file_search_results`](crate::InteractionResponse::file_search_results)
/// is empty. The `file_search_result` step arrives with no `result` payload —
/// retrieved chunks are folded into the response text rather than surfaced
/// separately.
///
/// This type is modeled from the spec for forward compatibility, so that a
/// future API that does emit it deserializes without a crate release. Until
/// then, read the answer via
/// [`as_text`](crate::InteractionResponse::as_text). Tracked in
/// [#429](https://github.com/evansenter/genai-rs/issues/429).
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Step, FileSearchResultItem};
/// # let step: Step = todo!();
/// // Written defensively: on today's API this loop body never runs.
/// if let Step::FileSearchResult { result, .. } = step {
///     for item in result {
///         println!("Match from '{}': {}", item.store, item.text);
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
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
