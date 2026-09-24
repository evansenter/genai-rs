//! Request types for creating interactions.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::content::Content;
use crate::environment::EnvironmentSpec;
use crate::response_format::ResponseFormatSpec;
use crate::safety::SafetySetting;
use crate::steps::Step;
use crate::tools::{Tool, ToolChoice};
use crate::webhooks::WebhookConfig;
use crate::wire_enum::wire_enum;

/// Who a conversation turn comes from, for
/// [`ConversationBuilder::turn`](crate::ConversationBuilder::turn).
///
/// Client-side only: under revision 2026-05-20 steps carry no role field on
/// the wire, their step type (`user_input` / `model_output`) does that job.
///
/// # Example
///
/// ```
/// use genai_rs::Role;
///
/// let role = Role::User;
/// assert!(matches!(role, Role::User));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Role {
    /// Content from the user
    User,
    /// Content from the model
    Model,
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
/// - `Content`: Array of content blocks for multimodal input — sent as a
///   single `user_input` step, not as a bare array
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
    /// Content blocks for a single user turn (multimodal input).
    ///
    /// Serialized as one `user_input` step wrapping the blocks, not as a
    /// bare content array. Both are valid input shapes, but only the step
    /// form accepts video `processing`. Deserializing that wire
    /// shape back yields [`Self::Steps`], since the two are indistinguishable
    /// on the wire.
    Content(Vec<Content>),
    /// Array of steps (multi-turn conversation history, function results,
    /// thought signatures, ...)
    Steps(Vec<Step>),
}

impl Default for InteractionInput {
    /// An empty text input — the zero value for struct-literal
    /// construction of [`InteractionRequest`]; replace it with real input
    /// before sending.
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl Serialize for InteractionInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Faithful to the variant, including the bare content array. The
        // request-side wrap lives on [`InteractionRequest::input`] instead —
        // see [`serialize_request_input`] — because it is a decision about
        // authoring a request, not a property of the type. `InteractionInput`
        // is also what [`InteractionResponse::input`] echoes back, and
        // re-serializing server data into a shape the server did not send
        // would work against the Evergreen roundtrip principle.
        match self {
            Self::Text(t) => serializer.serialize_str(t),
            Self::Content(c) => c.serialize(serializer),
            Self::Steps(s) => s.serialize(serializer),
        }
    }
}

/// Serializer for [`InteractionRequest::input`]: emits
/// [`InteractionInput::Content`] as a single `user_input` step rather than as
/// a bare content array.
///
/// Both are valid arms of the input union, but the API accepts video
/// `processing` only inside a step; the same content in a bare array is
/// rejected with `Unknown parameter 'processing'`. The step form is accepted
/// everywhere the bare one is (verified live for text, inline image, audio,
/// document, video by URI, and stored follow-ups). See
/// `docs/ENUM_WIRE_FORMATS.md`.
///
/// Scoped to this field rather than to `InteractionInput`'s own `Serialize`
/// so that [`InteractionResponse::input`](crate::InteractionResponse) keeps
/// re-serializing server data in the shape it arrived in.
fn serialize_request_input<S>(input: &InteractionInput, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // Exhaustive on purpose: a new `InteractionInput` arm must make a
    // wrap-or-not decision here rather than default to one by omission.
    match input {
        InteractionInput::Content(c) => {
            use serde::ser::SerializeSeq;
            let mut seq = serializer.serialize_seq(Some(1))?;
            seq.serialize_element(&UserInputRef { content: c })?;
            seq.end()
        }
        InteractionInput::Text(_) | InteractionInput::Steps(_) => input.serialize(serializer),
    }
}

/// A borrowed `user_input` step, for wrapping [`InteractionInput::Content`]
/// on the way out.
///
/// Serializes byte-identically to [`Step::UserInput`]; it exists only so the
/// wrap does not clone the content vector on every serialization.
struct UserInputRef<'a> {
    content: &'a [Content],
}

impl Serialize for UserInputRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", "user_input")?;
        map.serialize_entry("content", self.content)?;
        map.end()
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
        input_from_value(value).map_err(serde::de::Error::custom)
    }
}

/// The by-`Value` body of [`InteractionInput`]'s deserializer, shared with
/// the lenient trigger-side probe so neither path buffers the tree twice.
pub(crate) fn input_from_value(value: serde_json::Value) -> Result<InteractionInput, String> {
    match value {
        serde_json::Value::String(s) => Ok(InteractionInput::Text(s)),
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
                    .map_err(|e| e.to_string())?;
                Ok(InteractionInput::Content(contents))
            } else {
                let steps = serde_json::from_value(serde_json::Value::Array(items))
                    .map_err(|e| e.to_string())?;
                Ok(InteractionInput::Steps(steps))
            }
        }
        other @ serde_json::Value::Object(_) => {
            // A single content or step object.
            let is_content = other
                .get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| CONTENT_TYPE_TAGS.contains(&t));
            if is_content {
                let content: Content = serde_json::from_value(other).map_err(|e| e.to_string())?;
                Ok(InteractionInput::Content(vec![content]))
            } else {
                let step: Step = serde_json::from_value(other).map_err(|e| e.to_string())?;
                Ok(InteractionInput::Steps(vec![step]))
            }
        }
        other => Err(format!(
            "InteractionInput must be a string, array, or object; got {other}"
        )),
    }
}

wire_enum! {
    /// Thinking level for chain-of-thought reasoning.
    ///
    /// Controls the depth of reasoning the model performs before generating a response.
    /// Higher levels produce more detailed reasoning but consume more tokens.
    pub enum ThinkingLevel {
        /// Minimal reasoning, fastest responses.
        ///
        /// Not supported by every model: [`DEFAULT_MODEL`](crate::DEFAULT_MODEL)
        /// rejects it with a 400 (verified live 2026-08-15). Use
        /// [`MINIMAL_THINKING_MODEL`](crate::MINIMAL_THINKING_MODEL), which is
        /// pinned to a model that accepts it.
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
    /// Three forms are accepted on deserialize, all normalizing to the list:
    /// the list itself, a bare single object (legacy), and a
    /// `{"speakers": [...]}` wrapper (the spec's `SpeakerConfig`). Only the
    /// list is ever sent.
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
}

/// Audio transcription configuration.
///
/// Set via
/// [`GenerationConfig::transcription_config`] to control how audio input is
/// transcribed.
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
    /// Transcription mode. Accepted and validated live (2026-09-24); no
    /// output difference was observed on general models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<TranscriptionMode>,
}

/// Transcription mode (`transcription_config.mode`).
///
/// Serializes as the tagged object (`{"type": "smart"}`,
/// `{"type": "verbatim", ...}`); the bare strings `"smart"` / `"verbatim"`
/// the API also accepts deserialize to the same variants.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum TranscriptionMode {
    /// Smart transcription.
    Smart,
    /// Verbatim transcription.
    Verbatim {
        /// Speaker diarization; `"speaker"` is the documented value.
        diarization_mode: Option<String>,
        /// Timestamp granularities; `"word"` is the documented value.
        timestamp_granularities: Option<Vec<String>>,
    },
    /// Unknown variant for forward compatibility (Evergreen pattern).
    Unknown {
        /// The unrecognized mode type from the API.
        mode_type: String,
        /// The raw JSON value, preserved for roundtrip.
        data: serde_json::Value,
    },
}

impl TranscriptionMode {
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

impl Serialize for TranscriptionMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Smart => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "smart")?;
                map.end()
            }
            Self::Verbatim {
                diarization_mode,
                timestamp_granularities,
            } => {
                let mut map = serializer.serialize_map(None)?;
                map.serialize_entry("type", "verbatim")?;
                if let Some(d) = diarization_mode {
                    map.serialize_entry("diarization_mode", d)?;
                }
                if let Some(t) = timestamp_granularities {
                    map.serialize_entry("timestamp_granularities", t)?;
                }
                map.end()
            }
            Self::Unknown { data, .. } => data.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for TranscriptionMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let tag = value
            .as_str()
            .or_else(|| value.get("type").and_then(|t| t.as_str()));
        let strings = |key: &str| -> Option<Vec<String>> {
            serde_json::from_value(value.get(key)?.clone()).ok()
        };
        match tag {
            Some("smart") => Ok(Self::Smart),
            Some("verbatim") => Ok(Self::Verbatim {
                diarization_mode: value
                    .get("diarization_mode")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                timestamp_granularities: strings("timestamp_granularities"),
            }),
            other => {
                let mode_type = other.unwrap_or("<missing type>").to_string();
                tracing::warn!(
                    "Encountered unknown TranscriptionMode '{mode_type}' - using Unknown variant (Evergreen)"
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
/// | `[{voice, language, speaker}, ...]` | the spec list form; the only one the Gemini API accepts |
/// | `{"speakers": [...]}` | `SpeakerConfig`, added to the union in `google-genai` 2.18.x |
/// | `{voice, language, speaker}` | legacy single-object form |
///
/// The Gemini API rejects both object forms on **send** — `400 The value is
/// invalid for 'generation_config.speech_config'. Expected an array, got
/// object.` (verified live 2026-08-16) — so this leniency is deserialize-only
/// and the crate keeps emitting the list. It still matters: a
/// `GenerationConfig` also arrives nested inside a `Trigger`'s stored
/// interaction, which may have been created by another SDK using the object
/// form.
///
/// Variant order is load-bearing. `SpeechConfig`'s fields are all optional
/// and serde ignores unknown keys, so `{"speakers": [...]}` matches the
/// `Single` arm perfectly well — yielding an all-`None` config and
/// **silently discarding the speakers**. `Speakers` must therefore be tried
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
///     speaker: None,
/// };
/// ```
///
/// # Available Voices
///
/// Common voices include: Aoede, Charon, Fenrir, Kore, Puck, and others.
/// See [Google's TTS documentation](https://ai.google.dev/gemini-api/docs/text-generation)
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
    /// The full value list was confirmed live (2026-07) via the API's own
    /// validation error for `generation_config.video_config.task`.
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
///     .with_model(genai_rs::DEFAULT_MODEL)
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
/// #     .with_model(genai_rs::DEFAULT_MODEL)
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
/// #     .with_model(genai_rs::DEFAULT_MODEL)
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
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct InteractionRequest {
    /// Model name (e.g. [`DEFAULT_MODEL`](crate::DEFAULT_MODEL)) - mutually exclusive with agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Agent name (e.g. [`DEFAULT_DEEP_RESEARCH_AGENT`](crate::DEFAULT_DEEP_RESEARCH_AGENT)) - mutually exclusive with model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Agent-specific configuration (e.g., Deep Research thinking summaries)
    #[serde(rename = "agent_config", skip_serializing_if = "Option::is_none")]
    pub agent_config: Option<AgentConfig>,

    /// The input for this interaction.
    ///
    /// *Required* and strict on deserialize, so a typo in a config file
    /// feeding [`TriggerCreateParams`](crate::TriggerCreateParams) is a parse
    /// error rather than a scheduled empty prompt. (The lenient response side
    /// is [`Trigger::interaction`](crate::Trigger).)
    ///
    /// On the way out, [`InteractionInput::Content`] is wrapped in a single
    /// `user_input` step: the API accepts video `processing` only inside a
    /// step. Scoped to this field rather than to `InteractionInput`'s own
    /// `Serialize`, so [`InteractionResponse::input`](crate::InteractionResponse)
    /// still re-serializes server data in the shape it arrived in.
    #[serde(serialize_with = "serialize_request_input")]
    pub input: InteractionInput,

    /// Reference to a previous interaction for stateful conversations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,

    /// Tools available for function calling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,

    /// Response modalities (e.g., ["image"]; the API only accepts lowercase)
    ///
    /// Deprecation signal: the official SDK marks `response_modalities`
    /// (and `response_mime_type`, already removed from this crate) as
    /// deprecated in favor of the typed
    /// [`response_format`](Self::response_format) union — prefer
    /// [`ResponseFormatSpec`] for new code. Kept until the API removes it.
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

    /// Per-request webhook routing: deliver this request's events to the
    /// given URIs (instead of the registered webhooks) with optional
    /// user metadata echoed on each event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_config: Option<WebhookConfig>,

    /// Environment for the interaction: a string environment ID or a typed
    /// remote environment (sources + network allowlist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentSpec>,

    /// Safety settings for the interaction, one per harm category.
    ///
    /// Server-side constraint (verified live 2026-08-08): the Gemini API
    /// rejects `safety_settings` — "not available on the Gemini API but it
    /// is available on the Gemini Enterprise Agent Platform" (Vertex-only).
    /// The field is modeled for spec parity and forward compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,

    /// User-defined metadata labels for the request.
    ///
    /// Accepted by the Gemini API and echoed on the response as
    /// [`InteractionResponse::labels`](crate::InteractionResponse::labels)
    /// (verified live 2026-09-24; it was Vertex-only on 2026-08-08).
    ///
    /// `BTreeMap` (not `HashMap`) so the serialized key order is
    /// deterministic — wire captures and `LOUD_WIRE` diffs of the same
    /// logical request stay byte-identical across runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::BTreeMap<String, String>>,
}

wire_enum! {
    /// Latency/priority service tier for a request.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"flex"`, `"standard"`, `"priority"`.
    pub enum ServiceTier {
        /// Flexible latency, lower cost.
        Flex = "flex",
        /// Standard processing.
        Standard = "standard",
        /// Prioritized processing.
        Priority = "priority",
    }
    unknown(tier_type, unknown_tier_type)
}

// =============================================================================
// Agent Configuration Types
// =============================================================================

wire_enum! {
    /// Thinking summaries configuration for agent output.
    ///
    /// When using thinking mode (via `with_thinking_level`), you can control
    /// whether the model's reasoning process is summarized in the output.
    pub enum ThinkingSummaries {
        /// Automatically include thinking summaries (default when thinking is enabled)
        Auto = "auto" | "THINKING_SUMMARIES_AUTO",
        /// Do not include thinking summaries
        None = "none" | "THINKING_SUMMARIES_NONE",
    }
    unknown(summaries_type, unknown_summaries_type)
}

impl ThinkingSummaries {
    /// Convert to the `agent_config` wire format (`"auto"` / `"none"`).
    ///
    /// This used to emit the SCREAMING_CASE `THINKING_SUMMARIES_*` form,
    /// which the API accepted when `DeepResearchConfig` was written.
    /// Verified live 2026-08-10, it no longer does:
    ///
    /// ```text
    /// The value 'THINKING_SUMMARIES_AUTO' is not supported for
    /// 'agent_config.thinking_summaries'. Supported values: 'auto', 'none'.
    /// ```
    ///
    /// So both contexts now take the lowercase spelling and this agrees
    /// with [`Serialize`]. The seam is kept rather than inlined because
    /// the two spellings diverged once and could again; deserialization
    /// still accepts either form (Evergreen).
    #[must_use]
    pub fn to_agent_config_value(&self) -> serde_json::Value {
        match self {
            ThinkingSummaries::Auto => serde_json::Value::String("auto".to_string()),
            ThinkingSummaries::None => serde_json::Value::String("none".to_string()),
            ThinkingSummaries::Unknown { summaries_type, .. } => {
                // For unknown values, preserve the original format
                serde_json::Value::String(summaries_type.clone())
            }
        }
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

wire_enum! {
    /// Visualization mode for the Deep Research agent.
    ///
    /// Controls whether the agent includes visualizations in its response.
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"off"`, `"auto"`.
    pub enum Visualization {
        /// No visualizations in the response.
        Off = "off",
        /// The agent decides when to include visualizations.
        Auto = "auto",
    }
    unknown(visualization_type, unknown_visualization_type)
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

/// Configuration for the server-side Antigravity coding agent.
///
/// This configures [`DEFAULT_ANTIGRAVITY_AGENT`](crate::DEFAULT_ANTIGRAVITY_AGENT) interactions that
/// run in Google's sandbox (an
/// [`environment`](InteractionRequest::environment) is **required** for that
/// agent) — distinct from the local-harness bridge in the `antigravity`
/// module (feature `antigravity`), which runs the agent on your
/// machine. The bare `antigravity` string is only the `agent_config` *type*
/// discriminant (which [`From`] sets), not an agent ID.
///
/// Probe notes (verified live 2026-08-09, standard API key):
/// `agent_config: {"type": "antigravity"}` and `max_total_tokens` are
/// **accepted** on `antigravity-preview-05-2026` (the server's validation
/// error enumerates the supported config types as `dynamic`,
/// `deep-research`, `code-mender`, `antigravity`). Setting `model` to a
/// value the agent doesn't offer returns 404 `not_found` — as
/// `gemini-3.6-flash` did when this was recorded, despite being a valid
/// model for ordinary interactions. The agent's model catalog is not
/// enumerable on a standard key, so leave `model` unset unless you know an
/// accepted value.
///
/// # Example
///
/// ```
/// use genai_rs::{AgentConfig, AntigravityConfig};
///
/// let config: AgentConfig = AntigravityConfig::new()
///     .with_max_total_tokens(200_000)
///     .into();
/// ```
#[derive(Clone, Debug, Default)]
pub struct AntigravityConfig {
    model: Option<String>,
    max_total_tokens: Option<i64>,
}

impl AntigravityConfig {
    /// Create a new Antigravity agent configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the model the agent uses for its reasoning loop.
    ///
    /// A value the agent does not offer fails the interaction with 404
    /// `not_found` — see the [type docs](AntigravityConfig) for the probe
    /// notes; leave unset unless you know an accepted value.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Cap the total tokens the agent may consume across its whole run.
    #[must_use]
    pub fn with_max_total_tokens(mut self, max_total_tokens: i64) -> Self {
        self.max_total_tokens = Some(max_total_tokens);
        self
    }
}

impl From<AntigravityConfig> for AgentConfig {
    fn from(config: AntigravityConfig) -> Self {
        let mut map = serde_json::Map::new();
        map.insert(
            "type".into(),
            serde_json::Value::String("antigravity".into()),
        );
        if let Some(model) = config.model {
            map.insert("model".into(), serde_json::Value::String(model));
        }
        if let Some(max) = config.max_total_tokens {
            map.insert("max_total_tokens".into(), serde_json::Value::from(max));
        }
        AgentConfig(serde_json::Value::Object(map))
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
            serde_json::Value::String("auto".to_string())
        );

        assert_eq!(
            ThinkingSummaries::None.to_agent_config_value(),
            serde_json::Value::String("none".to_string())
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

    #[cfg(not(feature = "strict-unknown"))]
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
        assert_eq!(value["thinking_summaries"], "auto");
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
    fn test_transcription_mode_wire_forms() {
        let config = TranscriptionConfig::new().with_mode(TranscriptionMode::Verbatim {
            diarization_mode: Some("speaker".into()),
            timestamp_granularities: Some(vec!["word".into()]),
        });
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::json!({"mode": {
                "type": "verbatim",
                "diarization_mode": "speaker",
                "timestamp_granularities": ["word"]
            }})
        );
        for (wire, expected) in [
            (serde_json::json!("smart"), TranscriptionMode::Smart),
            (
                serde_json::json!({"type": "smart"}),
                TranscriptionMode::Smart,
            ),
            (
                serde_json::json!("verbatim"),
                TranscriptionMode::Verbatim {
                    diarization_mode: None,
                    timestamp_granularities: None,
                },
            ),
        ] {
            assert_eq!(
                serde_json::from_value::<TranscriptionMode>(wire).unwrap(),
                expected
            );
        }
        let unknown: TranscriptionMode =
            serde_json::from_value(serde_json::json!({"type": "future", "x": 1})).unwrap();
        assert_eq!(unknown.unknown_mode_type(), Some("future"));
        assert_eq!(
            serde_json::to_value(&unknown).unwrap(),
            serde_json::json!({"type": "future", "x": 1})
        );
    }

    #[test]
    fn test_speech_config_for_speaker() {
        let config = SpeechConfig::for_speaker("Bob", "Puck", "en-US");
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::json!({"voice": "Puck", "language": "en-US", "speaker": "Bob"})
        );
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
        // See docs/ENUM_WIRE_FORMATS.md ("SpeechConfig (generation_config)").
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

    #[cfg(not(feature = "strict-unknown"))]
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

    #[cfg(not(feature = "strict-unknown"))]
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

    #[cfg(not(feature = "strict-unknown"))]
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

    /// Serialize just the `input` field the way a request would.
    fn request_input_json(input: InteractionInput) -> serde_json::Value {
        let request = InteractionRequest {
            model: Some("test-model".into()),
            input,
            ..Default::default()
        };
        serde_json::to_value(&request).unwrap()["input"].clone()
    }

    /// `Content` input goes out wrapped in a `user_input` step, not as a bare
    /// content array — the shape the API accepts video `processing` in (#427).
    #[test]
    fn test_request_content_input_serializes_as_a_user_input_step() {
        let json = request_input_json(InteractionInput::Content(vec![
            Content::text("Describe briefly."),
            Content::from_uri_and_mime("files/clip", "video/mp4"),
        ]));

        assert!(json.is_array(), "input must still be an array: {json}");
        assert_eq!(json.as_array().unwrap().len(), 1, "one wrapping step");
        assert_eq!(json[0]["type"], "user_input");
        assert_eq!(
            json[0]["content"].as_array().map(Vec::len),
            Some(2),
            "both blocks are carried through unchanged: {json}"
        );
        assert_eq!(json[0]["content"][0]["type"], "text");
        assert_eq!(json[0]["content"][1]["type"], "video");
    }

    /// The wrap is byte-identical to building the step by hand, which is what
    /// lets the round-trip land on `Steps` rather than losing information.
    #[test]
    fn test_request_content_input_matches_a_hand_built_user_input_step() {
        let content = vec![Content::text("hi")];
        let wrapped = request_input_json(InteractionInput::Content(content.clone()));
        let by_hand = request_input_json(InteractionInput::Steps(vec![Step::user_input(content)]));
        assert_eq!(wrapped, by_hand);
    }

    /// Unconditional: an empty content vector produces the same shape rather
    /// than falling back to a bare `[]`, so the wire form never depends on
    /// how much content the caller happened to supply.
    #[test]
    fn test_empty_request_content_input_is_wrapped_too() {
        assert_eq!(
            request_input_json(InteractionInput::Content(vec![])),
            serde_json::json!([{"type": "user_input", "content": []}])
        );
    }

    /// The other two variants are untouched by the wrap.
    #[test]
    fn test_wrap_does_not_touch_text_or_steps_input() {
        assert_eq!(
            request_input_json(InteractionInput::Text("hi".into())),
            serde_json::json!("hi")
        );
        let json = request_input_json(InteractionInput::Steps(vec![Step::user_input(vec![
            Content::text("hi"),
        ])]));
        assert_eq!(json.as_array().map(Vec::len), Some(1));
        assert_eq!(json[0]["type"], "user_input");
    }

    /// The documented round-trip, asserted end to end rather than implied by
    /// chaining the serialize test with the steps-array parse test: a request
    /// built from `Content` comes back as `Steps` holding one `UserInput`,
    /// which is what the rustdoc and the CHANGELOG both claim.
    #[test]
    fn test_request_content_input_round_trips_as_a_user_input_step() {
        let request = InteractionRequest {
            model: Some("test-model".into()),
            input: InteractionInput::Content(vec![Content::text("hi")]),
            ..Default::default()
        };
        let json = serde_json::to_string(&request).unwrap();
        let back: InteractionRequest = serde_json::from_str(&json).unwrap();

        match &back.input {
            InteractionInput::Steps(steps) => {
                assert_eq!(steps.len(), 1, "one wrapping step, got {steps:?}");
                assert_eq!(
                    steps[0],
                    Step::user_input(vec![Content::text("hi")]),
                    "the step must carry the original content unchanged"
                );
            }
            other => panic!("expected Steps after the round trip, got {other:?}"),
        }
    }

    /// The wrap is request-only. `InteractionInput`'s own `Serialize` stays
    /// faithful to the variant, so a `Content` array echoed back on
    /// `InteractionResponse::input` re-serializes in the shape the server
    /// sent rather than being rewritten into a step.
    #[test]
    fn test_bare_input_serialization_is_unwrapped() {
        let input = InteractionInput::Content(vec![Content::text("hi")]);
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            serde_json::json!([{"type": "text", "text": "hi"}]),
            "the type's own Serialize must not wrap"
        );
    }
}
