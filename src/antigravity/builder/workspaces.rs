//! Workspace setters on [`AgentBuilder`]: the directories the agent works
//! in, whether they are announced to the model, and whether the harness
//! confines its file tools to them.

use super::AgentBuilder;

impl AgentBuilder {
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
    /// [custom](crate::antigravity::SystemInstructions::custom). **On by default.**
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
    /// [`AntigravityError::Config`](crate::antigravity::AntigravityError::Config).
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

    /// Whether workspace announcement is enabled (default on).
    pub(super) fn announce_workspaces(&self) -> bool {
        self.workspace_announcement.unwrap_or(true)
    }

    /// Whether the harness confines file tools to the workspaces: on by
    /// default, and only with a workspace to confine to (`spawn()` rejects
    /// an explicit opt-in without one).
    pub(super) fn contains_workspaces(&self) -> bool {
        self.workspace_containment.unwrap_or(true) && !self.workspaces.is_empty()
    }
}
