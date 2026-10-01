use super::{Annotation, Content, Resolution, VideoProcessing};
use serde::Deserialize;

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
                #[serde(default)]
                processing: Option<VideoProcessing>,
                #[serde(default)]
                name: Option<String>,
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
                    processing,
                    name,
                } => Content::Video {
                    data,
                    uri,
                    mime_type,
                    resolution,
                    processing,
                    name,
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
