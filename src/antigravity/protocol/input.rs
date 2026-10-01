use super::HookDecision;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// =============================================================================
// Client -> harness events (InputEvent oneof)
// =============================================================================

/// `InputEvent` — the client-to-harness message envelope (oneof `event`).
///
/// Serializes to a single-key proto-JSON object, e.g.
/// `{"userInput": {"parts": [{"text": "hello"}]}}` or
/// `{"toolResponse": {...}}`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum InputEvent {
    /// A user message (text, media, slash commands); starts a new turn.
    ///
    /// Harness 0.1.18 dropped the plain-string `user_input` arm and renamed
    /// the multi-part `complex_user_input` arm to `user_input`, so every
    /// user message is now a [`UserInput`]. The old string shape is a
    /// *parse error* on the harness side, reported only on its stderr —
    /// the turn silently never starts. Outbound-only, so no alias is kept:
    /// this is the one wire shape 0.1.18 accepts.
    UserInput(UserInput),
    /// Approve/reject a harness-side tool awaiting confirmation.
    ToolConfirmation(ToolConfirmation),
    /// Result of a client-executed custom tool call.
    ToolResponse(ToolResponse),
    /// Answers to a `questions_request`.
    QuestionResponse(UserQuestionsResponse),
    /// Cancel the current turn.
    HaltRequest(bool),
    /// Message injected by a client-side trigger.
    AutomatedTrigger(String),
    /// Reply to a `call_hook_request`.
    CallHookResponse(CallHookResponse),
    /// Ask the harness to run session-end hooks.
    SessionEndRequest(bool),
    /// An event variant this crate does not recognize (Evergreen).
    Unknown {
        /// The unrecognized oneof field name.
        event_type: String,
        /// The raw JSON payload, preserved for roundtrip.
        data: Value,
    },
}

impl InputEvent {
    /// A plain-text user message: one text part.
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::UserInput(UserInput::text(text))
    }

    /// Check if this is an unknown event.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized oneof field name if this is an unknown event.
    #[must_use]
    pub fn unknown_event_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { event_type, .. } => Some(event_type),
            _ => None,
        }
    }

    /// Returns the preserved raw JSON if this is an unknown event.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }

    fn oneof_key(&self) -> &str {
        match self {
            Self::UserInput(_) => "userInput",
            Self::ToolConfirmation(_) => "toolConfirmation",
            Self::ToolResponse(_) => "toolResponse",
            Self::QuestionResponse(_) => "questionResponse",
            Self::HaltRequest(_) => "haltRequest",
            Self::AutomatedTrigger(_) => "automatedTrigger",
            Self::CallHookResponse(_) => "callHookResponse",
            Self::SessionEndRequest(_) => "sessionEndRequest",
            Self::Unknown { event_type, .. } => event_type,
        }
    }

    fn oneof_value(&self) -> Result<Value, serde_json::Error> {
        match self {
            Self::UserInput(v) => serde_json::to_value(v),
            Self::AutomatedTrigger(s) => Ok(Value::String(s.clone())),
            Self::ToolConfirmation(v) => serde_json::to_value(v),
            Self::ToolResponse(v) => serde_json::to_value(v),
            Self::QuestionResponse(v) => serde_json::to_value(v),
            Self::HaltRequest(b) | Self::SessionEndRequest(b) => Ok(Value::Bool(*b)),
            Self::CallHookResponse(v) => serde_json::to_value(v),
            Self::Unknown { data, .. } => Ok(data.clone()),
        }
    }
}

impl Serialize for InputEvent {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let value = self.oneof_value().map_err(serde::ser::Error::custom)?;
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.oneof_key(), &value)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for InputEvent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let map = Map::deserialize(deserializer)?;
        let (key, value) = map
            .into_iter()
            .next()
            .ok_or_else(|| D::Error::custom("InputEvent must have exactly one field set"))?;
        let event = match key.as_str() {
            "userInput" => {
                Self::UserInput(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "toolConfirmation" => {
                Self::ToolConfirmation(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "toolResponse" => {
                Self::ToolResponse(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "questionResponse" => {
                Self::QuestionResponse(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "haltRequest" => Self::HaltRequest(value.as_bool().unwrap_or_default()),
            "automatedTrigger" => Self::AutomatedTrigger(
                value
                    .as_str()
                    .ok_or_else(|| D::Error::custom("automatedTrigger must be a string"))?
                    .to_string(),
            ),
            "callHookResponse" => {
                Self::CallHookResponse(serde_json::from_value(value).map_err(D::Error::custom)?)
            }
            "sessionEndRequest" => Self::SessionEndRequest(value.as_bool().unwrap_or_default()),
            _ => Self::Unknown {
                event_type: key,
                data: value,
            },
        };
        Ok(event)
    }
}

/// `UserInput` — multi-part user content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserInput {
    /// The content parts, in order.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parts: Vec<UserInputPart>,
}

impl UserInput {
    /// User content consisting of a single text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![UserInputPart::text(text)],
        }
    }
}

/// `UserInput.Part` (oneof `part`): text, media, or a slash command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserInputPart {
    /// Plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Inline media (image, document, audio, video).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    /// A named slash command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slash_command: Option<SlashCommand>,
}

impl UserInputPart {
    /// A text part.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Self::default()
        }
    }
}

/// `Media` — inline binary content (proto-JSON encodes `data` as base64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// MIME type of the data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Base64-encoded content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// `UserInput.SlashCommand`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    /// Command name (without the slash).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `ToolConfirmation` — approve or reject a pending harness-side tool step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfirmation {
    /// Trajectory of the waiting step.
    pub trajectory_id: String,
    /// Index of the waiting step.
    pub step_index: u32,
    /// `true` to run the tool, `false` to reject it.
    pub accepted: bool,
}

/// `ToolResponse` — the result of a client-executed custom tool call.
///
/// Sent by the client, but the harness also echoes it back inside a
/// [`ActionCustomTool`](super::ActionCustomTool) step, so it deserializes leniently: proto3 JSON
/// omits an empty `id`, which must not fail the whole step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolResponse {
    /// Correlates with [`ToolCall::id`](super::ToolCall::id).
    pub id: String,
    /// The result, serialized as a JSON string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json: Option<String>,
    /// Media attachments accompanying the result.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supplemental_media: Vec<Media>,
    /// The result as a structured value (the harness's `genai.Struct`
    /// encoding). Never written by this crate, which sends
    /// `response_json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    /// A failed call's error, as an alternative to an `{"error": ...}`
    /// `response_json`. Never written by this crate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// `UserQuestionsResponse` — answers to a `questions_request`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionsResponse {
    /// Trajectory of the asking step.
    pub trajectory_id: String,
    /// Index of the asking step.
    pub step_index: u32,
    /// Set to cancel the questions instead of answering.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
    /// The answers (oneof with `cancelled`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<QuestionsResponse>,
}

/// `UserQuestionsResponse.QuestionsResponse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuestionsResponse {
    /// One answer per question, in order.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub answers: Vec<UserQuestionAnswer>,
}

/// `UserQuestionAnswer` (oneof `answer`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionAnswer {
    /// The question was left unanswered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unanswered: Option<bool>,
    /// A multiple-choice answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple_choice_answer: Option<MultipleChoiceAnswer>,
}

impl UserQuestionAnswer {
    /// An explicit "unanswered" answer.
    #[must_use]
    pub fn unanswered() -> Self {
        Self {
            unanswered: Some(true),
            multiple_choice_answer: None,
        }
    }
}

/// `MultipleChoiceAnswer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MultipleChoiceAnswer {
    /// Zero-based indices of the selected choices.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub selected_choice_indices: Vec<i32>,
    /// Optional freeform text response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeform_response: Option<String>,
}

/// `CallHookResponse` — the client's reply to a hook callback.
///
/// The result fields form a oneof (`result`): set exactly one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CallHookResponse {
    /// Correlates with [`CallHookRequest::request_id`](super::CallHookRequest::request_id).
    pub request_id: String,
    /// Verdict for a pre-turn hook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_turn_result: Option<HookVerdict>,
    /// Verdict for a pre-tool hook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_tool_result: Option<HookVerdict>,
    /// Acknowledgement for observe-only hooks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_result: Option<EmptyResult>,
    /// Hook execution failed on the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// `PreToolResult` / `PreTurnResult` — an allow/deny verdict with a reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookVerdict {
    /// The decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
    /// Human-readable reason (surfaced to the model on deny).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `EmptyResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmptyResult {}
