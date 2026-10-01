use serde::{Deserialize, Serialize};
use std::fmt;

/// How the model processes a video for understanding
/// (wire: `processing` on `video` content).
///
/// This is a union of a bare mode string and an object carrying segment and
/// frame-rate options.
///
/// # What actually reduces token cost
///
/// **The segment window is the cost lever among the `static` forms.**
/// Re-measured 2026-08-18 against `gemini-3.7-flash`, same source video:
///
/// | `processing` | Video input tokens |
/// |--------------|--------------------|
/// | *(field omitted)* | 57,778 |
/// | `"static"` | 57,778 |
/// | `{"type": "static"}` | 57,778 |
/// | `{"type": "static", "fps": 1}` | 57,778 |
/// | `{"type": "static", "start_offset": "5s", "end_offset": "10s", "fps": 1}` | **16,198** |
/// | `"agentic"` | *no video modality* — billed as `image`: 2,112 and 4,158 on two runs |
///
/// So a window is what moves the count among the `static` forms; `fps` on
/// its own did not. Reach for [`VideoProcessing::segment()`] with an
/// explicit window when cost matters.
///
/// Both numbers moved since the 2026-08-16 measurement, and the difference
/// is worth recording because it bounds how much these figures are worth.
/// The window's saving was ~127x then (455 vs 57,775) and is ~3.6x now, on
/// the same video and model — the unclipped side barely moved, so it is the
/// clipped accounting the service revised. And `"agentic"` no longer
/// ingests video at all: it reports `image` tokens, in a quantity that
/// varies run to run, which makes it the cheapest mode rather than one
/// equivalent to `static` as previously recorded.
///
/// Treat the table as a dated observation, not a contract. What has held
/// across both measurements is that a window reduces ingestion among the
/// `static` forms. What has not: the magnitudes, and the earlier conclusion
/// that mode selection is never a lever — `"agentic"` is now the cheapest
/// option of all, below even the clipped window.
///
/// # Wire forms
///
/// | Variant | JSON |
/// |---------|------|
/// | [`Static`](Self::Static) | `"static"` |
/// | [`Agentic`](Self::Agentic) | `"agentic"` |
/// | [`StaticSegment`](Self::StaticSegment) | `{"type": "static", "start_offset": "10.5s", "end_offset": "30s", "fps": 1.0}` |
///
/// [`Static`](Self::Static) and [`StaticSegment`](Self::StaticSegment) are kept
/// distinct so each wire form round-trips to the form it arrived in, rather
/// than normalizing a bare `"static"` into an empty object or vice versa.
///
/// # The video must be inside a `user_input` step
///
/// The API accepts `processing` only when the video content sits inside a
/// [`Step::UserInput`](crate::Step::UserInput). Sending the same content
/// through the bare-content-array input form is rejected:
///
/// ```text
/// 400 Unknown parameter 'processing' at 'input[1]'.
/// ```
///
/// The crate always sends content input in the step form, so
/// [`with_content`](crate::InteractionBuilder::with_content) and
/// [`InteractionInput::Content`](crate::InteractionInput::Content) both work;
/// nothing needs wrapping by hand.
///
/// # Example
///
/// ```
/// use genai_rs::{Client, Content, VideoProcessing};
///
/// # fn example(client: &Client) {
/// // Clip a 5-second window and sample one frame per second.
/// let clipped = VideoProcessing::segment()
///     .start_offset("5s")
///     .end_offset("10s")
///     .fps(1.0)
///     .build();
///
/// let video = Content::video_uri("files/abc123", "video/mp4").with_processing(clipped);
///
/// let builder = client
///     .interaction()
///     .with_model(genai_rs::DEFAULT_MODEL)
///     .with_content(vec![Content::text("Describe this clip."), video]);
/// # let _ = builder;
/// # }
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum VideoProcessing {
    /// Default frame sampling (wire: the bare string `"static"`).
    Static,
    /// Model-driven exploration of the video (wire: the bare string
    /// `"agentic"`).
    ///
    /// **No longer reports video tokens at all.** As of 2026-08-18 this bills
    /// as `image`, in a quantity that varies run to run (2,112 and 4,158 on
    /// two consecutive runs) — which made it the cheapest of the measured
    /// options, below even a clipped window. It consumed the same count as
    /// [`Static`](Self::Static) when first measured on 2026-08-16, so treat
    /// this as a moving figure and see the token table on
    /// [`VideoProcessing`].
    Agentic,
    /// Static processing with an explicit segment window and/or frame rate
    /// (wire: `{"type": "static", ...}`).
    ///
    /// Build with [`VideoProcessing::segment()`].
    StaticSegment {
        /// Segment start time, as a decimal number of seconds with an `s`
        /// suffix (e.g. `"10.5s"`). Must be non-negative.
        start_offset: Option<String>,
        /// Segment end time, same format as `start_offset`. Must be greater
        /// than `start_offset` when both are set.
        end_offset: Option<String>,
        /// Video frame-rate sampling density.
        fps: Option<f64>,
    },
    /// Unknown processing mode (for forward compatibility).
    ///
    /// The `processing_type` field contains the unrecognized mode string (or a
    /// descriptor for a non-string value), and `data` contains the full JSON
    /// value preserved for round-trip.
    Unknown {
        /// The unrecognized processing mode from the API.
        processing_type: String,
        /// The raw JSON value, preserved for debugging and round-trip.
        data: serde_json::Value,
    },
}

impl VideoProcessing {
    /// Starts building a [`StaticSegment`](Self::StaticSegment).
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::VideoProcessing;
    ///
    /// let processing = VideoProcessing::segment().start_offset("5s").fps(2.0).build();
    /// ```
    #[must_use]
    pub const fn segment() -> VideoProcessingBuilder {
        VideoProcessingBuilder {
            start_offset: None,
            end_offset: None,
            fps: None,
        }
    }

    /// Check if this is an unknown processing mode.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the processing type name if this is an unknown mode.
    ///
    /// Returns `None` for known modes.
    #[must_use]
    pub fn unknown_processing_type(&self) -> Option<&str> {
        match self {
            Self::Unknown {
                processing_type, ..
            } => Some(processing_type),
            _ => None,
        }
    }

    /// Returns the raw JSON data if this is an unknown mode.
    ///
    /// Returns `None` for known modes.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

/// Builder for [`VideoProcessing::StaticSegment`].
///
/// Created by [`VideoProcessing::segment()`].
#[derive(Clone, Debug, Default)]
pub struct VideoProcessingBuilder {
    start_offset: Option<String>,
    end_offset: Option<String>,
    fps: Option<f64>,
}

impl VideoProcessingBuilder {
    /// Sets the segment start time (e.g. `"10.5s"`).
    #[must_use]
    pub fn start_offset(mut self, start_offset: impl Into<String>) -> Self {
        self.start_offset = Some(start_offset.into());
        self
    }

    /// Sets the segment end time (e.g. `"30s"`).
    #[must_use]
    pub fn end_offset(mut self, end_offset: impl Into<String>) -> Self {
        self.end_offset = Some(end_offset.into());
        self
    }

    /// Sets the frame-rate sampling density.
    #[must_use]
    pub const fn fps(mut self, fps: f64) -> Self {
        self.fps = Some(fps);
        self
    }

    /// Builds the [`VideoProcessing::StaticSegment`].
    #[must_use]
    pub fn build(self) -> VideoProcessing {
        VideoProcessing::StaticSegment {
            start_offset: self.start_offset,
            end_offset: self.end_offset,
            fps: self.fps,
        }
    }
}

impl Serialize for VideoProcessing {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        match self {
            Self::Static => serializer.serialize_str("static"),
            Self::Agentic => serializer.serialize_str("agentic"),
            Self::StaticSegment {
                start_offset,
                end_offset,
                fps,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "static")?;
                if let Some(s) = start_offset {
                    map.serialize_entry("start_offset", s)?;
                }
                if let Some(e) = end_offset {
                    map.serialize_entry("end_offset", e)?;
                }
                if let Some(f) = fps {
                    map.serialize_entry("fps", f)?;
                }
                map.end()
            }
            Self::Unknown { data, .. } => data.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for VideoProcessing {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        // Bare mode string: "static" | "agentic".
        if let Some(mode) = value.as_str() {
            return Ok(match mode {
                "static" => Self::Static,
                "agentic" => Self::Agentic,
                other => {
                    tracing::warn!(
                        "Encountered unknown VideoProcessing mode '{}'. \
                         This may indicate a new API feature. \
                         The mode will be preserved in the Unknown variant.",
                        other
                    );
                    Self::Unknown {
                        processing_type: other.to_string(),
                        data: value,
                    }
                }
            });
        }

        // Object form: {"type": "static", start_offset, end_offset, fps}.
        if let Some(obj) = value.as_object() {
            let tag = obj.get("type").and_then(serde_json::Value::as_str);
            if tag == Some("static") {
                return Ok(Self::StaticSegment {
                    start_offset: obj
                        .get("start_offset")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string),
                    end_offset: obj
                        .get("end_offset")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string),
                    fps: obj.get("fps").and_then(serde_json::Value::as_f64),
                });
            }

            let processing_type = tag.unwrap_or("<missing type>").to_string();
            tracing::warn!(
                "Encountered unknown VideoProcessing object type '{}'. \
                 This may indicate a new API feature. \
                 The value will be preserved in the Unknown variant.",
                processing_type
            );
            return Ok(Self::Unknown {
                processing_type,
                data: value,
            });
        }

        let processing_type = format!("<non-string: {}>", value);
        tracing::warn!(
            "VideoProcessing received an unsupported value: {}. \
             Preserving in Unknown variant.",
            value
        );
        Ok(Self::Unknown {
            processing_type,
            data: value,
        })
    }
}

impl fmt::Display for VideoProcessing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static | Self::StaticSegment { .. } => write!(f, "static"),
            Self::Agentic => write!(f, "agentic"),
            Self::Unknown {
                processing_type, ..
            } => write!(f, "{}", processing_type),
        }
    }
}
