//! [`Client`], the entry point, and [`ClientBuilder`] for its settings.

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
    /// The connect timeout `http`'s reqwest client was built with (`None`:
    /// none), as `ClientBuilder::reqwest_builder` applied it. reqwest's
    /// built client doesn't show it, so `Debug` shows this instead.
    connect_timeout: Option<Duration>,
    /// The TCP keepalive `http`'s reqwest client was built with, as
    /// `ClientBuilder::reqwest_builder` applied it, for `Debug` too.
    tcp_keepalive: TcpKeepalive,
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
            .field("connect_timeout", &self.connect_timeout)
            .field("tcp_keepalive", &self.tcp_keepalive)
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

/// The connect timeout a client gets unless
/// [`ClientBuilder::with_connect_timeout`] sets another: 30 seconds.
///
/// It covers DNS, the TCP handshake and the TLS handshake, which take well
/// under a second to Google's front ends. 30 s rides out four lost SYNs on
/// Linux (retransmits at 1, 3, 7 and 15 s) and a slow mobile or satellite
/// link, while a black-holed route fails in 30 s instead of the OS default
/// (about 75 s on macOS, about 127 s on Linux).
///
/// There is deliberately no default read or total timeout. A non-streaming
/// request's response headers arrive only after generation finishes, so any
/// default short enough to catch a hung connection would cut off a long
/// legitimate generation (thinking, a 4K image, deep context). Bound those
/// with [`ClientBuilder::with_read_timeout`] or
/// [`InteractionBuilder::with_timeout`](crate::InteractionBuilder::with_timeout)
/// once you know your workload; see `docs/RELIABILITY.md`.
///
/// # Tests on a paused tokio clock
///
/// Tokio advances a paused clock to the next timer whenever the runtime is
/// idle, even while a socket operation is in flight. The connect timeout is
/// such a timer, so a `#[tokio::test(start_paused = true)]` request to a
/// local mock server usually fails with a connect timeout "after 30 s" that
/// took no real time. Turn the timeout off in those tests with
/// `with_connect_timeout(Duration::MAX)`, which registers no timer.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// TCP keepalive settings for a client's connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TcpKeepalive {
    /// How long a connection is idle before the first probe.
    pub(crate) idle: Duration,
    /// The time between unanswered probes.
    pub(crate) interval: Duration,
    /// How many unanswered probes fail the connection. Windows can't set
    /// it and sends its own 10.
    pub(crate) retries: u32,
}

/// The TCP keepalive every client sets on its connections: a probe after
/// 15 s idle, then every 15 s, failing the connection after 3 unanswered,
/// so a peer that stopped answering is noticed within a minute. Windows
/// sends its own count of 10 probes, which makes that under three minutes
/// (15 s + 10 x 15 s).
///
/// There is no default read timeout (see [`DEFAULT_CONNECT_TIMEOUT`]), so on
/// a connection that is silent by design, such as a non-streaming request
/// while the model generates, this is what notices a dead peer: a NAT or
/// load balancer that dropped the connection, or a host gone from the
/// network. A live server answers the probes, so a long generation is not
/// cut off. The values are reqwest 0.13's defaults, set explicitly so that
/// a reqwest upgrade can't change or drop them.
pub(crate) const TCP_KEEPALIVE: TcpKeepalive = TcpKeepalive {
    idle: Duration::from_secs(15),
    interval: Duration::from_secs(15),
    retries: 3,
};

impl TcpKeepalive {
    /// `builder` with these settings on every connection it opens.
    fn apply(self, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
        builder
            .tcp_keepalive(self.idle)
            .tcp_keepalive_interval(self.interval)
            .tcp_keepalive_retries(self.retries)
    }
}

/// Builder for `Client` instances.
///
/// Every client gets [`DEFAULT_CONNECT_TIMEOUT`] (30 s) unless
/// [`with_connect_timeout`](Self::with_connect_timeout) sets another, and no
/// read or total timeout unless one is set. Every connection sends TCP
/// keepalive probes after 15 s idle, then every 15 s, and fails after 3
/// unanswered, so a dead peer is noticed within a minute even on a
/// connection with no read timeout (under three minutes on Windows, which
/// sends its own count of 10 probes).
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
    api_version: Option<String>,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    wire_inspectors: Vec<Arc<dyn WireInspector>>,
}

// Custom Debug implementation that redacts the API key for security.
impl std::fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("api_version", &self.api_version)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
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
    /// consider setting a longer timeout (e.g., 120-300 seconds). It also
    /// caps reading a streamed body, so it cuts off long streams.
    ///
    /// If not set, requests have no total timeout (only the
    /// [connect timeout](Self::with_connect_timeout) applies by default).
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
    /// This is the maximum time to wait for establishing a connection to the
    /// server: DNS, TCP and TLS. A shorter timeout here can help fail fast if
    /// the network is unavailable. Exceeding it is a [`GenaiError::Http`]
    /// whose `is_connect()` and `is_timeout()` are true, and which
    /// [`is_retryable`](GenaiError::is_retryable).
    ///
    /// If not set, it is [`DEFAULT_CONNECT_TIMEOUT`] (30 s). `Duration::MAX`
    /// turns it off: the client then registers no connect timer at all,
    /// which is what a test on a paused tokio clock against a local server
    /// needs (see [`DEFAULT_CONNECT_TIMEOUT`]).
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

    /// Sets the read timeout: the longest the client waits for the next
    /// read, from the response headers onward, reset by every chunk that
    /// arrives.
    ///
    /// Unlike [`with_timeout`](Self::with_timeout), which caps the whole
    /// request and so has to be left generous (or unset) for long streams,
    /// this suits SSE streams: a healthy stream keeps sending events, while
    /// a stalled connection (a dropped NAT mapping, a half-open proxy)
    /// otherwise waits forever. The stream then ends with
    /// [`GenaiError::StreamInterrupted`], whose `source` is the timed-out
    /// [`reqwest::Error`].
    ///
    /// It also bounds the wait for the response headers, so it applies to
    /// every request on the client: a non-streaming `create()` against a
    /// model that thinks longer than this before answering fails with it
    /// too, as a [`GenaiError::Http`]. Leave room for the longest silence the
    /// API legitimately has (a long think, a background agent between status
    /// updates). If not set, reads wait indefinitely: there is no default,
    /// for that reason.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// let client = Client::builder("api_key".to_string())
    ///     .with_read_timeout(Duration::from_secs(120))
    ///     .build()?;
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub const fn with_read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = Some(timeout);
        self
    }

    /// Sends every request to `base_url` instead of
    /// `https://generativelanguage.googleapis.com`.
    ///
    /// Takes a scheme and host, optionally with a path prefix; the
    /// `/v1beta/...` and `/upload/v1beta/...` paths are appended to it, so a
    /// proxy or a local mock server sees the same paths the real API does.
    /// A trailing slash is ignored. [`with_api_version`](Self::with_api_version)
    /// changes the `v1beta` segment.
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

    /// Sends every request under API version `version` (the path segment
    /// after the host) instead of `v1beta`; `"v1"` selects the stable
    /// endpoint.
    ///
    /// `/v1` serves interactions in full, the
    /// same protocol as `/v1beta` (create, get, delete, cancel, streaming,
    /// and decoding continuation, which the v1 spec leaves out), plus the
    /// Files API (uploads included, under `/upload/v1`) and the File Search
    /// Store API. It answers `404` for agents,
    /// environments, webhooks, credentials, voices and triggers, so their
    /// handles fail on such a client. An interaction created on one version
    /// can be read on the other. `v1beta` stays the default because it has
    /// every resource.
    ///
    /// # Example
    ///
    /// ```
    /// use genai_rs::Client;
    ///
    /// let client = Client::builder("api_key".to_string())
    ///     .with_api_version("v1")
    ///     .build()?;
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub fn with_api_version(mut self, version: impl Into<String>) -> Self {
        self.api_version = Some(version.into());
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
        let (builder, applied) = self.reqwest_builder();
        let http_client = builder.build().map_err(GenaiError::ClientBuild)?;
        Ok(self.finish(http_client, applied))
    }

    /// The reqwest builder with this builder's timeouts and
    /// [`TCP_KEEPALIVE`] applied, and what it applied, for the `Client` to
    /// record. Split from [`build`](Self::build) so tests can add a resolver
    /// before building.
    fn reqwest_builder(&self) -> (reqwest::ClientBuilder, Applied) {
        self.reqwest_builder_with(TCP_KEEPALIVE)
    }

    /// [`reqwest_builder`](Self::reqwest_builder) with `tcp_keepalive` in
    /// place of [`TCP_KEEPALIVE`], so a test can set a value reqwest doesn't
    /// default to and find it on the socket.
    fn reqwest_builder_with(
        &self,
        tcp_keepalive: TcpKeepalive,
    ) -> (reqwest::ClientBuilder, Applied) {
        let mut builder = tcp_keepalive.apply(ReqwestClient::builder());

        // `Duration::MAX` sets none. reqwest would otherwise arm a timer
        // decades out, and a paused test clock auto-advances to any timer.
        let connect_timeout = match self.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT) {
            Duration::MAX => None,
            connect_timeout => {
                builder = builder.connect_timeout(connect_timeout);
                Some(connect_timeout)
            }
        };

        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }

        if let Some(read_timeout) = self.read_timeout {
            builder = builder.read_timeout(read_timeout);
        }

        (
            builder,
            Applied {
                connect_timeout,
                tcp_keepalive,
            },
        )
    }

    fn finish(self, http_client: ReqwestClient, applied: Applied) -> Client {
        let mut http = HttpContext::new(
            http_client,
            self.api_key,
            with_env_inspectors(self.wire_inspectors),
        );
        if let Some(base_url) = self.base_url {
            http = http.with_base_url(base_url);
        }
        if let Some(api_version) = self.api_version {
            http = http.with_api_version(api_version);
        }
        Client {
            http,
            connect_timeout: applied.connect_timeout,
            tcp_keepalive: applied.tcp_keepalive,
        }
    }
}

/// What [`ClientBuilder::reqwest_builder`] set on the reqwest builder that
/// reqwest's built client doesn't show.
struct Applied {
    /// The connect timeout (`None`: none).
    connect_timeout: Option<Duration>,
    tcp_keepalive: TcpKeepalive,
}

impl Client {
    /// Creates a new builder for `Client` instances.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Your Google AI API key.
    #[must_use]
    pub fn builder(api_key: impl Into<String>) -> ClientBuilder {
        ClientBuilder {
            api_key: api_key.into(),
            base_url: None,
            api_version: None,
            timeout: None,
            connect_timeout: None,
            read_timeout: None,
            wire_inspectors: Vec::new(),
        }
    }

    /// Creates a new `GenAI` client with the default timeouts:
    /// [`DEFAULT_CONNECT_TIMEOUT`] and no read or total timeout. The same as
    /// `Client::builder(api_key).build()`, panicking instead of returning
    /// the error.
    ///
    /// # Arguments
    ///
    /// * `api_key` - Your Google AI API key: a `String` or a `&str`.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP client can't be built: the TLS backend fails to
    /// initialize, or the resolver can't load the system configuration (as
    /// in some minimal containers). [`Client::builder`]`(..).build()`
    /// reports the same failure as [`GenaiError::ClientBuild`] instead.
    #[must_use]
    #[expect(
        clippy::panic,
        reason = "documented under `# Panics`; `Client::builder(..).build()` returns the error"
    )]
    pub fn new(api_key: impl Into<String>) -> Self {
        match Self::builder(api_key).build() {
            Ok(client) => client,
            Err(e) => panic!("{e}"),
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
    /// let mut stream = client.execute_stream(request);
    /// while let Some(result) = stream.next().await {
    ///     let event = result?;
    ///     match event.chunk {
    ///         StreamChunk::StepDelta { delta, .. } => {
    ///             if let Some(text) = delta.as_text() {
    ///                 print!("{}", text);
    ///             }
    ///         }
    ///         StreamChunk::Completed { interaction, .. } => {
    ///             println!("\nDone! ID: {:?}", interaction.id);
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
    /// - `event_id`: Optional ID for resuming; only background interactions
    ///   carry it (a foreground stream can't be resumed)
    ///
    /// The stream owns a clone of the client, so it can be stored or
    /// `tokio::spawn`ed. Nothing is sent until it is polled.
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
    ///         StreamChunk::Completed { .. } => {
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
    ) -> futures_util::stream::BoxStream<'static, Result<crate::StreamEvent, GenaiError>> {
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
    fn test_client_new_takes_a_str() {
        let client = Client::new("test_key");
        assert_eq!(client.http.api_key, "test_key");
    }

    #[test]
    fn test_client_builder_takes_a_str() {
        let client = Client::builder("test_key").build().unwrap();
        assert_eq!(client.http.api_key, "test_key");
    }

    /// A resolver that never answers, so a request sits in the connect
    /// phase with no I/O for the paused clock to race.
    struct NeverResolves;

    impl reqwest::dns::Resolve for NeverResolves {
        fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
            Box::pin(std::future::pending())
        }
    }

    /// How long a request through `builder`'s client waits to connect
    /// before failing, on the paused clock, and the error it fails with.
    async fn connect_wait(builder: &ClientBuilder) -> (Duration, reqwest::Error) {
        let client = builder
            .reqwest_builder()
            .0
            .dns_resolver(NeverResolves)
            .build()
            .unwrap();
        let start = tokio::time::Instant::now();
        let err = client
            .get("http://never-resolves.test/")
            .send()
            .await
            .unwrap_err();
        (start.elapsed(), err)
    }

    #[tokio::test(start_paused = true)]
    async fn test_default_connect_timeout_is_thirty_seconds() {
        let (waited, err) = connect_wait(&Client::builder("k".to_string())).await;
        assert!(
            waited >= DEFAULT_CONNECT_TIMEOUT
                && waited < DEFAULT_CONNECT_TIMEOUT + Duration::from_secs(1),
            "waited {waited:?}"
        );
        assert!(err.is_connect() && err.is_timeout(), "{err:?}");
        assert!(GenaiError::Http(err).is_retryable());
    }

    #[tokio::test(start_paused = true)]
    async fn test_with_connect_timeout_replaces_the_default() {
        let builder = Client::builder("k".to_string()).with_connect_timeout(Duration::from_secs(5));
        let (waited, err) = connect_wait(&builder).await;
        assert!(
            waited >= Duration::from_secs(5) && waited < Duration::from_secs(6),
            "waited {waited:?}"
        );
        assert!(err.is_timeout(), "{err:?}");
    }

    #[test]
    fn test_connect_timeout_of_duration_max_sets_none() {
        // reqwest's builder Debug lists a connect timeout only when one is
        // set; with none, nothing is armed for a paused clock to fire.
        // (`tests/reliability_tests.rs` checks the paused-clock behavior.)
        let debug = |builder: ClientBuilder| format!("{:?}", builder.reqwest_builder().0);
        let default = debug(Client::builder("k".to_string()));
        assert!(default.contains("connect_timeout: 30s"), "{default}");
        let long = debug(
            Client::builder("k".to_string()).with_connect_timeout(Duration::from_secs(86_400)),
        );
        assert!(long.contains("connect_timeout: 86400s"), "{long}");
        let off = debug(Client::builder("k".to_string()).with_connect_timeout(Duration::MAX));
        assert!(!off.contains("connect_timeout"), "{off}");
    }

    #[test]
    fn test_public_constructors_carry_the_connect_timeout() {
        // reqwest's built client doesn't show its connect timeout, so the
        // `Client` records the one `reqwest_builder` applied. This pins that
        // `Client::new` and `build()` both go through it.
        for client in [
            Client::new("k".to_string()),
            Client::builder("k".to_string()).build().unwrap(),
        ] {
            assert_eq!(client.connect_timeout, Some(DEFAULT_CONNECT_TIMEOUT));
            let debug = format!("{client:?}");
            assert!(debug.contains("connect_timeout: Some(30s)"), "{debug}");
        }
        let set = Client::builder("k".to_string())
            .with_connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        assert_eq!(set.connect_timeout, Some(Duration::from_secs(5)));
        let off = Client::builder("k".to_string())
            .with_connect_timeout(Duration::MAX)
            .build()
            .unwrap();
        assert_eq!(off.connect_timeout, None);
    }

    #[test]
    fn test_public_constructors_record_tcp_keepalive() {
        // reqwest's built client doesn't show its keepalive either, so the
        // `Client` records what `reqwest_builder` applied. This pins that
        // both public constructors go through it with `TCP_KEEPALIVE`,
        // whatever the timeouts, and the values: the first probe plus three
        // retries notice a dead peer within a minute (Windows sends 10).
        // `test_tcp_keepalive_reaches_the_socket` checks the socket itself.
        assert_eq!(
            TCP_KEEPALIVE,
            TcpKeepalive {
                idle: Duration::from_secs(15),
                interval: Duration::from_secs(15),
                retries: 3,
            }
        );
        assert_eq!(
            TCP_KEEPALIVE.idle + TCP_KEEPALIVE.interval * TCP_KEEPALIVE.retries,
            Duration::from_secs(60)
        );
        for client in [
            Client::new("k".to_string()),
            Client::builder("k".to_string()).build().unwrap(),
            Client::builder("k".to_string())
                .with_connect_timeout(Duration::MAX)
                .with_read_timeout(Duration::from_secs(5))
                .with_timeout(Duration::from_secs(9))
                .build()
                .unwrap(),
        ] {
            assert_eq!(client.tcp_keepalive, TCP_KEEPALIVE);
            let debug = format!("{client:?}");
            assert!(
                debug.contains(
                    "tcp_keepalive: TcpKeepalive { idle: 15s, interval: 15s, retries: 3 }"
                ),
                "{debug}"
            );
        }
    }

    /// The keepalive the OS shows on the connection `client` opens for one
    /// request to a local server, read while both ends hold it open: the
    /// time left before the first probe on Linux, the idle time itself on
    /// macOS, `None` when keepalive is off.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    async fn connection_keepalive(client: &reqwest::Client) -> Option<Duration> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = listener.local_addr().unwrap();
        let serve = tokio::spawn(async move {
            let (mut stream, peer) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.ends_with(b"\r\n\r\n") {
                let n = stream.read(&mut buf).await.unwrap();
                assert!(n > 0, "the client closed before its request ended");
                request.extend_from_slice(&buf[..n]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                .await
                .unwrap();
            (stream, peer)
        });
        let response = client
            .get(format!("http://{server}/"))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success(), "{response:?}");
        // The server's end, held so the client's end stays established.
        let (_stream, peer) = serve.await.unwrap();

        // Linux shows one timer per socket, and a retransmit timer still
        // pending would hide the keepalive one for a moment.
        let mut last = String::new();
        for _ in 0..50 {
            match os_keepalive(peer.port(), server.port()) {
                Ok(shown) => return shown,
                Err(e) => last = e,
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{last}");
    }

    #[cfg(target_os = "linux")]
    fn os_keepalive(local: u16, remote: u16) -> Result<Option<Duration>, String> {
        let table = std::fs::read_to_string("/proc/net/tcp").map_err(|e| e.to_string())?;
        proc_net_tcp_keepalive(&table, local, remote)
    }

    #[cfg(target_os = "macos")]
    fn os_keepalive(local: u16, remote: u16) -> Result<Option<Duration>, String> {
        let output = std::process::Command::new("lsof")
            .args(["-a", "-n", "-P", "-T", "fs"])
            .arg(format!("-p{}", std::process::id()))
            .arg(format!("-iTCP:{remote}"))
            .output()
            .map_err(|e| format!("lsof: {e}"))?;
        lsof_keepalive(&String::from_utf8_lossy(&output.stdout), local, remote)
    }

    /// From a `/proc/net/tcp` table, the keepalive timer of the established
    /// connection from 127.0.0.1:`local` to 127.0.0.1:`remote`: the time
    /// left on it when it runs (`tr` 2), `None` when no timer runs (`tr`
    /// 0). An error when the connection isn't listed or shows another
    /// timer, such as a retransmit.
    fn proc_net_tcp_keepalive(
        table: &str,
        local: u16,
        remote: u16,
    ) -> Result<Option<Duration>, String> {
        // Matched on the ports alone: the address is in the host's byte
        // order (127.0.0.1 is `0100007F` on x86 and arm64).
        let (local, remote) = (format!(":{local:04X}"), format!(":{remote:04X}"));
        for line in table.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // sl, local_address, rem_address, st, tx_queue:rx_queue, tr:tm->when
            let [_, from, to, "01", _, timer, ..] = fields[..] else {
                continue;
            };
            if !from.ends_with(&local) || !to.ends_with(&remote) {
                continue;
            }
            let (active, when) = timer.split_once(':').ok_or(line)?;
            let when = u64::from_str_radix(when, 16).map_err(|e| format!("{e}: {line}"))?;
            return match active {
                "00" => Ok(None),
                // In clock ticks, USER_HZ: 100 a second on every Linux CI runs.
                "02" => Ok(Some(Duration::from_millis(when * 10))),
                _ => Err(format!("a timer other than keepalive runs: {line}")),
            };
        }
        Err(format!(
            "no established {local} -> {remote} (hex ports) in /proc/net/tcp"
        ))
    }

    /// From `lsof -T fs` output, the keepalive idle time macOS shows on the
    /// established connection from 127.0.0.1:`local` to 127.0.0.1:`remote`
    /// (`SO=KEEPALIVE=<ms>`), `None` when its socket options don't list it.
    fn lsof_keepalive(output: &str, local: u16, remote: u16) -> Result<Option<Duration>, String> {
        let connection = format!("127.0.0.1:{local}->127.0.0.1:{remote} (ESTABLISHED");
        let line = output
            .lines()
            .find(|line| line.contains(&connection))
            .ok_or_else(|| format!("no {connection}) in lsof's output:\n{output}"))?;
        let options = line
            .split_whitespace()
            .find_map(|field| field.strip_prefix("SO="))
            .ok_or_else(|| format!("no socket options: {line}"))?;
        for option in options.trim_end_matches(')').split(',') {
            if let Some(ms) = option.strip_prefix("KEEPALIVE=") {
                let ms = ms.parse().map_err(|e| format!("{e}: {line}"))?;
                return Ok(Some(Duration::from_millis(ms)));
            }
        }
        Ok(None)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn test_tcp_keepalive_reaches_the_socket() {
        // hyper-util only logs a keepalive it fails to set, and the record
        // `test_public_constructors_record_tcp_keepalive` checks is what was
        // passed on, so this reads the socket. `TCP_KEEPALIVE` is reqwest's
        // default too, so a second client sets an idle time reqwest doesn't
        // default to: dropping `apply` fails there. The OS shows only the
        // idle time; the interval and retries go through the same `apply`.
        let own = TcpKeepalive {
            idle: Duration::from_secs(27),
            ..TCP_KEEPALIVE
        };
        let new = Client::new("k".to_string());
        let built = Client::builder("k".to_string()).build().unwrap();
        let set = Client::builder("k".to_string())
            .reqwest_builder_with(own)
            .0
            .build()
            .unwrap();
        for (client, idle) in [
            (&new.http.http_client, TCP_KEEPALIVE.idle),
            (&built.http.http_client, TCP_KEEPALIVE.idle),
            (&set, own.idle),
        ] {
            // Linux shows the time left, a little under `idle`; macOS shows
            // `idle` give or take a millisecond.
            let near = idle - Duration::from_secs(5)..idle + Duration::from_secs(1);
            let shown = connection_keepalive(client).await;
            assert!(
                shown.is_some_and(|shown| near.contains(&shown)),
                "the socket shows keepalive {shown:?}, not about {idle:?}"
            );
        }
    }

    #[test]
    fn test_keepalive_is_read_from_proc_net_tcp() {
        // Lines as Linux prints them: a listener on 127.0.0.1:41969 (A3F1),
        // three clients of it, one per timer state, and the listener's end
        // of the first.
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:A3F1 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1001        0 41001 1 0000000000000000 100 0 0 10 0
   1: 0100007F:DD4E 0100007F:A3F1 01 00000000:00000000 02:00000A8C 00000000  1001        0 41002 2 0000000000000000 20 4 30 10 -1
   2: 0100007F:A3F1 0100007F:DD4E 01 00000000:00000000 00:00000000 00000000  1001        0 41003 1 0000000000000000 20 4 30 10 -1
   3: 0100007F:DD4F 0100007F:A3F1 01 00000000:00000000 00:00000000 00000000  1001        0 41004 1 0000000000000000 20 4 30 10 -1
   4: 0100007F:DD50 0100007F:A3F1 01 00000000:00000000 01:00000014 00000000  1001        0 41005 1 0000000000000000 20 4 30 10 -1
";
        assert_eq!(
            proc_net_tcp_keepalive(table, 56654, 41969),
            Ok(Some(Duration::from_secs(27)))
        );
        assert_eq!(proc_net_tcp_keepalive(table, 56655, 41969), Ok(None));
        assert!(proc_net_tcp_keepalive(table, 56656, 41969).is_err());
        assert!(proc_net_tcp_keepalive(table, 56657, 41969).is_err());
    }

    #[test]
    fn test_keepalive_is_read_from_lsof() {
        // As macOS's lsof prints it, for a client with keepalive and one
        // without.
        let output = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
genai 51659 u 4u IPv4 0xa1 0t0 TCP 127.0.0.1:56647->127.0.0.1:56646 (ESTABLISHED SO=KEEPALIVE=27000,PQLEN=0,QLEN=0,QLIM=0,RCVBUF=3266400,SNDBUF=1175904 TF=MSS=16332)
genai 51659 u 5u IPv4 0x97 0t0 TCP 127.0.0.1:56646->127.0.0.1:56647 (ESTABLISHED SO=PQLEN=0,QLEN=0,QLIM=0,RCVBUF=3266400,SNDBUF=1175904 TF=MSS=16332)
genai 51659 u 6u IPv4 0x68 0t0 TCP 127.0.0.1:56648->127.0.0.1:56646 (ESTABLISHED SO=PQLEN=0,QLEN=0,QLIM=0,RCVBUF=3266400,SNDBUF=1175904)
";
        assert_eq!(
            lsof_keepalive(output, 56647, 56646),
            Ok(Some(Duration::from_secs(27)))
        );
        assert_eq!(lsof_keepalive(output, 56648, 56646), Ok(None));
        assert!(lsof_keepalive(output, 56649, 56646).is_err());
    }

    #[test]
    fn test_no_default_read_or_total_timeout() {
        // reqwest's Debug lists a total or read timeout only when one is
        // set. (`tests/reliability_tests.rs` checks the behavior: an answer
        // an hour late still arrives.)
        for client in [
            Client::new("k".to_string()),
            Client::builder("k".to_string()).build().unwrap(),
        ] {
            let debug = format!("{:?}", client.http.http_client);
            assert!(!debug.contains("read_timeout"), "{debug}");
            assert!(!debug.contains("Timeout"), "{debug}");
        }
        let with_both = Client::builder("k".to_string())
            .with_timeout(Duration::from_secs(1))
            .with_read_timeout(Duration::from_secs(1))
            .build()
            .unwrap();
        let debug = format!("{:?}", with_both.http.http_client);
        assert!(
            debug.contains("read_timeout") && debug.contains("Timeout"),
            "the check above would miss a timeout: {debug}"
        );
    }

    #[test]
    fn test_client_debug_redacts_api_key() {
        let client = Client::new("super_secret_api_key_12345".to_string());
        let debug_output = format!("{client:?}");

        // API key should NOT appear in debug output
        assert!(
            !debug_output.contains("super_secret_api_key_12345"),
            "API key was exposed in debug output: {debug_output}"
        );
        // Should show [REDACTED] instead
        assert!(
            debug_output.contains("[REDACTED]"),
            "Debug output should contain [REDACTED]: {debug_output}"
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
        let debug_output = format!("{builder:?}");

        // API key should NOT appear in debug output
        assert!(
            !debug_output.contains("another_secret_key_67890"),
            "API key was exposed in builder debug output: {debug_output}"
        );
        // Should show [REDACTED] instead
        assert!(
            debug_output.contains("[REDACTED]"),
            "Builder debug output should contain [REDACTED]: {debug_output}"
        );
    }
}
