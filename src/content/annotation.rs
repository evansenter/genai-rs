use serde::{Deserialize, Serialize};

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
    pub const fn speech_metadata(speaker: Option<String>, style: Option<String>) -> Self {
        Self::SpeechMetadata {
            speaker,
            style,
            start_index: None,
            end_index: None,
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
            Self::SpeechMetadata {
                speaker,
                style,
                start_index,
                end_index,
            } => {
                map.serialize_entry("type", "speech_metadata")?;
                if let Some(s) = speaker {
                    map.serialize_entry("speaker", s)?;
                }
                if let Some(s) = style {
                    map.serialize_entry("style", s)?;
                }
                if let Some(i) = start_index {
                    map.serialize_entry("start_index", i)?;
                }
                if let Some(i) = end_index {
                    map.serialize_entry("end_index", i)?;
                }
            }
            Self::WordInfo {
                text,
                speaker,
                start_offset,
                end_offset,
                start_index,
                end_index,
            } => {
                map.serialize_entry("type", "word_info")?;
                if let Some(t) = text {
                    map.serialize_entry("text", t)?;
                }
                if let Some(s) = speaker {
                    map.serialize_entry("speaker", s)?;
                }
                if let Some(o) = start_offset {
                    map.serialize_entry("start_offset", o)?;
                }
                if let Some(o) = end_offset {
                    map.serialize_entry("end_offset", o)?;
                }
                if let Some(i) = start_index {
                    map.serialize_entry("start_index", i)?;
                }
                if let Some(i) = end_index {
                    map.serialize_entry("end_index", i)?;
                }
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
            SpeechMetadata {
                #[serde(default)]
                speaker: Option<String>,
                #[serde(default)]
                style: Option<String>,
                #[serde(default)]
                start_index: Option<usize>,
                #[serde(default)]
                end_index: Option<usize>,
            },
            WordInfo {
                #[serde(default)]
                text: Option<String>,
                #[serde(default)]
                speaker: Option<String>,
                #[serde(default)]
                start_offset: Option<String>,
                #[serde(default)]
                end_offset: Option<String>,
                #[serde(default)]
                start_index: Option<usize>,
                #[serde(default)]
                end_index: Option<usize>,
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
                KnownAnnotation::SpeechMetadata {
                    speaker,
                    style,
                    start_index,
                    end_index,
                } => Annotation::SpeechMetadata {
                    speaker,
                    style,
                    start_index,
                    end_index,
                },
                KnownAnnotation::WordInfo {
                    text,
                    speaker,
                    start_offset,
                    end_offset,
                    start_index,
                    end_index,
                } => Annotation::WordInfo {
                    text,
                    speaker,
                    start_offset,
                    end_offset,
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
