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
    /// Accepted by the Gemini API (verified live 2026-10-30); some category
    /// and threshold values are Vertex-only, see [`SafetySetting`].
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
    ///     let sig_count = response.thought_signatures().len();
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

    /// Continues a decode that stopped early, using the
    /// [`continuation_token`](crate::InteractionResponse::continuation_token)
    /// of the response that stopped.
    ///
    /// For a stored interaction, [`continue_decode`](Self::continue_decode)
    /// is simpler: it needs no input. This method is for a `store: false`
    /// one, which has no id to continue from: build the request as before,
    /// with the same input, and raise `max_output_tokens`, which the API
    /// counts across continuations. Only
    /// [`CONTINUATION_MODEL`](crate::CONTINUATION_MODEL) supports continuation
    /// (verified live 2026-11-01).
    ///
    /// ```no_run
    /// # use genai_rs::{Client, GenerationConfig, InteractionStatus};
    /// # async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
    /// let ask = |max_output_tokens| {
    ///     client
    ///         .interaction()
    ///         .with_model(genai_rs::CONTINUATION_MODEL)
    ///         .with_text("Write a sonnet about the sea.")
    ///         .with_generation_config(GenerationConfig {
    ///             max_output_tokens: Some(max_output_tokens),
    ///             ..Default::default()
    ///         })
    /// };
    /// let first = ask(30).create().await?;
    /// if let Some(token) = &first.continuation_token {
    ///     let rest = ask(400).with_continuation_token(token).create().await?;
    ///     println!("{}{}", first.all_text(), rest.all_text());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_continuation_token(mut self, token: impl Into<String>) -> Self {
        self.continuation_token = Some(token.into());
        self
    }

    /// Continues the decode `response` stopped early, without resending the
    /// input: sets the model (or agent) the response names, its `id` as the
    /// previous interaction, and its
    /// [`continuation_token`](crate::InteractionResponse::continuation_token).
    ///
    /// The API continues a stored interaction from its id and token alone
    /// (verified live 2026-11-05 on
    /// [`CONTINUATION_MODEL`](crate::CONTINUATION_MODEL), the only model
    /// that supports continuation). No `max_output_tokens` is sent unless
    /// you set one; the continuation then ran to completion (live
    /// 2026-11-05). A budget you set counts what the decode already
    /// produced, so it must exceed that.
    ///
    /// The previous interaction and the token always become the
    /// response's, replacing any set earlier on the builder, and are
    /// cleared when the response has none: a token continues only the
    /// interaction that issued it, so a builder-set id or token could only
    /// pair it with the wrong one. A `store: false` response has no `id`,
    /// and its request could not have had a previous interaction either
    /// (chaining needs storage), so its continuation has none. The model
    /// or agent is replaced only when the response names one.
    ///
    /// [`build`](Self::build) fails with
    /// [`GenaiError::InvalidInput`](crate::GenaiError::InvalidInput) when
    /// `response` carries no token and no input was set, and when it has no
    /// `id` (a `store: false` interaction): for that one, resend the
    /// original input, with `continue_decode` or
    /// [`with_continuation_token`](Self::with_continuation_token).
    ///
    /// ```no_run
    /// # use genai_rs::{Client, GenerationConfig, InteractionStatus};
    /// # async fn example(client: &Client) -> Result<(), genai_rs::GenaiError> {
    /// let mut response = client
    ///     .interaction()
    ///     .with_model(genai_rs::CONTINUATION_MODEL)
    ///     .with_text("Write a sonnet about the sea.")
    ///     .with_generation_config(GenerationConfig {
    ///         max_output_tokens: Some(30),
    ///         ..Default::default()
    ///     })
    ///     .create()
    ///     .await?;
    /// let mut text = response.all_text();
    /// while response.status == InteractionStatus::Incomplete
    ///     && response.continuation_token.is_some()
    /// {
    ///     response = client.interaction().continue_decode(&response).create().await?;
    ///     text.push_str(&response.all_text());
    /// }
    /// println!("{text}");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn continue_decode(mut self, response: &crate::InteractionResponse) -> Self {
        if let Some(model) = &response.model {
            self.model = Some(model.clone());
            self.agent = None;
        } else if let Some(agent) = &response.agent {
            self.agent = Some(agent.clone());
            self.model = None;
        }
        // Both replaced, even with nothing: see the doc comment.
        self.previous_interaction_id.clone_from(&response.id);
        self.continuation_token
            .clone_from(&response.continuation_token);
        self
    }

    /// Adds a request field this crate does not model yet, sent flat
    /// alongside the modeled ones (it lands in
    /// [`InteractionRequest::extra`](crate::InteractionRequest::extra)).
    /// Fields added earlier are kept, and a repeated key replaces its
    /// previous value, as with [`Self::add_label`].
    ///
    /// The server validates it like any other field: an unknown name is a
    /// `400 Unknown parameter`. A key that names a modeled field overrides
    /// it on the wire, since the API keeps the last of a repeated key,
    /// except `stream`: whether a request streams is decided by the method
    /// that sends it (`create()` or `create_stream()`), so a `stream` key
    /// added here is dropped before sending, with a `warn!`.
    ///
    /// ```
    /// # use genai_rs::Client;
    /// # let client = Client::new("api-key".to_string());
    /// let request = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello")
    ///     .add_extra("brand_new_field", serde_json::json!({"enabled": true}))
    ///     .build()?;
    ///
    /// let body = serde_json::to_value(&request).unwrap();
    /// assert_eq!(body["brand_new_field"], serde_json::json!({"enabled": true}));
    /// # Ok::<(), genai_rs::GenaiError>(())
    /// ```
    #[must_use]
    pub fn add_extra(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.extra.insert(key.into(), value.into());
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

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
