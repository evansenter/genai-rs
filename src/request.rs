//! Request types for creating interactions.

use serde::de::{self, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use crate::content::Content;
use crate::environment::EnvironmentSpec;
use crate::response_format::ResponseFormatSpec;
use crate::steps::Step;
use crate::tools::{Tool, ToolChoice};
use crate::webhooks::WebhookConfig;

/// Role in a conversation turn.
///
/// Indicates whether the content came from the user or the model.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New roles may be added in future API versions.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant, preserving
/// the original data for debugging and roundtrip serialization.
///
/// # Example
///
/// ```
/// use genai_rs::Role;
///
/// let role = Role::User;
/// assert!(matches!(role, Role::User));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Role {
    /// Content from the user
    User,
    /// Content from the model
    Model,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized role type from the API
        role_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl Role {
    /// Returns true if this is an unknown role.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the role type name if this is an unknown role.
    #[must_use]
    pub fn unknown_role_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { role_type, .. } => Some(role_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown role.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Model => write!(f, "model"),
            Self::Unknown { role_type, .. } => write!(f, "{}", role_type),
        }
    }
}

impl Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Role::User => serializer.serialize_str("user"),
            Role::Model => serializer.serialize_str("model"),
            Role::Unknown { role_type, .. } => serializer.serialize_str(role_type),
        }
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "user" => Ok(Role::User),
            "model" => Ok(Role::Model),
            other => {
                tracing::warn!(
                    "Encountered unknown Role '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Role::Unknown {
                    role_type: other.to_string(),
                    data: serde_json::Value::String(other.to_string()),
                })
            }
        }
    }
}

/// Content for a conversation turn.
///
/// Can be simple text or an array of content parts for multimodal turns.
///
/// # Example
///
/// ```
/// use genai_rs::TurnContent;
///
/// // Simple text
/// let content = TurnContent::Text("Hello!".to_string());
///
/// // From string reference
/// let content: TurnContent = "Hello!".into();
/// ```
// Note: Unlike tagged enums (e.g., Content), this untagged enum cannot
// have an Unknown variant. Untagged enums have no type discriminator field, so Serde
// tries variants in order - there's no way to detect "unknown" content. The
// #[non_exhaustive] attribute provides forward compatibility at the Rust level by
// preventing exhaustive matches.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum TurnContent {
    /// Simple text content
    Text(String),
    /// Array of content parts (for multimodal content)
    Parts(Vec<Content>),
}

impl From<String> for TurnContent {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for TurnContent {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl From<Vec<Content>> for TurnContent {
    fn from(parts: Vec<Content>) -> Self {
        Self::Parts(parts)
    }
}

impl TurnContent {
    /// Returns the text content if this is a `Text` variant.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(t),
            Self::Parts(_) => None,
        }
    }

    /// Returns the content parts if this is a `Parts` variant.
    #[must_use]
    pub fn as_parts(&self) -> Option<&[Content]> {
        match self {
            Self::Parts(p) => Some(p),
            Self::Text(_) => None,
        }
    }

    /// Returns `true` if this is text content.
    #[must_use]
    pub const fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Returns `true` if this is parts content.
    #[must_use]
    pub const fn is_parts(&self) -> bool {
        matches!(self, Self::Parts(_))
    }
}

/// Input for an interaction - a simple string, an array of content blocks,
/// or an array of steps (conversation history).
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New input types may be added in future versions.
///
/// # Variants
///
/// - `Text`: Simple text input for single-turn conversations
/// - `Content`: Array of content blocks for multimodal input
/// - `Steps`: Array of [`Step`]s — the canonical multi-turn/history form
///   under API revision 2026-05-20 (replaces the deprecated `Turn` array)
///
/// # Example
///
/// ```
/// use genai_rs::{InteractionInput, Step};
///
/// // Simple text
/// let input = InteractionInput::Text("Hello!".to_string());
///
/// // Multi-turn conversation history as steps
/// let steps = vec![
///     Step::user_text("What is 2+2?"),
///     Step::model_text("2+2 equals 4."),
///     Step::user_text("And what's that times 3?"),
/// ];
/// let input = InteractionInput::Steps(steps);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum InteractionInput {
    /// Simple text input
    Text(String),
    /// Array of content blocks (single-turn multimodal input)
    Content(Vec<Content>),
    /// Array of steps (multi-turn conversation history, function results,
    /// thought signatures, ...)
    Steps(Vec<Step>),
}

impl Serialize for InteractionInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Text(t) => serializer.serialize_str(t),
            Self::Content(c) => c.serialize(serializer),
            Self::Steps(s) => s.serialize(serializer),
        }
    }
}

/// The set of `type` tags that identify a content block (as opposed to a step).
const CONTENT_TYPE_TAGS: &[&str] = &["text", "image", "audio", "video", "document"];

impl<'de> Deserialize<'de> for InteractionInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(Self::Text(s)),
            serde_json::Value::Array(items) => {
                // Decide between [Content] and [Step] by inspecting element
                // type tags. Elements with content tags (text/image/...) are
                // content blocks; everything else (user_input, function_call,
                // unknown future types, ...) is treated as steps, the
                // canonical revision 2026-05-20 form.
                let is_content = items.iter().all(|item| {
                    item.get("type")
                        .and_then(|t| t.as_str())
                        .is_some_and(|t| CONTENT_TYPE_TAGS.contains(&t))
                });
                if is_content && !items.is_empty() {
                    let contents = serde_json::from_value(serde_json::Value::Array(items))
                        .map_err(serde::de::Error::custom)?;
                    Ok(Self::Content(contents))
                } else {
                    let steps = serde_json::from_value(serde_json::Value::Array(items))
                        .map_err(serde::de::Error::custom)?;
                    Ok(Self::Steps(steps))
                }
            }
            other @ serde_json::Value::Object(_) => {
                // A single content or step object.
                let is_content = other
                    .get("type")
                    .and_then(|t| t.as_str())
                    .is_some_and(|t| CONTENT_TYPE_TAGS.contains(&t));
                if is_content {
                    let content: Content =
                        serde_json::from_value(other).map_err(serde::de::Error::custom)?;
                    Ok(Self::Content(vec![content]))
                } else {
                    let step: Step =
                        serde_json::from_value(other).map_err(serde::de::Error::custom)?;
                    Ok(Self::Steps(vec![step]))
                }
            }
            other => Err(serde::de::Error::custom(format!(
                "InteractionInput must be a string, array, or object; got {other}"
            ))),
        }
    }
}

/// Thinking level for chain-of-thought reasoning.
///
/// Controls the depth of reasoning the model performs before generating a response.
/// Higher levels produce more detailed reasoning but consume more tokens.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New thinking levels may be added in future versions.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant, preserving
/// the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThinkingLevel {
    /// Minimal reasoning, fastest responses
    Minimal,
    /// Light reasoning for simple problems
    Low,
    /// Balanced reasoning for moderate complexity
    Medium,
    /// Extensive reasoning for complex problems
    High,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized level type from the API
        level_type: String,
        /// The full JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl ThinkingLevel {
    /// Returns true if this is an unknown thinking level.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the level type name if this is an unknown thinking level.
    #[must_use]
    pub fn unknown_level_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { level_type, .. } => Some(level_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown thinking level.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for ThinkingLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ThinkingLevel::Minimal => serializer.serialize_str("minimal"),
            ThinkingLevel::Low => serializer.serialize_str("low"),
            ThinkingLevel::Medium => serializer.serialize_str("medium"),
            ThinkingLevel::High => serializer.serialize_str("high"),
            ThinkingLevel::Unknown { level_type, data } => {
                // If data is a simple string, serialize just the level_type
                if data.is_string() || data.is_null() {
                    serializer.serialize_str(level_type)
                } else {
                    // For complex data, serialize as an object
                    let mut map = serializer.serialize_map(None)?;
                    map.serialize_entry("level", level_type)?;
                    map.serialize_entry("data", data)?;
                    map.end()
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for ThinkingLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ThinkingLevelVisitor)
    }
}

struct ThinkingLevelVisitor;

impl<'de> Visitor<'de> for ThinkingLevelVisitor {
    type Value = ThinkingLevel;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a thinking level string or object")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match value {
            "minimal" => Ok(ThinkingLevel::Minimal),
            "low" => Ok(ThinkingLevel::Low),
            "medium" => Ok(ThinkingLevel::Medium),
            "high" => Ok(ThinkingLevel::High),
            other => {
                tracing::warn!(
                    "Encountered unknown ThinkingLevel '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(ThinkingLevel::Unknown {
                    level_type: other.to_string(),
                    data: serde_json::Value::String(other.to_string()),
                })
            }
        }
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        // For object-based thinking levels (future API compatibility)
        let value: serde_json::Value =
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
        let level_type = value
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        tracing::warn!(
            "Encountered unknown ThinkingLevel object '{}' - using Unknown variant (Evergreen)",
            level_type
        );
        Ok(ThinkingLevel::Unknown {
            level_type,
            data: value,
        })
    }
}

/// Generation configuration for model behavior
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Thinking level for chain-of-thought reasoning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    /// Seed for deterministic output generation.
    ///
    /// Using the same seed with identical inputs will produce the same output,
    /// useful for testing and debugging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    /// Stop sequences that halt generation.
    ///
    /// When the model generates any of these sequences, generation stops immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Controls whether thinking summaries are included in output.
    ///
    /// Use with `thinking_level` to control reasoning output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_summaries: Option<ThinkingSummaries>,
    /// Controls function calling behavior.
    ///
    /// Either a plain mode string (`auto|any|none|validated`) or an
    /// `allowed_tools` restriction object. See [`ToolChoice`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Positive values penalize tokens that already appeared in the text,
    /// increasing the likelihood of new topics. Range: [-2.0, 2.0].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    /// Positive values penalize tokens proportionally to their frequency in
    /// the text so far, reducing repetition. Range: [-2.0, 2.0].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    /// Speech configuration for text-to-speech audio output.
    ///
    /// Required when using the `audio` response modality. The wire format is
    /// a **list** of speaker configurations: a single entry for single-voice
    /// TTS, multiple entries (each with a distinct `speaker` name matching
    /// the prompt) for multi-speaker TTS.
    ///
    /// A legacy single-object wire form is still accepted on deserialize.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_speech_configs"
    )]
    pub speech_config: Option<Vec<SpeechConfig>>,
    /// Image generation configuration.
    ///
    /// Controls aspect ratio and size for image generation output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_config: Option<ImageConfig>,
    /// Video generation configuration.
    ///
    /// Controls the video generation task mode when using the `video`
    /// response modality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_config: Option<VideoConfig>,
}

/// Deserializes `speech_config` from either the spec list form or the legacy
/// single-object form.
fn deserialize_speech_configs<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<SpeechConfig>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrSingle {
        List(Vec<SpeechConfig>),
        Single(SpeechConfig),
    }

    Ok(
        Option::<ListOrSingle>::deserialize(deserializer)?.map(|value| match value {
            ListOrSingle::List(list) => list,
            ListOrSingle::Single(single) => vec![single],
        }),
    )
}

/// Speech configuration for text-to-speech audio output.
///
/// Configure voice, language, and speaker settings when using the `audio` response modality.
///
/// # Example
///
/// ```
/// use genai_rs::SpeechConfig;
///
/// let config = SpeechConfig {
///     voice: Some("Kore".to_string()),
///     language: Some("en-US".to_string()),
///     speaker: None,
/// };
/// ```
///
/// # Available Voices
///
/// Common voices include: Aoede, Charon, Fenrir, Kore, Puck, and others.
/// See [Google's TTS documentation](https://ai.google.dev/gemini-api/docs/text-generation)
/// for the full list of available voices.
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct SpeechConfig {
    /// The voice to use for speech synthesis.
    ///
    /// Examples: "Kore", "Puck", "Charon", "Fenrir", "Aoede"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,

    /// The language/locale for speech synthesis.
    ///
    /// Examples: "en-US", "es-ES", "fr-FR"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// The speaker name for multi-speaker scenarios.
    ///
    /// Should match a speaker name given in the prompt when using
    /// multi-speaker text-to-speech.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

impl SpeechConfig {
    /// Creates a new `SpeechConfig` with the specified voice.
    #[must_use]
    pub fn with_voice(voice: impl Into<String>) -> Self {
        Self {
            voice: Some(voice.into()),
            ..Default::default()
        }
    }

    /// Creates a new `SpeechConfig` with the specified voice and language.
    #[must_use]
    pub fn with_voice_and_language(voice: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            voice: Some(voice.into()),
            language: Some(language.into()),
            ..Default::default()
        }
    }
}

/// Configuration for image generation output.
///
/// Controls aspect ratio and size when generating images.
///
/// # Example
///
/// ```
/// use genai_rs::{ImageConfig, ImageAspectRatio, ImageSize};
///
/// let config = ImageConfig {
///     aspect_ratio: Some(ImageAspectRatio::Square),
///     image_size: Some(ImageSize::Hd1k),
/// };
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageConfig {
    /// The aspect ratio for generated images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<ImageAspectRatio>,
    /// The size/resolution for generated images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<ImageSize>,
}

/// Aspect ratio for image generation output.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New aspect ratios may be added in future API versions.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant, preserving
/// the original data for debugging and roundtrip serialization.
///
/// # Wire Format
///
/// Values serialize as string ratios: `"1:1"`, `"16:9"`, etc.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageAspectRatio {
    /// 1:1 square
    Square,
    /// 2:3 portrait
    Portrait2x3,
    /// 3:2 landscape
    Landscape3x2,
    /// 3:4 portrait
    Portrait3x4,
    /// 4:3 landscape
    Landscape4x3,
    /// 4:5 portrait
    Portrait4x5,
    /// 5:4 landscape
    Landscape5x4,
    /// 9:16 tall portrait
    Portrait9x16,
    /// 16:9 widescreen
    Widescreen16x9,
    /// 21:9 ultrawide
    Ultrawide21x9,
    /// 1:8 very tall
    Tall1x8,
    /// 8:1 very wide
    Wide8x1,
    /// 1:4 tall
    Tall1x4,
    /// 4:1 wide
    Wide4x1,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized ratio type from the API
        ratio_type: String,
        /// The full JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl ImageAspectRatio {
    /// Returns true if this is an unknown aspect ratio.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the ratio type name if this is an unknown aspect ratio.
    #[must_use]
    pub fn unknown_ratio_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { ratio_type, .. } => Some(ratio_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown aspect ratio.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for ImageAspectRatio {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Square => serializer.serialize_str("1:1"),
            Self::Portrait2x3 => serializer.serialize_str("2:3"),
            Self::Landscape3x2 => serializer.serialize_str("3:2"),
            Self::Portrait3x4 => serializer.serialize_str("3:4"),
            Self::Landscape4x3 => serializer.serialize_str("4:3"),
            Self::Portrait4x5 => serializer.serialize_str("4:5"),
            Self::Landscape5x4 => serializer.serialize_str("5:4"),
            Self::Portrait9x16 => serializer.serialize_str("9:16"),
            Self::Widescreen16x9 => serializer.serialize_str("16:9"),
            Self::Ultrawide21x9 => serializer.serialize_str("21:9"),
            Self::Tall1x8 => serializer.serialize_str("1:8"),
            Self::Wide8x1 => serializer.serialize_str("8:1"),
            Self::Tall1x4 => serializer.serialize_str("1:4"),
            Self::Wide4x1 => serializer.serialize_str("4:1"),
            Self::Unknown { ratio_type, .. } => serializer.serialize_str(ratio_type),
        }
    }
}

impl<'de> Deserialize<'de> for ImageAspectRatio {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("1:1") => Ok(Self::Square),
            Some("2:3") => Ok(Self::Portrait2x3),
            Some("3:2") => Ok(Self::Landscape3x2),
            Some("3:4") => Ok(Self::Portrait3x4),
            Some("4:3") => Ok(Self::Landscape4x3),
            Some("4:5") => Ok(Self::Portrait4x5),
            Some("5:4") => Ok(Self::Landscape5x4),
            Some("9:16") => Ok(Self::Portrait9x16),
            Some("16:9") => Ok(Self::Widescreen16x9),
            Some("21:9") => Ok(Self::Ultrawide21x9),
            Some("1:8") => Ok(Self::Tall1x8),
            Some("8:1") => Ok(Self::Wide8x1),
            Some("1:4") => Ok(Self::Tall1x4),
            Some("4:1") => Ok(Self::Wide4x1),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown ImageAspectRatio '{}'. \
                     Preserving in Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    ratio_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let ratio_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "ImageAspectRatio received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    ratio_type,
                    data: value,
                })
            }
        }
    }
}

/// Image size/resolution for image generation output.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New sizes may be added in future API versions.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant, preserving
/// the original data for debugging and roundtrip serialization.
///
/// # Wire Format
///
/// Values serialize as strings: `"512"`, `"1K"`, `"2K"`, `"4K"`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageSize {
    /// 512px resolution
    Sd512,
    /// 1K resolution
    Hd1k,
    /// 2K resolution
    Hd2k,
    /// 4K resolution
    Uhd4k,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized size type from the API
        size_type: String,
        /// The full JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl ImageSize {
    /// Returns true if this is an unknown image size.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the size type name if this is an unknown image size.
    #[must_use]
    pub fn unknown_size_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { size_type, .. } => Some(size_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown image size.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for ImageSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Sd512 => serializer.serialize_str("512"),
            Self::Hd1k => serializer.serialize_str("1K"),
            Self::Hd2k => serializer.serialize_str("2K"),
            Self::Uhd4k => serializer.serialize_str("4K"),
            Self::Unknown { size_type, .. } => serializer.serialize_str(size_type),
        }
    }
}

impl<'de> Deserialize<'de> for ImageSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("512") => Ok(Self::Sd512),
            Some("1K") => Ok(Self::Hd1k),
            Some("2K") => Ok(Self::Hd2k),
            Some("4K") => Ok(Self::Uhd4k),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown ImageSize '{}'. \
                     Preserving in Unknown variant.",
                    other
                );
                Ok(Self::Unknown {
                    size_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let size_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "ImageSize received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    size_type,
                    data: value,
                })
            }
        }
    }
}

/// Task mode for video generation.
///
/// If not specified, the model automatically determines the appropriate mode
/// based on the provided text prompt and input media.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase snake_case strings: `"text_to_video"`,
/// `"image_to_video"`, `"reference_to_video"`, `"edit"`, `"extend"`.
/// The full value list was confirmed live (2026-07) via the API's own
/// validation error for `generation_config.video_config.task`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum VideoTask {
    /// Generate a video from a text prompt.
    TextToVideo,
    /// Generate a video from an input image.
    ImageToVideo,
    /// Generate a video from reference media.
    ReferenceToVideo,
    /// Edit an existing video.
    Edit,
    /// Extend an existing video.
    Extend,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized task type from the API
        task_type: String,
        /// The full JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl VideoTask {
    /// Returns true if this is an unknown video task.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the task type name if this is an unknown video task.
    #[must_use]
    pub fn unknown_task_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { task_type, .. } => Some(task_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown video task.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for VideoTask {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::TextToVideo => serializer.serialize_str("text_to_video"),
            Self::ImageToVideo => serializer.serialize_str("image_to_video"),
            Self::ReferenceToVideo => serializer.serialize_str("reference_to_video"),
            Self::Edit => serializer.serialize_str("edit"),
            Self::Extend => serializer.serialize_str("extend"),
            Self::Unknown { task_type, .. } => serializer.serialize_str(task_type),
        }
    }
}

impl<'de> Deserialize<'de> for VideoTask {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("text_to_video") => Ok(Self::TextToVideo),
            Some("image_to_video") => Ok(Self::ImageToVideo),
            Some("reference_to_video") => Ok(Self::ReferenceToVideo),
            Some("edit") => Ok(Self::Edit),
            Some("extend") => Ok(Self::Extend),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown VideoTask '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Self::Unknown {
                    task_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let task_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "VideoTask received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    task_type,
                    data: value,
                })
            }
        }
    }
}

/// Configuration for video generation output
/// (`generation_config.video_config`).
///
/// # Example
///
/// ```
/// use genai_rs::{VideoConfig, VideoTask};
///
/// let config = VideoConfig::new().with_task(VideoTask::TextToVideo);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    /// Optional task mode for video generation. When unset, the model picks
    /// the mode based on the prompt and input media.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<VideoTask>,
}

impl VideoConfig {
    /// Creates an empty video config (model chooses the task mode).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the video generation task mode.
    #[must_use]
    pub fn with_task(mut self, task: VideoTask) -> Self {
        self.task = Some(task);
        self
    }
}

/// Request body for the Interactions API endpoint.
///
/// This type represents a fully-constructed interaction request that can be
/// cloned, serialized, and executed via [`Client::execute()`](crate::Client::execute).
///
/// # Creating Requests
///
/// Use [`InteractionBuilder::build()`](crate::InteractionBuilder::build) to create requests:
///
/// ```no_run
/// # use genai_rs::Client;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("api_key".to_string());
///
/// let request = client.interaction()
///     .with_model("gemini-3-flash-preview")
///     .with_text("Hello!")
///     .build()?;
///
/// // Request can be cloned, serialized, inspected
/// let backup = request.clone();
/// println!("{}", serde_json::to_string_pretty(&request)?);
/// # Ok(())
/// # }
/// ```
///
/// # Executing Requests
///
/// ```no_run
/// # use genai_rs::Client;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = Client::new("api_key".to_string());
/// # let request = client.interaction()
/// #     .with_model("gemini-3-flash-preview")
/// #     .with_text("Hello!")
/// #     .build()?;
/// let response = client.execute(request).await?;
/// # Ok(())
/// # }
/// ```
///
/// # Retrying Requests
///
/// Since `InteractionRequest` is `Clone`, you can retry failed requests:
///
/// ```no_run
/// # use genai_rs::{Client, GenaiError};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = Client::new("api_key".to_string());
/// # let request = client.interaction()
/// #     .with_model("gemini-3-flash-preview")
/// #     .with_text("Hello!")
/// #     .build()?;
/// let response = loop {
///     match client.execute(request.clone()).await {
///         Ok(r) => break r,
///         Err(e) if e.is_retryable() => continue,
///         Err(e) => return Err(e.into()),
///     }
/// };
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct InteractionRequest {
    /// Model name (e.g., "gemini-3-flash-preview") - mutually exclusive with agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Agent name (e.g., "deep-research-pro-preview-12-2025") - mutually exclusive with model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Agent-specific configuration (e.g., Deep Research thinking summaries)
    #[serde(rename = "agent_config", skip_serializing_if = "Option::is_none")]
    pub agent_config: Option<AgentConfig>,

    /// The input for this interaction
    pub input: InteractionInput,

    /// Reference to a previous interaction for stateful conversations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,

    /// Tools available for function calling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Response modalities (e.g., ["image"]; the API only accepts lowercase)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,

    /// Typed response format(s) for structured/media output.
    ///
    /// A single [`ResponseFormat`](crate::ResponseFormat) or a list of them
    /// (see [`ResponseFormatSpec`]). For the common JSON-schema case use
    /// [`InteractionBuilder::with_response_format()`](crate::InteractionBuilder::with_response_format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormatSpec>,

    /// Model configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,

    /// Enable streaming responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// Background execution mode (agents only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,

    /// Persist interaction data (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    /// System instruction for the model (plain string per the API spec)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,

    /// Latency/priority tier for processing this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<ServiceTier>,

    /// Name of an explicit context cache to use for this request
    /// (e.g., `cachedContents/xyz`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,

    /// Per-request webhook routing: deliver this request's events to the
    /// given URIs (instead of the registered webhooks) with optional
    /// user metadata echoed on each event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_config: Option<WebhookConfig>,

    /// Environment for the interaction: a string environment ID or a typed
    /// remote environment (sources + network allowlist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentSpec>,
}

/// Latency/priority service tier for a request.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase strings: `"flex"`, `"standard"`, `"priority"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ServiceTier {
    /// Flexible latency, lower cost.
    Flex,
    /// Standard processing.
    Standard,
    /// Prioritized processing.
    Priority,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized tier type from the API
        tier_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl ServiceTier {
    /// Returns true if this is an unknown service tier.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the tier type name if this is an unknown service tier.
    #[must_use]
    pub fn unknown_tier_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { tier_type, .. } => Some(tier_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown service tier.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl fmt::Display for ServiceTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flex => write!(f, "flex"),
            Self::Standard => write!(f, "standard"),
            Self::Priority => write!(f, "priority"),
            Self::Unknown { tier_type, .. } => write!(f, "{}", tier_type),
        }
    }
}

impl Serialize for ServiceTier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Flex => serializer.serialize_str("flex"),
            Self::Standard => serializer.serialize_str("standard"),
            Self::Priority => serializer.serialize_str("priority"),
            Self::Unknown { tier_type, .. } => serializer.serialize_str(tier_type),
        }
    }
}

impl<'de> Deserialize<'de> for ServiceTier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("flex") => Ok(Self::Flex),
            Some("standard") => Ok(Self::Standard),
            Some("priority") => Ok(Self::Priority),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown ServiceTier '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Self::Unknown {
                    tier_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let tier_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "ServiceTier received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    tier_type,
                    data: value,
                })
            }
        }
    }
}

// =============================================================================
// Agent Configuration Types
// =============================================================================

/// Thinking summaries configuration for agent output.
///
/// When using thinking mode (via `with_thinking_level`), you can control
/// whether the model's reasoning process is summarized in the output.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
/// New summary modes may be added in future versions.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant, preserving
/// the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThinkingSummaries {
    /// Automatically include thinking summaries (default when thinking is enabled)
    Auto,
    /// Do not include thinking summaries
    None,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized summaries type from the API
        summaries_type: String,
        /// The full JSON data, preserved for debugging and roundtrip serialization
        data: serde_json::Value,
    },
}

impl ThinkingSummaries {
    /// Returns true if this is an unknown thinking summaries value.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the summaries type name if this is an unknown value.
    #[must_use]
    pub fn unknown_summaries_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { summaries_type, .. } => Some(summaries_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown value.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    /// Convert to the agent_config wire format (THINKING_SUMMARIES_*).
    ///
    /// AgentConfig uses a different wire format than GenerationConfig:
    /// - GenerationConfig: lowercase ("auto", "none")
    /// - AgentConfig: SCREAMING_CASE ("THINKING_SUMMARIES_AUTO", "THINKING_SUMMARIES_NONE")
    #[must_use]
    pub fn to_agent_config_value(&self) -> serde_json::Value {
        match self {
            ThinkingSummaries::Auto => {
                serde_json::Value::String("THINKING_SUMMARIES_AUTO".to_string())
            }
            ThinkingSummaries::None => {
                serde_json::Value::String("THINKING_SUMMARIES_NONE".to_string())
            }
            ThinkingSummaries::Unknown { summaries_type, .. } => {
                // For unknown values, preserve the original format
                serde_json::Value::String(summaries_type.clone())
            }
        }
    }
}

impl Serialize for ThinkingSummaries {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Note: GenerationConfig uses lowercase ("auto"/"none")
        // For AgentConfig, use to_agent_config_value() instead
        match self {
            ThinkingSummaries::Auto => serializer.serialize_str("auto"),
            ThinkingSummaries::None => serializer.serialize_str("none"),
            ThinkingSummaries::Unknown {
                summaries_type,
                data,
            } => {
                // If data is a simple string, serialize just the summaries_type
                if data.is_string() || data.is_null() {
                    serializer.serialize_str(summaries_type)
                } else {
                    // For complex data, serialize as an object
                    let mut map = serializer.serialize_map(None)?;
                    map.serialize_entry("summaries", summaries_type)?;
                    map.serialize_entry("data", data)?;
                    map.end()
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for ThinkingSummaries {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ThinkingSummariesVisitor)
    }
}

struct ThinkingSummariesVisitor;

impl<'de> Visitor<'de> for ThinkingSummariesVisitor {
    type Value = ThinkingSummaries;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a thinking summaries string or object")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        match value {
            // Wire format is THINKING_SUMMARIES_*, but also accept lowercase for flexibility
            "THINKING_SUMMARIES_AUTO" | "auto" => Ok(ThinkingSummaries::Auto),
            "THINKING_SUMMARIES_NONE" | "none" => Ok(ThinkingSummaries::None),
            other => {
                tracing::warn!(
                    "Encountered unknown ThinkingSummaries '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(ThinkingSummaries::Unknown {
                    summaries_type: other.to_string(),
                    data: serde_json::Value::String(other.to_string()),
                })
            }
        }
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        // For object-based thinking summaries (future API compatibility)
        let value: serde_json::Value =
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
        let summaries_type = value
            .get("summaries")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        tracing::warn!(
            "Encountered unknown ThinkingSummaries object '{}' - using Unknown variant (Evergreen)",
            summaries_type
        );
        Ok(ThinkingSummaries::Unknown {
            summaries_type,
            data: value,
        })
    }
}

/// Agent-specific configuration for specialized agents.
///
/// This is a thin wrapper around JSON that provides full forward compatibility.
/// Use typed config structs like [`DeepResearchConfig`] for compile-time guidance,
/// or construct directly from JSON for unknown/future agent types.
///
/// # Usage
///
/// ## Typed configs (recommended for known agents)
/// ```
/// use genai_rs::{AgentConfig, DeepResearchConfig, ThinkingSummaries};
///
/// let config: AgentConfig = DeepResearchConfig::new()
///     .with_thinking_summaries(ThinkingSummaries::Auto)
///     .into();
/// ```
///
/// ## Raw JSON (for unknown/future agents)
/// ```
/// use genai_rs::AgentConfig;
///
/// let config = AgentConfig::from_value(serde_json::json!({
///     "type": "future-agent",
///     "newOption": true
/// }));
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentConfig(serde_json::Value);

impl AgentConfig {
    /// Create an agent config from a raw JSON value.
    ///
    /// Use this for unknown or future agent types that don't have typed config structs.
    #[must_use]
    pub fn from_value(value: serde_json::Value) -> Self {
        Self(value)
    }

    /// Access the underlying JSON value.
    #[must_use]
    pub fn as_value(&self) -> &serde_json::Value {
        &self.0
    }

    /// Get the agent config type (e.g., "deep-research", "dynamic").
    #[must_use]
    pub fn config_type(&self) -> Option<&str> {
        self.0.get("type").and_then(|v| v.as_str())
    }
}

/// Visualization mode for the Deep Research agent.
///
/// Controls whether the agent includes visualizations in its response.
///
/// This enum is marked `#[non_exhaustive]` for forward compatibility.
///
/// # Wire Format
///
/// Serializes as lowercase strings: `"off"`, `"auto"`.
///
/// # Evergreen Pattern
///
/// Unknown values from the API deserialize into the `Unknown` variant,
/// preserving the original data for debugging and roundtrip serialization.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Visualization {
    /// No visualizations in the response.
    Off,
    /// The agent decides when to include visualizations.
    Auto,
    /// Unknown variant for forward compatibility (Evergreen pattern)
    Unknown {
        /// The unrecognized visualization type from the API
        visualization_type: String,
        /// The raw JSON value, preserved for debugging and roundtrip
        data: serde_json::Value,
    },
}

impl Visualization {
    /// Returns true if this is an unknown visualization mode.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the visualization type name if this is an unknown mode.
    #[must_use]
    pub fn unknown_visualization_type(&self) -> Option<&str> {
        match self {
            Self::Unknown {
                visualization_type, ..
            } => Some(visualization_type),
            _ => None,
        }
    }

    /// Returns the preserved data if this is an unknown mode.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for Visualization {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Off => serializer.serialize_str("off"),
            Self::Auto => serializer.serialize_str("auto"),
            Self::Unknown {
                visualization_type, ..
            } => serializer.serialize_str(visualization_type),
        }
    }
}

impl<'de> Deserialize<'de> for Visualization {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.as_str() {
            Some("off") => Ok(Self::Off),
            Some("auto") => Ok(Self::Auto),
            Some(other) => {
                tracing::warn!(
                    "Encountered unknown Visualization '{}' - using Unknown variant (Evergreen)",
                    other
                );
                Ok(Self::Unknown {
                    visualization_type: other.to_string(),
                    data: value,
                })
            }
            None => {
                let visualization_type = format!("<non-string: {}>", value);
                tracing::warn!(
                    "Visualization received non-string value: {}. \
                     Preserving in Unknown variant.",
                    value
                );
                Ok(Self::Unknown {
                    visualization_type,
                    data: value,
                })
            }
        }
    }
}

/// Configuration for Deep Research agent.
///
/// Deep Research agent performs comprehensive research tasks
/// and can optionally include thinking summaries, visualizations,
/// collaborative planning, and BigQuery access.
///
/// Known Deep Research agent IDs: `deep-research-pro-preview-12-2025`,
/// `deep-research-preview-04-2026`, `deep-research-max-preview-04-2026`.
/// See `docs/AGENTS_AND_BACKGROUND.md`.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, DeepResearchConfig, ThinkingSummaries, Visualization};
///
/// let config: AgentConfig = DeepResearchConfig::new()
///     .with_thinking_summaries(ThinkingSummaries::Auto)
///     .with_visualization(Visualization::Auto)
///     .with_collaborative_planning(true)
///     .with_bigquery_tool(true)
///     .into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct DeepResearchConfig {
    thinking_summaries: Option<ThinkingSummaries>,
    visualization: Option<Visualization>,
    collaborative_planning: Option<bool>,
    enable_bigquery_tool: Option<bool>,
}

impl DeepResearchConfig {
    /// Create a new Deep Research configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set thinking summaries mode.
    ///
    /// Controls whether the agent's reasoning process is summarized in output.
    #[must_use]
    pub fn with_thinking_summaries(mut self, summaries: ThinkingSummaries) -> Self {
        self.thinking_summaries = Some(summaries);
        self
    }

    /// Set the visualization mode (`off` | `auto`).
    #[must_use]
    pub fn with_visualization(mut self, visualization: Visualization) -> Self {
        self.visualization = Some(visualization);
        self
    }

    /// Enable (or disable) human-in-the-loop planning.
    ///
    /// When `true`, the agent first returns a research plan and only proceeds
    /// after the user confirms the plan in the next turn.
    #[must_use]
    pub fn with_collaborative_planning(mut self, enabled: bool) -> Self {
        self.collaborative_planning = Some(enabled);
        self
    }

    /// Enable (or disable) the BigQuery tool for the Deep Research agent.
    ///
    /// Server-side constraint (verified live 2026-07): the Gemini API
    /// rejects `agent_config.enable_bigquery_tool` — "not available on the
    /// Gemini API but it is available on the Gemini Enterprise Agent
    /// Platform" (Vertex-only).
    #[must_use]
    pub fn with_bigquery_tool(mut self, enabled: bool) -> Self {
        self.enable_bigquery_tool = Some(enabled);
        self
    }
}

impl From<DeepResearchConfig> for AgentConfig {
    fn from(config: DeepResearchConfig) -> Self {
        let mut map = serde_json::Map::new();
        map.insert(
            "type".into(),
            serde_json::Value::String("deep-research".into()),
        );
        if let Some(ts) = config.thinking_summaries {
            // Use agent_config format (THINKING_SUMMARIES_*), not generation_config format (auto/none)
            map.insert("thinking_summaries".into(), ts.to_agent_config_value());
        }
        if let Some(visualization) = config.visualization {
            map.insert(
                "visualization".into(),
                serde_json::to_value(&visualization)
                    .expect("Visualization serialization is infallible"),
            );
        }
        if let Some(planning) = config.collaborative_planning {
            map.insert(
                "collaborative_planning".into(),
                serde_json::Value::Bool(planning),
            );
        }
        if let Some(bigquery) = config.enable_bigquery_tool {
            map.insert(
                "enable_bigquery_tool".into(),
                serde_json::Value::Bool(bigquery),
            );
        }
        AgentConfig(serde_json::Value::Object(map))
    }
}

/// Configuration for Dynamic agent.
///
/// Dynamic agents adapt their behavior based on the task.
/// Currently has no configurable options.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, DynamicConfig};
///
/// let config: AgentConfig = DynamicConfig::new().into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct DynamicConfig;

impl DynamicConfig {
    /// Create a new Dynamic agent configuration.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl From<DynamicConfig> for AgentConfig {
    fn from(_: DynamicConfig) -> Self {
        AgentConfig(serde_json::json!({"type": "dynamic"}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Agent Config Tests
    // =========================================================================

    #[test]
    fn test_thinking_summaries_serialization() {
        // GenerationConfig wire format uses lowercase
        assert_eq!(
            serde_json::to_string(&ThinkingSummaries::Auto).unwrap(),
            "\"auto\""
        );

        assert_eq!(
            serde_json::to_string(&ThinkingSummaries::None).unwrap(),
            "\"none\""
        );
    }

    #[test]
    fn test_thinking_summaries_agent_config_format() {
        // AgentConfig uses THINKING_SUMMARIES_* format via to_agent_config_value()
        assert_eq!(
            ThinkingSummaries::Auto.to_agent_config_value(),
            serde_json::Value::String("THINKING_SUMMARIES_AUTO".to_string())
        );

        assert_eq!(
            ThinkingSummaries::None.to_agent_config_value(),
            serde_json::Value::String("THINKING_SUMMARIES_NONE".to_string())
        );
    }

    #[test]
    fn test_thinking_summaries_deserialization() {
        // Test wire format (THINKING_SUMMARIES_*)
        assert_eq!(
            serde_json::from_str::<ThinkingSummaries>("\"THINKING_SUMMARIES_AUTO\"").unwrap(),
            ThinkingSummaries::Auto
        );
        assert_eq!(
            serde_json::from_str::<ThinkingSummaries>("\"THINKING_SUMMARIES_NONE\"").unwrap(),
            ThinkingSummaries::None
        );

        // Also accept lowercase for flexibility
        assert_eq!(
            serde_json::from_str::<ThinkingSummaries>("\"auto\"").unwrap(),
            ThinkingSummaries::Auto
        );
        assert_eq!(
            serde_json::from_str::<ThinkingSummaries>("\"none\"").unwrap(),
            ThinkingSummaries::None
        );
    }

    #[test]
    fn test_thinking_summaries_unknown_roundtrip() {
        let unknown: ThinkingSummaries = serde_json::from_str("\"future_variant\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_summaries_type(), Some("future_variant"));

        // Roundtrip preserves the unknown value
        let json = serde_json::to_string(&unknown).unwrap();
        assert_eq!(json, "\"future_variant\"");
    }

    #[test]
    fn test_deep_research_config_serialization() {
        let config: AgentConfig = DeepResearchConfig::new()
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .into();

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "deep-research");
        assert_eq!(value["thinking_summaries"], "THINKING_SUMMARIES_AUTO");
    }

    #[test]
    fn test_deep_research_config_without_thinking_summaries() {
        let config: AgentConfig = DeepResearchConfig::new().into();

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "deep-research");
        assert!(value.get("thinking_summaries").is_none());
    }

    #[test]
    fn test_dynamic_config_serialization() {
        let config: AgentConfig = DynamicConfig::new().into();

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "dynamic");
    }

    #[test]
    fn test_agent_config_from_raw_json() {
        let config = AgentConfig::from_value(serde_json::json!({
            "type": "custom-agent",
            "option1": true,
            "option2": "value"
        }));

        assert_eq!(config.config_type(), Some("custom-agent"));
        assert_eq!(config.as_value()["option1"], true);
    }

    #[test]
    fn test_agent_config_roundtrip() {
        let config: AgentConfig = DeepResearchConfig::new()
            .with_thinking_summaries(ThinkingSummaries::Auto)
            .into();

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let parsed: AgentConfig = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(config, parsed);
    }

    // =========================================================================
    // SpeechConfig Tests
    // =========================================================================

    #[test]
    fn test_speech_config_with_voice() {
        let config = SpeechConfig::with_voice("Kore");
        assert_eq!(config.voice, Some("Kore".to_string()));
        assert_eq!(config.language, None);
        assert_eq!(config.speaker, None);
    }

    #[test]
    fn test_speech_config_with_voice_and_language() {
        let config = SpeechConfig::with_voice_and_language("Puck", "en-GB");
        assert_eq!(config.voice, Some("Puck".to_string()));
        assert_eq!(config.language, Some("en-GB".to_string()));
        assert_eq!(config.speaker, None);
    }

    #[test]
    fn test_speech_config_serialization() {
        let config = SpeechConfig {
            voice: Some("Fenrir".to_string()),
            language: Some("en-US".to_string()),
            speaker: None,
        };

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Verify flat format is produced (voice, language at top level)
        assert_eq!(value["voice"], "Fenrir");
        assert_eq!(value["language"], "en-US");
        assert!(value.get("speaker").is_none()); // None fields should be skipped

        // Verify nested format is NOT produced
        // Google docs suggest voiceConfig.prebuiltVoiceConfig.voiceName but that returns 400.
        // See docs/ENUM_WIRE_FORMATS.md and docs/INTERACTIONS_API_FEEDBACK.md Issue #7.
        assert!(
            value.get("voiceConfig").is_none(),
            "Should use flat format, not nested voiceConfig"
        );
        assert!(
            value.get("prebuiltVoiceConfig").is_none(),
            "Should use flat format, not nested prebuiltVoiceConfig"
        );
    }

    #[test]
    fn test_speech_config_roundtrip() {
        let config = SpeechConfig {
            voice: Some("Aoede".to_string()),
            language: Some("es-ES".to_string()),
            speaker: Some("narrator".to_string()),
        };

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let parsed: SpeechConfig = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(config.voice, parsed.voice);
        assert_eq!(config.language, parsed.language);
        assert_eq!(config.speaker, parsed.speaker);
    }

    #[test]
    fn test_speech_config_default() {
        let config = SpeechConfig::default();
        assert_eq!(config.voice, None);
        assert_eq!(config.language, None);
        assert_eq!(config.speaker, None);
    }

    // =========================================================================
    // ImageAspectRatio Tests
    // =========================================================================

    #[test]
    fn test_image_aspect_ratio_serialization() {
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Square).unwrap(),
            "\"1:1\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Portrait2x3).unwrap(),
            "\"2:3\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Landscape3x2).unwrap(),
            "\"3:2\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Portrait3x4).unwrap(),
            "\"3:4\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Landscape4x3).unwrap(),
            "\"4:3\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Portrait4x5).unwrap(),
            "\"4:5\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Landscape5x4).unwrap(),
            "\"5:4\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Portrait9x16).unwrap(),
            "\"9:16\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Widescreen16x9).unwrap(),
            "\"16:9\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Ultrawide21x9).unwrap(),
            "\"21:9\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Tall1x8).unwrap(),
            "\"1:8\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Wide8x1).unwrap(),
            "\"8:1\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Tall1x4).unwrap(),
            "\"1:4\""
        );
        assert_eq!(
            serde_json::to_string(&ImageAspectRatio::Wide4x1).unwrap(),
            "\"4:1\""
        );
    }

    #[test]
    fn test_image_aspect_ratio_deserialization_roundtrip() {
        let ratios = vec![
            ("\"1:1\"", ImageAspectRatio::Square),
            ("\"2:3\"", ImageAspectRatio::Portrait2x3),
            ("\"3:2\"", ImageAspectRatio::Landscape3x2),
            ("\"3:4\"", ImageAspectRatio::Portrait3x4),
            ("\"4:3\"", ImageAspectRatio::Landscape4x3),
            ("\"4:5\"", ImageAspectRatio::Portrait4x5),
            ("\"5:4\"", ImageAspectRatio::Landscape5x4),
            ("\"9:16\"", ImageAspectRatio::Portrait9x16),
            ("\"16:9\"", ImageAspectRatio::Widescreen16x9),
            ("\"21:9\"", ImageAspectRatio::Ultrawide21x9),
            ("\"1:8\"", ImageAspectRatio::Tall1x8),
            ("\"8:1\"", ImageAspectRatio::Wide8x1),
            ("\"1:4\"", ImageAspectRatio::Tall1x4),
            ("\"4:1\"", ImageAspectRatio::Wide4x1),
        ];

        for (json, expected) in ratios {
            let parsed: ImageAspectRatio = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);

            // Roundtrip
            let serialized = serde_json::to_string(&parsed).unwrap();
            assert_eq!(serialized, json);
        }
    }

    #[test]
    fn test_image_aspect_ratio_unknown_roundtrip() {
        let unknown: ImageAspectRatio = serde_json::from_str("\"7:3\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_ratio_type(), Some("7:3"));
        assert!(unknown.unknown_data().is_some());

        // Roundtrip preserves the unknown value
        let json = serde_json::to_string(&unknown).unwrap();
        assert_eq!(json, "\"7:3\"");
    }

    #[test]
    fn test_image_aspect_ratio_known_not_unknown() {
        assert!(!ImageAspectRatio::Square.is_unknown());
        assert_eq!(ImageAspectRatio::Widescreen16x9.unknown_ratio_type(), None);
        assert_eq!(ImageAspectRatio::Portrait2x3.unknown_data(), None);
    }

    // =========================================================================
    // ImageSize Tests
    // =========================================================================

    #[test]
    fn test_image_size_serialization() {
        assert_eq!(serde_json::to_string(&ImageSize::Sd512).unwrap(), "\"512\"");
        assert_eq!(serde_json::to_string(&ImageSize::Hd1k).unwrap(), "\"1K\"");
        assert_eq!(serde_json::to_string(&ImageSize::Hd2k).unwrap(), "\"2K\"");
        assert_eq!(serde_json::to_string(&ImageSize::Uhd4k).unwrap(), "\"4K\"");
    }

    #[test]
    fn test_image_size_deserialization_roundtrip() {
        let sizes = vec![
            ("\"512\"", ImageSize::Sd512),
            ("\"1K\"", ImageSize::Hd1k),
            ("\"2K\"", ImageSize::Hd2k),
            ("\"4K\"", ImageSize::Uhd4k),
        ];

        for (json, expected) in sizes {
            let parsed: ImageSize = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected);

            let serialized = serde_json::to_string(&parsed).unwrap();
            assert_eq!(serialized, json);
        }
    }

    #[test]
    fn test_image_size_unknown_roundtrip() {
        let unknown: ImageSize = serde_json::from_str("\"8K\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_size_type(), Some("8K"));
        assert!(unknown.unknown_data().is_some());

        let json = serde_json::to_string(&unknown).unwrap();
        assert_eq!(json, "\"8K\"");
    }

    #[test]
    fn test_image_size_known_not_unknown() {
        assert!(!ImageSize::Sd512.is_unknown());
        assert_eq!(ImageSize::Hd1k.unknown_size_type(), None);
        assert_eq!(ImageSize::Uhd4k.unknown_data(), None);
    }

    // =========================================================================
    // ImageConfig Tests
    // =========================================================================

    #[test]
    fn test_image_config_serialization_roundtrip() {
        let config = ImageConfig {
            aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
            image_size: Some(ImageSize::Hd2k),
        };

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let parsed: ImageConfig = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(config, parsed);
    }

    #[test]
    fn test_image_config_default() {
        let config = ImageConfig::default();
        assert_eq!(config.aspect_ratio, None);
        assert_eq!(config.image_size, None);
    }

    #[test]
    fn test_image_config_partial_fields() {
        let config = ImageConfig {
            aspect_ratio: Some(ImageAspectRatio::Square),
            image_size: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["aspect_ratio"], "1:1");
        assert!(value.get("image_size").is_none());
    }

    #[test]
    fn test_image_config_skip_serializing_none() {
        let config = ImageConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_generation_config_with_image_config() {
        let config = GenerationConfig {
            image_config: Some(ImageConfig {
                aspect_ratio: Some(ImageAspectRatio::Portrait9x16),
                image_size: Some(ImageSize::Uhd4k),
            }),
            ..Default::default()
        };

        let json = serde_json::to_string(&config).expect("Serialization failed");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["image_config"]["aspect_ratio"], "9:16");
        assert_eq!(value["image_config"]["image_size"], "4K");
    }

    // =========================================================================
    // GenerationConfig tool_choice / penalty Tests
    // =========================================================================

    #[test]
    fn test_generation_config_tool_choice_mode_serializes_lowercase() {
        let config = GenerationConfig {
            tool_choice: Some(ToolChoice::Mode(crate::tools::FunctionCallingMode::Any)),
            ..Default::default()
        };

        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(value["tool_choice"], "any");
    }

    #[test]
    fn test_generation_config_tool_choice_allowed_tools_object() {
        let config = GenerationConfig {
            tool_choice: Some(ToolChoice::allowed_tools(
                Some(crate::tools::FunctionCallingMode::Any),
                vec!["get_weather".to_string(), "get_time".to_string()],
            )),
            ..Default::default()
        };

        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(value["tool_choice"]["allowed_tools"]["mode"], "any");
        assert_eq!(
            value["tool_choice"]["allowed_tools"]["tools"][0],
            "get_weather"
        );
        assert!(
            value.get("allowed_tools").is_none(),
            "top-level allowed_tools was removed from generation_config"
        );
    }

    #[test]
    fn test_generation_config_penalties_serialize() {
        let config = GenerationConfig {
            presence_penalty: Some(0.5),
            frequency_penalty: Some(-0.5),
            ..Default::default()
        };
        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(value["presence_penalty"], 0.5);
        assert_eq!(value["frequency_penalty"], -0.5);
    }

    #[test]
    fn test_generation_config_has_no_top_k() {
        // top_k was dropped from the 2026-05-20 spec; ensure it never serializes.
        let config = GenerationConfig {
            temperature: Some(0.3),
            ..Default::default()
        };
        let value = serde_json::to_value(&config).unwrap();
        assert!(value.get("top_k").is_none());
    }

    // =========================================================================
    // ServiceTier Tests
    // =========================================================================

    #[test]
    fn test_service_tier_roundtrip() {
        for (tier, wire) in [
            (ServiceTier::Flex, "\"flex\""),
            (ServiceTier::Standard, "\"standard\""),
            (ServiceTier::Priority, "\"priority\""),
        ] {
            assert_eq!(serde_json::to_string(&tier).unwrap(), wire);
            let parsed: ServiceTier = serde_json::from_str(wire).unwrap();
            assert_eq!(parsed, tier);
        }
    }

    #[test]
    fn test_service_tier_unknown_roundtrip() {
        let unknown: ServiceTier = serde_json::from_str("\"turbo\"").unwrap();
        assert!(unknown.is_unknown());
        assert_eq!(unknown.unknown_tier_type(), Some("turbo"));
        assert!(unknown.unknown_data().is_some());
        assert_eq!(serde_json::to_string(&unknown).unwrap(), "\"turbo\"");
    }

    // =========================================================================
    // InteractionInput Tests
    // =========================================================================

    #[test]
    fn test_interaction_input_text_roundtrip() {
        let input = InteractionInput::Text("Hello".into());
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, "\"Hello\"");
        let back: InteractionInput = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, InteractionInput::Text(t) if t == "Hello"));
    }

    #[test]
    fn test_interaction_input_content_array_roundtrip() {
        let json = r#"[{"type":"text","text":"hi"},{"type":"image","uri":"files/x","mime_type":"image/png"}]"#;
        let input: InteractionInput = serde_json::from_str(json).unwrap();
        match &input {
            InteractionInput::Content(c) => assert_eq!(c.len(), 2),
            other => panic!("Expected Content, got {other:?}"),
        }
    }

    #[test]
    fn test_interaction_input_steps_array_roundtrip() {
        let json = r#"[
            {"type":"user_input","content":[{"type":"text","text":"hi"}]},
            {"type":"model_output","content":[{"type":"text","text":"hello"}]},
            {"type":"function_result","call_id":"c1","result":"done"}
        ]"#;
        let input: InteractionInput = serde_json::from_str(json).unwrap();
        match &input {
            InteractionInput::Steps(s) => assert_eq!(s.len(), 3),
            other => panic!("Expected Steps, got {other:?}"),
        }
    }

    #[test]
    fn test_interaction_input_single_content_object() {
        let json = r#"{"type":"text","text":"hi"}"#;
        let input: InteractionInput = serde_json::from_str(json).unwrap();
        match &input {
            InteractionInput::Content(c) => assert_eq!(c.len(), 1),
            other => panic!("Expected Content, got {other:?}"),
        }
    }
}
