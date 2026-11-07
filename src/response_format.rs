//! Typed `response_format` union for interaction requests.
//!
//! The API accepts either a single response format object or a list of them
//! (for multi-modality output). Each format is tagged by `type`:
//! `text`, `audio`, `image`, or `video`.
//!
//! For the common JSON-schema case use [`ResponseFormat::json_schema()`], or
//! keep passing a raw `serde_json::Value` schema to
//! [`InteractionBuilder::with_response_format()`](crate::InteractionBuilder::with_response_format) —
//! it converts into `ResponseFormat::Text { mime_type: "application/json", schema }`.
//!
//! See `docs/OUTPUT_MODALITIES.md` for delivery modes and per-modality options.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::request::{ImageAspectRatio, ImageSize};
use crate::wire_enum::wire_enum;
use crate::wire_fields::{Fallback, ModeledMap, WireFields};

wire_enum! {
    /// Delivery mode for generated media output.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"inline"`, `"uri"`.
    pub enum ResponseDelivery {
        /// Media bytes are returned inline (base64) in the response.
        Inline = "inline",
        /// Media is delivered by URI (e.g., a GCS object).
        Uri = "uri",
    }
    unknown(delivery_type, unknown_delivery_type)
}

/// A typed response format, tagged by `type` on the wire.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New output formats may be added in future API versions.
///
/// # Wire Format
///
/// ```json
/// {"type": "text", "mime_type": "application/json", "schema": {...}}
/// {"type": "audio", "mime_type": "audio/mp3", "delivery": "inline", "sample_rate": 24000}
/// {"type": "image", "mime_type": "image/jpeg", "delivery": "uri", "aspect_ratio": "16:9"}
/// {"type": "video", "delivery": "uri", "gcs_uri": "gs://bucket/out", "duration": "8s"}
/// ```
///
/// # Evergreen Pattern
///
/// Unknown `type` tags from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
/// A known format keeps the fields the crate does not model in its `extra`
/// map ([`ResponseFormat::extra`]). In a struct literal, set it to
/// `Default::default()`, or use [`ResponseFormat::json_schema`] /
/// [`ResponseFormat::text_plain`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ResponseFormat {
    /// Text output configuration.
    Text {
        /// MIME type of the text output. Known values: `application/json`,
        /// `text/plain`.
        mime_type: Option<String>,
        /// JSON schema the output must conform to. Only applicable when
        /// `mime_type` is `application/json`.
        schema: Option<serde_json::Value>,
        /// Unmodeled wire fields, kept for round trip
        /// ([`ResponseFormat::extra`]).
        extra: Map<String, Value>,
    },
    /// Audio output configuration.
    ///
    /// Server-side constraints on the Gemini API:
    /// `mime_type` and `delivery` are schema-valid but rejected
    /// ("Audio mime_type is not supported in response_format." /
    /// "Audio delivery mode is not supported."); `sample_rate` is accepted.
    /// Output is returned inline: `audio/wav` from the 3.8 TTS models,
    /// `audio/l16` from older ones.
    Audio {
        /// MIME type of the audio output. Known values: `audio/mp3`,
        /// `audio/ogg_opus`, `audio/l16`, `audio/wav`, `audio/alaw`,
        /// `audio/mulaw`. Rejected by the Gemini API.
        mime_type: Option<String>,
        /// Delivery mode for the audio output. Rejected by the Gemini API
        /// (inline-only).
        delivery: Option<ResponseDelivery>,
        /// Sample rate in Hz.
        sample_rate: Option<i32>,
        /// Bit rate in bits per second. Only applicable for compressed
        /// formats (MP3, Opus).
        bit_rate: Option<i32>,
        /// Unmodeled wire fields, kept for round trip
        /// ([`ResponseFormat::extra`]).
        extra: Map<String, Value>,
    },
    /// Image output configuration.
    ///
    /// Server-side constraints on the Gemini API:
    /// `mime_type` is accepted but only `image/jpeg` is supported;
    /// `delivery` is rejected ("Image delivery mode is not supported.",
    /// inline-only).
    Image {
        /// MIME type of the image output. Known value: `image/jpeg`
        /// (the only value the Gemini API accepts).
        mime_type: Option<String>,
        /// Delivery mode for the image output. Rejected by the Gemini API
        /// (inline-only).
        delivery: Option<ResponseDelivery>,
        /// Aspect ratio for the image output.
        aspect_ratio: Option<ImageAspectRatio>,
        /// Size of the image output.
        image_size: Option<ImageSize>,
        /// Unmodeled wire fields, kept for round trip
        /// ([`ResponseFormat::extra`]).
        extra: Map<String, Value>,
    },
    /// Video output configuration.
    ///
    /// No model reachable through the Interactions API produces video (Veo
    /// models 404 there), so none of these fields is live-verified beyond
    /// server-side validation.
    Video {
        /// Delivery mode for the video output.
        delivery: Option<ResponseDelivery>,
        /// GCS URI to store the video output. Required on Vertex when
        /// `delivery` is `uri`. Rejected on the Gemini API ("not available
        /// on the Gemini API but it is available on the Gemini Enterprise
        /// Agent Platform").
        gcs_uri: Option<String>,
        /// Aspect ratio for the video output. Known values: `16:9`, `9:16`.
        aspect_ratio: Option<ImageAspectRatio>,
        /// Duration for the video output (e.g., `"8s"`).
        duration: Option<String>,
        /// Output resolution; the server validates the value
        /// (`360p`, `720p`, `1080p`, `4k`).
        resolution: Option<VideoResolution>,
        /// Unmodeled wire fields, kept for round trip
        /// ([`ResponseFormat::extra`]).
        extra: Map<String, Value>,
    },
    /// Unknown variant for forward compatibility (Evergreen pattern)
    // Serializes as `data` exactly as captured; `format_type` is ignored
    // (see `serialize_unknown_data`), so a hand-built unknown format goes out
    // without a type unless `data` carries one.
    Unknown {
        /// The unrecognized format type from the API
        format_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

wire_enum! {
    /// Output resolution of generated video (`response_format` `resolution`).
    pub enum VideoResolution {
        /// 360p.
        Sd360p = "360p",
        /// 720p.
        Hd720p = "720p",
        /// 1080p.
        Fhd1080p = "1080p",
        /// 4K.
        Uhd4k = "4k",
    }
    unknown(resolution_type, unknown_resolution_type)
}

impl ResponseFormat {
    /// Creates a `text` format enforcing a JSON schema
    /// (`mime_type: "application/json"`).
    ///
    /// This is the common structured-output form used by
    /// [`InteractionBuilder::with_response_format()`](crate::InteractionBuilder::with_response_format).
    #[must_use]
    pub fn json_schema(schema: serde_json::Value) -> Self {
        Self::Text {
            mime_type: Some("application/json".to_string()),
            schema: Some(schema),
            extra: Map::new(),
        }
    }

    /// Creates a plain-text (`text/plain`) format.
    #[must_use]
    pub fn text_plain() -> Self {
        Self::Text {
            mime_type: Some("text/plain".to_string()),
            schema: None,
            extra: Map::new(),
        }
    }

    /// The fields of this format that the crate does not model, kept so
    /// they round-trip (#491). `None` for [`ResponseFormat::Unknown`].
    #[must_use]
    pub fn extra(&self) -> Option<&Map<String, Value>> {
        match self {
            Self::Text { extra, .. }
            | Self::Audio { extra, .. }
            | Self::Image { extra, .. }
            | Self::Video { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
        }
    }

    /// Mutable access to [`extra`](Self::extra). An entry named after a
    /// modeled field, or `type`, is never written.
    pub fn extra_mut(&mut self) -> Option<&mut Map<String, Value>> {
        match self {
            Self::Text { extra, .. }
            | Self::Audio { extra, .. }
            | Self::Image { extra, .. }
            | Self::Video { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
        }
    }

    /// Returns true if this is an unknown response format.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the format type name if this is an unknown response format.
    #[must_use]
    pub fn unknown_format_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { format_type, .. } => Some(format_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown response format.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

/// Converts a raw JSON value into a [`ResponseFormat`].
///
/// - Objects tagged `{"type": "text" | "audio" | "image" | "video"}` parse
///   as that typed variant.
/// - Any other value is treated as a raw JSON schema and becomes
///   `ResponseFormat::Text { mime_type: "application/json", schema }` —
///   preserving the pre-0.8 `with_response_format(schema_json)` behavior.
impl From<serde_json::Value> for ResponseFormat {
    fn from(value: serde_json::Value) -> Self {
        let tag = value.get("type").and_then(|t| t.as_str());
        if matches!(tag, Some("text" | "audio" | "image" | "video")) {
            // Deserialization only falls back to Unknown, never fails.
            serde_json::from_value(value).expect("ResponseFormat deserialization is infallible")
        } else {
            Self::json_schema(value)
        }
    }
}

/// The modeled fields, then the extras that do not name one (#491).
impl Serialize for ResponseFormat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text {
                mime_type,
                schema,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "text")?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("schema", schema.as_ref())?;
                map.end(extra)
            }
            Self::Audio {
                mime_type,
                delivery,
                sample_rate,
                bit_rate,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "audio")?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("delivery", delivery.as_ref())?;
                map.opt("sample_rate", sample_rate.as_ref())?;
                map.opt("bit_rate", bit_rate.as_ref())?;
                map.end(extra)
            }
            Self::Image {
                mime_type,
                delivery,
                aspect_ratio,
                image_size,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "image")?;
                map.opt("mime_type", mime_type.as_ref())?;
                map.opt("delivery", delivery.as_ref())?;
                map.opt("aspect_ratio", aspect_ratio.as_ref())?;
                map.opt("image_size", image_size.as_ref())?;
                map.end(extra)
            }
            Self::Video {
                delivery,
                gcs_uri,
                aspect_ratio,
                duration,
                resolution,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "video")?;
                map.opt("delivery", delivery.as_ref())?;
                map.opt("gcs_uri", gcs_uri.as_ref())?;
                map.opt("aspect_ratio", aspect_ratio.as_ref())?;
                map.opt("duration", duration.as_ref())?;
                map.opt("resolution", resolution.as_ref())?;
                map.end(extra)
            }
            Self::Unknown { format_type, data } => {
                crate::serde_util::serialize_unknown_data(format_type, data, serializer)
            }
        }
    }
}

/// Reads a known format; an error keeps it as [`ResponseFormat::Unknown`].
fn read_known(object: &Map<String, Value>) -> Result<ResponseFormat, Fallback> {
    let mut f = WireFields::tagged(object);
    let Some(format_type) = f.tag() else {
        return Err(Fallback::UnknownType);
    };
    let format = match format_type {
        "text" => ResponseFormat::Text {
            mime_type: f.optional("mime_type")?,
            schema: f.optional("schema")?,
            extra: f.extra(),
        },
        "audio" => ResponseFormat::Audio {
            mime_type: f.optional("mime_type")?,
            delivery: f.optional("delivery")?,
            sample_rate: f.optional("sample_rate")?,
            bit_rate: f.optional("bit_rate")?,
            extra: f.extra(),
        },
        "image" => ResponseFormat::Image {
            mime_type: f.optional("mime_type")?,
            delivery: f.optional("delivery")?,
            aspect_ratio: f.optional("aspect_ratio")?,
            image_size: f.optional("image_size")?,
            extra: f.extra(),
        },
        "video" => ResponseFormat::Video {
            delivery: f.optional("delivery")?,
            gcs_uri: f.optional("gcs_uri")?,
            aspect_ratio: f.optional("aspect_ratio")?,
            duration: f.optional("duration")?,
            resolution: f.optional("resolution")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    };
    Ok(format)
}

impl<'de> Deserialize<'de> for ResponseFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match Fallback::read(&value, read_known) {
            Ok(format) => Ok(format),
            Err(fallback) => {
                // Unknown/raw shape (the spec also allows a raw schema dict
                // here) - preserve the data intact.
                let format_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing type>")
                    .to_string();
                tracing::warn!(
                    "Keeping a response format whole in ResponseFormat::Unknown: {}.",
                    fallback.describe("ResponseFormat", &format_type)
                );
                Ok(Self::Unknown {
                    format_type,
                    data: value,
                })
            }
        }
    }
}

/// The `response_format` request field union: a single [`ResponseFormat`]
/// or a list of them.
///
/// Built implicitly via
/// [`InteractionBuilder::with_response_format()`](crate::InteractionBuilder::with_response_format)
/// (single) and
/// [`InteractionBuilder::with_response_formats()`](crate::InteractionBuilder::with_response_formats)
/// (list).
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
// One value per request, so boxing `Single` to match `List`'s size would buy
// nothing and break every `Single(format)` pattern.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum ResponseFormatSpec {
    // The variant order is load-bearing for Deserialize: untagged variants
    // are tried in declaration order, and `ResponseFormat` would also accept
    // an array (its tagged sequence form, else `Unknown`), so `List` must
    // come first to claim every array. Anything else fails `List` without
    // parsing and lands in `Single`, which relies on `ResponseFormat`
    // deserialization never failing (it falls back to `Unknown`).
    /// A list of response formats (serialized as an array).
    List(Vec<ResponseFormat>),
    /// A single response format (serialized as a bare object).
    Single(ResponseFormat),
}

impl From<ResponseFormat> for ResponseFormatSpec {
    fn from(format: ResponseFormat) -> Self {
        Self::Single(format)
    }
}

impl From<Vec<ResponseFormat>> for ResponseFormatSpec {
    fn from(formats: Vec<ResponseFormat>) -> Self {
        Self::List(formats)
    }
}

impl From<serde_json::Value> for ResponseFormatSpec {
    fn from(value: serde_json::Value) -> Self {
        Self::Single(value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_response_delivery_wire_roundtrip() {
        for (delivery, wire) in [
            (ResponseDelivery::Inline, "\"inline\""),
            (ResponseDelivery::Uri, "\"uri\""),
        ] {
            assert_eq!(serde_json::to_string(&delivery).unwrap(), wire);
            let parsed: ResponseDelivery = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, delivery);
        }
    }

    #[cfg(not(genai_strict_unknown))]
    #[test]
    fn test_response_delivery_unknown_roundtrip() {
        let unknown: ResponseDelivery = serde_json::from_str("\"multipart\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_delivery_type(), Some("multipart"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"multipart\"");
    }

    #[test]
    fn test_text_format_wire_shape() {
        let format = ResponseFormat::json_schema(json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        }));
        let value = serde_json::to_value(&format).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": {
                    "type": "object",
                    "properties": {"name": {"type": "string"}}
                }
            })
        );
    }

    #[test]
    fn test_audio_format_wire_shape() {
        let format = ResponseFormat::Audio {
            mime_type: Some("audio/mp3".to_string()),
            delivery: Some(ResponseDelivery::Inline),
            sample_rate: Some(24000),
            bit_rate: Some(128_000),
            extra: Default::default(),
        };
        let value = serde_json::to_value(&format).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "audio",
                "mime_type": "audio/mp3",
                "delivery": "inline",
                "sample_rate": 24000,
                "bit_rate": 128000
            })
        );
    }

    #[test]
    fn test_image_format_wire_shape() {
        let format = ResponseFormat::Image {
            mime_type: Some("image/jpeg".to_string()),
            delivery: Some(ResponseDelivery::Uri),
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            image_size: Some(ImageSize::Hd2k),
            extra: Default::default(),
        };
        let value = serde_json::to_value(&format).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "image",
                "mime_type": "image/jpeg",
                "delivery": "uri",
                "aspect_ratio": "16:9",
                "image_size": "2K"
            })
        );
    }

    #[test]
    fn test_video_format_wire_shape() {
        let format = ResponseFormat::Video {
            delivery: Some(ResponseDelivery::Uri),
            gcs_uri: Some("gs://bucket/out".to_string()),
            aspect_ratio: Some(ImageAspectRatio::Portrait9x16),
            duration: Some("8s".to_string()),
            resolution: Some(VideoResolution::Hd720p),
            extra: Default::default(),
        };
        let value = serde_json::to_value(&format).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "video",
                "delivery": "uri",
                "gcs_uri": "gs://bucket/out",
                "aspect_ratio": "9:16",
                "duration": "8s",
                "resolution": "720p"
            })
        );
    }

    #[test]
    fn test_response_format_none_fields_are_omitted() {
        // The Gemini API rejects some of these fields outright (audio
        // `mime_type`/`delivery`), so an unset field must stay off the wire.
        let cases = [
            (
                ResponseFormat::Text {
                    mime_type: None,
                    schema: None,
                    extra: Default::default(),
                },
                r#"{"type":"text"}"#,
            ),
            (
                ResponseFormat::Audio {
                    mime_type: None,
                    delivery: None,
                    sample_rate: None,
                    bit_rate: None,
                    extra: Default::default(),
                },
                r#"{"type":"audio"}"#,
            ),
            (
                ResponseFormat::Image {
                    mime_type: None,
                    delivery: None,
                    aspect_ratio: None,
                    image_size: None,
                    extra: Default::default(),
                },
                r#"{"type":"image"}"#,
            ),
            (
                ResponseFormat::Video {
                    delivery: None,
                    gcs_uri: None,
                    aspect_ratio: None,
                    duration: None,
                    resolution: None,
                    extra: Default::default(),
                },
                r#"{"type":"video"}"#,
            ),
            (
                ResponseFormat::text_plain(),
                r#"{"type":"text","mime_type":"text/plain"}"#,
            ),
            (
                ResponseFormat::Audio {
                    mime_type: None,
                    delivery: None,
                    sample_rate: Some(24000),
                    bit_rate: None,
                    extra: Default::default(),
                },
                r#"{"type":"audio","sample_rate":24000}"#,
            ),
            (
                ResponseFormat::Video {
                    delivery: None,
                    gcs_uri: None,
                    aspect_ratio: None,
                    duration: None,
                    resolution: Some(VideoResolution::Uhd4k),
                    extra: Default::default(),
                },
                r#"{"type":"video","resolution":"4k"}"#,
            ),
        ];
        for (format, wire) in cases {
            assert_eq!(serde_json::to_string(&format).unwrap(), wire);
        }
    }

    #[test]
    fn test_response_format_some_empty_values_are_sent() {
        // Only None is skipped: an empty string or a null schema the caller
        // set explicitly still goes out.
        let format = ResponseFormat::Text {
            mime_type: Some(String::new()),
            schema: Some(serde_json::Value::Null),
            extra: Default::default(),
        };
        assert_eq!(
            serde_json::to_string(&format).unwrap(),
            r#"{"type":"text","mime_type":"","schema":null}"#
        );
    }

    #[test]
    fn test_response_format_key_order() {
        // "type" first, then the fields in declaration order.
        let cases = [
            (
                ResponseFormat::json_schema(json!({"type": "object"})),
                r#"{"type":"text","mime_type":"application/json","schema":{"type":"object"}}"#,
            ),
            (
                ResponseFormat::Audio {
                    mime_type: Some("audio/mp3".to_string()),
                    delivery: Some(ResponseDelivery::Inline),
                    sample_rate: Some(24000),
                    bit_rate: Some(128_000),
                    extra: Default::default(),
                },
                r#"{"type":"audio","mime_type":"audio/mp3","delivery":"inline","sample_rate":24000,"bit_rate":128000}"#,
            ),
            (
                ResponseFormat::Image {
                    mime_type: Some("image/jpeg".to_string()),
                    delivery: Some(ResponseDelivery::Uri),
                    aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
                    image_size: Some(ImageSize::Hd2k),
                    extra: Default::default(),
                },
                r#"{"type":"image","mime_type":"image/jpeg","delivery":"uri","aspect_ratio":"16:9","image_size":"2K"}"#,
            ),
            (
                ResponseFormat::Video {
                    delivery: Some(ResponseDelivery::Uri),
                    gcs_uri: Some("gs://bucket/out".to_string()),
                    aspect_ratio: Some(ImageAspectRatio::Portrait9x16),
                    duration: Some("8s".to_string()),
                    resolution: Some(VideoResolution::Hd720p),
                    extra: Default::default(),
                },
                r#"{"type":"video","delivery":"uri","gcs_uri":"gs://bucket/out","aspect_ratio":"9:16","duration":"8s","resolution":"720p"}"#,
            ),
        ];
        for (format, wire) in cases {
            assert_eq!(serde_json::to_string(&format).unwrap(), wire);
        }
    }

    #[test]
    fn test_response_format_unknown_serializes_data_verbatim() {
        // `format_type` is not merged back in: the wire form is `data`
        // exactly, whatever its shape.
        let unknown = |data: serde_json::Value| {
            serde_json::to_string(&ResponseFormat::Unknown {
                format_type: "hologram".to_string(),
                data,
            })
            .unwrap()
        };
        assert_eq!(
            unknown(json!({"type": "stale", "a": 1})),
            r#"{"a":1,"type":"stale"}"#
        );
        assert_eq!(unknown(json!({"a": 1})), r#"{"a":1}"#);
        assert_eq!(unknown(json!({})), "{}");
        assert_eq!(unknown(json!("raw")), r#""raw""#);
        assert_eq!(unknown(json!([1, 2])), "[1,2]");
        assert_eq!(unknown(serde_json::Value::Null), "null");
    }

    #[test]
    fn test_response_format_roundtrip_all_variants() {
        let formats = vec![
            ResponseFormat::json_schema(json!({"type": "object"})),
            ResponseFormat::text_plain(),
            ResponseFormat::Audio {
                mime_type: Some("audio/wav".to_string()),
                delivery: None,
                sample_rate: Some(16000),
                bit_rate: None,
                extra: Default::default(),
            },
            ResponseFormat::Image {
                mime_type: None,
                delivery: Some(ResponseDelivery::Inline),
                aspect_ratio: Some(ImageAspectRatio::Square),
                image_size: None,
                extra: Default::default(),
            },
            ResponseFormat::Video {
                delivery: Some(ResponseDelivery::Inline),
                gcs_uri: None,
                aspect_ratio: None,
                duration: Some("4s".to_string()),
                resolution: None,
                extra: Default::default(),
            },
        ];
        for format in formats {
            let json = serde_json::to_string(&format).unwrap();
            let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, format, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn test_response_format_unknown_roundtrip() {
        let raw = json!({"type": "hologram", "dimensions": 3});
        let parsed: ResponseFormat = serde_json::from_value(raw.clone()).unwrap();
        assert!(parsed.is_unknown());
        assert_eq!(parsed.unknown_format_type(), Some("hologram"));
        assert_eq!(parsed.unknown_data(), Some(&raw));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    }

    #[test]
    fn test_raw_schema_dict_deserializes_to_unknown_and_roundtrips() {
        // The spec also allows a raw JSON-schema dict as a response format;
        // it has no recognized "type" tag, so it must be preserved intact.
        let raw = json!({"type": "object", "properties": {"x": {"type": "number"}}});
        let parsed: ResponseFormat = serde_json::from_value(raw.clone()).unwrap();
        assert!(parsed.is_unknown());
        assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    }

    #[test]
    fn test_from_value_raw_schema_maps_to_text() {
        // Backward-compatible construction path: a raw schema Value becomes
        // the typed text/application/json format.
        let schema = json!({"type": "object", "properties": {"name": {"type": "string"}}});
        let format: ResponseFormat = schema.clone().into();
        match &format {
            ResponseFormat::Text {
                mime_type,
                schema: s,
                ..
            } => {
                assert_eq!(mime_type.as_deref(), Some("application/json"));
                assert_eq!(s.as_ref(), Some(&schema));
            }
            other => panic!("Expected Text variant, got {other:?}"),
        }
    }

    #[test]
    fn test_from_value_typed_object_parses_as_variant() {
        let value = json!({"type": "audio", "mime_type": "audio/mp3"});
        let format: ResponseFormat = value.into();
        assert!(matches!(format, ResponseFormat::Audio { .. }));
    }

    #[test]
    fn test_response_format_spec_single_vs_list() {
        let single: ResponseFormatSpec = ResponseFormat::text_plain().into();
        let value = serde_json::to_value(&single).unwrap();
        assert!(value.is_object());

        let list: ResponseFormatSpec = vec![
            ResponseFormat::text_plain(),
            ResponseFormat::json_schema(json!({})),
        ]
        .into();
        let value = serde_json::to_value(&list).unwrap();
        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_response_format_spec_roundtrip() {
        let single: ResponseFormatSpec = ResponseFormat::json_schema(json!({"a": 1})).into();
        let json = serde_json::to_string(&single).unwrap();
        let parsed: ResponseFormatSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, single);

        let list: ResponseFormatSpec = vec![ResponseFormat::Video {
            delivery: Some(ResponseDelivery::Uri),
            gcs_uri: Some("gs://b/o".to_string()),
            aspect_ratio: None,
            duration: None,
            resolution: None,
            extra: Default::default(),
        }]
        .into();
        let json = serde_json::to_string(&list).unwrap();
        let parsed: ResponseFormatSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, list);
    }

    #[test]
    fn test_response_format_spec_wire_shape() {
        // Single is the bare format object; List is a plain array, even
        // when empty.
        let single = ResponseFormatSpec::Single(ResponseFormat::text_plain());
        assert_eq!(
            serde_json::to_string(&single).unwrap(),
            r#"{"type":"text","mime_type":"text/plain"}"#
        );
        let empty = ResponseFormatSpec::List(vec![]);
        assert_eq!(serde_json::to_string(&empty).unwrap(), "[]");
        let list = ResponseFormatSpec::List(vec![
            ResponseFormat::text_plain(),
            ResponseFormat::Audio {
                mime_type: None,
                delivery: None,
                sample_rate: Some(24000),
                bit_rate: None,
                extra: Default::default(),
            },
        ]);
        assert_eq!(
            serde_json::to_string(&list).unwrap(),
            r#"[{"type":"text","mime_type":"text/plain"},{"type":"audio","sample_rate":24000}]"#
        );
    }

    #[test]
    fn test_response_format_spec_deserialize_shapes() {
        let parse = |wire: &str| serde_json::from_str::<ResponseFormatSpec>(wire).unwrap();
        let missing = |data: serde_json::Value| ResponseFormat::Unknown {
            format_type: "<missing type>".to_string(),
            data,
        };
        let bare_text = ResponseFormat::Text {
            mime_type: None,
            schema: None,
            extra: Default::default(),
        };

        // Every array is a List, however its elements parse; a nested
        // array is one element, preserved as Unknown.
        assert_eq!(parse("[]"), ResponseFormatSpec::List(vec![]));
        assert_eq!(
            parse("[{}]"),
            ResponseFormatSpec::List(vec![missing(json!({}))])
        );
        assert_eq!(
            parse(r#"[{"type":"text"},{"type":"hologram"}]"#),
            ResponseFormatSpec::List(vec![
                bare_text.clone(),
                ResponseFormat::Unknown {
                    format_type: "hologram".to_string(),
                    data: json!({"type": "hologram"}),
                },
            ])
        );
        assert_eq!(
            parse(r#"[[{"type":"text"}]]"#),
            ResponseFormatSpec::List(vec![missing(json!([{"type": "text"}]))])
        );

        // Anything else is a Single.
        assert_eq!(
            parse(r#"{"type":"text"}"#),
            ResponseFormatSpec::Single(bare_text)
        );
        assert_eq!(
            parse("null"),
            ResponseFormatSpec::Single(missing(serde_json::Value::Null))
        );
        assert_eq!(
            parse(r#""x""#),
            ResponseFormatSpec::Single(missing(json!("x")))
        );
        assert_eq!(parse("3"), ResponseFormatSpec::Single(missing(json!(3))));
    }

    #[test]
    fn test_response_format_known_not_unknown() {
        let format = ResponseFormat::text_plain();
        assert!(!format.is_unknown());
        assert_eq!(format.unknown_format_type(), None);
        assert_eq!(format.unknown_data(), None);
    }

    /// Every known format keeps the fields it does not model and writes them
    /// back after its own (D-001): a stored interaction's `response_format`
    /// used to lose them.
    #[test]
    fn test_every_known_format_keeps_unmodeled_fields() {
        for wire in [
            r#"{"type":"text","mime_type":"text/plain","future_knob":1}"#,
            r#"{"type":"audio","sample_rate":24000,"future_knob":1}"#,
            r#"{"type":"image","mime_type":"image/jpeg","future_knob":1}"#,
            r#"{"type":"video","duration":"8s","future_knob":1}"#,
        ] {
            let format: ResponseFormat = serde_json::from_str(wire).unwrap();
            assert!(!format.is_unknown(), "{wire} read as {format:?}");
            assert_eq!(format.extra().unwrap()["future_knob"], 1, "{wire}");
            assert_eq!(serde_json::to_string(&format).unwrap(), wire);
        }

        // A hand-set entry never shadows a modeled field or the type.
        let mut format = ResponseFormat::text_plain();
        let extra = format.extra_mut().unwrap();
        extra.insert("mime_type".into(), json!("application/json"));
        extra.insert("schema".into(), json!({"type": "object"}));
        extra.insert("type".into(), json!("audio"));
        assert_eq!(
            serde_json::to_string(&format).unwrap(),
            r#"{"type":"text","mime_type":"text/plain"}"#
        );
        let unknown: ResponseFormat = serde_json::from_value(json!({"type": "zz"})).unwrap();
        assert!(unknown.extra().is_none());
    }

    /// A known format with a mistyped field is kept whole as `Unknown`, as
    /// before.
    #[test]
    fn test_a_format_with_a_malformed_field_is_kept_whole() {
        let wire = json!({"type": "audio", "sample_rate": "fast"});
        let format: ResponseFormat = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(format.unknown_format_type(), Some("audio"));
        assert_eq!(serde_json::to_value(&format).unwrap(), wire);
    }
}
