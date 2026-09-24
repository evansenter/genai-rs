use crate::GenaiError;
use crate::http::context::HttpContext;
use crate::wire::WireInspector;
use reqwest::Client as ReqwestClient;
use std::sync::Arc;
use std::time::Duration;

/// Logs a request or response body at debug level as pretty JSON.
///
/// Checked first: bodies can carry megabytes of base64 media, and
/// `tracing::debug!` only defers formatting, not this serialization.
fn log_body<T: std::fmt::Debug + serde::Serialize>(label: &str, body: &T) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    match serde_json::to_string_pretty(body) {
        Ok(json) => tracing::debug!("{label} Body (JSON):\n{json}"),
        Err(_) => tracing::debug!("{label} Body: {body:#?}"),
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
    base_url: Option<String>,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    wire_inspectors: Vec<Arc<dyn WireInspector>>,
}

// Custom Debug implementation that redacts the API key for security.
impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
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

    /// Sends every request to `base_url` instead of
    /// `https://generativelanguage.googleapis.com`.
    ///
    /// Takes a scheme and host, optionally with a path prefix; the
    /// `/v1beta/...` and `/upload/v1beta/...` paths are appended to it, so a
    /// proxy or a local mock server sees the same paths the real API does.
    /// A trailing slash is ignored.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Client;
    ///
    /// let client = Client::builder("api_key".to_string())
    ///     .with_base_url("http://127.0.0.1:8080")
    ///     .build()?;
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
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

        let mut http = HttpContext::new(
            http_client,
            self.api_key,
            with_env_inspectors(self.wire_inspectors),
        );
        if let Some(base_url) = self.base_url {
            http = http.with_base_url(base_url);
        }
        Ok(Client { http })
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
            base_url: None,
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
    /// let request = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Count to 5")
    ///     .build()?;
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
    #[tracing::instrument(skip_all, fields(model = ?request.model, agent = ?request.agent))]
    pub async fn execute(
        &self,
        request: crate::InteractionRequest,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Creating interaction");
        log_body("Request", &request);

        let response = crate::http::interactions::create_interaction(&self.http, request).await?;

        log_body("Response", &response);
        tracing::debug!("Interaction created: ID={:?}", response.id);

        Ok(response)
    }

    /// Executes a pre-built interaction request with streaming.
    ///
    /// This is the streaming variant of [`execute()`](Self::execute). The
    /// request's `stream` field is set for you (and cleared by `execute()`),
    /// since the endpoint and body must agree.
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
    #[tracing::instrument(skip_all, fields(model = ?request.model, agent = ?request.agent))]
    pub fn execute_stream(
        &self,
        request: crate::InteractionRequest,
    ) -> futures_util::stream::BoxStream<'_, Result<crate::StreamEvent, GenaiError>> {
        use futures_util::StreamExt;

        tracing::debug!("Creating streaming interaction");
        log_body("Request", &request);

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
    /// `interaction_id` is the bare ID ([`InteractionResponse::id`](crate::InteractionResponse)),
    /// not an `interactions/...` resource name.
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
    /// An empty or dot-segment ID fails locally with [`GenaiError::InvalidInput`].
    pub async fn get_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Getting interaction: ID={interaction_id}");

        let response =
            crate::http::interactions::get_interaction(&self.http, interaction_id, false).await?;

        log_body("Response", &response);
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
    /// An empty or dot-segment ID fails locally with [`GenaiError::InvalidInput`].
    pub async fn get_interaction_with_input(
        &self,
        interaction_id: &str,
    ) -> Result<crate::InteractionResponse, GenaiError> {
        tracing::debug!("Getting interaction (with input): ID={interaction_id}");

        let response =
            crate::http::interactions::get_interaction(&self.http, interaction_id, true).await?;

        log_body("Response", &response);
        tracing::debug!("Retrieved interaction: status={:?}", response.status);

        Ok(response)
    }

    /// Retrieves an existing interaction by its ID with streaming.
    ///
    /// `interaction_id` is the bare ID ([`InteractionResponse::id`](crate::InteractionResponse)),
    /// not an `interactions/...` resource name.
    ///
    /// Returns a stream of events for the interaction. This is useful for:
    /// - Resuming an interrupted stream using `last_event_id`
    /// - Streaming a long-running interaction's progress (e.g., deep research)
    ///
    /// Only **background** interactions (`with_background(true)`) can be
    /// streamed this way; for an ordinary one the API answers
    /// `400 Streaming retrieval of interactions is not supported`. Their
    /// events carry an `event_id` to resume from after an interruption; the
    /// resumed stream starts after that event.
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
    /// `interaction_id` is the bare ID ([`InteractionResponse::id`](crate::InteractionResponse)),
    /// not an `interactions/...` resource name.
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
    /// An empty or dot-segment ID fails locally with [`GenaiError::InvalidInput`].
    pub async fn delete_interaction(&self, interaction_id: &str) -> Result<(), GenaiError> {
        tracing::debug!("Deleting interaction: ID={interaction_id}");

        crate::http::interactions::delete_interaction(&self.http, interaction_id).await?;

        tracing::debug!("Interaction deleted successfully");

        Ok(())
    }

    /// Cancels an in-progress background interaction.
    ///
    /// `interaction_id` is the bare ID ([`InteractionResponse::id`](crate::InteractionResponse)),
    /// not an `interactions/...` resource name.
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

        log_body("Response", &response);
        tracing::debug!("Interaction cancelled: status={:?}", response.status);

        Ok(response)
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

        // The guard serializes LOUD_WIRE mutators; `unset()` also clears an
        // ambient `LOUD_WIRE=1`, which would add a third inspector.
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
}
