//! Request types for creating interactions.

mod agent_config;
mod generation_config;

pub use agent_config::{
    AgentConfig, AntigravityConfig, DeepResearchConfig, DynamicConfig, ThinkingSummaries,
    Visualization,
};
pub use generation_config::{
    GenerationConfig, ImageAspectRatio, ImageConfig, ImageSize, SpeechConfig, ThinkingLevel,
    TranscriptionConfig, TranscriptionMode, VideoConfig, VideoTask,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::content::Content;
use crate::environments::EnvironmentSpec;
use crate::response_format::ResponseFormatSpec;
use crate::safety::SafetySetting;
use crate::steps::Step;
use crate::tools::Tool;
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

#[cfg(test)]
mod request_tests;

#[cfg(test)]
mod tests;
