//! [`GenerationConfig`]'s wire form, hand-written because one field's form
//! depends on another: `speech_config` is written as the list, or in its
//! `{"speakers": [...]}` form when it was read in that form with keys beside
//! the list ([`GenerationConfig::speech_config_extra`]).

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::{
    GenerationConfig, ImageConfig, SpeechConfig, ThinkingLevel, ThinkingSummaries,
    TranscriptionConfig, VideoConfig,
};
use crate::tools::ToolChoice;

impl Serialize for GenerationConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Destructured, so a field added to the struct fails to compile here
        // until it is written.
        let Self {
            temperature,
            max_output_tokens,
            top_p,
            thinking_level,
            seed,
            stop_sequences,
            thinking_summaries,
            tool_choice,
            speech_config,
            speech_config_extra,
            image_config,
            video_config,
            transcription_config,
            extra,
        } = self;
        let mut map = serializer.serialize_map(None)?;
        if let Some(value) = temperature {
            map.serialize_entry("temperature", value)?;
        }
        if let Some(value) = max_output_tokens {
            map.serialize_entry("max_output_tokens", value)?;
        }
        if let Some(value) = top_p {
            map.serialize_entry("top_p", value)?;
        }
        if let Some(value) = thinking_level {
            map.serialize_entry("thinking_level", value)?;
        }
        if let Some(value) = seed {
            map.serialize_entry("seed", value)?;
        }
        if let Some(value) = stop_sequences {
            map.serialize_entry("stop_sequences", value)?;
        }
        if let Some(value) = thinking_summaries {
            map.serialize_entry("thinking_summaries", value)?;
        }
        if let Some(value) = tool_choice {
            map.serialize_entry("tool_choice", value)?;
        }
        if let Some(speakers) = speech_config {
            if speech_config_extra.is_empty() {
                map.serialize_entry("speech_config", speakers)?;
            } else {
                map.serialize_entry(
                    "speech_config",
                    &SpeakersForm {
                        speakers,
                        extra: speech_config_extra,
                    },
                )?;
            }
        }
        if let Some(value) = image_config {
            map.serialize_entry("image_config", value)?;
        }
        if let Some(value) = video_config {
            map.serialize_entry("video_config", value)?;
        }
        if let Some(value) = transcription_config {
            map.serialize_entry("transcription_config", value)?;
        }
        for (key, value) in extra {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// `speech_config` in its `{"speakers": [...]}` form: the list, then the
/// keys read beside it (a `speakers` among them is not written).
struct SpeakersForm<'a> {
    speakers: &'a [SpeechConfig],
    extra: &'a Map<String, Value>,
}

impl Serialize for SpeakersForm<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("speakers", self.speakers)?;
        for (key, value) in self.extra {
            if key != "speakers" {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for GenerationConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// The keys as they arrive; `speech_config` with what its
        /// `{"speakers": [...]}` form carried beside the list.
        #[derive(Deserialize)]
        struct Raw {
            #[serde(
                default,
                deserialize_with = "crate::serde_util::deserialize_f32_in_range"
            )]
            temperature: Option<f32>,
            max_output_tokens: Option<i32>,
            #[serde(
                default,
                deserialize_with = "crate::serde_util::deserialize_f32_in_range"
            )]
            top_p: Option<f32>,
            thinking_level: Option<ThinkingLevel>,
            seed: Option<i64>,
            stop_sequences: Option<Vec<String>>,
            thinking_summaries: Option<ThinkingSummaries>,
            tool_choice: Option<ToolChoice>,
            #[serde(default, deserialize_with = "deserialize_speech_configs")]
            speech_config: Option<SpeakersRead>,
            image_config: Option<ImageConfig>,
            video_config: Option<VideoConfig>,
            transcription_config: Option<TranscriptionConfig>,
            #[serde(flatten)]
            extra: Map<String, Value>,
        }

        let raw = Raw::deserialize(deserializer)?;
        let (speech_config, speech_config_extra) = match raw.speech_config {
            Some((speakers, extra)) => (Some(speakers), extra),
            None => (None, Map::new()),
        };
        Ok(Self {
            temperature: raw.temperature,
            max_output_tokens: raw.max_output_tokens,
            top_p: raw.top_p,
            thinking_level: raw.thinking_level,
            seed: raw.seed,
            stop_sequences: raw.stop_sequences,
            thinking_summaries: raw.thinking_summaries,
            tool_choice: raw.tool_choice,
            speech_config,
            speech_config_extra,
            image_config: raw.image_config,
            video_config: raw.video_config,
            transcription_config: raw.transcription_config,
            extra: raw.extra,
        })
    }
}

/// `speech_config` as read: the list, and the keys its `{"speakers": [...]}`
/// form carried beside it.
type SpeakersRead = (Vec<SpeechConfig>, Map<String, Value>);

/// Deserializes `speech_config` from any of its three observed wire forms,
/// normalizing all of them to the list the crate models, with the keys the
/// `{"speakers": [...]}` form carried beside it.
///
/// | Wire | Source |
/// |------|--------|
/// | `[{voice, language, speaker}, ...]` | the spec list form; the one the crate sends |
/// | `{"speakers": [...]}` | `SpeakerConfig`, added to the union in `google-genai` 2.18.x |
/// | `{voice, language, speaker}` | legacy single-object form |
///
/// The Gemini API rejects the single-object form on **send** and once
/// rejected `{"speakers": [...]}` too (both forms' history is under
/// speech_config wire forms in `docs/ENUM_WIRE_FORMATS.md`), so the crate
/// sends the list unless keys came beside `speakers`, which only the
/// `{"speakers": [...]}` form can carry. The leniency still matters: a
/// stored interaction read back serves the `{"speakers": [...]}` form, and a
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
fn deserialize_speech_configs<'de, D>(deserializer: D) -> Result<Option<SpeakersRead>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SpeechConfigWire {
        List(Vec<SpeechConfig>),
        // Must precede `Single` — see the doc comment above.
        Speakers {
            speakers: Vec<SpeechConfig>,
            #[serde(flatten)]
            extra: Map<String, Value>,
        },
        Single(SpeechConfig),
    }

    Ok(
        Option::<SpeechConfigWire>::deserialize(deserializer)?.map(|value| match value {
            SpeechConfigWire::List(list) => (list, Map::new()),
            SpeechConfigWire::Speakers { speakers, extra } => (speakers, extra),
            SpeechConfigWire::Single(single) => (vec![single], Map::new()),
        }),
    )
}
