use super::{Annotation, Content, Resolution, VideoProcessing};
use serde::{Deserialize, Serialize};

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
                processing,
                name,
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
                if let Some(p) = processing {
                    map.serialize_entry("processing", p)?;
                }
                if let Some(n) = name {
                    map.serialize_entry("name", n)?;
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
