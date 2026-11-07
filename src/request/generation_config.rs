//! [`GenerationConfig`], its [`ThinkingLevel`], and the per-modality
//! configs nested in it (transcription, speech, image, video). See the
//! [parent module](super).

use serde::{Deserialize, Deserializer, Serialize};

use super::agent_config::ThinkingSummaries;
use crate::tools::ToolChoice;
use crate::wire_enum::wire_enum;
use crate::wire_fields::{Fallback, ModeledMap, WireFields};

wire_enum! {
    /// Thinking level for chain-of-thought reasoning.
    ///
    /// Controls the depth of reasoning the model performs before generating a response.
    /// Higher levels produce more detailed reasoning but consume more tokens.
    pub enum ThinkingLevel {
        /// Minimal reasoning, fastest responses.
        ///
        /// Not supported by every model: [`DEFAULT_MODEL`](crate::DEFAULT_MODEL)
        /// rejects it with a 400.
        Minimal = "minimal",
        /// Light reasoning for simple problems
        Low = "low",
        /// Balanced reasoning for moderate complexity
        Medium = "medium",
        /// Extensive reasoning for complex problems
        High = "high",
    }
    unknown(level_type, unknown_level_type)
}

/// Generation configuration for model behavior
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct GenerationConfig {
    /// Sampling temperature. Deprecated in the google-genai 2.34.0 bindings,
    /// but still accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Output cap. With decoding continuation it counts across
    /// continuations; see
    /// [`InteractionBuilder::with_continuation_token`](crate::InteractionBuilder::with_continuation_token).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    /// Nucleus sampling. Deprecated in the google-genai 2.34.0 bindings, but
    /// still accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Thinking level for chain-of-thought reasoning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    /// Seed for repeatable sampling.
    ///
    /// Identical seeded requests have returned identical text, but the API
    /// documents no guarantee, so don't rely on it for exact-text
    /// assertions (see `docs/CONFIGURATION.md`, "Seeds").
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
    /// Speech configuration for text-to-speech audio output.
    ///
    /// Required when using the `audio` response modality. The wire format is
    /// a **list** of speaker configurations: a single entry for single-voice
    /// TTS, multiple entries (each with a distinct `speaker` name matching
    /// the prompt) for multi-speaker TTS.
    ///
    /// Three forms are accepted on deserialize, all normalizing to the list:
    /// the list itself, a bare single object (legacy), and a
    /// `{"speakers": [...]}` wrapper (the spec's `SpeakerConfig`). Only the
    /// list is ever sent. A stored interaction read back serves the
    /// `{"speakers": [...]}` form whichever was sent, and it serializes back
    /// as the list, which every TTS model accepts.
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
    /// Audio transcription configuration.
    ///
    /// Controls language hints, diarization, custom vocabulary and
    /// timestamp granularity when transcribing audio input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription_config: Option<TranscriptionConfig>,
    /// Fields this crate does not model, sent flat beside the modeled ones
    /// and kept from a deserialized config, such as the
    /// [`generation_config`](crate::InteractionResponse::generation_config)
    /// a stored interaction serves.
    ///
    /// The server checks these like any other key: a name it doesn't know
    /// is `400 Unknown parameter`. An entry named after a modeled field is
    /// written after it, and the API keeps the last of a repeated key, so it
    /// overrides that field.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Audio transcription configuration.
///
/// Set via
/// [`GenerationConfig::transcription_config`] to control how audio input is
/// transcribed. It takes effect on
/// [`DEFAULT_TRANSCRIPTION_MODEL`](crate::DEFAULT_TRANSCRIPTION_MODEL).
///
/// # Example
///
/// ```
/// use genai_rs::TranscriptionConfig;
///
/// // Prefer the builder over a struct literal: the struct is
/// // constructible, so literals break when fields are added (see the
/// // 0.9.0 CHANGELOG entry).
/// let config = TranscriptionConfig::new()
///     .with_language_codes(["en-US"])
///     .with_diarization_mode("speaker");
/// assert_eq!(
///     serde_json::to_value(&config).unwrap(),
///     serde_json::json!({
///         "language_codes": ["en-US"],
///         "diarization_mode": "speaker"
///     })
/// );
/// ```
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct TranscriptionConfig {
    /// Phrases to bias recognition toward (contextual adaptation).
    /// Deprecated in the google-genai 2.34.0 bindings, but still accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptation_phrases: Option<Vec<String>>,
    /// Domain-specific vocabulary to bias recognition toward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_vocabulary: Option<Vec<String>>,
    /// Speaker diarization mode. The SDK spec documents `"speaker"` as the
    /// only supported value today; kept an open string (Evergreen) so new
    /// modes work without a crate release. Deprecated upstream (google-genai
    /// 2.25) in favor of [`TranscriptionMode::Verbatim`] in `mode`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diarization_mode: Option<String>,
    /// BCP-47 language codes hinting the audio's language(s). Omitted or
    /// empty means automatic language detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_codes: Option<Vec<String>>,
    /// Timestamp granularities to include. The SDK spec documents `"word"`
    /// as the only supported value today (empty = no timestamps); kept an
    /// open string list (Evergreen). Deprecated upstream in favor of
    /// [`TranscriptionMode::Verbatim`] in `mode`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_granularities: Option<Vec<String>>,
    /// Transcription mode. It changes the output on
    /// [`DEFAULT_TRANSCRIPTION_MODEL`](crate::DEFAULT_TRANSCRIPTION_MODEL);
    /// general models validate it and accept it without effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<TranscriptionMode>,
    /// Fields this crate does not model, kept from a deserialized config (such
    /// as one a stored interaction serves) and sent flat beside the modeled
    /// ones (D-001).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Transcription mode (`transcription_config.mode`).
///
/// Serializes as the tagged object (`{"type": "smart", ...}`,
/// `{"type": "verbatim", ...}`); the bare strings `"smart"` / `"verbatim"`
/// the API also accepts deserialize to the same variants. A known mode keeps
/// the fields the crate does not model in its `extra` map
/// ([`TranscriptionMode::extra`]); build one with
/// [`TranscriptionMode::smart`] or [`TranscriptionMode::verbatim`], which
/// leave it empty.
///
/// A mode changes the output on a transcription model
/// ([`DEFAULT_TRANSCRIPTION_MODEL`](crate::DEFAULT_TRANSCRIPTION_MODEL));
/// general models accept it without effect.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TranscriptionMode {
    /// Smart transcription.
    Smart {
        /// The text the audio is expected to say, such as the script a
        /// speaker read. The transcript follows its wording and spelling
        /// where the audio agrees with it.
        source_text: Option<String>,
        /// Unmodeled wire fields, kept for round trip
        /// ([`TranscriptionMode::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Verbatim transcription.
    Verbatim {
        /// Speaker diarization; `"speaker"` is the documented value.
        diarization_mode: Option<String>,
        /// Timestamp granularities; `"word"` is the documented value.
        timestamp_granularities: Option<Vec<String>>,
        /// Unmodeled wire fields, kept for round trip
        /// ([`TranscriptionMode::extra`]).
        extra: serde_json::Map<String, serde_json::Value>,
    },
    /// Unknown variant for forward compatibility (Evergreen pattern).
    // Serializes as `data` exactly as captured; `mode_type` is ignored (see
    // `serialize_unknown_data`), so an unknown mode read as a bare string
    // goes back out as that string.
    Unknown {
        /// The unrecognized mode type from the API.
        mode_type: String,
        /// The raw JSON value, preserved for roundtrip.
        data: serde_json::Value,
    },
}

impl TranscriptionMode {
    /// A smart mode, guided by `source_text` when given.
    #[must_use]
    pub fn smart(source_text: Option<String>) -> Self {
        Self::Smart {
            source_text,
            extra: serde_json::Map::new(),
        }
    }

    /// A verbatim mode with the given diarization and timestamp
    /// granularities (`Some("speaker")` and `Some(vec!["word"])` are the
    /// documented values).
    #[must_use]
    pub fn verbatim(
        diarization_mode: Option<String>,
        timestamp_granularities: Option<Vec<String>>,
    ) -> Self {
        Self::Verbatim {
            diarization_mode,
            timestamp_granularities,
            extra: serde_json::Map::new(),
        }
    }

    /// The fields of this mode that the crate does not model, kept so they
    /// round-trip (#491). `None` for [`TranscriptionMode::Unknown`].
    #[must_use]
    pub fn extra(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        match self {
            Self::Smart { extra, .. } | Self::Verbatim { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
        }
    }

    /// Mutable access to [`extra`](Self::extra).
    pub fn extra_mut(&mut self) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
        match self {
            Self::Smart { extra, .. } | Self::Verbatim { extra, .. } => Some(extra),
            Self::Unknown { .. } => None,
        }
    }

    /// Returns true if this is an unknown mode.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized mode type, if this is unknown.
    #[must_use]
    pub fn unknown_mode_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { mode_type, .. } => Some(mode_type),
            _ => None,
        }
    }

    /// Returns the preserved JSON, if this is unknown.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

/// The modeled fields, then the extras that do not name one (#491).
impl Serialize for TranscriptionMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Smart { source_text, extra } => {
                let mut map = ModeledMap::tagged(serializer, "smart")?;
                map.opt("source_text", source_text.as_ref())?;
                map.end(extra)
            }
            Self::Verbatim {
                diarization_mode,
                timestamp_granularities,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "verbatim")?;
                map.opt("diarization_mode", diarization_mode.as_ref())?;
                map.opt("timestamp_granularities", timestamp_granularities.as_ref())?;
                map.end(extra)
            }
            Self::Unknown { mode_type, data } => {
                crate::serde_util::serialize_unknown_data(mode_type, data, serializer)
            }
        }
    }
}

/// Reads a known mode, in its string or object form; an error keeps it as
/// [`TranscriptionMode::Unknown`].
fn read_known_mode(value: &serde_json::Value) -> Result<TranscriptionMode, Fallback> {
    let empty = serde_json::Map::new();
    let (tag, object) = match value {
        serde_json::Value::String(tag) => (tag.as_str(), &empty),
        serde_json::Value::Object(object) => {
            let tag = object.get("type").and_then(serde_json::Value::as_str);
            (tag.ok_or(Fallback::UnknownType)?, object)
        }
        _ => return Err(Fallback::UnknownType),
    };
    let mut f = WireFields::tagged(object);
    Ok(match tag {
        "smart" => TranscriptionMode::Smart {
            source_text: f.optional("source_text")?,
            extra: f.extra(),
        },
        "verbatim" => TranscriptionMode::Verbatim {
            diarization_mode: f.optional("diarization_mode")?,
            timestamp_granularities: f.optional("timestamp_granularities")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    })
}

impl<'de> Deserialize<'de> for TranscriptionMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match read_known_mode(&value) {
            Ok(mode) => Ok(mode),
            Err(fallback) => {
                let mode_type = value
                    .as_str()
                    .or_else(|| value.get("type").and_then(serde_json::Value::as_str))
                    .unwrap_or("<missing type>")
                    .to_string();
                tracing::warn!(
                    "Keeping a transcription mode whole in TranscriptionMode::Unknown: {} \
                     (Evergreen)",
                    fallback.describe("TranscriptionMode", &mode_type)
                );
                Ok(Self::Unknown {
                    mode_type,
                    data: value,
                })
            }
        }
    }
}

impl TranscriptionConfig {
    /// Create a new transcription configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the transcription mode.
    #[must_use]
    pub fn with_mode(mut self, mode: TranscriptionMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set the BCP-47 language hints (omitted means automatic detection).
    #[must_use]
    pub fn with_language_codes(
        mut self,
        language_codes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.language_codes = Some(language_codes.into_iter().map(Into::into).collect());
        self
    }

    /// Set the diarization mode (`"speaker"` is the only value the SDK
    /// spec documents today; open string per the Evergreen posture).
    #[must_use]
    pub fn with_diarization_mode(mut self, mode: impl Into<String>) -> Self {
        self.diarization_mode = Some(mode.into());
        self
    }

    /// Set the timestamp granularities (`"word"` is the only value the SDK
    /// spec documents today; open string list per the Evergreen posture).
    #[must_use]
    pub fn with_timestamp_granularities(
        mut self,
        granularities: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.timestamp_granularities = Some(granularities.into_iter().map(Into::into).collect());
        self
    }

    /// Set the domain-specific vocabulary to bias recognition toward,
    /// replacing any set earlier.
    #[must_use]
    pub fn with_custom_vocabulary(
        mut self,
        vocabulary: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.custom_vocabulary = Some(vocabulary.into_iter().map(Into::into).collect());
        self
    }

    /// Set the contextual-adaptation phrases, replacing any set earlier.
    ///
    /// This is the one `Vec` field here with an accumulating `add_*`
    /// companion — phrases are the incremental-assembly case; the other
    /// list fields take a pre-built `Vec` via their `with_*` setter.
    #[must_use]
    pub fn with_adaptation_phrases(
        mut self,
        phrases: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.adaptation_phrases = Some(phrases.into_iter().map(Into::into).collect());
        self
    }

    /// Add a single contextual-adaptation phrase, accumulating with any
    /// added earlier.
    #[must_use]
    pub fn add_adaptation_phrase(mut self, phrase: impl Into<String>) -> Self {
        self.adaptation_phrases
            .get_or_insert_with(Vec::new)
            .push(phrase.into());
        self
    }
}

/// Deserializes `speech_config` from any of its three observed wire forms,
/// normalizing all of them to the list the crate models.
///
/// | Wire | Source |
/// |------|--------|
/// | `[{voice, language, speaker}, ...]` | the spec list form; the one the crate sends |
/// | `{"speakers": [...]}` | `SpeakerConfig`, added to the union in `google-genai` 2.18.x |
/// | `{voice, language, speaker}` | legacy single-object form |
///
/// The Gemini API rejects the single-object form on **send** and once
/// rejected `{"speakers": [...]}` too (both forms' history is under
/// speech_config wire forms in `docs/ENUM_WIRE_FORMATS.md`), so this
/// leniency is deserialize-only and the crate keeps emitting the list,
/// which every TTS model accepts. It still matters: a stored interaction
/// read back serves the `{"speakers": [...]}` form, and a
/// `GenerationConfig` also arrives nested inside a `Trigger`'s stored
/// interaction, which may have been created by another SDK using the object
/// form.
///
/// Variant order is load-bearing. `SpeechConfig`'s fields are all optional
/// and its `extra` map takes every key it does not model, so
/// `{"speakers": [...]}` matches the `Single` arm perfectly well — yielding
/// a config whose modeled fields are all `None`, with **the speakers left
/// unparsed in `extra`**. `Speakers` must therefore be tried
/// first, and its field must stay required so a genuine single object still
/// falls through to `Single`.
fn deserialize_speech_configs<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<SpeechConfig>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SpeechConfigWire {
        List(Vec<SpeechConfig>),
        // Must precede `Single` — see the doc comment above.
        Speakers { speakers: Vec<SpeechConfig> },
        Single(SpeechConfig),
    }

    Ok(
        Option::<SpeechConfigWire>::deserialize(deserializer)?.map(|value| match value {
            SpeechConfigWire::List(list) | SpeechConfigWire::Speakers { speakers: list } => list,
            SpeechConfigWire::Single(single) => vec![single],
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
///     ..Default::default()
/// };
/// ```
///
/// # Available Voices
///
/// Common voices include: Aoede, Charon, Fenrir, Kore, Puck, and others.
/// See [Google's TTS documentation](https://aistudio.google.com/docs/speech-generation)
/// for the full list of available voices.
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
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
    /// Fields this crate does not model, kept from a deserialized config (such
    /// as one a stored interaction serves) and sent flat beside the modeled
    /// ones (D-001).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

    /// Creates the config for one speaker of a multi-speaker request; pair
    /// it with [`Content::speaker_text`](crate::Content::speaker_text).
    ///
    /// ```
    /// use genai_rs::SpeechConfig;
    ///
    /// let alice = SpeechConfig::for_speaker("Alice", "Kore", "en-US");
    /// assert_eq!(alice.speaker.as_deref(), Some("Alice"));
    /// ```
    #[must_use]
    pub fn for_speaker(
        speaker: impl Into<String>,
        voice: impl Into<String>,
        language: impl Into<String>,
    ) -> Self {
        Self {
            voice: Some(voice.into()),
            language: Some(language.into()),
            speaker: Some(speaker.into()),
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
///     ..Default::default()
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
    /// Fields this crate does not model, kept from a deserialized config (such
    /// as one a stored interaction serves) and sent flat beside the modeled
    /// ones (D-001).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

wire_enum! {
    /// Aspect ratio for image generation output.
    ///
    /// # Wire Format
    ///
    /// Values serialize as string ratios: `"1:1"`, `"16:9"`, etc.
    pub enum ImageAspectRatio {
        /// 1:1 square
        Square = "1:1",
        /// 2:3 portrait
        Portrait2x3 = "2:3",
        /// 3:2 landscape
        Landscape3x2 = "3:2",
        /// 3:4 portrait
        Portrait3x4 = "3:4",
        /// 4:3 landscape
        Landscape4x3 = "4:3",
        /// 4:5 portrait
        Portrait4x5 = "4:5",
        /// 5:4 landscape
        Landscape5x4 = "5:4",
        /// 9:16 tall portrait
        Portrait9x16 = "9:16",
        /// 16:9 widescreen
        Widescreen16x9 = "16:9",
        /// 21:9 ultrawide
        Ultrawide21x9 = "21:9",
        /// 1:8 very tall
        Tall1x8 = "1:8",
        /// 8:1 very wide
        Wide8x1 = "8:1",
        /// 1:4 tall
        Tall1x4 = "1:4",
        /// 4:1 wide
        Wide4x1 = "4:1",
    }
    unknown(ratio_type, unknown_ratio_type)
}

wire_enum! {
    /// Image size/resolution for image generation output.
    ///
    /// # Wire Format
    ///
    /// Values serialize as strings: `"512"`, `"1K"`, `"2K"`, `"4K"`.
    pub enum ImageSize {
        /// 512px resolution
        Sd512 = "512",
        /// 1K resolution
        Hd1k = "1K",
        /// 2K resolution
        Hd2k = "2K",
        /// 4K resolution
        Uhd4k = "4K",
    }
    unknown(size_type, unknown_size_type)
}

wire_enum! {
    /// Task mode for video generation.
    ///
    /// If not specified, the model automatically determines the appropriate mode
    /// based on the provided text prompt and input media.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase snake_case strings: `"text_to_video"`,
    /// `"image_to_video"`, `"reference_to_video"`, `"edit"`, `"extend"`.
    /// The full value list is confirmed by the API's own validation error
    /// for `generation_config.video_config.task`.
    pub enum VideoTask {
        /// Generate a video from a text prompt.
        TextToVideo = "text_to_video",
        /// Generate a video from an input image.
        ImageToVideo = "image_to_video",
        /// Generate a video from reference media.
        ReferenceToVideo = "reference_to_video",
        /// Edit an existing video.
        Edit = "edit",
        /// Extend an existing video.
        Extend = "extend",
    }
    unknown(task_type, unknown_task_type)
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
    /// Fields this crate does not model, kept from a deserialized config (such
    /// as one a stored interaction serves) and sent flat beside the modeled
    /// ones (D-001).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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

#[cfg(test)]
#[path = "generation_config_tests.rs"]
mod tests;
