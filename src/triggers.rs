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
//! Server-side constraint (verified live 2026-08-08): the trigger's
//! `interaction` must target a custom `agent` (an [`agents`](crate::agents)
//! resource ID) — plain `model` interactions are rejected ("Agent '' is
//! invalid or not found") and `store` is not allowed in the nested request.
//! Custom-agent creation is gated/allowlisted on standard API keys, so
//! trigger creation is too; the CRUD surface is modeled for accounts where
//! it is available.
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

use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
use crate::request::{InteractionInput, InteractionRequest};
use crate::wire_enum::wire_enum;
use chrono::{DateTime, Utc};
use futures_util::stream::BoxStream;
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
/// preserve-don't-reject posture, mirroring [`Agent`](crate::agents::Agent)):
/// this resource shape is not yet fully live-verified — creation is
/// agent-gated — so a projection that elides fields must degrade per-field
/// rather than failing the whole list response.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct Trigger {
    /// Output only. The ID of the trigger — the value the ID-taking
    /// [`Triggers`] methods ([`get`](Triggers::get),
    /// [`delete`](Triggers::delete), ...) expect.
    /// Like its siblings on this wire-unverified family (see
    /// [`Trigger::environment_id`]), it may arrive in `triggers/...`
    /// resource-name form; strip such a prefix before passing it back.
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
    /// prompt. [`TriggerCreateParams`]'s send side stays strict.
    ///
    /// Two roundtrip asymmetries follow from this being an
    /// [`InteractionRequest`]: interaction fields this crate does not model
    /// are dropped (it has no `extra`), and a bare `[Content]` input
    /// re-serializes as a `user_input` step.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_lenient_interaction"
    )]
    pub interaction: Option<InteractionRequest>,
    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ID of the environment fired interactions run against.
    ///
    /// This wire-unverified family may deliver IDs in `environments/...`
    /// resource-name form; strip such a prefix before passing the value
    /// back to an ID-taking client method (they percent-encode a slash
    /// into the path — see [`Client::get_environment`](crate::Client::get_environment)).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Output only. The current status of the trigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerStatus>,
    /// Consecutive failures before the trigger is disabled.
    ///
    /// Tolerates the protobuf-JSON string form on deserialize (live-verified
    /// on the environments resource's int64s) so one string-encoded int
    /// can't fail the whole list response, and re-serializes in the same
    /// string form for roundtrip uniformity with [`Environment`]'s counts.
    /// (The send direction is [`TriggerCreateParams`], which emits plain
    /// numbers — protobuf-JSON accepts both on input.)
    ///
    /// [`Environment`]: crate::environments::Environment
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_util::serialize_string_i64",
        deserialize_with = "crate::serde_util::deserialize_string_i64::<_, crate::serde_util::ForTrigger>"
    )]
    pub max_consecutive_failures: Option<i64>,
    /// Output only. Current count of consecutive failed executions.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_util::serialize_string_i64",
        deserialize_with = "crate::serde_util::deserialize_string_i64::<_, crate::serde_util::ForTrigger>"
    )]
    pub consecutive_failure_count: Option<i64>,
    /// Per-execution timeout in seconds.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::serde_util::serialize_string_i64",
        deserialize_with = "crate::serde_util::deserialize_string_i64::<_, crate::serde_util::ForTrigger>"
    )]
    pub execution_timeout_seconds: Option<i64>,
    /// Output only. ID of the previous fired interaction, chained into the
    /// next firing's context. May arrive in `interactions/...` resource-name
    /// form on this wire-unverified family — strip the prefix before
    /// feeding it back to an ID-taking client method (see
    /// [`Trigger::environment_id`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_interaction_id: Option<String>,
    /// Output only. When the trigger was created.
    ///
    /// The SDK spec spells this `create_time` while the live-verified
    /// environments resource next door uses `created` — the alias hedges
    /// that documented bet (deserialize-only; serialization keeps the
    /// spec spelling). One arm still hard-fails: *both* spellings in one
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
    /// unrecoverable. This is the sharpest case in the family: trigger creation is
    /// agent-gated, so the response shape has never been live-verified and a
    /// field the API returns today would be both invisible and unrecoverable.
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
/// rejects (explicit null, a stray scalar) onto the empty default with a
/// `warn!` before parsing the interaction, and a non-object `interaction`
/// onto `None`. (Under default features a malformed steps *array* never
/// reaches the rejection path — the Evergreen `Step` deserializer absorbs
/// unrecognized elements as `Step::Unknown` per-element; `strict-unknown`
/// rejects it and it degrades here like any other bad input.)
///
/// The nested `input` is the one non-`Option` field in the trigger tree,
/// so without this a projection carrying `input: null` (or `input: 0`)
/// would propagate a hard error up through `Trigger` and fail the whole
/// list response — the same wholesale failure the lenient int64 and
/// timestamp helpers exist to avoid. Scoped here (not on
/// `InteractionRequest` itself) so the send side stays strict.
fn deserialize_lenient_interaction<'de, D>(
    deserializer: D,
) -> Result<Option<InteractionRequest>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        // Catch-all like the serde_util helpers: a non-object interaction
        // (a stray scalar, an array) degrades to None with a warn! instead
        // of failing the whole list response.
        Some(other) if !other.is_object() => {
            tracing::warn!("Unexpected JSON type for trigger interaction, dropping: {other:?}");
            Ok(None)
        }
        Some(mut value) => {
            // Take `input` out and parse the rest — one parse each, no
            // clone (the catch-all arm above guarantees an object here).
            // The placeholder satisfies the now-required field; a sparse
            // projection's absent input thus deserializes to empty text
            // *on this path only* — the send side keeps input required.
            let obj = value
                .as_object_mut()
                .expect("non-object interactions are handled by the arm above");
            let raw_input = obj.remove("input");
            obj.insert(
                "input".to_string(),
                serde_json::Value::String(String::new()),
            );
            // Warn-and-drop like the arms above: a type mismatch on a
            // modeled field (numeric `model`, string `tools`) must not
            // zero the whole page either.
            let Ok(mut request) = serde_json::from_value::<InteractionRequest>(value)
                .map_err(|e| tracing::warn!("Undeserializable trigger interaction, dropping: {e}"))
            else {
                return Ok(None);
            };
            if let Some(raw) = raw_input {
                request.input = crate::request::input_from_value(raw).unwrap_or_else(|e| {
                    tracing::warn!(
                        "Undeserializable input in trigger interaction ({e}); \
                         degrading to empty text"
                    );
                    crate::request::InteractionInput::default()
                });
            }
            Ok(Some(request))
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
/// use genai_rs::{InteractionInput, InteractionRequest, TriggerCreateParams};
///
/// let interaction = InteractionRequest {
///     agent: Some("my-custom-agent".to_string()),
///     input: InteractionInput::Text("Daily repo audit".to_string()),
///     ..Default::default()
/// };
/// let params = TriggerCreateParams::new("0 9 * * *", "UTC", interaction)
///     .with_display_name("daily-audit");
/// ```
///
/// [`Client`]: crate::Client
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TriggerCreateParams {
    /// Cron expression the trigger fires on.
    pub schedule: String,
    /// IANA time zone the schedule is evaluated in (e.g. `"UTC"`).
    pub time_zone: String,
    /// The interaction request created on each firing. Must target a
    /// custom `agent`; `store` is not allowed here (server-verified —
    /// [`TriggerCreateParams::new`], the deserialize path, and
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
    /// Sends as a plain JSON number. The same logical field on the
    /// *response*-side [`Trigger`] re-serializes in the protobuf-JSON
    /// string form (for roundtrip fidelity to captured wire), so a
    /// read-modify-recreate flow changes the wire spelling — both forms
    /// are accepted on deserialize (here too, so a config file seeded
    /// from a stored [`Trigger`] loads cleanly — but strip the stored
    /// trigger's output-only keys first: they land in [`Self::extra`]
    /// and are forwarded verbatim, which the API rejects).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_strict_string_i64"
    )]
    pub max_consecutive_failures: Option<i64>,
    /// Per-execution timeout in seconds. Sends as a plain JSON number
    /// (see [`Self::max_consecutive_failures`] on the wire-form
    /// asymmetry with [`Trigger`]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_strict_string_i64"
    )]
    pub execution_timeout_seconds: Option<i64>,
    /// Unrecognized fields, preserved for roundtrip (Evergreen) — lets an
    /// unmodeled create-request field be set without a crate release,
    /// which matters here because trigger creation is agent-gated and the
    /// body cannot be live-verified against the wire.
    ///
    /// The inherent cost: a *typo'd optional* key in a deserialized
    /// config (`dispaly_name`, ...) is silently absorbed here and
    /// forwarded to the server verbatim rather than rejected — the
    /// send-side strictness documented on
    /// [`Trigger::interaction`] covers the nested request and the
    /// required top-level fields, not optional key spellings.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value` — the form the request path uses — so the
    /// escape hatch can also override a modeled field whose wire shape
    /// turns out wrong. (`to_string` on a flattened struct emits both keys
    /// rather than deduplicating; don't hand-serialize colliding params.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Pre-flight warnings for a trigger's nested interaction, each a
/// server-side rejection or a silent failure the agent gate would otherwise
/// hide until the round-trip:
/// - `store` set: the API rejects it inside a trigger's interaction.
/// - no `agent`: a trigger must target a custom agent; a model-only request
///   is refused ("Agent '' is invalid or not found").
/// - empty input: would fire on a schedule with an empty prompt.
///
/// Called from [`TriggerCreateParams::new`], the deserialize path, and
/// [`Triggers::create`] (which also catches struct literals and later
/// mutation), so `new`-then-create warns twice. Warns rather than errors: the
/// full shape can't be validated while creation is agent-gated.
pub(crate) fn warn_on_interaction_footguns(interaction: &InteractionRequest) {
    if interaction.store.is_some() {
        tracing::warn!(
            "TriggerCreateParams: `store` is set on the nested interaction; \
             the API rejects it in trigger requests"
        );
    }
    if interaction.agent.is_none() {
        tracing::warn!(
            "TriggerCreateParams: the nested interaction targets no `agent`; \
             the API requires a custom agent for triggers and rejects \
             model-only interactions (\"Agent '' is invalid or not found\")"
        );
    }
    let input_is_empty = match &interaction.input {
        InteractionInput::Text(text) => text.is_empty(),
        InteractionInput::Content(content) => content.is_empty(),
        InteractionInput::Steps(steps) => steps.is_empty(),
    };
    if input_is_empty {
        tracing::warn!(
            "TriggerCreateParams: the nested interaction's input is empty \
             (empty text, content, or steps); the trigger would fire on \
             its schedule with an empty prompt"
        );
    }
}

/// Derived [`InteractionRequest`] deserialization plus the pre-flight
/// warns, so a config-file load gets the same signals as `new()`.
fn deserialize_interaction_with_warns<'de, D>(
    deserializer: D,
) -> Result<InteractionRequest, D::Error>
where
    D: Deserializer<'de>,
{
    let interaction = InteractionRequest::deserialize(deserializer)?;
    warn_on_interaction_footguns(&interaction);
    Ok(interaction)
}

impl TriggerCreateParams {
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

    /// Sets the per-execution timeout in seconds.
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
/// (google-genai 2.17.0: `triggers.update(id, display_name, status)` only),
/// so field omission is the only scoping mechanism available. The sibling
/// webhooks PATCH was observed live (2026-07) to apply exactly the fields
/// present in the body, but trigger updates are not live-verifiable while
/// creation is agent-gated — treat the partial-update semantics as
/// unconfirmed until then.
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
    /// [`TriggerCreateParams::extra`].
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
    /// caller meaningfully sends. [`Error`](TriggerStatus::Error) is
    /// output-only (the server sets it after consecutive failures); the
    /// open enum accepts it here per the Evergreen posture, but sending it
    /// is untested against the live API.
    ///
    /// In a read-modify-write flow, don't echo back a status whose
    /// [`is_unknown()`](TriggerStatus::is_unknown) type came from a
    /// *non-string* wire value: its wire form is the crate's
    /// `<non-string: ...>` debug marker, not the original value. (An
    /// unknown *string* status round-trips faithfully.)
    #[must_use]
    pub fn with_status(mut self, status: TriggerStatus) -> Self {
        self.status = Some(status);
        self
    }
}

/// A single firing of a [`Trigger`].
///
/// All fields optional with a struct-level serde default; see [`Trigger`]
/// for the rationale.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct TriggerExecution {
    /// Output only. The ID of the execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Output only. The ID of the trigger that fired. Like the other IDs
    /// on this wire-unverified family, may arrive in `triggers/...`
    /// resource-name form — strip the prefix before feeding it back to an
    /// ID-taking client method (see [`Trigger::environment_id`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    /// Output only. The interaction created by this firing (same
    /// resource-name caveat as [`TriggerExecution::trigger_id`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    /// Output only. The environment the firing ran against (same
    /// resource-name caveat as [`TriggerExecution::trigger_id`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Output only. Status of this execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerExecutionStatus>,
    /// Output only. Error message when the execution failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Output only. When the firing was scheduled for.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTriggerExecution>"
    )]
    pub scheduled_time: Option<DateTime<Utc>>,
    /// Output only. When the execution started.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTriggerExecution>"
    )]
    pub start_time: Option<DateTime<Utc>>,
    /// Output only. When the execution finished.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::serde_util::deserialize_lenient_timestamp::<_, crate::serde_util::ForTriggerExecution>"
    )]
    pub end_time: Option<DateTime<Utc>>,
    /// Fields the API returned that this struct does not model, preserved
    /// for roundtrip (Evergreen).
    ///
    /// Without this, a deserialize-then-serialize cycle silently drops any
    /// field the crate has not modeled yet — invisible to the caller, and
    /// unrecoverable. Same unverified-shape caveat as [`Trigger`]: the executions
    /// listing has not been observed against a real agent-gated trigger.
    ///
    /// A key that collides with a modeled field **wins on serialize** via
    /// `serde_json::to_value`, matching the request-side escape hatches.
    /// (`to_string` on a flattened struct emits both keys rather than
    /// deduplicating; don't hand-serialize colliding keys.)
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response from listing triggers.
///
/// The API returns `{}` when no triggers exist (verified live 2026-08-08),
/// so both fields default.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct TriggerListResponse {
    /// The triggers in this page. A null or malformed list degrades to
    /// empty; malformed elements drop individually.
    #[serde(deserialize_with = "crate::serde_util::deserialize_lenient_vec")]
    pub triggers: Vec<Trigger>,
    /// Token for fetching the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

paging::impl_list_page!(TriggerListResponse, triggers: Trigger);

/// Response from listing a trigger's executions.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct TriggerExecutionListResponse {
    /// The executions in this page. A null or malformed list degrades to
    /// empty; malformed elements drop individually. The envelope key is
    /// the unverified `trigger_executions` per the SDK spec; `executions`
    /// (the path-segment spelling) is accepted on deserialize as a hedge.
    /// Both keys in one envelope is a serde duplicate-field error raised
    /// before the lenient helper runs — the one alias arm that fails the
    /// whole call rather than degrading.
    #[serde(
        alias = "executions",
        deserialize_with = "crate::serde_util::deserialize_lenient_vec"
    )]
    pub trigger_executions: Vec<TriggerExecution>,
    /// Token for fetching the next page, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

paging::impl_list_page!(TriggerExecutionListResponse, trigger_executions: TriggerExecution);

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
    /// [`crate::triggers`] for the live-verified constraints); trigger
    /// creation is gated with custom-agent creation on standard API keys.
    /// Like [`TriggerCreateParams::new`], this warns when the nested
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
    /// use genai_rs::{Client, InteractionInput, InteractionRequest, TriggerCreateParams};
    ///
    /// # async fn example(client: Client) -> Result<(), genai_rs::GenaiError> {
    /// let interaction = InteractionRequest {
    ///     agent: Some("my-custom-agent".to_string()),
    ///     input: InteractionInput::Text("Daily repo audit".to_string()),
    ///     ..Default::default()
    /// };
    /// let trigger = client
    ///     .triggers()
    ///     .create(
    ///         &TriggerCreateParams::new("0 9 * * *", "UTC", interaction)
    ///             .with_display_name("daily-audit"),
    ///     )
    ///     .await?;
    /// println!("Created {:?}, next run {:?}", trigger.id, trigger.next_run_time);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(self, params: &TriggerCreateParams) -> Result<Trigger, GenaiError> {
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
        ListTriggers {
            client: self.client,
            page_size: None,
            page_token: None,
        }
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

    /// Fires a trigger immediately, outside its schedule.
    ///
    /// This posts to the `executions` sub-collection (not a `:run` colon
    /// verb), the path in the google-genai generated bindings. The path
    /// exists server-side (live 2026-09-26: a missing trigger is a
    /// structured `404 NOT_FOUND`), but the response shape is unverified: it
    /// needs an existing trigger, and trigger creation is agent-gated (see
    /// [`triggers`](crate::triggers)).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
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
    /// to. A trigger that does not exist lists as an empty page, not a 404
    /// (live 2026-09-26). The execution shape is unverified, for the reason
    /// given on [`run`](Self::run).
    pub fn list_executions(self, trigger_id: &str) -> ListTriggerExecutions<'a> {
        ListTriggerExecutions {
            client: self.client,
            trigger_id: trigger_id.to_owned(),
            page_size: None,
            page_token: None,
        }
    }
}

/// A `GET /v1beta/triggers` request, from [`Triggers::list`].
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `next_page_token` to the end of the list. The page size is sent with
/// every page.
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client.triggers().list().with_page_size(50).send().await?;
/// println!("{} triggers, more: {}", page.triggers.len(), page.next_page_token.is_some());
///
/// // Every trigger, across pages
/// let all: Vec<genai_rs::Trigger> = client.triggers().list().items().try_collect().await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListTriggers<'a> {
    client: &'a Client,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListTriggers<'a> {
    /// Sets the maximum number of triggers per page. Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Sends the request and returns one page.
    ///
    /// An empty collection comes back as an empty page with no
    /// `next_page_token` (the API answers `{}`).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, an invalid page token, or a
    /// response that fails to parse.
    pub async fn send(self) -> Result<TriggerListResponse, GenaiError> {
        crate::http::triggers::list_triggers(
            &self.client.http,
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
    pub fn pages(self) -> BoxStream<'static, Result<TriggerListResponse, GenaiError>> {
        let Self {
            client,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("triggers", page_token, move |token| {
            let client = client.clone();
            async move {
                crate::http::triggers::list_triggers(&client.http, page_size, token.as_deref())
                    .await
            }
        })
    }

    /// Streams every trigger across pages, in server order. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<Trigger, GenaiError>> {
        paging::items(self.pages())
    }
}

/// A `GET /v1beta/triggers/{id}/executions` request, from
/// [`Triggers::list_executions`].
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `next_page_token` to the end of the list. The page size is sent with
/// every page. An empty or dot-segment trigger ID fails with
/// [`GenaiError::InvalidInput`] before any request (from `.send()`, or as
/// the streams' only item).
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // The ten most recent firings
/// let page = client
///     .triggers()
///     .list_executions("trig-123")
///     .with_page_size(10)
///     .send()
///     .await?;
/// for execution in &page.trigger_executions {
///     println!("{:?}: {:?}", execution.scheduled_time, execution.status);
/// }
///
/// // Every firing, across pages
/// let all: Vec<genai_rs::TriggerExecution> = client
///     .triggers()
///     .list_executions("trig-123")
///     .items()
///     .try_collect()
///     .await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListTriggerExecutions<'a> {
    client: &'a Client,
    trigger_id: String,
    page_size: Option<u32>,
    page_token: Option<String>,
}

impl<'a> ListTriggerExecutions<'a> {
    /// Sets the maximum number of executions per page. Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Sends the request and returns one page.
    ///
    /// No executions, and a trigger that does not exist, both come back as
    /// an empty page with no `next_page_token` (the API answers `{}`).
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid trigger ID or page token, on network
    /// failure, or when the response fails to parse.
    pub async fn send(self) -> Result<TriggerExecutionListResponse, GenaiError> {
        crate::http::triggers::list_trigger_executions(
            &self.client.http,
            &self.trigger_id,
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
    pub fn pages(self) -> BoxStream<'static, Result<TriggerExecutionListResponse, GenaiError>> {
        let Self {
            client,
            trigger_id,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("trigger executions", page_token, move |token| {
            let (client, trigger_id) = (client.clone(), trigger_id.clone());
            async move {
                crate::http::triggers::list_trigger_executions(
                    &client.http,
                    &trigger_id,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every execution across pages, in server order. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<TriggerExecution, GenaiError>> {
        paging::items(self.pages())
    }
}

#[cfg(test)]
#[path = "triggers_tests.rs"]
mod tests;
