use super::ReviewSnippet;
use serde::{Deserialize, Serialize, Serializer};

use crate::wire_fields::ModeledMap;

// =============================================================================
// Google Search Result Item
// =============================================================================

/// A single result from a Google Search.
///
/// Items carry only `search_suggestions`, an HTML rendering of the search
/// suggestions (the google-genai 2.34.0 bindings' `GoogleSearchResult` has no
/// other field, and none other is served). The sources the answer drew on arrive as
/// [`Annotation`](crate::Annotation)s on the model's text instead.
///
/// # Example
///
/// ```no_run
/// # use genai_rs::{Step, GoogleSearchResultItem};
/// # let step: Step = todo!();
/// if let Step::GoogleSearchResult { result, .. } = step {
///     for item in result {
///         if let Some(html) = &item.search_suggestions {
///             println!("{} bytes of search suggestions", html.len());
///         }
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct GoogleSearchResultItem {
    /// Search suggestions rendering payload (HTML).
    pub search_suggestions: Option<String>,
    /// Fields not yet modeled, kept for round trip (Evergreen, #491). An
    /// entry named after a modeled field is not written.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl GoogleSearchResultItem {
    /// Creates an item carrying the given search suggestions payload.
    #[must_use]
    pub fn new(search_suggestions: impl Into<String>) -> Self {
        Self {
            search_suggestions: Some(search_suggestions.into()),
            extra: serde_json::Map::new(),
        }
    }
}

// =============================================================================
// Google Maps Result Types
// =============================================================================

/// Place data returned by the Google Maps tool.
///
/// A place carries its `name`, `place_id` and a Maps `url`, plus
/// `review_snippets` in the google-genai 2.34.0 bindings.
/// Unknown fields from future API additions are preserved via the `extra`
/// field for Evergreen forward compatibility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Place {
    /// Name of the place
    pub name: Option<String>,
    /// Unique identifier for this place
    pub place_id: Option<String>,
    /// URL of the place
    pub url: Option<String>,
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
            search_suggestions,
            extra,
        } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.opt("search_suggestions", search_suggestions.as_ref())?;
        map.end(extra)
    }
}

impl Serialize for Place {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Self {
            name,
            place_id,
            url,
            review_snippets,
            extra,
        } = self;
        let mut map = ModeledMap::untagged(serializer)?;
        map.opt("name", name.as_ref())?;
        map.opt("place_id", place_id.as_ref())?;
        map.opt("url", url.as_ref())?;
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
