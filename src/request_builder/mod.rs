mod auto_functions;

use auto_functions::DEFAULT_MAX_FUNCTION_CALL_LOOPS;

use crate::GenaiError;
use crate::client::Client;
use crate::function_calling::ToolService;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

use crate::{
    AgentConfig, Content, DeepResearchConfig, FunctionCallingMode, FunctionDeclaration,
    GenerationConfig, ImageConfig, InteractionInput, InteractionRequest, InteractionResponse, Role,
    SpeechConfig, StreamEvent, ThinkingLevel, ThinkingSummaries, Tool as InternalTool, Turn,
    TurnContent,
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
///     .with_model("gemini-3-flash-preview")
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
///     .with_model("gemini-3-flash-preview")
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
    /// Conversation history (set by `with_history()`)
    history: Vec<Turn>,
    /// Current user message (set by `with_text()`)
    current_message: Option<String>,
    /// Content input for function results (set by `with_content()`)
    content_input: Option<Vec<Content>>,
    previous_interaction_id: Option<String>,
    tools: Option<Vec<InternalTool>>,
    response_modalities: Option<Vec<String>>,
    response_format: Option<serde_json::Value>,
    response_mime_type: Option<String>,
    generation_config: Option<GenerationConfig>,
    speech_config: Option<SpeechConfig>,
    background: Option<bool>,
    store: Option<bool>,
    system_instruction: Option<InteractionInput>,
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
            .field("response_mime_type", &self.response_mime_type)
            .field("generation_config", &self.generation_config)
            .field("speech_config", &self.speech_config)
            .field("background", &self.background)
            .field("store", &self.store)
            .field("system_instruction", &self.system_instruction)
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
            response_mime_type: None,
            generation_config: None,
            speech_config: None,
            background: None,
            store: None,
            system_instruction: None,
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

    /// Sets the model to use for this interaction (e.g., "gemini-3-flash-preview").
    ///
    /// Note: Mutually exclusive with `with_agent()`.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the agent to use for this interaction (e.g., "deep-research-pro-preview-12-2025").
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
    ///     .with_agent("deep-research-pro-preview-12-2025")
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
    /// Only applicable when using `with_agent("deep-research-pro-preview-12-2025")`.
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
    ///     .with_agent("deep-research-pro-preview-12-2025")
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
    /// - `InteractionInput::Turns(turns)` → `with_history(turns)`
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
            InteractionInput::Turns(turns) => {
                self.history = turns;
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
    /// are composed into `[...history, Turn::user(current_message)]`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use genai_rs::{Client, Turn};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // Simple single-turn message
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Hello!")
    ///     .create()
    ///     .await?;
    ///
    /// // With conversation history - both orders are equivalent
    /// let history = vec![
    ///     Turn::user("What is 2+2?"),
    ///     Turn::model("4"),
    /// ];
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_history(history)
    ///     .with_text("And times 3?")  // Appended as final user turn
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
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_system_instruction("You are a helpful assistant specializing in Rust")
    ///     .with_text("Hello!")
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_system_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.system_instruction = Some(InteractionInput::Text(instruction.into()));
        self
    }

    /// Sets the input from a vector of content objects, replacing any existing content.
    ///
    /// This is useful for building multi-part inputs or for sending function results.
    ///
    /// # Panics / Errors
    ///
    /// Calling `build()` will return an error if `with_content()` is combined with
    /// `with_history()`. If you need to combine function results with conversation
    /// history, include them in the history via [`Turn`] objects instead.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, Content};
    /// # use serde_json::json;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// let result = Content::function_result("my_func", "call_123", json!({"data": "result"}));
    ///
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_content(vec![result])
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

    /// Sets the conversation history from an explicit array of turns.
    ///
    /// This can be combined with [`with_text()`](Self::with_text) to build a conversation:
    /// - `with_history()` sets the conversation history (previous turns)
    /// - `with_text()` sets the current user message to append
    ///
    /// The order doesn't matter - at build time, the history and current message
    /// are composed into `[...history, Turn::user(current_message)]`.
    ///
    /// This enables multi-turn conversations without relying on server-side
    /// storage via `previous_interaction_id`. Useful for:
    /// - Stateless deployments
    /// - Migrating conversations from other providers
    /// - Custom history management (e.g., sliding window, summarization)
    /// - Testing with controlled conversation states
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Turn};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// // History-only (last turn must be a user message)
    /// let history = vec![
    ///     Turn::user("What is 2+2?"),
    ///     Turn::model("2+2 equals 4."),
    ///     Turn::user("And what's that times 3?"),
    /// ];
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_history(history)
    ///     .create()
    ///     .await?;
    ///
    /// // History + current message (order doesn't matter)
    /// let history = vec![
    ///     Turn::user("What is 2+2?"),
    ///     Turn::model("4"),
    /// ];
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_history(history)
    ///     .with_text("And times 3?")  // Appended as final user turn
    ///     .create()
    ///     .await?;
    ///
    /// println!("{}", response.as_text().unwrap_or("No response"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_history(mut self, turns: Vec<Turn>) -> Self {
        self.history = turns;
        self
    }

    /// Starts building a conversation with a fluent API.
    ///
    /// Returns a [`ConversationBuilder`] that allows chaining `.user()` and `.model()`
    /// calls to construct a multi-turn conversation. Call `.done()` to return to
    /// the [`InteractionBuilder`].
    ///
    /// This is an alternative to [`with_history()`] that provides a more readable
    /// syntax for constructing conversations inline.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .conversation()
    ///         .user("What is 2+2?")
    ///         .model("2+2 equals 4.")
    ///         .user("And what's that times 3?")
    ///         .done()
    ///     .create()
    ///     .await?;
    ///
    /// println!("{}", response.as_text().unwrap_or("No response"));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`with_history()`]: InteractionBuilder::with_history
    #[must_use]
    pub fn conversation(self) -> ConversationBuilder<'a> {
        ConversationBuilder {
            parent: self,
            turns: Vec::new(),
        }
    }

    /// Internal helper to push a tool to the tools list.
    fn push_tool(&mut self, tool: InternalTool) {
        self.tools.get_or_insert_with(Vec::new).push(tool);
    }

    /// Adds any tool that implements `Into<Tool>` to the interaction.
    ///
    /// This is the unified entry point for configurable tools. Use the corresponding
    /// config struct to construct the tool:
    ///
    /// - [`GoogleSearchConfig`](crate::GoogleSearchConfig) for Google Search with search types
    /// - [`GoogleMapsConfig`](crate::GoogleMapsConfig) for Google Maps
    /// - [`McpServerConfig`](crate::McpServerConfig) for MCP servers
    /// - [`ComputerUseConfig`](crate::ComputerUseConfig) for browser automation
    /// - [`FileSearchConfig`](crate::FileSearchConfig) for file search
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, ComputerUseConfig, FileSearchConfig, McpServerConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Hello")
    ///     .add_tool(ComputerUseConfig::new().excluding(vec!["download_file".to_string()]))
    ///     .add_tool(FileSearchConfig::new(vec!["docs".to_string()]).with_top_k(5))
    ///     .add_tool(McpServerConfig::new("fs", "https://mcp.example.com/fs"))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn add_tool(mut self, tool: impl Into<InternalTool>) -> Self {
        self.push_tool(tool.into());
        self
    }

    /// Sets the tools for function calling, replacing any existing tools.
    ///
    /// Use `add_function()` to accumulate functions instead of replacing.
    #[must_use]
    pub fn set_tools(mut self, tools: Vec<InternalTool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Adds a single function declaration to the request.
    ///
    /// This method can be called multiple times to accumulate functions.
    /// Each function is converted into a [`crate::Tool`] and added to the request.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, FunctionDeclaration};
    /// use serde_json::json;
    ///
    /// let client = Client::new("api-key".to_string());
    ///
    /// let func = FunctionDeclaration::builder("get_temperature")
    ///     .description("Get the temperature for a location")
    ///     .parameter("location", json!({"type": "string"}))
    ///     .required(vec!["location".to_string()])
    ///     .build();
    ///
    /// let builder = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What's the temperature in Paris?")
    ///     .add_function(func);
    /// ```
    #[must_use]
    pub fn add_function(mut self, function: FunctionDeclaration) -> Self {
        self.push_tool(function.into_tool());
        self
    }

    /// Adds multiple function declarations to the request at once.
    ///
    /// This is a convenience method equivalent to calling [`add_function`] multiple times.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, FunctionDeclaration};
    ///
    /// let client = Client::new("api-key".to_string());
    ///
    /// let functions = vec![
    ///     FunctionDeclaration::builder("get_weather").build(),
    ///     FunctionDeclaration::builder("get_time").build(),
    /// ];
    ///
    /// let builder = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What's the weather and time?")
    ///     .add_functions(functions);
    /// ```
    ///
    /// [`add_function`]: InteractionBuilder::add_function
    #[must_use]
    pub fn add_functions(mut self, functions: Vec<FunctionDeclaration>) -> Self {
        for func in functions {
            self.push_tool(func.into_tool());
        }
        self
    }

    /// Sets a tool service for dependency-injected functions.
    ///
    /// Use this when your tool functions need access to shared state like
    /// database connections, API clients, or configuration. The service
    /// provides callable functions that can access the service's internal state.
    ///
    /// Tools from the service are used in addition to any auto-discovered
    /// tools from the global registry (via `#[tool]` macro).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use genai_rs::{Client, ToolService, CallableFunction};
    /// use std::sync::Arc;
    ///
    /// struct MyService { db: Database }
    ///
    /// impl ToolService for MyService {
    ///     fn tools(&self) -> Vec<Arc<dyn CallableFunction>> {
    ///         vec![Arc::new(QueryTool { db: self.db.clone() })]
    ///     }
    /// }
    ///
    /// let service = Arc::new(MyService { db: Database::new() });
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_tool_service(service)
    ///     .with_text("Query the database for users")
    ///     .create_with_auto_functions()
    ///     .await?;
    /// ```
    #[must_use]
    pub fn with_tool_service(mut self, service: Arc<dyn ToolService>) -> Self {
        self.tool_service = Some(service);
        self
    }

    /// Enables Google Search grounding for this interaction.
    ///
    /// This adds the built-in `GoogleSearch` tool which allows the model to
    /// search the web and ground its responses in real-time information.
    /// Grounding metadata will be available in the response via
    /// [`InteractionResponse::google_search_metadata`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Who won the 2024 World Series?")
    ///     .with_google_search()
    ///     .create()
    ///     .await?;
    ///
    /// // Access grounding metadata
    /// if let Some(metadata) = response.google_search_metadata() {
    ///     println!("Search queries: {:?}", metadata.web_search_queries);
    ///     for chunk in &metadata.grounding_chunks {
    ///         println!("Source: {}", chunk.web.uri);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`InteractionResponse::google_search_metadata`]: crate::InteractionResponse::google_search_metadata
    #[must_use]
    pub fn with_google_search(mut self) -> Self {
        self.push_tool(InternalTool::GoogleSearch { search_types: None });
        self
    }

    /// Enables the Google Maps built-in tool for location-grounded responses.
    ///
    /// For configuration options (e.g., widget support), use
    /// `.add_tool(GoogleMapsConfig::new().with_widget())`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Find coffee shops near Times Square")
    ///     .with_google_maps()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_google_maps(mut self) -> Self {
        self.push_tool(InternalTool::GoogleMaps {
            enable_widget: None,
        });
        self
    }

    /// Enables code execution for this interaction.
    ///
    /// This adds the built-in `CodeExecution` tool which allows the model to
    /// write and execute Python code to help answer questions. The code runs
    /// in a sandboxed environment on Google's servers.
    ///
    /// # Security Considerations
    ///
    /// Code execution runs in a sandboxed environment with the following
    /// limitations:
    /// - Maximum execution time: 30 seconds
    /// - No network access
    /// - Limited file I/O capabilities
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Calculate the factorial of 50")
    ///     .with_code_execution()
    ///     .create()
    ///     .await?;
    ///
    /// // Access code execution results
    /// for result in response.code_execution_results() {
    ///     if !result.is_error {
    ///         println!("Code output: {}", result.result);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_code_execution(mut self) -> Self {
        self.push_tool(InternalTool::CodeExecution);
        self
    }

    /// Enables URL context fetching for this interaction.
    ///
    /// This adds the built-in `UrlContext` tool which allows the model to
    /// fetch and analyze content from URLs provided in the prompt.
    /// URL context metadata will be available in the response via
    /// [`InteractionResponse::url_context_metadata`].
    ///
    /// # Limitations
    ///
    /// - Maximum 20 URLs per request
    /// - Maximum 34MB content size per URL
    /// - Unsupported: paywalled content, YouTube, Google Workspace files, video/audio
    /// - Retrieved content counts toward input token usage
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Summarize the content from https://example.com")
    ///     .with_url_context()
    ///     .create()
    ///     .await?;
    ///
    /// // Access URL context metadata
    /// if let Some(metadata) = response.url_context_metadata() {
    ///     for entry in &metadata.url_metadata {
    ///         println!("URL: {} - Status: {:?}",
    ///             entry.retrieved_url,
    ///             entry.url_retrieval_status);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`InteractionResponse::url_context_metadata`]: crate::InteractionResponse::url_context_metadata
    #[must_use]
    pub fn with_url_context(mut self) -> Self {
        self.push_tool(InternalTool::UrlContext);
        self
    }

    /// Sets response modalities (e.g., ["IMAGE"]).
    #[must_use]
    pub fn with_response_modalities(mut self, modalities: Vec<String>) -> Self {
        self.response_modalities = Some(modalities);
        self
    }

    /// Configures the request to return image output.
    ///
    /// This is a convenience method equivalent to:
    /// ```ignore
    /// .with_response_modalities(vec!["IMAGE".to_string()])
    /// ```
    ///
    /// Use this when you want the model to generate images. Requires a model
    /// that supports image generation (e.g., `gemini-3-pro-image-preview`).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-pro-image-preview")
    ///     .with_text("A cute cat playing with yarn")
    ///     .with_image_output()
    ///     .create()
    ///     .await?;
    ///
    /// // Extract generated image
    /// if let Some(bytes) = response.first_image_bytes()? {
    ///     std::fs::write("cat.png", &bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_image_output(self) -> Self {
        self.with_response_modalities(vec!["IMAGE".to_string()])
    }

    /// Configures the request to return audio output.
    ///
    /// This is a convenience method equivalent to:
    /// ```ignore
    /// .with_response_modalities(vec!["AUDIO".to_string()])
    /// ```
    ///
    /// Use this when you want the model to generate speech audio. Requires a model
    /// that supports text-to-speech (e.g., `gemini-2.5-pro-preview-tts`).
    ///
    /// For voice customization, chain with [`with_speech_config`](Self::with_speech_config)
    /// or [`with_voice`](Self::with_voice).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-2.5-pro-preview-tts")
    ///     .with_text("Hello, world! Welcome to text-to-speech.")
    ///     .with_audio_output()
    ///     .with_voice("Kore")
    ///     .create()
    ///     .await?;
    ///
    /// // Extract generated audio using the helper methods
    /// if let Some(audio) = response.first_audio() {
    ///     let bytes = audio.bytes()?;
    ///     std::fs::write("speech.wav", &bytes)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_audio_output(self) -> Self {
        self.with_response_modalities(vec!["AUDIO".to_string()])
    }

    /// Sets speech configuration for text-to-speech output.
    ///
    /// Use this to customize voice, language, and speaker settings when
    /// generating audio output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, SpeechConfig};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let config = SpeechConfig {
    ///     voice: Some("Puck".to_string()),
    ///     language: Some("en-US".to_string()),
    ///     speaker: None,
    /// };
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-2.5-pro-preview-tts")
    ///     .with_text("Hello from Puck!")
    ///     .with_audio_output()
    ///     .with_speech_config(config)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_speech_config(mut self, config: SpeechConfig) -> Self {
        self.speech_config = Some(config);
        self
    }

    /// Sets the image generation configuration.
    ///
    /// Controls aspect ratio and size for image generation output.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, ImageConfig, ImageAspectRatio, ImageSize};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let config = ImageConfig {
    ///     aspect_ratio: Some(ImageAspectRatio::Widescreen16x9),
    ///     image_size: Some(ImageSize::Hd2k),
    /// };
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-pro-image-preview")
    ///     .with_text("Generate a landscape photo")
    ///     .with_image_config(config)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_image_config(mut self, config: ImageConfig) -> Self {
        let gen_config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        gen_config.image_config = Some(config);
        self
    }

    /// Sets the voice for text-to-speech output (defaults to en-US language).
    ///
    /// This is a convenience method that sets the voice with a default language of "en-US".
    /// For other languages, use [`with_speech_config`](Self::with_speech_config).
    ///
    /// # Available Voices
    ///
    /// Common voices include: Aoede, Charon, Fenrir, Kore, Puck, and others.
    /// See [Google's TTS documentation](https://ai.google.dev/gemini-api/docs/text-generation)
    /// for the full list.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-2.5-pro-preview-tts")
    ///     .with_text("Hello, world!")
    ///     .with_audio_output()
    ///     .with_voice("Kore")
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_voice(self, voice: impl Into<String>) -> Self {
        // Language is required by the API, default to en-US
        self.with_speech_config(SpeechConfig::with_voice_and_language(voice, "en-US"))
    }

    /// Sets a JSON schema to enforce structured output from the model.
    ///
    /// When you provide a JSON schema, the model will return responses that
    /// conform exactly to your schema structure. This is useful for:
    /// - Extracting structured data from text
    /// - Building reliable data pipelines
    /// - Ensuring consistent API responses
    ///
    /// The schema should be a standard JSON Schema object with `type`, `properties`,
    /// and optionally `required` fields.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    /// use serde_json::json;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let schema = json!({
    ///     "type": "object",
    ///     "properties": {
    ///         "name": {"type": "string"},
    ///         "age": {"type": "integer"},
    ///         "hobbies": {
    ///             "type": "array",
    ///             "items": {"type": "string"}
    ///         }
    ///     },
    ///     "required": ["name", "age"]
    /// });
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Generate info for someone named Alice who is 30 and likes hiking")
    ///     .with_response_format(schema)
    ///     .create()
    ///     .await?;
    ///
    /// // Response is guaranteed to be valid JSON matching the schema
    /// let text = response.as_text().unwrap();
    /// let data: serde_json::Value = serde_json::from_str(text)?;
    /// println!("Name: {}", data["name"]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Combining with Tools
    ///
    /// Structured output can be combined with built-in tools like Google Search
    /// or URL Context to get structured data from real-time sources:
    ///
    /// ```no_run
    /// # use genai_rs::Client;
    /// # use serde_json::json;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = Client::new("api-key".to_string());
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("What is the current weather in Tokyo?")
    ///     .with_google_search()
    ///     .with_response_format(json!({
    ///         "type": "object",
    ///         "properties": {
    ///             "temperature": {"type": "string"},
    ///             "conditions": {"type": "string"}
    ///         },
    ///         "required": ["temperature", "conditions"]
    ///     }))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_response_format(mut self, format: serde_json::Value) -> Self {
        self.response_format = Some(format);
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
    ///     .with_model("gemini-3-flash-preview")
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
    ///     .with_model("gemini-3-flash-preview")
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
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Generate a random number")
    ///     .with_seed(42)
    ///     .create()
    ///     .await?;
    ///
    /// let response2 = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
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
    ///     .with_model("gemini-3-flash-preview")
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

    /// Sets the function calling mode.
    ///
    /// Controls how the model uses function calling capabilities.
    ///
    /// # Modes
    ///
    /// - `Auto` (default): Model decides whether to call functions or respond naturally
    /// - `Any`: Model must call a function; guarantees schema adherence for calls
    /// - `None`: Prohibits function calling entirely
    /// - `Validated` (Preview): Ensures either function calls OR natural language adhere to schema
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::{Client, FunctionCallingMode};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    ///
    /// // Force the model to use a function
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Get weather in Tokyo")
    ///     .with_function_calling_mode(FunctionCallingMode::Any)
    ///     .create()
    ///     .await?;
    ///
    /// // Use VALIDATED mode for guaranteed schema adherence
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Get weather in Tokyo")
    ///     .with_function_calling_mode(FunctionCallingMode::Validated)
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_function_calling_mode(mut self, mode: FunctionCallingMode) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.tool_choice = Some(mode);
        self
    }

    /// Restricts the model to only calling the named tools.
    ///
    /// When set, the model can only call functions whose names appear in
    /// the provided list, even if other tools are declared.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Get weather in Tokyo")
    ///     .with_allowed_tools(vec!["get_weather".to_string()])
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_allowed_tools(mut self, tool_names: Vec<String>) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.allowed_tools = Some(tool_names);
        self
    }

    /// Sets the response MIME type for structured output.
    ///
    /// Required when using `with_response_format()` with a JSON schema.
    /// Typically "application/json" for structured JSON output.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::Client;
    /// # use serde_json::json;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api-key".to_string()).build()?;
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Generate user data")
    ///     .with_response_mime_type("application/json")
    ///     .with_response_format(json!({
    ///         "type": "object",
    ///         "properties": {
    ///             "name": {"type": "string"},
    ///             "age": {"type": "integer"}
    ///         }
    ///     }))
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_response_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.response_mime_type = Some(mime_type.into());
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

    /// Sets the maximum number of function call loops for `create_with_auto_functions()`.
    ///
    /// Default is 5. Increase for complex multi-step function calling scenarios,
    /// or decrease to fail faster if the model is stuck in a loop.
    ///
    /// # Example
    /// ```no_run
    /// # use genai_rs::Client;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder("api_key".to_string()).build()?;
    ///
    /// let response = client.interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Complex multi-step task")
    ///     .with_max_function_call_loops(10)  // Allow up to 10 iterations
    ///     .create_with_auto_functions()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_max_function_call_loops(mut self, max_loops: usize) -> Self {
        if max_loops == 0 {
            tracing::warn!(
                "max_function_call_loops set to 0 - auto function calling will immediately fail \
                 if the model returns any function calls. Consider using create() instead of \
                 create_with_auto_functions() if you don't want automatic function execution."
            );
        }
        self.max_function_call_loops = max_loops;
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
    ///     .with_model("gemini-3-flash-preview")
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
    /// - `StreamChunk::Delta`: Incremental content (text or thought)
    /// - `StreamChunk::Complete`: The final complete interaction response
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
    ///     .with_model("gemini-3-flash-preview")
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
    ///     .with_model("gemini-3-flash-preview")
    ///     .with_text("Count to 5")
    ///     .create_stream();
    ///
    /// while let Some(event) = stream.next().await {
    ///     let event = event?;
    ///     // event.event_id can be saved for stream resumption
    ///     match &event.chunk {
    ///         StreamChunk::Delta(delta) => {
    ///             if let Some(text) = delta.as_text() {
    ///                 print!("{}", text);
    ///             }
    ///         }
    ///         StreamChunk::Complete(response) => {
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
            let mut request = self.build()?;
            request.stream = Some(true);
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
    ///     .with_model("gemini-3-flash-preview")
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

        // Validate that content input is not combined with history
        // Content input is fundamentally incompatible with multi-turn history
        if self.content_input.is_some() && !self.history.is_empty() {
            return Err(GenaiError::InvalidInput(
                "Content input (with_content()) cannot be combined with with_history(). \
                 For multimodal multi-turn conversations, build Turn objects with content arrays \
                 instead."
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
        // - history + current_message: merge (text appended as user turn)
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
                (false, None) => InteractionInput::Turns(self.history),
                (false, Some(msg)) => {
                    // Compose: history + current message as final user turn
                    let mut turns = self.history;
                    turns.push(Turn::user(msg));
                    InteractionInput::Turns(turns)
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

        // Merge speech_config into generation_config if present
        let generation_config = match (self.generation_config, self.speech_config) {
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
            response_mime_type: self.response_mime_type,
            generation_config,
            stream: None, // Set by create() vs create_stream()
            background: self.background,
            store: self.store,
            system_instruction: self.system_instruction,
        })
    }
}

// ============================================================================
// ConversationBuilder - Fluent API for building multi-turn conversations
// ============================================================================

/// Builder for constructing multi-turn conversations with a fluent API.
///
/// Created via [`InteractionBuilder::conversation()`]. Allows chaining `.user()` and
/// `.model()` calls to build a conversation history, then `.done()` to return to
/// the parent builder.
///
/// # Example
///
/// ```no_run
/// use genai_rs::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new("api-key".to_string());
///
/// let response = client
///     .interaction()
///     .with_model("gemini-3-flash-preview")
///     .conversation()
///         .user("What is the capital of France?")
///         .model("The capital of France is Paris.")
///         .user("What's the population?")
///         .done()
///     .create()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct ConversationBuilder<'a> {
    parent: InteractionBuilder<'a>,
    turns: Vec<Turn>,
}

impl<'a> ConversationBuilder<'a> {
    /// Adds a user message to the conversation.
    ///
    /// Accepts any type that can be converted to [`TurnContent`], including:
    /// - `&str` or `String` for text content
    /// - `Vec<Content>` for multimodal content
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .conversation()
    ///         .user("Hello!")
    ///         .done()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn user(mut self, content: impl Into<TurnContent>) -> Self {
        self.turns.push(Turn::user(content));
        self
    }

    /// Adds a model message to the conversation.
    ///
    /// Use this to include previous model responses in the conversation history.
    /// The model will use this context when generating its next response.
    ///
    /// Accepts any type that can be converted to [`TurnContent`], including:
    /// - `&str` or `String` for text content
    /// - `Vec<Content>` for multimodal content
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .conversation()
    ///         .user("What is 2+2?")
    ///         .model("2+2 equals 4.")
    ///         .user("Multiply that by 3")
    ///         .done()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn model(mut self, content: impl Into<TurnContent>) -> Self {
        self.turns.push(Turn::model(content));
        self
    }

    /// Adds a turn with an explicit role.
    ///
    /// This is useful when you need to dynamically construct conversations
    /// where the role is determined at runtime.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::{Client, Role};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let role = Role::User; // Determined at runtime
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .conversation()
    ///         .turn(role, "Dynamic message")
    ///         .done()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn turn(mut self, role: Role, content: impl Into<TurnContent>) -> Self {
        self.turns.push(Turn::new(role, content));
        self
    }

    /// Finishes building the conversation and returns to the parent [`InteractionBuilder`].
    ///
    /// The accumulated turns are set as the input for the interaction.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use genai_rs::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new("api-key".to_string());
    ///
    /// let response = client
    ///     .interaction()
    ///     .with_model("gemini-3-flash-preview")
    ///     .conversation()
    ///         .user("Hello!")
    ///         .done()  // Returns to InteractionBuilder
    ///     .create()    // Now we can call create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn done(self) -> InteractionBuilder<'a> {
        let mut parent = self.parent;
        parent.history = self.turns;
        parent
    }
}

#[cfg(test)]
mod tests;
