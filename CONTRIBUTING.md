# Contributing

## Setup

Requires Rust 1.88+ (edition 2024) and [cargo-nextest](https://nexte.st/).

```bash
cargo install cargo-nextest --locked
```

**Linker**: `.cargo/config.toml` selects [mold](https://github.com/rui314/mold)
for faster builds. It scopes that to `[target.x86_64-unknown-linux-gnu]`, so
on that target — and only that one — every build fails without mold
installed, before compiling anything, with a message that names the wrong
tool. On macOS or aarch64 Linux the config is inert, and the override below
will not clear an unrelated linker error there:

```text
error: linking with `cc` failed: exit status: 1
  = note: collect2: fatal error: cannot find 'ld'
```

`ld` is present; `mold` is not. The zero-install way out is to drop the flag
entirely — the env var overrides `target.x86_64-unknown-linux-gnu.rustflags`
from `.cargo/config.toml`, falling back to the system linker:

```bash
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=""
```

Slower, but always present. For the fast path install a linker first —
`sudo apt install mold`, or `lld` and:

```bash
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=-fuse-ld=lld"
```

Note that `lld` is no more installed by default than `mold` is: reaching for
that line without installing it reproduces the same failure under a
different name.

(Tracked as #428 — the config hard-requires a non-default tool.)

## The gate

```bash
make check      # fmt + clippy + test — run before pushing
make test-all   # full suite including integration tests (needs GEMINI_API_KEY)
```

Integration tests take 2-5 minutes and some flake on LLM variability. Doctests
run in CI only, not in `make test` (see D-009).

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
   # unpack both, then:
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
