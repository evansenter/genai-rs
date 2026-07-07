# Example Projects

Ideas for real applications built on `genai-rs`, chosen to exercise the
library's two modes — the **Interactions API** (direct model inference) and
the **Antigravity harness** (local agentic runtime) — separately and
together. Each entry lists the features it exercises so these double as
end-to-end validation targets. Candidates for `examples/real_world/` are
marked.

## 1. `repo-auditor` — agentic codebase review *(BUILT: `examples/real_world/repo_auditor/`)*

An Antigravity agent pointed at a repository workspace with read-only
built-in tools, plus a custom `#[tool]` severity classifier. Walks the code,
investigates hot spots with grep/view tools, and emits a structured audit
report (JSON schema via structured output).

- Mode: Antigravity (local).
- Exercises: workspaces, built-in read-only tools, custom Rust tools via
  `#[tool]`, `deny_all` + selective allow policies, structured output,
  step streaming (live progress UI).
- **Status: built** — see `examples/real_world/repo_auditor/`. The built
  version also registers a `file_auditor` subagent (`add_subagent` +
  `BuiltinTool::StartSubagent`) and a pre-tool hook guarding `.env` files.

## 2. `release-notes-bot` — dual-mode pipeline *(real_world candidate)*

An Antigravity agent does the git archaeology (diffs, blame, PR titles via
`run_command`), then the Interactions API summarizes and rewrites for a
public changelog with `service_tier: flex` for cost control. Shows one tool
set (`#[tool]` functions) registered in both modes.

- Modes: both, sharing tools.
- Exercises: run_command policy confirmation, tool reuse across modes,
  Interactions structured output, service tiers, explicit caching
  (`cached_content`) for the style-guide preamble.

## 3. `doc-gardener` — trigger-driven maintenance daemon

A long-running agent with a file-change trigger on `docs/`: when sources
change, it re-checks cross-references, updates stale snippets, and opens a
diff for approval (pre-tool hook gates writes to an "ask user" policy).

- Mode: Antigravity (local).
- Exercises: triggers, hooks (pre-tool decide), `workspace_only` policy,
  session persistence/resume (`save_dir` + `conversation_id`).

## 4. `support-triage` — MCP + subagents

Connects an MCP server for the ticket system (stdio transport), spawns
subagents to investigate independent tickets in parallel, and posts
dispositions. Audit logging via post-tool hooks.

- Mode: Antigravity (local).
- Exercises: MCP stdio servers, subagent configs, post-tool hooks,
  usage-metadata accounting per trajectory.

## 5. `transcribe-and-brief` — multimodal inference pipeline

Pure Interactions API: audio meeting recordings in, per-speaker TTS brief
out. Uses multimodal input, multi-speaker `speech_config`, typed
`response_format` list (text summary + audio rendition), and background mode
with webhooks for long files.

- Mode: Interactions.
- Exercises: audio input, multi-speaker TTS, typed response formats with
  `delivery: uri`, background + webhook delivery, resumable streaming.

## 6. `grounded-research-cli` — deep research with enterprise grounding

CLI wrapping the deep-research agent with the `retrieval` tool (Vertex AI
Search / RAG store) and Google Search, streaming thought summaries, saving
citation-annotated markdown (typed `url_citation`/`file_citation`
annotations).

- Mode: Interactions (hosted agent).
- Exercises: `agent` + `agent_config` (visualization, collaborative
  planning), retrieval tool, citation annotations, `budget_exceeded`
  handling, cancel/resume.

## 7. `hosted-vs-local` — the Antigravity comparison demo

The same task ("fix the failing test in this repo") run twice: once via the
hosted `agent: "antigravity-preview-05-2026"` with an Environment
(repository source + network allowlist), once via the local harness with a
workspace. Prints a side-by-side step trace using the shared wire-inspection
layer.

- Modes: both (the flagship demo of the dual-mode story).
- Exercises: Environments + Agents resources, hosted agent steps vs local
  harness steps, `WireInspector` capture for the side-by-side.
