//! Decisions resource (`/v1beta/decisions`).
//!
//! Evaluates structured questions against one shared input and returns a
//! calibrated answer to each: a yes/no probability for a
//! [`Question::Noul`], a pick among options for a [`Question::Choice`], and
//! an expected level on a rubric for a [`Question::Score`]. The input is
//! read once for all the questions, and the model writes no text: each
//! answer is a set of probabilities with a confidence.
//!
//! Call it through the [`Decisions`] handle from [`Client::decisions`]. It
//! runs on [`DEFAULT_DECISION_MODEL`](crate::DEFAULT_DECISION_MODEL), the
//! one model that serves it; any other model is a `404 Requested model not
//! found`. The input is a string or content blocks (text, image, audio,
//! document or video), and each question's instructions are text.
//!
//! Errors use the Interactions envelope (`{"error": {"message": ...,
//! "code": "invalid_request"}}`): an unknown question type, a choice with
//! fewer than two options and a request with no questions are 400s that
//! say so.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::client::Client;
use crate::content::Content;
use crate::errors::GenaiError;
use crate::response::UsageMetadata;
use crate::wire_enum::wire_enum;
use crate::wire_fields::{Fallback, ModeledMap, WireFields};

// =============================================================================
// Request
// =============================================================================

/// The input every question of a [`CreateDecisionRequest`] is evaluated
/// against: text, or content blocks.
///
/// Converts from a string, one [`Content`] or a `Vec<Content>`. The endpoint
/// takes text, image, audio, document and video blocks.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum DecisionInput {
    /// Plain text, sent as a string.
    Text(String),
    /// Content blocks, sent as a list.
    Content(Vec<Content>),
}

impl From<&str> for DecisionInput {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<String> for DecisionInput {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Content> for DecisionInput {
    fn from(content: Content) -> Self {
        Self::Content(vec![content])
    }
}

impl From<Vec<Content>> for DecisionInput {
    fn from(contents: Vec<Content>) -> Self {
        Self::Content(contents)
    }
}

/// What the `true` and `false` outcomes of a [`Question::Noul`] mean. Both
/// descriptions are optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct NoulCriteria {
    /// What a `true` outcome means.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub true_description: Option<String>,
    /// What a `false` outcome means.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub false_description: Option<String>,
}

impl NoulCriteria {
    /// Criteria with neither description set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets what a `true` outcome means.
    #[must_use]
    pub fn with_true_description(mut self, description: impl Into<String>) -> Self {
        self.true_description = Some(description.into());
        self
    }

    /// Sets what a `false` outcome means.
    #[must_use]
    pub fn with_false_description(mut self, description: impl Into<String>) -> Self {
        self.false_description = Some(description.into());
        self
    }
}

/// One question of a [`CreateDecisionRequest`] (wire: tagged by `type`).
///
/// Build one with [`noul`](Self::noul), [`choice`](Self::choice) or
/// [`score`](Self::score). Each takes its instructions, the guidelines or
/// rubric the input is judged by.
///
/// ```
/// use genai_rs::{NoulCriteria, Question};
///
/// let refund = Question::noul_with_criteria(
///     "Does the customer ask for a refund?",
///     NoulCriteria::new().with_true_description("A refund is requested"),
/// );
/// let topic = Question::choice(
///     "What is the complaint about?",
///     [("food", "The food itself"), ("service", "The staff or the wait")],
/// );
/// let stars = Question::score("How many stars?", ["zero", "one", "two", "three"]);
/// assert_eq!(
///     [refund.question_type(), topic.question_type(), stars.question_type()],
///     ["noul", "choice", "score"]
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Question {
    /// A binary (true or false) question, answered with
    /// [`Answer::Noul`]'s probability of `true`.
    Noul {
        /// The guidelines the input is judged by.
        instructions: String,
        /// What each outcome means.
        criteria: Option<NoulCriteria>,
    },
    /// A single-select question over mutually exclusive options, answered
    /// with [`Answer::Choice`].
    Choice {
        /// The guidelines the input is judged by.
        instructions: String,
        /// Option ID to its description (which may be empty). The API takes
        /// between 2 and 255 options.
        criteria: BTreeMap<String, String>,
    },
    /// A rubric over discrete levels, answered with [`Answer::Score`]. Level
    /// `i` is `criteria[i]`.
    Score {
        /// The guidelines the input is judged by.
        instructions: String,
        /// Each level's description, from level 0 up. The API takes between
        /// 2 and 255 non-empty levels.
        criteria: Vec<String>,
    },
    /// A question type this crate does not model, sent as `data` exactly
    /// (Evergreen pattern).
    Unknown {
        /// The question's `type`.
        question_type: String,
        /// The whole question object, sent as is.
        data: Value,
    },
}

impl Question {
    /// A binary question.
    #[must_use]
    pub fn noul(instructions: impl Into<String>) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: None,
        }
    }

    /// A binary question with descriptions of its outcomes.
    #[must_use]
    pub fn noul_with_criteria(instructions: impl Into<String>, criteria: NoulCriteria) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: Some(criteria),
        }
    }

    /// A single-select question over `options`, each an option ID and its
    /// description (empty when the ID says enough).
    #[must_use]
    pub fn choice(
        instructions: impl Into<String>,
        options: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self::Choice {
            instructions: instructions.into(),
            criteria: options
                .into_iter()
                .map(|(id, description)| (id.into(), description.into()))
                .collect(),
        }
    }

    /// A rubric question over `levels`, described from level 0 up.
    #[must_use]
    pub fn score(
        instructions: impl Into<String>,
        levels: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::Score {
            instructions: instructions.into(),
            criteria: levels.into_iter().map(Into::into).collect(),
        }
    }

    /// The wire `type`: `"noul"`, `"choice"`, `"score"`, or an unknown
    /// question's own.
    #[must_use]
    pub fn question_type(&self) -> &str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
            Self::Unknown { question_type, .. } => question_type,
        }
    }

    /// Returns true if this is an unknown question type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized question type, if this is unknown.
    #[must_use]
    pub fn unknown_question_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { question_type, .. } => Some(question_type),
            _ => None,
        }
    }

    /// Returns the question object sent as is, if this is unknown.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

impl Serialize for Question {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let none = Map::new();
        match self {
            Self::Noul {
                instructions,
                criteria,
            } => {
                let mut map = ModeledMap::tagged(serializer, "noul")?;
                map.field("instructions", instructions)?;
                map.opt("criteria", criteria.as_ref())?;
                map.end(&none)
            }
            Self::Choice {
                instructions,
                criteria,
            } => {
                let mut map = ModeledMap::tagged(serializer, "choice")?;
                map.field("instructions", instructions)?;
                map.field("criteria", criteria)?;
                map.end(&none)
            }
            Self::Score {
                instructions,
                criteria,
            } => {
                let mut map = ModeledMap::tagged(serializer, "score")?;
                map.field("instructions", instructions)?;
                map.field("criteria", criteria)?;
                map.end(&none)
            }
            Self::Unknown {
                question_type,
                data,
            } => crate::serde_util::serialize_unknown_data(question_type, data, serializer),
        }
    }
}

/// Request body for [`Decisions::create`]: the model, the shared input and
/// the questions, each under an ID its answer comes back keyed by.
///
/// ```
/// use genai_rs::{CreateDecisionRequest, DEFAULT_DECISION_MODEL, Question};
///
/// let request = CreateDecisionRequest::new(DEFAULT_DECISION_MODEL, "The soup was cold.")
///     .add_question("positive", Question::noul("Is the review positive?"));
/// assert_eq!(request.questions.len(), 1);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CreateDecisionRequest {
    /// The model, [`DEFAULT_DECISION_MODEL`](crate::DEFAULT_DECISION_MODEL).
    /// A `models/` prefix is accepted too.
    pub model: String,
    /// The input every question is evaluated against.
    pub input: DecisionInput,
    /// The questions, keyed by caller-chosen IDs. The API takes at least
    /// one.
    pub questions: BTreeMap<String, Question>,
}

impl CreateDecisionRequest {
    /// A request on `model` over `input`, with no questions yet: add each
    /// with [`add_question`](Self::add_question).
    #[must_use]
    pub fn new(model: impl Into<String>, input: impl Into<DecisionInput>) -> Self {
        Self {
            model: model.into(),
            input: input.into(),
            questions: BTreeMap::new(),
        }
    }

    /// Adds a question under `id`, the key its answer comes back under. A
    /// second question with the same ID replaces the first.
    #[must_use]
    pub fn add_question(mut self, id: impl Into<String>, question: Question) -> Self {
        self.questions.insert(id.into(), question);
        self
    }
}

// =============================================================================
// Response
// =============================================================================

/// One question's answer in a [`CreateDecisionResponse`] (wire: tagged by
/// `type`, the type of the question it answers).
///
/// Every probability and confidence is calibrated, in `[0, 1]`. A
/// `confidence` is the probability of the winning outcome. A known answer
/// keeps the fields the crate does not model in its `extra` map
/// ([`Answer::extra`]); one with a type the crate does not know, or with a
/// field it cannot read, is kept whole as [`Answer::Unknown`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Answer {
    /// The answer to a [`Question::Noul`].
    Noul {
        /// The probability that the answer is `true`.
        noul: f64,
        /// `max(noul, 1 - noul)`.
        confidence: f64,
        /// Unmodeled wire fields, kept for round trip.
        extra: Map<String, Value>,
    },
    /// The answer to a [`Question::Choice`].
    Choice {
        /// The option ID with the highest probability.
        choice: String,
        /// The probability of `choice`.
        confidence: f64,
        /// Each option ID's probability; they sum to 1.
        probabilities: BTreeMap<String, f64>,
        /// Unmodeled wire fields, kept for round trip.
        extra: Map<String, Value>,
    },
    /// The answer to a [`Question::Score`].
    Score {
        /// The expected level: each level times its probability, summed.
        score: f64,
        /// The highest probability in `probabilities`.
        confidence: f64,
        /// Each level's description, keyed by its index as a string
        /// (`"0"`, `"1"`, ...).
        legend: BTreeMap<String, String>,
        /// Each level's probability, keyed by its index as a string; they
        /// sum to 1.
        probabilities: BTreeMap<String, f64>,
        /// Unmodeled wire fields, kept for round trip.
        extra: Map<String, Value>,
    },
    /// Unknown variant for forward compatibility (Evergreen pattern).
    Unknown {
        /// The unrecognized answer type.
        answer_type: String,
        /// The raw JSON value, preserved for roundtrip.
        data: Value,
    },
}

impl Answer {
    /// The wire `type`: `"noul"`, `"choice"`, `"score"`, or an unknown
    /// answer's own.
    #[must_use]
    pub fn answer_type(&self) -> &str {
        match self {
            Self::Noul { .. } => "noul",
            Self::Choice { .. } => "choice",
            Self::Score { .. } => "score",
            Self::Unknown { answer_type, .. } => answer_type,
        }
    }

    /// The probability of the winning outcome. `None` for
    /// [`Answer::Unknown`].
    #[must_use]
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Noul { confidence, .. }
            | Self::Choice { confidence, .. }
            | Self::Score { confidence, .. } => Some(*confidence),
            Self::Unknown { .. } => None,
        }
    }

    /// The fields of this answer that the crate does not model, kept so they
    /// round-trip. `None` for [`Answer::Unknown`].
    #[must_use]
    pub fn extra(&self) -> Option<&Map<String, Value>> {
        match self {
            Self::Noul { extra, .. } | Self::Choice { extra, .. } | Self::Score { extra, .. } => {
                Some(extra)
            }
            Self::Unknown { .. } => None,
        }
    }

    /// Mutable access to [`extra`](Self::extra). An entry named after a
    /// modeled field, or `type`, is never written.
    pub fn extra_mut(&mut self) -> Option<&mut Map<String, Value>> {
        match self {
            Self::Noul { extra, .. } | Self::Choice { extra, .. } | Self::Score { extra, .. } => {
                Some(extra)
            }
            Self::Unknown { .. } => None,
        }
    }

    /// Returns true if this is an unknown answer.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// Returns the unrecognized answer type, if this is unknown.
    #[must_use]
    pub fn unknown_answer_type(&self) -> Option<&str> {
        match self {
            Self::Unknown { answer_type, .. } => Some(answer_type),
            _ => None,
        }
    }

    /// Returns the preserved JSON, if this is unknown.
    #[must_use]
    pub fn unknown_data(&self) -> Option<&Value> {
        match self {
            Self::Unknown { data, .. } => Some(data),
            _ => None,
        }
    }
}

/// The modeled fields, then the extras that do not name one.
impl Serialize for Answer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Noul {
                noul,
                confidence,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "noul")?;
                map.field("noul", noul)?;
                map.field("confidence", confidence)?;
                map.end(extra)
            }
            Self::Choice {
                choice,
                confidence,
                probabilities,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "choice")?;
                map.field("choice", choice)?;
                map.field("confidence", confidence)?;
                map.field("probabilities", probabilities)?;
                map.end(extra)
            }
            Self::Score {
                score,
                confidence,
                legend,
                probabilities,
                extra,
            } => {
                let mut map = ModeledMap::tagged(serializer, "score")?;
                map.field("score", score)?;
                map.field("confidence", confidence)?;
                map.field("legend", legend)?;
                map.field("probabilities", probabilities)?;
                map.end(extra)
            }
            Self::Unknown { answer_type, data } => {
                crate::serde_util::serialize_unknown_data(answer_type, data, serializer)
            }
        }
    }
}

/// Reads a known answer. Every field falls back to its zero value when
/// absent, as protobuf JSON leaves a zero out; a field of the wrong type
/// keeps the answer whole as [`Answer::Unknown`].
fn read_known_answer(object: &Map<String, Value>) -> Result<Answer, Fallback> {
    let mut f = WireFields::tagged(object);
    Ok(match f.tag() {
        Some("noul") => Answer::Noul {
            noul: f.or_default("noul")?,
            confidence: f.or_default("confidence")?,
            extra: f.extra(),
        },
        Some("choice") => Answer::Choice {
            choice: f.or_default("choice")?,
            confidence: f.or_default("confidence")?,
            probabilities: f.or_default("probabilities")?,
            extra: f.extra(),
        },
        Some("score") => Answer::Score {
            score: f.or_default("score")?,
            confidence: f.or_default("confidence")?,
            legend: f.or_default("legend")?,
            probabilities: f.or_default("probabilities")?,
            extra: f.extra(),
        },
        _ => return Err(Fallback::UnknownType),
    })
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match Fallback::read(&value, read_known_answer) {
            Ok(answer) => Ok(answer),
            Err(fallback) => {
                let answer_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<missing type>")
                    .to_string();
                tracing::warn!(
                    "Keeping a decision answer whole in Answer::Unknown: {} (Evergreen)",
                    fallback.describe("Answer", &answer_type)
                );
                Ok(Self::Unknown {
                    answer_type,
                    data: value,
                })
            }
        }
    }
}

wire_enum! {
    /// Why the input of a decision was blocked
    /// ([`PromptFeedback::block_reason`]).
    pub enum BlockReason {
        /// Blocked for safety.
        Safety = "safety",
        /// Blocked for another reason.
        Other = "other",
        /// The input holds a term from the terminology blocklist.
        Blocklist = "blocklist",
        /// The input holds prohibited content.
        ProhibitedContent = "prohibited_content",
        /// An image in the input was blocked for safety.
        ImageSafety = "image_safety",
    }
    unknown(reason_type, unknown_reason_type)
}

/// The input content filters' verdict on a decision request.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct PromptFeedback {
    /// Set when the input was blocked, in which case no answers come back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<BlockReason>,
    /// Why it was blocked, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason_message: Option<String>,
    /// Unmodeled fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The response of [`Decisions::create`].
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct CreateDecisionResponse {
    /// The answers, keyed by the question IDs of the request. Empty when the
    /// input was blocked ([`prompt_feedback`](Self::prompt_feedback)).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, Answer>,
    /// The input filters' verdict, when they had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<PromptFeedback>,
    /// Token usage: a decision reads input tokens and writes none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
    /// Unmodeled fields, preserved for roundtrip (Evergreen).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl CreateDecisionResponse {
    /// The answer to the question with this ID.
    #[must_use]
    pub fn answer(&self, question_id: &str) -> Option<&Answer> {
        self.answers.get(question_id)
    }

    /// Why the input was blocked, if it was.
    #[must_use]
    pub fn block_reason(&self) -> Option<&BlockReason> {
        self.prompt_feedback.as_ref()?.block_reason.as_ref()
    }
}

// =============================================================================
// Handle
// =============================================================================

impl Client {
    /// The `/v1beta/decisions` resource: evaluate decision questions against
    /// an input.
    ///
    /// The handle borrows the client and is `Copy`.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// use genai_rs::{CreateDecisionRequest, DEFAULT_DECISION_MODEL, Question};
    ///
    /// let request = CreateDecisionRequest::new(DEFAULT_DECISION_MODEL, "The soup was cold.")
    ///     .add_question("positive", Question::noul("Is the review positive?"));
    /// let response = client.decisions().create(&request).await?;
    /// println!("{:?}", response.answer("positive"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn decisions(&self) -> Decisions<'_> {
        Decisions { client: self }
    }
}

/// The `/v1beta/decisions` resource, from [`Client::decisions`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle.
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct Decisions<'a> {
    client: &'a Client,
}

impl Decisions<'_> {
    /// Evaluates the request's questions against its input.
    ///
    /// # Errors
    ///
    /// Returns an error if the request is rejected (a model other than
    /// [`DEFAULT_DECISION_MODEL`](crate::DEFAULT_DECISION_MODEL) is a 404;
    /// no questions, an unknown question type or a choice with fewer than
    /// two options is a 400), the HTTP request fails, or the response cannot
    /// be parsed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn run(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// use genai_rs::{Answer, CreateDecisionRequest, DEFAULT_DECISION_MODEL, Question};
    ///
    /// let request = CreateDecisionRequest::new(
    ///     DEFAULT_DECISION_MODEL,
    ///     "The food was cold and the waiter ignored us.",
    /// )
    /// .add_question(
    ///     "topic",
    ///     Question::choice("What is the complaint about?", [("food", ""), ("service", "")]),
    /// );
    /// let response = client.decisions().create(&request).await?;
    /// if let Some(Answer::Choice { choice, confidence, .. }) = response.answer("topic") {
    ///     println!("{choice} ({confidence:.2})");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(
        self,
        request: &CreateDecisionRequest,
    ) -> Result<CreateDecisionResponse, GenaiError> {
        crate::http::decisions::create_decision(&self.client.http, request).await
    }
}

#[cfg(test)]
#[path = "decisions_tests.rs"]
mod tests;
