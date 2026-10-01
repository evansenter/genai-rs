//! Voices resource (`/v1beta/voices`).
//!
//! Lists the prebuilt TTS voice catalog (with descriptions and filters), and
//! creates custom voices — designed from a text prompt, or replicated from
//! sample audio. A voice's [`id`](Voice::id) (or a replicated voice's
//! [`key`](Voice::key)) works anywhere a prebuilt voice name does, e.g.
//! [`InteractionBuilder::with_voice`](crate::InteractionBuilder::with_voice).
//!
//! Manage them through the [`Voices`] handle from [`Client::voices`]:
//! [`create`](Voices::create), [`get`](Voices::get), [`list`](Voices::list)
//! and [`delete`](Voices::delete).
//!
//! Verified live 2026-09-24: list (all filters), get, prompted create, and
//! delete. Prebuilt IDs are lowercase (`kore`, `ar-001-advisor-2`). Errors on
//! this resource use the standard Google envelope
//! (`{"error": {"code": 400, "status": "INVALID_ARGUMENT", ...}}`).
//!
//! # IDs
//!
//! Methods take the bare ID ([`Voice::id`]), not a `voices/...` resource
//! name: the ID is percent-encoded into a single path segment. Stored custom
//! voices have `voice_...` IDs, matched case-sensitively, and
//! [`get`](Voices::get) finds only those: a prebuilt catalog ID (`achernar`,
//! `kore`), an upper-cased custom ID and a percent-encoded `voices/<id>`
//! resource name all get a 404 (verified live 2026-09-27), though a prebuilt
//! ID works as a voice name. An empty or dot-segment ID fails locally with
//! [`GenaiError::InvalidInput`] before any request.

use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
use crate::response::UsageMetadata;
use crate::serde_util::{ResourceName, deserialize_lenient_timestamp};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
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
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(VoiceListResponse, voices: Voice);

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

/// Request body for [`Voices::create`].
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

/// The catalog filters of a [`ListVoices`]. Every page of a listing carries
/// the same filters: the API rejects a page token sent with other filters.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct VoiceFilters {
    pub(crate) search: Option<String>,
    pub(crate) voice_type: Option<VoiceType>,
    pub(crate) gender: Option<String>,
    pub(crate) language_code: Option<String>,
    pub(crate) region_code: Option<String>,
    pub(crate) accent: Option<String>,
    pub(crate) persona: Option<String>,
    pub(crate) context: Option<String>,
    pub(crate) pitch: Option<VoicePitch>,
}

impl VoiceFilters {
    /// The set filters as query pairs, in wire names (`voice_type` is sent
    /// as `type`).
    pub(crate) fn query_pairs(&self) -> Vec<(&'static str, String)> {
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
    /// The `/v1beta/voices` resource: list the voice catalog, and create,
    /// get and delete custom voices.
    ///
    /// The handle borrows the client and is `Copy`; see
    /// [IDs](crate::voices#ids) for what the methods take.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let voice = client.voices().get("voice_3d8wi4xx4yxg").await?;
    /// # let _ = voice;
    /// # Ok(())
    /// # }
    /// ```
    pub fn voices(&self) -> Voices<'_> {
        Voices { client: self }
    }
}

/// The `/v1beta/voices` resource, from [`Client::voices`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle: `client.voices().get(id)` can be stored or
/// joined with others. See [IDs](crate::voices#ids).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct Voices<'a> {
    client: &'a Client,
}

impl<'a> Voices<'a> {
    /// Creates a custom voice.
    ///
    /// # Errors
    ///
    /// Returns an error if the request is rejected (e.g. a prompted voice
    /// with `store: false`, or the stored-voice quota is exhausted).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// use genai_rs::CreateVoiceRequest;
    ///
    /// let voice = client
    ///     .voices()
    ///     .create(&CreateVoiceRequest::prompted("A warm, gravelly storyteller."))
    ///     .await?;
    /// println!("{:?}", voice.id);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(self, request: &CreateVoiceRequest) -> Result<Voice, GenaiError> {
        crate::http::voices::create_voice(&self.client.http, request).await
    }

    /// Retrieves a stored custom voice by its bare ID (`voice_...`).
    /// Prebuilt catalog IDs are not found here; see
    /// [IDs](crate::voices#ids).
    ///
    /// # Errors
    ///
    /// Returns an error if the voice doesn't exist (404) or the request
    /// fails.
    pub async fn get(self, voice_id: &str) -> Result<Voice, GenaiError> {
        crate::http::voices::get_voice(&self.client.http, voice_id).await
    }

    /// Lists voices, the prebuilt catalog plus your stored custom voices:
    /// configure the returned [`ListVoices`] (filters and paging), then call
    /// [`send`](ListVoices::send) for one page, or
    /// [`pages`](ListVoices::pages) / [`items`](ListVoices::items) to stream
    /// them all.
    pub fn list(self) -> ListVoices<'a> {
        ListVoices {
            client: self.client,
            filters: VoiceFilters::default(),
            page_size: None,
            page_token: None,
        }
    }

    /// Deletes a stored custom voice.
    ///
    /// # Errors
    ///
    /// Returns an error if the voice doesn't exist or the request fails.
    pub async fn delete(self, voice_id: &str) -> Result<(), GenaiError> {
        crate::http::voices::delete_voice(&self.client.http, voice_id).await
    }
}

/// A `GET /v1beta/voices` request, from [`Voices::list`]. All filters are
/// optional; each one set narrows the list.
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `next_page_token` to the end of the list. The filters and page size are
/// sent with every page.
///
/// The API ties a page token to the filters of the request that returned
/// it: the same token with other filters, or with none, is a 400 (verified
/// live 2026-09-27). The streams take care of this; when resuming with
/// [`with_page_token`](Self::with_page_token), set the same filters again.
///
/// # Example
///
/// ```no_run
/// use futures_util::{StreamExt, TryStreamExt};
/// use genai_rs::{Voice, VoiceType};
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // One page of the British English catalog
/// let page = client
///     .voices()
///     .list()
///     .with_voice_type(VoiceType::Prebuilt)
///     .with_language_code("en-GB")
///     .with_page_size(10)
///     .send()
///     .await?;
/// for voice in &page.voices {
///     println!("{:?}: {:?}", voice.id, voice.description);
/// }
///
/// // The first 20 matches of a search, across pages
/// let narrators: Vec<Voice> = client
///     .voices()
///     .list()
///     .with_search("narrator")
///     .items()
///     .take(20)
///     .try_collect()
///     .await?;
/// # let _ = narrators;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListVoices<'a> {
    client: &'a Client,
    filters: VoiceFilters,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListVoices<'a> {
    /// Sets the maximum number of voices per page (the API default is 50).
    /// Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token`. Set the filters that page was listed with, too.
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Free-text search over the catalog, e.g. `narrator`.
    pub fn with_search(mut self, search: impl Into<String>) -> Self {
        self.filters.search = Some(search.into());
        self
    }

    /// Only voices of this type (wire parameter `type`).
    pub fn with_voice_type(mut self, voice_type: VoiceType) -> Self {
        self.filters.voice_type = Some(voice_type);
        self
    }

    /// Only voices with this gender, e.g. `female`.
    pub fn with_gender(mut self, gender: impl Into<String>) -> Self {
        self.filters.gender = Some(gender.into());
        self
    }

    /// Only voices for this language, a BCP-47 tag such as `en-US`.
    pub fn with_language_code(mut self, language_code: impl Into<String>) -> Self {
        self.filters.language_code = Some(language_code.into());
        self
    }

    /// Only voices for this region, e.g. `GB`.
    pub fn with_region_code(mut self, region_code: impl Into<String>) -> Self {
        self.filters.region_code = Some(region_code.into());
        self
    }

    /// Only voices with this accent, e.g. `General American`.
    pub fn with_accent(mut self, accent: impl Into<String>) -> Self {
        self.filters.accent = Some(accent.into());
        self
    }

    /// Only voices with this persona, e.g. `Storyteller & Narrator`.
    pub fn with_persona(mut self, persona: impl Into<String>) -> Self {
        self.filters.persona = Some(persona.into());
        self
    }

    /// Only voices for this usage context, e.g. `Content & Media`.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.filters.context = Some(context.into());
        self
    }

    /// Only voices with this pitch.
    pub fn with_pitch(mut self, pitch: VoicePitch) -> Self {
        self.filters.pitch = Some(pitch);
        self
    }

    /// Sends the request and returns one page.
    ///
    /// A list that matches nothing comes back as an empty page with no
    /// `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn send(self) -> Result<VoiceListResponse, GenaiError> {
        crate::http::voices::list_voices(
            &self.client.http,
            &self.filters,
            self.page_size,
            self.page_token.as_deref(),
        )
        .await
    }

    /// Streams every page, starting at [`with_page_token`](Self::with_page_token)
    /// or the first page.
    ///
    /// Nothing is sent until the stream is polled. It ends after a page
    /// without a `next_page_token`; an error is yielded once and ends it. A
    /// page whose token was already requested (the starting token included)
    /// is yielded, then [`GenaiError::MalformedResponse`]. The stream owns a
    /// clone of the client, so it can be stored or spawned.
    #[must_use = "streams do nothing unless polled"]
    pub fn pages(self) -> BoxStream<'static, Result<VoiceListResponse, GenaiError>> {
        let Self {
            client,
            filters,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("voices", page_token, move |token| {
            let (client, filters) = (client.clone(), filters.clone());
            async move {
                crate::http::voices::list_voices(
                    &client.http,
                    &filters,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every voice across pages, in server order. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<Voice, GenaiError>> {
        paging::items(self.pages())
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
    fn voice_list_without_setters_sends_no_query() {
        let client = Client::new("k".to_string());
        let list = client.voices().list();
        assert_eq!(list.filters, VoiceFilters::default());
        assert!(list.filters.query_pairs().is_empty());
        assert_eq!(list.page_size, None);
        assert_eq!(list.page_token, None);
    }

    #[test]
    fn voice_list_setters_render_wire_names() {
        let client = Client::new("k".to_string());
        let list = client
            .voices()
            .list()
            .with_page_size(3)
            .with_page_token("t1")
            .with_search("warm")
            .with_voice_type(VoiceType::Prompted)
            .with_gender("female")
            .with_language_code("en-US")
            .with_region_code("US")
            .with_accent("General American")
            .with_persona("Storyteller & Narrator")
            .with_context("Content & Media")
            .with_pitch(VoicePitch::Low)
            // `with_*` replaces.
            .with_page_token("t2")
            .with_voice_type(VoiceType::Prebuilt);
        assert_eq!(list.page_size, Some(3));
        assert_eq!(list.page_token.as_deref(), Some("t2"));
        // Paging stays out of the filters; `voice_type` is sent as `type`.
        assert_eq!(
            list.filters.query_pairs(),
            vec![
                ("search", "warm".to_string()),
                ("gender", "female".to_string()),
                ("language_code", "en-US".to_string()),
                ("region_code", "US".to_string()),
                ("accent", "General American".to_string()),
                ("persona", "Storyteller & Narrator".to_string()),
                ("context", "Content & Media".to_string()),
                ("type", "prebuilt".to_string()),
                ("pitch", "low".to_string()),
            ]
        );
    }

    #[test]
    fn voices_handle_and_list_debug_redact_the_api_key() {
        let client = Client::new("secret-api-key".to_string());
        for debug in [
            format!("{:?}", client.voices()),
            format!("{:?}", client.voices().list().with_search("warm")),
        ] {
            assert!(!debug.contains("secret-api-key"), "{debug}");
            assert!(debug.contains("[REDACTED]"), "{debug}");
        }
    }

    #[test]
    fn empty_list_response_deserializes() {
        let list: VoiceListResponse = serde_json::from_value(json!({})).unwrap();
        assert!(list.voices.is_empty());
    }
}
