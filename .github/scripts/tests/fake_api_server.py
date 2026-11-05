#!/usr/bin/env python3
"""A local stand-in for the Gemini API that replays recorded responses.

For the janitor harness (`test_cleanup_test_resources.sh`). Serves on
127.0.0.1 at a free port, writes the port to `<port-file>` once listening,
and logs every request as `METHOD /path?query` to `<request-log>`.

Usage: fake_api_server.py <routes.json> <fixture-dir> <port-file> <request-log>

`routes.json` maps `METHOD /path?query` (the query's keys sorted) to a
response: a fixture file name (200), an object `{"status": 500, "body":
"file-or-JSON", "headers": {...}}`, or a list of those served in turn, the
last one repeating. A DELETE with no route answers 200 `{}`. Anything else
unrouted answers 404 naming the request, so a harness case fails loudly
instead of reading an empty listing.
"""

import json
import os
import socketserver
import sys
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

routes_path, fixture_dir, port_file, request_log = sys.argv[1:5]
with open(routes_path, encoding="utf-8") as handle:
    ROUTES = json.load(handle)
SERVED = {}
LOCK = threading.Lock()


def canonical(method: str, raw_path: str) -> str:
    parsed = urllib.parse.urlsplit(raw_path)
    query = sorted(urllib.parse.parse_qsl(parsed.query, keep_blank_values=True))
    key = f"{method} {parsed.path}"
    return key + ("?" + urllib.parse.urlencode(query) if query else "")


def resolve(spec):
    if isinstance(spec, str):
        spec = {"body": spec}
    body = spec.get("body", {})
    if isinstance(body, str):
        with open(f"{fixture_dir}/{body}", encoding="utf-8") as handle:
            body = json.load(handle)
    return spec.get("status", 200), body, spec.get("headers", {})


class Handler(BaseHTTPRequestHandler):
    def handle_any(self):
        key = canonical(self.command, self.path)
        with LOCK:
            with open(request_log, "a", encoding="utf-8") as log:
                log.write(key + "\n")
            spec = ROUTES.get(key)
            if isinstance(spec, list):
                n = SERVED.get(key, 0)
                SERVED[key] = n + 1
                spec = spec[min(n, len(spec) - 1)]
        if spec is None:
            spec = {"body": {}} if self.command == "DELETE" else {
                "status": 404, "body": {"error": {"message": f"no route for {key}"}}}
        status, body, headers = resolve(spec)
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        for name, value in headers.items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(payload)

    do_GET = do_DELETE = do_POST = do_PATCH = handle_any

    def log_message(self, *args):
        pass


class Server(ThreadingHTTPServer):
    # HTTPServer.server_bind resolves the host's FQDN, which stalls for
    # seconds where DNS is slow or blocked; the name is never used here.
    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


server = Server(("127.0.0.1", 0), Handler)
with open(port_file + ".tmp", "w", encoding="utf-8") as handle:
    handle.write(str(server.server_address[1]))
# Renamed into place, so the harness never reads a half-written port.
os.replace(port_file + ".tmp", port_file)
server.serve_forever()
