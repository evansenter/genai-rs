use super::hooks::PolicyEngine;
use super::process::{self, HarnessProcess};
use super::protocol::{self, InputEvent};
use super::session::{Session, WireContext};
use super::tools::ToolDispatcher;
use super::{
    AgentBehavior, AgentQuestion, AntigravityAgent, AntigravityError, BuiltinTool, Capabilities,
    McpServer, Policy, PostToolHook, PreToolDecision, PreToolHook, QuestionHook, QuestionReply,
    Subagent, ToolInvocation, ToolOutcome, TriggerConfig, handshake, triggers,
};
use crate::wire::WireInspector;
use crate::{FunctionDeclaration, ToolService};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
    ///
    /// [`policy`]: super::policy
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

    /// Sets a questions hook, invoked when the agent asks the user
    /// questions via the `ask_question` builtin.
    ///
    /// The hook receives the whole question batch and returns answers (or
    /// cancels). Without a hook, every question is answered "unanswered"
    /// so the harness never deadlocks — that fallback covers only the
    /// *hookless* case.
    ///
    /// The hook is synchronous and runs inline in the harness event pump:
    /// **do not block in it** waiting for a human. Answer from policy or
    /// pre-collected state (e.g. a channel drained with `try_recv`);
    /// blocking stalls all event processing for as long as the answer
    /// takes.
    ///
    /// The hook only fires if [`BuiltinTool::AskQuestion`] is enabled —
    /// it is *not* in [`Capabilities::read_only()`] (the default), so on a
    /// default builder this hook is a no-op: the agent never asks, and
    /// `spawn()` emits a `warn!` for the hook-set-but-builtin-disabled
    /// combination. Enable it with
    /// `Capabilities::read_only().enable(BuiltinTool::AskQuestion)`, which
    /// as a write-capable capability also needs a policy or
    /// [`on_pre_tool`](Self::on_pre_tool) hook to pass the spawn-time
    /// gate — and set
    /// [`with_agent_behavior(AgentBehavior::Interactive)`](Self::with_agent_behavior),
    /// without which the model is told nobody will answer.
    ///
    /// On harness 0.1.18 the `ask_question` call passes through the
    /// pre-tool hook *before* its `questions_request` arrives, so policies
    /// govern it like any other tool: `deny_all()` needs an
    /// `allow("ask_question")` beside it, and `deny("ask_question")`
    /// prevents questions. (On 0.1.10 question requests bypassed the policy
    /// engine.)
    #[must_use]
    pub fn on_questions(
        mut self,
        hook: impl Fn(&[AgentQuestion]) -> QuestionReply + Send + Sync + 'static,
    ) -> Self {
        self.questions = Some(Arc::new(hook));
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
        // Not an error (the combination is harmless), but the likeliest
        // first-time misconfiguration: a questions hook with the builtin
        // disabled never fires, and without this there is no signal
        // distinguishing "the agent chose not to ask" from "asking was
        // never enabled".
        if self.questions.is_some() && !self.capabilities.is_enabled(BuiltinTool::AskQuestion) {
            tracing::warn!(
                "on_questions hook set but BuiltinTool::AskQuestion is not enabled - the agent \
                 will never ask questions and the hook will never run. Enable it with \
                 Capabilities::read_only().enable(BuiltinTool::AskQuestion)."
            );
        }
        // Same failure shape one layer down (reference-SDK parity): the
        // tool is declared, but the default autonomous prompt tells the
        // model nobody will answer, so it asks in prose instead.
        if self.capabilities.is_enabled(BuiltinTool::AskQuestion)
            && self.agent_behavior != Some(AgentBehavior::Interactive)
        {
            tracing::warn!(
                "BuiltinTool::AskQuestion is enabled but agent behavior is not Interactive - \
                 the harness's autonomous prompt tells the model the user will not respond, \
                 so it rarely calls ask_question. Add \
                 .with_agent_behavior(AgentBehavior::Interactive)."
            );
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
                    // A real model id, not a placeholder: this is the id the
                    // harness is actually asked to run when the caller never
                    // called `with_model`.
                    name: Some(
                        self.model
                            .clone()
                            .unwrap_or_else(|| crate::DEFAULT_MODEL.to_string()),
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
            agent_behavior: self.agent_behavior.map(AgentBehavior::to_wire),
        }
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

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
