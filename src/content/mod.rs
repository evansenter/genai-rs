//! Content types for the Interactions API.
//!
//! Under API revision 2026-05-20, [`Content`] models the media block union
//! used inside `user_input` / `model_output` steps: text, image, audio,
//! video, and document. Tool calls, tool results, and thoughts are NOT
//! content — they are typed [`Step`](crate::Step) variants.

mod annotation;
mod results;
mod serde_impls;
mod video_processing;

pub use annotation::{Annotation, ReviewSnippet};
pub use results::{
    FileSearchResultItem, GoogleMapsResultItem, GoogleSearchResultItem, Place, UrlContextResultItem,
};
pub use video_processing::{VideoProcessing, VideoProcessingBuilder};

use crate::wire_enum::wire_enum;

wire_enum! {
    /// Programming language for code execution.
    ///
    /// Currently only Python is supported by the Gemini API.
    ///
    /// # Wire Format
    ///
    /// Lowercase: `"python"`.
    #[derive(Default)]
    pub enum CodeExecutionLanguage {
        /// Python programming language
        #[default]
        Python = "python",
    }
    unknown(language_type, unknown_language_type)
}

wire_enum! {
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
    #[derive(Default)]
    pub enum Resolution {
        /// Lowest token cost, basic shapes and colors
        Low = "low",
        /// Moderate token cost, standard detail (default)
        #[default]
        Medium = "medium",
        /// Higher token cost, fine details visible
        High = "high",
        /// Highest token cost, maximum fidelity
        UltraHigh = "ultra_high",
    }
    unknown(resolution_type, unknown_resolution_type)
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
        /// How the model processes this video for understanding.
        ///
        /// Controls segment clipping, frame-rate sampling, and static vs
        /// agentic processing. Has a large effect on token cost — see
        /// [`VideoProcessing`].
        processing: Option<VideoProcessing>,
        /// Optional label for the video. Accepted on input (verified live
        /// 2026-09-24); no effect on the output was observed.
        name: Option<String>,
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

    /// Creates text spoken by one speaker of a multi-speaker TTS request.
    ///
    /// The text carries an [`Annotation::SpeechMetadata`] naming `speaker`,
    /// which must match a `speaker` in the request's speech configs. Required
    /// for multi-speaker synthesis on `gemini-3.8-flash-tts`; older TTS models
    /// reject the annotation.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Content;
    ///
    /// let turns = vec![
    ///     Content::speaker_text("Alice", "Hello Bob!"),
    ///     Content::speaker_text("Bob", "Hi Alice."),
    /// ];
    /// assert_eq!(turns[0].as_text(), Some("Hello Bob!"));
    /// ```
    #[must_use]
    pub fn speaker_text(speaker: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Text {
            text: Some(text.into()),
            annotations: Some(vec![Annotation::speech_metadata(
                Some(speaker.into()),
                None,
            )]),
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
            processing: None,
            name: None,
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
            processing: None,
            name: None,
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
                processing: None,
                name: None,
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
    pub fn from_file(file: &crate::files::FileMetadata) -> Self {
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
                processing,
                name,
                ..
            } => Self::Video {
                data,
                uri,
                mime_type,
                resolution: Some(resolution),
                processing,
                name,
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

    /// Sets how the model processes this video for understanding.
    ///
    /// Only applicable to [`Content::Video`]; on any other content type this
    /// logs a warning and returns the content unchanged.
    ///
    /// A segment window is what reduces video token cost among the `static`
    /// forms — see [`VideoProcessing`] for the wire forms and the measured
    /// numbers, which the service has revised once already.
    ///
    /// **Note:** content carrying `processing` must be sent inside a
    /// [`Step::UserInput`](crate::Step::UserInput); the bare-content-array
    /// input form is rejected by the API. See [`VideoProcessing`] for details.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::{Content, VideoProcessing};
    ///
    /// // Sample only the first 10 seconds, one frame per second.
    /// let video = Content::video_uri("files/abc123", "video/mp4")
    ///     .with_processing(VideoProcessing::segment().end_offset("10s").fps(1.0).build());
    /// ```
    #[must_use]
    pub fn with_processing(self, processing: VideoProcessing) -> Self {
        match self {
            Self::Video {
                data,
                uri,
                mime_type,
                resolution,
                name,
                ..
            } => Self::Video {
                data,
                uri,
                mime_type,
                resolution,
                processing: Some(processing),
                name,
            },
            other => {
                tracing::warn!(
                    "with_processing() called on content type that doesn't support processing. \
                     Processing is only applicable to Video content."
                );
                other
            }
        }
    }

    /// Sets a label on video content; other content is returned unchanged
    /// with a warning. Accepted by the API (verified live 2026-09-24).
    #[must_use]
    pub fn with_video_name(self, name: impl Into<String>) -> Self {
        match self {
            Self::Video {
                data,
                uri,
                mime_type,
                resolution,
                processing,
                ..
            } => Self::Video {
                data,
                uri,
                mime_type,
                resolution,
                processing,
                name: Some(name.into()),
            },
            other => {
                tracing::warn!("with_video_name() called on non-video content; ignoring.");
                other
            }
        }
    }
}

#[cfg(test)]
mod binding_2_25_tests;

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;
