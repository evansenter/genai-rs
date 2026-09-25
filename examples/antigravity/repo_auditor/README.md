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
| Capabilities | `Capabilities::read_only().enable(BuiltinTool::StartSubagent)` |
| Policies | `deny_all()` + explicit `allow(...)` for each tool the audit needs |
| Pre-tool hook | Confines file tools to the workspace and denies `view_file` on `.env` paths, even though `view_file` is allowed; denies a file tool whose path it cannot read (fail closed) |
| Post-tool hook | Logs every completed tool call, custom and harness-side (audit trail) |
| Turn bound | `with_turn_timeout(Duration::from_secs(600))` |

The pre-tool hook fires *after* the policy allow — defense in depth: the
agent is told to inventory dotfiles, tries to open `fixture/.env`, and gets
denied by Rust code.

## Running

```bash
pip install google-antigravity==0.1.18   # ships the localharness binary
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

Trimmed from a real run against harness 0.1.18 (the harness's own post-tool
lines and the subagent's repeat reads are cut):

```text
=== Repo Auditor (Antigravity harness) ===

Workspace: .../examples/antigravity/repo_auditor/fixture
Harness up. conversation_id=Some("...")

--- Audit in progress ---
[list_directory] file:///.../fixture
[list_directory] file:///.../fixture/app
[view_file] file:///.../fixture/README.md
[pre-tool hook] denied view_file on /.../fixture/.env
[DENIED] Secret files are off-limits; report them as findings instead.
[harness noise] Secret files are off-limits; ... ("denied by pre-tool hook: ...")
[start_subagent] delegated (subagent runs its own trajectory)
[view_file] file:///.../fixture/app/database.py
[view_file] file:///.../fixture/app/backup.py
[tool] classify_severity -> {"category":"sql_injection","severity":"critical"}
[custom tool dispatched] classify_severity
[tool] classify_severity -> {"category":"command_injection","severity":"critical"}
[custom tool dispatched] classify_severity
[tool] classify_severity -> {"category":"hardcoded_credentials","severity":"high"}
[custom tool dispatched] classify_severity
[finish] structured report received

--- Audit report ---
Repo summary: The audited project, notes-app, is a lightweight Python note-taking
application providing database utilities ... alongside backup and restore helpers.

Findings (5):
  1. [CRITICAL] app/database.py — SQL Injection in User Query Construction
     category: sql_injection | fix: Use parameterized queries with SQLite placeholders ...
  2. [HIGH] app/database.py — Hardcoded Credential in Database Module
     category: hardcoded_credentials | fix: Remove hardcoded credentials from source ...
  3. [CRITICAL] app/backup.py — OS Command Injection in Notes Backup
     category: command_injection | fix: Avoid passing shell command strings to os.system ...
  4. [CRITICAL] app/backup.py — OS Command Injection in Notes Restore
     category: command_injection | fix: Avoid shell invocation via os.system ...
  5. [HIGH] .env — Committed Secrets in .env Configuration File
     category: hardcoded_credentials | fix: Add .env to .gitignore, untrack the file ...

Overall risk: CRITICAL
Severity cross-check: all findings match the classifier table.

Usage: prompt=Some(408606) total=Some(427009)
```

Notes from real runs:

- The workspace root is announced to the model (and appended to the
  subagent's instructions) automatically; without that, agents guess paths
  and wander the filesystem.
- The pre-tool hook is handed the *model's* arguments — on 0.1.18 plain
  absolute paths under `AbsolutePath` / `DirectoryPath` / `SearchDirectory`
  / `SearchPath` — while the streamed actions carry `file:///` URIs. The
  hook denies a file tool whose path it cannot find rather than waving it
  through: this guard was silently dead for a harness release because it
  read the action-record keys (`filePath`) the hook is never given.
- A denied call never runs, so it has no action record: the harness turns
  it into an error step carrying the hook's reason, which surfaces as a
  `ToolAction::Error` marked `ToolDecision::Denied` (plus the harness's own
  error event), and the model sees the reason.
- It is expensive: on 0.1.18 a run costs a few hundred thousand prompt
  tokens (the subagent re-reads the fixture and polls subagent status),
  against roughly ten thousand on 0.1.10.

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
- Pin the harness wheel (`google-antigravity==0.1.18`, see
  `antigravity::SUPPORTED_HARNESS_VERSION`)
