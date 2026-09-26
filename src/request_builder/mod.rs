mod auto_functions;
mod conversation;
mod generation;
mod input;
mod output;
mod tools;

pub use conversation::ConversationBuilder;

use auto_functions::DEFAULT_MAX_FUNCTION_CALL_LOOPS;

use crate::GenaiError;
use crate::client::Client;
use crate::function_calling::ToolService;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

use crate::{
    AgentConfig, Content, EnvironmentSpec, GenerationConfig, InteractionInput, InteractionRequest,
    InteractionResponse, ResponseFormatSpec, SafetySetting, ServiceTier, SpeechConfig, Step,
    StreamEvent, Tool as InternalTool, WebhookConfig,
};
use futures_util::{StreamExt, stream::BoxStream};

/// Builder for creating interactions with the Gemini Interactions API.
///
/// Provides a fluent interface for constructing interaction requests with models or agents.
/// All methods are available in any order - invalid combinations are validated at runtime
/// when calling `build()`, `create()`, or other terminal methods.
///
/// # Runtime Validation
///
/// The following combinations are invalid and will return an error:
/// - `with_store_disabled()` + `with_previous_interaction()`: chained interactions require storage
/// - `with_store_disabled()` + `with_background(true)`: background execution requires storage
/// - `with_store_disabled()` + `create_with_auto_functions()`: auto-function calling requires storage
///
/// # Examples
///
/// ## Simple interaction
///
/// ```no_run
/// # use genai_rs::{Client, StreamChunk};
/// # use futures_util::StreamExt;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::builder("api_key".to_string()).build()?;
///
/// let response = client.interaction()
///     .with_model(genai_rs::DEFAULT_MODEL)
///     .with_text("What is the capital of France?")
///     .create()
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Conditional chaining (no code duplication needed)
///
/// ```no_run
/// # use genai_rs::{Client, Content};
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = Client::builder("api_key".to_string()).build()?;
/// # let previous_interaction_id: Option<String> = None;
/// # let input = "Hello";
/// // Build common configuration, then conditionally add previous_interaction
/// let mut builder = client.interaction()
///     .with_model(genai_rs::DEFAULT_MODEL)
///     .with_system_instruction("You are a helpful assistant.")
///     .with_content(vec![Content::text(input)]);
///
/// if let Some(prev_id) = previous_interaction_id {
///     builder = builder.with_previous_interaction(prev_id);
/// }
///
/// let response = builder.create().await?;
/// # Ok(())
/// # }
/// ```
pub struct InteractionBuilder<'a> {
    client: &'a Client,
    model: Option<String>,
    agent: Option<String>,
    agent_config: Option<AgentConfig>,
    /// Conversation history as steps (set by `with_history()`)
    history: Vec<Step>,
    /// Current user message (set by `with_text()`)
    current_message: Option<String>,
    /// Content input for function results (set by `with_content()`)
    content_input: Option<Vec<Content>>,
    previous_interaction_id: Option<String>,
    tools: Option<Vec<InternalTool>>,
    response_modalities: Option<Vec<String>>,
    response_format: Option<ResponseFormatSpec>,
    generation_config: Option<GenerationConfig>,
    speech_configs: Option<Vec<SpeechConfig>>,
    background: Option<bool>,
    store: Option<bool>,
    system_instruction: Option<String>,
    service_tier: Option<ServiceTier>,
    webhook_config: Option<WebhookConfig>,
    environment: Option<EnvironmentSpec>,
    safety_settings: Option<Vec<SafetySetting>>,
    labels: Option<std::collections::BTreeMap<String, String>>,
    /// Maximum iterations for auto function calling loop
    max_function_call_loops: usize,
    /// Tool service for dependency-injected functions
    tool_service: Option<Arc<dyn ToolService>>,
    /// Optional timeout for the request
    timeout: Option<Duration>,
}

impl std::fmt::Debug for InteractionBuilder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionBuilder")
            .field("model", &self.model)
            .field("agent", &self.agent)
            .field("agent_config", &self.agent_config)
            .field("history", &self.history)
            .field("current_message", &self.current_message)
            .field("content_input", &self.content_input)
            .field("previous_interaction_id", &self.previous_interaction_id)
            .field("tools", &self.tools)
            .field("response_modalities", &self.response_modalities)
            .field("response_format", &self.response_format)
            .field("generation_config", &self.generation_config)
            .field("speech_configs", &self.speech_configs)
            .field("webhook_config", &self.webhook_config)
            .field("environment", &self.environment)
            .field("background", &self.background)
            .field("store", &self.store)
            .field("system_instruction", &self.system_instruction)
            .field("service_tier", &self.service_tier)
            .field("safety_settings", &self.safety_settings)
            .field("labels", &self.labels)
            .field("max_function_call_loops", &self.max_function_call_loops)
            .field("tool_service", &self.tool_service.as_ref().map(|_| "..."))
            .field("timeout", &self.timeout)
            .finish()
    }
}

// ============================================================================
// InteractionBuilder implementation
// ============================================================================

impl<'a> InteractionBuilder<'a> {
    /// Creates a new interaction builder.
    pub(crate) fn new(client: &'a Client) -> Self {
        Self {
            client,
            model: None,
            agent: None,
            agent_config: None,
            history: Vec::new(),
            current_message: None,
            content_input: None,
            previous_interaction_id: None,
            tools: None,
            response_modalities: None,
            response_format: None,
            generation_config: None,
            speech_configs: None,
            background: None,
            store: None,
            system_instruction: None,
            service_tier: None,
            webhook_config: None,
            environment: None,
            safety_settings: None,
            labels: None,
            max_function_call_loops: DEFAULT_MAX_FUNCTION_CALL_LOOPS,
            tool_service: None,
            timeout: None,
        }
    }

    /// Validates the builder configuration against API constraints.
    ///
    /// This is called automatically by terminal methods (`build()`, `create()`, etc.).
    /// Invalid combinations return clear, actionable error messages.
    ///
    /// # Constraints
    ///
    /// - `with_store_disabled()` + `with_previous_interaction()`: chained interactions require storage
    /// - `with_store_disabled()` + `with_background(true)`: background execution requires storage
    fn validate(&self) -> Result<(), GenaiError> {
        // Constraint: Storage is required for chained interactions
        if self.store == Some(false) && self.previous_interaction_id.is_some() {
            return Err(GenaiError::InvalidInput(
                "Chained interactions require storage. \
                 Cannot use with_previous_interaction() with with_store_disabled(). \
                 Solution: Remove .with_store_disabled() to enable storage, \
                 or remove .with_previous_interaction() if this is a new conversation."
                    .to_string(),
            ));
        }

        // Constraint: Storage is required for background execution
        if self.store == Some(false) && self.background == Some(true) {
            return Err(GenaiError::InvalidInput(
                "Background execution requires storage. \
                 Cannot use with_background(true) with with_store_disabled(). \
                 Solution: Remove .with_store_disabled() to enable storage, \
                 or set .with_background(false)."
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// Validates for auto-function calling (stricter than general validation).
    fn validate_for_auto_functions(&self) -> Result<(), GenaiError> {
        // Base validation first
        self.validate()?;

        // Auto-functions require storage to maintain context across function calls
        if self.store == Some(false) {
            return Err(GenaiError::InvalidInput(
                "create_with_auto_functions() requires storage to maintain conversation context \
                 across multiple function execution rounds. \
                 Solution: Remove .with_store_disabled() to enable storage, \
                 or use create() for single-turn function handling."
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// Creates the interaction and returns the response.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No input was provided
    /// - Neither model nor agent was specified
    /// - The API request fails
    /// - The request times out (if `with_timeout()` was set)
    pub async fn create(self) -> Result<InteractionResponse, GenaiError> {
        let client = self.client;
        let timeout = self.timeout;
        let request = self.build()?;

        let future = client.execute(request);

        match timeout {
            Some(duration) => tokio::time::timeout(duration, future).await.map_err(|_| {
                debug!("Request timed out after {:?}", duration);
                GenaiError::Timeout(duration)
            })?,
            None => future.await,
        }
    }

    /// Creates a streaming interaction that yields chunks as they arrive.
    ///
    /// Returns a stream of `StreamChunk` items:
    /// - `StreamChunk::StepDelta`: Incremental step payload (text, thought
    ///   signatures, streaming function-call arguments, ...)
    /// - `StreamChunk::Completed`: The final complete interaction response
    ///
    /// # Timeout Behavior
    ///
    /// If `with_timeout()` was set, the timeout applies **per-chunk**, not to
    /// the total stream duration. Each `stream.next().await` call must complete
    /// within the timeout, or a [`GenaiError::Timeout`] error is yielded.
    ///
    /// This is useful for detecting stalled connections (e.g., model stops
    /// responding mid-stream), but does **not** limit the total time to
    /// complete the stream. For a total timeout, wrap the stream consumption
    /// in `tokio::time::timeout()`:
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # use futures_util::StreamExt;
    /// # use std::time::Duration;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api_key".to_string());
    /// let mut stream = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Write a story")
    ///     .create_stream();
    ///
    /// // Total timeout for entire stream consumption
    /// tokio::time::timeout(Duration::from_secs(60), async {
    ///     while let Some(chunk) = stream.next().await {
    ///         // process chunk...
    ///     }
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns errors if:
    /// - No input was provided
    /// - Neither model nor agent was specified
    /// - The API request fails
    /// - A chunk doesn't arrive within the timeout (if set)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use genai_rs::{Client, StreamChunk, StreamEvent};
    /// # use futures_util::StreamExt;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// let mut stream = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Count to 5")
    ///     .create_stream();
    ///
    /// while let Some(event) = stream.next().await {
    ///     let event = event?;
    ///     // event.event_id can be saved for stream resumption
    ///     match &event.chunk {
    ///         StreamChunk::StepDelta { delta, .. } => {
    ///             if let Some(text) = delta.as_text() {
    ///                 print!("{}", text);
    ///             }
    ///         }
    ///         StreamChunk::Completed(response) => {
    ///             println!("\nFinal response ID: {:?}", response.id);
    ///         }
    ///         _ => {} // Handle unknown future variants
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`GenaiError::Timeout`]: crate::GenaiError::Timeout
    pub fn create_stream(self) -> BoxStream<'a, Result<StreamEvent, GenaiError>> {
        let client = self.client;
        let timeout = self.timeout;
        Box::pin(async_stream::try_stream! {
            let request = self.build()?;
            let mut stream = client.execute_stream(request);

            loop {
                let next_chunk = stream.next();
                let result = match timeout {
                    Some(duration) => {
                        match tokio::time::timeout(duration, next_chunk).await {
                            Ok(Some(result)) => Some(result),
                            Ok(None) => None,
                            Err(_) => {
                                debug!("Stream chunk timed out after {:?}", duration);
                                Err(GenaiError::Timeout(duration))?;
                                unreachable!()
                            }
                        }
                    }
                    None => next_chunk.await,
                };

                match result {
                    Some(Ok(event)) => yield event,
                    Some(Err(e)) => Err(e)?,
                    None => break,
                }
            }
        })
    }

    /// Builds the [`InteractionRequest`] without executing it.
    ///
    /// Returns a fully-constructed request that can be:
    /// - Cloned for retry logic
    /// - Serialized for logging or replay
    /// - Inspected for debugging
    /// - Executed later via [`Client::execute()`](crate::Client::execute)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api_key".to_string());
    ///
    /// let request = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello!")
    ///     .build()?;
    ///
    /// // Clone, serialize, inspect
    /// let backup = request.clone();
    /// println!("{}", serde_json::to_string_pretty(&request)?);
    ///
    /// // Execute later
    /// // let response = client.execute(request).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] if:
    /// - No input was provided (via `with_text()`, `with_history()`, `with_content()`, etc.)
    /// - `with_content()` was combined with `with_history()` (mutually exclusive)
    /// - Neither model nor agent was specified
    /// - Both model and agent were specified (mutually exclusive)
    pub fn build(self) -> Result<InteractionRequest, GenaiError> {
        // Runtime validation for storage-related constraints
        self.validate()?;

        // A builder policy, not a wire constraint: say what the caller should
        // build instead.
        if self.content_input.is_some() && !self.history.is_empty() {
            return Err(GenaiError::InvalidInput(
                "Content input (with_content()) cannot be combined with with_history(). \
                 For multimodal multi-turn conversations, wrap the content in \
                 Step::user_input(...) and include it in the history instead."
                    .to_string(),
            ));
        }

        // Validate that agent_config is not set without agent
        if self.agent_config.is_some() && self.agent.is_none() {
            return Err(GenaiError::InvalidInput(
                "with_agent_config() requires with_agent(). \
                 Agent config is ignored when using with_model()."
                    .to_string(),
            ));
        }

        // Compose input from the separate fields
        // Priority: content_input > history > current_message
        // - content_input + current_message: merge (text prepended to content)
        // - history + current_message: merge (text appended as user_input step)
        let input = if let Some(mut content) = self.content_input {
            // Content input mode (single-turn multimodal)
            // If there's also a current_message, prepend it as text content
            if let Some(text) = self.current_message {
                content.insert(0, Content::text(text));
            }
            InteractionInput::Content(content)
        } else {
            // Text/history mode
            match (self.history.is_empty(), self.current_message) {
                (true, None) => {
                    return Err(GenaiError::InvalidInput(
                        "Input is required for interaction".to_string(),
                    ));
                }
                (true, Some(msg)) => InteractionInput::Text(msg),
                (false, None) => InteractionInput::Steps(self.history),
                (false, Some(msg)) => {
                    // Compose: history + current message as final user_input step
                    let mut steps = self.history;
                    steps.push(Step::user_text(msg));
                    InteractionInput::Steps(steps)
                }
            }
        };

        // Validate that we have either model or agent (but not both)
        match (&self.model, &self.agent) {
            (None, None) => {
                return Err(GenaiError::InvalidInput(
                    "Either model or agent must be specified".to_string(),
                ));
            }
            (Some(model), Some(agent)) => {
                return Err(GenaiError::InvalidInput(format!(
                    "Cannot specify both model ('{}') and agent ('{}') - use one or the other",
                    model, agent
                )));
            }
            _ => {} // Valid: exactly one is set
        }

        // Merge speech_configs into generation_config if present
        let generation_config = match (self.generation_config, self.speech_configs) {
            (Some(mut config), Some(speech)) => {
                config.speech_config = Some(speech);
                Some(config)
            }
            (None, Some(speech)) => Some(GenerationConfig {
                speech_config: Some(speech),
                ..Default::default()
            }),
            (config, None) => config,
        };

        Ok(InteractionRequest {
            model: self.model,
            agent: self.agent,
            agent_config: self.agent_config,
            input,
            previous_interaction_id: self.previous_interaction_id,
            tools: self.tools,
            response_modalities: self.response_modalities,
            response_format: self.response_format,
            generation_config,
            stream: None, // Set by the transport: execute() vs execute_stream()
            background: self.background,
            store: self.store,
            system_instruction: self.system_instruction,
            service_tier: self.service_tier,
            webhook_config: self.webhook_config,
            environment: self.environment,
            safety_settings: self.safety_settings,
            labels: self.labels,
        })
    }
}

#[cfg(test)]
mod tests;
