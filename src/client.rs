use crate::GenaiError;
use crate::http::context::HttpContext;
use crate::wire::WireInspector;
use reqwest::Client as ReqwestClient;
use std::sync::Arc;
use std::time::Duration;

/// Logs a request body at debug level, preferring JSON format when possible.
fn log_request_body<T: std::fmt::Debug + serde::Serialize>(body: &T) {
    match serde_json::to_string_pretty(body) {
        Ok(json) => tracing::debug!("Request Body (JSON):\n{json}"),
        Err(_) => tracing::debug!("Request Body: {body:#?}"),
    }
}

/// Logs a response body at debug level, preferring JSON format when possible.
fn log_response_body<T: std::fmt::Debug + serde::Serialize>(body: &T) {
    match serde_json::to_string_pretty(body) {
        Ok(json) => tracing::debug!("Response Body (JSON):\n{json}"),
        Err(_) => tracing::debug!("Response Body: {body:#?}"),
    }
}

/// The main client for interacting with the Google Generative AI API.
#[derive(Clone)]
pub struct Client {
    /// Shared HTTP context: reqwest client, API key, wire inspectors, and
    /// the request-id counter for wire-event correlation.
    pub(crate) http: HttpContext,
}

// Custom Debug implementation that redacts the API key for security.
// This prevents accidental exposure of credentials in logs, error messages, or debug output.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("api_key", &"[REDACTED]")
            .field("http_client", &self.http.http_client)
            .finish()
    }
}

/// Appends a [`crate::wire::LoudWirePrinter`] when the `LOUD_WIRE`
/// environment variable is set, filtered by its value. Checked once at
/// `Client` construction time; shares
/// [`crate::wire::env_inspector`] with the antigravity agent builder so
/// the variable means the same thing on both paths.
fn with_env_inspectors(mut inspectors: Vec<Arc<dyn WireInspector>>) -> Vec<Arc<dyn WireInspector>> {
    if let Some(printer) = crate::wire::env_inspector() {
        inspectors.push(Arc::new(printer));
    }
    inspectors
}

/// Builder for `Client` instances.
///
/// # Example
///
/// ```
/// use genai_rs::Client;
/// use std::time::Duration;
///
/// let client = Client::builder("api_key".to_string())
///     .with_timeout(Duration::from_secs(120))
///     .with_connect_timeout(Duration::from_secs(10))
///     .build()?;
/// # Ok::<(), genai_rs::GenaiError>(())
/// ```
pub struct ClientBuilder {
    api_key: String,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    wire_inspectors: Vec<Arc<dyn WireInspector>>,
}

// Custom Debug implementation that redacts the API key for security.
impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &"[REDACTED]")
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("wire_inspectors", &self.wire_inspectors.len())
            .finish()
    }
}

impl ClientBuilder {
    /// Sets the total request timeout.
    ///
    /// This is the maximum time a request can take from start to finish,
    /// including connection time, sending the request, and receiving the response.
    ///
    /// For LLM requests that may take a long time to generate responses,
    /// consider setting a longer timeout (e.g., 120-300 seconds).
    ///
    /// If not set, requests will wait indefinitely (no timeout).
    /// Connection-level timeouts like TCP keepalive may still apply at the OS level.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// let client = Client::builder("api_key".to_string())
    ///     .with_timeout(Duration::from_secs(120))
    ///     .build()?;
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Sets the connection timeout.
    ///
    /// This is the maximum time to wait for establishing a connection to the server.
    /// A shorter timeout here can help fail fast if the network is unavailable.
    ///
    /// If not set, the connection phase will wait indefinitely (no timeout).
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// let client = Client::builder("api_key".to_string())
    ///     .with_connect_timeout(Duration::from_secs(10))
    ///     .build()?;
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub const fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Adds a wire inspector that observes raw API traffic.
    ///
    /// Inspectors receive a [`crate::wire::WireEvent`] for every request,
    /// response, error body, SSE frame, and file upload. Multiple inspectors
    /// may be registered; each receives every event. When the `LOUD_WIRE`
    /// environment variable is set, a [`crate::wire::LoudWirePrinter`] is
    /// appended automatically at `build()` time.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Client;
    /// use genai_rs::wire::TracingForwarder;
    /// use std::sync::Arc;
    ///
    /// let client = Client::builder("api_key".to_string())
    ///     .add_wire_inspector(Arc::new(TracingForwarder::new()))
    ///     .build()?;
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub fn add_wire_inspector(mut self, inspector: Arc<dyn WireInspector>) -> Self {
        self.wire_inspectors.push(inspector);
        self
    }

    /// Builds the `Client`.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed. This should only
    /// happen in exceptional circumstances such as TLS backend initialization failures.
    pub fn build(self) -> Result<Client, GenaiError> {
        let mut builder = ReqwestClient::builder();

        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }

        if let Some(connect_timeout) = self.connect_timeout {
            builder = builder.connect_timeout(connect_timeout);
        }

        let http_client = builder
            .build()
            .map_err(|e| GenaiError::ClientBuild(e.to_string()))?;

        Ok(Client {
            http: HttpContext::new(
                http_client,
                self.api_key,
                with_env_inspectors(self.wire_inspectors),
            ),
        })
    }
}

impl Client {
    /// Creates a new builder for `Client` instances.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Your Google AI API key.
    #[must_use]
    pub const fn builder(api_key: String) -> ClientBuilder {
        ClientBuilder {
            api_key,
            timeout: None,
            connect_timeout: None,
            wire_inspectors: Vec::new(),
        }
    }

    /// Creates a new `GenAI` client.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Your Google AI API key.
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            http: HttpContext::new(
                ReqwestClient::new(),
                api_key,
                with_env_inspectors(Vec::new()),
            ),
        }
    }

    // --- Interactions API methods ---

    /// Creates a builder for constructing an interaction request.
    ///
    /// This provides a fluent interface for building interactions with models or agents.
    /// Use this method for a more ergonomic API compared to manually constructing
    /// `InteractionRequest`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// // Simple interaction
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello, world!")
    ///     .create()
    ///     .await?;
    ///
    /// // Stateful conversation (requires stored interaction)
    /// let response2 = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What did I just say?")
    ///     .with_previous_interaction(response.id.as_ref().expect("stored interaction has id"))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn interaction(&self) -> crate::request_builder::InteractionBuilder<'_> {
        crate::request_builder::InteractionBuilder::new(self)
    }

    /// Creates a new interaction using the Gemini Interactions API.
    ///
    /// The Interactions API provides a unified interface for working with models and agents,
    /// with built-in support for stateful conversations, function calling, and long-running tasks.
    ///
    /// # Arguments
    ///
    /// * `request` - The interaction request with model/agent, input, and optional configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails
    /// - Response parsing fails
    /// - The API returns an error
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("your-api-key".to_string());
    ///
    /// // Build a reusable request with the builder, then execute it.
    /// let request = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello, world!")
    ///     .build()?;
    ///
    /// let response = client.execute(request).await?;
    /// println!("Interaction ID: {:?}", response.id);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Streaming Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, StreamChunk};
    /// use futures_util::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    /// let mut request = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Count to 5")
    ///     .build()?;
    /// request.stream = Some(true);
    ///
    /// let mut last_event_id = None;
    /// let mut stream = client.execute_stream(request);
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     last_event_id = event.event_id.clone();  // Track for resume
    ///     match event.chunk {
    ///         StreamChunk::StepDelta { delta, .. } => {
    ///             if let Some(text) = delta.as_text() {
    ///                 print!("{}", text);
    ///             }
    ///         }
    ///         StreamChunk::Completed(response) => {
    ///             println!("\nDone! ID: {:?}", response.id);
    ///         }
    ///         _ => {} // Handle unknown future variants
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Retry Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api_key".to_string());
    /// let request = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello!")
    ///     .build()?;
    ///
    /// // Retry loop with exponential backoff
    /// let mut attempts = 0;
    /// let response = loop {
    ///     match client.execute(request.clone()).await {
    ///         Ok(r) => break r,
    ///         Err(e) if e.is_retryable() && attempts < 3 => {
    ///             attempts += 1;
    ///             tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(attempts))).await;
    ///         }
    ///         Err(e) => return Err(e.into()),
    ///     }
    /// };
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip(self), fields(model = ?request.model, agent = ?request.agent))]
    pub async fn execute(
        &self,
        request: crate::InteractionRequest,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Creating interaction");
        log_request_body(&request);

        let response = crate::http::interactions::create_interaction(&self.http, request).await?;

        log_response_body(&response);
        tracing::debug!("Interaction created: ID={:?}", response.id);

        Ok(response)
    }

    /// Executes a pre-built interaction request with streaming.
    ///
    /// This is the streaming variant of [`execute()`](Self::execute).
    ///
    /// Returns a stream of [`StreamEvent`](crate::StreamEvent) items as they arrive.
    /// Each event contains:
    /// - `chunk`: The content (delta or complete response)
    /// - `event_id`: Optional ID for resuming interrupted streams
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
    /// let request = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Count to 5")
    ///     .build()?;
    ///
    /// let mut stream = client.execute_stream(request);
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     match event.chunk {
    ///         StreamChunk::StepDelta { delta, .. } => {
    ///             if let Some(text) = delta.as_text() {
    ///                 print!("{}", text);
    ///             }
    ///         }
    ///         StreamChunk::Completed(response) => {
    ///             println!("\nDone!");
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip(self), fields(model = ?request.model, agent = ?request.agent))]
    pub fn execute_stream(
        &self,
        request: crate::InteractionRequest,
    ) -> futures_util::stream::BoxStream<'_, Result<crate::StreamEvent, GenaiError>> {
        use futures_util::StreamExt;

        tracing::debug!("Creating streaming interaction");
        log_request_body(&request);

        let stream = crate::http::interactions::create_interaction_stream(&self.http, request);

        stream
            .map(move |result| {
                result.inspect(|event| {
                    tracing::debug!(
                        "Received stream event: chunk={:?}, event_id={:?}",
                        event.chunk,
                        event.event_id
                    );
                })
            })
            .boxed()
    }

    /// Retrieves an existing interaction by its ID.
    ///
    /// `interaction_id` is the bare ID (the form [`InteractionResponse::id`](crate::InteractionResponse)
    /// returns) — not an `interactions/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// Useful for checking the status of long-running interactions or agents,
    /// or for retrieving the full conversation history.
    ///
    /// # Arguments
    ///
    /// * `interaction_id` - The unique identifier of the interaction to retrieve.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails
    /// - Response parsing fails
    /// - The API returns an error
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn get_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Getting interaction: ID={interaction_id}");

        let response =
            crate::http::interactions::get_interaction(&self.http, interaction_id, false).await?;

        log_response_body(&response);
        tracing::debug!("Retrieved interaction: status={:?}", response.status);

        Ok(response)
    }

    /// Retrieves an existing interaction by its ID, including the original input.
    ///
    /// Like [`get_interaction()`](Self::get_interaction), but sets the
    /// `include_input=true` query parameter so the response's `input` field is
    /// populated.
    ///
    /// Live behavior note (2026-07): the parameter is accepted, but the
    /// Gemini API was observed to return identical responses with and
    /// without it — no `input` echo (and no `generation_config` echo) was
    /// observed on completed interactions.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails
    /// - Response parsing fails
    /// - The API returns an error
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn get_interaction_with_input(
        &self,
        interaction_id: &str,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Getting interaction (with input): ID={interaction_id}");

        let response =
            crate::http::interactions::get_interaction(&self.http, interaction_id, true).await?;

        log_response_body(&response);
        tracing::debug!("Retrieved interaction: status={:?}", response.status);

        Ok(response)
    }

    /// Retrieves an existing interaction by its ID with streaming.
    ///
    /// `interaction_id` is the bare ID (the form [`InteractionResponse::id`](crate::InteractionResponse)
    /// returns) — not an `interactions/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// Returns a stream of events for the interaction. This is useful for:
    /// - Resuming an interrupted stream using `last_event_id`
    /// - Streaming a long-running interaction's progress (e.g., deep research)
    ///
    /// Each event includes an `event_id` that can be used to resume the stream
    /// from that point if the connection is interrupted.
    ///
    /// # Arguments
    ///
    /// * `interaction_id` - The unique identifier of the interaction to stream.
    /// * `last_event_id` - Optional event ID to resume from. Pass the last received
    ///   event's `event_id` to continue from where you left off.
    ///
    /// # Returns
    /// A boxed stream that yields `StreamEvent` items.
    ///
    /// An empty or dot-segment `interaction_id` is rejected locally as
    /// [`GenaiError::InvalidInput`]: the returned stream yields that error
    /// as its first (and only) item and no request is sent.
    ///
    /// # Example
    /// ```no_run
    /// use genai_rs::{Client, StreamChunk};
    /// use futures_util::StreamExt;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    /// let interaction_id = "some-interaction-id";
    ///
    /// // Resume a stream from a previous event
    /// let last_event_id = Some("evt_abc123");
    /// let mut stream = client.get_interaction_stream(interaction_id, last_event_id);
    ///
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     println!("Event ID: {:?}", event.event_id);
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
    pub fn get_interaction_stream<'a>(
        &'a self,
        interaction_id: &'a str,
        last_event_id: Option<&'a str>,
    ) -> futures_util::stream::BoxStream<'a, Result<crate::StreamEvent, GenaiError>> {
        use futures_util::StreamExt;

        tracing::debug!(
            "Getting interaction stream: ID={}, resume_from={:?}",
            interaction_id,
            last_event_id
        );

        let stream = crate::http::interactions::get_interaction_stream(
            &self.http,
            interaction_id,
            last_event_id,
        );

        stream
            .map(move |result| {
                result.inspect(|event| {
                    tracing::debug!(
                        "Received stream event: chunk={:?}, event_id={:?}",
                        event.chunk,
                        event.event_id
                    );
                })
            })
            .boxed()
    }

    /// Deletes an interaction by its ID.
    ///
    /// `interaction_id` is the bare ID (the form [`InteractionResponse::id`](crate::InteractionResponse)
    /// returns) — not an `interactions/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// Removes the interaction from the server, freeing up storage and making it
    /// unavailable for future reference via `previous_interaction_id`.
    ///
    /// # Arguments
    ///
    /// * `interaction_id` - The unique identifier of the interaction to delete.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails
    /// - The API returns an error
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn delete_interaction(&self, interaction_id: &str) -> Result<(), GenaiError> {
        tracing::debug!("Deleting interaction: ID={interaction_id}");

        crate::http::interactions::delete_interaction(&self.http, interaction_id).await?;

        tracing::debug!("Interaction deleted successfully");

        Ok(())
    }

    /// Cancels an in-progress background interaction.
    ///
    /// `interaction_id` is the bare ID (the form [`InteractionResponse::id`](crate::InteractionResponse)
    /// returns) — not an `interactions/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
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
    /// # Arguments
    ///
    /// * `interaction_id` - The unique identifier of the interaction to cancel.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The interaction doesn't exist
    /// - The interaction is not in a cancellable state (not background or already complete)
    /// - The HTTP request fails
    /// - The API returns an error
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
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
    ///     .with_agent("deep-research-pro-preview-12-2025")
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
    ///     let cancelled = client.cancel_interaction(interaction_id).await?;
    ///     assert_eq!(cancelled.status, InteractionStatus::Cancelled);
    ///     println!("Interaction cancelled");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn cancel_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Cancelling interaction: ID={interaction_id}");

        let response =
            crate::http::interactions::cancel_interaction(&self.http, interaction_id).await?;

        log_response_body(&response);
        tracing::debug!("Interaction cancelled: status={:?}", response.status);

        Ok(response)
    }

    // --- Webhooks resource methods (`/v1beta/webhooks`) ---

    /// Registers a new webhook.
    ///
    /// The returned webhook includes `new_signing_secret` — only populated on
    /// create — which is used to verify event payload signatures. Store it
    /// securely; it is not returned again.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the API returns an error,
    /// or response parsing fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Webhook, WebhookEvent};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let webhook = client.create_webhook(
    ///     &Webhook::new(
    ///         "https://example.com/hooks/genai",
    ///         vec![WebhookEvent::InteractionCompleted, WebhookEvent::InteractionFailed],
    ///     )
    ///     .with_name("my-hook"),
    /// ).await?;
    ///
    /// println!("Created {:?}; secret: {:?}", webhook.id, webhook.new_signing_secret);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_webhook(
        &self,
        webhook: &crate::Webhook,
    ) -> Result<crate::Webhook, GenaiError> {
        crate::http::webhooks::create_webhook(&self.http, webhook).await
    }

    /// Retrieves a registered webhook by ID.
    ///
    /// `webhook_id` is the bare ID (the form [`Webhook::id`](crate::Webhook)
    /// returns) — not a `webhooks/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error if the webhook doesn't exist, the HTTP request fails,
    /// or response parsing fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn get_webhook(&self, webhook_id: &str) -> Result<crate::Webhook, GenaiError> {
        crate::http::webhooks::get_webhook(&self.http, webhook_id).await
    }

    /// Lists registered webhooks.
    ///
    /// # Arguments
    ///
    /// * `page_size` - Optional maximum number of webhooks per page.
    /// * `page_token` - Optional token from a previous list call.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn list_webhooks(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::WebhookListResponse, GenaiError> {
        crate::http::webhooks::list_webhooks(&self.http, page_size, page_token).await
    }

    /// Updates a registered webhook.
    ///
    /// `webhook_id` is the bare ID (the form [`Webhook::id`](crate::Webhook)
    /// returns) — not a `webhooks/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Arguments
    ///
    /// * `webhook_id` - The webhook to update.
    /// * `update` - The fields to change (only set fields are sent).
    /// * `update_mask` - Optional comma-separated list of fields to update
    ///   (e.g. `"uri,subscribed_events"`).
    ///
    /// Live behavior note (2026-07): `update_mask` is not required — PATCH
    /// applies exactly the fields present in the body. The mask was also
    /// observed to be ignored when supplied (fields outside the mask still
    /// updated), so rely on the partial body, not the mask, to scope updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the webhook doesn't exist, the HTTP request fails,
    /// or response parsing fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, WebhookState, WebhookUpdate};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// // Temporarily disable a webhook
    /// let updated = client.update_webhook(
    ///     "wh-123",
    ///     &WebhookUpdate::new().with_state(WebhookState::Disabled),
    ///     Some("state"),
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update_webhook(
        &self,
        webhook_id: &str,
        update: &crate::WebhookUpdate,
        update_mask: Option<&str>,
    ) -> Result<crate::Webhook, GenaiError> {
        crate::http::webhooks::update_webhook(&self.http, webhook_id, update, update_mask).await
    }

    /// Deletes a registered webhook.
    ///
    /// `webhook_id` is the bare ID (the form [`Webhook::id`](crate::Webhook)
    /// returns) — not a `webhooks/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error if the webhook doesn't exist or the HTTP request fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn delete_webhook(&self, webhook_id: &str) -> Result<(), GenaiError> {
        crate::http::webhooks::delete_webhook(&self.http, webhook_id).await
    }

    /// Sends a test event to a webhook (`:ping`).
    ///
    /// `webhook_id` is the bare ID (the form [`Webhook::id`](crate::Webhook)
    /// returns) — not a `webhooks/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// Use this to verify your endpoint receives and validates deliveries
    /// before relying on it for real events.
    ///
    /// Live behavior note (2026-07): the RPC accepts an empty JSON body
    /// (`{}`, which this client sends) and returns `{}` on success even
    /// when the destination URI is unreachable.
    ///
    /// # Errors
    ///
    /// Returns an error if the webhook doesn't exist or the HTTP request fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn ping_webhook(&self, webhook_id: &str) -> Result<(), GenaiError> {
        crate::http::webhooks::ping_webhook(&self.http, webhook_id).await
    }

    /// Rotates a webhook's signing secret (`:rotateSigningSecret`).
    ///
    /// `webhook_id` is the bare ID (the form [`Webhook::id`](crate::Webhook)
    /// returns) — not a `webhooks/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// Returns the newly generated secret. Pass a
    /// [`RevocationBehavior`](crate::RevocationBehavior) to control whether
    /// previous secrets stay valid for 24 hours (safe rollover) or are
    /// revoked immediately; `None` uses the API default.
    ///
    /// # Errors
    ///
    /// Returns an error if the webhook doesn't exist, the HTTP request fails,
    /// or response parsing fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn rotate_webhook_signing_secret(
        &self,
        webhook_id: &str,
        revocation_behavior: Option<crate::RevocationBehavior>,
    ) -> Result<crate::RotateSigningSecretResponse, GenaiError> {
        crate::http::webhooks::rotate_signing_secret(&self.http, webhook_id, revocation_behavior)
            .await
    }

    // --- Triggers resource methods (`/v1beta/triggers`) ---

    /// Creates a server-side scheduled trigger.
    ///
    /// The trigger's `interaction` must target a custom `agent` (see
    /// [`crate::triggers`] for the live-verified constraints); trigger
    /// creation is gated with custom-agent creation on standard API keys.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the API rejects the
    /// trigger definition.
    pub async fn create_trigger(
        &self,
        params: &crate::TriggerCreateParams,
    ) -> Result<crate::Trigger, GenaiError> {
        crate::http::triggers::create_trigger(&self.http, params).await
    }

    /// Retrieves a trigger by ID.
    ///
    /// `trigger_id` is the bare ID (the form [`Trigger::id`](crate::Trigger)
    /// returns) — not a `triggers/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn get_trigger(&self, trigger_id: &str) -> Result<crate::Trigger, GenaiError> {
        crate::http::triggers::get_trigger(&self.http, trigger_id).await
    }

    /// Lists triggers, paged.
    ///
    /// # Arguments
    ///
    /// * `page_size` - Optional maximum number of triggers per page.
    /// * `page_token` - Optional token from a previous list call.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or an invalid page token.
    pub async fn list_triggers(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::TriggerListResponse, GenaiError> {
        crate::http::triggers::list_triggers(&self.http, page_size, page_token).await
    }

    /// Updates a trigger (display name and/or status; `paused` pauses it,
    /// `active` resumes it).
    ///
    /// # Arguments
    ///
    /// * `trigger_id` - The trigger to update.
    /// * `update` - The fields to change (only set fields are sent; there
    ///   is no `update_mask` on this endpoint — see [`crate::TriggerUpdate`]).
    ///
    /// `trigger_id` is the bare ID (the form [`Trigger::id`](crate::Trigger)
    /// returns) — not a `triggers/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn update_trigger(
        &self,
        trigger_id: &str,
        update: &crate::TriggerUpdate,
    ) -> Result<crate::Trigger, GenaiError> {
        crate::http::triggers::update_trigger(&self.http, trigger_id, update).await
    }

    /// Deletes a trigger.
    ///
    /// `trigger_id` is the bare ID (the form [`Trigger::id`](crate::Trigger)
    /// returns) — not a `triggers/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn delete_trigger(&self, trigger_id: &str) -> Result<(), GenaiError> {
        crate::http::triggers::delete_trigger(&self.http, trigger_id).await
    }

    /// Fires a trigger immediately, outside its schedule.
    ///
    /// `trigger_id` is the bare ID (the form [`Trigger::id`](crate::Trigger)
    /// returns) — not a `triggers/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// **Unverified endpoint shape**: this posts to the `executions`
    /// sub-collection (not a `:run` colon verb), a path derived from the
    /// google-genai generated bindings rather than observed live — it
    /// needs an existing trigger, and trigger creation is agent-gated
    /// (see [`triggers`](crate::triggers)). The same caveat applies to
    /// [`list_trigger_executions`](Self::list_trigger_executions).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn run_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<crate::TriggerExecution, GenaiError> {
        crate::http::triggers::run_trigger(&self.http, trigger_id).await
    }

    /// Lists a trigger's past executions, paged.
    ///
    /// # Arguments
    ///
    /// * `trigger_id` - The trigger whose executions to list.
    /// * `page_size` - Optional maximum number of executions per page.
    /// * `page_token` - Optional token from a previous list call.
    ///
    /// `trigger_id` is the bare ID (the form [`Trigger::id`](crate::Trigger)
    /// returns) — not a `triggers/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// **Unverified endpoint shape**: reads the same `executions`
    /// sub-collection [`run_trigger`](Self::run_trigger) posts to, with
    /// the same caveat — the path comes from the google-genai generated
    /// bindings, not live observation, because it needs an existing
    /// trigger and trigger creation is agent-gated.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the trigger doesn't exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn list_trigger_executions(
        &self,
        trigger_id: &str,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::TriggerExecutionListResponse, GenaiError> {
        crate::http::triggers::list_trigger_executions(
            &self.http, trigger_id, page_size, page_token,
        )
        .await
    }

    // --- Environments resource methods (`/v1beta/environments`) ---

    /// Creates an environment explicitly, for reuse across interactions.
    ///
    /// Requests can also create environments implicitly by passing a typed
    /// [`EnvironmentSpec`](crate::EnvironmentSpec) inline; explicit creation
    /// returns the ID so many interactions can share one container.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the definition is
    /// rejected.
    pub async fn create_environment(
        &self,
        request: &crate::CreateEnvironmentRequest,
    ) -> Result<crate::Environment, GenaiError> {
        crate::http::environments::create_environment(&self.http, request).await
    }

    /// Retrieves an environment by ID.
    ///
    /// `environment_id` is the bare ID (the form
    /// [`Environment::id`](crate::Environment) returns, live-verified) — not
    /// an `environments/...` resource name, which would be percent-encoded
    /// into a single path segment and 404. The one unobserved source is
    /// [`environment_id`](crate::InteractionResponse::environment_id) on a
    /// response: strip a leading `environments/` prefix before passing it
    /// here.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the environment doesn't
    /// exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn get_environment(
        &self,
        environment_id: &str,
    ) -> Result<crate::Environment, GenaiError> {
        crate::http::environments::get_environment(&self.http, environment_id).await
    }

    /// Lists environments, paged.
    ///
    /// # Arguments
    ///
    /// * `page_size` - Optional maximum number of environments per page.
    /// * `page_token` - Optional token from a previous list call.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or an invalid page token.
    pub async fn list_environments(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::EnvironmentListResponse, GenaiError> {
        crate::http::environments::list_environments(&self.http, page_size, page_token).await
    }

    /// Deletes an environment.
    ///
    /// `environment_id` is the bare ID (the form
    /// [`Environment::id`](crate::Environment) returns, live-verified) — not
    /// an `environments/...` resource name, which would be percent-encoded
    /// into a single path segment and 404. The one unobserved source is
    /// [`environment_id`](crate::InteractionResponse::environment_id) on a
    /// response: strip a leading `environments/` prefix before passing it
    /// here.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or when the environment doesn't
    /// exist.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn delete_environment(&self, environment_id: &str) -> Result<(), GenaiError> {
        crate::http::environments::delete_environment(&self.http, environment_id).await
    }

    // --- Agents resource methods (`/v1beta/agents`) ---

    /// Creates a custom agent.
    ///
    /// Once created, run the agent with
    /// [`InteractionBuilder::with_agent()`](crate::InteractionBuilder::with_agent)
    /// using its ID.
    ///
    /// Live behavior notes (2026-07):
    /// - Agent creation was rejected with a generic
    ///   `400 "Request contains an invalid argument."` for every payload
    ///   tried on a standard Gemini API key (even schema-valid ones), which
    ///   suggests the resource is allowlisted/gated. Field names are still
    ///   validated first (snake_case: `id`, `base_agent`,
    ///   `system_instruction`, `description`, `tools`, `base_environment`).
    /// - `tools` on an agent only accepts `code_execution`, `google_search`,
    ///   and `url_context` (per the API's own validation error).
    /// - Managed agent IDs (e.g. `deep-research-preview-04-2026`) are not
    ///   retrievable through `GET /v1beta/agents/{id}` (404).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, the API returns an error,
    /// or response parsing fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Agent, Client, Tool};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let agent = client.create_agent(
    ///     &Agent::new("customer-sentinel")
    ///         .with_system_instruction("You monitor customer feedback.")
    ///         .add_tool(Tool::CodeExecution),
    /// ).await?;
    ///
    /// // Run it
    /// let response = client.interaction()
    ///     .with_agent(agent.id.as_deref().unwrap_or("customer-sentinel"))
    ///     .with_text("Summarize this week's feedback")
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_agent(&self, agent: &crate::Agent) -> Result<crate::Agent, GenaiError> {
        crate::http::agents::create_agent(&self.http, agent).await
    }

    /// Retrieves an agent by ID.
    ///
    /// `agent_id` is the bare ID (the form [`Agent::id`](crate::Agent)
    /// returns) — not an `agents/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent doesn't exist, the HTTP request fails,
    /// or response parsing fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn get_agent(&self, agent_id: &str) -> Result<crate::Agent, GenaiError> {
        crate::http::agents::get_agent(&self.http, agent_id).await
    }

    /// Lists agents.
    ///
    /// # Arguments
    ///
    /// * `page_size` - Optional maximum number of agents per page.
    /// * `page_token` - Optional token from a previous list call.
    /// * `parent` - Optional parent resource filter.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn list_agents(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
        parent: Option<&str>,
    ) -> Result<crate::AgentListResponse, GenaiError> {
        crate::http::agents::list_agents(&self.http, page_size, page_token, parent).await
    }

    /// Deletes an agent by ID.
    ///
    /// `agent_id` is the bare ID (the form [`Agent::id`](crate::Agent)
    /// returns) — not an `agents/...` resource name, which would be
    /// percent-encoded into a single path segment and 404.
    ///
    /// # Errors
    ///
    /// Returns an error if the agent doesn't exist or the HTTP request fails.
    ///
    /// An empty or dot-segment ID is rejected locally as
    /// [`GenaiError::InvalidInput`]
    /// before any request is sent.
    pub async fn delete_agent(&self, agent_id: &str) -> Result<(), GenaiError> {
        crate::http::agents::delete_agent(&self.http, agent_id).await
    }

    // --- Files API methods ---

    /// Uploads a file from a path to the Files API.
    ///
    /// Files are stored for 48 hours and can be referenced in interactions by their URI.
    /// This is more efficient than inline base64 encoding for large files or files
    /// that will be used across multiple interactions.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The MIME type cannot be determined
    /// - The upload fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Content};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload a video file
    /// let file = client.upload_file("video.mp4").await?;
    /// println!("Uploaded: {} -> {}", file.name, file.uri);
    ///
    /// // Use in interaction
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_content(vec![
    ///         Content::text("Describe this video"),
    ///         Content::from_file(&file),
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<crate::FileMetadata, GenaiError> {
        let path = path.as_ref();

        // Read file contents
        let file_data = tokio::fs::read(path).await.map_err(|e| {
            tracing::warn!("Failed to read file '{}': {}", path.display(), e);
            GenaiError::InvalidInput(format!("Failed to read file '{}': {}", path.display(), e))
        })?;

        // Detect MIME type from extension
        let mime_type = crate::multimodal::detect_mime_type(path).ok_or_else(|| {
            tracing::warn!(
                "Could not determine MIME type for '{}' - unknown extension",
                path.display()
            );
            GenaiError::InvalidInput(format!(
                "Could not determine MIME type for '{}'. Please use upload_file_with_mime() to specify explicitly.",
                path.display()
            ))
        })?;

        // Use filename as display name
        let display_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        tracing::debug!(
            "Uploading file: path={}, size={} bytes, mime_type={}",
            path.display(),
            file_data.len(),
            mime_type
        );

        crate::http::files::upload_file(&self.http, file_data, mime_type, display_name.as_deref())
            .await
    }

    /// Uploads a file with an explicit MIME type.
    ///
    /// Use this when automatic MIME type detection isn't suitable.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    /// * `mime_type` - MIME type of the file (e.g., "video/mp4")
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.upload_file_with_mime("data.bin", "application/octet-stream").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_with_mime(
        &self,
        path: impl AsRef<std::path::Path>,
        mime_type: &str,
    ) -> Result<crate::FileMetadata, GenaiError> {
        let path = path.as_ref();

        let file_data = tokio::fs::read(path).await.map_err(|e| {
            tracing::warn!("Failed to read file '{}': {}", path.display(), e);
            GenaiError::InvalidInput(format!("Failed to read file '{}': {}", path.display(), e))
        })?;

        let display_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        tracing::debug!(
            "Uploading file: path={}, size={} bytes, mime_type={}",
            path.display(),
            file_data.len(),
            mime_type
        );

        crate::http::files::upload_file(&self.http, file_data, mime_type, display_name.as_deref())
            .await
    }

    /// Uploads file bytes directly with a specified MIME type.
    ///
    /// Use this when you already have file contents in memory.
    ///
    /// # Arguments
    ///
    /// * `data` - File contents as bytes
    /// * `mime_type` - MIME type of the file
    /// * `display_name` - Optional display name for the file
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload bytes from memory
    /// let video_bytes = std::fs::read("video.mp4")?;
    /// let file = client.upload_file_bytes(video_bytes, "video/mp4", Some("my-video")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_bytes(
        &self,
        data: Vec<u8>,
        mime_type: &str,
        display_name: Option<&str>,
    ) -> Result<crate::FileMetadata, GenaiError> {
        tracing::debug!(
            "Uploading file bytes: size={} bytes, mime_type={}, display_name={:?}",
            data.len(),
            mime_type,
            display_name
        );

        crate::http::files::upload_file(&self.http, data, mime_type, display_name).await
    }

    /// Gets metadata for an uploaded file.
    ///
    /// Use this to check the processing status of a recently uploaded file.
    ///
    /// # Arguments
    ///
    /// * `file_name` - The full resource name of the file (e.g.,
    ///   "files/abc123" — the form [`FileMetadata::name`](crate::FileMetadata)
    ///   returns). Anything else — a bare ID, extra path segments — is
    ///   rejected locally as [`GenaiError::InvalidInput`] before a request
    ///   is sent.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>`, and an API or network error if the request fails or
    /// the file doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.get_file("files/abc123").await?;
    /// if file.is_active() {
    ///     println!("File is ready to use");
    /// } else if file.is_processing() {
    ///     println!("File is still processing...");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_file(&self, file_name: &str) -> Result<crate::FileMetadata, GenaiError> {
        crate::http::files::get_file(&self.http, file_name).await
    }

    /// Lists all uploaded files.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client.list_files(None, None).await?;
    /// for file in response.files {
    ///     println!("{}: {} ({})", file.name, file.display_name.as_deref().unwrap_or(""), file.mime_type);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_files(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::ListFilesResponse, GenaiError> {
        crate::http::files::list_files(&self.http, page_size, page_token).await
    }

    /// Deletes an uploaded file.
    ///
    /// # Arguments
    ///
    /// * `file_name` - The full resource name of the file to delete (e.g.,
    ///   "files/abc123" — the form [`FileMetadata::name`](crate::FileMetadata)
    ///   returns). Anything else — a bare ID, extra path segments — is
    ///   rejected locally as [`GenaiError::InvalidInput`] before a request
    ///   is sent.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a name that is not
    /// `files/<id>`, and an API or network error if the request fails or
    /// the file doesn't exist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload, use, then delete
    /// let file = client.upload_file("video.mp4").await?;
    /// // ... use in interactions ...
    /// client.delete_file(&file.name).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn delete_file(&self, file_name: &str) -> Result<(), GenaiError> {
        crate::http::files::delete_file(&self.http, file_name).await
    }

    /// Uploads a file using chunked transfer to minimize memory usage.
    ///
    /// Unlike `upload_file`, this method streams the file from disk in chunks,
    /// never loading the entire file into memory. This is ideal for large files
    /// (500MB-2GB) or memory-constrained environments.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    ///
    /// # Returns
    ///
    /// Returns a tuple of:
    /// - `FileMetadata`: The uploaded file's metadata
    /// - `ResumableUpload`: A handle that can be used to resume if the upload is interrupted
    ///
    /// # Memory Usage
    ///
    /// This method uses approximately 8MB of memory for buffering, regardless of
    /// the file size. A 2GB file uses the same memory as a 10MB file.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The MIME type cannot be determined
    /// - The upload fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Content};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Upload a large video file without loading it all into memory
    /// let (file, _upload_handle) = client.upload_file_chunked("large_video.mp4").await?;
    /// println!("Uploaded: {} -> {}", file.name, file.uri);
    ///
    /// // Use in interaction
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_content(vec![
    ///         Content::text("Describe this video"),
    ///         Content::from_file(&file),
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_chunked(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(crate::FileMetadata, crate::ResumableUpload), GenaiError> {
        let path = path.as_ref();

        // Detect MIME type from extension
        let mime_type = crate::multimodal::detect_mime_type(path).ok_or_else(|| {
            tracing::warn!(
                "Could not determine MIME type for '{}' - unknown extension",
                path.display()
            );
            GenaiError::InvalidInput(format!(
                "Could not determine MIME type for '{}'. Please use upload_file_chunked_with_mime() to specify explicitly.",
                path.display()
            ))
        })?;

        // Use filename as display name
        let display_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        tracing::debug!(
            "Chunked upload: path={}, mime_type={}",
            path.display(),
            mime_type
        );

        crate::http::files::upload_file_chunked(
            &self.http,
            path,
            mime_type,
            display_name.as_deref(),
        )
        .await
    }

    /// Uploads a file using chunked transfer with an explicit MIME type.
    ///
    /// Use this when automatic MIME type detection isn't suitable.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    /// * `mime_type` - MIME type of the file (e.g., "video/mp4")
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let (file, _) = client.upload_file_chunked_with_mime(
    ///     "data.bin",
    ///     "application/octet-stream"
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_chunked_with_mime(
        &self,
        path: impl AsRef<std::path::Path>,
        mime_type: &str,
    ) -> Result<(crate::FileMetadata, crate::ResumableUpload), GenaiError> {
        let path = path.as_ref();

        let display_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        tracing::debug!(
            "Chunked upload: path={}, mime_type={}",
            path.display(),
            mime_type
        );

        crate::http::files::upload_file_chunked(
            &self.http,
            path,
            mime_type,
            display_name.as_deref(),
        )
        .await
    }

    /// Uploads a file using chunked transfer with a custom chunk size.
    ///
    /// This is the same as `upload_file_chunked_with_mime` but allows
    /// specifying the chunk size for streaming. Larger chunks are more
    /// efficient for fast networks, while smaller chunks use less memory.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    /// * `mime_type` - MIME type of the file
    /// * `chunk_size` - Size of chunks to stream in bytes (default: 8MB)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Use 16MB chunks for faster upload on a fast network
    /// let chunk_size = 16 * 1024 * 1024;
    /// let (file, _) = client.upload_file_chunked_with_options(
    ///     "large_video.mp4",
    ///     "video/mp4",
    ///     chunk_size
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_file_chunked_with_options(
        &self,
        path: impl AsRef<std::path::Path>,
        mime_type: &str,
        chunk_size: usize,
    ) -> Result<(crate::FileMetadata, crate::ResumableUpload), GenaiError> {
        let path = path.as_ref();

        let display_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        tracing::debug!(
            "Chunked upload: path={}, mime_type={}, chunk_size={}",
            path.display(),
            mime_type,
            chunk_size
        );

        crate::http::files::upload_file_chunked_with_chunk_size(
            &self.http,
            path,
            mime_type,
            display_name.as_deref(),
            chunk_size,
        )
        .await
    }

    /// Waits for a file to finish processing.
    ///
    /// Some files (especially videos) require processing before they can be used.
    /// This method polls the file status until it becomes active or fails.
    ///
    /// # Arguments
    ///
    /// * `file` - The file metadata to wait for
    /// * `poll_interval` - How often to check the status
    /// * `timeout` - Maximum time to wait
    ///
    /// # Returns
    ///
    /// Returns the updated file metadata when processing completes.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file processing fails
    /// - The timeout is exceeded
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let file = client.upload_file("large_video.mp4").await?;
    ///
    /// // Wait for processing to complete
    /// let ready_file = client.wait_for_file_ready(
    ///     &file,
    ///     Duration::from_secs(2),
    ///     Duration::from_secs(120)
    /// ).await?;
    ///
    /// println!("File ready: {}", ready_file.uri);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn wait_for_file_ready(
        &self,
        file: &crate::FileMetadata,
        poll_interval: std::time::Duration,
        timeout: std::time::Duration,
    ) -> Result<crate::FileMetadata, GenaiError> {
        use std::time::Instant;

        let start = Instant::now();

        loop {
            let current = self.get_file(&file.name).await?;

            if current.is_active() {
                return Ok(current);
            }

            if current.is_failed() {
                let error_code = current.error.as_ref().and_then(|e| e.code);
                let error_msg = current
                    .error
                    .as_ref()
                    .and_then(|e| e.message.as_deref())
                    .unwrap_or("File processing failed without details");

                tracing::error!(
                    "File '{}' processing failed: code={:?}, message={}",
                    file.name,
                    error_code,
                    error_msg
                );

                // Use Api error since this is a server-side processing failure
                return Err(GenaiError::Api {
                    status_code: error_code.map_or(500, |c| c as u16),
                    message: format!("File processing failed: {}", error_msg),
                    request_id: None,
                    retry_after: None,
                });
            }

            // Log unknown states per Evergreen logging strategy
            if let Some(state) = &current.state
                && state.is_unknown()
            {
                tracing::warn!(
                    "File '{}' is in unknown state {:?}, continuing to poll. \
                     This may indicate API evolution - consider updating genai-rs.",
                    file.name,
                    state
                );
            }

            if start.elapsed() > timeout {
                // Use Internal error since this is an operational issue, not invalid input
                let state_info = current
                    .state
                    .as_ref()
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_else(|| "unknown".to_string());
                return Err(GenaiError::Internal(format!(
                    "Timeout waiting for file '{}' to be ready (waited {:?}, last state: {}). \
                     The file may still be processing - try again with a longer timeout.",
                    file.name,
                    start.elapsed(),
                    state_info
                )));
            }

            tracing::debug!(
                "File '{}' still processing, waiting {:?}...",
                file.name,
                poll_interval
            );
            tokio::time::sleep(poll_interval).await;
        }
    }

    // =========================================================================
    // File Search Stores (/v1beta/fileSearchStores)
    // =========================================================================

    /// Creates a file search store.
    ///
    /// The returned [`FileSearchStore::name`](crate::FileSearchStore::name) is
    /// what [`Tool::FileSearch`](crate::Tool::FileSearch) takes in
    /// `store_names`.
    ///
    /// `display_name` is a human-readable label; the API derives the resource
    /// name from it by stripping non-alphanumeric characters and appending a
    /// unique suffix, so the two are related but not interchangeable.
    ///
    /// # Errors
    ///
    /// Returns an API or network error if the request fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let store = client.create_file_search_store(Some("my-docs")).await?;
    /// println!("created {}", store.name);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create_file_search_store(
        &self,
        display_name: Option<&str>,
    ) -> Result<crate::FileSearchStore, GenaiError> {
        // Through the builders rather than a struct literal, so the two
        // construction paths cannot drift and the builders have an in-crate
        // caller.
        let mut request = crate::CreateFileSearchStoreRequest::new();
        if let Some(name) = display_name {
            request = request.with_display_name(name);
        }
        crate::http::file_search_stores::create_file_search_store(&self.http, &request).await
    }

    /// Creates a file search store from an explicit request body.
    ///
    /// Use this over [`create_file_search_store`](Self::create_file_search_store)
    /// to set fields the crate does not model yet, via
    /// [`CreateFileSearchStoreRequest::extra`](crate::CreateFileSearchStoreRequest::extra).
    ///
    /// # Errors
    ///
    /// Returns an API or network error if the request fails.
    pub async fn create_file_search_store_with_request(
        &self,
        request: &crate::CreateFileSearchStoreRequest,
    ) -> Result<crate::FileSearchStore, GenaiError> {
        crate::http::file_search_stores::create_file_search_store(&self.http, request).await
    }

    /// Retrieves a file search store by resource name.
    ///
    /// # Arguments
    ///
    /// * `store_name` - Full resource name (e.g. `fileSearchStores/abc123`).
    ///   A bare ID is rejected locally as [`GenaiError::InvalidInput`].
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn get_file_search_store(
        &self,
        store_name: &str,
    ) -> Result<crate::FileSearchStore, GenaiError> {
        crate::http::file_search_stores::get_file_search_store(&self.http, store_name).await
    }

    /// Lists file search stores.
    ///
    /// # Errors
    ///
    /// Returns an API or network error if the request fails.
    pub async fn list_file_search_stores(
        &self,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::FileSearchStoreListResponse, GenaiError> {
        crate::http::file_search_stores::list_file_search_stores(&self.http, page_size, page_token)
            .await
    }

    /// Deletes a file search store.
    ///
    /// # Arguments
    ///
    /// * `store_name` - Full resource name (e.g. `fileSearchStores/abc123`).
    /// * `force` - Delete even when the store still holds documents. Without
    ///   it, a non-empty store is rejected with `FAILED_PRECONDITION`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn delete_file_search_store(
        &self,
        store_name: &str,
        force: bool,
    ) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_file_search_store(&self.http, store_name, force)
            .await
    }

    /// Uploads a local file into a file search store.
    ///
    /// MIME type is inferred from the file extension, matching
    /// [`upload_file`](Self::upload_file). Use
    /// [`upload_to_file_search_store_with_mime`](Self::upload_to_file_search_store_with_mime)
    /// to set it explicitly.
    ///
    /// The document is indexed asynchronously and starts in
    /// [`DocumentState::Pending`](crate::DocumentState::Pending); file search
    /// will not match it until it reaches
    /// [`Active`](crate::DocumentState::Active). See
    /// [`wait_for_document_active`](Self::wait_for_document_active).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name or an
    /// unreadable file, and an API or network error if the request fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    /// let store = client.create_file_search_store(Some("my-docs")).await?;
    ///
    /// let doc = client
    ///     .upload_to_file_search_store(&store.name, "handbook.pdf", Some("handbook"))
    ///     .await?;
    /// client.wait_for_document_active(&doc.name, None, None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload_to_file_search_store(
        &self,
        store_name: &str,
        file_path: impl AsRef<std::path::Path>,
        display_name: Option<&str>,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        let path = file_path.as_ref();
        let mime_type = crate::multimodal::detect_mime_type(path).ok_or_else(|| {
            GenaiError::InvalidInput(format!(
                "Could not determine MIME type for '{}'. Please use \
                 upload_to_file_search_store_with_mime() to specify explicitly.",
                path.display()
            ))
        })?;
        crate::http::file_search_stores::upload_to_file_search_store(
            &self.http,
            store_name,
            path,
            display_name,
            mime_type,
        )
        .await
    }

    /// Uploads a local file into a store with an explicit MIME type.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, an
    /// unreadable file, or an invalid MIME type, and an API or network error
    /// if the request fails.
    pub async fn upload_to_file_search_store_with_mime(
        &self,
        store_name: &str,
        file_path: impl AsRef<std::path::Path>,
        display_name: Option<&str>,
        mime_type: &str,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        crate::http::file_search_stores::upload_to_file_search_store(
            &self.http,
            store_name,
            file_path.as_ref(),
            display_name,
            mime_type,
        )
        .await
    }

    /// Lists documents in a file search store.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, and
    /// an API or network error if the request fails.
    pub async fn list_file_search_documents(
        &self,
        store_name: &str,
        page_size: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<crate::DocumentListResponse, GenaiError> {
        crate::http::file_search_stores::list_documents(
            &self.http, store_name, page_size, page_token,
        )
        .await
    }

    /// Retrieves a document from a file search store.
    ///
    /// # Arguments
    ///
    /// * `document_name` - Full resource name (e.g.
    ///   `fileSearchStores/abc/documents/doc1`).
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn get_file_search_document(
        &self,
        document_name: &str,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        crate::http::file_search_stores::get_document(&self.http, document_name).await
    }

    /// Deletes a document from a file search store.
    ///
    /// # Arguments
    ///
    /// * `document_name` - Full resource name.
    /// * `force` - Required for a document that has been chunked, which is
    ///   every successfully indexed one. Without it the API responds
    ///   `400 Cannot delete non-empty Document`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed name, and an API
    /// or network error if the request fails.
    pub async fn delete_file_search_document(
        &self,
        document_name: &str,
        force: bool,
    ) -> Result<(), GenaiError> {
        crate::http::file_search_stores::delete_document(&self.http, document_name, force).await
    }

    /// Polls a document until it is indexed and queryable.
    ///
    /// Uploading is not enough: file search silently returns no matches for a
    /// document still in [`Pending`](crate::DocumentState::Pending), so
    /// anything that uploads and immediately queries needs this in between.
    /// Indexing is typically fast (observed ~1-2s for a small text file), but
    /// it is not synchronous.
    ///
    /// Unknown states are polled through rather than treated as terminal, per
    /// the Evergreen principle — the `timeout` is what bounds the wait.
    ///
    /// # Arguments
    ///
    /// * `document_name` - Full resource name.
    /// * `timeout` - Maximum time to wait; defaults to 60s when `None`.
    /// * `poll_interval` - Delay between polls; defaults to 500ms when `None`.
    ///   Worth raising when uploading many documents at once, since the
    ///   default issues a GET every half second per document.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::Internal`] if the document reaches
    /// [`Failed`](crate::DocumentState::Failed) or if the wait times out —
    /// neither is retryable, and the two are distinguished by their message —
    /// and [`GenaiError::InvalidInput`] for a malformed name. Errors from the
    /// underlying polling GET propagate as they are.
    pub async fn wait_for_document_active(
        &self,
        document_name: &str,
        timeout: Option<std::time::Duration>,
        poll_interval: Option<std::time::Duration>,
    ) -> Result<crate::FileSearchDocument, GenaiError> {
        use std::time::{Duration, Instant};

        let timeout = timeout.unwrap_or(Duration::from_secs(60));
        let poll_interval = poll_interval.unwrap_or(Duration::from_millis(500));
        let start = Instant::now();

        loop {
            let current = self.get_file_search_document(document_name).await?;

            match &current.state {
                Some(crate::DocumentState::Active) => return Ok(current),
                Some(crate::DocumentState::Failed) => {
                    // `Internal`, not `Api { status_code: 500 }`. `Failed` is
                    // terminal, but `is_retryable()` reports true for any
                    // `Api` with a 5xx — so the 500 spelling tells a caller
                    // following `examples/retry_with_backoff.rs` to keep
                    // re-polling a document that will never index, burning
                    // the whole retry budget and re-issuing the GET loop each
                    // time. The status was invented here rather than observed:
                    // unlike `wait_for_file_ready`, which carries the API's
                    // own `error_code`, `DocumentState::Failed` is a state
                    // value with no HTTP error behind it. The timeout arm
                    // below already uses `Internal` for the same reason.
                    return Err(GenaiError::Internal(format!(
                        "Document '{document_name}' failed to index. This is \
                         terminal — re-uploading is the only recovery."
                    )));
                }
                Some(state) if state.is_unknown() => {
                    tracing::warn!(
                        "Document '{}' is in unknown state {}, continuing to poll. \
                         This may indicate API evolution - consider updating genai-rs.",
                        document_name,
                        state
                    );
                }
                _ => {}
            }

            if start.elapsed() > timeout {
                let state_info = current
                    .state
                    .as_ref()
                    .map_or_else(|| "unknown".to_string(), ToString::to_string);
                return Err(GenaiError::Internal(format!(
                    "Timeout waiting for document '{document_name}' to become active \
                     (waited {:?}, last state: {state_info}). It may still be indexing - \
                     try again with a longer timeout.",
                    start.elapsed()
                )));
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder_default() {
        let client = Client::builder("test_key".to_string()).build().unwrap();
        assert_eq!(client.http.api_key, "test_key");
    }

    #[test]
    fn test_client_builder_with_timeout() {
        let client = Client::builder("test_key".to_string())
            .with_timeout(Duration::from_secs(120))
            .build()
            .unwrap();
        assert_eq!(client.http.api_key, "test_key");
        // Note: We can't easily inspect the reqwest client's timeout,
        // but this test verifies the builder chain works
    }

    #[test]
    fn test_client_builder_with_connect_timeout() {
        let client = Client::builder("test_key".to_string())
            .with_connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        assert_eq!(client.http.api_key, "test_key");
    }

    #[test]
    fn test_client_builder_with_both_timeouts() {
        let client = Client::builder("test_key".to_string())
            .with_timeout(Duration::from_secs(120))
            .with_connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        assert_eq!(client.http.api_key, "test_key");
    }

    #[test]
    fn test_client_new() {
        let client = Client::new("test_key".to_string());
        assert_eq!(client.http.api_key, "test_key");
    }

    #[test]
    fn test_client_debug_redacts_api_key() {
        let client = Client::new("super_secret_api_key_12345".to_string());
        let debug_output = format!("{:?}", client);

        // API key should NOT appear in debug output
        assert!(
            !debug_output.contains("super_secret_api_key_12345"),
            "API key was exposed in debug output: {}",
            debug_output
        );
        // Should show [REDACTED] instead
        assert!(
            debug_output.contains("[REDACTED]"),
            "Debug output should contain [REDACTED]: {}",
            debug_output
        );
    }

    #[test]
    fn test_client_builder_returns_result() {
        let result = Client::builder("test_key".to_string()).build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_add_wire_inspector_accumulates() {
        struct Noop;
        impl WireInspector for Noop {
            fn on_event(&self, _event: &crate::wire::WireEvent) {}
        }

        // Held for the build: an unrelated LOUD_WIRE=1 window would add a
        // third inspector to this client. The guard is shared with the
        // other LOUD_WIRE mutator in `src/wire.rs`.
        //
        // `unset()` because the guard blocks *concurrent* mutators but does
        // not neutralize an *ambient* one: under `LOUD_WIRE=1 cargo test`,
        // `build()` would append a printer on top of the two Noops and this
        // would see 3. Previously that depended on whether the sibling
        // test's unconditional `remove_var` had already landed; now that the
        // sibling restores instead of clearing, it would be deterministic.
        let mut guard = crate::test_subscriber::LoudWireGuard::acquire();
        guard.unset();

        let client = Client::builder("test_key".to_string())
            .add_wire_inspector(Arc::new(Noop))
            .add_wire_inspector(Arc::new(Noop))
            .build()
            .unwrap();

        assert_eq!(
            client.http.inspectors.len(),
            2,
            "add_wire_inspector should accumulate, not replace"
        );
    }

    #[test]
    fn test_loud_wire_env_installs_printer() {
        // Held across the whole set/build/unset/build sequence so no other
        // test builds a client inside the LOUD_WIRE=1 window, and so the
        // ambient value is restored on drop rather than cleared.
        let mut guard = crate::test_subscriber::LoudWireGuard::acquire();

        guard.set("1");
        let with_env = Client::builder("test_key".to_string()).build().unwrap();
        guard.unset();
        let without_env = Client::builder("test_key".to_string()).build().unwrap();

        assert!(
            with_env.http.has_inspectors(),
            "LOUD_WIRE should install a LoudWirePrinter at construction"
        );
        assert!(
            !without_env.http.has_inspectors(),
            "no inspectors expected without LOUD_WIRE or add_wire_inspector"
        );
    }

    #[test]
    fn test_client_builder_debug_redacts_api_key() {
        let builder = Client::builder("another_secret_key_67890".to_string())
            .with_timeout(Duration::from_secs(60));
        let debug_output = format!("{:?}", builder);

        // API key should NOT appear in debug output
        assert!(
            !debug_output.contains("another_secret_key_67890"),
            "API key was exposed in builder debug output: {}",
            debug_output
        );
        // Should show [REDACTED] instead
        assert!(
            debug_output.contains("[REDACTED]"),
            "Builder debug output should contain [REDACTED]: {}",
            debug_output
        );
    }

    #[tokio::test]
    async fn test_upload_file_unknown_extension_error() {
        let client = Client::new("test_key".to_string());

        // Create a temp file with an unknown extension
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("data.xyz");
        std::fs::write(&file_path, b"test data").unwrap();

        // upload_file should fail with InvalidInput for unknown MIME type
        let result = client.upload_file(&file_path).await;
        assert!(result.is_err(), "Should fail for unknown extension");

        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(
            err_string.contains("Could not determine MIME type"),
            "Error should mention MIME type issue: {}",
            err_string
        );
        assert!(
            err_string.contains("data.xyz"),
            "Error should include filename: {}",
            err_string
        );
    }

    #[tokio::test]
    async fn test_upload_file_nonexistent_file_error() {
        let client = Client::new("test_key".to_string());

        // Try to upload a file that doesn't exist
        let result = client.upload_file("/nonexistent/path/to/file.txt").await;
        assert!(result.is_err(), "Should fail for nonexistent file");

        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(
            err_string.contains("Failed to read file"),
            "Error should mention file read failure: {}",
            err_string
        );
    }

    #[tokio::test]
    async fn test_upload_file_bytes_empty_file_error() {
        let client = Client::new("test_key".to_string());

        // Try to upload empty bytes
        let result = client
            .upload_file_bytes(Vec::new(), "text/plain", Some("empty.txt"))
            .await;
        assert!(result.is_err(), "Should fail for empty file");

        let err = result.unwrap_err();
        let err_string = err.to_string();
        assert!(
            err_string.contains("Cannot upload empty file"),
            "Error should mention empty file: {}",
            err_string
        );
    }

    #[tokio::test]
    async fn test_upload_file_bytes_validates_before_network() {
        // This test verifies that validation happens before any network call
        // by using an invalid API key - if we reach the network, we'd get auth error
        let client = Client::new("invalid_key".to_string());

        // Empty file should fail with validation error, not auth error
        let result = client
            .upload_file_bytes(Vec::new(), "text/plain", None)
            .await;
        assert!(result.is_err());
        let err_string = result.unwrap_err().to_string();
        assert!(
            err_string.contains("Cannot upload empty file"),
            "Should fail validation before hitting network: {}",
            err_string
        );
    }
}
