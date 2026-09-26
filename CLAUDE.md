# CLAUDE.md

Guidance for Claude Code when working in this repository. The reasoning
behind the rules lives in `DECISIONS.md`; setup and review checklist in
`CONTRIBUTING.md`.

`genai-rs` is a Rust client for Google's Gemini **Interactions API**
(wire revision 2026-05-20), plus an optional native client for the
Antigravity `localharness` agent runtime (`antigravity` feature).

## Sources of truth for API behavior

Prose docs lag the API; absence from them is not evidence of absence. In
order (D-004):

1. **Generated bindings** — `google-genai`'s `_gaos/types/` (diff two
   releases to find new surface).
2. **Live probe** against `generativelanguage.googleapis.com` — what the
   Gemini endpoint actually accepts, often narrower than the bindings.
   Probe with `LOUD_WIRE=1` or curl before modeling anything.
3. **Prose** — [Interactions API reference](https://ai.google.dev/static/api/interactions.md.txt),
   [guide](https://ai.google.dev/static/api/interactions-api.md.txt),
   and `docs/INTERACTIONS_API_GAP.md` (a dated snapshot; the
   `api-surface-sweep` workflow opens an issue when the SDK moves).

A field that serializes correctly is not a working feature (D-010): verify
against the live API.

## Commands

Requires [cargo-nextest](https://nexte.st/). Optional: `./scripts/setup-dev.sh`
enables the mold linker when the compiler supports it (`.cargo/config.toml`
is gitignored).

```bash
make check        # pre-push gate: fmt + clippy + test + test-scripts
make test         # unit/offline tests (no doctests)
make test-all     # + live integration tests (needs GEMINI_API_KEY)
make test-scripts # shell-script harnesses (needs bash, jq, python3)
make docs         # rustdoc -D warnings, all-features and docs.rs feature set

cargo nextest run -E 'test(/name/)'            # one test
cargo nextest run --test <file> --run-ignored all   # one live test file
cargo test --workspace --doc --all-features    # doctests (CI runs these; make does not)
cargo nextest run --features antigravity --run-ignored all -E 'binary(antigravity_harness)'
                                               # harness suite (needs localharness)
```

Live tests take minutes and can flake on model variability. `LOUD_WIRE=1`
prints the wire; `RUST_LOG=genai_rs=debug` enables debug logs.

## Layout

| Path | Contents |
|------|----------|
| `src/client.rs`, `src/request_builder/` | `Client`, `ClientBuilder`, the interaction methods, `InteractionBuilder` (one file per concern: `input`, `tools`, `output`, `generation`; `ConversationBuilder` in `conversation.rs`), the auto-function loop (`auto_functions.rs`) |
| `src/request/`, `src/content/`, `src/tools/` | Request types (`generation_config.rs`, `agent_config.rs`), `Content` (annotations, result items, video processing alongside), tool configs (`function`, `choice`, `retrieval`, `builtin`) |
| `src/steps/`, `src/response/` | The steps response model (`Step`, `StepDelta`, the stream `accumulator`), `InteractionResponse` and its accessors (usage, view types and `StepSummary` alongside) |
| `src/wire_streaming.rs`, `src/streaming.rs` | Stream chunk/event types; auto-function stream types |
| `src/webhooks.rs`, `triggers.rs`, `agents.rs`, `environments/`, `files.rs`, `file_search_stores.rs`, `credentials.rs`, `voices.rs` | Resource types (`/v1beta/...`) and each resource's `impl Client` methods |
| `src/http/` | `pub(crate)` HTTP layer: one request path (`common.rs`), SSE parser, error mapping |
| `src/wire/` | `WireInspector`, `LOUD_WIRE` printer and `WireFilter` (`printer.rs`), `TracingForwarder` |
| `src/function_calling.rs`, `genai-rs-macros/` | Function registry, `#[tool]` macro (`inventory` registration) |
| `src/antigravity/` | Harness client (feature-gated); see `docs/ANTIGRAVITY.md` |

Submodules are private; a directory's `mod.rs` re-exports its public items
explicitly, so public paths do not depend on file layout. Keep implementation
modules under roughly 800 lines excluding tests: add a module rather than
growing a large file (D-014). When a module or directory moves, record it in
`DECISIONS.md` (D-011).

## Rules

### Evergreen soft-typing (D-001)

Unknown API data is preserved, never rejected. Enums that can grow are
`#[non_exhaustive]` and carry an Unknown variant:

```rust
Unknown {
    <context>_type: String,      // the unrecognized wire value
    data: serde_json::Value,     // full JSON, re-serialized unchanged
}
```

with helpers `is_unknown()`, `unknown_<context>_type()`, `unknown_data()`
(reference: `Content` in `src/content/`). Response and resource structs are
`#[non_exhaustive]` (D-002, guarded by `tests/non_exhaustive_responses.rs`)
and keep unmodeled fields in a `#[serde(flatten)] extra` map. Polling
continues on unknown statuses, bounded by timeouts. The `strict-unknown`
feature makes `Content`, `Step` and every string enum fail on unknown values
instead. New string enums use the crate-private `wire_enum!` macro
(`src/wire_enum.rs`), which supplies all of the above.

When adding or changing an enum, record its verified wire format in
`docs/ENUM_WIRE_FORMATS.md`.

### Wire format

- Interactions API field names are **snake_case**; send snake_case even if
  camelCase is also accepted. (The Files and File Search Store APIs are
  camelCase.)
- Fields the endpoint rejects as *Vertex-only* are kept and documented;
  fields rejected as *Unknown parameter* are removed (D-005).

### Model and agent ids (D-006)

Never hardcode one. Use the constants in `src/lib.rs`;
`tests/model_literals.rs` fails on `"gemini-<digit>..."` or dated agent-id
literals elsewhere.

| Constant | Use for |
|----------|---------|
| `DEFAULT_MODEL` | Everything, unless a row below applies |
| `MINIMAL_THINKING_MODEL` | `ThinkingLevel::Minimal` (the default model rejects it) |
| `DEFAULT_IMAGE_MODEL` | Image generation |
| `DEFAULT_TTS_MODEL` | Text-to-speech |
| `DEFAULT_DEEP_RESEARCH_AGENT`, `DEFAULT_ANTIGRAVITY_AGENT` | `with_agent()` |

Unit tests may use `"test-model"` where the id is filler. Non-test code that
reaches the wire must use a real id.

### API conventions

- `with_*` configures a setting (calling twice replaces); `add_*` appends to a
  collection. See `docs/BUILDER_API.md`.
- `*_with_auto_functions()` runs the function-calling loop
  (`docs/FUNCTION_CALLING.md`).
- `#[must_use]` on getters, handles and boolean checks.
- Errors: `GenaiError` (`src/errors.rs`) for API/transport, `FunctionError`
  for tool execution.
- Logging (`docs/LOGGING_STRATEGY.md`): `warn` for recoverable issues
  including Evergreen unknowns, `debug` for lifecycle; user content and
  request bodies only at `debug`; API keys always redacted.
- Breaking changes are fine when they simplify the API (D-007); no
  compatibility shims. Pre-1.0.

### Multi-turn inheritance

| Field | Inherited via `previous_interaction_id`? |
|-------|------------------------------------------|
| Conversation history | Yes |
| `system_instruction` | No — resend per turn |
| `tools` | No — resend on every user turn |

### Tests

- Unit tests for a new module go in a sibling file, declared at the bottom of
  the module as `#[cfg(test)] #[path = "<module>_tests.rs"] mod tests;`
  (existing inline test modules stay until their code moves). Offline HTTP
  behavior is tested against a local stub in `tests/http_mock_tests.rs` via
  `ClientBuilder::with_base_url`.
- Live tests are `#[ignore = "Requires API key"]` (exact string) and must
  **fail**, not skip, when the request fails. Every live test file belongs to
  a `test-integration` matrix group in `.github/workflows/rust.yml`.
- Don't assert `text.contains(..)` on non-deterministic model output; use
  `assert_response_semantic()` (see `docs/TESTING.md`). Exact computed values
  are fine.
- Tests are organized by the feature they verify, not the mechanics they use
  (D-008).
- New public constructors need unit tests.

### Examples

See `examples/CLAUDE.md`, which covers every example including
`examples/antigravity/`: each runs live and exits 0, propagates errors, fails
when its own claim doesn't hold, prints results rather than commentary, and
cleans up.

## Changelog and versioning

Update `CHANGELOG.md` `[Unreleased]` for user-facing changes (features,
breaking changes with migration, fixes). Not for internal refactors or CI.

Version bump — update all of:

| File | Field |
|------|-------|
| `Cargo.toml` | `version`, and the `genai-rs-macros` dependency version |
| `genai-rs-macros/Cargo.toml` | `version` |
| `README.md` | `genai-rs = "X.Y"`, `genai-rs-macros = "X.Y"` |
| `docs/ANTIGRAVITY.md` | `genai-rs = { version = "X.Y", ... }` |
| `CHANGELOG.md` | `## [Unreleased]` → `## [X.Y.Z] - YYYY-MM-DD` |

## Release

1. Build docs on nightly first (docs.rs builds on nightly; CI checks stable,
   D-009):
   `RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --workspace --no-deps --features antigravity --target-dir target/doc-docsrs`
2. `git tag -a vX.Y.Z origin/main -m "Release vX.Y.Z" && git push origin vX.Y.Z`.
   `release.yml` validates (including the live suite), publishes
   `genai-rs-macros` then `genai-rs`, and creates the GitHub release from the
   `## [X.Y.Z]` CHANGELOG section. Never `cargo publish` or
   `gh release create` by hand.
3. On failure: `validate` → re-run (the live suite can flake); tag/version
   mismatch → delete the tag, fix, re-tag; failed after `genai-rs-macros`
   published → bump the patch version and release again; `github-release` →
   re-run.

## CI notes

- `rust.yml` jobs: check, test, test-strict-unknown, test-antigravity (pinned
  `localharness` wheel), test-integration (live, grouped), example-smoke
  (live), fmt, shell-scripts, clippy, doc, msrv, cross-platform, coverage,
  build-metrics. Live jobs run only for same-repo pushes/PRs and fail on an
  empty `GEMINI_API_KEY`.
- Other workflows: `audit.yml`, `api-surface-sweep.yml`, `consumer-crate.yml`,
  `ci-flakiness-report.yml`, `release.yml`, `release-drafter.yml`.
- **Example size gate:** `build-metrics` fails when an example binary grows
  more than 15% against the last main build. For intended growth, add the
  `size-growth-ok` label **and push a commit** (a re-run replays the old event
  payload).
- `gh run view --log-failed` lines are prefixed `Job\tStep\tTimestamp\t`; use
  `sed 's/.*test //'` to extract test names.

## Technical notes

Rust edition 2024, MSRV 1.88. `reqwest` with `rustls` (OS trust store).
Tokio runtime.
