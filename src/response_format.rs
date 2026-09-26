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

use serde::{Deserialize, Deserializer, Serialize};

use crate::request::{ImageAspectRatio, ImageSize};
use crate::wire_enum::wire_enum;

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
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResponseFormat {
    /// Text output configuration.
    Text {
        /// MIME type of the text output. Known values: `application/json`,
        /// `text/plain`.
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        /// JSON schema the output must conform to. Only applicable when
        /// `mime_type` is `application/json`.
        #[serde(skip_serializing_if = "Option::is_none")]
        schema: Option<serde_json::Value>,
    },
    /// Audio output configuration.
    ///
    /// Server-side constraints (verified live 2026-07 on the Gemini API):
    /// `mime_type` and `delivery` are schema-valid but rejected
    /// ("Audio mime_type is not supported in response_format." /
    /// "Audio delivery mode is not supported."); `sample_rate` is accepted.
    /// Output is returned inline: `audio/wav` from the 3.8 TTS models,
    /// `audio/l16` from older ones (2026-09-24).
    Audio {
        /// MIME type of the audio output. Known values: `audio/mp3`,
        /// `audio/ogg_opus`, `audio/l16`, `audio/wav`, `audio/alaw`,
        /// `audio/mulaw`. Rejected by the Gemini API as of 2026-07.
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        /// Delivery mode for the audio output. Rejected by the Gemini API
        /// as of 2026-07 (inline-only).
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery: Option<ResponseDelivery>,
        /// Sample rate in Hz.
        #[serde(skip_serializing_if = "Option::is_none")]
        sample_rate: Option<i32>,
        /// Bit rate in bits per second. Only applicable for compressed
        /// formats (MP3, Opus).
        #[serde(skip_serializing_if = "Option::is_none")]
        bit_rate: Option<i32>,
    },
    /// Image output configuration.
    ///
    /// Server-side constraints (verified live 2026-07 on the Gemini API):
    /// `mime_type` is accepted but only `image/jpeg` is supported;
    /// `delivery` is rejected ("Image delivery mode is not supported.",
    /// inline-only).
    Image {
        /// MIME type of the image output. Known value: `image/jpeg`
        /// (the only value the Gemini API accepts as of 2026-07).
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        /// Delivery mode for the image output. Rejected by the Gemini API
        /// as of 2026-07 (inline-only).
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery: Option<ResponseDelivery>,
        /// Aspect ratio for the image output.
        #[serde(skip_serializing_if = "Option::is_none")]
        aspect_ratio: Option<ImageAspectRatio>,
        /// Size of the image output.
        #[serde(skip_serializing_if = "Option::is_none")]
        image_size: Option<ImageSize>,
    },
    /// Video output configuration.
    ///
    /// No model reachable through the Interactions API produces video as of
    /// 2026-09-24 (Veo models 404 there), so none of these fields is
    /// live-verified beyond server-side validation.
    Video {
        /// Delivery mode for the video output.
        #[serde(skip_serializing_if = "Option::is_none")]
        delivery: Option<ResponseDelivery>,
        /// GCS URI to store the video output. Required on Vertex when
        /// `delivery` is `uri`. Rejected on the Gemini API (2026-07:
        /// "not available on the Gemini API but it is available on the
        /// Gemini Enterprise Agent Platform").
        #[serde(skip_serializing_if = "Option::is_none")]
        gcs_uri: Option<String>,
        /// Aspect ratio for the video output. Known values: `16:9`, `9:16`.
        #[serde(skip_serializing_if = "Option::is_none")]
        aspect_ratio: Option<ImageAspectRatio>,
        /// Duration for the video output (e.g., `"8s"`).
        #[serde(skip_serializing_if = "Option::is_none")]
        duration: Option<String>,
        /// Output resolution; the server validates the value
        /// (`360p`, `720p`, `1080p`, `4k`).
        #[serde(skip_serializing_if = "Option::is_none")]
        resolution: Option<VideoResolution>,
    },
    /// Unknown variant for forward compatibility (Evergreen pattern)
    // Serializes as `data` exactly as captured; `format_type` is ignored
    // (see `serialize_unknown_data`), so a hand-built unknown format goes out
    // without a type unless `data` carries one.
    #[serde(untagged, serialize_with = "crate::serde_util::serialize_unknown_data")]
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
        }
    }

    /// Creates a plain-text (`text/plain`) format.
    #[must_use]
    pub fn text_plain() -> Self {
        Self::Text {
            mime_type: Some("text/plain".to_string()),
            schema: None,
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

impl<'de> Deserialize<'de> for ResponseFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        #[serde(tag = "type")]
        enum KnownFormat {
            #[serde(rename = "text")]
            Text {
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                schema: Option<serde_json::Value>,
            },
            #[serde(rename = "audio")]
            Audio {
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                delivery: Option<ResponseDelivery>,
                #[serde(default)]
                sample_rate: Option<i32>,
                #[serde(default)]
                bit_rate: Option<i32>,
            },
            #[serde(rename = "image")]
            Image {
                #[serde(default)]
                mime_type: Option<String>,
                #[serde(default)]
                delivery: Option<ResponseDelivery>,
                #[serde(default)]
                aspect_ratio: Option<ImageAspectRatio>,
                #[serde(default)]
                image_size: Option<ImageSize>,
            },
            #[serde(rename = "video")]
            Video {
                #[serde(default)]
                delivery: Option<ResponseDelivery>,
                #[serde(default)]
                gcs_uri: Option<String>,
                #[serde(default)]
                aspect_ratio: Option<ImageAspectRatio>,
                #[serde(default)]
                duration: Option<String>,
                #[serde(default)]
                resolution: Option<VideoResolution>,
            },
        }

        match serde_json::from_value::<KnownFormat>(value.clone()) {
            Ok(known) => Ok(match known {
                KnownFormat::Text { mime_type, schema } => Self::Text { mime_type, schema },
                KnownFormat::Audio {
                    mime_type,
                    delivery,
                    sample_rate,
                    bit_rate,
                } => Self::Audio {
                    mime_type,
                    delivery,
                    sample_rate,
                    bit_rate,
                },
                KnownFormat::Image {
                    mime_type,
                    delivery,
                    aspect_ratio,
                    image_size,
                } => Self::Image {
                    mime_type,
                    delivery,
                    aspect_ratio,
                    image_size,
                },
                KnownFormat::Video {
                    delivery,
                    gcs_uri,
                    aspect_ratio,
                    duration,
                    resolution,
                } => Self::Video {
                    delivery,
                    gcs_uri,
                    aspect_ratio,
                    duration,
                    resolution,
                },
            }),
            Err(parse_error) => {
                // Unknown/raw shape (the spec also allows a raw schema dict
                // here) - preserve the data intact.
                let format_type = value
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing type>")
                    .to_string();
                tracing::warn!(
                    "Encountered unknown ResponseFormat type '{}'. \
                     Parse error: {}. \
                     The format will be preserved in the Unknown variant.",
                    format_type,
                    parse_error
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

    #[cfg(not(feature = "strict-unknown"))]
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
                },
                r#"{"type":"text"}"#,
            ),
            (
                ResponseFormat::Audio {
                    mime_type: None,
                    delivery: None,
                    sample_rate: None,
                    bit_rate: None,
                },
                r#"{"type":"audio"}"#,
            ),
            (
                ResponseFormat::Image {
                    mime_type: None,
                    delivery: None,
                    aspect_ratio: None,
                    image_size: None,
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
                },
                r#"{"type":"audio","mime_type":"audio/mp3","delivery":"inline","sample_rate":24000,"bit_rate":128000}"#,
            ),
            (
                ResponseFormat::Image {
                    mime_type: Some("image/jpeg".to_string()),
                    delivery: Some(ResponseDelivery::Uri),
                    aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
                    image_size: Some(ImageSize::Hd2k),
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
            },
            ResponseFormat::Image {
                mime_type: None,
                delivery: Some(ResponseDelivery::Inline),
                aspect_ratio: Some(ImageAspectRatio::Square),
                image_size: None,
            },
            ResponseFormat::Video {
                delivery: Some(ResponseDelivery::Inline),
                gcs_uri: None,
                aspect_ratio: None,
                duration: Some("4s".to_string()),
                resolution: None,
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
}
