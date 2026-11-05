# Repo Auditor Example

Agentic codebase security review running on the local
[Antigravity harness](../../../docs/ANTIGRAVITY.md) — the fullest of the
harness examples in this directory, and the one to read after
[`agent.rs`](../agent.rs).

## Overview

The agent audits `fixture/`, a tiny note-taking app with planted
vulnerabilities:

1. **Explores** the workspace with the harness's read-only built-in tools
   (`list_directory`, `view_file`, `search_directory`, `find_file`)
2. **Delegates** per-file source review to a `file_auditor` **subagent**
   that runs in its own trajectory
3. **Grades** every finding by calling `classify_severity` — a deterministic
   Rust `#[tool]` backed by a fixed severity table (severity policy lives in
   code, not in the prompt)
4. **Finishes** with a structured JSON report enforced by
   `with_response_schema` (the harness `finish` tool's schema)

After the run, `report::render()` cross-checks each finding's severity
against the same Rust table the tool answered from — a deterministic
correctness check that doesn't depend on LLM phrasing.

## Safety layout

| Layer | Configuration |
|-------|---------------|
| Capabilities | `Capabilities::read_only().enable(BuiltinTool::InvokeSubagent)` |
| Policies | `deny_all()` + explicit `allow(...)` for each tool the audit needs |
| Pre-tool hook | Confines file tools to the workspace and denies `view_file` on `.env` paths, even though `view_file` is allowed; denies a file tool whose path it cannot read (fail closed) |
| Post-tool hook | Logs every completed tool call, custom and harness-side (audit trail) |
| Turn bound | `with_turn_timeout(Duration::from_secs(600))` |

The pre-tool hook fires *after* the policy allow — defense in depth: the
agent is told to inventory dotfiles, tries to open `fixture/.env`, and gets
denied by Rust code.

These layers gate the **parent agent's** calls. On harness 0.1.23 a
subagent's own tool calls reach neither the policies nor the hook, and
workspace containment does not confine them: in the run below the
`file_auditor` subagent read `fixture/.env` right after the parent was
denied it. The subagent's read-only capabilities are its only bound
(see Subagents in `docs/ANTIGRAVITY.md`).

## Running

```bash
pip install google-antigravity==0.1.23   # ships the localharness binary
# ...or point at an existing binary:
export ANTIGRAVITY_HARNESS_PATH=/path/to/localharness

export GEMINI_API_KEY=your_api_key
cargo run --example repo_auditor --features antigravity
```

Wire-level trace (harness spawn, WebSocket frames, stderr):

```bash
LOUD_WIRE=1 cargo run --example repo_auditor --features antigravity
```

## Sample output

Trimmed from a real run against harness 0.1.23 (2026-11-05; paths and the
report's longer lines are cut):

```text
=== Repo Auditor (Antigravity harness) ===

Workspace: .../examples/antigravity/repo_auditor/fixture
Harness up. conversation_id=Some("...")

--- Audit in progress ---
[list_directory] file:///.../fixture
[tool] list_directory -> {"directory_path":"file:///.../fixture","results":[...]}
[pre-tool hook] denied view_file on /.../fixture/.env
[DENIED view_file] Secret files are off-limits; report them as findings instead.
[harness noise] Secret files are off-limits; ... ("denied by pre-tool hook: ...")
[view_file] file:///.../fixture/README.md
[tool] view_file -> View README.md
[list_directory] file:///.../fixture/app
[tool] list_directory -> {...}
[invoke_subagent] delegated (subagent runs its own trajectory)
[tool] invoke_subagent -> Delegate review to file_auditor
[list_directory] file:///.../fixture
[find_file] query="*"
[view_file] file:///.../fixture/.env
[view_file] file:///.../fixture/README.md
[view_file] file:///.../fixture/app/database.py
[view_file] file:///.../fixture/app/backup.py
[tool] classify_severity -> {"category":"sql_injection","severity":"critical"}
[custom tool dispatched] classify_severity
[tool] classify_severity -> {"category":"hardcoded_credentials","severity":"high"}
[custom tool dispatched] classify_severity
[tool] classify_severity -> {"category":"command_injection","severity":"critical"}
[custom tool dispatched] classify_severity
[finish] structured report received

--- Audit report ---
Repo summary: The notes-app repository is a lightweight Python application providing
note storage and user lookup backed by SQLite, along with backup and restore utilities ...

Findings (3):
  1. [CRITICAL] app/database.py — SQL Injection in User Lookup Query
     category: sql_injection | fix: Use parameterized queries ...
  2. [HIGH] app/database.py — Hardcoded Database Password in Source Code
     category: hardcoded_credentials | fix: Remove hardcoded credentials from source code ...
  3. [CRITICAL] app/backup.py — Command Injection in Backup and Restore Functions
     category: command_injection | fix: Avoid invoking shell commands with concatenated ...

Overall risk: CRITICAL
Severity cross-check: all findings match the classifier table.

Usage: prompt=Some(125240) total=Some(134775)
```

Notes from real runs:

- The harness's prompt lists the workspace root, for the subagent too;
  without that, agents guess paths and wander the filesystem.
- The pre-tool hook is handed the *model's* arguments — on 0.1.18 plain
  absolute paths under `AbsolutePath` / `DirectoryPath` / `SearchDirectory`
  / `SearchPath` — while the streamed actions carry `file:///` URIs. The
  hook denies a file tool whose path it cannot find rather than waving it
  through: this guard was silently dead for a harness release because it
  read the action-record keys (`filePath`) the hook is never given.
- A denied call never runs, so it has no action record: the harness turns
  it into an error step carrying the hook's reason, which surfaces as a
  `ToolAction::Denied` naming the tool, marked `ToolDecision::Denied` (plus
  the harness's own error event, the `[harness noise]` line), and the model
  sees the reason.
- It is expensive: on 0.1.18 a run costs a few hundred thousand prompt
  tokens (the subagent re-reads the fixture and polls subagent status),
  against roughly ten thousand on 0.1.10; the 0.1.23 run above used 125K.

## Files

| File | Purpose |
|------|---------|
| `main.rs` | Agent setup, policies, hooks, streaming loop |
| `report.rs` | Severity table, report JSON schema, parsing + cross-check |
| `fixture/` | The deliberately vulnerable sample project (fake credentials) |

## Production Considerations

- Keep auditor agents read-only — never enable `edit_file`/`run_command`
- Prefer allow-lists (`deny_all()` + `allow`): tools the harness grows later
  stay denied by default
- Encode judgment calls (severity policy) as deterministic tools so output
  can be verified mechanically
- Bound turns with `with_turn_timeout`; use `with_save_dir` +
  `conversation_id()` to resume long audits
- Pin the harness wheel (`google-antigravity==0.1.23`, see
  `antigravity::SUPPORTED_HARNESS_VERSION`)
