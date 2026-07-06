//! User-facing configuration types for Antigravity agents.

use std::collections::BTreeMap;
use std::collections::HashSet;

use super::protocol;

/// The `google-antigravity` wheel version this crate's protocol support is
/// pinned to and tested against.
///
/// The localharness wire protocol is internal to Google's SDK and changes
/// across 0.1.x releases. Each genai-rs release documents (and CI-tests
/// against) exactly one harness version; protocol drift on newer harnesses
/// degrades gracefully through the Evergreen `Unknown` variants rather than
/// erroring, but only the pinned version is verified end-to-end. Install it
/// with:
///
/// ```bash
/// pip install google-antigravity==0.1.5
/// ```
pub const SUPPORTED_HARNESS_VERSION: &str = "0.1.5";

/// Harness-executed built-in tools.
///
/// The variants map to the harness's wire names (also the names used in
/// [`policy`](super::policy) targets), e.g. [`BuiltinTool::RunCommand`] is
/// `"run_command"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BuiltinTool {
    /// List directory contents (`list_directory`).
    ListDir,
    /// Grep within directories (`search_directory`).
    SearchDir,
    /// Find files by name (`find_file`).
    FindFile,
    /// View file contents (`view_file`).
    ViewFile,
    /// Create a new file (`create_file`). Write-capable.
    CreateFile,
    /// Edit an existing file (`edit_file`). Write-capable.
    EditFile,
    /// Execute a shell command (`run_command`). Write-capable.
    RunCommand,
    /// Ask the user a clarifying question (`ask_question`).
    AskQuestion,
    /// Invoke a subagent (`start_subagent`). Write-capable.
    StartSubagent,
    /// Generate or edit images (`generate_image`). Write-capable.
    GenerateImage,
    /// Search the web (`search_web`). Write-capable (network egress).
    SearchWeb,
    /// Finish with structured output (`finish`).
    Finish,
}

impl BuiltinTool {
    /// The harness wire name (used in step updates and policy targets).
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::ListDir => "list_directory",
            Self::SearchDir => "search_directory",
            Self::FindFile => "find_file",
            Self::ViewFile => "view_file",
            Self::CreateFile => "create_file",
            Self::EditFile => "edit_file",
            Self::RunCommand => "run_command",
            Self::AskQuestion => "ask_question",
            Self::StartSubagent => "start_subagent",
            Self::GenerateImage => "generate_image",
            Self::SearchWeb => "search_web",
            Self::Finish => "finish",
        }
    }

    /// All built-in tools.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![
            Self::ListDir,
            Self::SearchDir,
            Self::FindFile,
            Self::ViewFile,
            Self::CreateFile,
            Self::EditFile,
            Self::RunCommand,
            Self::AskQuestion,
            Self::StartSubagent,
            Self::GenerateImage,
            Self::SearchWeb,
            Self::Finish,
        ]
    }

    /// Tools that only read state (no writes, deletes, or commands).
    /// This is the default capability set, matching the reference SDK.
    #[must_use]
    pub fn read_only() -> Vec<Self> {
        vec![
            Self::ListDir,
            Self::SearchDir,
            Self::FindFile,
            Self::ViewFile,
            Self::Finish,
        ]
    }

    /// Whether enabling this tool lets the agent mutate state or reach
    /// beyond read-only inspection. Used by the spawn-time safety check.
    #[must_use]
    pub fn is_write_capable(self) -> bool {
        !Self::read_only().contains(&self)
    }
}

/// Which built-in harness tools the agent may use.
///
/// The default is the read-only set ([`BuiltinTool::read_only`]), matching
/// the reference SDK. Enabling any write-capable tool requires a policy or
/// pre-tool hook at spawn time (safety parity with the reference SDK).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    enabled: HashSet<BuiltinTool>,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::read_only()
    }
}

impl Capabilities {
    /// Only read-only built-ins (the default).
    #[must_use]
    pub fn read_only() -> Self {
        Self {
            enabled: BuiltinTool::read_only().into_iter().collect(),
        }
    }

    /// All built-ins, including shell access and file writes.
    #[must_use]
    pub fn all() -> Self {
        Self {
            enabled: BuiltinTool::all().into_iter().collect(),
        }
    }

    /// No built-ins at all (custom tools only).
    #[must_use]
    pub fn none() -> Self {
        Self {
            enabled: HashSet::new(),
        }
    }

    /// Enables one built-in tool.
    #[must_use]
    pub fn enable(mut self, tool: BuiltinTool) -> Self {
        self.enabled.insert(tool);
        self
    }

    /// Disables one built-in tool.
    #[must_use]
    pub fn disable(mut self, tool: BuiltinTool) -> Self {
        self.enabled.remove(&tool);
        self
    }

    /// Whether the given tool is enabled.
    #[must_use]
    pub fn is_enabled(&self, tool: BuiltinTool) -> bool {
        self.enabled.contains(&tool)
    }

    /// Whether any write-capable built-in is enabled.
    #[must_use]
    pub fn has_write_tools(&self) -> bool {
        self.enabled.iter().any(|t| t.is_write_capable())
    }

    /// Builds the harness `HarnessSideTools` flags. Every flag is written
    /// explicitly (the harness defaults most tools to *enabled*, so omitting
    /// a disabled tool would silently enable it).
    pub(crate) fn to_harness_side_tools(&self) -> protocol::HarnessSideTools {
        let on = |tool: BuiltinTool| Some(protocol::ToolToggle::new(self.is_enabled(tool)));
        protocol::HarnessSideTools {
            find: on(BuiltinTool::FindFile),
            run_command: on(BuiltinTool::RunCommand),
            subagents: on(BuiltinTool::StartSubagent),
            user_questions: on(BuiltinTool::AskQuestion),
            file_edit: on(BuiltinTool::EditFile),
            view_file: on(BuiltinTool::ViewFile),
            write_to_file: on(BuiltinTool::CreateFile),
            grep_search: on(BuiltinTool::SearchDir),
            list_dir: on(BuiltinTool::ListDir),
            permissions: None,
            generate_image: on(BuiltinTool::GenerateImage),
            search_web: on(BuiltinTool::SearchWeb),
        }
    }
}

/// A static subagent the parent agent can delegate to.
///
/// Subagents run in their own trajectory with their own instructions and
/// tool set. The parent model invokes them through the `start_subagent`
/// builtin — enable it with
/// `Capabilities::read_only().enable(BuiltinTool::StartSubagent)` (it is
/// write-capable, so the spawn-time safety gate requires a policy or
/// pre-tool hook).
///
/// ```rust,ignore
/// use genai_rs::antigravity::{AntigravityAgent, BuiltinTool, Capabilities, Subagent, policy};
///
/// let agent = AntigravityAgent::builder()
///     .add_tool(severity_classifier_declaration())   // parent registration
///     .add_subagent(
///         Subagent::new("auditor")
///             .with_description("Audits one file for security issues.")
///             .with_system_instructions("Focus on injection vectors.")
///             .add_tool("severity_classifier"),      // reference by name
///     )
///     .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))
///     .add_policy(policy::allow_all())
///     // ...
///     ;
/// ```
///
/// Custom tools are referenced **by name** and must also be registered on
/// the parent agent (`add_tool` / `with_tool_service`) — the harness
/// dispatches a subagent's custom tool calls through the parent's tool
/// registry. `spawn()` validates this and fails with
/// [`AntigravityError::Config`](super::AntigravityError::Config) on a
/// dangling reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subagent {
    name: String,
    description: Option<String>,
    system_instructions: Option<String>,
    capabilities: Capabilities,
    tool_names: Vec<String>,
}

impl Subagent {
    /// Creates a subagent with the given (unique) name and the default
    /// read-only builtin set (matching the reference SDK).
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            system_instructions: None,
            capabilities: Capabilities::read_only(),
            tool_names: Vec::new(),
        }
    }

    /// Sets the description shown to the parent model (how it decides when
    /// to delegate).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets system instructions for the subagent. These are **appended** to
    /// the harness's default subagent instructions (reference SDK
    /// semantics), not a full replacement.
    #[must_use]
    pub fn with_system_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.system_instructions = Some(instructions.into());
        self
    }

    /// Configures which built-in harness tools the subagent may use
    /// (default: the read-only set). `start_subagent` is force-disabled —
    /// the harness does not support nested subagents.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Adds a custom tool, referenced by name. The tool must also be
    /// registered on the parent agent; `spawn()` validates this.
    #[must_use]
    pub fn add_tool(mut self, name: impl Into<String>) -> Self {
        self.tool_names.push(name.into());
        self
    }

    /// The subagent's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The custom tool names this subagent references.
    #[must_use]
    pub fn tool_names(&self) -> &[String] {
        &self.tool_names
    }

    /// Builds the wire `CustomAgent`, resolving custom tool names against
    /// the parent's declarations. Callers must have validated the names
    /// beforehand (`spawn()` does); unresolved names are skipped here.
    pub(crate) fn to_wire(&self, parent_tools: &[protocol::Tool]) -> protocol::CustomAgent {
        if self.capabilities.is_enabled(BuiltinTool::StartSubagent) {
            tracing::warn!(
                "Subagent '{}' enables start_subagent, but nested subagents are not \
                 supported by the harness; disabling it.",
                self.name
            );
        }
        let mut harness_side_tools = self.capabilities.to_harness_side_tools();
        // Nested subagents are unsupported (parity with the reference SDK).
        harness_side_tools.subagents = Some(protocol::ToolToggle::new(false));

        let tools = self
            .tool_names
            .iter()
            .filter_map(|name| {
                parent_tools
                    .iter()
                    .find(|tool| tool.name.as_deref() == Some(name))
                    .cloned()
            })
            .collect();

        protocol::CustomAgent {
            name: Some(self.name.clone()),
            description: self.description.clone(),
            // Reference SDK semantics: subagent instructions are appended
            // sections (title "System"), never a full replacement.
            system_instructions: self.system_instructions.as_ref().map(|text| {
                protocol::SystemInstructions {
                    custom: None,
                    appended: Some(protocol::AppendedSystemInstructions {
                        custom_identity: None,
                        appended_sections: vec![protocol::InstructionSection {
                            title: Some("System".to_string()),
                            content: Some(text.clone()),
                        }],
                    }),
                }
            }),
            harness_side_tools: Some(harness_side_tools),
            tools,
        }
    }
}

/// An MCP server for the harness to connect to.
///
/// ```rust,ignore
/// use genai_rs::antigravity::McpServer;
///
/// let git = McpServer::stdio("uvx", ["mcp-server-git"]);
/// let api = McpServer::http("http://localhost:8931/mcp").with_name("api");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    name: String,
    transport: McpTransport,
    enabled_tools: Vec<String>,
    disabled_tools: Vec<String>,
    timeout_seconds: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum McpTransport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
}

impl McpServer {
    /// A stdio-transport MCP server: the harness spawns `command args...`.
    ///
    /// The server name defaults to the command's basename; override it with
    /// [`Self::with_name`]. The name is the `<server>` part of policy
    /// targets (`mcp_<server>_<tool>`).
    #[must_use]
    pub fn stdio(
        command: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let command = command.into();
        let name = std::path::Path::new(&command)
            .file_stem()
            .map_or_else(|| command.clone(), |s| s.to_string_lossy().into_owned());
        Self {
            name,
            transport: McpTransport::Stdio {
                command,
                args: args.into_iter().map(Into::into).collect(),
                env: BTreeMap::new(),
            },
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            timeout_seconds: None,
        }
    }

    /// A streamable-HTTP MCP server at the given URL.
    ///
    /// The server name defaults to the URL's host; override it with
    /// [`Self::with_name`].
    #[must_use]
    pub fn http(url: impl Into<String>) -> Self {
        let url = url.into();
        let name = url
            .split("//")
            .nth(1)
            .and_then(|rest| rest.split(['/', ':']).next())
            .filter(|h| !h.is_empty())
            .map_or_else(|| "http".to_string(), ToString::to_string);
        Self {
            name,
            transport: McpTransport::Http {
                url,
                headers: BTreeMap::new(),
            },
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            timeout_seconds: None,
        }
    }

    /// Overrides the server name (used in policy targets).
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets an environment variable for a stdio server's subprocess.
    /// No-op for HTTP servers.
    #[must_use]
    pub fn add_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let McpTransport::Stdio { env, .. } = &mut self.transport {
            env.insert(key.into(), value.into());
        }
        self
    }

    /// Adds an HTTP header for an HTTP server. No-op for stdio servers.
    #[must_use]
    pub fn add_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let McpTransport::Http { headers, .. } = &mut self.transport {
            headers.insert(key.into(), value.into());
        }
        self
    }

    /// Restricts the server to the given tool names (empty = all tools).
    #[must_use]
    pub fn with_enabled_tools(
        mut self,
        tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.enabled_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    /// Hides the given tool names from the agent.
    #[must_use]
    pub fn with_disabled_tools(
        mut self,
        tools: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.disabled_tools = tools.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the per-call timeout in seconds.
    #[must_use]
    pub fn with_timeout_seconds(mut self, seconds: i32) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }

    /// The server name (the `<server>` part of `mcp_<server>_<tool>`
    /// policy targets).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn to_wire(&self) -> protocol::McpServerConfig {
        let mut config = protocol::McpServerConfig {
            name: Some(self.name.clone()),
            enabled_tools: self.enabled_tools.clone(),
            disabled_tools: self.disabled_tools.clone(),
            timeout_seconds: self.timeout_seconds,
            ..Default::default()
        };
        match &self.transport {
            McpTransport::Stdio { command, args, env } => {
                config.stdio = Some(protocol::McpStdioTransport {
                    command: Some(command.clone()),
                    args: args.clone(),
                    env: env.clone(),
                });
            }
            McpTransport::Http { url, headers } => {
                config.http = Some(protocol::McpHttpTransport {
                    url: Some(url.clone()),
                    headers: headers.clone(),
                });
            }
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_capabilities_are_read_only() {
        let caps = Capabilities::default();
        assert!(caps.is_enabled(BuiltinTool::ViewFile));
        assert!(caps.is_enabled(BuiltinTool::ListDir));
        assert!(caps.is_enabled(BuiltinTool::Finish));
        assert!(!caps.is_enabled(BuiltinTool::RunCommand));
        assert!(!caps.is_enabled(BuiltinTool::EditFile));
        assert!(!caps.is_enabled(BuiltinTool::StartSubagent));
        assert!(!caps.has_write_tools());
    }

    #[test]
    fn test_capabilities_enable_disable() {
        let caps = Capabilities::read_only()
            .enable(BuiltinTool::RunCommand)
            .disable(BuiltinTool::ViewFile);
        assert!(caps.is_enabled(BuiltinTool::RunCommand));
        assert!(!caps.is_enabled(BuiltinTool::ViewFile));
        assert!(caps.has_write_tools());
    }

    #[test]
    fn test_capabilities_all_and_none() {
        assert!(Capabilities::all().has_write_tools());
        assert!(!Capabilities::none().has_write_tools());
        assert!(!Capabilities::none().is_enabled(BuiltinTool::ViewFile));
    }

    #[test]
    fn test_harness_side_tools_flags_all_explicit() {
        let flags = Capabilities::read_only().to_harness_side_tools();
        // Read-only tools enabled...
        assert!(flags.view_file.unwrap().enabled);
        assert!(flags.list_dir.unwrap().enabled);
        assert!(flags.grep_search.unwrap().enabled);
        assert!(flags.find.unwrap().enabled);
        // ...write tools explicitly disabled (not omitted: the harness
        // defaults them to enabled).
        assert!(!flags.run_command.unwrap().enabled);
        assert!(!flags.file_edit.unwrap().enabled);
        assert!(!flags.write_to_file.unwrap().enabled);
        assert!(!flags.subagents.unwrap().enabled);
        assert!(!flags.generate_image.unwrap().enabled);
        assert!(!flags.search_web.unwrap().enabled);
        assert!(!flags.user_questions.unwrap().enabled);
    }

    #[test]
    fn test_builtin_wire_names_match_reference_sdk() {
        // Values must match google.antigravity.types.BuiltinTools exactly.
        let expected = [
            (BuiltinTool::ListDir, "list_directory"),
            (BuiltinTool::SearchDir, "search_directory"),
            (BuiltinTool::FindFile, "find_file"),
            (BuiltinTool::ViewFile, "view_file"),
            (BuiltinTool::CreateFile, "create_file"),
            (BuiltinTool::EditFile, "edit_file"),
            (BuiltinTool::RunCommand, "run_command"),
            (BuiltinTool::AskQuestion, "ask_question"),
            (BuiltinTool::StartSubagent, "start_subagent"),
            (BuiltinTool::GenerateImage, "generate_image"),
            (BuiltinTool::SearchWeb, "search_web"),
            (BuiltinTool::Finish, "finish"),
        ];
        for (tool, name) in expected {
            assert_eq!(tool.wire_name(), name);
        }
    }

    fn parent_tool(name: &str) -> protocol::Tool {
        protocol::Tool {
            name: Some(name.to_string()),
            description: Some(format!("{name} description")),
            parameters_json_schema: Some(r#"{"type":"object"}"#.to_string()),
            response_json_schema: None,
        }
    }

    #[test]
    fn test_subagent_defaults() {
        let subagent = Subagent::new("auditor");
        assert_eq!(subagent.name(), "auditor");
        assert!(subagent.tool_names().is_empty());
        let wire = subagent.to_wire(&[]);
        assert_eq!(wire.name.as_deref(), Some("auditor"));
        assert!(wire.description.is_none());
        assert!(wire.system_instructions.is_none());
        assert!(wire.tools.is_empty());
        // Default capabilities: read-only builtins, explicit flags.
        let side_tools = wire.harness_side_tools.unwrap();
        assert!(side_tools.view_file.unwrap().enabled);
        assert!(!side_tools.run_command.unwrap().enabled);
        assert!(!side_tools.subagents.unwrap().enabled);
    }

    #[test]
    fn test_subagent_to_wire_resolves_parent_tools() {
        let subagent = Subagent::new("auditor")
            .with_description("Audits files.")
            .with_system_instructions("Focus on injection vectors.")
            .add_tool("severity_classifier");
        assert_eq!(subagent.tool_names(), ["severity_classifier"]);

        let parent_tools = [parent_tool("other"), parent_tool("severity_classifier")];
        let wire = subagent.to_wire(&parent_tools);
        assert_eq!(wire.description.as_deref(), Some("Audits files."));
        // Instructions are appended sections (reference SDK semantics),
        // never a custom replacement.
        let instructions = wire.system_instructions.unwrap();
        assert!(instructions.custom.is_none());
        let appended = instructions.appended.unwrap();
        assert_eq!(appended.appended_sections.len(), 1);
        assert_eq!(
            appended.appended_sections[0].title.as_deref(),
            Some("System")
        );
        assert_eq!(
            appended.appended_sections[0].content.as_deref(),
            Some("Focus on injection vectors.")
        );
        // The full parent declaration is copied, not just the name.
        assert_eq!(wire.tools.len(), 1);
        assert_eq!(wire.tools[0].name.as_deref(), Some("severity_classifier"));
        assert_eq!(
            wire.tools[0].parameters_json_schema.as_deref(),
            Some(r#"{"type":"object"}"#)
        );
    }

    #[test]
    fn test_subagent_start_subagent_forced_off() {
        // Nested subagents are unsupported: even when explicitly enabled,
        // the wire config disables the builtin.
        let subagent = Subagent::new("nested")
            .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent));
        let wire = subagent.to_wire(&[]);
        assert!(!wire.harness_side_tools.unwrap().subagents.unwrap().enabled);
    }

    #[test]
    fn test_subagent_wire_serialization_fixture() {
        let subagent = Subagent::new("auditor")
            .with_description("Audits files.")
            .with_system_instructions("Be thorough.")
            .add_tool("severity_classifier");
        let wire = subagent.to_wire(&[parent_tool("severity_classifier")]);
        let value = serde_json::to_value(&wire).unwrap();
        // Proto-JSON: camelCase fields, JSON-string schema.
        assert_eq!(
            value["systemInstructions"]["appended"]["appendedSections"][0]["content"],
            "Be thorough."
        );
        assert_eq!(value["tools"][0]["name"], "severity_classifier");
        assert_eq!(
            value["tools"][0]["parametersJsonSchema"],
            r#"{"type":"object"}"#
        );
        assert_eq!(value["harnessSideTools"]["subagents"]["enabled"], false);
        assert_eq!(value["harnessSideTools"]["viewFile"]["enabled"], true);
    }

    #[test]
    fn test_mcp_stdio_defaults_name_from_command() {
        let server = McpServer::stdio("/usr/bin/uvx", ["mcp-server-git"]);
        assert_eq!(server.name(), "uvx");
        let wire = server.to_wire();
        let stdio = wire.stdio.unwrap();
        assert_eq!(stdio.command.as_deref(), Some("/usr/bin/uvx"));
        assert_eq!(stdio.args, vec!["mcp-server-git"]);
        assert!(wire.http.is_none());
    }

    #[test]
    fn test_mcp_http_defaults_name_from_host() {
        let server = McpServer::http("http://localhost:8931/mcp");
        assert_eq!(server.name(), "localhost");
        let wire = server.to_wire();
        assert_eq!(
            wire.http.unwrap().url.as_deref(),
            Some("http://localhost:8931/mcp")
        );
        assert!(wire.stdio.is_none());
    }

    #[test]
    fn test_mcp_builders_accumulate() {
        let server = McpServer::stdio("uvx", ["x"])
            .with_name("git")
            .add_env("A", "1")
            .add_env("B", "2")
            .with_enabled_tools(["status"])
            .with_timeout_seconds(30);
        assert_eq!(server.name(), "git");
        let wire = server.to_wire();
        assert_eq!(wire.stdio.unwrap().env.len(), 2);
        assert_eq!(wire.enabled_tools, vec!["status"]);
        assert_eq!(wire.timeout_seconds, Some(30));
    }
}
