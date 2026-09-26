use super::InteractionBuilder;
use crate::{
    EnvironmentSpec, GenerationConfig, SafetySetting, ServiceTier, ThinkingLevel,
    ThinkingSummaries, WebhookConfig,
};
use std::time::Duration;

impl<'a> InteractionBuilder<'a> {
    /// Sets per-request webhook routing.
    ///
    /// Events for this request are delivered to the config's URIs instead of
    /// the registered webhooks, with optional user metadata echoed on each
    /// event.
    ///
    /// The API **requires** [`with_background(true)`](Self::with_background)
    /// when a webhook config is set (verified live: requests are rejected
    /// with HTTP 400 `"background=true is required when webhook_config is
    /// specified."` otherwise).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, WebhookConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
    ///     .with_text("Research the history of quantum computing")
    ///     .with_background(true)
    ///     .with_webhook_config(
    ///         WebhookConfig::new()
    ///             .with_uris(vec!["https://example.com/hooks/genai".to_string()])
    ///             .with_user_metadata(serde_json::json!({"job_id": "job-42"})),
    ///     )
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_webhook_config(mut self, config: WebhookConfig) -> Self {
        self.webhook_config = Some(config);
        self
    }

    /// Sets the environment for this interaction.
    ///
    /// Accepts a string environment ID (e.g., from a previous response's
    /// `environment_id`) or a typed
    /// [`RemoteEnvironment`](crate::RemoteEnvironment) with sources and a
    /// network allowlist.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, EnvironmentSource, RemoteEnvironment};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Typed remote environment
    /// let response = client
    ///     .interaction()
    ///     .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
    ///     .with_text("Run the test suite")
    ///     .with_environment(
    ///         RemoteEnvironment::new()
    ///             .add_source(EnvironmentSource::repository("github.com/org/repo", "/workspace")),
    ///     )
    ///     .create()
    ///     .await?;
    ///
    /// // Or reuse an environment by ID on the next turn
    /// let env_id = response.environment_id.clone().unwrap_or_default();
    /// let follow_up = client
    ///     .interaction()
    ///     .with_agent(genai_rs::DEFAULT_ANTIGRAVITY_AGENT)
    ///     .with_previous_interaction(response.id.clone().unwrap_or_default())
    ///     .with_text("Now fix the failing test")
    ///     .with_environment(env_id)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_environment(mut self, environment: impl Into<EnvironmentSpec>) -> Self {
        self.environment = Some(environment.into());
        self
    }

    /// Sets the safety settings for this request, replacing any previously
    /// added ones.
    ///
    /// Server-side constraint (verified live 2026-08-08): the Gemini API
    /// rejects `safety_settings` (Vertex-only); see [`SafetySetting`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, HarmCategory, SafetySetting, SafetyThreshold};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Tell me about safety filters")
    ///     .with_safety_settings(vec![SafetySetting::new(
    ///         HarmCategory::Harassment,
    ///         SafetyThreshold::BlockOnlyHigh,
    ///     )])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_safety_settings(mut self, settings: Vec<SafetySetting>) -> Self {
        self.safety_settings = Some(settings);
        self
    }

    /// Adds a single safety setting, accumulating with any added earlier.
    #[must_use]
    pub fn add_safety_setting(mut self, setting: SafetySetting) -> Self {
        self.safety_settings
            .get_or_insert_with(Vec::new)
            .push(setting);
        self
    }

    /// Sets the user-defined metadata labels for this request, replacing
    /// any previously added ones.
    ///
    /// Accepted by the Gemini API and echoed as
    /// [`InteractionResponse::labels`](crate::InteractionResponse::labels)
    /// (verified live 2026-09-24). Stored in a `BTreeMap` so the serialized
    /// key order is deterministic; a repeated key in the input keeps the
    /// last value.
    #[must_use]
    pub fn with_labels(
        mut self,
        labels: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.labels = Some(
            labels
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    /// Adds a single metadata label, merging with any added earlier — a
    /// repeated key replaces its previous value (the backing store is a
    /// map, unlike [`Self::add_safety_setting`]'s accumulating list).
    #[must_use]
    pub fn add_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels
            .get_or_insert_with(std::collections::BTreeMap::new)
            .insert(key.into(), value.into());
        self
    }

    /// Sets generation configuration (temperature, max tokens, etc.).
    #[must_use]
    pub fn with_generation_config(mut self, config: GenerationConfig) -> Self {
        self.generation_config = Some(config);
        self
    }

    /// Sets the thinking level for reasoning/chain-of-thought output.
    ///
    /// Higher levels produce more detailed reasoning but consume more tokens.
    /// When thinking is enabled, the model's reasoning process is exposed
    /// in the response as `Thought` content. Use `response.usage.total_reasoning_tokens`
    /// to track reasoning token costs.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, ThinkingLevel};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Solve this step by step: 15 * 23")
    ///     .with_thinking_level(ThinkingLevel::Medium)
    ///     .create()
    ///     .await?;
    ///
    /// if response.has_thoughts() {
    ///     // Thoughts contain cryptographic signatures, not readable text
    ///     let sig_count = response.thought_signatures().count();
    ///     println!("Model used reasoning ({} thought signatures)", sig_count);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_thinking_level(mut self, level: ThinkingLevel) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.thinking_level = Some(level);
        self
    }

    /// Controls whether thinking summaries are included in output.
    ///
    /// When using `with_thinking_level()`, summaries of the model's reasoning
    /// process can be included alongside thought signatures. Use `Auto` to
    /// include summaries, or `None` to exclude them.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, ThinkingLevel, ThinkingSummaries};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Solve this step by step: 15 * 23")
    ///     .with_thinking_level(ThinkingLevel::Medium)
    ///     .with_thinking_summaries(ThinkingSummaries::Auto)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_thinking_summaries(mut self, summaries: ThinkingSummaries) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.thinking_summaries = Some(summaries);
        self
    }

    /// Sets a seed for deterministic output generation.
    ///
    /// Using the same seed with identical inputs will produce the same output,
    /// useful for testing and debugging. The exact same seed, model, and input
    /// should produce reproducible results.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    ///
    /// // Two requests with the same seed should produce the same output
    /// let response1 = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Generate a random number")
    ///     .with_seed(42)
    ///     .create()
    ///     .await?;
    ///
    /// let response2 = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Generate a random number")
    ///     .with_seed(42)
    ///     .create()
    ///     .await?;
    ///
    /// // response1.as_text() should equal response2.as_text()
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_seed(mut self, seed: i64) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.seed = Some(seed);
        self
    }

    /// Sets stop sequences that halt generation.
    ///
    /// When the model generates any of these sequences, generation stops
    /// immediately. Useful for controlling output boundaries in chat applications
    /// or structured generation.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Write a story")
    ///     .with_stop_sequences(vec!["THE END".to_string(), "---".to_string()])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_stop_sequences(mut self, sequences: Vec<String>) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.stop_sequences = Some(sequences);
        self
    }

    /// Sets the latency/priority service tier for this request.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, ServiceTier};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("key".to_string());
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello")
    ///     .with_service_tier(ServiceTier::Flex)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_service_tier(mut self, tier: ServiceTier) -> Self {
        self.service_tier = Some(tier);
        self
    }

    /// Sets the presence penalty (range [-2.0, 2.0]).
    ///
    /// Positive values penalize tokens that already appeared in the text,
    /// increasing the likelihood of new topics.
    #[must_use]
    pub fn with_presence_penalty(mut self, penalty: f32) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.presence_penalty = Some(penalty);
        self
    }

    /// Sets the frequency penalty (range [-2.0, 2.0]).
    ///
    /// Positive values penalize tokens proportionally to their frequency in
    /// the text so far, reducing repetition.
    #[must_use]
    pub fn with_frequency_penalty(mut self, penalty: f32) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.frequency_penalty = Some(penalty);
        self
    }

    /// Sets a timeout for the request.
    ///
    /// If the request takes longer than the specified duration, it will be
    /// cancelled and return [`GenaiError::Timeout`].
    ///
    /// # Behavior by Method
    ///
    /// | Method | Timeout Applies To |
    /// |--------|-------------------|
    /// | `create()` | Entire request |
    /// | `create_stream()` | Per-chunk (inter-chunk timeout) |
    /// | `create_with_auto_functions()` | Per-API-call (each round) |
    /// | `create_stream_with_auto_functions()` | Per-chunk (each streaming round) |
    ///
    /// For auto-function methods, function execution time is **not** counted against
    /// the timeout. For a total timeout including function execution, wrap the call
    /// in `tokio::time::timeout()`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    /// use std::time::Duration;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("What is the meaning of life?")
    ///     .with_timeout(Duration::from_secs(30))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`GenaiError::Timeout`]: crate::GenaiError::Timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}
