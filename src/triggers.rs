//! Triggers resource (`/v1beta/triggers`) — server-side scheduled
//! interactions.
//!
//! A [`Trigger`] runs a stored interaction request on a cron
//! [`schedule`](Trigger::schedule) with **no client process running**: the
//! API creates a fresh interaction per firing, and past firings are
//! inspectable via [`list_executions`](Triggers::list_executions).
//!
//! Manage triggers through the [`Triggers`] handle from
//! [`Client::triggers`]: [`create`](Triggers::create),
//! [`get`](Triggers::get), [`list`](Triggers::list),
//! [`update`](Triggers::update), [`delete`](Triggers::delete),
//! [`run`](Triggers::run) and [`list_executions`](Triggers::list_executions).
//!
//! The whole lifecycle works against a custom agent built on
//! [`DEFAULT_ANTIGRAVITY_AGENT`](crate::DEFAULT_ANTIGRAVITY_AGENT): create,
//! get, list, update, run, list executions and delete (the live record is
//! under Triggers in `docs/INTERACTIONS_API_GAP.md`). The trigger's
//! `interaction` must target a custom `agent` (an [`agents`](crate::agents)
//! resource ID): plain `model` interactions are rejected ("Agent '' is
//! invalid or not found"), and `store` is not allowed in the nested request
//! ("Field 'store' is not allowed in 'interaction'"). A malformed cron
//! schedule gets only a generic 400 ("Request contains an invalid
//! argument").
//!
//! This is distinct from
//! `antigravity::TriggerConfig` (feature `antigravity`), which schedules
//! messages inside a *local* harness session.
//!
//! # IDs
//!
//! Methods take the bare ID ([`Trigger::id`]), not a `triggers/...` resource name:
//! the ID is percent-encoded into a single path segment, so a resource name
//! addresses nothing and 404s. An empty or dot-segment ID fails
//! locally with [`GenaiError::InvalidInput`]
//! before any request.

mod list;

pub use list::{
    ListTriggerExecutions, ListTriggers, TriggerExecution, TriggerExecutionListResponse,
    TriggerListResponse,
};

use crate::client::Client;
use crate::errors::GenaiError;
use crate::request::{InteractionInput, InteractionRequest};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

wire_enum! {
    /// Current status of a [`Trigger`].
    ///
    /// # Wire Format
    ///
    /// Serializes as lowercase strings: `"active"`, `"paused"`, `"error"`.
    pub enum TriggerStatus {
        /// The trigger fires on its schedule.
        Active = "active",
        /// The trigger is paused and does not fire.
        Paused = "paused",
        /// The trigger is disabled after consecutive failures.
        Error = "error",
    }
    unknown(status_type, unknown_status_type)
}

wire_enum! {
    /// Status of a single [`TriggerExecution`].
    ///
    /// # Wire Format
    ///
    /// Serializes as snake_case strings: `"in_progress"`, `"completed"`,
    /// `"failed"`, `"skipped"`, `"timed_out"`.
    pub enum TriggerExecutionStatus {
        /// The execution is still running.
        InProgress = "in_progress",
        /// The execution finished successfully.
        Completed = "completed",
        /// The execution failed.
        Failed = "failed",
        /// The execution was skipped (e.g. the prior one was still running).
        Skipped = "skipped",
        /// The execution exceeded its timeout.
        TimedOut = "timed_out",
    }
    unknown(status_type, unknown_status_type)
}

/// A server-side scheduled trigger, as returned by `/v1beta/triggers`.
///
/// All fields are optional with a struct-level serde default (the Evergreen
/// preserve-don't-reject posture, mirroring [`Agent`](crate::agents::Agent)),
/// so a field the API stops sending, or a projection that elides fields,
/// degrades per-field rather than failing the whole list response. A
/// paused trigger has no
/// `next_run_time`, and `environment_id`, `last_run_time` and
/// `previous_interaction_id` appear once it has fired.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct Trigger {
    /// Output only. The ID of the trigger — the value the ID-taking
    /// [`Triggers`] methods ([`get`](Triggers::get),
    /// [`delete`](Triggers::delete), ...) expect. A bare UUID on the wire,
    /// not a `triggers/...` resource name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Cron expression the trigger fires on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// IANA time zone the schedule is evaluated in (e.g. `"UTC"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
    /// The interaction request created on each firing.
    ///
    /// Lenient on this response side, so one odd trigger cannot fail a whole
    /// list: an undeserializable `input` reads as empty text and any other
    /// undeserializable `interaction` as `None`, each with a `warn!`. An
    /// absent `input` (list projections elide it) reads as empty text
    /// silently, so don't treat `interaction.input` as evidence of the stored
    /// prompt. [`CreateTriggerRequest`]'s send side stays strict.
    ///
    /// Interaction fields this crate does not model are kept in
    /// [`InteractionRequest::extra`] and sent back on reuse. Two roundtrip
    /// asymmetries remain: a bare `[Content]` input re-serializes as a
    /// `user_input` step, and a single content or step object as a
    /// one-element array (see [`InteractionInput`]).
    ///
    /// The server normalizes what it stores: a text input
    /// comes back as one `user_input` step, and the echo carries
    /// `store: true` plus `object: "interaction"` (kept in `extra`, and
    /// accepted back on create). To reuse a stored
    /// trigger's interaction in a new [`CreateTriggerRequest`], clear
    /// [`store`](InteractionRequest::store) first: the API rejects it on
    /// create.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_interaction"
    )]
    pub interaction: Option<InteractionRequest>,
    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ID of the environment fired interactions run against, as a bare ID
    /// for [`Environments::get`](crate::Environments::get). Without
    /// [`CreateTriggerRequest::with_environment_id`], the first firing
    /// creates one and the trigger reports it from then on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Output only. The current status of the trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerStatus>,
    /// Consecutive failures before the trigger is disabled (the server
    /// defaults it to 5).
    ///
    /// The API sends the trigger's int64s as plain JSON numbers, and they
    /// re-serialize that way. The protobuf-JSON string
    /// form (`"5"`, which the environments resource uses) is also accepted
    /// on deserialize, so one string-encoded value can't fail the whole
    /// list response.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_string_i64::<_, crate::serde_util::ForTrigger>"
    )]
    pub max_consecutive_failures: Option<i64>,
    /// Output only. Current count of consecutive failed executions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_string_i64::<_, crate::serde_util::ForTrigger>"
    )]
    pub consecutive_failure_count: Option<i64>,
    /// Per-execution timeout in seconds (the server defaults it to 600, its
    /// maximum).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_string_i64::<_, crate::serde_util::ForTrigger>"
    )]
    pub execution_timeout_seconds: Option<i64>,
    /// Output only. ID of the interaction the latest firing created, chained
    /// into the next firing's context; a bare ID for
    /// [`Interactions::get`](crate::Interactions::get).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,
    /// Output only. When the trigger was created.
    ///
    /// The wire spells this `create_time`, as the SDK spec does. The
    /// environments resource next door uses `created`, so
    /// that spelling is accepted too (deserialize-only; serialization keeps
    /// `create_time`). One arm still hard-fails: *both* spellings in one
    /// object is a serde duplicate-field error, raised before any
    /// lenient deserializer runs (inside a listed page the element-drop
    /// arm absorbs it, costing the one trigger).
    #[serde(
        default,
        alias = "created",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTrigger>"
    )]
    pub create_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last updated (`updated` accepted
    /// on deserialize; see [`Trigger::create_time`]).
    #[serde(
        default,
        alias = "updated",
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTrigger>"
    )]
    pub update_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger last fired.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTrigger>"
    )]
    pub last_run_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger next fires.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTrigger>"
    )]
    pub next_run_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last paused.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTrigger>"
    )]
    pub last_pause_time: Option<DateTime<Utc>>,
    /// Output only. When the trigger was last resumed.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTrigger>"
    )]
    pub last_resume_time: Option<DateTime<Utc>>,
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen).
    ///
    /// Without this, a deserialize-then-serialize cycle silently drops any
    /// field the crate has not modeled yet — invisible to the caller, and
    /// unrecoverable.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value`, matching the request-side escape hatches.
    /// (`to_string` on a flattened struct emits both keys rather than
    /// deduplicating; don't hand-serialize colliding keys.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Deserializes `Trigger::interaction`, degrading a nested `input` that
/// [`InteractionInput`]'s deserializer
/// rejects (a stray scalar) onto `None` with a `warn!`, and a non-object
/// `interaction` onto `None`. (Under default features a malformed steps
/// *array* never reaches the rejection path — the Evergreen `Step`
/// deserializer absorbs unrecognized elements as `Step::Unknown`
/// per-element; `genai_strict_unknown` rejects it and it degrades here like any
/// other bad input.)
///
/// Without this a projection carrying `input: 0` would propagate a hard
/// error up through `Trigger` and fail the whole list response — the same
/// wholesale failure the lenient int64 and timestamp helpers exist to
/// avoid. Scoped here (not on `InteractionRequest` itself) so a request
/// loaded from a config file still fails on a malformed input, and
/// [`CreateTriggerRequest`] also fails on a missing one.
fn deserialize_lenient_interaction<'de, D>(
    deserializer: D,
) -> Result<Option<InteractionRequest>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Object(mut obj)) => {
            // Take `input` out and parse the rest: one parse each, no clone.
            let raw_input = obj.remove("input");
            // Warn-and-drop like the arm below: a type mismatch on a
            // modeled field (numeric `model`, string `tools`) must not
            // zero the whole page either.
            let Ok(mut request) =
                serde_json::from_value::<InteractionRequest>(serde_json::Value::Object(obj))
                    .map_err(|e| {
                        tracing::warn!("Undeserializable trigger interaction, dropping: {e}")
                    })
            else {
                return Ok(None);
            };
            request.input = match raw_input {
                None | Some(serde_json::Value::Null) => None,
                Some(raw) => crate::request::input_from_value(raw)
                    .map_err(|e| {
                        tracing::warn!(
                            "Undeserializable input in trigger interaction ({e}); \
                             dropping it"
                        );
                    })
                    .ok(),
            };
            Ok(Some(request))
        }
        // Catch-all like the serde_util helpers: a non-object interaction
        // (a stray scalar, an array) degrades to None with a warn! instead
        // of failing the whole list response.
        Some(other) => {
            tracing::warn!("Unexpected JSON type for trigger interaction, dropping: {other:?}");
            Ok(None)
        }
    }
}

/// Request body for creating a [`Trigger`].
///
/// # Example
///
/// The struct literal below is the no-client form; with a [`Client`] in
/// scope, prefer `client.interaction()...build()` — the builder yields the
/// same [`InteractionRequest`] but stays source-compatible as fields are
/// added (struct literals break on every new public field; see the 0.9.0
/// CHANGELOG entry).
///
/// ```
/// use genai_rs::{InteractionInput, InteractionRequest, CreateTriggerRequest};
///
/// let interaction = InteractionRequest {
///     agent: Some("my-custom-agent".to_string()),
///     input: Some(InteractionInput::Text("Daily repo audit".to_string())),
///     ..Default::default()
/// };
/// let params = CreateTriggerRequest::new("0 9 * * *", "UTC", interaction)
///     .with_display_name("daily-audit");
/// ```
///
/// [`Client`]: crate::Client
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CreateTriggerRequest {
    /// Cron expression the trigger fires on.
    pub schedule: String,
    /// IANA time zone the schedule is evaluated in (e.g. `"UTC"`).
    pub time_zone: String,
    /// The interaction request created on each firing. Must target a
    /// custom `agent`; `store` is not allowed here (server-verified —
    /// [`CreateTriggerRequest::new`], the deserialize path, and
    /// [`Triggers::create`] itself all warn when it is set, the last
    /// covering struct literals and post-construction mutation too).
    #[serde(deserialize_with = "deserialize_interaction_with_warns")]
    pub interaction: InteractionRequest,
    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ID of the environment fired interactions run against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Consecutive failures before the trigger is disabled.
    ///
    /// Sends as a plain JSON number, as the response-side [`Trigger`]
    /// does. The protobuf-JSON string form is accepted on deserialize too,
    /// so a config file seeded from a stored [`Trigger`] loads cleanly —
    /// but strip the stored trigger's output-only keys first: they land in
    /// [`Self::extra`] and are forwarded verbatim, which the API rejects.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_strict_string_i64"
    )]
    pub max_consecutive_failures: Option<i64>,
    /// Per-execution timeout in seconds, at most 600 (the API rejects more
    /// with a 400). Sends as a plain JSON number (see
    /// [`Self::max_consecutive_failures`] on the wire-form asymmetry with
    /// [`Trigger`]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_strict_string_i64"
    )]
    pub execution_timeout_seconds: Option<i64>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — lets an
    /// unmodeled create-request field be set without a crate release.
    ///
    /// The inherent cost: a *typo'd optional* key in a deserialized
    /// config (`dispaly_name`, ...) is silently absorbed here and
    /// forwarded to the server verbatim rather than rejected — the
    /// send-side strictness documented on
    /// [`Trigger::interaction`] covers the nested request and the
    /// required top-level fields, not optional key spellings.
    ///
    /// A key that collides with a modeled field **wins**, so the escape
    /// hatch can also override a modeled field whose wire shape turns out
    /// wrong. The request body is written with both keys, the `extra` one
    /// last, and the API keeps the last of a repeated key;
    /// `serde_json::to_value` keeps only the `extra` one.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Pre-flight warnings for a trigger's nested interaction, each a
/// server-side rejection or a silent failure caught before the round-trip:
/// - `store` set: the API rejects it inside a trigger's interaction.
/// - no `agent`: a trigger must target a custom agent; a model-only request
///   is refused ("Agent '' is invalid or not found").
/// - no input, or an empty one: would fire on a schedule with an empty
///   prompt.
///
/// Called from [`CreateTriggerRequest::new`], the deserialize path, and
/// [`Triggers::create`] (which also catches struct literals and later
/// mutation), so `new`-then-create warns twice. Warns rather than errors, so
/// the server stays the authority if it relaxes a rule.
pub(crate) fn warn_on_interaction_footguns(interaction: &InteractionRequest) {
    if interaction.store.is_some() {
        tracing::warn!(
            "CreateTriggerRequest: `store` is set on the nested interaction; \
             the API rejects it in trigger requests"
        );
    }
    if interaction.agent.is_none() {
        tracing::warn!(
            "CreateTriggerRequest: the nested interaction targets no `agent`; \
             the API requires a custom agent for triggers and rejects \
             model-only interactions (\"Agent '' is invalid or not found\")"
        );
    }
    let input_is_empty = match &interaction.input {
        None => true,
        Some(InteractionInput::Text(text)) => text.is_empty(),
        Some(InteractionInput::Content(content)) => content.is_empty(),
        Some(InteractionInput::Steps(steps)) => steps.is_empty(),
    };
    if input_is_empty {
        tracing::warn!(
            "CreateTriggerRequest: the nested interaction's input is missing \
             or empty (empty text, content, or steps); the trigger would fire \
             on its schedule with an empty prompt"
        );
    }
}

/// Derived [`InteractionRequest`] deserialization plus the pre-flight
/// warns, so a config-file load gets the same signals as `new()`.
///
/// A missing `input` is an error here, as it was before `input` became
/// optional for decoding continuations (#489): a trigger fires a fresh
/// interaction, never a continuation, so an absent or misspelled
/// (`"inputs"`) input in a config file fails the load instead of
/// scheduling an empty prompt.
fn deserialize_interaction_with_warns<'de, D>(
    deserializer: D,
) -> Result<InteractionRequest, D::Error>
where
    D: Deserializer<'de>,
{
    let interaction = InteractionRequest::deserialize(deserializer)?;
    if interaction.input.is_none() {
        return Err(serde::de::Error::missing_field("input"));
    }
    warn_on_interaction_footguns(&interaction);
    Ok(interaction)
}

impl CreateTriggerRequest {
    /// Creates trigger parameters for `schedule` (cron) in `time_zone`.
    #[must_use]
    pub fn new(
        schedule: impl Into<String>,
        time_zone: impl Into<String>,
        interaction: InteractionRequest,
    ) -> Self {
        warn_on_interaction_footguns(&interaction);
        Self {
            schedule: schedule.into(),
            time_zone: time_zone.into(),
            interaction,
            display_name: None,
            environment_id: None,
            max_consecutive_failures: None,
            execution_timeout_seconds: None,
            extra: serde_json::Map::new(),
        }
    }

    /// Sets the display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Sets the environment ID fired interactions run against.
    #[must_use]
    pub fn with_environment_id(mut self, id: impl Into<String>) -> Self {
        self.environment_id = Some(id.into());
        self
    }

    /// Sets the consecutive-failure limit before the trigger is disabled.
    #[must_use]
    pub fn with_max_consecutive_failures(mut self, count: i64) -> Self {
        self.max_consecutive_failures = Some(count);
        self
    }

    /// Sets the per-execution timeout in seconds, at most 600.
    #[must_use]
    pub fn with_execution_timeout_seconds(mut self, seconds: i64) -> Self {
        self.execution_timeout_seconds = Some(seconds);
        self
    }
}

/// Update payload for a [`Trigger`] — unset fields are omitted from the
/// PATCH body.
///
/// Unlike webhook updates
/// ([`WebhookUpdate::update_mask`](crate::WebhookUpdate::update_mask)), the
/// SDK spec exposes **no `update_mask` parameter** for trigger updates
/// (google-genai 2.34.0: `triggers.update(id, display_name, status)` only),
/// so field omission is the only scoping mechanism. It works: the PATCH
/// applies exactly the fields present in the body.
/// Nothing else is updatable; sending `schedule` gets
/// `400 Unknown parameter 'schedule'`, so a new schedule means a new trigger.
/// The body is flat, as the bindings define it: the OpenAPI spec's wrapped
/// form (`{trigger, update_mask, id}`) is a `400 Unknown parameter` (under
/// "Spec-only query parameters" in `docs/INTERACTIONS_API_GAP.md`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TriggerUpdate {
    /// New display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// New status (`active` to resume, `paused` to pause).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerStatus>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — lets an
    /// unmodeled update field be sent without a crate release. Empty maps
    /// add nothing to the body, keeping the empty-update-is-`{}` contract.
    /// Colliding keys win on serialize, as on
    /// [`CreateTriggerRequest::extra`].
    ///
    /// The same typo cost as its siblings, sharpened by this endpoint: a
    /// typo'd optional key in a deserialized config (`dispaly_name`, ...)
    /// is silently absorbed here and forwarded verbatim — and since there
    /// is no `update_mask`, body-key omission is the only update-scoping
    /// mechanism, so the swallowed typo is indistinguishable from a
    /// deliberate no-op update.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl TriggerUpdate {
    /// Creates an empty update.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the display name.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Sets the status.
    ///
    /// [`Active`](TriggerStatus::Active) resumes the trigger and
    /// [`Paused`](TriggerStatus::Paused) pauses it — the two values a
    /// caller can send. [`Error`](TriggerStatus::Error) is output-only (the
    /// server sets it after consecutive failures): sending it gets
    /// `400 Invalid status transition from active to error`. The open enum
    /// accepts it here per the Evergreen posture.
    ///
    /// In a read-modify-write flow an
    /// [`is_unknown()`](TriggerStatus::is_unknown) status read from the API
    /// is sent back as the value it arrived as, string or not.
    #[must_use]
    pub fn with_status(mut self, status: TriggerStatus) -> Self {
        self.status = Some(status);
        self
    }
}

impl Client {
    /// The `/v1beta/triggers` resource: create, inspect, update, fire and
    /// delete server-side scheduled triggers, and list their executions.
    ///
    /// The handle borrows the client and is `Copy`; see
    /// [IDs](crate::triggers#ids) for what the methods take.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let trigger = client.triggers().get("trig-123").await?;
    /// # let _ = trigger;
    /// # Ok(())
    /// # }
    /// ```
    pub fn triggers(&self) -> Triggers<'_> {
        Triggers { client: self }
    }
}

/// The `/v1beta/triggers` resource, from [`Client::triggers`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle: `client.triggers().get(id)` can be stored or
/// joined with others. See [IDs](crate::triggers#ids).
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct Triggers<'a> {
    client: &'a Client,
}

impl<'a> Triggers<'a> {
    /// Creates a server-side scheduled trigger.
    ///
    /// The trigger's `interaction` must target a custom `agent` (see
    /// [`crate::triggers`] for the live-verified constraints).
    /// Like [`CreateTriggerRequest::new`], this warns when the nested
    /// interaction sets `store`, targets no `agent`, or has empty input.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the API rejects the
    /// trigger definition.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, InteractionInput, InteractionRequest, CreateTriggerRequest};
    ///
    /// # async fn example(client: Client) -> Result<(), genai_rs::GenaiError> {
    /// let interaction = InteractionRequest {
    ///     agent: Some("my-custom-agent".to_string()),
    ///     input: Some(InteractionInput::Text("Daily repo audit".to_string())),
    ///     ..Default::default()
    /// };
    /// let trigger = client
    ///     .triggers()
    ///     .create(
    ///         &CreateTriggerRequest::new("0 9 * * *", "UTC", interaction)
    ///             .with_display_name("daily-audit"),
    ///     )
    ///     .await?;
    /// println!("Created {:?}, next run {:?}", trigger.id, trigger.next_run_time);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(self, params: &CreateTriggerRequest) -> Result<Trigger, GenaiError> {
        crate::http::triggers::create_trigger(&self.client.http, params).await
    }

    /// Retrieves a trigger by ID.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    pub async fn get(self, trigger_id: &str) -> Result<Trigger, GenaiError> {
        crate::http::triggers::get_trigger(&self.client.http, trigger_id).await
    }

    /// Lists triggers: configure the returned [`ListTriggers`], then call
    /// [`send`](ListTriggers::send) for one page, or
    /// [`pages`](ListTriggers::pages) / [`items`](ListTriggers::items) to
    /// stream them all.
    pub fn list(self) -> ListTriggers<'a> {
        ListTriggers::new(self.client)
    }

    /// Updates a trigger with the fields set on `update` (display name
    /// and/or status; `paused` pauses it, `active` resumes it).
    ///
    /// Only set fields are sent. There is no `update_mask` on this endpoint;
    /// see [`TriggerUpdate`].
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, TriggerStatus, TriggerUpdate};
    ///
    /// # async fn example(client: Client) -> Result<(), genai_rs::GenaiError> {
    /// // Pause a trigger
    /// let paused = client
    ///     .triggers()
    ///     .update("trig-123", &TriggerUpdate::new().with_status(TriggerStatus::Paused))
    ///     .await?;
    /// # let _ = paused;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update(
        self,
        trigger_id: &str,
        update: &TriggerUpdate,
    ) -> Result<Trigger, GenaiError> {
        crate::http::triggers::update_trigger(&self.client.http, trigger_id, update).await
    }

    /// Deletes a trigger.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    pub async fn delete(self, trigger_id: &str) -> Result<(), GenaiError> {
        crate::http::triggers::delete_trigger(&self.client.http, trigger_id).await
    }

    /// Fires a trigger immediately, outside its schedule, and returns the
    /// run's execution once the run has ended.
    ///
    /// This posts to the `executions` sub-collection (not a `:run` colon
    /// verb), the path in the google-genai generated bindings. It works on
    /// a paused trigger too.
    ///
    /// **The call lasts as long as the run.** The API holds the request
    /// until the run ends, up to the trigger's
    /// [`execution_timeout_seconds`](Trigger::execution_timeout_seconds)
    /// (at most 600), and then answers with the execution:
    /// [`Completed`](TriggerExecutionStatus::Completed) with the stored
    /// [`interaction_id`](TriggerExecution::interaction_id), or
    /// [`TimedOut`](TriggerExecutionStatus::TimedOut) with an
    /// [`error`](TriggerExecution::error) and no interaction, for example.
    /// A client [timeout](crate::ClientBuilder::with_timeout) shorter than
    /// the run ends the call with an error, not the run:
    /// [`list_executions`](Self::list_executions) lists it, in progress or
    /// ended. Every field of the execution stays optional, since the API
    /// once answered `{}`; the record is under Triggers in
    /// `docs/INTERACTIONS_API_GAP.md`.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, when the trigger doesn't exist,
    /// or when a client timeout ends the call before the run does.
    pub async fn run(self, trigger_id: &str) -> Result<TriggerExecution, GenaiError> {
        crate::http::triggers::run_trigger(&self.client.http, trigger_id).await
    }

    /// Lists a trigger's past executions: configure the returned
    /// [`ListTriggerExecutions`], then call
    /// [`send`](ListTriggerExecutions::send) for one page, or
    /// [`pages`](ListTriggerExecutions::pages) /
    /// [`items`](ListTriggerExecutions::items) to stream them all.
    ///
    /// Reads the same `executions` sub-collection [`run`](Self::run) posts
    /// to. A trigger that does not exist lists as an empty page, not a
    /// 404.
    pub fn list_executions(self, trigger_id: &str) -> ListTriggerExecutions<'a> {
        ListTriggerExecutions::new(self.client, trigger_id.to_owned())
    }
}
#[cfg(test)]
#[path = "triggers_tests.rs"]
mod tests;
