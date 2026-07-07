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
//! ## Running
//!
//! ```bash
//! pip install google-antigravity==0.1.5   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=your_api_key
//! cargo run --example repo_auditor --features antigravity
//! LOUD_WIRE=1 cargo run --example repo_auditor --features antigravity  # wire trace
//! ```
//!
//! ## Sample output (trimmed from a real run)
//!
//! ```text
//! === Repo Auditor (Antigravity harness) ===
//!
//! Workspace: .../examples/real_world/repo_auditor/fixture
//! Harness up. conversation_id=Some("9d772d7c9085062190aefd6723785b77")
//!
//! --- Audit in progress ---
//! [list_directory] file:///.../fixture
//! [list_directory] file:///.../fixture/app
//! [pre-tool hook] denied view_file on /.../fixture/.env
//! [start_subagent] delegated (subagent runs its own trajectory)
//! [view_file] file:///.../fixture/app/backup.py
//! [view_file] file:///.../fixture/app/database.py
//! [tool] classify_severity -> {"result":"{\"category\":\"sql_injection\",\"severity\":\"critical\"}"}
//! [tool] classify_severity -> {"result":"{\"category\":\"hardcoded_credentials\",\"severity\":\"high\"}"}
//! [tool] classify_severity -> {"result":"{\"category\":\"command_injection\",\"severity\":\"critical\"}"}
//! [search_directory] query="password|secret|key|token|auth|db_password"
//! [finish] structured report received
//!
//! --- Audit report ---
//! Repo summary: The project is a deliberately flawed notes-app fixture designed for
//! testing security auditing tools. It contains multiple high-severity vulnerabilities...
//!
//! Findings (3):
//!   1. [CRITICAL] app/database.py — SQL Injection in find_user function
//!      category: sql_injection | fix: Use parameterized queries instead of string formatting...
//!   2. [HIGH] app/database.py — Hardcoded DB Password in database.py
//!      category: hardcoded_credentials | fix: Remove hardcoded passwords from the source code...
//!   3. [CRITICAL] app/backup.py — Command Injection in backup_notes and restore_notes functions
//!      category: command_injection | fix: Avoid using `os.system` with user-controlled input...
//!
//! Overall risk: CRITICAL
//! Severity cross-check: all findings match the classifier table.
//!
//! Usage: prompt=Some(9425) total=Some(9725)
//! ```

mod report;

use futures_util::StreamExt;
use genai_rs::CallableFunction;
use genai_rs::antigravity::{
    AgentEvent, AntigravityAgent, BuiltinTool, Capabilities, PreToolDecision, Subagent, ToolAction,
    policy,
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

/// The task prompt. The harness passes the workspace to its tools but does
/// not announce the path to the model, so a workspace-rooted task must name
/// it explicitly (the pre-tool hook below enforces the boundary regardless).
fn audit_task(workspace: &str) -> String {
    format!(
        "Audit the project rooted at {workspace} for security vulnerabilities. \
         Stay inside that directory. Inspect its files — including dotfiles \
         like .env, which often hold committed secrets. Delegate the source \
         review to the file_auditor subagent, classify each finding's \
         severity with the classify_severity tool, then produce the \
         structured report."
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not found in environment");

    // The deliberately vulnerable sample project that ships next to this file.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/real_world/repo_auditor/fixture")
        .canonicalize()?;
    let workspace = fixture.to_string_lossy().into_owned();

    println!("=== Repo Auditor (Antigravity harness) ===\n");
    println!("Workspace: {workspace}");

    // The subagent runs in its own trajectory with the default read-only
    // built-ins. Custom tools are referenced by name and must also be
    // registered on the parent (dispatch goes through the parent's registry).
    // Its trajectory does not inherit the parent's context, so name the
    // workspace root in its (appended) instructions too.
    let file_auditor = Subagent::new("file_auditor")
        .with_description(
            "Reviews the workspace's source files for security vulnerabilities \
             and reports each finding with its file, category, and evidence.",
        )
        .with_system_instructions(format!(
            "The project under review is rooted at {workspace}; use absolute \
             paths under that directory only. Focus on injection vectors \
             (SQL, shell) and secrets committed to source. Read the code \
             before concluding anything; cite the exact function for each \
             finding. Use classify_severity for severities."
        ))
        .add_tool("classify_severity");

    let mut agent = AntigravityAgent::builder()
        .with_api_key(api_key)
        .with_model("gemini-3-flash-preview")
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
        // .env files stays off-limits. Action paths arrive as file:// URIs.
        .on_pre_tool({
            let workspace = workspace.clone();
            move |call| {
                let raw = call.args["filePath"]
                    .as_str()
                    .or_else(|| call.args["directoryPath"].as_str());
                if let Some(path) = raw.map(|p| p.strip_prefix("file://").unwrap_or(p)) {
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
                }
                PreToolDecision::Allow
            }
        })
        // Audit trail for every custom-tool execution.
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
        let mut stream = agent.send_streaming(audit_task(&workspace)).await?;
        while let Some(event) = stream.next().await {
            match event? {
                AgentEvent::ToolAction(action) => print_action(&action),
                AgentEvent::ToolCallDispatched { name, .. } => {
                    println!("[custom tool dispatched] {name}");
                }
                AgentEvent::ThinkingDelta(_) => {
                    // Progress dots keep the stream visibly alive.
                    print!(".");
                    stdout().flush()?;
                }
                AgentEvent::TextDelta(_) => {}
                AgentEvent::Error(message) => eprintln!("[harness error] {message}"),
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

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  HARNESS /path/to/localharness (pid N) - process spawn");
    println!(
        "  WS Send: {{\"config\": ...}} - init with workspace, tools, customSubagents, finish schema"
    );
    println!("  WS Receive: {{\"initializeConversationResponse\": ...}} - cascade id");
    println!("  WS Send: {{\"userInput\": ...}} - the audit task");
    println!("  WS Receive: {{\"stepUpdate\": ...}} - listDirectory/viewFile/invokeSubagent steps");
    println!(
        "  WS Receive: {{\"toolConfirmationRequest\": {{}}}} / WS Send: {{\"toolConfirmation\": \
         {{\"accepted\": ...}}}} - Rust-side policy verdicts"
    );
    println!(
        "  WS Receive: {{\"toolCall\": ...}} / WS Send: {{\"toolResponse\": ...}} - classify_severity"
    );
    println!("  WS Receive: {{\"stepUpdate\": {{\"finish\": ...}}}} - structured report\n");

    println!("--- Production Considerations ---");
    println!("• Keep audits read-only: never enable edit_file/run_command for an auditor agent");
    println!("• Policies are allow-lists here — new harness tools stay denied by deny_all()");
    println!(
        "• Put severity policy in code (a table), not prompts: it stays auditable and testable"
    );
    println!("• Cross-check model output against deterministic tools, as render() does here");
    println!(
        "• Bound runaway turns with with_turn_timeout; add with_save_dir to resume long audits"
    );
    println!("• Pin the harness wheel (google-antigravity==0.1.5, SUPPORTED_HARNESS_VERSION)");

    if mismatches > 0 {
        return Err(format!("{mismatches} finding(s) contradict the severity classifier").into());
    }
    Ok(())
}

/// One concise progress line per completed harness-side tool action.
fn print_action(action: &ToolAction) {
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
