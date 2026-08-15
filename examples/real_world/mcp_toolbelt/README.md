# MCP Toolbelt Example

Giving an agent tools it didn't ship with, over the
[Model Context Protocol](https://modelcontextprotocol.io).

## Overview

Unlike `#[tool]` functions — which *your* process executes — MCP tools run
in the server's process and the harness brokers the call. That makes MCP
the way to reach a tool you didn't write: a git server, a database bridge,
an internal API wrapper.

This example spawns `widgets_server.py` (next to this file), a
dependency-free stdio MCP server whose answers are values no model could
guess. A correct response is therefore evidence of a real round trip
rather than of the model improvising.

## What it covers that the other examples don't

| Surface | Why it matters |
|---------|----------------|
| `add_mcp_server` (stdio) | The whole external-tool path; nothing else exercises it |
| `McpServer::with_name` | The name is the `<server>` half of the policy target, not cosmetic |
| `mcp_<server>_<tool>` naming | What a policy or `on_pre_tool` hook must match on — the usual reason an MCP policy silently never applies |
| `Capabilities::none()` + MCP | MCP is configured independently of the builtin set, so an agent can have *only* external tools |

## Running

```bash
pip install google-antigravity==0.1.10   # or set ANTIGRAVITY_HARNESS_PATH
export GEMINI_API_KEY=...
cargo run --example mcp_toolbelt --features antigravity

# Just the MCP traffic, one line per message:
LOUD_WIRE=mcpTool,summary cargo run --example mcp_toolbelt --features antigravity
```

## Swapping in a real server

The fixture stands in for something like:

```rust,ignore
McpServer::stdio("uvx", ["mcp-server-git"]).with_name("git")
// or
McpServer::http("http://localhost:8931/mcp").with_name("api")
```

Both forms take `with_enabled_tools` / `with_disabled_tools` to narrow the
surface, and `with_timeout_seconds` to bound a slow server.

## Gotchas

- **The server name drives the policy target.** Omit `with_name` and it
  defaults to the command's basename (`python3` here), so a policy naming
  `mcp_widgets_*` would never match.
- **MCP tools run in the server's process** — the server's filesystem,
  network and credentials, not your agent's. Capability gating does not
  contain them.
- **Tool errors come back as results, not transport failures.** A server
  returning `isError` surfaces to the model, which can adapt.
- **Stdio servers are spawned per agent**, so a slow-starting server
  delays `spawn()`.
