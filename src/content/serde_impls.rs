//! Wire (de)serialization for [`Content`].
//!
//! Known types write their modeled fields and then `extra`, and read with
//! `WireFields`, which leaves the unread keys for `extra` (#491). Unknown
//! types round-trip through [`Content::Unknown`].

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::wire_fields::{ModeledMap, WireFields, as_object};

use super::Content;

impl Serialize for Content {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Text {
                text,
                annotations,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "text")?;
                map.opt("text", text.as_ref())?;
                let empty = annotations.as_ref().is_none_or(Vec::is_empty);
                map.field_unless(empty, "annotations", annotations)?;
                map.end(extra)
            }
            Self::Image {
                data,
                uri,
                mime_type,
                resolution,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "image")?;
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("resolution", resolution.as_ref())?;
                map.end(extra)
            }
            Self::Audio {
                data,
                uri,
                mime_type,
                sample_rate,
                channels,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "audio")?;
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("sample_rate", sample_rate.as_ref())?;
                map.opt("channels", channels.as_ref())?;
                map.end(extra)
            }
            Self::Video {
                data,
                uri,
                mime_type,
                resolution,
                processing,
                name,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "video")?;
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("resolution", resolution.as_ref())?;
                map.opt("processing", processing.as_ref())?;
                map.opt("name", name.as_ref())?;
                map.end(extra)
            }
            Self::Document {
                data,
                uri,
                mime_type,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "document")?;
                map.opt("data", data.as_ref())?;
                map.opt("uri", uri.as_ref())?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.end(extra)
            }
            Self::Unknown {
                content_type, data, ..
            } => crate::serde_util::serialize_unknown_merged(content_type, data, serializer),
        }
    }
}

/// Reads a known content block; an error keeps it as [`Content::Unknown`].
fn read_known(object: &Map<String, Value>) -> Result<Content, serde_json::Error> {
    use serde::de::Error as _;

    let mut f = WireFields::tagged(object);
    let content_type: String = f.required("type")?;
    let content = match content_type.as_str() {
        "text" => Content::Text {
            text: f.optional("text")?,
            annotations: f.optional("annotations")?,
            extra: f.extra(),
        },
        "image" => Content::Image {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            resolution: f.optional("resolution")?,
            extra: f.extra(),
        },
        "audio" => Content::Audio {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            sample_rate: f.optional("sample_rate")?,
            channels: f.optional("channels")?,
            extra: f.extra(),
        },
        "video" => Content::Video {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            resolution: f.optional("resolution")?,
            processing: f.optional("processing")?,
            name: f.optional("name")?,
            extra: f.extra(),
        },
        "document" => Content::Document {
            data: f.optional("data")?,
            uri: f.optional("uri")?,
            mime_type: f.optional("mime_type")?,
            extra: f.extra(),
        },
        other => {
            return Err(serde_json::Error::custom(format!(
                "unknown variant `{other}`"
            )));
        }
    };
    Ok(content)
}

// Known types first, falling back to `Unknown` for unrecognized ones, so a
// content type Google adds does not fail the response.
impl<'de> Deserialize<'de> for Content {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[cfg(feature = "strict-unknown")]
        use serde::de::Error as _;

        let value = Value::deserialize(deserializer)?;

        match as_object(&value).and_then(read_known) {
            Ok(content) => Ok(content),
            Err(parse_error) => {
                let content_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();

                // The parse error distinguishes a truly unknown type from a
                // malformed known one.
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
