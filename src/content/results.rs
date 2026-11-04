use super::ReviewSnippet;
use serde::{Deserialize, Serialize, Serializer};

use crate::wire_fields::ModeledMap;

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
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct GoogleSearchResultItem {
    /// Title of the search result (often the domain name).
    ///
    /// Empty when the wire item omits it — the live API returns items that
    /// carry only `search_suggestions` (verified 2026-07). Empty values are
    /// skipped on serialize so replayed history matches the wire.
    pub title: String,
    /// URL of the source (typically a grounding redirect URL).
    ///
    /// Empty when the wire item omits it; skipped on serialize when empty.
    pub url: String,
    /// The rendered content from the source (if available)
    pub rendered_content: Option<String>,
    /// Search suggestions rendering payload (if provided by the API)
    pub search_suggestions: Option<String>,
    /// Fields not yet modeled, kept for round trip (Evergreen, #491). An
    /// entry named after a modeled field is not written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
            extra: serde_json::Map::new(),
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
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Place {
    /// Name of the place
    pub name: Option<String>,
    /// Formatted address of the place
    pub formatted_address: Option<String>,
    /// Unique identifier for this place
    pub place_id: Option<String>,
    /// URL of the place
    pub url: Option<String>,
    /// Latitude coordinate
    pub lat: Option<f64>,
    /// Longitude coordinate
    pub lng: Option<f64>,
    /// Place type categories
    pub types: Option<Vec<String>>,
    /// Average user rating
    pub rating: Option<f64>,
    /// Total number of user ratings
    pub user_ratings_total: Option<u32>,
    /// Website URL
    pub website: Option<String>,
    /// Phone number
    pub phone_number: Option<String>,
    /// Review snippets supporting this place
    pub review_snippets: Option<Vec<ReviewSnippet>>,
    /// Additional fields not yet modeled (Evergreen forward compatibility).
    /// An entry named after a modeled field is not written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// A single result item from a Google Maps tool response.
///
/// Contains place data and an optional widget context token for rendering
/// interactive map widgets.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct GoogleMapsResultItem {
    /// Place data returned by the Maps tool
    pub places: Option<Vec<Place>>,
    /// Widget context token for rendering interactive map widgets
    pub widget_context_token: Option<String>,
    /// Fields not yet modeled, kept for round trip (Evergreen, #491). An
    /// entry named after a modeled field is not written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct UrlContextResultItem {
    /// The URL that was fetched
    pub url: String,
    /// Status of the fetch operation (e.g., "success", "error", "paywall", "unsafe")
    pub status: String,
    /// Fields not yet modeled, kept for round trip (Evergreen, #491). An
    /// entry named after a modeled field is not written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl UrlContextResultItem {
    /// Creates a new UrlContextResultItem.
    #[must_use]
    pub fn new(url: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            status: status.into(),
            extra: serde_json::Map::new(),
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
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
    /// Fields not yet modeled, kept for round trip (Evergreen, #491). An
    /// entry named after a modeled field is not written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
            extra: serde_json::Map::new(),
        }
    }

    /// Returns `true` if this result has any text content.
    #[must_use]
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}

// =============================================================================
// Serialization
// =============================================================================
//
// Each item derives `Deserialize`, whose flattened `extra` collects the keys
// it does not model. Serializing writes the modeled fields, then the extras
// that do not name one, so a hand-built `extra` cannot shadow a field or
// write its key twice (#491). Each impl destructures the whole struct, so a
// new field fails to compile until it is written here.

impl Serialize for GoogleSearchResultItem {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            title,
            url,
            rendered_content,
            search_suggestions,
            extra,
        } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.field_unless(title.is_empty(), "title", title)?;
        map.field_unless(url.is_empty(), "url", url)?;
        map.opt("rendered_content", rendered_content.as_ref())?;
        map.opt("search_suggestions", search_suggestions.as_ref())?;
        map.end(extra)
    }
}

impl Serialize for Place {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            name,
            formatted_address,
            place_id,
            url,
            lat,
            lng,
            types,
            rating,
            user_ratings_total,
            website,
            phone_number,
            review_snippets,
            extra,
        } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.opt("name", name.as_ref())?;
        map.opt("formatted_address", formatted_address.as_ref())?;
        map.opt("place_id", place_id.as_ref())?;
        map.opt("url", url.as_ref())?;
        map.opt("lat", lat.as_ref())?;
        map.opt("lng", lng.as_ref())?;
        map.opt("types", types.as_ref())?;
        map.opt("rating", rating.as_ref())?;
        map.opt("user_ratings_total", user_ratings_total.as_ref())?;
        map.opt("website", website.as_ref())?;
        map.opt("phone_number", phone_number.as_ref())?;
        map.opt("review_snippets", review_snippets.as_ref())?;
        map.end(extra)
    }
}

impl Serialize for GoogleMapsResultItem {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            places,
            widget_context_token,
            extra,
        } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.opt("places", places.as_ref())?;
        map.opt("widget_context_token", widget_context_token.as_ref())?;
        map.end(extra)
    }
}

impl Serialize for UrlContextResultItem {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self { url, status, extra } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.field("url", url)?;
        map.field("status", status)?;
        map.end(extra)
    }
}

impl Serialize for FileSearchResultItem {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            title,
            text,
            store,
            extra,
        } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.field("title", title)?;
        map.field("text", text)?;
        map.field("file_search_store", store)?;
        map.end(extra)
    }
}
