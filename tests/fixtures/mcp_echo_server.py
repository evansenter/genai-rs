#!/usr/bin/env python3
"""A minimal stdio MCP server, used by `tests/antigravity_harness.rs`.

Exposes exactly one tool, `lookup_widget_code`, whose answer is a token
that exists nowhere else — so a response containing it can only have come
from a real MCP round trip through the harness, not from the model.

Deliberately hand-rolled JSON-RPC over stdio rather than a dependency:
the test fixture must run on whatever `python3` the harness tests already
require, with nothing installed.
"""

import json
import sys

PROTOCOL_VERSION = "2024-11-05"

TOOLS = [
    {
        "name": "lookup_widget_code",
        "description": (
            "Look up the internal code for a widget. This is the only way to "
            "obtain a widget code; it cannot be guessed or derived."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "widget": {
                    "type": "string",
                    "description": "The widget name to look up.",
                }
            },
            "required": ["widget"],
        },
    }
]

# The marker the test greps for. Arbitrary on purpose.
#
# Duplicated as WIDGET_CODE in test_antigravity_mcp_server_tool_is_called
# (tests/antigravity_harness.rs) — change both together. Getting it wrong
# fails loudly rather than silently: the assertion prints the expected
# token alongside the model's response.
WIDGET_CODE = "wibble-3317-quux"


def respond(msg_id, result):
    return {"jsonrpc": "2.0", "id": msg_id, "result": result}


def error(msg_id, code, message):
    return {
        "jsonrpc": "2.0",
        "id": msg_id,
        "error": {"code": code, "message": message},
    }


def handle(request):
    """Returns a response dict, or None for notifications."""
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
        params = request.get("params") or {}
        if params.get("name") != "lookup_widget_code":
            return error(msg_id, -32602, f"unknown tool: {params.get('name')!r}")
        widget = (params.get("arguments") or {}).get("widget", "unnamed")
        return respond(
            msg_id,
            {
                "content": [
                    {
                        "type": "text",
                        "text": f"The code for widget {widget!r} is {WIDGET_CODE}.",
                    }
                ],
                "isError": False,
            },
        )
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
