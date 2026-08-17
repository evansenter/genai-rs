# Contributing

## Setup

Requires Rust 1.88+ (edition 2024) and [cargo-nextest](https://nexte.st/).

```bash
cargo install cargo-nextest --locked
```

Also `jq` and `python3`. They are not optional extras: `make check` runs
`make test-scripts`, which hard-fails when either is missing rather than
skipping, so the pre-push gate needs both.

**Linker (optional)**: run `./scripts/setup-dev.sh` once per clone to enable
[mold](https://github.com/rui314/mold) for faster builds. It probes whether
your compiler can actually link with mold rather than just checking that the
binary exists, and is a no-op when it cannot — so it is safe to run either
way.

`.cargo/config.toml` is deliberately *not* checked in. When it was, it set
`-fuse-ld=mold` unconditionally for `x86_64-unknown-linux-gnu`, so a clone on
a machine without mold failed every build before compiling anything, with a
message naming the wrong tool (#428):

```text
error: linking with `cc` failed: exit status: 1
  = note: collect2: fatal error: cannot find 'ld'
```

`ld` is present in that state; mold is not, and the message never says so.
The file is now gitignored and ships as `.cargo/config.toml.example`. If you
have an old checkout still carrying the generated config and hit the error
above, delete `.cargo/config.toml` — that is the fix, rather than overriding
rustflags around it.

## The gate

```bash
make check      # fmt + clippy + test + test-scripts — run before pushing
make test-all   # full suite including integration tests (needs GEMINI_API_KEY)
```

Integration tests take 2-5 minutes and some flake on LLM variability. Doctests
run in CI only, not in `make test` (see D-009).

One side effect worth knowing, since it touches the file Setup just told you
to generate: `make test-scripts` exercises `setup-dev.sh` against the
checkout's *real* `.cargo/config.toml`, restoring it from a temp copy on an
EXIT trap. That file is gitignored, so if you kill the run outright rather
than letting it finish, git cannot bring it back (#455).

Recover by **deleting it first**:

```bash
rm -f .cargo/config.toml && ./scripts/setup-dev.sh
```

Re-running the script on its own is not enough. It exits early with
`already exists; leaving it alone` whenever the file is present, and a
killed harness usually leaves one behind rather than removing it — one
written under the harness's stub compiler, pinning `linker` to a path
inside a temp directory that no longer exists. Builds then fail at link
time while the file looks present and the script reports nothing to do.

## Verifying API behavior

**Do not conclude the API supports something from documentation alone.** The
sources disagree, consistently in one direction — see D-004 in
[DECISIONS.md](DECISIONS.md). Order:

1. **Generated bindings** — `google-genai`'s `_gaos/types/interactions/*.py`.
   This repo does not depend on that Python package, so there is no path in
   the tree that leads there; fetch two releases and diff them:

   ```bash
   # One version per invocation — pip resolves a single version per package,
   # so two pins in one command is a conflict rather than two downloads.
   pip download --no-deps --no-binary :all: google-genai==2.17.0 -d /tmp/gg
   pip download --no-deps --no-binary :all: google-genai==2.18.1 -d /tmp/gg
   # --no-binary :all: is load-bearing: it is what yields the version-named
   # sdist directories the diff below refers to.
   cd /tmp/gg && tar xf google_genai-2.17.0.tar.gz && tar xf google_genai-2.18.1.tar.gz
   diff -ru google_genai-2.17.0/google/genai/_gaos/types/interactions \
            google_genai-2.18.1/google/genai/_gaos/types/interactions
   ```

   The version last swept against is in the `docs/INTERACTIONS_API_GAP.md`
   header — currently 2.17.0, which is why the 2.18.1 `processing` field
   (#419) went unmodeled behind a "fully covered" conclusion
2. **Live probe** — `curl` against `generativelanguage.googleapis.com`, or a
   test with `LOUD_WIRE=1`
3. **Prose docs** — `ai.google.dev` and this repo's own; both lag

Changes to wire format need a live probe, not a spec reading. Video
`processing` (#419) appears in the 2.18.1 bindings and in neither published
doc, and one field
(`cached_content`) was modeled from the spec and has been rejected by the API
for its entire shipped life — its removal is pending in #439.

## Adding wire types

When adding or changing an enum or request/response field:

- [ ] Live-probe the actual wire format (`LOUD_WIRE=1`)
- [ ] Add an `Unknown` variant with all three helpers — `is_unknown()`,
      `unknown_<context>_type()`, `unknown_data()` (D-001)
- [ ] Round-trip test: unknown data survives deserialize → serialize
- [ ] Update `docs/ENUM_WIRE_FORMATS.md` with the **verified** format and
      the date it was verified
- [ ] Use snake_case on the wire; if the API accepts both spellings, send
      snake_case

## Tests

Structural assertions (status, field presence) are the default. Use a
**behavioral** assertion where the feature has an observable effect — a
serialization test passing tells you nothing about whether the API accepts
the request (D-010).

- Gate live tests with `#[ignore = "Requires API key"]`, exactly that string
- Make them **fail** on request errors, not skip — a test that swallows the
  error and returns early passes without reaching the API
- Avoid `text.contains("word")` on model output; use
  `assert_response_semantic()` for natural-language checks
- Organize by what the test primarily verifies, not by the mechanics it uses
  (D-008)

See `docs/TESTING.md` for the full decision flowchart.

## When to add a DECISIONS.md entry

Add one when a change:

- Departs from a stated principle (D-003 departs from D-001)
- Chooses between defensible alternatives for a reason worth preserving
- Relocates or restructures code — record what moved (D-011)
- Reverses an earlier decision — mark the old one superseded, don't delete it

Not needed for ordinary bug fixes, new API surface that follows existing
patterns, or anything the CHANGELOG already covers.

## CHANGELOG

Update `CHANGELOG.md` for user-facing changes: features, breaking changes, bug
fixes, deprecations. Internal refactors and CI changes don't need entries.

Breaking changes are permitted and preferred over compatibility shims (D-007);
say plainly what breaks and what the migration is.
