//! Voices resource (`/v1beta/voices`).
//!
//! Lists the prebuilt TTS voice catalog (with descriptions and filters), and
//! creates custom voices — designed from a text prompt, or replicated from
//! sample audio. A voice's [`id`](Voice::id) (or a replicated voice's
//! [`key`](Voice::key)) works anywhere a prebuilt voice name does, e.g.
//! [`InteractionBuilder::with_voice`](crate::InteractionBuilder::with_voice).
//!
//! Verified live 2026-09-24: list (all filters), get, prompted create, and
//! delete. Prebuilt IDs are lowercase (`kore`, `ar-001-advisor-2`). Errors on
//! this resource use the standard Google envelope
//! (`{"error": {"code": 400, "status": "INVALID_ARGUMENT", ...}}`).

use crate::client::Client;
use crate::errors::GenaiError;
use crate::response::UsageMetadata;
use crate::serde_util::{ResourceName, deserialize_lenient_timestamp};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

struct ForVoice;
impl ResourceName for ForVoice {
    const NAME: &'static str = "Voice";
}

wire_enum! {
    /// How a voice was made (wire field `type`).
    pub enum VoiceType {
        /// A catalog voice returned by the list endpoint.
        Prebuilt = "prebuilt",
        /// Designed from a natural-language prompt.
        Prompted = "prompted",
        /// Replicated from sample audio.
        Replicated = "replicated",
    }
    unknown(voice_type, unknown_voice_type)
}

wire_enum! {
    /// Perceived pitch of a voice.
    pub enum VoicePitch {
        /// Low pitch.
        Low = "low",
        /// Medium pitch.
        Medium = "medium",
        /// High pitch.
        High = "high",
    }
    unknown(pitch_type, unknown_pitch_type)
}

/// Audio payload used to create a voice.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct VoiceAudio {
    /// Base64-encoded audio bytes.
    pub data: String,
    /// MIME type, e.g. `audio/wav`.
    pub mime_type: String,
}

impl VoiceAudio {
    /// Creates an audio payload from base64 data and its MIME type.
    #[must_use]
    pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }
}

/// The prompt a prompted voice was designed from.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct PromptedVoice {
    /// Natural-language description of the voice.
    pub input: String,
}

impl PromptedVoice {
    /// Creates a voice-design prompt.
    #[must_use]
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
        }
    }
}

/// A voice, as returned by `/v1beta/voices`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct Voice {
    /// Voice ID: `voice_...` for stored custom voices, the speaker name for
    /// prebuilt ones. Unset for voices created with `store: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// How the voice was made.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub voice_type: Option<VoiceType>,
    /// Display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Summary of timbre, personality and tone.
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
    /// Perceived gender presentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    /// Perceived pitch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch: Option<VoicePitch>,
    /// Model that designed or replicated a custom voice
    /// (e.g. `models/gemini-3.8-flash-tts`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The design prompt of a prompted voice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompted: Option<PromptedVoice>,
    /// Client-managed key (`voicekey_...`), returned only for a replicated
    /// voice created with `store: false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// When a custom voice expires (one year after creation, observed).
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_timestamp::<_, ForVoice>"
    )]
    pub expire_time: Option<DateTime<Utc>>,
    /// Sample audio for the voice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_audio: Option<VoiceAudio>,
    /// Token usage of the create call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
    /// Unmodeled fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing voices.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct VoiceListResponse {
    /// The voices in this page.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub voices: Vec<Voice>,
    /// Token for the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// The voice definition inside a [`CreateVoiceRequest`].
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
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
    /// Discovery metadata (`description`, `language_code`, `gender`, ...)
    /// and anything else not modeled here. Not persisted when `store` is
    /// false.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Source and consent recordings for a replicated voice.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReplicatedVoice {
    /// Audio of the voice to replicate.
    pub source_audio: VoiceAudio,
    /// The speaker's recorded consent.
    pub consent_audio: VoiceAudio,
}

/// Request body for [`Client::create_voice`].
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

    /// Sets whether Google stores the voice.
    #[must_use]
    pub const fn with_store(mut self, store: bool) -> Self {
        self.store = Some(store);
        self
    }
}

/// Filters and paging for [`Client::list_voices`]. All filters are optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListVoicesParams {
    /// Maximum voices per page.
    pub page_size: Option<u32>,
    /// Token from a previous page.
    pub page_token: Option<String>,
    /// Free-text search over the catalog.
    pub search: Option<String>,
    /// Only voices of this type.
    pub voice_type: Option<VoiceType>,
    /// Only voices with this gender, e.g. `female`.
    pub gender: Option<String>,
    /// Only voices for this language, e.g. `en-US`.
    pub language_code: Option<String>,
    /// Only voices for this region, e.g. `GB`.
    pub region_code: Option<String>,
    /// Only voices with this accent.
    pub accent: Option<String>,
    /// Only voices with this persona.
    pub persona: Option<String>,
    /// Only voices for this usage context.
    pub context: Option<String>,
    /// Only voices with this pitch.
    pub pitch: Option<VoicePitch>,
}

impl ListVoicesParams {
    /// No filters, server-default page size.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the page size.
    #[must_use]
    pub const fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Sets the page token.
    #[must_use]
    pub fn with_page_token(mut self, token: impl Into<String>) -> Self {
        self.page_token = Some(token.into());
        self
    }

    /// Sets the free-text search.
    #[must_use]
    pub fn with_search(mut self, search: impl Into<String>) -> Self {
        self.search = Some(search.into());
        self
    }

    /// Filters by voice type.
    #[must_use]
    pub fn with_voice_type(mut self, voice_type: VoiceType) -> Self {
        self.voice_type = Some(voice_type);
        self
    }

    /// Filters by language.
    #[must_use]
    pub fn with_language_code(mut self, language_code: impl Into<String>) -> Self {
        self.language_code = Some(language_code.into());
        self
    }

    /// Filters by gender.
    #[must_use]
    pub fn with_gender(mut self, gender: impl Into<String>) -> Self {
        self.gender = Some(gender.into());
        self
    }

    /// Filters by pitch.
    #[must_use]
    pub fn with_pitch(mut self, pitch: VoicePitch) -> Self {
        self.pitch = Some(pitch);
        self
    }

    /// The non-paging filters as query pairs, in wire names.
    pub(crate) fn filters(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let strings = [
            ("search", &self.search),
            ("gender", &self.gender),
            ("language_code", &self.language_code),
            ("region_code", &self.region_code),
            ("accent", &self.accent),
            ("persona", &self.persona),
            ("context", &self.context),
        ];
        for (key, value) in strings {
            if let Some(v) = value {
                out.push((key, v.clone()));
            }
        }
        if let Some(t) = &self.voice_type {
            out.push(("type", t.to_string()));
        }
        if let Some(p) = &self.pitch {
            out.push(("pitch", p.to_string()));
        }
        out
    }
}

impl Client {
    /// Lists voices: the prebuilt catalog plus your stored custom voices.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or a non-success status.
    pub async fn list_voices(
        &self,
        params: &ListVoicesParams,
    ) -> Result<VoiceListResponse, GenaiError> {
        crate::http::voices::list_voices(&self.http, params).await
    }

    /// Retrieves a voice by bare ID (`voice_...`, or a prebuilt name).
    ///
    /// # Errors
    ///
    /// Returns an error if the voice doesn't exist or the request fails.
    pub async fn get_voice(&self, voice_id: &str) -> Result<Voice, GenaiError> {
        crate::http::voices::get_voice(&self.http, voice_id).await
    }

    /// Creates a custom voice.
    ///
    /// ```no_run
    /// # async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// use genai_rs::CreateVoiceRequest;
    ///
    /// let voice = client
    ///     .create_voice(&CreateVoiceRequest::prompted("A warm, gravelly storyteller."))
    ///     .await?;
    /// println!("{:?}", voice.id);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the request is rejected (e.g. a prompted voice
    /// with `store: false`, or the stored-voice quota is exhausted).
    pub async fn create_voice(&self, request: &CreateVoiceRequest) -> Result<Voice, GenaiError> {
        crate::http::voices::create_voice(&self.http, request).await
    }

    /// Deletes a stored custom voice.
    ///
    /// # Errors
    ///
    /// Returns an error if the voice doesn't exist or the request fails.
    pub async fn delete_voice(&self, voice_id: &str) -> Result<(), GenaiError> {
        crate::http::voices::delete_voice(&self.http, voice_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Captured from a live `GET /v1beta/voices` (2026-09-24).
    #[test]
    fn prebuilt_voice_deserializes_the_live_shape() {
        let wire = json!({
            "id": "achernar",
            "type": "prebuilt",
            "display_name": "Achernar",
            "language_code": "en-US",
            "region_code": "US",
            "accent": "General American",
            "persona": "Storyteller & Narrator",
            "context": "Content & Media",
            "gender": "female",
            "pitch": "high",
            "description": "Soft, calm, and soothing voice with a higher pitch."
        });
        let voice: Voice = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(voice.voice_type, Some(VoiceType::Prebuilt));
        assert_eq!(voice.pitch, Some(VoicePitch::High));
        assert!(voice.extra.is_empty());
        assert_eq!(serde_json::to_value(&voice).unwrap(), wire);
    }

    /// Captured from a live prompted `POST /v1beta/voices` (2026-09-24),
    /// usage trimmed.
    #[test]
    fn created_voice_deserializes_the_live_shape() {
        let voice: Voice = serde_json::from_value(json!({
            "id": "voice_3d8wi4xx4yxg",
            "model": format!("models/{}", crate::DEFAULT_TTS_MODEL),
            "type": "prompted",
            "expire_time": "2027-09-24T01:44:20.854771486Z",
            "display_name": "sweep-robot",
            "prompted": {"input": "A calm, low-pitched robot narrator."},
            "usage": {"total_tokens": 1334},
            "sample_audio": {"data": "UklGRg==", "mime_type": "audio/wav"},
            "future_field": 1
        }))
        .unwrap();
        assert_eq!(voice.voice_type, Some(VoiceType::Prompted));
        assert!(voice.expire_time.is_some());
        assert_eq!(
            voice.prompted.unwrap().input,
            "A calm, low-pitched robot narrator."
        );
        assert_eq!(voice.usage.unwrap().total_tokens, Some(1334));
        assert_eq!(voice.sample_audio.unwrap().mime_type, "audio/wav");
        assert_eq!(voice.extra["future_field"], 1);
    }

    #[test]
    fn create_requests_serialize_the_binding_shape() {
        assert_eq!(
            serde_json::to_value(CreateVoiceRequest::prompted("deep voice").with_display_name("d"))
                .unwrap(),
            json!({
                "voice": {"type": "prompted", "prompted": {"input": "deep voice"}, "display_name": "d"},
                "store": true
            })
        );
        assert_eq!(
            serde_json::to_value(CreateVoiceRequest::replicated(
                VoiceAudio::new("c3Jj", "audio/wav"),
                VoiceAudio::new("Y25z", "audio/mpeg"),
            ))
            .unwrap(),
            json!({"voice": {
                "type": "replicated",
                "replicated": {
                    "source_audio": {"data": "c3Jj", "mime_type": "audio/wav"},
                    "consent_audio": {"data": "Y25z", "mime_type": "audio/mpeg"}
                }
            }})
        );
    }

    #[test]
    fn list_params_render_wire_names() {
        let params = ListVoicesParams::new()
            .with_page_size(3)
            .with_voice_type(VoiceType::Prebuilt)
            .with_language_code("en-US")
            .with_pitch(VoicePitch::Low);
        assert_eq!(
            params.filters(),
            vec![
                ("language_code", "en-US".to_string()),
                ("type", "prebuilt".to_string()),
                ("pitch", "low".to_string()),
            ]
        );
    }

    #[test]
    fn empty_list_response_deserializes() {
        let list: VoiceListResponse = serde_json::from_value(json!({})).unwrap();
        assert!(list.voices.is_empty());
    }
}
