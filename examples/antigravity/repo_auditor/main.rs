//! # Repo Auditor — agentic codebase review on the Antigravity harness
//!
//! An [Antigravity](../../../docs/ANTIGRAVITY.md) agent pointed at a small
//! (deliberately vulnerable) project in `fixture/`. The agent explores the
//! code with the harness's read-only built-in tools, delegates per-file
//! analysis to a `file_auditor` subagent, grades every finding with a
//! deterministic Rust `#[tool]` severity classifier, and finishes with a
//! structured JSON audit report.
//!
//! ## What this exercises
//!
//! - Workspaces (`add_workspace`) and read-only built-in tools
//! - `deny_all()` + selective `allow(...)` policies, evaluated in Rust
//! - A pre-tool hook guarding secret files (defense in depth over policy)
//! - A custom `#[tool]` function shared with a subagent
//! - Subagent registration (`add_subagent` + `BuiltinTool::StartSubagent`)
//! - Structured output via `with_response_schema` (the `finish` tool schema)
//! - Live step streaming (`send_streaming` + `AgentEvent`)
//!
//! ## Things to know
//!
//! - Keep an auditor read-only: never enable `edit_file` or `run_command`.
//!   `deny_all()` also keeps tools added in later harness versions denied.
//! - Severity policy lives in code (`classify_severity`), and `render()`
//!   cross-checks the model's report against it rather than trusting it.
//! - Cost: a run makes ~35 model calls and uses roughly 300–650K prompt
//!   tokens on harness 0.1.18, because every call resends the trajectory and
//!   the harness's requests get almost no implicit cache hits (4K of 274K
//!   cached, measured 2026-09-24).
//!
//! ## Running
//!
//! ```bash
//! pip install google-antigravity==0.1.18   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=your_api_key
//! cargo run --example repo_auditor --features antigravity
//! LOUD_WIRE=1 cargo run --example repo_auditor --features antigravity  # wire trace
//! ```
//!
//! ## Sample output (trimmed from a real run against harness 0.1.18)
//!
//! ```text
//! === Repo Auditor (Antigravity harness) ===
//!
//! Workspace: .../examples/antigravity/repo_auditor/fixture
//! Harness up. conversation_id=Some("...")
//!
//! --- Audit in progress ---
//! [list_directory] file:///.../fixture
//! [list_directory] file:///.../fixture/app
//! [view_file] file:///.../fixture/README.md
//! [pre-tool hook] denied view_file on /.../fixture/.env
//! [DENIED] Secret files are off-limits; report them as findings instead.
//! [harness noise] Secret files are off-limits; ... ("denied by pre-tool hook: ...")
//! [start_subagent] delegated (subagent runs its own trajectory)
//! [view_file] file:///.../fixture/app/database.py
//! [view_file] file:///.../fixture/app/backup.py
//! [tool] classify_severity -> {"category":"sql_injection","severity":"critical"}
//! [custom tool dispatched] classify_severity
//! [tool] classify_severity -> {"category":"command_injection","severity":"critical"}
//! [custom tool dispatched] classify_severity
//! [tool] classify_severity -> {"category":"hardcoded_credentials","severity":"high"}
//! [custom tool dispatched] classify_severity
//! [finish] structured report received
//!
//! --- Audit report ---
//! Repo summary: The audited project, notes-app, is a lightweight Python note-taking
//! application providing database utilities ... alongside backup and restore helpers.
//!
//! Findings (5):
//!   1. [CRITICAL] app/database.py — SQL Injection in User Query Construction
//!      category: sql_injection | fix: Use parameterized queries with SQLite placeholders ...
//!   2. [HIGH] app/database.py — Hardcoded Credential in Database Module
//!      category: hardcoded_credentials | fix: Remove hardcoded credentials from source ...
//!   3. [CRITICAL] app/backup.py — OS Command Injection in Notes Backup
//!      category: command_injection | fix: Avoid passing shell command strings to os.system ...
//!   4. [CRITICAL] app/backup.py — OS Command Injection in Notes Restore
//!      category: command_injection | fix: Avoid shell invocation via os.system ...
//!   5. [HIGH] .env — Committed Secrets in .env Configuration File
//!      category: hardcoded_credentials | fix: Add .env to .gitignore, untrack the file ...
//!
//! Overall risk: CRITICAL
//! Severity cross-check: all findings match the classifier table.
//!
//! Usage: prompt=Some(408606) total=Some(427009)
//! ```

mod report;

use futures_util::StreamExt;
use genai_rs::CallableFunction;
use genai_rs::antigravity::{
    AgentEvent, AntigravityAgent, BuiltinTool, Capabilities, ErrorSeverity, PreToolDecision,
    Subagent, ToolAction, ToolDecision, policy,
};
use genai_rs_macros::tool;
use std::error::Error;
use std::io::{Write, stdout};
use std::path::Path;
use std::time::Duration;

/// Returns the severity for one security finding as JSON. Answers come from
/// a fixed severity table (company policy), not model judgment — the demo
/// cross-checks the final report against the same table.
#[tool(category(
    description = "The vulnerability category of the finding",
    // Keep in sync with report::SEVERITY_TABLE (macro attributes need literals).
    enum_values = [
        "sql_injection",
        "command_injection",
        "hardcoded_credentials",
        "path_traversal",
        "insecure_deserialization",
        "weak_crypto",
        "other"
    ]
))]
fn classify_severity(category: String) -> String {
    serde_json::json!({
        "category": category,
        "severity": report::severity_for(&category),
    })
    .to_string()
}

const AUDITOR_INSTRUCTIONS: &str = "You are a security auditor reviewing one code workspace. \
     Workflow: (1) explore the workspace with list/view tools; \
     (2) delegate the detailed review of the workspace's source files to the \
     file_auditor subagent (one delegation for the whole review) and use the \
     findings it reports back; \
     (3) call classify_severity exactly once per finding and copy its \
     'severity' answer into the report verbatim — never grade severity \
     yourself; (4) finish with the structured report. Report file paths \
     relative to the workspace root. Only report real vulnerabilities in \
     the project's own source code. Never touch paths outside the workspace.";

/// The task prompt. The workspace root is announced to the model
/// automatically (`with_workspace_announcement`, on by default) and appended
/// to the subagent's instructions too, so neither the task nor the subagent
/// has to spell out the absolute path — the pre-tool hook still enforces the
/// boundary regardless.
const AUDIT_TASK: &str = "Audit this project's workspace for security vulnerabilities. \
     Inspect its files — including dotfiles like .env, which often hold \
     committed secrets. Delegate the source review to the file_auditor \
     subagent, classify each finding's severity with the classify_severity \
     tool, then produce the structured report.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = match std::env::var("GEMINI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => {
            // Empty counts as absent: a fork push gets the secret as ""
            // rather than unset, and spawning with it fails mid-turn
            // instead of skipping.
            println!("Skipping: GEMINI_API_KEY not set");
            return Ok(());
        }
    };

    // The deliberately vulnerable sample project that ships next to this file.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/antigravity/repo_auditor/fixture")
        .canonicalize()?;
    let workspace = fixture.to_string_lossy().into_owned();

    println!("=== Repo Auditor (Antigravity harness) ===\n");
    println!("Workspace: {workspace}");

    // The subagent runs in its own trajectory with the default read-only
    // built-ins. Custom tools are referenced by name and must also be
    // registered on the parent (dispatch goes through the parent's registry).
    // Its trajectory does not inherit the parent's context, but the workspace
    // announcement is appended to its instructions automatically, so it need
    // not name the workspace root itself.
    let file_auditor = Subagent::new("file_auditor")
        .with_description(
            "Reviews the workspace's source files for security vulnerabilities \
             and reports each finding with its file, category, and evidence.",
        )
        .with_system_instructions(
            "Focus on injection vectors (SQL, shell) and secrets committed to \
             source. Read the code before concluding anything; cite the exact \
             function for each finding. Use classify_severity for severities.",
        )
        .add_tool("classify_severity");

    let mut agent = AntigravityAgent::builder()
        .with_api_key(api_key)
        .with_model(genai_rs::DEFAULT_MODEL)
        .with_system_instructions(AUDITOR_INSTRUCTIONS)
        .add_workspace(&workspace)
        // Read-only built-ins plus subagent delegation. start_subagent is
        // write-capable, so spawn() requires a policy below (safety gate).
        .with_capabilities(Capabilities::read_only().enable(BuiltinTool::StartSubagent))
        .add_tool(ClassifySeverityCallable.declaration())
        .add_subagent(file_auditor)
        // Deny by default; allow exactly the tools this audit needs.
        // Exact-name rules beat the deny_all() wildcard, so order is free.
        .add_policy(policy::deny_all())
        .add_policy(policy::allow("list_directory"))
        .add_policy(policy::allow("search_directory"))
        .add_policy(policy::allow("find_file"))
        .add_policy(policy::allow("view_file"))
        .add_policy(policy::allow("start_subagent"))
        .add_policy(policy::allow("classify_severity"))
        .add_policy(policy::allow("finish"))
        // Defense in depth on top of the allow rules: even allowed read
        // tools are confined to the workspace, and secret material like
        // .env files stays off-limits.
        .on_pre_tool({
            let workspace = workspace.clone();
            move |call| {
                const FILE_TOOLS: [&str; 4] = [
                    "view_file",
                    "list_directory",
                    "find_file",
                    "search_directory",
                ];
                if !FILE_TOOLS.contains(&call.name.as_str()) {
                    return PreToolDecision::Allow;
                }
                // Fail closed: a file tool whose path this hook cannot find
                // is denied, not waved through. A gate keyed on an argument
                // name that is not there allows *everything* — exactly how
                // this check sat dead for a harness release, reading the
                // action-record keys (`filePath`) while the hook was being
                // handed the model's arguments (`AbsolutePath`).
                let Some(path) = tool_path(&call.args) else {
                    println!("[pre-tool hook] denied {}: no recognizable path", call.name);
                    return PreToolDecision::deny("Could not determine the target path.");
                };
                if !path.starts_with(workspace.as_str()) {
                    println!(
                        "[pre-tool hook] denied {} outside workspace: {path}",
                        call.name
                    );
                    return PreToolDecision::deny(format!(
                        "Path is outside the workspace {workspace}; stay inside it."
                    ));
                }
                if call.name == "view_file" && path.contains(".env") {
                    println!("[pre-tool hook] denied view_file on {path}");
                    return PreToolDecision::deny(
                        "Secret files are off-limits; report them as findings instead.",
                    );
                }
                PreToolDecision::Allow
            }
        })
        // Audit trail for every completed tool call (custom and harness-side).
        .on_post_tool(|outcome| {
            let result = outcome
                .result
                .as_deref()
                .or(outcome.error.as_deref())
                .unwrap_or("<no result>");
            println!("[tool] {} -> {result}", outcome.name);
        })
        // Structured output: the harness enforces this schema on `finish`.
        .with_response_schema(report::schema())
        .with_turn_timeout(Duration::from_secs(600))
        .spawn()
        .await?;

    println!(
        "Harness up. conversation_id={:?}\n",
        agent.conversation_id()
    );
    println!("--- Audit in progress ---");

    // Stream the whole audit turn: harness tool actions, subagent
    // delegation, custom-tool dispatches, and the final structured report.
    let mut final_response = None;
    {
        let mut stream = agent.send_streaming(AUDIT_TASK).await?;
        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::ToolAction {
                    action, decision, ..
                } => print_action(&action, &decision),
                AgentEvent::ToolCallDispatched { name, .. } => {
                    println!("[custom tool dispatched] {name}");
                }
                AgentEvent::ThinkingDelta(_) => {
                    // Progress dots keep the stream visibly alive.
                    print!(".");
                    stdout().flush()?;
                }
                AgentEvent::TextDelta(_) => {}
                // Severe errors are serious but do NOT end the turn (a
                // turn-ending failure surfaces as AntigravityError instead);
                // transient ones are harness-internal noise safe to ignore.
                AgentEvent::Error { message, severity } => match severity {
                    ErrorSeverity::Severe => eprintln!("[harness error] {message}"),
                    _ => eprintln!("[harness noise] {message}"),
                },
                AgentEvent::Finished(response) => {
                    final_response = Some(response);
                    break;
                }
                _ => {} // AgentEvent is non-exhaustive (Evergreen).
            }
        }
    }
    println!();

    let response = final_response.ok_or("turn ended without a Finished event")?;
    let structured = response
        .structured_output()
        .ok_or("no structured output in final response")?;
    let audit: report::AuditReport = serde_json::from_value(structured.clone())?;

    println!("\n--- Audit report ---");
    let mismatches = report::render(&audit);

    if let Some(usage) = response.usage() {
        println!(
            "\nUsage: prompt={:?} total={:?}",
            usage.prompt_token_count, usage.total_token_count
        );
    }

    agent.shutdown().await?; // graceful: persists the harness trajectory

    if mismatches > 0 {
        return Err(format!("{mismatches} finding(s) contradict the severity classifier").into());
    }
    Ok(())
}

/// The target path of a file-tool call, as the pre-tool hook sees it.
///
/// The hook is handed the *model's* arguments, whose names are the
/// harness's tool schema (on 0.1.18: `AbsolutePath` for `view_file`,
/// `DirectoryPath` for `list_directory`, `SearchDirectory` / `SearchPath`
/// for the finders). A confirmation step carries the action record's
/// camelCase keys instead, and models — subagents especially — sometimes
/// echo the record's snake_case keys back as arguments, so all three
/// spellings are accepted. Anything else is not a path this hook can vouch
/// for, and the caller denies it.
fn tool_path(args: &serde_json::Value) -> Option<&str> {
    const PATH_KEYS: [&str; 8] = [
        "AbsolutePath",
        "DirectoryPath",
        "SearchDirectory",
        "SearchPath",
        "filePath",
        "directoryPath",
        "file_path",
        "directory_path",
    ];
    PATH_KEYS
        .iter()
        .find_map(|key| args[*key].as_str())
        .map(|p| p.strip_prefix("file://").unwrap_or(p))
}

/// One concise progress line per harness-side tool action. Denied actions
/// (blocked by a policy rule or the pre-tool hook) are flagged distinctly so
/// they don't read like executed ones.
fn print_action(action: &ToolAction, decision: &ToolDecision) {
    if let ToolDecision::Denied { reason } = decision {
        // A hook-denied call never ran, so there is no action record for
        // it: the harness reports it as an error step carrying the reason.
        match action {
            ToolAction::Error(_) => println!("[DENIED] {reason}"),
            other => println!("[DENIED {}] {reason}", other.tool_name()),
        }
        return;
    }
    match action {
        ToolAction::ListDirectory(a) => {
            println!(
                "[list_directory] {}",
                a.directory_path.as_deref().unwrap_or("?")
            );
        }
        ToolAction::ViewFile(a) => {
            println!("[view_file] {}", a.file_path.as_deref().unwrap_or("?"));
        }
        ToolAction::SearchDirectory(a) => {
            println!(
                "[search_directory] query={:?}",
                a.query.as_deref().unwrap_or("")
            );
        }
        ToolAction::FindFile(a) => {
            println!("[find_file] query={:?}", a.query.as_deref().unwrap_or(""));
        }
        ToolAction::InvokeSubagent(_) => {
            println!("[start_subagent] delegated (subagent runs its own trajectory)");
        }
        ToolAction::Finish(_) => println!("[finish] structured report received"),
        ToolAction::Error(a) => {
            println!("[error step] {}", a.error_message.as_deref().unwrap_or("?"));
        }
        other => println!("[{}]", other.tool_name()),
    }
}
