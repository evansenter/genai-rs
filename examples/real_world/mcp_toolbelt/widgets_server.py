#!/usr/bin/env python3
"""A minimal stdio MCP server, spawned by the `mcp_toolbelt` example.

Real deployments point `McpServer::stdio` at something like `uvx
mcp-server-git`. This stands in for that: a dependency-free server whose
answers are values the model provably cannot know, so a correct response
is evidence of a real MCP round trip rather than of the model guessing.

Speaks JSON-RPC 2.0 over stdin/stdout, one message per line:
`initialize` -> `tools/list` -> `tools/call`.
"""

import json
import sys

PROTOCOL_VERSION = "2024-11-05"

# The inventory. Deliberately arbitrary — nothing in a model's training
# could supply these, so an answer containing them came from this process.
INVENTORY = {
    "flange": {"code": "wibble-3317-quux", "in_stock": 42},
    "grommet": {"code": "zorble-8891-frob", "in_stock": 0},
    "sprocket": {"code": "quaffle-2205-nurb", "in_stock": 7},
}

TOOLS = [
    {
        "name": "lookup_widget",
        "description": (
            "Look up a widget's internal code and stock level. This is the "
            "only source for widget codes; they cannot be guessed."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "widget": {
                    "type": "string",
                    "description": "Widget name, e.g. 'flange'.",
                }
            },
            "required": ["widget"],
        },
    },
    {
        "name": "list_widgets",
        "description": "List every widget name this server knows about.",
        "inputSchema": {"type": "object", "properties": {}},
    },
]


def respond(msg_id, result):
    return {"jsonrpc": "2.0", "id": msg_id, "result": result}


def error(msg_id, code, message):
    return {"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}}


def text_result(text, is_error=False):
    return {"content": [{"type": "text", "text": text}], "isError": is_error}


def call_tool(params):
    name = params.get("name")
    args = params.get("arguments") or {}

    if name == "list_widgets":
        return text_result("Known widgets: " + ", ".join(sorted(INVENTORY)))

    if name == "lookup_widget":
        widget = args.get("widget", "")
        entry = INVENTORY.get(widget.lower().strip())
        if entry is None:
            # A tool-level failure, not a protocol error: the model should
            # see this and adapt rather than the call blowing up.
            return text_result(f"No widget named {widget!r}.", is_error=True)
        return text_result(
            f"Widget {widget!r}: code {entry['code']}, {entry['in_stock']} in stock."
        )

    return None


def handle(request):
    """Returns a response dict, or None for notifications (no id)."""
    method = request.get("method")
    msg_id = request.get("id")

    # Notifications carry no id and must not be answered.
    if msg_id is None:
        return None

    if method == "initialize":
        return respond(
            msg_id,
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "widgets", "version": "0.0.1"},
            },
        )
    if method == "tools/list":
        return respond(msg_id, {"tools": TOOLS})
    if method == "tools/call":
        result = call_tool(request.get("params") or {})
        if result is None:
            return error(msg_id, -32602, f"unknown tool: {(request.get('params') or {}).get('name')!r}")
        return respond(msg_id, result)
    if method in ("ping", "shutdown"):
        return respond(msg_id, {})

    return error(msg_id, -32601, f"method not found: {method}")


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            continue
        try:
            response = handle(request)
        except Exception as exc:  # noqa: BLE001 - a fixture must not die silently
            response = error(request.get("id"), -32603, f"internal error: {exc!r}")
        if response is not None:
            sys.stdout.write(json.dumps(response) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
