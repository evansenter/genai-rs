#!/usr/bin/env python3
"""Capture real API responses as offline fixtures (#492).

Every offline response the tests replay used to be hand-written, so a
shape the API actually sends could drift away from what the crate parses
without an offline test noticing. This records a few real exchanges into
`tests/fixtures/captured/`, each stamped with its capture date, and
`tests/captured_fixtures.rs` replays them through the client against the
local stub. The weekly run re-captures them and runs the replay against
the fresh copies (rust.yml, `captured-fixtures`), so a change in what the
API sends fails there and the new captures are kept as an artifact to
commit.

Usage: capture_fixtures.py [--out DIR] [--model MODEL]

Needs GEMINI_API_KEY. The model defaults to `DEFAULT_MODEL` in
src/lib.rs. Deletes what it creates (two stored interactions and an
environment marked for the test-resource janitor).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

BASE = "https://generativelanguage.googleapis.com"
API_REVISION = "2026-05-20"


class Recorder:
    def __init__(self, key: str, out: str):
        self.key = key
        self.out = out
        self.today = dt.datetime.now(dt.timezone.utc).date().isoformat()

    def call(self, method: str, path: str, body=None):
        headers = {"x-goog-api-key": self.key, "Api-Revision": API_REVISION}
        data = None
        if body is not None:
            headers["Content-Type"] = "application/json"
            data = json.dumps(body).encode()
        request = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
        for attempt in range(4):
            try:
                with urllib.request.urlopen(request, timeout=300) as response:
                    return response.status, response.headers.get("Content-Type", ""), response.read().decode()
            except urllib.error.HTTPError as e:
                status, ctype, text = e.code, e.headers.get("Content-Type", ""), e.read().decode()
                if status in (429, 503) and attempt < 3:
                    time.sleep(2 ** (attempt + 1))
                    continue
                return status, ctype, text
        raise AssertionError("unreachable")

    def record(self, name: str, note: str, method: str, path: str, body=None, expect=200):
        status, ctype, text = self.call(method, path, body)
        if status != expect:
            sys.exit(f"{name}: expected HTTP {expect}, got {status}: {text[:300]}")
        response = {"status": status, "content_type": ctype.split(";")[0].strip()}
        if response["content_type"] == "text/event-stream":
            response["sse"] = text
        else:
            response["json"] = json.loads(text) if text.strip() else {}
        fixture = {
            "captured": self.today,
            "note": note,
            "request": {"method": method, "path": path, **({"body": body} if body is not None else {})},
            "response": response,
        }
        with open(os.path.join(self.out, f"{name}.json"), "w", encoding="utf-8") as handle:
            json.dump(fixture, handle, indent=2, ensure_ascii=False)
            handle.write("\n")
        print(f"captured {name}: HTTP {status} {response['content_type']}")
        return response.get("json")


def default_model() -> str:
    text = open("src/lib.rs", encoding="utf-8").read()
    match = re.search(r'pub const DEFAULT_MODEL: &str = "([^"]+)"', text)
    if not match:
        sys.exit("DEFAULT_MODEL not found in src/lib.rs; pass --model")
    return match.group(1)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", default="tests/fixtures/captured")
    parser.add_argument("--model", default=None)
    args = parser.parse_args()
    key = os.environ.get("GEMINI_API_KEY", "").strip()
    if not key:
        sys.exit("GEMINI_API_KEY is empty or unset")
    model = args.model or default_model()
    os.makedirs(args.out, exist_ok=True)
    rec = Recorder(key, args.out)
    cleanup: list[str] = []

    try:
        rec.record(
            "create_stream",
            "A streamed text answer, store off: interaction.created, step deltas, "
            "interaction.completed without `steps`, then done.",
            "POST", "/v1beta/interactions?alt=sse",
            {"model": model, "input": "Count from 1 to 5, one number per line.", "stream": True, "store": False},
        )

        created = rec.record(
            "create_interaction",
            "A stored, non-streamed text answer.",
            "POST", "/v1beta/interactions",
            {"model": model, "input": "Name three primary colors, comma-separated."},
        )
        cleanup.append(f"/v1beta/interactions/{created['id']}")
        rec.record(
            "get_interaction",
            "The stored interaction above, read back.",
            "GET", f"/v1beta/interactions/{created['id']}",
        )

        background = rec.call(
            "POST", "/v1beta/interactions",
            {"model": model, "input": "Count from 1 to 3.", "background": True},
        )
        bg = json.loads(background[2])
        cleanup.append(f"/v1beta/interactions/{bg['id']}")
        for _ in range(30):
            if json.loads(rec.call("GET", f"/v1beta/interactions/{bg['id']}")[2]).get("status") != "in_progress":
                break
            time.sleep(2)
        rec.record(
            "retrieve_stream",
            "A finished background interaction streamed back from storage "
            "(interactions().stream).",
            "GET", f"/v1beta/interactions/{bg['id']}?alt=sse&stream=true",
        )

        env = rec.call(
            "POST", "/v1beta/environments",
            {"sources": [{"type": "inline", "target": "/etc/motd", "content": "genai-rs-test captured fixture"}]},
        )
        env_id = json.loads(env[2])["id"]
        cleanup.append(f"/v1beta/environments/{env_id}")
        rec.record(
            "get_environment",
            "An environment with one inline source, read back.",
            "GET", f"/v1beta/environments/{env_id}",
        )

        rec.record(
            "list_voices",
            "The first page of the prebuilt voice catalog.",
            "GET", "/v1beta/voices?page_size=3",
        )

        rec.record(
            "error_unknown_parameter",
            "A create with a field the API does not know: the 400 envelope.",
            "POST", "/v1beta/interactions",
            {"model": model, "input": "hi", "store": False, "genai_rs_test_unknown": 1},
            expect=400,
        )
        # A malformed id is a 400 ("Invalid interaction name", 2026-11-05);
        # a deleted one is the 404.
        gone = f"/v1beta/interactions/{created['id']}"
        rec.call("DELETE", gone)
        cleanup.remove(gone)
        rec.record(
            "error_not_found",
            "A read of the interaction above after deleting it: the 404 envelope.",
            "GET", gone,
            expect=404,
        )
    finally:
        for path in cleanup:
            status = rec.call("DELETE", path)[0]
            if status not in (200, 404):
                print(f"cleanup: DELETE {path} answered {status}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
