//! User-facing configuration types for Antigravity agents.

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
/// pip install google-antigravity==0.1.23
/// ```
///
/// The 0.1.5 → 0.1.10 move renamed two wire spellings that the bridge
/// *reads* (`STATE_IDLE` → `STATE_FULLY_IDLE`, `usageMetadata` →
/// `usageUpdate`); both old spellings are still accepted. The 0.1.10 →
/// 0.1.18 move changed one the bridge *sends*: a user message is now a
/// multi-part `userInput` object, and the old plain-string form is a
/// harness-side parse error. An outbound shape cannot be aliased, so a
/// build drives one revision. 0.1.23 added the `runWorkflow` toggle, which
/// this build writes (disabled) and 0.1.18 rejects at init with `unknown
/// field "runWorkflow"`, so this build drives 0.1.23 only.
///
/// "Degrades gracefully" is doing real work in the sentence above, too: a
/// renamed value an `Unknown` variant absorbs is invisible until behavior
/// that *matched* on it silently stops firing, and a rejected input is
/// reported only on the harness's stderr.
pub const SUPPORTED_HARNESS_VERSION: &str = "0.1.23";

/// How the harness frames the agent's task in its system prompt.
///
/// New in harness 0.1.18. Unset, the harness defaults to
/// [`Autonomous`](Self::Autonomous) — the same default as the reference
/// SDK, and a byte-identical system prompt. That default is why
/// [`BuiltinTool::AskQuestion`] needs [`Interactive`](Self::Interactive):
/// the autonomous prompt tells the model *"the user will not respond to
/// questions"*, so it asks in prose instead of calling the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AgentBehavior {
    /// Work unattended from start to finish.
    Autonomous,
    /// Work with a human in the loop: clarifying questions via
    /// `ask_question`, plus the harness's artifact conventions.
    Interactive,
    /// A pruned system prompt for small-context models.
    Minimal,
}

impl AgentBehavior {
    pub(crate) fn to_wire(self) -> protocol::AgentBehavior {
        match self {
            Self::Autonomous => protocol::AgentBehavior::Autonomous,
            Self::Interactive => protocol::AgentBehavior::Interactive,
            Self::Minimal => protocol::AgentBehavior::Minimal,
        }
    }
}

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
    /// Ask the user a clarifying question (`ask_question`). Write-capable.
    AskQuestion,
    /// Invoke a subagent (`start_subagent`). Write-capable.
    StartSubagent,
    /// Generate or edit images (`generate_image`). Write-capable.
    GenerateImage,
    /// Search the web (`search_web`). Write-capable (network egress).
    SearchWeb,
    /// Fetch the content of a URL (`read_url_content`). Write-capable
    /// (network egress) — unlike the reference SDK, which counts it as
    /// read-only and enables it by default: a fetched URL can carry data
    /// out as easily as it brings data in.
    ReadUrlContent,
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
            Self::ReadUrlContent => "read_url_content",
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
            Self::ReadUrlContent,
            Self::Finish,
        ]
    }

    /// Tools that only read state (no writes, deletes, commands, or
    /// network egress). This is the default capability set.
    ///
    /// It deliberately diverges from the reference SDK's 0.1.18
    /// `read_only()`, which dropped `list_directory` / `search_directory`
    /// / `find_file` as deprecated (the harness still serves all three)
    /// and added `read_url_content` and `schedule`. Without the three
    /// local-exploration tools a read-only agent can open a file only by
    /// guessing its path; `read_url_content` is egress (see
    /// [`BuiltinTool::ReadUrlContent`]); and `schedule` is not offered at
    /// all (see [`Capabilities`]).
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
/// The default is the read-only set ([`BuiltinTool::read_only`]). Enabling
/// any write-capable tool requires a policy or pre-tool hook at spawn time
/// (safety parity with the reference SDK).
///
/// Two harness builtins are not selectable here. `schedule` lets the agent
/// schedule its own future turns; this client drives one turn at a time
/// and cannot observe a turn it did not start, so a self-scheduled turn's
/// events would be read as the *next* `chat`'s response. It is always
/// sent disabled. `manage_task` (list/kill background tasks) follows
/// [`BuiltinTool::RunCommand`], as in the reference SDK.
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
    /// explicitly, disabled ones included: what an omitted flag means is
    /// the harness's call and has changed between revisions. Harness
    /// 0.1.18 treats omitted as off, but 0.1.10 exposed `manage_task` and
    /// `schedule` to every agent — including the default read-only one —
    /// because it had no flag for them at all.
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
            read_url_content: on(BuiltinTool::ReadUrlContent),
            // Left to the harness default (off); tool search only applies
            // to tools declared with deferred loading, which this crate
            // never declares.
            tool_search_config: None,
            manage_task: on(BuiltinTool::RunCommand),
            schedule: Some(protocol::ToolToggle::new(false)),
            run_workflow: Some(protocol::ToolToggle::new(false)),
        }
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
        assert!(!flags.read_url_content.unwrap().enabled);
        assert!(!flags.manage_task.unwrap().enabled);
        // `schedule` is never offered: a self-scheduled turn is one this
        // client cannot observe.
        assert!(!flags.schedule.unwrap().enabled);
        // `run_workflow` (0.1.23) is written, disabled, under every
        // capability set.
        assert!(!flags.run_workflow.unwrap().enabled);
        assert!(
            !Capabilities::all()
                .to_harness_side_tools()
                .run_workflow
                .unwrap()
                .enabled
        );
        assert!(
            !Capabilities::all()
                .to_harness_side_tools()
                .schedule
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn test_manage_task_follows_run_command() {
        // Reference-SDK parity: background tasks come from run_command, so
        // the tool that lists/kills them comes with it.
        let with = Capabilities::none().enable(BuiltinTool::RunCommand);
        assert!(with.to_harness_side_tools().manage_task.unwrap().enabled);
        let without = Capabilities::all().disable(BuiltinTool::RunCommand);
        assert!(!without.to_harness_side_tools().manage_task.unwrap().enabled);
    }

    #[test]
    fn test_read_url_content_is_write_capable() {
        // Egress, like search_web — so enabling it trips the safety gate.
        assert!(BuiltinTool::ReadUrlContent.is_write_capable());
        assert!(!Capabilities::read_only().is_enabled(BuiltinTool::ReadUrlContent));
        let caps = Capabilities::none().enable(BuiltinTool::ReadUrlContent);
        assert!(caps.has_write_tools());
        assert!(
            caps.to_harness_side_tools()
                .read_url_content
                .unwrap()
                .enabled
        );
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
            (BuiltinTool::ReadUrlContent, "read_url_content"),
            (BuiltinTool::Finish, "finish"),
        ];
        for (tool, name) in expected {
            assert_eq!(tool.wire_name(), name);
        }
    }
}
