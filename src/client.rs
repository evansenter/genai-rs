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
pub(crate) fn log_body<T: std::fmt::Debug + serde::Serialize>(label: &str, body: &T) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    match serde_json::to_string_pretty(body) {
        Ok(json) => tracing::debug!("{label} Body (JSON):\n{json}"),
        Err(_) => tracing::debug!("{label} Body: {body:#?}"),
    }
}

/// The main client for interacting with the Google Generative AI API.
///
/// Build one `Client` and reuse it. It holds a connection pool, so calls
/// after the first skip the connection and TLS handshake that a freshly built
/// client pays on every request. `Clone` is cheap and clones share the pool,
/// so give each task a clone rather than a new client.
#[derive(Clone)]
pub struct Client {
    /// Shared HTTP context: reqwest client, API key, wire inspectors, and
    /// the request-id counter for wire-event correlation.
    pub(crate) http: HttpContext,
}

// Resource handles hand out futures and `'static` list streams that carry the
// client (or a clone of it) across tasks, so it must stay `Send + Sync`
// (D-016).
const _: () = {
    const fn assert<T: Send + Sync>() {}
    assert::<Client>();
};

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
