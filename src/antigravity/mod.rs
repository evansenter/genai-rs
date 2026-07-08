//! Native client for Google's Antigravity `localharness` agent runtime.
//!
//! The [`google-antigravity` Python SDK](https://pypi.org/project/google-antigravity/)
//! ships a Go binary (`localharness`) that *is* the agent runtime: model
//! calls, streaming, history/compaction, built-in tool execution (shell,
//! file edits, web search), MCP, and trajectory persistence all live inside
//! it. This module speaks the harness's protocol directly — a stdio
//! handshake plus proto-JSON over a localhost WebSocket — so Rust
//! applications get the full agent runtime with **Rust-native tools, hooks,
//! and policies** and no Python in the loop.
//!
//! Enable with the `antigravity` cargo feature. See `docs/ANTIGRAVITY.md`
//! for the full guide, and [`SUPPORTED_HARNESS_VERSION`] for the pinned
//! harness version.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use genai_rs::antigravity::{AntigravityAgent, policy};
//!
//! let mut agent = AntigravityAgent::builder()
//!     .with_api_key(std::env::var("GEMINI_API_KEY")?)
//!     .with_model("gemini-3-flash-preview")
//!     .with_system_instructions("You are a code-review assistant.")
//!     .add_workspace("/path/to/repo")
//!     .add_policy(policy::deny_all())
//!     .add_policy(policy::allow("view_file"))
//!     .spawn()
//!     .await?;
//!
//! let response = agent.chat("Summarize src/lib.rs").await?;
//! println!("{}", response.text());
//! agent.shutdown().await?;
//! ```

mod config;
mod handshake;
mod hooks;
mod process;
pub mod protocol;
mod session;
mod streaming;
mod tools;
pub mod triggers;

pub use config::{BuiltinTool, Capabilities, McpServer, SUPPORTED_HARNESS_VERSION, Subagent};
pub use hooks::{
    Policy, PolicyDecision, PostToolHook, PreToolDecision, PreToolHook, ToolInvocation,
    ToolOutcome, policy,
};
pub use streaming::{AgentEvent, AgentEventStream, ErrorSeverity, ToolAction, ToolDecision};
pub use triggers::TriggerConfig;

use crate::wire::{LoudWirePrinter, WireInspector};
use crate::{FunctionDeclaration, ToolService};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

use hooks::PolicyEngine;
use process::HarnessProcess;
use protocol::{
    HookDecision, HookVerdict, InputEvent, OutputEvent, OutputPayload, StepSource, StepState,
    StepTarget, StepUpdate, TrajectoryState,
};
use session::{Session, SinkHandle, WireContext};
use tools::ToolDispatcher;

/// How long to wait for the `initializeConversationResponse`.
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Overall budget for halting-and-draining an orphaned harness turn (one
/// that timed out, or one started by a trigger) before giving up.
const HALT_DRAIN_BUDGET: Duration = Duration::from_secs(5);

/// How long the event stream must stay silent during a halt-and-drain
/// before it is considered fully drained.
const HALT_DRAIN_SILENCE: Duration = Duration::from_millis(500);

/// Model backend HTTP codes that abort the turn (bad request / auth
/// failures cannot recover by retrying within the turn).
const FATAL_HTTP_CODES: [u32; 3] = [400, 401, 403];

// =============================================================================
// Errors
// =============================================================================

/// Errors from the Antigravity harness client.
///
/// Spawn- and init-time variants carry the tail of the harness's stderr —
/// that is where the harness reports actionable problems (e.g. `no text
/// model configuration provided`).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AntigravityError {
    /// The `localharness` binary could not be found.
    #[error(
        "localharness binary not found. Searched: {searched:?}. \
         Install it with `pip install google-antigravity=={SUPPORTED_HARNESS_VERSION}` \
         or set {}.",
        process::HARNESS_PATH_ENV
    )]
    HarnessNotFound {
        /// The locations that were searched, in order.
        searched: Vec<String>,
    },
    /// The stdio handshake with the harness failed.
    #[error("harness handshake failed: {message}\nharness stderr:\n{stderr}")]
    HandshakeFailed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// The conversation could not be initialized.
    #[error("conversation initialization failed: {message}\nharness stderr:\n{stderr}")]
    InitFailed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// The harness closed the connection unexpectedly (it likely crashed).
    #[error("harness connection closed: {message}\nharness stderr:\n{stderr}")]
    ConnectionClosed {
        /// What went wrong.
        message: String,
        /// Tail of the harness's stderr.
        stderr: String,
    },
    /// A custom tool could not be dispatched.
    #[error("tool dispatch failed for '{name}': {message}")]
    ToolDispatch {
        /// The tool name.
        name: String,
        /// What went wrong.
        message: String,
    },
    /// The agent configuration is invalid.
    #[error("invalid agent configuration: {0}")]
    Config(String),
    /// An operation exceeded its time budget.
    #[error("{operation} timed out after {timeout:?}")]
    Timeout {
        /// The operation that timed out.
        operation: String,
        /// The configured budget.
        timeout: Duration,
    },
    /// The turn failed (model backend error, cancellation, or pre-turn
    /// denial).
    #[error("agent turn failed: {0}")]
    Turn(String),
    /// WebSocket transport error.
    #[error("websocket error: {0}")]
    WebSocket(String),
    /// The harness sent something this client could not parse.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON (de)serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// =============================================================================
// ChatResponse
// =============================================================================

/// The assembled result of one agent turn.
#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    text: String,
    thoughts: String,
    usage: Option<protocol::UsageMetadata>,
    structured_output: Option<Value>,
    errors: Vec<String>,
}

impl ChatResponse {
    /// The final response text (the last completed model step directed at
    /// the user).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Concatenated thinking text from the turn's completed steps.
    #[must_use]
    pub fn thoughts(&self) -> &str {
        &self.thoughts
    }

    /// Token usage reported for the turn (last report wins).
    #[must_use]
    pub fn usage(&self) -> Option<&protocol::UsageMetadata> {
        self.usage.as_ref()
    }

    /// Structured output from the agent's `finish` action, when a response
    /// schema was configured.
    #[must_use]
    pub fn structured_output(&self) -> Option<&Value> {
        self.structured_output.as_ref()
    }

    /// Non-fatal errors the harness reported during the turn.
    #[must_use]
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
}

// =============================================================================
// Builder
// =============================================================================

/// Builder for [`AntigravityAgent`]. Create via
/// [`AntigravityAgent::builder`].
#[derive(Default)]
pub struct AgentBuilder {
    harness_path: Option<PathBuf>,
    api_key: Option<String>,
    model: Option<String>,
    system_instructions: Option<String>,
    workspaces: Vec<String>,
    tools: Vec<FunctionDeclaration>,
    tool_services: Vec<Arc<dyn ToolService>>,
    mcp_servers: Vec<McpServer>,
    policies: Vec<Policy>,
    pre_tool: Option<PreToolHook>,
    post_tool: Option<PostToolHook>,
    save_dir: Option<String>,
    conversation_id: Option<String>,
    capabilities: Capabilities,
    response_schema: Option<Value>,
    app_data_dir: Option<String>,
    skills_paths: Vec<String>,
    turn_timeout: Option<Duration>,
    inspectors: Vec<Arc<dyn WireInspector>>,
    triggers: Vec<TriggerConfig>,
    subagents: Vec<Subagent>,
    /// Whether to announce workspace roots in the effective system
    /// instructions. `None` means the default (on); see
    /// [`Self::with_workspace_announcement`].
    workspace_announcement: Option<bool>,
}

impl std::fmt::Debug for AgentBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentBuilder")
            .field("harness_path", &self.harness_path)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("model", &self.model)
            .field("workspaces", &self.workspaces)
            .field("tools", &self.tools.len())
            .field("mcp_servers", &self.mcp_servers.len())
            .field("policies", &self.policies.len())
            .field("capabilities", &self.capabilities)
            .field("triggers", &self.triggers.len())
            .field("subagents", &self.subagents.len())
            .finish_non_exhaustive()
    }
}

impl AgentBuilder {
    /// Sets an explicit path to the `localharness` binary, bypassing
    /// discovery.
    #[must_use]
    pub fn with_harness_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.harness_path = Some(path.into());
        self
    }

    /// Sets the Gemini API key used by the harness for model calls.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the text model (default: `gemini-3-flash-preview`).
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets custom system instructions for the agent.
    #[must_use]
    pub fn with_system_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.system_instructions = Some(instructions.into());
        self
    }

    /// Sets the workspace directory, replacing any previously configured
    /// workspaces. Use [`Self::add_workspace`] to accumulate several.
    #[must_use]
    pub fn with_workspace(mut self, directory: impl Into<String>) -> Self {
        self.workspaces = vec![directory.into()];
        self
    }

    /// Adds a workspace directory the agent may operate in.
    #[must_use]
    pub fn add_workspace(mut self, directory: impl Into<String>) -> Self {
        self.workspaces.push(directory.into());
        self
    }

    /// Controls whether the agent's configured workspace root(s) are
    /// announced to the model. **On by default.**
    ///
    /// The harness points its built-in tools at the workspaces but never
    /// tells the model their absolute paths, so agents otherwise guess
    /// (`/workdir`, `/workspace`, …) and wander. When on and at least one
    /// workspace is configured, `spawn()` appends a short, clearly
    /// delimited note listing the configured root(s) to the effective system
    /// instructions (composed at send time — the string passed to
    /// [`Self::with_system_instructions`] is never mutated). The same note
    /// is appended to each subagent's instructions, since subagent
    /// trajectories do not inherit the parent's context.
    ///
    /// Turn it off to manage workspace grounding yourself:
    ///
    /// ```rust,ignore
    /// let agent = AntigravityAgent::builder()
    ///     .add_workspace("/repo")
    ///     .with_workspace_announcement(false)
    ///     // ...
    ///     ;
    /// ```
    #[must_use]
    pub fn with_workspace_announcement(mut self, announce: bool) -> Self {
        self.workspace_announcement = Some(announce);
        self
    }

    /// Adds a custom tool by declaration. Execution resolves through the
    /// crate's global function registry (`#[tool]` macro), exactly like the
    /// Interactions-API auto-function path.
    #[must_use]
    pub fn add_tool(mut self, declaration: FunctionDeclaration) -> Self {
        self.tools.push(declaration);
        self
    }

    /// Registers a [`ToolService`] providing stateful custom tools.
    #[must_use]
    pub fn with_tool_service(mut self, service: Arc<dyn ToolService>) -> Self {
        self.tool_services.push(service);
        self
    }

    /// Adds an MCP server for the harness to connect to.
    #[must_use]
    pub fn add_mcp_server(mut self, server: McpServer) -> Self {
        self.mcp_servers.push(server);
        self
    }

    /// Adds a tool policy. See [`policy`] for constructors.
    ///
    /// Exact-name rules beat wildcard rules; within the same specificity
    /// tier the first registered matching rule wins, so
    /// `[deny_all(), allow("get_weather")]` allows only `get_weather`.
    /// When no rule matches, the call is allowed (default open), subject
    /// to the pre-tool hook.
    #[must_use]
    pub fn add_policy(mut self, policy: Policy) -> Self {
        self.policies.push(policy);
        self
    }

    /// Sets a pre-tool hook, consulted before every tool dispatch and for
    /// `confirm(...)` policies.
    #[must_use]
    pub fn on_pre_tool(
        mut self,
        hook: impl Fn(&ToolInvocation) -> PreToolDecision + Send + Sync + 'static,
    ) -> Self {
        self.pre_tool = Some(Arc::new(hook));
        self
    }

    /// Sets a post-tool hook, observing completed custom tool calls.
    #[must_use]
    pub fn on_post_tool(mut self, hook: impl Fn(&ToolOutcome) + Send + Sync + 'static) -> Self {
        self.post_tool = Some(Arc::new(hook));
        self
    }

    /// Sets the directory where the harness persists trajectories,
    /// enabling session resume via [`Self::with_conversation_id`].
    #[must_use]
    pub fn with_save_dir(mut self, dir: impl Into<String>) -> Self {
        self.save_dir = Some(dir.into());
        self
    }

    /// Resumes a saved conversation by id (see
    /// [`AntigravityAgent::conversation_id`]). Requires
    /// [`Self::with_save_dir`] pointing at the same directory.
    #[must_use]
    pub fn with_conversation_id(mut self, id: impl Into<String>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Configures which built-in harness tools are available.
    /// Default: the read-only set.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Sets a JSON schema for structured final output (delivered through
    /// [`ChatResponse::structured_output`]).
    #[must_use]
    pub fn with_response_schema(mut self, schema: Value) -> Self {
        self.response_schema = Some(schema);
        self
    }

    /// Sets the harness's application data directory.
    #[must_use]
    pub fn with_app_data_dir(mut self, dir: impl Into<String>) -> Self {
        self.app_data_dir = Some(dir.into());
        self
    }

    /// Adds a path to search for agent skills.
    #[must_use]
    pub fn add_skills_path(mut self, path: impl Into<String>) -> Self {
        self.skills_paths.push(path.into());
        self
    }

    /// Sets a wall-clock budget per turn. When exceeded, `chat` /
    /// `send_streaming` fail with [`AntigravityError::Timeout`].
    /// Default: unlimited.
    #[must_use]
    pub fn with_turn_timeout(mut self, timeout: Duration) -> Self {
        self.turn_timeout = Some(timeout);
        self
    }

    /// Registers a wire inspector receiving this session's
    /// [`WireEvent`](crate::wire::WireEvent)s (harness spawn, WebSocket
    /// traffic, harness stderr). The `LOUD_WIRE` environment variable
    /// additionally installs the standard stderr printer, exactly like the
    /// Interactions client.
    #[must_use]
    pub fn add_wire_inspector(mut self, inspector: Arc<dyn WireInspector>) -> Self {
        self.inspectors.push(inspector);
        self
    }

    /// Adds a recurring client-side trigger: after `spawn()`, a timer task
    /// sends [`InputEvent::AutomatedTrigger`](protocol::InputEvent) with the
    /// trigger's message every interval, **deferred while a turn is in
    /// flight** (missed intervals collapse into one delivery). See
    /// [`triggers`] for the full delivery semantics and
    /// lifecycle guarantees.
    ///
    /// Intervals must be non-zero; `spawn()` fails with
    /// [`AntigravityError::Config`] otherwise.
    #[must_use]
    pub fn add_trigger(mut self, trigger: TriggerConfig) -> Self {
        self.triggers.push(trigger);
        self
    }

    /// Adds a static subagent the parent agent can delegate to via the
    /// `start_subagent` builtin (enable it with [`Self::with_capabilities`];
    /// it is write-capable, so the safety gate requires a policy or
    /// pre-tool hook).
    ///
    /// Custom tools listed on the subagent (by name) must also be
    /// registered on this builder (`add_tool` / `with_tool_service`);
    /// `spawn()` fails with [`AntigravityError::Config`] on a dangling
    /// reference or a duplicate subagent name.
    #[must_use]
    pub fn add_subagent(mut self, subagent: Subagent) -> Self {
        self.subagents.push(subagent);
        self
    }

    /// Launches the harness, performs the handshake, connects the
    /// WebSocket, and initializes the conversation.
    ///
    /// # Errors
    ///
    /// - [`AntigravityError::Config`] when write-capable built-ins or MCP
    ///   servers are enabled without any policy or pre-tool hook (safety
    ///   parity with the reference SDK), or when a model is configured
    ///   without an API key.
    /// - [`AntigravityError::HarnessNotFound`] when discovery fails.
    /// - [`AntigravityError::HandshakeFailed`] / [`AntigravityError::InitFailed`]
    ///   with the harness's stderr tail when startup fails.
    pub async fn spawn(self) -> Result<AntigravityAgent, AntigravityError> {
        // Safety parity with the reference SDK: refuse to run write-capable
        // agents with no policy and no pre-tool hook.
        if (self.capabilities.has_write_tools() || !self.mcp_servers.is_empty())
            && self.policies.is_empty()
            && self.pre_tool.is_none()
        {
            return Err(AntigravityError::Config(
                "write-capable built-in tools or MCP servers are enabled without a safety \
                 policy. Add `.add_policy(policy::allow_all())` to approve all tool calls, \
                 `.add_policy(policy::deny_all())` plus specific `policy::allow(..)` rules \
                 to selectively allow tools, or an `.on_pre_tool(..)` hook."
                    .to_string(),
            ));
        }
        if self.model.is_some() && self.api_key.is_none() {
            return Err(AntigravityError::Config(
                "a model is configured without an API key; call with_api_key(..)".to_string(),
            ));
        }
        if let Some(trigger) = self.triggers.iter().find(|t| t.interval.is_zero()) {
            return Err(AntigravityError::Config(format!(
                "trigger '{}' has a zero interval; trigger intervals must be non-zero",
                trigger.message
            )));
        }

        // Subagent validation: unique names, and every referenced custom
        // tool registered on the parent (the harness dispatches subagent
        // custom-tool calls through the parent's registry).
        let dispatcher = ToolDispatcher::new(self.tools.clone(), &self.tool_services);
        {
            let declared: HashSet<String> = dispatcher
                .harness_declarations()
                .into_iter()
                .filter_map(|tool| tool.name)
                .collect();
            let mut subagent_names = HashSet::new();
            for subagent in &self.subagents {
                if !subagent_names.insert(subagent.name().to_string()) {
                    return Err(AntigravityError::Config(format!(
                        "duplicate subagent name '{}'; subagent names must be unique",
                        subagent.name()
                    )));
                }
                if let Some(missing) = subagent
                    .tool_names()
                    .iter()
                    .find(|name| !declared.contains(name.as_str()))
                {
                    return Err(AntigravityError::Config(format!(
                        "subagent '{}' references custom tool '{missing}' which is not \
                         registered on the agent; custom tools used by subagents must also \
                         be added via add_tool(..) or with_tool_service(..)",
                        subagent.name()
                    )));
                }
            }
            if !self.subagents.is_empty()
                && !self.capabilities.is_enabled(BuiltinTool::StartSubagent)
            {
                tracing::warn!(
                    "Subagents are configured but the start_subagent builtin is disabled; \
                     the parent agent cannot invoke them. Enable it with \
                     with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))."
                );
            }
        }

        let binary = process::discover_harness(self.harness_path.as_deref())?;

        let mut inspectors = self.inspectors.clone();
        if std::env::var("LOUD_WIRE").is_ok() {
            inspectors.push(Arc::new(LoudWirePrinter::new()));
        }
        let wire = WireContext::new(inspectors);

        // Stdio handshake.
        let input_config = handshake::InputConfig {
            storage_directory: self.save_dir.clone().unwrap_or_default(),
            port: 0,
            bind_address: String::new(),
            client_info: Some(handshake::ClientInfo {
                language: "rust".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                language_version: env!("CARGO_PKG_RUST_VERSION").to_string(),
            }),
        };
        let (mut harness, output_config) =
            HarnessProcess::spawn(&binary, &input_config, &wire).await?;

        // WebSocket connect (retry with backoff).
        let session = match Session::connect(output_config.port, &output_config.api_key, wire).await
        {
            Ok(session) => session,
            Err(e) => {
                let stderr = harness.stderr_tail().await;
                harness.kill().await;
                return Err(AntigravityError::InitFailed {
                    message: e.to_string(),
                    stderr,
                });
            }
        };

        // Conversation init.
        let config = self.build_harness_config(&dispatcher);
        let init = protocol::InitializeConversationEvent {
            config: Some(config),
        };

        let mut agent = AntigravityAgent {
            harness,
            session,
            dispatcher,
            policy_engine: PolicyEngine::new(self.policies),
            pre_tool: self.pre_tool,
            post_tool: self.post_tool,
            conversation_id: None,
            initial_history: Vec::new(),
            turn_timeout: self.turn_timeout,
            idle: Arc::new(tokio::sync::watch::channel(true).0),
            trigger_fired: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            turn_sync: Arc::new(tokio::sync::Mutex::new(())),
            trigger_tasks: triggers::TriggerTasks::default(),
        };

        match agent.initialize(init).await {
            Ok(()) => {
                // Start trigger timers only once the conversation is live.
                // Each task writes through the shared sink handle (the same
                // out-of-band path CancelHandle uses) and watches the
                // agent's idle flag.
                for trigger in self.triggers {
                    let sink = agent.session.sink_handle();
                    let fired = Arc::clone(&agent.trigger_fired);
                    agent.trigger_tasks.push(triggers::spawn_trigger_task(
                        trigger,
                        agent.idle.subscribe(),
                        Arc::clone(&agent.turn_sync),
                        move |message| {
                            let sink = sink.clone();
                            let fired = Arc::clone(&fired);
                            async move {
                                // Flag before sending so the next chat can
                                // never miss an in-flight delivery (a false
                                // positive on send failure only costs a
                                // no-op drain).
                                fired.store(true, std::sync::atomic::Ordering::SeqCst);
                                sink.send(&InputEvent::AutomatedTrigger(message)).await
                            }
                        },
                    ));
                }
                Ok(agent)
            }
            Err(e) => {
                agent.session.close().await;
                agent.harness.kill().await;
                Err(e)
            }
        }
    }

    /// Whether workspace announcement is enabled (default on).
    fn announce_workspaces(&self) -> bool {
        self.workspace_announcement.unwrap_or(true)
    }

    fn build_harness_config(&self, dispatcher: &ToolDispatcher) -> protocol::HarnessConfig {
        let mut enabled_hooks = Vec::new();
        if !self.policies.is_empty() || self.pre_tool.is_some() {
            enabled_hooks.push(protocol::LifecycleHook::PreTool);
        }
        if self.post_tool.is_some() {
            enabled_hooks.push(protocol::LifecycleHook::PostTool);
        }

        let models = self
            .api_key
            .as_ref()
            .map(|api_key| {
                vec![protocol::ModelConfig {
                    name: Some(
                        self.model
                            .clone()
                            .unwrap_or_else(|| "gemini-3-flash-preview".to_string()),
                    ),
                    types: vec![protocol::ModelType::Text],
                    gemini_api_endpoint: Some(protocol::GeminiApiEndpoint {
                        api_key: Some(api_key.clone()),
                        ..Default::default()
                    }),
                    vertex_endpoint: None,
                }]
            })
            .unwrap_or_default();

        let tools = dispatcher.harness_declarations();
        // Announce the workspace roots to the model (opt-outable). The note
        // is composed here at send time; the stored `system_instructions`
        // string is never mutated. Subagents get the same note appended
        // (their trajectories don't inherit the parent's context).
        let workspace_note = (self.announce_workspaces() && !self.workspaces.is_empty())
            .then(|| workspace_announcement(&self.workspaces));
        let custom_subagents = self
            .subagents
            .iter()
            .map(|subagent| subagent.to_wire(&tools, workspace_note.as_deref()))
            .collect();

        protocol::HarnessConfig {
            cascade_id: self.conversation_id.clone(),
            system_instructions: build_system_instructions(
                self.system_instructions.as_deref(),
                workspace_note.as_deref(),
            ),
            tools,
            harness_side_tools: Some(self.capabilities.to_harness_side_tools()),
            compaction_threshold: None,
            workspaces: self
                .workspaces
                .iter()
                .map(protocol::Workspace::filesystem)
                .collect(),
            skills_paths: self.skills_paths.clone(),
            finish_tool_schema_json: self.response_schema.as_ref().map(ToString::to_string),
            initial_trajectory: None,
            app_data_dir: self.app_data_dir.clone(),
            mcp_servers: self.mcp_servers.iter().map(McpServer::to_wire).collect(),
            models,
            enabled_hooks,
            custom_subagents,
        }
    }
}

// =============================================================================
// Agent
// =============================================================================

/// A running Antigravity agent session.
///
/// Created with [`AntigravityAgent::builder`]. One turn runs at a time:
/// [`chat`](Self::chat) drives it to completion, and
/// [`send_streaming`](Self::send_streaming) exposes the same loop as an
/// event stream. Call [`shutdown`](Self::shutdown) for a graceful exit (the
/// harness then persists trajectories); dropping the agent kills the
/// harness process without persistence.
pub struct AntigravityAgent {
    harness: HarnessProcess,
    session: Session,
    dispatcher: ToolDispatcher,
    policy_engine: PolicyEngine,
    pre_tool: Option<PreToolHook>,
    post_tool: Option<PostToolHook>,
    conversation_id: Option<String>,
    initial_history: Vec<StepUpdate>,
    turn_timeout: Option<Duration>,
    /// `true` while no turn is being driven; trigger tasks watch this to
    /// defer deliveries (see [`triggers`]).
    idle: Arc<tokio::sync::watch::Sender<bool>>,
    /// Set by trigger tasks on each delivery. A delivered trigger starts a
    /// harness-side turn nobody consumes; the next `chat`/`send_streaming`
    /// checks this flag and halts-and-drains that turn before sending its
    /// own input, so stale events cannot desync the user's turn.
    trigger_fired: Arc<std::sync::atomic::AtomicBool>,
    /// Serializes trigger delivery against turn begin. A trigger task
    /// holds this lock across [idle re-check → set `trigger_fired` →
    /// send]; [`begin_turn`](Self::begin_turn) holds it across [mark busy
    /// → consume `trigger_fired`]. Without it, a trigger that passed its
    /// idle check could deliver *after* the new turn consumed the flag,
    /// injecting its message into the user's turn (see [`triggers`]).
    turn_sync: Arc<tokio::sync::Mutex<()>>,
    /// Timer tasks spawned for [`AgentBuilder::add_trigger`] configs;
    /// aborted on shutdown and on drop.
    trigger_tasks: triggers::TriggerTasks,
}

impl std::fmt::Debug for AntigravityAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AntigravityAgent")
            .field("harness", &self.harness)
            .field("conversation_id", &self.conversation_id)
            .field("dispatcher", &self.dispatcher)
            .finish_non_exhaustive()
    }
}

/// Cancels an in-flight turn from outside the turn-driving future (e.g.
/// while consuming an [`AgentEventStream`]). Obtain via
/// [`AntigravityAgent::cancel_handle`]; cheap to clone.
///
/// Cancellation makes the in-flight `chat`/stream fail with
/// [`AntigravityError::Turn`] once the harness confirms.
#[derive(Debug, Clone)]
pub struct CancelHandle {
    sink: SinkHandle,
}

impl CancelHandle {
    /// Sends a halt request for the current turn.
    pub async fn cancel(&self) -> Result<(), AntigravityError> {
        self.sink.send(&InputEvent::HaltRequest(true)).await
    }
}

impl AntigravityAgent {
    /// Starts building an agent.
    #[must_use]
    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    /// The conversation id assigned by the harness. Persist it together
    /// with [`AgentBuilder::with_save_dir`] to resume the session later.
    #[must_use]
    pub fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    /// Steps restored from a saved conversation, when resuming.
    #[must_use]
    pub fn initial_history(&self) -> &[StepUpdate] {
        &self.initial_history
    }

    /// Returns a handle that can cancel an in-flight turn.
    #[must_use]
    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle {
            sink: self.session.sink_handle(),
        }
    }

    /// Cancels the current turn (equivalent to
    /// [`CancelHandle::cancel`]).
    pub async fn cancel(&self) -> Result<(), AntigravityError> {
        self.session.send(&InputEvent::HaltRequest(true)).await
    }

    /// Sends a message and drives the turn to completion.
    ///
    /// If a [trigger](AgentBuilder::add_trigger) started a harness-side turn
    /// since the last call, that turn is halted and its events are discarded
    /// before this message is sent (see [`triggers`]).
    pub async fn chat(
        &mut self,
        prompt: impl Into<String>,
    ) -> Result<ChatResponse, AntigravityError> {
        // Mark busy before sending so a trigger cannot slip in between.
        let guard = self.begin_turn().await;
        self.session
            .send(&InputEvent::UserInput(prompt.into()))
            .await?;
        let mut turn = TurnState::new(self.turn_timeout, Some(guard));
        loop {
            match self.next_turn_event(&mut turn).await? {
                Some(AgentEvent::Finished(response)) => return Ok(*response),
                Some(_) => {}
                None => {
                    return Err(AntigravityError::Protocol(
                        "turn ended without a Finished event".to_string(),
                    ));
                }
            }
        }
    }

    /// Sends a message and returns a stream of [`AgentEvent`]s for the
    /// turn. The stream ends after [`AgentEvent::Finished`].
    ///
    /// If a [trigger](AgentBuilder::add_trigger) started a harness-side turn
    /// since the last call, that turn is halted and its events are discarded
    /// before this message is sent (see [`triggers`]).
    pub async fn send_streaming(
        &mut self,
        prompt: impl Into<String>,
    ) -> Result<AgentEventStream<'_>, AntigravityError> {
        // Mark busy before sending so a trigger cannot slip in between.
        // The guard moves into the stream's turn state: dropping the
        // stream mid-turn marks the agent idle again.
        let guard = self.begin_turn().await;
        self.session
            .send(&InputEvent::UserInput(prompt.into()))
            .await?;
        let timeout = self.turn_timeout;
        let stream = async_stream::try_stream! {
            let mut turn = TurnState::new(timeout, Some(guard));
            while let Some(event) = self.next_turn_event(&mut turn).await? {
                let finished = matches!(event, AgentEvent::Finished(_));
                yield event;
                if finished {
                    break;
                }
            }
        };
        Ok(AgentEventStream::new(Box::pin(stream)))
    }

    /// Gracefully shuts down: closes the WebSocket (the harness serializes
    /// its trajectory), closes stdin (EOF triggers the harness's clean
    /// exit), then escalates to SIGTERM and SIGKILL if it lingers.
    pub async fn shutdown(mut self) -> Result<(), AntigravityError> {
        // Stop trigger timers first so nothing writes to the closing socket.
        self.trigger_tasks.abort_all();
        self.session.close().await;
        self.harness.shutdown().await
    }

    // -------------------------------------------------------------------
    // Init
    // -------------------------------------------------------------------

    async fn initialize(
        &mut self,
        init: protocol::InitializeConversationEvent,
    ) -> Result<(), AntigravityError> {
        self.session.send_raw(serde_json::to_value(&init)?).await?;

        let deadline = tokio::time::Instant::now() + INIT_TIMEOUT;
        loop {
            let event = match tokio::time::timeout_at(deadline, self.session.next_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Ok(None)) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::InitFailed {
                        message: "harness closed the connection during initialization".to_string(),
                        stderr,
                    });
                }
                Ok(Err(e)) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::InitFailed {
                        message: e.to_string(),
                        stderr,
                    });
                }
                Err(_) => {
                    let stderr = self.harness.stderr_tail().await;
                    return Err(AntigravityError::InitFailed {
                        message: format!("no initialization response within {INIT_TIMEOUT:?}"),
                        stderr,
                    });
                }
            };
            match event.payload {
                Some(OutputPayload::InitializeConversationResponse(response)) => {
                    self.conversation_id = response.cascade_id.clone();
                    self.initial_history = response.history;
                    return Ok(());
                }
                Some(other) => {
                    tracing::debug!(
                        "Ignoring pre-init event while waiting for initialization: {other:?}"
                    );
                }
                None => {}
            }
        }
    }

    // -------------------------------------------------------------------
    // Turn loop
    // -------------------------------------------------------------------

    /// Returns the next agent event for the running turn, or `None` once
    /// the turn has finished and all events were drained.
    async fn next_turn_event(
        &mut self,
        turn: &mut TurnState,
    ) -> Result<Option<AgentEvent>, AntigravityError> {
        loop {
            if let Some(event) = turn.queue.pop_front() {
                return Ok(Some(event));
            }
            if turn.finished {
                return Ok(None);
            }
            let next = self.session.next_event();
            let event = match turn.deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, next).await {
                    Ok(result) => result?,
                    Err(_) => {
                        // The harness is still driving this turn. Without a
                        // halt, its remaining events (including its terminal
                        // trajectory-idle) would be consumed by the *next*
                        // turn and desync every turn after it.
                        self.halt_and_drain("recovering from a turn timeout").await;
                        return Err(AntigravityError::Timeout {
                            operation: "agent turn".to_string(),
                            timeout: turn.timeout.unwrap_or_default(),
                        });
                    }
                },
                None => next.await?,
            };
            let Some(event) = event else {
                let stderr = self.harness.stderr_tail().await;
                return Err(AntigravityError::ConnectionClosed {
                    message: "harness closed the WebSocket mid-turn".to_string(),
                    stderr,
                });
            };
            self.process_event(event, turn).await?;
        }
    }

    /// Marks the agent busy and, when a trigger delivered since the last
    /// client-driven turn, halts and drains the trigger-initiated harness
    /// turn. Returns the guard that flips the agent back to idle on drop.
    ///
    /// Invariant (shared with [`triggers::spawn_trigger_task`]): flipping
    /// the idle flag and consuming `trigger_fired` happen under
    /// `turn_sync`, the same lock a trigger task holds across its [idle
    /// re-check → set flag → send] window. This makes the fire decision
    /// and the turn begin mutually exclusive — without it, a trigger that
    /// already passed its idle check could set the flag and send *after*
    /// the `swap` below returned `false`, delivering its message into the
    /// turn we are about to start with nobody ever draining it.
    async fn begin_turn(&mut self) -> TurnGuard {
        let (guard, trigger_fired) = {
            let _sync = self.turn_sync.lock().await;
            let guard = TurnGuard::begin(&self.idle);
            let fired = self
                .trigger_fired
                .swap(false, std::sync::atomic::Ordering::SeqCst);
            (guard, fired)
        };
        // Drain outside the critical section: the agent is already marked
        // busy, so trigger tasks defer and nothing new can fire meanwhile.
        if trigger_fired {
            self.halt_and_drain("discarding a trigger-initiated turn")
                .await;
        }
        guard
    }

    /// Best-effort recovery when the harness may be driving (or have
    /// finished) a turn whose events nobody will consume — a turn that hit
    /// its per-turn timeout, or one started by an automated trigger.
    ///
    /// Sends a halt, then drains events into a throwaway state — answering
    /// protocol-required requests (confirmations, hooks) so the harness
    /// never deadlocks, discarding everything surfaced — until the stream
    /// stays silent for [`HALT_DRAIN_SILENCE`] or [`HALT_DRAIN_BUDGET`]
    /// runs out. Stale-turn errors (including the halt's own cancellation)
    /// are logged and swallowed; only transport health matters here.
    async fn halt_and_drain(&mut self, why: &str) {
        tracing::debug!("Halting and draining the harness turn ({why}).");
        if let Err(e) = self.session.send(&InputEvent::HaltRequest(true)).await {
            tracing::warn!("Failed to send halt request while {why}: {e}");
            return;
        }
        let mut state = TurnState::new(None, None);
        let deadline = tokio::time::Instant::now() + HALT_DRAIN_BUDGET;
        loop {
            let silence = tokio::time::Instant::now() + HALT_DRAIN_SILENCE;
            match tokio::time::timeout_at(silence.min(deadline), self.session.next_event()).await {
                Err(_) => {
                    // Silent for the grace window: drained (or nothing was
                    // running and the halt was a no-op). A budget overrun
                    // instead means the stale turn is still streaming.
                    if tokio::time::Instant::now() >= deadline {
                        tracing::warn!(
                            "Drain budget exhausted while {why}; the next turn may still \
                             observe stale events."
                        );
                    }
                    return;
                }
                Ok(Ok(Some(event))) => {
                    match self.process_event(event, &mut state).await {
                        Ok(()) => state.queue.clear(),
                        Err(AntigravityError::Turn(message)) => {
                            // The stale turn's cancellation or its own
                            // fatal error: expected terminals. Keep
                            // draining until silence in case more turns
                            // (a second trigger) are queued behind it.
                            tracing::debug!("Drained stale-turn terminal ({why}): {message}");
                        }
                        Err(e) => {
                            tracing::warn!("Error while draining stale turn ({why}): {e}");
                            return;
                        }
                    }
                }
                Ok(Ok(None)) => {
                    tracing::warn!("Harness closed the connection while {why}.");
                    return;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Transport error while draining stale turn ({why}): {e}");
                    return;
                }
            }
        }
    }

    async fn process_event(
        &mut self,
        event: OutputEvent,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        if let Some(usage) = event.usage_metadata {
            turn.usage = Some(usage);
        }
        match event.payload {
            Some(OutputPayload::StepUpdate(step)) => self.process_step(*step, turn).await?,
            Some(OutputPayload::TrajectoryStateUpdate(update)) => {
                Self::process_trajectory_update(&update, turn)?;
            }
            Some(OutputPayload::ToolCall(call)) => self.process_tool_call(call, turn).await?,
            Some(OutputPayload::CallHookRequest(request)) => {
                self.process_hook_request(request).await?;
            }
            Some(OutputPayload::SessionEndResponse(_)) => {}
            Some(OutputPayload::InitializeConversationResponse(_)) => {
                tracing::warn!("Unexpected initializeConversationResponse mid-turn; ignoring.");
            }
            Some(OutputPayload::Unknown { event_type, data }) => {
                tracing::warn!("Unknown harness event '{event_type}'; surfacing and continuing.");
                turn.queue
                    .push_back(AgentEvent::Unknown { event_type, data });
            }
            None => {}
        }
        Ok(())
    }

    async fn process_step(
        &mut self,
        step: StepUpdate,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        // Proto3 JSON omits default-valued scalars, so an absent
        // `step_index` *is* index 0 (and an absent `trajectory_id` is the
        // empty id) — mapping `None` to the default is the correct
        // decoding, not a key collision between distinct steps.
        let step_key = (
            step.trajectory_id.clone().unwrap_or_default(),
            step.step_index.unwrap_or_default(),
        );
        if turn.main_trajectory.is_none()
            && let Some(trajectory_id) = &step.trajectory_id
        {
            turn.main_trajectory = Some(trajectory_id.clone());
        }
        let is_main = turn.main_trajectory.as_deref() == step.trajectory_id.as_deref();

        // Debounce bookkeeping: leaving the waiting state clears the
        // handled-request markers for the step.
        if step.state != Some(StepState::WaitingForUser) {
            turn.handled_waits.remove(&step_key);
        }

        // Deltas stream through from every trajectory (subagents included).
        if let Some(delta) = &step.thinking_delta
            && !delta.is_empty()
        {
            turn.queue
                .push_back(AgentEvent::ThinkingDelta(delta.clone()));
        }
        if let Some(delta) = &step.text_delta
            && !delta.is_empty()
        {
            turn.queue.push_back(AgentEvent::TextDelta(delta.clone()));
        }

        let is_terminal = matches!(step.state, Some(StepState::Done) | Some(StepState::Error));

        // Completed tool actions surface once. A denied action was already
        // announced (with its `Denied` decision) at its confirmation step,
        // so `announced_actions` dedups it here; anything reaching a
        // terminal state executed, so its decision is `Allowed`.
        if is_terminal
            && let Some(action) = streaming::ToolAction::from_step(&step)
            && turn.announced_actions.insert(step_key.clone())
        {
            turn.queue.push_back(AgentEvent::ToolAction {
                action: Box::new(action),
                decision: ToolDecision::Allowed,
                trajectory_id: step.trajectory_id.clone(),
            });
        }

        // Structured output from the finish action.
        if let Some(finish) = &step.finish
            && let Some(output) = &finish.output_string
            && !output.is_empty()
        {
            match serde_json::from_str(output) {
                Ok(value) => turn.structured_output = Some(value),
                Err(e) => tracing::warn!("Failed to parse structured output JSON: {e}"),
            }
        }

        // Errors: fatal model-backend codes abort the turn; everything else
        // is surfaced as an event and recorded (the harness retries or the
        // model reacts).
        if step.state == Some(StepState::Error) || step.error.is_some() {
            let message = step
                .error
                .as_ref()
                .and_then(|e| e.error_message.clone())
                .or_else(|| step.error_message.clone())
                .or_else(|| step.text.clone())
                .unwrap_or_else(|| "unknown harness error".to_string());
            let http_code = step.error.as_ref().and_then(|e| e.http_code);
            if step.source == Some(StepSource::System)
                && http_code.is_some_and(|code| FATAL_HTTP_CODES.contains(&code))
            {
                return Err(AntigravityError::Turn(format!(
                    "model backend error (HTTP {}): {message}",
                    http_code.unwrap_or_default()
                )));
            }
            turn.errors.push(message.clone());
            // Reaching here means the turn continues: the turn-aborting case
            // (system-source fatal HTTP code) returned above. A fatal code
            // that did *not* abort (non-system source) is still serious, so
            // it surfaces as `Terminal`; everything else is transient
            // harness-internal noise (retried internally, model reacts).
            let severity = classify_error_severity(http_code);
            turn.queue
                .push_back(AgentEvent::Error { message, severity });
        }

        // Thinking text accumulates from completed steps.
        if step.state == Some(StepState::Done)
            && let Some(thinking) = &step.thinking
            && !thinking.is_empty()
            && turn.thought_steps.insert(step_key.clone())
        {
            if !turn.thoughts.is_empty() {
                turn.thoughts.push('\n');
            }
            turn.thoughts.push_str(thinking);
        }

        // Final-response candidate: completed model text directed at the
        // user, on the main trajectory. The last one wins.
        if is_main
            && step.source == Some(StepSource::Model)
            && step.state == Some(StepState::Done)
            && step.target == Some(StepTarget::User)
            && let Some(text) = &step.text
            && !text.is_empty()
        {
            turn.final_text = Some(text.clone());
        }

        // Waiting state: answer confirmation/question requests, debounced —
        // the harness re-broadcasts them on every internal tick.
        if step.state == Some(StepState::WaitingForUser) {
            if step.tool_confirmation_request.is_some()
                && turn.mark_wait_handled(&step_key, "tool_confirmation_request")
            {
                self.answer_tool_confirmation(&step, &step_key, turn)
                    .await?;
            }
            if let Some(questions) = &step.questions_request
                && turn.mark_wait_handled(&step_key, "questions_request")
            {
                self.answer_questions(&step, questions.questions.len())
                    .await?;
            }
        }
        Ok(())
    }

    /// Policy-checks a pending harness-side tool and replies with a
    /// `tool_confirmation`. When the decision is a denial, the blocked
    /// action is surfaced as a [`AgentEvent::ToolAction`] with a
    /// [`ToolDecision::Denied`] marker (deduped against the terminal-step
    /// emission via `announced_actions`) so consumers see denied actions
    /// distinctly — a denied action still reaches a terminal step, but with
    /// no signal that it was blocked.
    async fn answer_tool_confirmation(
        &mut self,
        step: &StepUpdate,
        step_key: &(String, u32),
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let decision = confirmation_decision(step, &self.policy_engine, self.pre_tool.as_ref());
        let accepted = matches!(decision, PreToolDecision::Allow);
        if let PreToolDecision::Deny { reason } = &decision
            && let Some(action) = streaming::ToolAction::from_step(step)
            && turn.announced_actions.insert(step_key.clone())
        {
            turn.queue.push_back(AgentEvent::ToolAction {
                action: Box::new(action),
                decision: ToolDecision::Denied {
                    reason: reason.clone(),
                },
                trajectory_id: step.trajectory_id.clone(),
            });
        }
        self.session
            .send(&InputEvent::ToolConfirmation(protocol::ToolConfirmation {
                trajectory_id: step.trajectory_id.clone().unwrap_or_default(),
                step_index: step.step_index.unwrap_or_default(),
                accepted,
            }))
            .await
    }

    /// Replies to a `questions_request`. Interactive question handling is
    /// not supported yet; every question is answered "unanswered" so the
    /// harness never deadlocks (the protocol requires a response).
    async fn answer_questions(
        &mut self,
        step: &StepUpdate,
        question_count: usize,
    ) -> Result<(), AntigravityError> {
        tracing::warn!(
            "Harness asked {question_count} user question(s) but interactive question \
             handling is not supported; answering as unanswered. Disable the \
             ask_question builtin (Capabilities) to prevent this."
        );
        let response = protocol::UserQuestionsResponse {
            trajectory_id: step.trajectory_id.clone().unwrap_or_default(),
            step_index: step.step_index.unwrap_or_default(),
            cancelled: None,
            response: Some(protocol::QuestionsResponse {
                answers: (0..question_count)
                    .map(|_| protocol::UserQuestionAnswer::unanswered())
                    .collect(),
            }),
        };
        self.session
            .send(&InputEvent::QuestionResponse(response))
            .await
    }

    /// Routes a trajectory lifecycle update into the turn state.
    ///
    /// Only the *main* trajectory's terminal states decide the turn's
    /// fate: subagent trajectories go idle (and can be cancelled, e.g. by
    /// a pre-turn hook denial) while the parent keeps running — subagent
    /// failures surface through their step errors, not here. Associated
    /// fn (no `&self`) so the routing logic is unit-testable.
    fn process_trajectory_update(
        update: &protocol::TrajectoryStateUpdate,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let is_main = turn.main_trajectory.is_none()
            || turn.main_trajectory.as_deref() == update.trajectory_id.as_deref();
        match update.state {
            Some(TrajectoryState::Idle) if is_main => {
                if let Some(error) = &update.error {
                    return Err(AntigravityError::Turn(error.clone()));
                }
                let response = turn.take_response();
                turn.finished = true;
                turn.queue
                    .push_back(AgentEvent::Finished(Box::new(response)));
            }
            Some(TrajectoryState::Cancelled) if is_main => {
                let message = update
                    .error
                    .clone()
                    .unwrap_or_else(|| "turn cancelled".to_string());
                return Err(AntigravityError::Turn(message));
            }
            _ => {
                // Running / subagent idle or cancelled / unknown states:
                // nothing to do for the parent turn.
            }
        }
        Ok(())
    }

    /// Dispatches a custom (client-executed) tool call: policy check first
    /// (defense in depth), then execution through the crate's function
    /// registry / tool services, then a `tool_response` back to the
    /// harness.
    async fn process_tool_call(
        &mut self,
        call: protocol::ToolCall,
        turn: &mut TurnState,
    ) -> Result<(), AntigravityError> {
        let id = call.id.clone().unwrap_or_default();
        let name = call.name.clone().unwrap_or_default();
        let args: Value = match call
            .arguments_json
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(serde_json::from_str)
        {
            Some(Ok(value)) => value,
            Some(Err(e)) => {
                tracing::warn!("Unparseable arguments for tool '{name}': {e}");
                call.arguments.clone().unwrap_or(Value::Null)
            }
            None => call
                .arguments
                .clone()
                .unwrap_or_else(|| Value::Object(Default::default())),
        };

        let invocation = ToolInvocation {
            name: name.clone(),
            args: args.clone(),
            id: Some(id.clone()),
        };
        let result = match hooks::decide(&self.policy_engine, self.pre_tool.as_ref(), &invocation) {
            PreToolDecision::Deny { reason } => {
                tracing::info!("Denying custom tool '{name}': {reason}");
                serde_json::json!({
                    "error": format!("Tool execution denied by policy: {reason}")
                })
            }
            PreToolDecision::Allow => {
                let result = self.dispatcher.execute(&name, args).await;
                if let Some(post_tool) = &self.post_tool {
                    // Hand the hook the inner value, not the `{"result": ...}`
                    // wire envelope the harness expects (Item: unwrap
                    // ToolOutcome.result). The error branch keeps the
                    // envelope's error string.
                    let error = result
                        .get("error")
                        .and_then(Value::as_str)
                        .map(ToString::to_string);
                    let outcome = ToolOutcome {
                        name: name.clone(),
                        result: error.is_none().then(|| unwrap_result_value(&result)),
                        error,
                    };
                    post_tool(&outcome);
                }
                turn.queue.push_back(AgentEvent::ToolCallDispatched {
                    name: name.clone(),
                    id: id.clone(),
                });
                result
            }
        };
        self.session
            .send(&InputEvent::ToolResponse(protocol::ToolResponse {
                id,
                response_json: Some(result.to_string()),
                supplemental_media: Vec::new(),
            }))
            .await
    }

    /// Answers a lifecycle hook callback. The protocol requires a response
    /// for every request, so unknown hook kinds get an `empty_result`.
    async fn process_hook_request(
        &mut self,
        request: protocol::CallHookRequest,
    ) -> Result<(), AntigravityError> {
        let request_id = request.request_id.clone().unwrap_or_default();
        let mut response = protocol::CallHookResponse {
            request_id,
            ..Default::default()
        };
        if let Some(pre_tool_args) = &request.pre_tool_args {
            let name = pre_tool_args.tool_name.clone().unwrap_or_default();
            let args = pre_tool_args
                .arguments_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| Value::Object(Default::default()));
            let invocation = ToolInvocation {
                name,
                args,
                id: None,
            };
            let verdict =
                match hooks::decide(&self.policy_engine, self.pre_tool.as_ref(), &invocation) {
                    PreToolDecision::Allow => HookVerdict {
                        decision: Some(HookDecision::Allow),
                        reason: None,
                    },
                    PreToolDecision::Deny { reason } => HookVerdict {
                        decision: Some(HookDecision::Deny),
                        reason: Some(reason),
                    },
                };
            response.pre_tool_result = Some(verdict);
        } else if let Some(post_tool_args) = &request.post_tool_args {
            if let Some(post_tool) = &self.post_tool {
                post_tool(&ToolOutcome {
                    name: post_tool_args.tool_name.clone().unwrap_or_default(),
                    // Unwrap the `{"result": ...}` envelope for consistency
                    // with the custom-tool dispatch path.
                    result: post_tool_args.result.as_deref().map(unwrap_result_string),
                    error: post_tool_args.error.clone(),
                });
            }
            response.empty_result = Some(protocol::EmptyResult {});
        } else {
            response.empty_result = Some(protocol::EmptyResult {});
        }
        self.session
            .send(&InputEvent::CallHookResponse(response))
            .await
    }
}

// =============================================================================
// Tool-confirmation decision
// =============================================================================

/// Inserts the step's free-text confirmation prompt into the args map, when
/// both are present.
fn insert_request_text(step: &StepUpdate, args: &mut Value) {
    if let Some(request_text) = &step.request_text
        && let Value::Object(map) = args
    {
        map.insert(
            "request_text".to_string(),
            Value::String(request_text.clone()),
        );
    }
}

/// Decides whether a pending harness-side `tool_confirmation_request` is
/// accepted. Pure decision logic, separated from the wire reply for unit
/// testing.
///
/// Three cases (the request itself is an empty marker on the wire — the
/// step's action fields are the only discriminator, verified against the
/// harness 0.1.5 proto):
///
/// 1. **Recognized action** — the normal policy/hook decision.
/// 2. **No action and no unrecognized step fields** — a pre-request
///    notification for a host-side (client-executed) tool. Auto-approved,
///    mirroring the reference SDK: the concrete call follows as a
///    `tool_call` with its own policy check, so nothing is bypassed.
/// 3. **No recognized action but unrecognized step fields** — most likely a
///    harness builtin newer than this client, whose confirmation is its
///    *only* gate. Fails closed unless a policy rule (wildcard `allow_all`
///    or an exact rule naming the unknown wire field) or the pre-tool hook
///    allows it; a `warn!` records the unknown fields either way
///    (Evergreen: reply and continue — never deadlock the harness).
fn confirmation_decision(
    step: &StepUpdate,
    engine: &PolicyEngine,
    pre_tool: Option<&PreToolHook>,
) -> PreToolDecision {
    if let Some(action) = streaming::ToolAction::from_step(step) {
        let name = action.tool_name();
        let mut args = action.args();
        insert_request_text(step, &mut args);
        let invocation = ToolInvocation {
            name: name.clone(),
            args,
            id: None,
        };
        let decision = hooks::decide(engine, pre_tool, &invocation);
        if let PreToolDecision::Deny { reason } = &decision {
            tracing::info!("Rejecting harness tool '{name}': {reason}");
        }
        return decision;
    }

    if step.extra.is_empty() {
        // Case 2: genuine pre-request notification.
        tracing::debug!("Auto-approving a pre-request host-tool confirmation.");
        return PreToolDecision::Allow;
    }

    // Case 3: unrecognized fields — evaluate policies against the first
    // unknown wire field name (a new action field is the expected shape).
    let mut keys: Vec<&str> = step.extra.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let name = keys.first().copied().unwrap_or_default().to_string();
    let mut args = Value::Object(step.extra.clone());
    insert_request_text(step, &mut args);
    let invocation = ToolInvocation {
        name: name.clone(),
        args,
        id: None,
    };
    let decision = hooks::decide_with_default(
        engine,
        pre_tool,
        &invocation,
        PreToolDecision::deny(
            "unrecognized tool confirmation matched no policy rule (failing closed)",
        ),
    );
    let accepted = matches!(decision, PreToolDecision::Allow);
    tracing::warn!(
        "Unrecognized tool confirmation (unknown step fields {keys:?}); {} '{name}'. \
         Add a policy rule for the wire field name to control this explicitly.",
        if accepted { "allowing" } else { "denying" }
    );
    decision
}

/// Classifies a harness error step's severity for
/// [`AgentEvent::Error`](streaming::AgentEvent). A fatal model-backend
/// HTTP code (400/401/403) that reached the event path — i.e. did *not* abort
/// the turn through the system-source [`AntigravityError::Turn`] path — is
/// still serious and surfaces as [`ErrorSeverity::Terminal`]; every other
/// error is transient harness-internal noise the turn recovers from.
fn classify_error_severity(http_code: Option<u32>) -> ErrorSeverity {
    if http_code.is_some_and(|code| FATAL_HTTP_CODES.contains(&code)) {
        ErrorSeverity::Terminal
    } else {
        ErrorSeverity::Transient
    }
}

/// Builds the workspace-announcement note (Item: announce workspace roots).
/// A concise, clearly delimited block listing the configured root(s) so the
/// model uses real paths instead of guessing. The roots are announced exactly
/// as configured (the same strings the harness roots its tools at), so the
/// wording makes no claim about them being absolute — a caller may pass a
/// relative path, and the model must use the same string the harness does.
fn workspace_announcement(workspaces: &[String]) -> String {
    let roots = workspaces
        .iter()
        .map(|root| format!("- {root}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "=== Workspace ===\n\
         Your workspace is rooted at the following path(s). Use these exact \
         paths for all file and directory operations; do not guess or invent \
         workspace paths, and stay within these root(s):\n{roots}\n\
         === End Workspace ==="
    )
}

/// Composes the effective parent system instructions from the user's
/// instructions (never mutated) and an optional workspace-announcement note.
///
/// - With a note and user instructions: the note is a second `custom` part
///   (keeps the user's text intact, still fully custom).
/// - With a note and no user instructions: the note is an *appended* section
///   so the harness's default instructions are preserved (not replaced).
/// - With no note: the user's instructions as fully-custom text, or `None`.
fn build_system_instructions(
    user_instructions: Option<&str>,
    workspace_note: Option<&str>,
) -> Option<protocol::SystemInstructions> {
    match (user_instructions, workspace_note) {
        (Some(text), Some(note)) => Some(protocol::SystemInstructions {
            custom: Some(protocol::CustomSystemInstructions {
                part: vec![
                    protocol::SystemInstructionPart {
                        text: Some(text.to_string()),
                    },
                    protocol::SystemInstructionPart {
                        text: Some(note.to_string()),
                    },
                ],
            }),
            appended: None,
        }),
        (Some(text), None) => Some(protocol::SystemInstructions::custom_text(text.to_string())),
        (None, Some(note)) => Some(protocol::SystemInstructions {
            custom: None,
            appended: Some(protocol::AppendedSystemInstructions {
                custom_identity: None,
                appended_sections: vec![protocol::InstructionSection {
                    title: Some("Workspace".to_string()),
                    content: Some(note.to_string()),
                }],
            }),
        }),
        (None, None) => None,
    }
}

/// Unwraps the dispatcher's result envelope for a post-tool hook. A scalar
/// tool return is wrapped as `{"result": X}` before going to the harness;
/// hooks want the inner `X` (its string form, or the value serialized when
/// non-string). A verbatim object return (any shape other than a lone
/// `result` key) is passed through serialized.
///
/// Edge case: a custom tool that legitimately returns a single-key object
/// `{"result": X}` is indistinguishable from a wrapped scalar and will be
/// unwrapped to `X` — the same inherent ambiguity as the dispatcher's
/// scalar-wrapping. Return a multi-key object to preserve the outer shape.
fn unwrap_result_value(value: &Value) -> String {
    if let Value::Object(map) = value
        && map.len() == 1
        && let Some(inner) = map.get("result")
    {
        return match inner {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    value.to_string()
}

/// Like [`unwrap_result_value`], for a harness-supplied result *string*
/// (the `PostToolArgs.result` wire field). Only a lone-`result` JSON object
/// envelope is unwrapped; any other payload (plain text, a multi-key object)
/// is handed back verbatim.
fn unwrap_result_string(raw: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(raw)
        && let Value::Object(map) = &value
        && map.len() == 1
        && map.contains_key("result")
    {
        return unwrap_result_value(&value);
    }
    raw.to_string()
}

// =============================================================================
// Per-turn state
// =============================================================================

/// RAII marker for "a turn is being driven": construction flips the
/// agent's idle flag to busy, drop (turn completion, error, timeout, or a
/// dropped mid-turn stream) flips it back to idle, releasing any deferred
/// trigger deliveries.
struct TurnGuard {
    idle: Arc<tokio::sync::watch::Sender<bool>>,
}

impl TurnGuard {
    fn begin(idle: &Arc<tokio::sync::watch::Sender<bool>>) -> Self {
        idle.send_replace(false);
        Self {
            idle: Arc::clone(idle),
        }
    }
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.idle.send_replace(true);
    }
}

struct TurnState {
    queue: VecDeque<AgentEvent>,
    finished: bool,
    main_trajectory: Option<String>,
    handled_waits: HashMap<(String, u32), HashSet<&'static str>>,
    announced_actions: HashSet<(String, u32)>,
    thought_steps: HashSet<(String, u32)>,
    final_text: Option<String>,
    thoughts: String,
    usage: Option<protocol::UsageMetadata>,
    structured_output: Option<Value>,
    errors: Vec<String>,
    timeout: Option<Duration>,
    deadline: Option<tokio::time::Instant>,
    /// Held for the turn's lifetime; dropping the state marks the agent
    /// idle again (releasing deferred trigger deliveries). `None` for
    /// throwaway drain states, which run *inside* a caller that already
    /// holds the real guard.
    _turn_guard: Option<TurnGuard>,
}

impl TurnState {
    fn new(timeout: Option<Duration>, turn_guard: Option<TurnGuard>) -> Self {
        Self {
            queue: VecDeque::new(),
            finished: false,
            main_trajectory: None,
            handled_waits: HashMap::new(),
            announced_actions: HashSet::new(),
            thought_steps: HashSet::new(),
            final_text: None,
            thoughts: String::new(),
            usage: None,
            structured_output: None,
            errors: Vec::new(),
            timeout,
            deadline: timeout.map(|t| tokio::time::Instant::now() + t),
            _turn_guard: turn_guard,
        }
    }

    /// Marks a waiting-state request as handled for the step; returns
    /// `true` on first sighting (the harness re-broadcasts requests on
    /// every internal tick while waiting).
    fn mark_wait_handled(&mut self, step_key: &(String, u32), kind: &'static str) -> bool {
        self.handled_waits
            .entry(step_key.clone())
            .or_default()
            .insert(kind)
    }

    fn take_response(&mut self) -> ChatResponse {
        ChatResponse {
            text: self.final_text.take().unwrap_or_default(),
            thoughts: std::mem::take(&mut self.thoughts),
            usage: self.usage.take(),
            structured_output: self.structured_output.take(),
            errors: std::mem::take(&mut self.errors),
        }
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;

    fn trajectory_update(
        trajectory_id: Option<&str>,
        state: TrajectoryState,
        error: Option<&str>,
    ) -> protocol::TrajectoryStateUpdate {
        protocol::TrajectoryStateUpdate {
            trajectory_id: trajectory_id.map(str::to_string),
            state: Some(state),
            error: error.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn test_main_trajectory_idle_finishes_turn() {
        let mut turn = TurnState::new(None, None);
        turn.main_trajectory = Some("main".to_string());
        turn.final_text = Some("done".to_string());
        let update = trajectory_update(Some("main"), TrajectoryState::Idle, None);
        AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
        assert!(turn.finished);
        assert!(matches!(
            turn.queue.pop_front(),
            Some(AgentEvent::Finished(_))
        ));
    }

    #[test]
    fn test_main_trajectory_cancelled_fails_turn() {
        let mut turn = TurnState::new(None, None);
        turn.main_trajectory = Some("main".to_string());
        let update = trajectory_update(Some("main"), TrajectoryState::Cancelled, Some("halted"));
        let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
        assert!(matches!(&err, AntigravityError::Turn(m) if m == "halted"));
    }

    #[test]
    fn test_cancelled_before_any_step_fails_turn() {
        // No main trajectory yet (e.g. a pre-turn hook denial cancels the
        // turn before its first step): treated as the main trajectory.
        let mut turn = TurnState::new(None, None);
        let update = trajectory_update(Some("t-0"), TrajectoryState::Cancelled, None);
        let err = AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap_err();
        assert!(matches!(&err, AntigravityError::Turn(m) if m == "turn cancelled"));
    }

    #[test]
    fn test_subagent_trajectory_cancelled_is_not_fatal() {
        // A cancelled subagent trajectory must not fail the parent's turn
        // (mirroring the subagent-idle no-op); subagent failures surface
        // through their step errors instead.
        let mut turn = TurnState::new(None, None);
        turn.main_trajectory = Some("main".to_string());
        let update = trajectory_update(Some("sub"), TrajectoryState::Cancelled, Some("denied"));
        AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
        assert!(!turn.finished);
        assert!(turn.queue.is_empty());
    }

    #[test]
    fn test_subagent_trajectory_idle_does_not_finish_turn() {
        let mut turn = TurnState::new(None, None);
        turn.main_trajectory = Some("main".to_string());
        let update = trajectory_update(Some("sub"), TrajectoryState::Idle, None);
        AntigravityAgent::process_trajectory_update(&update, &mut turn).unwrap();
        assert!(!turn.finished);
        assert!(turn.queue.is_empty());
    }

    #[tokio::test]
    async fn test_spawn_requires_policy_for_write_tools() {
        let err = AntigravityAgent::builder()
            .with_capabilities(Capabilities::all())
            .spawn()
            .await
            .unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("expected Config error, got {err:?}");
        };
        assert!(message.contains("safety"));
        assert!(message.contains("allow_all"));
    }

    #[tokio::test]
    async fn test_spawn_requires_policy_for_mcp_servers() {
        let err = AntigravityAgent::builder()
            .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]))
            .spawn()
            .await
            .unwrap_err();
        assert!(matches!(err, AntigravityError::Config(_)));
    }

    /// A harness path that exists but cannot be executed, so `spawn()`
    /// gets past the safety gate and discovery, then fails fast at process
    /// launch (without ever falling through to a real installed harness).
    fn unexecutable_harness() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("localharness");
        std::fs::write(&path, b"not a binary").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        (dir, path)
    }

    #[tokio::test]
    async fn test_spawn_write_tools_allowed_with_policy() {
        let (_dir, path) = unexecutable_harness();
        // With a policy the safety gate passes; the spawn then fails at
        // process launch, proving we got past the Config check.
        let err = AntigravityAgent::builder()
            .with_harness_path(&path)
            .with_capabilities(Capabilities::all())
            .add_policy(policy::allow_all())
            .spawn()
            .await
            .unwrap_err();
        assert!(
            matches!(err, AntigravityError::HandshakeFailed { .. }),
            "unexpected error: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_spawn_write_tools_allowed_with_pre_tool_hook() {
        let (_dir, path) = unexecutable_harness();
        let err = AntigravityAgent::builder()
            .with_harness_path(&path)
            .with_capabilities(Capabilities::all())
            .on_pre_tool(|_| PreToolDecision::Allow)
            .spawn()
            .await
            .unwrap_err();
        assert!(!matches!(err, AntigravityError::Config(_)));
    }

    #[tokio::test]
    async fn test_spawn_model_without_api_key_is_config_error() {
        let err = AntigravityAgent::builder()
            .with_model("gemini-3-flash-preview")
            .spawn()
            .await
            .unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("expected Config error, got {err:?}");
        };
        assert!(message.contains("with_api_key"));
    }

    #[test]
    fn test_builder_harness_config_assembly() {
        let builder = AntigravityAgent::builder()
            .with_api_key("test-key")
            .with_model("gemini-3-flash-preview")
            .with_system_instructions("Be brief.")
            .add_workspace("/w1")
            .add_workspace("/w2")
            .with_conversation_id("resume-me")
            .with_response_schema(serde_json::json!({"type": "object"}))
            .with_app_data_dir("/data")
            .add_skills_path("/skills")
            .add_policy(policy::allow_all())
            .add_mcp_server(McpServer::stdio("uvx", ["mcp-server-git"]).with_name("git"))
            .with_capabilities(Capabilities::read_only().enable(BuiltinTool::RunCommand));
        let dispatcher = ToolDispatcher::new(builder.tools.clone(), &builder.tool_services);
        let config = builder.build_harness_config(&dispatcher);

        assert_eq!(config.cascade_id.as_deref(), Some("resume-me"));
        assert_eq!(config.models.len(), 1);
        let model = &config.models[0];
        assert_eq!(model.name.as_deref(), Some("gemini-3-flash-preview"));
        assert_eq!(model.types, vec![protocol::ModelType::Text]);
        assert_eq!(
            model
                .gemini_api_endpoint
                .as_ref()
                .unwrap()
                .api_key
                .as_deref(),
            Some("test-key")
        );
        assert_eq!(config.workspaces.len(), 2);
        assert_eq!(
            config.finish_tool_schema_json.as_deref(),
            Some(r#"{"type":"object"}"#)
        );
        assert_eq!(config.app_data_dir.as_deref(), Some("/data"));
        assert_eq!(config.skills_paths, vec!["/skills"]);
        assert_eq!(config.mcp_servers[0].name.as_deref(), Some("git"));
        // Policies enable the pre-tool lifecycle hook.
        assert_eq!(config.enabled_hooks, vec![protocol::LifecycleHook::PreTool]);
        // Custom instructions round through the `custom.part` shape.
        let instructions = config.system_instructions.as_ref().unwrap();
        assert_eq!(
            instructions.custom.as_ref().unwrap().part[0]
                .text
                .as_deref(),
            Some("Be brief.")
        );
        // run_command was enabled on top of the read-only set.
        let side_tools = config.harness_side_tools.as_ref().unwrap();
        assert!(side_tools.run_command.unwrap().enabled);
        assert!(!side_tools.file_edit.unwrap().enabled);
    }

    #[tokio::test]
    async fn test_spawn_rejects_zero_interval_trigger() {
        let err = AntigravityAgent::builder()
            .add_trigger(TriggerConfig::new("tick", Duration::ZERO))
            .spawn()
            .await
            .unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("expected Config error, got {err:?}");
        };
        assert!(message.contains("non-zero"));
        assert!(message.contains("tick"));
    }

    #[test]
    fn test_builder_add_trigger_accumulates() {
        let builder = AntigravityAgent::builder()
            .add_trigger(TriggerConfig::new("a", Duration::from_secs(1)))
            .add_trigger(TriggerConfig::new("b", Duration::from_secs(2)));
        assert_eq!(builder.triggers.len(), 2);
        assert_eq!(builder.triggers[0].message, "a");
        assert_eq!(builder.triggers[1].interval, Duration::from_secs(2));
    }

    #[tokio::test]
    async fn test_spawn_rejects_subagent_with_unregistered_tool() {
        let err = AntigravityAgent::builder()
            .add_subagent(Subagent::new("auditor").add_tool("not_registered"))
            .spawn()
            .await
            .unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("expected Config error, got {err:?}");
        };
        assert!(message.contains("auditor"));
        assert!(message.contains("not_registered"));
        assert!(message.contains("add_tool"));
    }

    #[tokio::test]
    async fn test_spawn_rejects_duplicate_subagent_names() {
        let err = AntigravityAgent::builder()
            .add_subagent(Subagent::new("twin"))
            .add_subagent(Subagent::new("twin"))
            .spawn()
            .await
            .unwrap_err();
        let AntigravityError::Config(message) = &err else {
            panic!("expected Config error, got {err:?}");
        };
        assert!(message.contains("twin"));
        assert!(message.contains("unique"));
    }

    #[test]
    fn test_builder_subagents_reach_harness_config() {
        let declaration = crate::FunctionDeclaration::builder("severity_classifier")
            .description("Classifies severity.")
            .build();
        let builder = AntigravityAgent::builder()
            .add_tool(declaration)
            .add_subagent(
                Subagent::new("auditor")
                    .with_description("Audits files.")
                    .add_tool("severity_classifier"),
            );
        let dispatcher = ToolDispatcher::new(builder.tools.clone(), &builder.tool_services);
        let config = builder.build_harness_config(&dispatcher);

        assert_eq!(config.custom_subagents.len(), 1);
        let subagent = &config.custom_subagents[0];
        assert_eq!(subagent.name.as_deref(), Some("auditor"));
        assert_eq!(subagent.description.as_deref(), Some("Audits files."));
        // The subagent carries the parent's full tool declaration.
        assert_eq!(subagent.tools.len(), 1);
        assert_eq!(
            subagent.tools[0].name.as_deref(),
            Some("severity_classifier")
        );
        assert!(subagent.tools[0].parameters_json_schema.is_some());
    }

    #[test]
    fn test_builder_no_api_key_sends_no_models() {
        let builder = AntigravityAgent::builder();
        let dispatcher = ToolDispatcher::new(vec![], &[]);
        let config = builder.build_harness_config(&dispatcher);
        assert!(config.models.is_empty());
        assert!(config.enabled_hooks.is_empty());
    }

    #[test]
    fn test_builder_with_workspace_replaces() {
        let builder = AntigravityAgent::builder()
            .add_workspace("/a")
            .with_workspace("/only");
        assert_eq!(builder.workspaces, vec!["/only"]);
    }

    #[test]
    fn test_post_tool_hook_enables_post_tool_lifecycle_hook() {
        let builder = AntigravityAgent::builder().on_post_tool(|_| {});
        let dispatcher = ToolDispatcher::new(vec![], &[]);
        let config = builder.build_harness_config(&dispatcher);
        assert_eq!(
            config.enabled_hooks,
            vec![protocol::LifecycleHook::PostTool]
        );
    }

    // -------------------------------------------------------------------
    // Tool-confirmation decisions (see `confirmation_decision`)
    // -------------------------------------------------------------------

    /// A waiting step carrying a `run_command` action confirmation.
    fn run_command_confirmation_step() -> StepUpdate {
        StepUpdate {
            state: Some(StepState::WaitingForUser),
            request_text: Some("run `ls`?".to_string()),
            tool_confirmation_request: Some(protocol::ToolConfirmationRequest::default()),
            run_command: Some(protocol::ActionRunCommand {
                command_line: Some("ls".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// A waiting confirmation step with no action payload at all (a
    /// pre-request notification for a host-side tool).
    fn pre_request_confirmation_step() -> StepUpdate {
        StepUpdate {
            state: Some(StepState::WaitingForUser),
            tool_confirmation_request: Some(protocol::ToolConfirmationRequest::default()),
            ..Default::default()
        }
    }

    /// A waiting confirmation step whose action landed in `extra` — the
    /// shape a harness builtin newer than this client produces.
    fn unknown_action_confirmation_step() -> StepUpdate {
        let mut step = pre_request_confirmation_step();
        step.request_text = Some("do the new thing?".to_string());
        step.extra.insert(
            "deleteEverything".to_string(),
            serde_json::json!({"target": "/"}),
        );
        step
    }

    fn engine(policies: Vec<Policy>) -> PolicyEngine {
        PolicyEngine::new(policies)
    }

    /// Whether a confirmation is accepted (the decision maps to `Allow`).
    fn confirmation_accepted(
        step: &StepUpdate,
        engine: &PolicyEngine,
        pre_tool: Option<&PreToolHook>,
    ) -> bool {
        matches!(
            confirmation_decision(step, engine, pre_tool),
            PreToolDecision::Allow
        )
    }

    #[test]
    fn test_confirmation_denied_for_deny_policied_known_tool() {
        let step = run_command_confirmation_step();
        // Exact deny and wildcard deny must both reject (accepted=false).
        assert!(!confirmation_accepted(
            &step,
            &engine(vec![policy::deny("run_command")]),
            None
        ));
        assert!(!confirmation_accepted(
            &step,
            &engine(vec![policy::deny_all()]),
            None
        ));
    }

    #[test]
    fn test_confirmation_allowed_for_allowed_known_tool() {
        let step = run_command_confirmation_step();
        assert!(confirmation_accepted(
            &step,
            &engine(vec![policy::allow_all()]),
            None
        ));
        assert!(confirmation_accepted(
            &step,
            &engine(vec![policy::deny_all(), policy::allow("run_command")]),
            None
        ));
    }

    #[test]
    fn test_confirmation_hook_sees_known_tool_args_and_request_text() {
        let step = run_command_confirmation_step();
        let hook: PreToolHook = Arc::new(|invocation| {
            assert_eq!(invocation.name, "run_command");
            assert_eq!(invocation.args["commandLine"], "ls");
            assert_eq!(invocation.args["request_text"], "run `ls`?");
            PreToolDecision::deny("hook says no")
        });
        assert!(!confirmation_accepted(&step, &engine(vec![]), Some(&hook)));
    }

    #[test]
    fn test_confirmation_pre_request_auto_approved_even_under_deny_all() {
        // No action payload and no unknown fields: a pre-request
        // notification. The concrete call gets its own policy check, so
        // this is approved regardless of policy (mirrors the reference SDK).
        let step = pre_request_confirmation_step();
        assert!(confirmation_accepted(
            &step,
            &engine(vec![policy::deny_all()]),
            None
        ));
        assert!(confirmation_accepted(&step, &engine(vec![]), None));
    }

    #[test]
    fn test_confirmation_unknown_action_allowed_only_under_allow_all() {
        let step = unknown_action_confirmation_step();
        // allow_all (wildcard) approves.
        assert!(confirmation_accepted(
            &step,
            &engine(vec![policy::allow_all()]),
            None
        ));
        // Restrictive policy sets fail closed.
        assert!(!confirmation_accepted(
            &step,
            &engine(vec![policy::deny_all()]),
            None
        ));
        assert!(!confirmation_accepted(
            &step,
            &engine(vec![policy::deny_all(), policy::allow("run_command")]),
            None
        ));
        // No policies and no hook at all: still fail closed — an unknown
        // builtin's confirmation is its only gate.
        assert!(!confirmation_accepted(&step, &engine(vec![]), None));
    }

    #[test]
    fn test_confirmation_unknown_action_matches_exact_rule_by_wire_field() {
        let step = unknown_action_confirmation_step();
        assert!(confirmation_accepted(
            &step,
            &engine(vec![policy::deny_all(), policy::allow("deleteEverything")]),
            None
        ));
        assert!(!confirmation_accepted(
            &step,
            &engine(vec![policy::allow_all(), policy::deny("deleteEverything")]),
            None
        ));
    }

    #[test]
    fn test_confirmation_unknown_action_defers_to_hook() {
        let step = unknown_action_confirmation_step();
        let hook: PreToolHook = Arc::new(|invocation| {
            // The hook sees the unknown wire field as the tool name and the
            // preserved payload as args.
            assert_eq!(invocation.name, "deleteEverything");
            assert_eq!(invocation.args["deleteEverything"]["target"], "/");
            assert_eq!(invocation.args["request_text"], "do the new thing?");
            PreToolDecision::Allow
        });
        assert!(confirmation_accepted(&step, &engine(vec![]), Some(&hook)));

        let deny_hook: PreToolHook = Arc::new(|_| PreToolDecision::deny("nope"));
        assert!(!confirmation_accepted(
            &step,
            &engine(vec![]),
            Some(&deny_hook)
        ));
    }

    // -------------------------------------------------------------------
    // Accepted/denied decision surfacing (Item: ToolAction decision marker)
    // -------------------------------------------------------------------

    #[test]
    fn test_confirmation_decision_carries_denial_reason() {
        let step = run_command_confirmation_step();
        let decision = confirmation_decision(&step, &engine(vec![policy::deny_all()]), None);
        let PreToolDecision::Deny { reason } = decision else {
            panic!("expected a denial");
        };
        assert!(reason.contains("run_command"));
        // The allowed path yields `Allow` (no reason).
        assert_eq!(
            confirmation_decision(&step, &engine(vec![policy::allow_all()]), None),
            PreToolDecision::Allow
        );
    }

    #[test]
    fn test_denied_confirmation_emits_denied_tool_action() {
        // A denied confirmation must surface as a ToolAction event carrying
        // the Denied decision and the trajectory id; an allowed one must not
        // emit at the confirmation step (it surfaces at its terminal step).
        let mut step = run_command_confirmation_step();
        step.trajectory_id = Some("traj-1".to_string());

        let denied = matches!(
            confirmation_decision(&step, &engine(vec![policy::deny_all()]), None),
            PreToolDecision::Deny { .. }
        );
        assert!(denied);
        // Mirror the emission the turn loop performs on a denial.
        let action = streaming::ToolAction::from_step(&step).unwrap();
        let event = AgentEvent::ToolAction {
            action: Box::new(action),
            decision: ToolDecision::Denied {
                reason: "Denied by policy for tool 'run_command'.".to_string(),
            },
            trajectory_id: step.trajectory_id.clone(),
        };
        let AgentEvent::ToolAction {
            decision,
            trajectory_id,
            ..
        } = &event
        else {
            panic!("expected a ToolAction");
        };
        assert!(decision.is_denied());
        assert_eq!(
            decision.denial_reason(),
            Some("Denied by policy for tool 'run_command'.")
        );
        assert_eq!(trajectory_id.as_deref(), Some("traj-1"));

        assert_eq!(
            confirmation_decision(&step, &engine(vec![policy::allow_all()]), None),
            PreToolDecision::Allow
        );
    }

    // -------------------------------------------------------------------
    // Error severity classification (Item: classify harness error severity)
    // -------------------------------------------------------------------

    #[test]
    fn test_classify_error_severity() {
        // No code / retryable / non-fatal codes → transient noise.
        assert_eq!(classify_error_severity(None), ErrorSeverity::Transient);
        assert_eq!(classify_error_severity(Some(500)), ErrorSeverity::Transient);
        assert_eq!(classify_error_severity(Some(429)), ErrorSeverity::Transient);
        // Fatal model-backend codes that reached the event path are serious.
        for code in FATAL_HTTP_CODES {
            assert_eq!(classify_error_severity(Some(code)), ErrorSeverity::Terminal);
        }
    }

    // -------------------------------------------------------------------
    // ToolOutcome result unwrapping (Item: unwrap the wire envelope)
    // -------------------------------------------------------------------

    #[test]
    fn test_unwrap_result_value_unwraps_scalar_envelope() {
        // {"result": "<string>"} → the inner string, verbatim.
        assert_eq!(
            unwrap_result_value(&serde_json::json!({"result": "hello"})),
            "hello"
        );
        // {"result": <non-string>} → the inner value serialized.
        assert_eq!(
            unwrap_result_value(&serde_json::json!({"result": 42})),
            "42"
        );
        // A verbatim object (not a lone `result` key) passes through.
        assert_eq!(
            unwrap_result_value(&serde_json::json!({"echo": "hi"})),
            r#"{"echo":"hi"}"#
        );
        // A multi-key object that happens to contain `result` is not
        // unwrapped (data preservation).
        let multi = serde_json::json!({"result": 1, "other": 2});
        assert_eq!(unwrap_result_value(&multi), multi.to_string());
    }

    #[test]
    fn test_unwrap_result_string_unwraps_only_the_envelope() {
        assert_eq!(unwrap_result_string(r#"{"result":"inner"}"#), "inner");
        assert_eq!(unwrap_result_string(r#"{"result":7}"#), "7");
        // Plain (non-JSON) harness output is handed back verbatim.
        assert_eq!(unwrap_result_string("plain text"), "plain text");
        // A non-envelope object stays verbatim.
        assert_eq!(unwrap_result_string(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    // -------------------------------------------------------------------
    // Workspace announcement (Item: announce workspace roots)
    // -------------------------------------------------------------------

    #[test]
    fn test_workspace_announcement_lists_roots() {
        let note = workspace_announcement(&["/a".to_string(), "/b".to_string()]);
        assert!(note.contains("/a"));
        assert!(note.contains("/b"));
        assert!(note.contains("Workspace"));
    }

    #[test]
    fn test_build_system_instructions_composition() {
        // User + note: two custom parts, user text unmutated and first.
        let si = build_system_instructions(Some("Be brief."), Some("ROOTS")).unwrap();
        let parts = si.custom.unwrap().part;
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].text.as_deref(), Some("Be brief."));
        assert_eq!(parts[1].text.as_deref(), Some("ROOTS"));

        // Note only (no user instructions): an appended section preserves
        // the harness defaults.
        let si = build_system_instructions(None, Some("ROOTS")).unwrap();
        assert!(si.custom.is_none());
        let sections = si.appended.unwrap().appended_sections;
        assert_eq!(sections[0].title.as_deref(), Some("Workspace"));
        assert_eq!(sections[0].content.as_deref(), Some("ROOTS"));

        // User only: fully-custom single part.
        let si = build_system_instructions(Some("Hi"), None).unwrap();
        assert_eq!(si.custom.unwrap().part[0].text.as_deref(), Some("Hi"));

        // Neither: no instructions.
        assert!(build_system_instructions(None, None).is_none());
    }

    #[test]
    fn test_builder_announces_workspace_by_default() {
        let builder = AntigravityAgent::builder()
            .with_system_instructions("Audit it.")
            .add_workspace("/repo");
        let dispatcher = ToolDispatcher::new(vec![], &[]);
        let config = builder.build_harness_config(&dispatcher);
        let parts = config.system_instructions.unwrap().custom.unwrap().part;
        assert_eq!(parts[0].text.as_deref(), Some("Audit it."));
        assert!(parts[1].text.as_ref().unwrap().contains("/repo"));
    }

    #[test]
    fn test_builder_workspace_announcement_opt_out() {
        let builder = AntigravityAgent::builder()
            .with_system_instructions("Audit it.")
            .add_workspace("/repo")
            .with_workspace_announcement(false);
        let dispatcher = ToolDispatcher::new(vec![], &[]);
        let config = builder.build_harness_config(&dispatcher);
        // Just the user's instruction, no announcement part.
        let parts = config.system_instructions.unwrap().custom.unwrap().part;
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].text.as_deref(), Some("Audit it."));
    }

    #[test]
    fn test_builder_no_workspace_no_announcement() {
        // No workspace → nothing to announce, instructions untouched.
        let builder = AntigravityAgent::builder().with_system_instructions("Hi");
        let dispatcher = ToolDispatcher::new(vec![], &[]);
        let config = builder.build_harness_config(&dispatcher);
        let parts = config.system_instructions.unwrap().custom.unwrap().part;
        assert_eq!(parts.len(), 1);
    }
}
