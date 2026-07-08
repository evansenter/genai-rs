# Repo Auditor Example

Agentic codebase security review running on the local
[Antigravity harness](../../../docs/ANTIGRAVITY.md) — the `real_world`
demo for the crate's agent mode (everything else in this directory uses the
Interactions API).

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
| Capabilities | `Capabilities::read_only().enable(BuiltinTool::StartSubagent)` |
| Policies | `deny_all()` + explicit `allow(...)` for each tool the audit needs |
| Pre-tool hook | Denies `view_file` on `.env` paths, even though `view_file` is allowed |
| Post-tool hook | Logs every custom-tool execution (audit trail) |
| Turn bound | `with_turn_timeout(Duration::from_secs(600))` |

The pre-tool hook fires *after* the policy allow — defense in depth: the
agent is told to inventory dotfiles, tries to open `fixture/.env`, and gets
denied by Rust code.

## Running

```bash
pip install google-antigravity==0.1.5   # ships the localharness binary
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

Trimmed from a real run:

```text
=== Repo Auditor (Antigravity harness) ===

Workspace: .../examples/real_world/repo_auditor/fixture
Harness up. conversation_id=Some("9d772d7c9085062190aefd6723785b77")

--- Audit in progress ---
[list_directory] file:///.../fixture
[list_directory] file:///.../fixture/app
[pre-tool hook] denied view_file on /.../fixture/.env
[start_subagent] delegated (subagent runs its own trajectory)
[view_file] file:///.../fixture/app/backup.py
[view_file] file:///.../fixture/app/database.py
[tool] classify_severity -> {"result":"{\"category\":\"command_injection\",\"severity\":\"critical\"}"}
[custom tool dispatched] classify_severity
[tool] classify_severity -> {"result":"{\"category\":\"hardcoded_credentials\",\"severity\":\"high\"}"}
[custom tool dispatched] classify_severity
[tool] classify_severity -> {"result":"{\"category\":\"sql_injection\",\"severity\":\"critical\"}"}
[custom tool dispatched] classify_severity
[search_directory] query="password|secret|key|token|auth|db_password"
[finish] structured report received

--- Audit report ---
Repo summary: The project is a deliberately flawed notes-app fixture designed for
testing security auditing tools. It contains multiple high-severity vulnerabilities
including SQL injection, command injection, and hardcoded credentials.

Findings (3):
  1. [CRITICAL] app/database.py — SQL Injection in find_user function
     category: sql_injection | fix: Use parameterized queries instead of string
     formatting to prevent SQL injection. ...
  2. [HIGH] app/database.py — Hardcoded DB Password in database.py
     category: hardcoded_credentials | fix: Remove hardcoded passwords from the
     source code. Use environment variables or a secret management system. ...
  3. [CRITICAL] app/backup.py — Command Injection in backup_notes and restore_notes
     category: command_injection | fix: Avoid using `os.system` with user-controlled
     input. Use the `subprocess` module with argument lists. ...

Overall risk: CRITICAL
Severity cross-check: all findings match the classifier table.

Usage: prompt=Some(9425) total=Some(9725)
```

Notes from real runs:

- The harness passes workspaces to its tools but does not announce the path
  to the model — name the workspace root in the task prompt (and in the
  subagent's instructions, since its trajectory starts fresh) or the agent
  wanders the filesystem.
- Harness action paths arrive as `file:///` URIs; the pre-tool hook strips
  the scheme before comparing against the workspace root.
- A denied action still surfaces as a `ToolAction` event (the harness echoes
  the rejected step); the wire shows `"accepted": false` and the model sees
  "User denied permission for tool call."

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
- Pin the harness wheel (`google-antigravity==0.1.5`, see
  `antigravity::SUPPORTED_HARNESS_VERSION`)
