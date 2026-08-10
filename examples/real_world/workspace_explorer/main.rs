//! # Workspace Explorer — watching an agent work, and gating it live
//!
//! Points the harness's read-only builtins at a real directory and then
//! *observes every move it makes*: each `list_directory` / `view_file` /
//! `search_directory` surfaces as a typed [`ToolAction`] on the stream,
//! and a pre-tool hook adjudicates custom tool calls before they run.
//!
//! Where `repo_auditor` shows the agent producing a structured verdict,
//! this one is about the **control and observability surface** underneath:
//! what the agent touched, in what order, and what you were able to stop.
//!
//! What this example demonstrates that the others don't:
//!
//! - **Workspaces end to end** — `add_workspace` on a directory with real
//!   content, so the builtin file tools actually run against real files
//!   (rather than being merely enabled).
//! - **Typed [`ToolAction`] observation** — matching the streamed action
//!   variants to build an audit trail, including `decision` (was it
//!   allowed or denied?) and `trajectory_id` (whose action was it?).
//! - **A live `on_pre_tool` gate** — a hook that denies by *content*, not
//!   just by tool name, which is the thing a static policy cannot express.
//! - **`on_post_tool`** — recording what each custom call actually
//!   returned.
//!
//! ## Requirements
//!
//! ```bash
//! pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
//! export GEMINI_API_KEY=...
//! cargo run --example workspace_explorer --features antigravity
//! LOUD_WIRE=1 cargo run --example workspace_explorer --features antigravity
//! ```
//!
//! ## Expected output
//!
//! ```text
//! === Workspace Explorer ===
//!
//! Workspace: /tmp/workspace-explorer-.../
//!   src/config.rs, src/main.rs, README.md, secrets.env
//!
//! --- Exploring ---
//! [action] list_directory      (allowed)  .
//! [action] view_file           (allowed)  README.md
//! [tool]   record_finding      -> ok
//!
//! --- Audit trail (6 streamed actions, 12 post-tool callbacks, 0 denied) ---
//! ```
//!
//! The deny path is prompt-dependent: the system instruction tells the
//! agent not to touch credentials, so a well-behaved run never trips the
//! hook. Seeding `secrets.env` and gating on content means the refusal is
//! there when it is needed rather than relied upon for the demo.

use futures_util::StreamExt;
use genai_rs::CallableFunction;
use genai_rs::antigravity::{
    AgentEvent, AntigravityAgent, Capabilities, PreToolDecision, ToolAction, policy,
};
use genai_rs_macros::tool;
use std::sync::{Arc, Mutex};

/// Records a finding about the workspace.
///
/// A deliberately trivial custom tool — the interesting part is that the
/// pre-tool hook inspects its *arguments* and can refuse the call.
#[tool(
    file(description = "The file the finding is about"),
    note(description = "One sentence describing the finding")
)]
fn record_finding(file: String, note: String) -> String {
    // Build it with `json!` rather than interpolating into a string
    // literal: `note` is free-form model output about code, so a quoted
    // identifier or a backslash is likely rather than contrived, and
    // hand-built JSON breaks on both.
    serde_json::json!({ "recorded": true, "file": file, "note": note }).to_string()
}

/// One observed event, for the end-of-run audit trail.
#[derive(Debug)]
enum Audit {
    /// A harness-executed builtin (file tools, etc.).
    HarnessAction { name: String, allowed: bool },
    /// A completed tool call as seen by `on_post_tool`. Note this fires
    /// for harness-executed builtins as well as custom tools — the hook is
    /// "a tool finished", not "your code ran".
    PostTool { name: String, ok: bool },
    /// A custom tool call the pre-tool hook refused.
    Denied { name: String, reason: String },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY not set");

    let workspace = seed_workspace()?;
    let workspace_path = workspace.path().to_string_lossy().to_string();

    println!("=== Workspace Explorer ===\n");
    println!("Workspace: {workspace_path}");
    println!("  src/config.rs, src/main.rs, README.md, secrets.env\n");

    // The audit trail is shared between the hooks (which run inline on the
    // event pump) and the stream loop, so it needs interior mutability.
    let audit: Arc<Mutex<Vec<Audit>>> = Arc::new(Mutex::new(Vec::new()));
    let hook_audit = Arc::clone(&audit);
    let post_audit = Arc::clone(&audit);

    let mut agent = AntigravityAgent::builder()
        .with_api_key(api_key)
        .with_model("gemini-3.6-flash")
        .with_system_instructions(
            "You are exploring a small codebase. List the directory, read the files \
             that look interesting, and call record_finding once for each notable \
             thing you find. Do not read or record credentials. Finish when done.",
        )
        // Real content for the builtins to work against. The crate
        // announces the root to the model by default, so it does not have
        // to guess paths.
        .add_workspace(workspace_path.clone())
        // Read-only builtins are exactly what an explorer needs; the file
        // tools below are all in this set.
        .with_capabilities(Capabilities::read_only())
        .add_tool(RecordFindingCallable.declaration())
        // Static policy: the shape of what may run.
        .add_policy(policy::deny_all())
        .add_policy(policy::allow("record_finding"))
        .add_policy(policy::allow("list_directory"))
        .add_policy(policy::allow("view_file"))
        .add_policy(policy::allow("search_directory"))
        .add_policy(policy::allow("find_file"))
        .add_policy(policy::allow("finish"))
        // Dynamic gate: policies match on *names*, so they cannot express
        // "this particular call is bad". Content-based refusal lives here.
        .on_pre_tool(move |call| {
            // The hook is consulted for harness builtins too, and they use
            // their own argument names — `view_file` sends `file_path`, not
            // `file`. Reading only the custom tool's keys would silently
            // allow every builtin (a missing key indexes to Null), so the
            // gate would cover recording a secret but not *reading* one.
            const ARG_KEYS: [&str; 5] = ["note", "file", "file_path", "directory_path", "query"];
            let looks_secret = ARG_KEYS
                .iter()
                .filter_map(|k| call.args[*k].as_str())
                .any(|s| {
                    let s = s.to_lowercase();
                    s.contains("secret") || s.contains("api_key") || s.contains("password")
                });
            if looks_secret {
                let reason = format!("refusing to touch a secret value via {}", call.name);
                println!("[DENIED] {}: {reason}", call.name);
                hook_audit.lock().unwrap().push(Audit::Denied {
                    name: call.name.clone(),
                    reason: reason.clone(),
                });
                PreToolDecision::deny(reason)
            } else {
                PreToolDecision::Allow
            }
        })
        // Post-tool: what the call actually returned. Denied calls never
        // execute, so they never reach this hook.
        .on_post_tool(move |outcome| {
            println!(
                "[tool]   {:<20} -> {}",
                outcome.name,
                if outcome.error.is_some() {
                    "error"
                } else {
                    "ok"
                }
            );
            post_audit.lock().unwrap().push(Audit::PostTool {
                name: outcome.name.clone(),
                ok: outcome.error.is_none(),
            });
        })
        .spawn()
        .await?;

    println!("--- Exploring ---");
    {
        let mut stream = agent
            .send_streaming("Explore this workspace and record what you find.")
            .await?;

        while let Some(event) = stream.next().await {
            match event? {
                // The typed action surface: every harness-executed builtin
                // arrives here with its decision and originating trajectory.
                AgentEvent::ToolAction {
                    action, decision, ..
                } => {
                    let name = action.tool_name();
                    let allowed = decision.is_allowed();
                    // A few variants carry a path worth showing; the rest
                    // are summarized by name alone.
                    // `action` is boxed on the event, so match through it.
                    let detail = match action.as_ref() {
                        ToolAction::ViewFile(v) => v.file_path.clone().unwrap_or_default(),
                        ToolAction::ListDirectory(l) => {
                            l.directory_path.clone().unwrap_or_default()
                        }
                        ToolAction::SearchDirectory(s) => s.query.clone().unwrap_or_default(),
                        _ => String::new(),
                    };
                    println!(
                        "[action] {name:<20} ({})  {detail}",
                        if allowed { "allowed" } else { "denied" }
                    );
                    audit
                        .lock()
                        .unwrap()
                        .push(Audit::HarnessAction { name, allowed });
                }
                AgentEvent::Finished(_) => break,
                AgentEvent::Error { message, severity } => {
                    eprintln!("[error ({severity:?})] {message}");
                }
                _ => {}
            }
        }
    }

    // ---- Audit trail -------------------------------------------------
    // Snapshot and release: the guard must not be alive across the
    // shutdown await below.
    let trail: Vec<Audit> = std::mem::take(&mut *audit.lock().unwrap());
    let actions = trail
        .iter()
        .filter(|a| matches!(a, Audit::HarnessAction { .. }))
        .count();
    let calls = trail
        .iter()
        .filter(|a| matches!(a, Audit::PostTool { .. }))
        .count();
    let denied = trail
        .iter()
        .filter(|a| matches!(a, Audit::Denied { .. }))
        .count();

    println!(
        "\n--- Audit trail ({actions} streamed actions, {calls} post-tool callbacks, {denied} denied) ---"
    );
    for entry in trail.iter() {
        match entry {
            Audit::HarnessAction { name, allowed } => {
                println!(
                    "  action  {name:<20} {}",
                    if *allowed { "ok" } else { "denied" }
                );
            }
            Audit::PostTool { name, ok } => {
                println!("  posttool {name:<20} {}", if *ok { "ok" } else { "error" });
            }
            Audit::Denied { name, reason } => println!("  DENIED   {name:<20} {reason}"),
        }
    }
    agent.shutdown().await?;

    println!("\n=== Example Complete ===\n");

    println!("--- What You'll See with LOUD_WIRE=1 ---");
    println!("  WS Send: {{\"config\": {{\"workspaces\": [{{\"filesystemWorkspace\": ...}}]}}}}");
    println!("    - the workspace root the builtins are pointed at");
    println!("  WS Receive: {{\"stepUpdate\": {{\"listDirectory\": ...}}}} - a harness action");
    println!("  WS Receive: {{\"stepUpdate\": {{\"viewFile\": ...}}}} - each file read");
    println!("  WS Receive: {{\"toolCall\": ...}} / WS Send: {{\"toolResponse\": ...}}");
    println!("    - record_finding, dispatched through the crate's registry");
    println!("  (a denied call sends a toolResponse carrying the refusal reason,");
    println!("   so the model sees why and can adapt)\n");

    println!("--- Production Considerations ---");
    println!("• Policies gate by NAME; on_pre_tool gates by CONTENT. Use both —");
    println!("  a name-based allow cannot tell a safe argument from a dangerous one");
    println!("• Hooks run inline on the event pump: keep them non-blocking, and");
    println!("  never await a human decision inside one");
    println!("• ToolAction carries trajectory_id — with subagents running, that is");
    println!("  what tells the parent's actions apart from a subagent's");
    println!("• Denials are visible to the model as tool responses, so a good");
    println!("  reason string steers the next attempt instead of stalling it");
    println!("• Workspace announcement is on by default; disable it only if you");
    println!("  ground the model on paths yourself");

    Ok(())
}

/// Creates a small workspace with a planted credential, so the
/// content-based deny path has something real to refuse.
fn seed_workspace() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let dir = tempfile::Builder::new()
        .prefix("workspace-explorer-")
        .tempdir()?;
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src)?;

    std::fs::write(
        dir.path().join("README.md"),
        "# Widget Service\n\nA tiny service that widgets the widgets.\n\
         Configuration lives in `src/config.rs`.\n",
    )?;
    std::fs::write(
        src.join("main.rs"),
        "fn main() {\n    // TODO: handle the empty-input case\n    \
         println!(\"widgets\");\n}\n",
    )?;
    std::fs::write(
        src.join("config.rs"),
        "pub const RETRIES: u32 = 3;\n\
         // FIXME: timeout is not configurable\n\
         pub const TIMEOUT_SECS: u64 = 30;\n",
    )?;
    // The bait: present so the agent can find it, and the pre-tool hook
    // refuses to let its contents be recorded.
    std::fs::write(
        dir.path().join("secrets.env"),
        "API_KEY=sk-do-not-record-this\nPASSWORD=hunter2\n",
    )?;
    Ok(dir)
}
