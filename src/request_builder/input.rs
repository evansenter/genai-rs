use super::InteractionBuilder;
use crate::{AgentConfig, Content, DeepResearchConfig, InteractionInput, Step, ThinkingSummaries};

impl<'a> InteractionBuilder<'a> {
    /// References a previous interaction for stateful conversations.
    ///
    /// The interaction will have access to the context from the previous interaction.
    ///
    /// # Runtime Validation
    ///
    /// This is incompatible with `with_store_disabled()` - chained interactions require
    /// storage. Calling both will return an error from `build()` or `create()`.
    ///
    /// # Important: System Instructions
    ///
    /// The API does NOT inherit system instructions via `previousInteractionId`.
    /// You must call `with_system_instruction()` on each turn if needed.
    #[must_use]
    pub fn with_previous_interaction(mut self, id: impl Into<String>) -> Self {
        self.previous_interaction_id = Some(id.into());
        self
    }

    /// Explicitly disables storage for this interaction.
    ///
    /// When `store` is `false`, the interaction will not be stored and cannot be
    /// referenced by future interactions via `previousInteractionId`.
    ///
    /// # Runtime Validation
    ///
    /// This is incompatible with:
    /// - `with_previous_interaction()`: chained interactions require storage
    /// - `with_background(true)`: background execution requires storage
    /// - `create_with_auto_functions()`: auto-function calling requires storage
    ///
    /// Combining these will return an error from `build()` or `create()`.
    #[must_use]
    pub fn with_store_disabled(mut self) -> Self {
        self.store = Some(false);
        self
    }

    /// Enables background execution for this interaction.
    ///
    /// Background execution allows long-running operations to continue after
    /// the initial API response. Only supported for agents.
    ///
    /// # Runtime Validation
    ///
    /// This is incompatible with `with_store_disabled()` - background execution
    /// requires storage. Combining these will return an error from `build()` or `create()`.
    #[must_use]
    pub fn with_background(mut self, background: bool) -> Self {
        self.background = Some(background);
        self
    }

    /// Sets the model to use for this interaction (e.g. [`DEFAULT_MODEL`](crate::DEFAULT_MODEL)).
    ///
    /// Note: Mutually exclusive with `with_agent()`.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the agent to use for this interaction (e.g. [`DEFAULT_DEEP_RESEARCH_AGENT`](crate::DEFAULT_DEEP_RESEARCH_AGENT)).
    ///
    /// Note: Mutually exclusive with `with_model()`.
    #[must_use]
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = Some(agent.into());
        self
    }

    /// Sets the agent configuration for specialized agents.
    ///
    /// This configures agent-specific behavior. Only applicable when using
    /// `with_agent()` with specialized agents like Deep Research or Dynamic.
    ///
    /// Accepts typed config structs (recommended) or raw `AgentConfig`.
    ///
    /// # Example with typed config (recommended)
    ///
    /// ```no_run
    /// use genai_rs::{Client, DeepResearchConfig, ThinkingSummaries};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
    ///     .with_text("Research the history of quantum computing")
    ///     .with_agent_config(DeepResearchConfig::new()
    ///         .with_thinking_summaries(ThinkingSummaries::Auto))
    ///     .with_background(true)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Example with raw JSON (for unknown/future agents)
    ///
    /// ```no_run
    /// use genai_rs::{Client, AgentConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_agent("future-agent-2026")
    ///     .with_text("Do something new")
    ///     .with_agent_config(AgentConfig::from_value(serde_json::json!({
    ///         "type": "future-agent",
    ///         "newOption": true
    ///     })))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_agent_config(mut self, config: impl Into<AgentConfig>) -> Self {
        self.agent_config = Some(config.into());
        self
    }

    /// Configures the Deep Research agent with thinking summaries.
    ///
    /// This is a convenience method equivalent to:
    /// ```ignore
    /// .with_agent_config(DeepResearchConfig::new()
    ///     .with_thinking_summaries(summaries))
    /// ```
    ///
    /// Only applicable to Deep Research agents (e.g. [`DEFAULT_DEEP_RESEARCH_AGENT`](crate::DEFAULT_DEEP_RESEARCH_AGENT)).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, ThinkingSummaries};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_agent(genai_rs::DEFAULT_DEEP_RESEARCH_AGENT)
    ///     .with_text("Research the history of quantum computing")
    ///     .with_deep_research_config(ThinkingSummaries::Auto)
    ///     .with_background(true)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_deep_research_config(mut self, thinking_summaries: ThinkingSummaries) -> Self {
        self.agent_config = Some(
            DeepResearchConfig::new()
                .with_thinking_summaries(thinking_summaries)
                .into(),
        );
        self
    }

    /// Sets the input for this interaction from an `InteractionInput`.
    ///
    /// This is a convenience method that dispatches to the appropriate setter:
    /// - `InteractionInput::Text(text)` → `with_text(text)`
    /// - `InteractionInput::Content(content)` → `with_content(content)`
    /// - `InteractionInput::Steps(steps)` → `with_history(steps)`
    ///
    /// For direct usage, prefer the specific methods (`with_text()`, `with_content()`,
    /// `with_history()`) for clarity.
    #[must_use]
    pub fn with_input(mut self, input: InteractionInput) -> Self {
        match input {
            InteractionInput::Text(text) => {
                self.current_message = Some(text);
            }
            InteractionInput::Content(content) => {
                self.content_input = Some(content);
            }
            InteractionInput::Steps(steps) => {
                self.history = steps;
            }
        }
        self
    }

    /// Sets the current user message for this interaction.
    ///
    /// This can be combined with [`with_history()`](Self::with_history) to build a conversation:
    /// - `with_history()` sets the conversation history (previous turns)
    /// - `with_text()` sets the current user message to append
    ///
    /// The order doesn't matter - at build time, the history and current message
    /// are composed into `[...history, Step::user_text(current_message)]`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Client, Step};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Simple single-turn message
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello!")
    ///     .create()
    ///     .await?;
    ///
    /// // With conversation history - both orders are equivalent
    /// let history = vec![
    ///     Step::user_text("What is 2+2?"),
    ///     Step::model_text("4"),
    /// ];
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_history(history)
    ///     .with_text("And times 3?")  // Appended as final user_input step
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.current_message = Some(text.into());
        self
    }

    /// Sets a system instruction for the model.
    ///
    /// System instructions provide context or guidelines for the model's behavior
    /// throughout the interaction.
    ///
    /// # Note on Multi-Turn Conversations
    ///
    /// The Gemini API does NOT inherit system instructions via `previousInteractionId`.
    /// You must explicitly set the system instruction on each turn where you want it
    /// to apply.
    ///
    /// For `create_with_auto_functions()`, the system instruction is automatically
    /// included on all turns within the auto-function loop (the request is reused).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_system_instruction("You are a helpful assistant specializing in Rust")
    ///     .with_text("Hello!")
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_system_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.system_instruction = Some(instruction.into());
        self
    }

    /// Sets the input from a vector of content objects, replacing any existing content.
    ///
    /// This is useful for building multi-part inputs or for sending function results.
    ///
    /// # Panics / Errors
    ///
    /// Calling `build()` will return an error if `with_content()` is combined with
    /// `with_history()`. To combine multimodal content with conversation
    /// history, wrap the content in a [`Step::user_input`] and use
    /// [`with_history()`](Self::with_history) instead. For function results,
    /// use `with_history(vec![Step::function_result(...)])`.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, Content};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_content(vec![
    ///         Content::text("Describe this image"),
    ///         Content::image_uri("files/abc", "image/png"),
    ///     ])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_content(mut self, content: Vec<Content>) -> Self {
        self.content_input = Some(content);
        self
    }

    /// Sets the conversation history from an explicit array of steps.
    ///
    /// This can be combined with [`with_text()`](Self::with_text) to build a conversation:
    /// - `with_history()` sets the conversation history (previous steps)
    /// - `with_text()` sets the current user message to append
    ///
    /// The order doesn't matter - at build time, the history and current message
    /// are composed into `[...history, Step::user_text(current_message)]`.
    ///
    /// This enables multi-turn conversations without relying on server-side
    /// storage via `previous_interaction_id`. Useful for:
    /// - Stateless deployments
    /// - Migrating conversations from other providers
    /// - Custom history management (e.g., sliding window, summarization)
    /// - Testing with controlled conversation states
    ///
    /// Steps from a previous response (including `thought` steps whose
    /// signatures validate the reasoning chain) can be replayed directly via
    /// [`InteractionResponse::output_steps()`](crate::InteractionResponse::output_steps).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Step};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // History-only (last step should be a user message)
    /// let history = vec![
    ///     Step::user_text("What is 2+2?"),
    ///     Step::model_text("2+2 equals 4."),
    ///     Step::user_text("And what's that times 3?"),
    /// ];
    /// let response = client.interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_history(history)
    ///     .create()
    ///     .await?;
    ///
    /// println!("{}", response.as_text().unwrap_or("No response"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_history(mut self, steps: Vec<Step>) -> Self {
        self.history = steps;
        self
    }

    /// Explicitly enables storage for this interaction.
    ///
    /// Storage is enabled by default, so this method is typically only needed
    /// to be explicit about intent or to re-enable after conditional logic.
    ///
    /// When storage is enabled:
    /// - The response will include an `id` field
    /// - The interaction can be retrieved later with `get_interaction()`
    /// - The interaction can be referenced via `with_previous_interaction()` in follow-up requests
    /// - Auto-function calling (`create_with_auto_functions()`) will work
    ///
    /// # See Also
    ///
    /// Use [`with_store_disabled()`](Self::with_store_disabled) to disable storage.
    #[must_use]
    pub fn with_store_enabled(mut self) -> Self {
        self.store = Some(true);
        self
    }
}
