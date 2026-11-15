//! The create request of the voices resource: [`CreateVoiceRequest`] and
//! the [`VoiceSpec`] and [`ReplicatedVoice`] it sends.

use super::{PromptedVoice, VoiceAudio, VoicePitch, VoiceType};
use crate::redact;
use serde::Serialize;

/// The voice definition inside a [`CreateVoiceRequest`].
///
/// `Debug` prints [`extra`](Self::extra) through `LOUD_WIRE`'s redaction,
/// so a replicated voice's key ([`Voice::key`](super::Voice::key)) sent there prints as
/// `[REDACTED]`, as it does on the wire.
#[derive(Clone, Default, Serialize, PartialEq)]
pub struct VoiceSpec {
    /// How to make the voice.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub voice_type: Option<VoiceType>,
    /// Prompt, required for [`VoiceType::Prompted`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompted: Option<PromptedVoice>,
    /// Source and consent audio, required for [`VoiceType::Replicated`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicated: Option<ReplicatedVoice>,
    /// Display name for a stored voice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Model to design with; the API defaults to its latest voice model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Summary of timbre, personality and tone.
    ///
    /// This and the discovery fields below are stored with the voice, served
    /// back on create, get and list, and matched by [`ListVoices`](super::ListVoices)'s
    /// filters. They are free text, except
    /// [`pitch`](Self::pitch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// BCP-47 language tag, e.g. `en-US`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
    /// Region code, e.g. `US`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region_code: Option<String>,
    /// Accent descriptor, e.g. `General American`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// Persona, e.g. `Storyteller & Narrator`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// Usage context, e.g. `Content & Media`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Perceived gender presentation, e.g. `male`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    /// Perceived pitch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<VoicePitch>,
    /// Anything else to send that is not modeled here.
    ///
    /// The bindings' `sample_audio` is output only: a value sent on create
    /// is ignored, and the response carries a sample the API generated, so
    /// it is read on [`Voice::sample_audio`](super::Voice::sample_audio) and not
    /// modeled here.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// `extra` can carry a replicated voice's key, a secret (D-067): printed
// through the wire's walk, as `PromptedVoice`'s is.
impl std::fmt::Debug for VoiceSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            voice_type,
            prompted,
            replicated,
            display_name,
            model,
            description,
            language_code,
            region_code,
            accent,
            persona,
            context,
            gender,
            pitch,
            extra,
        } = self;
        f.debug_struct("VoiceSpec")
            .field("voice_type", voice_type)
            .field("prompted", prompted)
            .field("replicated", replicated)
            .field("display_name", display_name)
            .field("model", model)
            .field("description", description)
            .field("language_code", language_code)
            .field("region_code", region_code)
            .field("accent", accent)
            .field("persona", persona)
            .field("context", context)
            .field("gender", gender)
            .field("pitch", pitch)
            .field("extra", &redact::map(extra))
            .finish()
    }
}

/// Source and consent recordings for a replicated voice.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplicatedVoice {
    /// Audio of the voice to replicate.
    pub source_audio: VoiceAudio,
    /// The speaker's recorded consent.
    pub consent_audio: VoiceAudio,
}

/// Request body for [`Voices::create`](super::Voices::create).
///
/// ```
/// use genai_rs::CreateVoiceRequest;
///
/// let request = CreateVoiceRequest::prompted("A calm, low-pitched narrator.")
///     .with_display_name("narrator");
/// assert_eq!(request.store, Some(true));
/// ```
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct CreateVoiceRequest {
    /// The voice to create.
    pub voice: VoiceSpec,
    /// Whether Google stores the voice (returns an `id`) or returns a
    /// client-held `key`. Prompted voices require `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
}

impl CreateVoiceRequest {
    /// A voice designed from `prompt`, stored by Google (the API rejects
    /// prompted voices with `store: false`).
    #[must_use]
    pub fn prompted(prompt: impl Into<String>) -> Self {
        Self {
            voice: VoiceSpec {
                voice_type: Some(VoiceType::Prompted),
                prompted: Some(PromptedVoice::new(prompt)),
                ..Default::default()
            },
            store: Some(true),
        }
    }

    /// A voice replicated from `source` audio, with the speaker's recorded
    /// `consent`. Not stored unless [`with_store`](Self::with_store) is set;
    /// the response then carries a `key` instead of an `id`.
    #[must_use]
    pub fn replicated(source: VoiceAudio, consent: VoiceAudio) -> Self {
        Self {
            voice: VoiceSpec {
                voice_type: Some(VoiceType::Replicated),
                replicated: Some(ReplicatedVoice {
                    source_audio: source,
                    consent_audio: consent,
                }),
                ..Default::default()
            },
            store: None,
        }
    }

    /// Sets the display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.voice.display_name = Some(name.into());
        self
    }

    /// Starts a prompted voice's design from a prebuilt voice, such as
    /// `"achird"`. A replicated voice has no prompt to base, so there it
    /// only logs a warning.
    #[must_use]
    pub fn with_base_voice(mut self, voice: impl Into<String>) -> Self {
        match self.voice.prompted.take() {
            Some(prompted) => self.voice.prompted = Some(prompted.with_base_voice(voice)),
            None => tracing::warn!(
                "CreateVoiceRequest::with_base_voice has no effect on a replicated voice"
            ),
        }
        self
    }

    /// Sets whether Google stores the voice.
    #[must_use]
    pub const fn with_store(mut self, store: bool) -> Self {
        self.store = Some(store);
        self
    }

    /// Sets the description, a summary of timbre, personality and tone.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.voice.description = Some(description.into());
        self
    }

    /// Sets the language, a BCP-47 tag such as `en-US`.
    #[must_use]
    pub fn with_language_code(mut self, language_code: impl Into<String>) -> Self {
        self.voice.language_code = Some(language_code.into());
        self
    }

    /// Sets the region, e.g. `US`.
    #[must_use]
    pub fn with_region_code(mut self, region_code: impl Into<String>) -> Self {
        self.voice.region_code = Some(region_code.into());
        self
    }

    /// Sets the accent, e.g. `General American`.
    #[must_use]
    pub fn with_accent(mut self, accent: impl Into<String>) -> Self {
        self.voice.accent = Some(accent.into());
        self
    }

    /// Sets the persona, e.g. `Storyteller & Narrator`.
    #[must_use]
    pub fn with_persona(mut self, persona: impl Into<String>) -> Self {
        self.voice.persona = Some(persona.into());
        self
    }

    /// Sets the usage context, e.g. `Content & Media`.
    #[must_use]
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.voice.context = Some(context.into());
        self
    }

    /// Sets the gender presentation, e.g. `male`.
    #[must_use]
    pub fn with_gender(mut self, gender: impl Into<String>) -> Self {
        self.voice.gender = Some(gender.into());
        self
    }

    /// Sets the pitch.
    #[must_use]
    pub fn with_pitch(mut self, pitch: VoicePitch) -> Self {
        self.voice.pitch = Some(pitch);
        self
    }
}
