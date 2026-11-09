//! [`AgentBuilder`]: configuration setters and `spawn()`. Hook setters live
//! in `hooks.rs`, harness option setters in `options.rs`, spawn-time checks
//! in `validate.rs`, and the translation to the harness's `HarnessConfig` in
//! `wire.rs`.

use super::hooks::LifecycleHooks;
use super::policies::PolicyEngine;
use super::process::{self, HarnessProcess};
use super::protocol;
use super::session::{Session, WireContext};
use super::tools::ToolDispatcher;
use super::triggers::CustomTrigger;
use super::{
    AgentBehavior, AntigravityAgent, AntigravityError, Capabilities, McpServer, Policy,
    PostToolHook, PreToolHook, QuestionHook, Subagent, SystemInstructions, TriggerConfig,
    handshake, triggers,
};
use crate::wire::WireInspector;
use crate::{FunctionDeclaration, ToolService};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

mod hooks;
mod options;
mod validate;
mod wire;

use options::HarnessOptions;

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
    system_instructions: Option<SystemInstructions>,
    workspaces: Vec<String>,
    tools: Vec<FunctionDeclaration>,
    tool_services: Vec<Arc<dyn ToolService>>,
    mcp_servers: Vec<McpServer>,
    policies: Vec<Policy>,
    pre_tool: Option<PreToolHook>,
    pub(super) questions: Option<QuestionHook>,
    post_tool: Option<PostToolHook>,
    lifecycle: LifecycleHooks,
    save_dir: Option<String>,
    conversation_id: Option<String>,
    capabilities: Capabilities,
    response_schema: Option<Value>,
    app_data_dir: Option<String>,
    skills_paths: Vec<String>,
    turn_timeout: TurnBudget,
    inspectors: Vec<Arc<dyn WireInspector>>,
    triggers: Vec<TriggerConfig>,
    custom_triggers: Vec<CustomTrigger>,
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
    /// Whether `spawn()` may start subagents alongside gates they bypass;
    /// see [`Self::with_ungated_subagents`].
    ungated_subagents: bool,
    /// Model, subagent and harness options (`options.rs`).
    options: HarnessOptions,
}

impl std::fmt::Debug for AgentBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentBuilder")
            .field("harness_path", &self.harness_path)
            .field("api_key", &super::redact::redacted(self.api_key.as_ref()))
            .field("gemini_base_url", &self.gemini_base_url)
            .field("model", &self.model)
            .field("workspaces", &self.workspaces)
            .field("tools", &self.tools.len())
            .field("mcp_servers", &self.mcp_servers.len())
            .field("policies", &self.policies.len())
            .field("capabilities", &self.capabilities)
            .field(
                "triggers",
                &(self.triggers.len() + self.custom_triggers.len()),
            )
            .field("subagents", &self.subagents.len())
            .field("ungated_subagents", &self.ungated_subagents)
            .field("lifecycle", &self.lifecycle)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl AgentBuilder {
    /// Sets an explicit path to the `localharness` binary, in place of
    /// discovery: `ANTIGRAVITY_HARNESS_PATH`, site-packages and `PATH` are
    /// not searched. When the path is not a file, `spawn()` fails with
    /// [`AntigravityError::HarnessNotFound`] naming it.
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
    /// (harness 0.1.23), which would hand a real key
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

    /// Sets the agent's system instructions.
    ///
    /// A string is **added to** the harness's own prompt, under a
    /// `<user_system_instructions>` section, as in the Python SDK: the
    /// harness keeps its tool guidance, safety rules, workspace list,
    /// skills list and built-in subagents.
    /// [`SystemInstructions::custom`] replaces that prompt instead, and
    /// [`SystemInstructions::harness_default`] lets you replace its
    /// identity line and add sections of your own.
    ///
    /// ```rust,no_run
    /// use genai_rs::antigravity::{AntigravityAgent, SystemInstructions};
    ///
    /// let added = AntigravityAgent::builder()
    ///     .with_system_instructions("Review for security issues only.");
    /// let replaced = AntigravityAgent::builder()
    ///     .with_system_instructions(SystemInstructions::custom("You are a linter."));
    /// # let _ = (added, replaced);
    /// ```
    #[must_use]
    pub fn with_system_instructions(mut self, instructions: impl Into<SystemInstructions>) -> Self {
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
    /// announced to the model when its prompt is
    /// [custom](SystemInstructions::custom). **On by default.**
    ///
    /// The harness's own prompt lists the workspaces ("Active Workspaces",
    /// verified on 0.1.23 for agents and subagents), so instructions added
    /// to it need no note. Custom instructions replace that prompt, and
    /// agents that don't know the paths guess (`/workdir`, `/workspace`, …)
    /// and wander. With custom instructions and at least one workspace,
    /// `spawn()` adds a short, clearly delimited note listing the
    /// configured root(s) as a final part (composed at send time — your
    /// instructions are never mutated), and the same note to each subagent
    /// with custom instructions.
    ///
    /// Turn it off to manage workspace grounding yourself:
    ///
    /// ```rust,no_run
    /// # use genai_rs::antigravity::AntigravityAgent;
    /// let builder = AntigravityAgent::builder()
    ///     .add_workspace("/repo")
    ///     .with_workspace_announcement(false)
    ///     // ...
    ///     ;
    /// # let _ = builder;
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
    /// It confines the agent's own calls, not a subagent's (harness
    /// 0.1.23), so with subagents enabled `spawn()` also needs
    /// [`Self::with_ungated_subagents`].
    ///
    /// Turn it off only if the agent must reach paths outside its
    /// workspaces, and gate those tools with a pre-tool hook or policy:
    ///
    /// ```rust,no_run
    /// # use genai_rs::antigravity::AntigravityAgent;
    /// let builder = AntigravityAgent::builder()
    ///     .add_workspace("/repo")
    ///     .with_workspace_containment(false)
    ///     // ...
    ///     ;
    /// # let _ = builder;
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

    /// Registers a [`ToolService`] providing stateful custom tools. Each
    /// call adds a service; the tools of every one are declared.
    #[must_use]
    pub fn add_tool_service(mut self, service: Arc<dyn ToolService>) -> Self {
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
    /// A rule naming the tool beats a server-wide MCP rule, which beats the
    /// wildcard; within a tier deny beats ask beats allow, whatever the
    /// order (the harness evaluator's precedence; see the [`policy`] module
    /// docs). So `[deny_all(), allow("get_weather")]` allows only
    /// `get_weather`. When no rule matches, the call is allowed (default
    /// open), subject to the pre-tool hook.
    ///
    /// Policies judge the agent's own calls, not a subagent's (harness
    /// 0.1.23), so with subagents enabled `spawn()` refuses any policy but
    /// an allow without [`Self::with_ungated_subagents`].
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
    /// [`BuiltinTool::AskQuestion`](super::BuiltinTool::AskQuestion) needs
    /// [`AgentBehavior::Interactive`]: the default autonomous prompt tells
    /// the model the user will not answer, so it asks in prose rather than
    /// calling the tool, and an [`on_questions`](Self::on_questions) hook
    /// never fires. `spawn()` warns about that combination.
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

    /// Adds a directory to search for agent skills (`SKILL.md` files). The
    /// harness lists them in its own prompt, so custom
    /// [`SystemInstructions`] hide them from the model; they cannot be
    /// combined with [`Self::add_inline_skill`] (`spawn()` refuses, as the
    /// harness does not support both).
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
    ///
    /// An abandoned blocking body keeps its worker thread until it returns,
    /// so the runtime has one worker fewer meanwhile, and dropping the
    /// runtime (the end of `#[tokio::main]`) waits for it. A tool that may
    /// block for long should bound itself.
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

    /// Adds a custom trigger (the Python SDK's `@trigger`): an async
    /// function, started after `spawn()`, that sends the agent messages
    /// through its [`TriggerContext`](triggers::TriggerContext) whenever it
    /// decides to. [`triggers::on_file_change`] builds one that watches a
    /// path. Deliveries wait while a turn runs; see [`triggers`].
    ///
    /// ```rust,no_run
    /// # use genai_rs::antigravity::AntigravityAgent;
    /// # let (_jobs, mut queue) = tokio::sync::mpsc::channel::<u32>(8);
    /// let builder = AntigravityAgent::builder()
    ///     .add_custom_trigger(|ctx| async move {
    ///         while let Some(job) = queue.recv().await {
    ///             ctx.send(format!("Process job {job}.")).await?;
    ///         }
    ///         Ok(())
    ///     })
    ///     // ...
    ///     ;
    /// # let _ = builder;
    /// ```
    ///
    /// An error the trigger returns is logged and ends that trigger only.
    #[must_use]
    pub fn add_custom_trigger<F, Fut>(mut self, trigger: F) -> Self
    where
        F: FnOnce(triggers::TriggerContext) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), AntigravityError>> + Send + 'static,
    {
        self.custom_triggers
            .push(Box::new(move |context| Box::pin(trigger(context))));
        self
    }

    /// Adds a static subagent the parent agent can delegate to via the
    /// `invoke_subagent` builtin (enable it with [`Self::with_capabilities`];
    /// it is write-capable, so the safety gate requires a policy or
    /// pre-tool hook). Nesting, the allow-list and `define_subagent` are set
    /// with [`Self::with_max_subagent_depth`],
    /// [`Self::with_allowed_subagents`] and
    /// [`Self::with_define_subagent_tool`]. A subagent's own tool calls
    /// bypass this agent's gates, so with any gate set `spawn()` needs
    /// [`Self::with_ungated_subagents`].
    ///
    /// Custom tools listed on the subagent (by name) must also be
    /// registered on this builder (`add_tool` / `add_tool_service`);
    /// `spawn()` fails with [`AntigravityError::Config`] on a dangling
    /// reference or a duplicate subagent name.
    #[must_use]
    pub fn add_subagent(mut self, subagent: Subagent) -> Self {
        self.subagents.push(subagent);
        self
    }

    /// Lets `spawn()` start an agent whose subagents bypass its gates.
    /// **Off by default.**
    ///
    /// On harness 0.1.23 a subagent's own tool calls reach neither the
    /// pre-tool hook and this client's policies nor the harness's policy
    /// evaluator (probed with a deny rule), and workspace containment does
    /// not confine them. [`policy::auto`](super::policy::auto) runs in that
    /// evaluator, so it is treated as bypassed too, though it was not
    /// probed on its own. The agent's `run_command` sandbox is its own: a
    /// subagent's commands are sent with the subagent's capabilities. Only
    /// the subagent's [capabilities](Subagent::with_capabilities) limit
    /// it. That holds for the harness's built-in `research` subagent too,
    /// which `invoke_subagent` offers whether or not you add a subagent.
    ///
    /// So while any of those gates is set (the sandbox only with
    /// `run_command` enabled), `spawn()` refuses an agent that
    /// enables [`BuiltinTool::InvokeSubagent`](super::BuiltinTool::InvokeSubagent)
    /// or [`Self::with_define_subagent_tool`], with an
    /// [`AntigravityError::Config`] naming the gates, unless this is
    /// `true`. A policy set that only allows (such as
    /// [`policy::allow_all`](super::policy::allow_all)) is not a gate: it
    /// refuses nothing. Opt in once every subagent has only the builtins
    /// it may use unchecked:
    ///
    /// ```rust,no_run
    /// use genai_rs::antigravity::{AntigravityAgent, BuiltinTool, Capabilities, Subagent, policy};
    ///
    /// let builder = AntigravityAgent::builder()
    ///     .add_workspace("/repo")                         // containment: a gate
    ///     .add_policy(policy::deny("run_command"))        // a gate
    ///     .with_capabilities(Capabilities::read_only().enable(BuiltinTool::InvokeSubagent))
    ///     .add_subagent(Subagent::new("reader").with_description("Reads files."))
    ///     // The subagents may read files outside /repo; accept that.
    ///     .with_ungated_subagents(true)
    ///     // ...
    ///     ;
    /// # let _ = builder;
    /// ```
    #[must_use]
    pub fn with_ungated_subagents(mut self, allow: bool) -> Self {
        self.ungated_subagents = allow;
        self
    }

    /// Launches the harness, performs the handshake, connects the
    /// WebSocket, and initializes the conversation.
    ///
    /// # Errors
    ///
    /// - [`AntigravityError::Config`] when write-capable built-ins or MCP
    ///   servers are enabled without any policy or pre-tool hook (safety
    ///   parity with the reference SDK), when subagents are enabled
    ///   alongside a gate they bypass without
    ///   [`Self::with_ungated_subagents`], or when a model or a Gemini base
    ///   URL is configured without a non-blank API key.
    /// - [`AntigravityError::HarnessNotFound`] when discovery fails, or
    ///   when the path [`Self::with_harness_path`] or
    ///   `ANTIGRAVITY_HARNESS_PATH` names is not a file.
    /// - [`AntigravityError::HandshakeFailed`] / [`AntigravityError::InitFailed`]
    ///   with the harness's stderr tail when startup fails.
    pub async fn spawn(self) -> Result<AntigravityAgent, AntigravityError> {
        let dispatcher = ToolDispatcher::new(self.tools.clone(), &self.tool_services);
        self.validate(&dispatcher)?;
        let policy_engine = PolicyEngine::new(self.policies.clone());

        let binary = process::discover_harness(self.harness_path.as_deref()).await?;

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
            env: self.options.harness_env.clone(),
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
                return Err(AntigravityError::init_failed_by(
                    "connecting to the harness failed",
                    stderr,
                    e,
                ));
            }
        };

        // Conversation init.
        let config = self.build_harness_config(&dispatcher);
        let sandbox_requested = self.sandboxes_commands();
        let init = protocol::InitializeConversationEvent {
            config: Some(config),
        };

        let mut agent = AntigravityAgent {
            harness,
            session,
            dispatcher,
            policy_engine: Arc::new(policy_engine),
            pre_tool: self.pre_tool,
            questions: self.questions,
            post_tool: self.post_tool,
            lifecycle: self.lifecycle,
            conversation_id: None,
            initial_history: Vec::new(),
            sandbox_status: None,
            turn_timeout: self.turn_timeout.resolve(),
            idle: Arc::new(tokio::sync::watch::channel(true).0),
            trigger_fired: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            turn_sync: Arc::new(tokio::sync::Mutex::new(())),
            turn_unfinished: false,
            trigger_tasks: triggers::TriggerTasks::default(),
        };

        let started = match agent.initialize(init).await {
            Ok(()) => agent.start_session().await,
            Err(e) => Err(e),
        };
        match started {
            Ok(()) => {
                if sandbox_requested
                    && let Some(status) = &agent.sandbox_status
                    && status.available == Some(false)
                {
                    tracing::warn!(
                        "run_command asked for the OS sandbox, which is unavailable ({}); \
                         commands run unsandboxed",
                        status
                            .unavailable_reason
                            .as_deref()
                            .unwrap_or("no reason given")
                    );
                }
                // Start triggers only once the conversation is live. Each
                // task writes through the shared sink handle (the same
                // out-of-band path CancelHandle uses) and watches the
                // agent's idle flag.
                for trigger in self.triggers {
                    let deliver = agent.trigger_delivery();
                    agent.trigger_tasks.push(triggers::spawn_trigger_task(
                        trigger,
                        agent.idle.subscribe(),
                        Arc::clone(&agent.turn_sync),
                        move |message| deliver(message),
                    ));
                }
                for trigger in self.custom_triggers {
                    let context = triggers::TriggerContext::new(
                        agent.idle.subscribe(),
                        Arc::clone(&agent.turn_sync),
                        agent.trigger_delivery(),
                    );
                    agent
                        .trigger_tasks
                        .push(triggers::spawn_custom_trigger(trigger, context));
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

    /// Whether the harness confines file tools to the workspaces: on by
    /// default, and only with a workspace to confine to (`spawn()` rejects
    /// an explicit opt-in without one).
    fn contains_workspaces(&self) -> bool {
        self.workspace_containment.unwrap_or(true) && !self.workspaces.is_empty()
    }

    /// Whether the agent's own commands run in the OS sandbox: asked for,
    /// and `run_command` enabled (its options apply only then).
    fn sandboxes_commands(&self) -> bool {
        self.capabilities.run_command().sandbox()
            && self.capabilities.is_enabled(super::BuiltinTool::RunCommand)
    }
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
