#!/usr/bin/env python3
"""Delete the resources genai-rs's live tests leaked on the shared test key.

A live test deletes what it creates, but a test that dies first (a timeout,
a panic before its guard, a cancelled job) leaves it behind, and apart from
uploaded files nothing it creates expires on its own: on 2026-11-03 the key
held 99 environments, all test-created, some from July (#492). This runs on a schedule
(`test-resource-janitor.yml`) and deletes only resources that carry a live
test's marker, and only once they are older than `--min-age-hours`, so a
run never pulls a resource out from under a test still using it.

The markers, by kind (`tests/common/mod.rs`, `test_resource_name`):

- credentials, agents: an `id` starting `genai-rs-test`.
- webhooks: the test endpoint `https://example.com/genai-rs-test...` as
  `uri`, or a `name` starting `genai-rs-test`.
- triggers, file search stores, prompted voices: a display name starting
  `genai-rs-test`.
- environments: every source inline and a fixture: content starting
  `genai-rs-test`, a target under `genai_rs_test/`, or one of the /etc/motd
  strings the tests used before they carried the prefix
  (`LEGACY_ENV_SOURCES`). Or no sources and no files: see below.

Environments have no field a test can mark. The resource is `id`,
`sources`, `network`, `status`, `file_count`, `size_bytes` and three
timestamps, with no name, display name, label or metadata (google-genai
2.34.0's `Environment` and `CreateEnvironmentRequest`, and the live listing
of 2026-11-06), so only inline sources can carry a marker. An environment
with no sources cannot be told apart from one a person made: an agent
interaction run without an environment creates one, a fork
(`from_environment`) lists none (both verified 2026-11-05), and so does a
create with only a network. The janitor takes such an environment only
when it also holds no files (`file_count` and `size_bytes` zero or absent)
and has not been used within the age floor: it then has nothing in it to
lose. That still matches an empty sourceless environment that is not a
test's, so opting in deletes those too. A sourceless environment that holds
files, a leaked fork for one, is kept and listed for a person to check.
On 2026-11-06 the shared key held 35 sourceless environments, all empty
(34 from July, one from October).

Age comes from the resource's create time, and for an environment from the
latest of its create, update and last-access times. Agents have no create
time, and prompted voices only an `expire_time` (a year after creation,
2026-11-05), so for them it is read from the Unix timestamp
`test_resource_name` embeds
(`genai-rs-test-<label>-<unix seconds>-<nanos>`), else from the voice's
`expire_time`. A marked resource whose age cannot be told is kept and
reported.

Not covered: Files expire 48 hours after upload (`expirationTime`,
2026-11-05), and stored interactions have no list endpoint; a test that
starts a background interaction cancels and deletes it itself.

Usage:
  cleanup_test_resources.py --dry-run | --delete | --scheduled
                            [--min-age-hours N] [--kinds a,b] [--summary FILE]

`--dry-run` lists what would be deleted and deletes nothing. `--scheduled`
is what the daily workflow passes: a dry run unless the environment
variable `JANITOR_DELETE` is `true` (any case), when it deletes. The
workflow sets it from the repository variable of that name, so deleting on
a schedule is the key owner's opt-in: the first run against the shared key
would remove 112 environments (77 holding only fixture sources, 35 with no
sources and no files) and 3 credentials (dry run, 2026-11-06).
`--summary` appends a Markdown report to FILE (the job summary): the
mode, every resource deleted or that would be, the marked resources whose
age is unknown, the unmarked ones kept for a person to check (sourceless
environments that hold files), and the per-kind table. A delete
that answers 404 counts as done: the resource is already gone (a test's own
cleanup got there first, or a retried delete whose first try went through).
Credentials and webhooks answer a delete of a missing id with 404;
environments, agents and triggers with 200 (2026-11-05).
Needs GEMINI_API_KEY. Exits 1 when a list or a delete failed, 2 on a usage
error.
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
import urllib.parse
import urllib.request

DEFAULT_BASE_URL = "https://generativelanguage.googleapis.com"
API_REVISION = "2026-05-20"
PREFIX = "genai-rs-test"
TEST_WEBHOOK_URI_PREFIX = "https://example.com/genai-rs-test"
# Legacy trigger display names, from before the tests used the prefix.
LEGACY_TRIGGER_PREFIXES = ("genai-rs trigger test",)
# Inline sources the live tests used before 2026-11-05, when every fixture
# string took the prefix. "hello from genai-rs" and "probe" are on the 77
# environments leaked July to October; the rest were current until then.
LEGACY_ENV_SOURCES = {
    ("/etc/motd", "hello from genai-rs"),
    ("/etc/motd", "probe"),
    ("/etc/motd", "config probe"),
    ("/etc/motd", "hello"),
    ("/etc/motd", "hello from genai-rs environments CRUD"),
    ("/paging/a.txt", "a"),
    ("/paging/b.txt", "b"),
    ("/paging/c.txt", "c"),
}
# `test_resource_name`'s embedded creation time.
EMBEDDED_TIME = re.compile(r"-(\d{10})-\d+$")
# A prompted voice's expire_time is its creation plus a year (2026-11-05).
VOICE_LIFETIME = dt.timedelta(days=365)
MAX_ATTEMPTS = 4
# The repository variable that lets a scheduled run delete.
DELETE_OPT_IN = "JANITOR_DELETE"


class ApiError(Exception):
    def __init__(self, message: str, status: int | None = None):
        super().__init__(message)
        self.status = status


class Api:
    def __init__(self, base_url: str, key: str):
        self.base = base_url.rstrip("/")
        self.headers = {"x-goog-api-key": key, "Api-Revision": API_REVISION}

    def call(self, method: str, path: str, query: dict | None = None) -> dict:
        url = f"{self.base}{path}"
        if query:
            url += "?" + urllib.parse.urlencode(query)
        for attempt in range(1, MAX_ATTEMPTS + 1):
            request = urllib.request.Request(url, method=method, headers=self.headers)
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    body = response.read()
                return json.loads(body) if body.strip() else {}
            except urllib.error.HTTPError as e:
                detail = e.read()[:300].decode("utf-8", "replace")
                retryable = e.code == 429 or e.code >= 500
                if retryable and attempt < MAX_ATTEMPTS:
                    time.sleep(retry_delay(e.headers.get("Retry-After"), attempt))
                    continue
                raise ApiError(f"{method} {path}: HTTP {e.code}: {detail}", e.code) from None
            except (urllib.error.URLError, TimeoutError) as e:
                if attempt < MAX_ATTEMPTS:
                    time.sleep(retry_delay(None, attempt))
                    continue
                raise ApiError(f"{method} {path}: {e}") from None
        raise AssertionError("unreachable")


def retry_delay(retry_after: str | None, attempt: int) -> float:
    if retry_after and retry_after.strip().isdigit():
        return min(float(retry_after), 60.0)
    return float(min(2**attempt, 30)) * float(os.environ.get("JANITOR_BACKOFF_SCALE", "1"))


def parse_time(value) -> dt.datetime | None:
    if not isinstance(value, str) or not value:
        return None
    text = value.strip().replace("Z", "+00:00")
    # Python before 3.11 takes at most six fractional digits; the API sends
    # nine.
    text = re.sub(r"(\.\d{6})\d+", r"\1", text)
    try:
        parsed = dt.datetime.fromisoformat(text)
    except ValueError:
        return None
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=dt.timezone.utc)


def create_time(resource: dict) -> dt.datetime | None:
    """The create time, in any of the spellings the resources use: triggers
    send `create_time` (2026-11-01) where environments send `created`, and
    the camelCase resources `createTime`."""
    for field in ("create_time", "created", "createTime"):
        parsed = parse_time(resource.get(field))
        if parsed:
            return parsed
    return None


def embedded_time(name) -> dt.datetime | None:
    match = EMBEDDED_TIME.search(name or "")
    if not match:
        return None
    return dt.datetime.fromtimestamp(int(match.group(1)), tz=dt.timezone.utc)


def is_marked(text) -> bool:
    return isinstance(text, str) and text.startswith(PREFIX)


def is_fixture_source(source: dict) -> bool:
    if source.get("type") != "inline":
        return False
    target = source.get("target") or ""
    content = source.get("content") or ""
    return (
        content.startswith(PREFIX)
        or target.startswith("genai_rs_test/")
        or "/genai_rs_test/" in target
        or (target, content) in LEGACY_ENV_SOURCES
    )


# Each kind: how to list it, how to name one, why it is a test's, when it
# was created, and how to delete it. `marker` returns a reason, or None for
# a resource that is not a test's.
def count(value) -> int:
    """An int64 the API may send as a string, a number, or (when zero) not
    at all."""
    try:
        return int(value or 0)
    except (TypeError, ValueError):
        # Unreadable: count it as something there, so nothing is assumed
        # empty.
        return 1


def holds_files(env: dict) -> bool:
    return count(env.get("file_count")) > 0 or count(env.get("size_bytes")) > 0


def environment_marker(env: dict):
    sources = env.get("sources") or []
    if not sources:
        # No field marks a test's sourceless environment (see the module
        # docs), so only an empty one is taken: it holds nothing to lose.
        if holds_files(env):
            return None
        return "no sources and no files"
    if all(is_fixture_source(s) for s in sources):
        return "only test fixture sources"
    return None


def environment_review(env: dict):
    """A note for an unmarked environment a person should look at: one with
    no sources that holds files, as a leaked fork does. It is kept."""
    if env.get("sources") or not holds_files(env):
        return None
    return (f"no sources, but it holds {count(env.get('file_count'))} file(s): "
            "a fork or an agent's workspace, a test's or a person's")


def last_active(env: dict) -> dt.datetime | None:
    """When an environment was last created, updated or used: a test's is
    never touched after its test, so recent use means someone has it."""
    created = create_time(env)
    if created is None:
        return None
    times = [created] + [t for t in (parse_time(env.get("updated")),
                                     parse_time(env.get("last_accessed"))) if t]
    return max(times)


def webhook_marker(hook: dict):
    if str(hook.get("uri") or "").startswith(TEST_WEBHOOK_URI_PREFIX):
        return "the test webhook endpoint"
    if is_marked(hook.get("name")):
        return "a genai-rs-test name"
    return None


def trigger_marker(trigger: dict):
    name = trigger.get("display_name") or ""
    if is_marked(name):
        return "a genai-rs-test display name"
    if name.startswith(LEGACY_TRIGGER_PREFIXES):
        return "a legacy test display name"
    return None


def voice_created(voice: dict):
    created = embedded_time(voice.get("display_name"))
    if created:
        return created
    expires = parse_time(voice.get("expire_time"))
    return expires - VOICE_LIFETIME if expires else None


KINDS = {
    "environments": {
        "list": [("/v1beta/environments", {})],
        "items": "environments",
        "page_size": ("page_size", 100),
        "token": ("page_token", "next_page_token"),
        "id": lambda r: r.get("id"),
        "marker": environment_marker,
        "review": environment_review,
        "created": last_active,
        "age_label": "last active",
        "delete": lambda r: (f"/v1beta/environments/{r['id']}", {}),
    },
    "credentials": {
        "list": [("/v1beta/credentials", {})],
        "items": "credentials",
        "page_size": ("page_size", 100),
        "token": ("page_token", "next_page_token"),
        "id": lambda r: r.get("id"),
        "marker": lambda r: "a genai-rs-test id" if is_marked(r.get("id")) else None,
        "created": create_time,
        "delete": lambda r: (f"/v1beta/credentials/{r['id']}", {}),
    },
    "webhooks": {
        "list": [("/v1beta/webhooks", {})],
        "items": "webhooks",
        "page_size": ("page_size", 100),
        "token": ("page_token", "next_page_token"),
        "id": lambda r: r.get("id"),
        "marker": webhook_marker,
        "created": create_time,
        "delete": lambda r: (f"/v1beta/webhooks/{r['id']}", {}),
    },
    "agents": {
        "list": [("/v1beta/agents", {})],
        "items": "agents",
        "page_size": ("page_size", 100),
        "token": ("page_token", "next_page_token"),
        "id": lambda r: r.get("id"),
        "marker": lambda r: "a genai-rs-test id" if is_marked(r.get("id")) else None,
        "created": lambda r: embedded_time(r.get("id")),
        "delete": lambda r: (f"/v1beta/agents/{r['id']}", {}),
    },
    "triggers": {
        "list": [("/v1beta/triggers", {})],
        "items": "triggers",
        "page_size": ("page_size", 100),
        "token": ("page_token", "next_page_token"),
        "id": lambda r: r.get("id"),
        "marker": trigger_marker,
        "created": create_time,
        "delete": lambda r: (f"/v1beta/triggers/{r['id']}", {}),
    },
    # camelCase, and at most 20 a page (2026-11-05).
    "fileSearchStores": {
        "list": [("/v1beta/fileSearchStores", {})],
        "items": "fileSearchStores",
        "page_size": ("pageSize", 20),
        "token": ("pageToken", "nextPageToken"),
        "id": lambda r: r.get("name"),
        "marker": lambda r: "a genai-rs-test display name" if is_marked(r.get("displayName")) else None,
        "created": create_time,
        # A store that still holds documents needs force.
        "delete": lambda r: (f"/v1beta/{r['name']}", {"force": "true"}),
    },
    # The default listing is the prebuilt catalog; custom voices are listed
    # by type.
    "voices": {
        "list": [("/v1beta/voices", {"type": "prompted"}), ("/v1beta/voices", {"type": "replicated"})],
        "items": "voices",
        "page_size": ("page_size", 50),
        "token": ("page_token", "next_page_token"),
        "id": lambda r: r.get("id"),
        "marker": lambda r: "a genai-rs-test display name" if is_marked(r.get("display_name")) else None,
        "created": voice_created,
        "delete": lambda r: (f"/v1beta/voices/{r['id']}", {}),
    },
}


def list_all(api: Api, kind: dict) -> list[dict]:
    size_param, size = kind["page_size"]
    token_param, token_field = kind["token"]
    found = []
    for path, base_query in kind["list"]:
        token = None
        for _ in range(1000):
            query = dict(base_query)
            query[size_param] = size
            if token:
                query[token_param] = token
            page = api.call("GET", path, query)
            found.extend(page.get(kind["items"]) or [])
            token = page.get(token_field)
            if not token:
                break
        else:
            raise ApiError(f"GET {path}: more than 1000 pages; is the page token ignored?")
    return found


def scheduled_mode() -> tuple[bool, str]:
    """`(dry_run, why)` for a scheduled run: it deletes only when the owner
    has set `JANITOR_DELETE` to `true`."""
    value = os.environ.get(DELETE_OPT_IN, "")
    if value.strip().lower() == "true":
        return False, f"Scheduled run: {DELETE_OPT_IN} is true, so it deletes."
    shown = f"'{value.strip()}'" if value.strip() else "unset"
    return True, (
        f"Scheduled run: {DELETE_OPT_IN} is {shown}, not true, so this is a dry run. "
        f"Set the repository variable {DELETE_OPT_IN} to true to let scheduled runs delete."
    )


def md_code(text) -> str:
    return "`" + str(text).replace("`", "'") + "`"


def write_summary(path: str, title: str, notes: list[str], acted: list[tuple],
                  unknown: list[tuple], review: list[tuple], verb: str,
                  totals: list[tuple], closing: str) -> None:
    out = [f"## Test resource janitor ({title})", ""]
    for note in notes:
        out += [note, ""]
    out += [f"### {verb.capitalize()} ({len(acted)})", ""]
    if acted:
        out += [f"- {md_code(ref)}: {detail}" for ref, detail in acted]
    else:
        out.append("Nothing.")
    out.append("")
    if unknown:
        out += [f"### Marked, age unknown: delete by hand ({len(unknown)})", ""]
        out += [f"- {md_code(ref)}: {detail}" for ref, detail in unknown]
        out.append("")
    if review:
        out += [f"### Unmarked, kept: check by hand ({len(review)})", ""]
        out += [f"- {md_code(ref)}: {detail}" for ref, detail in review]
        out.append("")
    out += table(totals, verb)
    out += ["", closing, ""]
    with open(path, "a", encoding="utf-8") as handle:
        handle.write("\n".join(out))


def table(totals: list[tuple], verb: str) -> list[str]:
    rows = [
        f"| Kind | Listed | Test-created | {verb.capitalize()} | Too new | Age unknown |",
        "|------|--------|--------------|--------------|---------|-------------|",
    ]
    rows += ["| " + " | ".join(str(c) for c in row) + " |" for row in totals]
    return rows


def run(args) -> int:
    key = os.environ.get("GEMINI_API_KEY", "").strip()
    if not key:
        print("::error::GEMINI_API_KEY is empty or unset", file=sys.stderr)
        return 2
    api = Api(args.base_url, key)
    now = parse_time(args.now) if args.now else dt.datetime.now(dt.timezone.utc)
    if now is None:
        print(f"::error::--now is not a timestamp: {args.now}", file=sys.stderr)
        return 2
    notes = []
    if args.scheduled:
        args.dry_run, why = scheduled_mode()
        print(why)
        print()
        notes.append(why)
    cutoff = now - dt.timedelta(hours=args.min_age_hours)
    verb = "would delete" if args.dry_run else "deleted"
    failed = 0
    totals = []
    acted = []    # (kind/id, detail): deleted, or would be
    unknown = []  # (kind/id, detail): marked, but its age cannot be told
    review = []   # (kind/id, detail): unmarked, kept, worth a person's look

    for name in args.kinds:
        kind = KINDS[name]
        try:
            resources = list_all(api, kind)
        except ApiError as e:
            print(f"::error::{name}: listing failed: {e}")
            failed += 1
            totals.append((name, "list failed", 0, 0, 0, 0))
            continue

        age_label = kind.get("age_label", "created")
        marked = removed = too_new = unknown_age = 0
        for resource in resources:
            ident = kind["id"](resource)
            reason = kind["marker"](resource)
            if not ident:
                continue
            if not reason:
                note = kind.get("review", lambda r: None)(resource)
                if note:
                    review.append((f"{name}/{ident}", note))
                    print(f"{'review':<8} {name}/{ident}: {note}; kept")
                continue
            marked += 1
            created = kind["created"](resource)
            when = f"{age_label} {created.isoformat(timespec='seconds')}" if created else f"{age_label} unknown"
            if created is None:
                unknown_age += 1
                unknown.append((f"{name}/{ident}", reason))
                print(f"kept     {name}/{ident}: {reason}, but its age is unknown; delete it by hand")
                continue
            if created > cutoff:
                too_new += 1
                print(f"kept     {name}/{ident}: {reason}, {when}, under {args.min_age_hours:g}h ago")
                continue
            if not args.dry_run:
                path, query = kind["delete"](resource)
                try:
                    api.call("DELETE", path, query)
                except ApiError as e:
                    if e.status != 404:
                        print(f"::error::{name}/{ident}: delete failed: {e}")
                        failed += 1
                        continue
                    # Gone already: a retry after a delete that went through
                    # but answered 5xx, or the test's own cleanup.
                    removed += 1
                    acted.append((f"{name}/{ident}", f"{reason}, {when}; already deleted (404)"))
                    print(f"{'gone':<8} {name}/{ident}: {reason}, {when}; already deleted (404)")
                    continue
            removed += 1
            acted.append((f"{name}/{ident}", f"{reason}, {when}"))
            print(f"{verb:<8} {name}/{ident}: {reason}, {when}")
        totals.append((name, len(resources), marked, removed, too_new, unknown_age))

    print()
    for row in table(totals, verb):
        print(row)
    print()
    mode = "Dry run: nothing was deleted" if args.dry_run else "Deleted"
    closing = f"{mode}. Kept anything without a test marker, and test resources under {args.min_age_hours:g}h old."
    print(closing)
    if args.scheduled and args.dry_run and acted:
        print(f"::notice::{len(acted)} test resource(s) would be deleted. "
              f"Set the repository variable {DELETE_OPT_IN} to true to let scheduled runs delete them.")
    if args.summary:
        title = "dry run" if args.dry_run else "delete"
        if args.scheduled:
            title = f"scheduled, {title}"
        write_summary(args.summary, title, notes, acted, unknown, review, verb, totals, closing)
    return 1 if failed else 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--dry-run", action="store_true", help="list what would be deleted; delete nothing")
    mode.add_argument("--delete", action="store_true", help="delete the test-created resources")
    mode.add_argument("--scheduled", action="store_true",
                      help=f"a scheduled run: --delete when ${DELETE_OPT_IN} is true, else --dry-run")
    parser.add_argument("--min-age-hours", type=float, default=6.0,
                        help="only delete resources at least this old (default 6)")
    parser.add_argument("--kinds", default=",".join(KINDS),
                        help=f"comma-separated subset of {','.join(KINDS)}")
    parser.add_argument("--summary", default=None, metavar="FILE",
                        help="append a Markdown report to FILE (the job summary)")
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL, help=argparse.SUPPRESS)
    # For the harness: a fixed "now", so ages in recorded responses hold.
    parser.add_argument("--now", default=None, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    args.kinds = [k.strip() for k in args.kinds.split(",") if k.strip()]
    unknown = [k for k in args.kinds if k not in KINDS]
    if unknown or not args.kinds:
        parser.error(f"unknown kind(s) {unknown}; choose from {','.join(KINDS)}")
    if args.min_age_hours < 1:
        parser.error("--min-age-hours must be at least 1, so a running test's resources are never deleted")
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
