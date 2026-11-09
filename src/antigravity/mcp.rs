//! [`McpServer`]: an MCP server for the harness to connect to.

use std::collections::BTreeMap;

use super::protocol;
use super::redact::RedactedValues;

/// An MCP server for the harness to connect to.
///
/// ```rust
/// use genai_rs::antigravity::McpServer;
///
/// let git = McpServer::stdio("uvx", ["mcp-server-git"]);
/// let api = McpServer::http("http://localhost:8931/mcp").with_name("api");
/// # let _ = (git, api);
/// ```
///
/// `Debug` redacts the values of the environment variables and headers,
/// keeping their names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    name: String,
    transport: McpTransport,
    enabled_tools: Vec<String>,
    disabled_tools: Vec<String>,
    timeout_seconds: Option<i32>,
    force_all_tools_eager: bool,
}

#[derive(Clone, PartialEq, Eq)]
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

impl std::fmt::Debug for McpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio { command, args, env } => f
                .debug_struct("Stdio")
                .field("command", command)
                .field("args", args)
                .field("env", &RedactedValues(env))
                .finish(),
            Self::Http { url, headers } => f
                .debug_struct("Http")
                .field("url", url)
                .field("headers", &RedactedValues(headers))
                .finish(),
        }
    }
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
            force_all_tools_eager: false,
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
            force_all_tools_eager: false,
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

    /// Sets how long the harness waits for the server, in seconds: to
    /// connect and list its tools, which it does on the first turn (a
    /// slower server fails that turn), and for each tool call (a slower call
    /// returns an error to the model). Harness 0.1.23 applies both.
    #[must_use]
    pub fn with_timeout_seconds(mut self, seconds: i32) -> Self {
        self.timeout_seconds = Some(seconds);
        self
    }

    /// Declares every tool of this server to the model up front, by name
    /// (`mcp_<server>_<tool>`), instead of listing it in the system prompt
    /// for the model to reach through the harness's `call_mcp_tool`
    /// (the default, which keeps a large server's schemas out of the
    /// context).
    #[must_use]
    pub fn with_force_all_tools_eager(mut self, eager: bool) -> Self {
        self.force_all_tools_eager = eager;
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
            force_all_tools_eager: self.force_all_tools_eager,
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
#[path = "mcp_tests.rs"]
mod tests;
