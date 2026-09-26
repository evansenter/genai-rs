use super::InteractionBuilder;
use crate::function_calling::ToolService;
use crate::{
    FunctionCallingMode, FunctionDeclaration, GenerationConfig, Tool as InternalTool, ToolChoice,
};
use std::sync::Arc;

impl<'a> InteractionBuilder<'a> {
    /// Internal helper to push a tool to the tools list.
    fn push_tool(&mut self, tool: InternalTool) {
        self.tools.get_or_insert_with(Vec::new).push(tool);
    }

    /// Replaces any tool of the same kind as `tool` (the `with_*` built-in
    /// tool setters), so calling one twice does not send a duplicate.
    fn replace_tool(&mut self, tool: InternalTool) {
        let kind = std::mem::discriminant(&tool);
        let tools = self.tools.get_or_insert_with(Vec::new);
        tools.retain(|existing| std::mem::discriminant(existing) != kind);
        tools.push(tool);
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Hello")
    ///     .add_tool(ComputerUseConfig::new().with_excluded_predefined_functions(vec!["download_file".to_string()]))
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
    /// Use `add_tool()` or `add_function()` to accumulate instead.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<InternalTool>) -> Self {
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
    ///     .with_description("Get the temperature for a location")
    ///     .add_parameter("location", json!({"type": "string"}))
    ///     .with_required(vec!["location".to_string()])
    ///     .build();
    ///
    /// let builder = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
    /// With the `*_with_auto_functions` methods, the service's functions are
    /// always declared to the model, alongside any tools set explicitly. When
    /// no tools are set, `#[tool]` functions from the global registry are
    /// declared too; a service function shadows a registry one of the same
    /// name.
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
    /// Replaces an earlier tool of the same kind, including one added with
    /// `add_tool`, so calling this twice sends it once.
    ///
    /// This adds the built-in `GoogleSearch` tool which allows the model to
    /// search the web and ground its responses in real-time information.
    /// Grounding metadata will be available in the response via
    /// [`InteractionResponse::google_search_results`].
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Who won the 2024 World Series?")
    ///     .with_google_search()
    ///     .create()
    ///     .await?;
    ///
    /// // Access grounding data from steps
    /// for query in response.google_search_calls() {
    ///     println!("Search query: {}", query);
    /// }
    /// for result in response.google_search_results() {
    ///     println!("Source: {} - {}", result.title, result.url);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`InteractionResponse::google_search_results`]: crate::InteractionResponse::google_search_results
    #[must_use]
    pub fn with_google_search(mut self) -> Self {
        self.replace_tool(InternalTool::GoogleSearch { search_types: None });
        self
    }

    /// Enables the Google Maps built-in tool for location-grounded responses.
    ///
    /// Replaces an earlier tool of the same kind, including one added with
    /// `add_tool`, so calling this twice sends it once.
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Find coffee shops near Times Square")
    ///     .with_google_maps()
    ///     .create()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn with_google_maps(mut self) -> Self {
        self.replace_tool(InternalTool::GoogleMaps {
            latitude: None,
            longitude: None,
            enable_widget: None,
        });
        self
    }

    /// Enables code execution for this interaction.
    ///
    /// Replaces an earlier tool of the same kind, including one added with
    /// `add_tool`, so calling this twice sends it once.
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
        self.replace_tool(InternalTool::CodeExecution);
        self
    }

    /// Enables URL context fetching for this interaction.
    ///
    /// Replaces an earlier tool of the same kind, including one added with
    /// `add_tool`, so calling this twice sends it once.
    ///
    /// This adds the built-in `UrlContext` tool which allows the model to
    /// fetch and analyze content from URLs provided in the prompt.
    /// URL context metadata will be available in the response via
    /// [`InteractionResponse::url_context_results`].
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Summarize the content from https://example.com")
    ///     .with_url_context()
    ///     .create()
    ///     .await?;
    ///
    /// // Access URL context results from steps
    /// for result in response.url_context_results() {
    ///     for item in result.items {
    ///         println!("URL: {} - Status: {}", item.url, item.status);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`InteractionResponse::url_context_results`]: crate::InteractionResponse::url_context_results
    #[must_use]
    pub fn with_url_context(mut self) -> Self {
        self.replace_tool(InternalTool::UrlContext);
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
    ///     .with_text("Get weather in Tokyo")
    ///     .with_function_calling_mode(FunctionCallingMode::Any)
    ///     .create()
    ///     .await?;
    ///
    /// // Use VALIDATED mode for guaranteed schema adherence
    /// let response = client
    ///     .interaction()
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
        config.tool_choice = Some(ToolChoice::Mode(mode));
        self
    }

    /// Sets the full `tool_choice` union directly.
    ///
    /// Prefer [`with_function_calling_mode()`](Self::with_function_calling_mode)
    /// for the plain-mode form and [`with_allowed_tools()`](Self::with_allowed_tools)
    /// for the restriction form; this method is the escape hatch for custom
    /// shapes.
    #[must_use]
    pub fn with_tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        let config = self
            .generation_config
            .get_or_insert_with(GenerationConfig::default);
        config.tool_choice = Some(tool_choice);
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
        // Preserve any previously-set mode when upgrading to the object form.
        let mode = match config.tool_choice.take() {
            Some(ToolChoice::Mode(mode)) => Some(mode),
            Some(ToolChoice::AllowedTools(allowed)) => allowed.mode,
            _ => None,
        };
        config.tool_choice = Some(ToolChoice::allowed_tools(mode, tool_names));
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
    ///     .with_model(genai_rs::DEFAULT_MODEL)
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
}
