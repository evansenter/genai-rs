//! [`AgentBuilder`]: configuration setters and `spawn()`. Hook setters live
//! in `hooks.rs`, spawn-time checks in `validate.rs`, and the translation to
//! the harness's `HarnessConfig` in `wire.rs`.

use super::policies::PolicyEngine;
use super::process::{self, HarnessProcess};
use super::protocol::{self, InputEvent};
use super::session::{Session, WireContext};
use super::tools::ToolDispatcher;
use super::{
    AgentBehavior, AntigravityAgent, AntigravityError, Capabilities, McpServer, Policy,
    PostToolHook, PreToolHook, QuestionHook, Subagent, TriggerConfig, handshake, triggers,
};
use crate::wire::WireInspector;
use crate::{FunctionDeclaration, ToolService};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

mod hooks;
mod validate;
mod wire;

// =============================================================================
// Builder
// =============================================================================

/// Default wall-clock budget for a single agent turn.
///
/// Generous enough for real agent work (tool calls, subagents) while still
/// bounding the failure mode that motivated having a default at all: a
/// harness that stops signalling turn completion hangs rather than errors.
/// Override with [`AgentBuilder::with_turn_timeout`], or remove it
/// deliberately with [`AgentBuilder::without_turn_timeout`].
pub const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(300);

/// Whether the caller has chosen a per-turn budget, and what.
///
/// Three states rather than `Option<Duration>` because "unset" and
/// "explicitly unlimited" must resolve differently: the first gets
/// [`DEFAULT_TURN_TIMEOUT`], the second gets nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TurnBudget {
    /// No choice made — resolves to [`DEFAULT_TURN_TIMEOUT`].
    #[default]
    Unset,
    /// `without_turn_timeout()`: run until the harness ends the turn.
    Unlimited,
    /// `with_turn_timeout(d)`.
    Explicit(Duration),
}

impl TurnBudget {
    fn resolve(self) -> Option<Duration> {
        match self {
            Self::Unset => Some(DEFAULT_TURN_TIMEOUT),
            Self::Unlimited => None,
            Self::Explicit(d) => Some(d),
        }
    }
}

/// Builder for [`AntigravityAgent`]. Create via
/// [`AntigravityAgent::builder`].
#[derive(Default)]
pub struct AgentBuilder {
    harness_path: Option<PathBuf>,
    api_key: Option<String>,
    gemini_base_url: Option<String>,
    model: Option<String>,
    system_instructions: Option<String>,
    workspaces: Vec<String>,
    tools: Vec<FunctionDeclaration>,
    tool_services: Vec<Arc<dyn ToolService>>,
    mcp_servers: Vec<McpServer>,
    policies: Vec<Policy>,
    pre_tool: Option<PreToolHook>,
    pub(super) questions: Option<QuestionHook>,
    post_tool: Option<PostToolHook>,
    save_dir: Option<String>,
    conversation_id: Option<String>,
    capabilities: Capabilities,
    response_schema: Option<Value>,
    app_data_dir: Option<String>,
    skills_paths: Vec<String>,
    turn_timeout: TurnBudget,
    inspectors: Vec<Arc<dyn WireInspector>>,
    triggers: Vec<TriggerConfig>,
    subagents: Vec<Subagent>,
    /// Whether to announce workspace roots in the effective system
    /// instructions. `None` means the default (on); see
    /// [`Self::with_workspace_announcement`].
    workspace_announcement: Option<bool>,
    /// `None` leaves the harness default (autonomous).
    agent_behavior: Option<AgentBehavior>,
    /// Whether to confine file tools to the workspaces. `None` means the
    /// default (on); see [`Self::with_workspace_containment`].
    workspace_containment: Option<bool>,
}

impl std::fmt::Debug for AgentBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentBuilder")
            .field("harness_path", &self.harness_path)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("gemini_base_url", &self.gemini_base_url)
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
    ///
    /// A blank key (empty or whitespace) counts as no key: the harness
    /// would treat it as unset and fall back to its environment's
    /// `GEMINI_API_KEY`, so `spawn()` treats it the same way.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Points the harness's model calls at another Gemini API host: a
    /// gateway or recording proxy, or a scripted local server in tests (see
    /// `docs/TESTING.md`). The harness appends the usual
    /// `/v1beta/models/<model>:streamGenerateContent?alt=sse` path and sends
    /// the key in the `x-goog-api-key` header.
    ///
    /// Needs a non-blank [`Self::with_api_key`]; `spawn()` fails with
    /// [`AntigravityError::Config`] without one. Without a configured key
    /// (an empty one included) the harness falls back to its own
    /// environment's `GEMINI_API_KEY` and sends *that* to the custom host
    /// (verified on harness 0.1.23, 2026-11-04), which would hand a real key
    /// to whatever the URL names. Pass the key that host expects, or any
    /// placeholder for a server that checks none. As a second guard, the
    /// harness is started without `GEMINI_API_KEY` and `GOOGLE_API_KEY` in
    /// its environment while a base URL is set, so commands it runs
    /// (`run_command`) don't see them either.
    #[must_use]
    pub fn with_gemini_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.gemini_base_url = Some(base_url.into());
        self
    }

    /// Sets the text model (default: [`DEFAULT_MODEL`](crate::DEFAULT_MODEL)).
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
    /// The harness's own prompt lists the workspaces, but
    /// [`Self::with_system_instructions`] replaces that prompt, and agents
    /// that don't know the paths guess (`/workdir`, `/workspace`, …) and
    /// wander. When on and at least one
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

    /// Controls whether the harness confines its file tools to the
    /// configured workspaces. **On by default** whenever a workspace is
    /// configured. Without a workspace there is nothing to confine to: the
    /// agent's file tools can read anything the process can, and an
    /// explicit `true` makes `spawn()` fail with
    /// [`AntigravityError::Config`].
    ///
    /// It covers the file tools (`view_file`, `list_directory`, …), not
    /// `run_command`: a shell command can read any path the process can, so
    /// keep `run_command` off, or gate it with a hook or policy, if the
    /// workspace boundary matters.
    ///
    /// The harness enforces containment only when the client sends a
    /// policy config, and before this setting existed this crate never
    /// did: an agent could read files anywhere the process could, and the
    /// content reached the model (verified on harness 0.1.18 and 0.1.23).
    /// With containment on, a read outside the workspaces fails with
    /// `Access to path "…" is denied`. The harness still allows two roots
    /// besides the workspaces: the system temp directory and its own
    /// `~/.gemini/antigravity/brain` (verified on 0.1.23). The Python SDK
    /// contains by default too.
    ///
    /// Turn it off only if the agent must reach paths outside its
    /// workspaces, and gate those tools with a pre-tool hook or policy:
    ///
    /// ```rust,ignore
    /// let agent = AntigravityAgent::builder()
    ///     .add_workspace("/repo")
    ///     .with_workspace_containment(false)
    ///     // ...
    ///     ;
    /// ```
    #[must_use]
    pub fn with_workspace_containment(mut self, contain: bool) -> Self {
        self.workspace_containment = Some(contain);
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
    ///
    /// [`policy`]: super::policy
    #[must_use]
    pub fn add_policy(mut self, policy: Policy) -> Self {
        self.policies.push(policy);
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

    /// Sets how the harness frames the agent's task (default: the
    /// harness's, which is [`AgentBehavior::Autonomous`]).
    ///
    /// [`BuiltinTool::AskQuestion`] needs [`AgentBehavior::Interactive`]:
    /// the default autonomous prompt tells the model the user will not
    /// answer, so it asks in prose rather than calling the tool, and an
    /// [`on_questions`](Self::on_questions) hook never fires. `spawn()`
    /// warns about that combination.
    #[must_use]
    pub fn with_agent_behavior(mut self, behavior: AgentBehavior) -> Self {
        self.agent_behavior = Some(behavior);
        self
    }

    /// Sets a JSON schema for structured final output (delivered through
    /// [`ChatResponse::structured_output`]).
    ///
    /// [`ChatResponse::structured_output`]: super::ChatResponse::structured_output
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
    ///
    /// Defaults to [`DEFAULT_TURN_TIMEOUT`]. Raise it for agents that
    /// legitimately run long (deep subagent trees, many tool calls); lower
    /// it for interactive use where a stall should surface fast.
    ///
    /// The budget includes custom tools and hooks. Each tool call runs on a
    /// task of its own that the deadline aborts; a synchronous tool body
    /// that blocks its thread is abandoned on a multi-thread runtime with
    /// two or more workers. On a current-thread runtime, or with one worker
    /// (`#[tokio::main]` on a one-CPU host), it holds the timer's thread
    /// until it returns. Make such a tool `async`, or call `spawn_blocking`
    /// inside it.
    #[must_use]
    pub fn with_turn_timeout(mut self, timeout: Duration) -> Self {
        self.turn_timeout = TurnBudget::Explicit(timeout);
        self
    }

    /// Removes the per-turn budget entirely, so a turn runs until the
    /// harness ends it.
    ///
    /// Deliberately explicit rather than the default. An unbounded turn
    /// does not fail when the harness stops signalling completion — it
    /// *hangs*, which is strictly less diagnosable than an error and looks
    /// identical to latency. That is not hypothetical: harness 0.1.10
    /// renamed the terminal trajectory state, and every turn ran to its
    /// budget with no error, no failed parse, and nothing in the logs.
    /// Callers who want that behavior should say so.
    #[must_use]
    pub fn without_turn_timeout(mut self) -> Self {
        self.turn_timeout = TurnBudget::Unlimited;
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
    ///   parity with the reference SDK), or when a model or a Gemini base
    ///   URL is configured without a non-blank API key.
    /// - [`AntigravityError::HarnessNotFound`] when discovery fails.
    /// - [`AntigravityError::HandshakeFailed`] / [`AntigravityError::InitFailed`]
    ///   with the harness's stderr tail when startup fails.
    pub async fn spawn(self) -> Result<AntigravityAgent, AntigravityError> {
        let dispatcher = ToolDispatcher::new(self.tools.clone(), &self.tool_services);
        self.validate(&dispatcher)?;

        let binary = process::discover_harness(self.harness_path.as_deref())?;

        let mut inspectors = self.inspectors.clone();
        if let Some(printer) = crate::wire::env_inspector() {
            inspectors.push(Arc::new(printer));
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
        // With a custom host, the harness gets no key to fall back on: the
        // configured one is the only key it can send there.
        let scrub_key_env = self.gemini_base_url.is_some();
        let (mut harness, output_config) =
            HarnessProcess::spawn(&binary, &input_config, scrub_key_env, &wire).await?;

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
            policy_engine: Arc::new(PolicyEngine::new(self.policies)),
            pre_tool: self.pre_tool,
            questions: self.questions,
            post_tool: self.post_tool,
            conversation_id: None,
            initial_history: Vec::new(),
            turn_timeout: self.turn_timeout.resolve(),
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
                Err(e.with_version_note(agent.harness.version()))
            }
        }
    }

    /// The configured API key, unless it is blank. The harness treats an
    /// empty key as unset and falls back to its environment's
    /// `GEMINI_API_KEY`, so a blank key must count as none here too.
    fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref().filter(|key| !key.trim().is_empty())
    }

    /// Whether workspace announcement is enabled (default on).
    fn announce_workspaces(&self) -> bool {
        self.workspace_announcement.unwrap_or(true)
    }
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
