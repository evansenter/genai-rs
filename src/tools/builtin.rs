use super::{AllowedTools, Tool};
use crate::wire_enum::wire_enum;
use std::collections::HashMap;

wire_enum! {
    /// Types of search to perform with the Google Search tool.
    ///
    /// # Wire Format
    ///
    /// Values serialize as snake_case strings: `"web_search"`, `"image_search"`,
    /// `"enterprise_web_search"`.
    pub enum SearchType {
        /// Web search
        WebSearch = "web_search",
        /// Image search (only available for specific models like `gemini-3.1-flash-image-preview`)
        ImageSearch = "image_search",
        /// Enterprise web search
        EnterpriseWebSearch = "enterprise_web_search",
    }
    unknown(search_type, unknown_search_type)
}

// --- Tool Configuration Structs ---
//
// These provide ergonomic builders for constructing Tool variants with optional fields.
// Each implements `From<Config> for Tool` so they can be passed to `InteractionBuilder::add_tool()`.

/// Configuration for the Google Search built-in tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::{GoogleSearchConfig, SearchType};
///
/// // Default (web search only)
/// let config = GoogleSearchConfig::new();
///
/// // With image search enabled
/// let config = GoogleSearchConfig::new()
///     .with_search_types(vec![SearchType::WebSearch, SearchType::ImageSearch]);
/// ```
#[derive(Clone, Debug, Default)]
pub struct GoogleSearchConfig {
    search_types: Option<Vec<SearchType>>,
}

impl GoogleSearchConfig {
    /// Creates a new `GoogleSearchConfig` with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the search types to perform.
    #[must_use]
    pub fn with_search_types(mut self, search_types: Vec<SearchType>) -> Self {
        self.search_types = Some(search_types);
        self
    }
}

impl From<GoogleSearchConfig> for Tool {
    fn from(config: GoogleSearchConfig) -> Self {
        Tool::GoogleSearch {
            search_types: config.search_types,
        }
    }
}

/// Configuration for the Google Maps built-in tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::GoogleMapsConfig;
///
/// // Default
/// let config = GoogleMapsConfig::new();
///
/// // With widget enabled
/// let config = GoogleMapsConfig::new().with_widget();
/// ```
#[derive(Clone, Debug, Default)]
pub struct GoogleMapsConfig {
    enable_widget: Option<bool>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

impl GoogleMapsConfig {
    /// Creates a new `GoogleMapsConfig` with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables the widget context token in the response.
    #[must_use]
    pub fn with_widget(mut self) -> Self {
        self.enable_widget = Some(true);
        self
    }

    /// Biases results toward the given coordinates.
    #[must_use]
    pub fn with_location(mut self, latitude: f64, longitude: f64) -> Self {
        self.latitude = Some(latitude);
        self.longitude = Some(longitude);
        self
    }
}

impl From<GoogleMapsConfig> for Tool {
    fn from(config: GoogleMapsConfig) -> Self {
        Tool::GoogleMaps {
            enable_widget: config.enable_widget,
            latitude: config.latitude,
            longitude: config.longitude,
        }
    }
}

/// Configuration for an MCP (Model Context Protocol) server tool.
///
/// # Example
///
/// ```no_run
/// use genai_rs::McpServerConfig;
/// use std::collections::HashMap;
///
/// let config = McpServerConfig::new("filesystem", "https://mcp.example.com/fs")
///     .with_allowed_tools(vec!["read_file".to_string(), "list_dir".to_string()])
///     .with_headers(HashMap::from([
///         ("Authorization".to_string(), "Bearer token".to_string()),
///     ]));
/// ```
#[derive(Clone, Debug)]
pub struct McpServerConfig {
    name: String,
    url: String,
    allowed_tools: Option<Vec<AllowedTools>>,
    headers: Option<HashMap<String, String>>,
}

impl McpServerConfig {
    /// Creates a new `McpServerConfig` with the given name and URL.
    #[must_use]
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            allowed_tools: None,
            headers: None,
        }
    }

    /// Restricts the model to the given tool names (no explicit mode).
    ///
    /// For per-mode restrictions use
    /// [`with_allowed_tools_config`](Self::with_allowed_tools_config).
    #[must_use]
    pub fn with_allowed_tools(mut self, allowed_tools: Vec<String>) -> Self {
        self.allowed_tools = Some(vec![AllowedTools::new(allowed_tools)]);
        self
    }

    /// Sets the full `[{mode, tools}]` allowed-tools restriction list.
    #[must_use]
    pub fn with_allowed_tools_config(mut self, allowed_tools: Vec<AllowedTools>) -> Self {
        self.allowed_tools = Some(allowed_tools);
        self
    }

    /// Sets authentication/configuration headers.
    #[must_use]
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }
}

impl From<McpServerConfig> for Tool {
    fn from(config: McpServerConfig) -> Self {
        Tool::McpServer {
            name: config.name,
            url: config.url,
            allowed_tools: config.allowed_tools,
            headers: config.headers,
        }
    }
}

/// Configuration for the Computer Use built-in tool.
///
/// # Security Warning
///
/// Computer use allows the model to control a real browser. Use
/// [`ComputerUseConfig::with_excluded_predefined_functions`] to restrict dangerous actions.
///
/// # Example
///
/// ```no_run
/// use genai_rs::ComputerUseConfig;
///
/// let config = ComputerUseConfig::new()
///     .with_excluded_predefined_functions(vec!["submit_form".to_string(), "download_file".to_string()]);
/// ```
#[derive(Clone, Debug)]
pub struct ComputerUseConfig {
    environment: String,
    excluded_predefined_functions: Vec<String>,
    enable_prompt_injection_detection: Option<bool>,
    disabled_safety_policies: Vec<String>,
}

impl ComputerUseConfig {
    /// Creates a new `ComputerUseConfig` targeting the browser environment.
    #[must_use]
    pub fn new() -> Self {
        Self {
            environment: "browser".to_string(),
            excluded_predefined_functions: Vec::new(),
            enable_prompt_injection_detection: None,
            disabled_safety_policies: Vec::new(),
        }
    }

    /// Sets the operating environment. Known values: `browser`, `mobile`,
    /// `desktop`.
    #[must_use]
    pub fn with_environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = environment.into();
        self
    }

    /// Sets the predefined browser functions hidden from the model.
    #[must_use]
    pub fn with_excluded_predefined_functions(mut self, functions: Vec<String>) -> Self {
        self.excluded_predefined_functions = functions;
        self
    }

    /// Enables (or disables) prompt injection detection.
    #[must_use]
    pub fn with_prompt_injection_detection(mut self, enabled: bool) -> Self {
        self.enable_prompt_injection_detection = Some(enabled);
        self
    }

    /// Sets the safety policies to disable.
    ///
    /// Known values include `financial_transactions`,
    /// `sensitive_data_modification`, `communication_tool`,
    /// `account_creation`, `data_modification`, `user_consent_management`,
    /// and `legal_terms_and_agreements`.
    #[must_use]
    pub fn with_disabled_safety_policies(mut self, policies: Vec<String>) -> Self {
        self.disabled_safety_policies = policies;
        self
    }
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ComputerUseConfig> for Tool {
    fn from(config: ComputerUseConfig) -> Self {
        Tool::ComputerUse {
            environment: config.environment,
            excluded_predefined_functions: config.excluded_predefined_functions,
            enable_prompt_injection_detection: config.enable_prompt_injection_detection,
            disabled_safety_policies: config.disabled_safety_policies,
        }
    }
}

/// Configuration for the File Search built-in tool.
///
/// Store names are full resource names (`fileSearchStores/<id>`), as returned
/// by [`create_file_search_store`](crate::Client::create_file_search_store).
/// The store-management methods reject a bare ID locally; whether the
/// Interactions API does the same for this field has not been probed, so
/// pass the full name here too.
///
/// Cannot be combined with `google_search` or `url_context` in one request;
/// the API returns a 400 naming the pair (verified live 2026-08-16).
///
/// # Example
///
/// ```no_run
/// use genai_rs::FileSearchConfig;
///
/// # let store_name = String::new();
/// let config = FileSearchConfig::new(vec![store_name])
///     .with_top_k(10)
///     .with_metadata_filter("category:technical");
/// ```
#[derive(Clone, Debug)]
pub struct FileSearchConfig {
    store_names: Vec<String>,
    top_k: Option<i32>,
    metadata_filter: Option<String>,
}

impl FileSearchConfig {
    /// Creates a new `FileSearchConfig` with the given store names.
    #[must_use]
    pub fn new(store_names: Vec<String>) -> Self {
        Self {
            store_names,
            top_k: None,
            metadata_filter: None,
        }
    }

    /// Sets the maximum number of semantic retrieval chunks to return.
    #[must_use]
    pub fn with_top_k(mut self, top_k: i32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    /// Sets a metadata filter expression for document filtering.
    #[must_use]
    pub fn with_metadata_filter(mut self, filter: impl Into<String>) -> Self {
        self.metadata_filter = Some(filter.into());
        self
    }
}

impl From<FileSearchConfig> for Tool {
    fn from(config: FileSearchConfig) -> Self {
        Tool::FileSearch {
            store_names: config.store_names,
            top_k: config.top_k,
            metadata_filter: config.metadata_filter,
        }
    }
}

#[cfg(test)]
#[path = "builtin_tests.rs"]
mod tests;
