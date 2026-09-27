//! The [`Interactions`] handle for stored interactions
//! (`/v1beta/interactions/{id}`).
//!
//! Creating an interaction stays on [`Client`]: [`Client::interaction`]
//! (the builder), [`Client::execute`] and [`Client::execute_stream`]. This
//! handle addresses one that already exists: fetch it, stream or resume it,
//! cancel it or delete it.

use crate::client::{Client, log_body};
use crate::errors::GenaiError;
use crate::{InteractionResponse, StreamEvent};
use futures_util::StreamExt;
use futures_util::stream::BoxStream;

impl Client {
    /// Returns the [`Interactions`] handle for stored interactions.
    ///
    /// ```no_run
    /// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
    /// let response = client.interactions().get("interaction-id").await?;
    /// println!("{:?}", response.status);
    /// # Ok(())
    /// # }
    /// ```
    pub fn interactions(&self) -> Interactions<'_> {
        Interactions { client: self }
    }
}

/// Stored interactions (`/v1beta/interactions/{id}`), from
/// [`Client::interactions`].
///
/// Methods take `self` by value, so each call's future holds only the client
/// borrow, never the handle: `client.interactions().get(id)` can be stored or
/// joined with others. The streams own a clone of the client, so they can be
/// stored or spawned.
///
/// # IDs
///
/// Methods take the bare ID ([`InteractionResponse::id`]), not an
/// `interactions/...` resource name: the ID is percent-encoded into a single
/// path segment, so a resource name addresses nothing and 404s. An empty or
/// dot-segment ID fails locally with [`GenaiError::InvalidInput`] before any
/// request.
#[derive(Clone, Copy, Debug)]
#[must_use = "a resource handle does nothing until you call one of its methods"]
pub struct Interactions<'a> {
    client: &'a Client,
}

impl Interactions<'_> {
    /// Retrieves an existing interaction by its ID.
    ///
    /// Useful for checking the status of long-running interactions or agents,
    /// or for retrieving the full conversation history. The response's
    /// `input` is `None`; use [`get_with_input`](Self::get_with_input) to ask
    /// for it.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the API returns an error,
    /// or response parsing fails. An empty or dot-segment ID fails locally
    /// with [`GenaiError::InvalidInput`].
    pub async fn get(self, interaction_id: &str) -> Result<InteractionResponse, GenaiError> {
        tracing::debug!("Getting interaction: ID={interaction_id}");

        let response =
            crate::http::interactions::get_interaction(&self.client.http, interaction_id, false)
                .await?;

        log_body("Response", &response);
        tracing::debug!("Retrieved interaction: status={:?}", response.status);

        Ok(response)
    }

    /// Retrieves an existing interaction by its ID, including the original input.
    ///
    /// Like [`get`](Self::get), but sets the `include_input=true` query
    /// parameter so the response's `input` field is populated.
    ///
    /// Live behavior note (2026-07): the parameter is accepted, but the
    /// Gemini API was observed to return identical responses with and
    /// without it — no `input` echo (and no `generation_config` echo) was
    /// observed on completed interactions.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the API returns an error,
    /// or response parsing fails. An empty or dot-segment ID fails locally
    /// with [`GenaiError::InvalidInput`].
    pub async fn get_with_input(
        self,
        interaction_id: &str,
    ) -> Result<InteractionResponse, GenaiError> {
        tracing::debug!("Getting interaction (with input): ID={interaction_id}");

        let response =
            crate::http::interactions::get_interaction(&self.client.http, interaction_id, true)
                .await?;

        log_body("Response", &response);
        tracing::debug!("Retrieved interaction: status={:?}", response.status);

        Ok(response)
    }

    /// Streams an existing interaction's events from the start.
    ///
    /// Useful for following a long-running interaction's progress (e.g.,
    /// deep research). Only **background** interactions
    /// (`with_background(true)`) can be streamed this way; for an ordinary
    /// one the API answers `400 Streaming retrieval of interactions is not
    /// supported`. Their events carry an `event_id` to resume from after an
    /// interruption with [`resume_stream`](Self::resume_stream).
    ///
    /// The stream owns a clone of the client and the ID, so it can be stored
    /// or spawned. Nothing is sent until it is first polled. An empty or
    /// dot-segment `interaction_id` is rejected locally as
    /// [`GenaiError::InvalidInput`]: the stream yields that error as its
    /// first (and only) item and no request is sent. Any other error is
    /// likewise the stream's last item.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, StreamChunk};
    /// use futures_util::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api_key".to_string());
    ///
    /// let mut stream = client.interactions().stream("some-interaction-id");
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     println!("Event ID: {:?}", event.event_id);
    ///     if let StreamChunk::Completed(response) = event.chunk {
    ///         println!("Done! Status: {:?}", response.status);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "streams do nothing unless polled"]
    pub fn stream(
        self,
        interaction_id: &str,
    ) -> BoxStream<'static, Result<StreamEvent, GenaiError>> {
        self.open_stream(interaction_id, None)
    }

    /// Resumes an existing interaction's stream after `last_event_id`.
    ///
    /// Pass the `event_id` of the last event you received; the resumed stream
    /// starts after that event. Otherwise like [`stream`](Self::stream),
    /// including its ownership, laziness and error behavior.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, StreamChunk};
    /// use futures_util::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api_key".to_string());
    ///
    /// let mut stream = client
    ///     .interactions()
    ///     .resume_stream("some-interaction-id", "evt_abc123");
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     match event.chunk {
    ///         StreamChunk::StepDelta { delta, .. } => {
    ///             if let Some(text) = delta.as_text() {
    ///                 print!("{}", text);
    ///             }
    ///         }
    ///         StreamChunk::Completed(response) => {
    ///             println!("\nDone! Status: {:?}", response.status);
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "streams do nothing unless polled"]
    pub fn resume_stream(
        self,
        interaction_id: &str,
        last_event_id: &str,
    ) -> BoxStream<'static, Result<StreamEvent, GenaiError>> {
        self.open_stream(interaction_id, Some(last_event_id))
    }

    fn open_stream(
        self,
        interaction_id: &str,
        last_event_id: Option<&str>,
    ) -> BoxStream<'static, Result<StreamEvent, GenaiError>> {
        let client = self.client.clone();
        let interaction_id = interaction_id.to_owned();
        let last_event_id = last_event_id.map(str::to_owned);

        Box::pin(async_stream::try_stream! {
            tracing::debug!(
                "Getting interaction stream: ID={}, resume_from={:?}",
                interaction_id,
                last_event_id
            );
            let events = crate::http::interactions::get_interaction_stream(
                &client.http,
                &interaction_id,
                last_event_id.as_deref(),
            );
            futures_util::pin_mut!(events);
            while let Some(event) = events.next().await {
                let event = event?;
                tracing::debug!(
                    "Received stream event: chunk={:?}, event_id={:?}",
                    event.chunk,
                    event.event_id
                );
                yield event;
            }
        })
    }

    /// Cancels an in-progress background interaction.
    ///
    /// Only applicable to interactions created with `background: true` that are
    /// still in `InProgress` status. Returns the updated interaction with
    /// status `Cancelled`.
    ///
    /// This is useful for:
    /// - Halting long-running agent tasks (e.g., deep-research) when requirements change
    /// - Cost control by stopping interactions consuming significant tokens
    /// - Implementing timeout handling in application logic
    /// - Supporting user-initiated cancellation in UIs
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The interaction doesn't exist
    /// - The interaction is not in a cancellable state (not background or already complete)
    /// - The HTTP request fails
    /// - The API returns an error
    ///
    /// An empty or dot-segment ID fails locally with [`GenaiError::InvalidInput`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, InteractionStatus};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("your-api-key".to_string());
    ///
    /// // Start a background agent interaction
    /// let response = client.interaction()
    ///     .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
    ///     .with_text("Research AI safety")
    ///     .with_background(true)
    ///     .with_store_enabled()
    ///     .create()
    ///     .await?;
    ///
    /// let interaction_id = response.id.as_ref().expect("stored interaction has id");
    ///
    /// // Later, cancel if still in progress
    /// if response.status == InteractionStatus::InProgress {
    ///     let cancelled = client.interactions().cancel(interaction_id).await?;
    ///     assert_eq!(cancelled.status, InteractionStatus::Cancelled);
    ///     println!("Interaction cancelled");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn cancel(self, interaction_id: &str) -> Result<InteractionResponse, GenaiError> {
        tracing::debug!("Cancelling interaction: ID={interaction_id}");

        let response =
            crate::http::interactions::cancel_interaction(&self.client.http, interaction_id)
                .await?;

        log_body("Response", &response);
        tracing::debug!("Interaction cancelled: status={:?}", response.status);

        Ok(response)
    }

    /// Deletes an interaction by its ID.
    ///
    /// Removes the interaction from the server, freeing up storage and making it
    /// unavailable for future reference via `previous_interaction_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the API returns an
    /// error. An empty or dot-segment ID fails locally with
    /// [`GenaiError::InvalidInput`].
    pub async fn delete(self, interaction_id: &str) -> Result<(), GenaiError> {
        tracing::debug!("Deleting interaction: ID={interaction_id}");

        crate::http::interactions::delete_interaction(&self.client.http, interaction_id).await?;

        tracing::debug!("Interaction deleted successfully");

        Ok(())
    }
}
