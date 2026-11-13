//! The listing side of triggers: the [`ListTriggers`] and
//! [`ListTriggerExecutions`] builders, the pages they return, and the
//! [`TriggerExecution`] record a firing leaves (D-029).

use super::{Trigger, TriggerExecutionStatus};
use crate::paging;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    /// Output only. The ID of the trigger that fired (a bare ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
    /// Output only. The interaction created by this firing, a bare ID for
    /// [`Interactions::get`](crate::Interactions::get).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    /// Output only. The environment the firing ran against, also on
    /// [`Trigger::environment_id`]. Not sent for a run that timed out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    /// Output only. Status of this execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TriggerExecutionStatus>,
    /// Output only. Error message when the execution failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Output only. When the firing was scheduled for. Absent on a manual
    /// [`run`](super::Triggers::run).
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
    /// unrecoverable.
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
/// The API returns `{}` when no triggers exist,
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
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(TriggerListResponse, triggers: Trigger);

/// Response from listing a trigger's executions.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[non_exhaustive]
pub struct TriggerExecutionListResponse {
    /// The executions in this page. A null or malformed list degrades to
    /// empty; malformed elements drop individually. The envelope key is
    /// `trigger_executions`, as in the SDK spec;
    /// `executions` (the path-segment spelling) is accepted on deserialize
    /// as a hedge.
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
    /// Fields the crate does not model yet, kept so a deserialize/serialize
    /// round trip preserves them. A list envelope is where the API is
    /// likeliest to add something (a total count, a page-size echo).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

paging::impl_list_page!(TriggerExecutionListResponse, trigger_executions: TriggerExecution);

paging::list_builder! {
    /// A `GET /v1beta/triggers` request, from
    /// [`Triggers::list`](super::Triggers::list).
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
    pub struct ListTriggers<'a> {
    }
    page: TriggerListResponse => Trigger;
    label: "triggers";

    /// Sets the maximum number of triggers per page. Sent with every page.
    fn with_page_size;

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    fn with_page_token;

    /// Sends the request and returns one page.
    ///
    /// An empty collection comes back as an empty page with no
    /// `next_page_token` (the API answers `{}`).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, an invalid page token, or a
    /// response that fails to parse.
    fn send;

    /// Streams every trigger across pages, in server order. Same rules as
    /// [`pages`](Self::pages).
    fn items;

    fetch: |http, page_size, token| crate::http::triggers::list_triggers(
        http,
        page_size,
        token.as_deref(),
    ),
}

paging::list_builder! {
    /// A `GET /v1beta/triggers/{id}/executions` request, from
    /// [`Triggers::list_executions`](super::Triggers::list_executions).
    ///
    /// End it with [`send`](Self::send) for one page, or
    /// [`pages`](Self::pages) / [`items`](Self::items) to follow
    /// `next_page_token` to the end of the list. The page size is sent with
    /// every page. An empty or dot-segment trigger ID fails with
    /// [`GenaiError::InvalidInput`](crate::GenaiError::InvalidInput) before
    /// any request (from `.send()`, or as the streams' only item).
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
    pub struct ListTriggerExecutions<'a> {
        trigger_id: String,
    }
    page: TriggerExecutionListResponse => TriggerExecution;
    label: "trigger executions";

    /// Sets the maximum number of executions per page. Sent with every page.
    fn with_page_size;

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    fn with_page_token;

    /// Sends the request and returns one page.
    ///
    /// No executions, and a trigger that does not exist, both come back as
    /// an empty page with no `next_page_token` (the API answers `{}`).
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid trigger ID or page token, on network
    /// failure, or when the response fails to parse.
    fn send;

    /// Streams every execution across pages, in server order. Same rules as
    /// [`pages`](Self::pages).
    fn items;

    fetch: |http, page_size, token| crate::http::triggers::list_trigger_executions(
        http,
        &trigger_id,
        page_size,
        token.as_deref(),
    ),
}
