use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::wire_fields::{Fallback, ModeledMap, WireFields};

// =============================================================================
// Annotations (typed citations)
// =============================================================================

/// A review snippet attached to a place citation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
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
    /// Fields not yet modeled, kept for round trip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
        /// Unmodeled wire fields, kept for round trip ([`Annotation::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
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
        /// Unmodeled wire fields, kept for round trip ([`Annotation::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
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
        /// Unmodeled wire fields, kept for round trip ([`Annotation::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Speaker and style for a span of TTS input text
    /// (`type: "speech_metadata"`).
    ///
    /// Multi-speaker synthesis on `gemini-3.8-flash-tts` requires one per
    /// text turn, naming a speaker from `speech_config`; the older
    /// `Name: line` transcript form is rejected there. Older TTS models
    /// reject these annotations outright (verified live 2026-09-24). Omit the
    /// indices to cover the whole text block; with indices, the spans must
    /// tile the text without gaps. See [`Content::speaker_text`](crate::Content::speaker_text).
    SpeechMetadata {
        /// Speaker name, matching a `speaker` in `speech_config`.
        speaker: Option<String>,
        /// Delivery instruction, e.g. `"whisper"`.
        style: Option<String>,
        /// Start of the span (UTF-8 byte offset, inclusive).
        start_index: Option<usize>,
        /// End of the span (UTF-8 byte offset, exclusive).
        end_index: Option<usize>,
        /// Unmodeled wire fields, kept for round trip ([`Annotation::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Per-word transcription detail (`type: "word_info"`).
    WordInfo {
        /// The word.
        text: Option<String>,
        /// Diarized speaker label.
        speaker: Option<String>,
        /// Start time within the audio, as a duration string (e.g. `"1.2s"`).
        start_offset: Option<String>,
        /// End time within the audio, as a duration string.
        end_offset: Option<String>,
        /// Start of the span (UTF-8 byte offset, inclusive).
        start_index: Option<usize>,
        /// End of the span (UTF-8 byte offset, exclusive).
        end_index: Option<usize>,
        /// Unmodeled wire fields, kept for round trip ([`Annotation::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Unknown annotation type for forward compatibility.
    // Serializes as `"type": annotation_type` merged with the other entries
    // of `data`, so a hand-built unknown annotation still goes out with its
    // type.
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
            extra: serde_json::Map::new(),
        }
    }

    /// Creates a `speech_metadata` annotation covering a whole text block.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Annotation;
    ///
    /// let whisper = Annotation::speech_metadata(None, Some("whisper".into()));
    /// assert_eq!(whisper.start_index(), None);
    /// ```
    #[must_use]
    pub fn speech_metadata(speaker: Option<String>, style: Option<String>) -> Self {
        Self::SpeechMetadata {
            speaker,
            style,
            start_index: None,
            end_index: None,
            extra: serde_json::Map::new(),
        }
    }

    /// The fields of this annotation that the crate does not model, kept so
    /// they round-trip (#491); see [`Content::extra`](crate::Content::extra).
    /// `None` for [`Annotation::Unknown`].
    #[must_use]
    pub fn extra(&self) -> Option<&Map<String, Value>> {
        match self {
            Self::UrlCitation { extra, .. }
            | Self::FileCitation { extra, .. }
            | Self::PlaceCitation { extra, .. }
            | Self::SpeechMetadata { extra, .. }
            | Self::WordInfo { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
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
            Self::SpeechMetadata { start_index, .. } | Self::WordInfo { start_index, .. } => {
                *start_index
            }
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
            Self::SpeechMetadata { end_index, .. } | Self::WordInfo { end_index, .. } => *end_index,
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
            Self::SpeechMetadata { .. } | Self::WordInfo { .. } | Self::Unknown { .. } => None,
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
        match self {
            Self::UrlCitation {
                url,
                title,
                start_index,
                end_index,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "url_citation")?;
                map.opt("url", url.as_ref())?;
                map.opt("title", title.as_ref())?;
                map.field("start_index", start_index)?;
                map.field("end_index", end_index)?;
                map.end(extra)
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
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "file_citation")?;
                map.opt("document_uri", document_uri.as_ref())?;
                map.opt("file_name", file_name.as_ref())?;
                map.opt("source", source.as_ref())?;
                map.opt("custom_metadata", custom_metadata.as_ref())?;
                map.opt("page_number", page_number.as_ref())?;
                map.opt("media_id", media_id.as_ref())?;
                map.field("start_index", start_index)?;
                map.field("end_index", end_index)?;
                map.end(extra)
            }
            Self::PlaceCitation {
                place_id,
                name,
                url,
                review_snippets,
                start_index,
                end_index,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "place_citation")?;
                map.opt("place_id", place_id.as_ref())?;
                map.opt("name", name.as_ref())?;
                map.opt("url", url.as_ref())?;
                map.field_unless(
                    review_snippets.is_empty(),
                    "review_snippets",
                    review_snippets,
                )?;
                map.field("start_index", start_index)?;
                map.field("end_index", end_index)?;
                map.end(extra)
            }
            Self::SpeechMetadata {
                speaker,
                style,
                start_index,
                end_index,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "speech_metadata")?;
                map.opt("speaker", speaker.as_ref())?;
                map.opt("style", style.as_ref())?;
                map.opt("start_index", start_index.as_ref())?;
                map.opt("end_index", end_index.as_ref())?;
                map.end(extra)
            }
            Self::WordInfo {
                text,
                speaker,
                start_offset,
                end_offset,
                start_index,
                end_index,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "word_info")?;
                map.opt("text", text.as_ref())?;
                map.opt("speaker", speaker.as_ref())?;
                map.opt("start_offset", start_offset.as_ref())?;
                map.opt("end_offset", end_offset.as_ref())?;
                map.opt("start_index", start_index.as_ref())?;
                map.opt("end_index", end_index.as_ref())?;
                map.end(extra)
            }
            Self::Unknown {
                annotation_type,
                data,
            } => crate::serde_util::serialize_unknown_merged(annotation_type, data, serializer),
        }
    }
}

/// Reads a known annotation; an error keeps it as [`Annotation::Unknown`].
fn read_known(object: &Map<String, Value>) -> Result<Annotation, Fallback> {
    let mut f = WireFields::tagged(object);
    let Some(annotation_type) = f.tag() else {
        return Err(Fallback::UnknownType);
    };
    let annotation = match annotation_type {
        "url_citation" => Annotation::UrlCitation {
            url: f.optional("url")?,
            title: f.optional("title")?,
            start_index: f.or_default("start_index")?,
            end_index: f.or_default("end_index")?,
            extra: f.extra(),
        },
        "file_citation" => Annotation::FileCitation {
            document_uri: f.optional("document_uri")?,
            file_name: f.optional("file_name")?,
            source: f.optional("source")?,
            custom_metadata: f.optional("custom_metadata")?,
            page_number: f.optional("page_number")?,
            media_id: f.optional("media_id")?,
            start_index: f.or_default("start_index")?,
            end_index: f.or_default("end_index")?,
            extra: f.extra(),
        },
        "place_citation" => Annotation::PlaceCitation {
            place_id: f.optional("place_id")?,
            name: f.optional("name")?,
            url: f.optional("url")?,
            review_snippets: f.or_default("review_snippets")?,
            start_index: f.or_default("start_index")?,
            end_index: f.or_default("end_index")?,
            extra: f.extra(),
        },
        "speech_metadata" => Annotation::SpeechMetadata {
            speaker: f.optional("speaker")?,
            style: f.optional("style")?,
            start_index: f.optional("start_index")?,
            end_index: f.optional("end_index")?,
            extra: f.extra(),
        },
        "word_info" => Annotation::WordInfo {
            text: f.optional("text")?,
            speaker: f.optional("speaker")?,
            start_offset: f.optional("start_offset")?,
            end_offset: f.optional("end_offset")?,
            start_index: f.optional("start_index")?,
            end_index: f.optional("end_index")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    };
    Ok(annotation)
}

impl<'de> Deserialize<'de> for Annotation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        match Fallback::read(&value, read_known) {
            Ok(annotation) => Ok(annotation),
            Err(fallback) => {
                let annotation_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                tracing::warn!(
                    "Keeping an annotation whole in Annotation::Unknown: {}.",
                    fallback.describe("Annotation", &annotation_type)
                );

                Ok(Annotation::Unknown {
                    annotation_type,
                    data: value,
                })
            }
        }
    }
}

#[cfg(test)]
#[path = "annotation_tests.rs"]
mod tests;
