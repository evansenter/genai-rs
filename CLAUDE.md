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
   releases to find new surface). The OpenAPI spec is the same surface in
   machine-readable form. Its URL (`ai.google.dev/static/api/interactions.openapi.json`)
   moved: by 2026-11-03 it redirected to an aistudio.google.com path that
   returns 404, and no new public URL was found (2026-11-04). The
   2026-11-01 copy is the latest.
2. **Live probe** against `generativelanguage.googleapis.com` — what the
   Gemini endpoint actually accepts, often narrower than the bindings.
   Probe with `LOUD_WIRE=1` or curl before modeling anything.
3. **Prose** — [Interactions API reference](https://aistudio.google.com/api-reference/interactions-api.md),
   [guide](https://aistudio.google.com/docs/interactions-overview.md)
   (the old ai.google.dev URLs redirect there),
   and `docs/INTERACTIONS_API_GAP.md` (a dated snapshot; the
   `api-surface-sweep` workflow opens an issue when the SDK moves).

`tests/fixtures/api_surface/` holds the bindings (for the version in
`.github/last-swept-sdk-version`) and the spec as a snapshot, and the
coverage oracle (`src/coverage_oracle/`, `tests/request_wire_oracle.rs`)
checks the crate against it offline: request keys against the binding
classes, every response field for an empty `extra`, and every class
reached or listed with a reason. `tests/ci_coverage.rs` maps every endpoint
to the handle verbs that call it and requires a live test of each verb on
its handle. `tests/fixtures/api_surface/bindings.json` is the one
committed dump of the bindings, and only
`.github/scripts/refresh_api_snapshot.sh` writes it: bump the baseline with
`refresh_api_snapshot.sh --version X.Y.Z`, which regenerates the snapshot
too. `refresh_api_snapshot.sh --check` fails on a
stale, empty or hand-edited snapshot (`make test-scripts` runs its offline
half); see `docs/TESTING.md`.

A field that serializes correctly is not a working feature (D-010): verify
against the live API.

## Commands

Requires [cargo-nextest](https://nexte.st/). Optional: `./scripts/setup-dev.sh`
enables the mold linker when the compiler supports it (`.cargo/config.toml`
is gitignored).

```bash
make check        # pre-push gate: fmt + clippy + test + test-scripts
make test         # unit/offline tests (no doctests)
make test-strict  # the same in strict mode (--cfg genai_strict_unknown, D-030)
make test-all     # + live integration tests (needs GEMINI_API_KEY)
make test-scripts # shell-script harnesses (needs bash, jq, python3; PyYAML for one)
make docs         # rustdoc -D warnings, all-features and docs.rs feature set

cargo nextest run -E 'test(/name/)'            # one test
cargo nextest run --test <file> --run-ignored all   # one live test file
cargo test --workspace --doc --all-features    # doctests (CI runs these; make does not)
cargo nextest run --features antigravity --run-ignored all -E 'binary(antigravity_harness)'
                                               # harness suite (needs localharness)
```

Live tests take minutes and can flake on model variability. `LOUD_WIRE=1`
prints the wire. `RUST_LOG` does nothing in tests or examples: neither
installs a `tracing` subscriber.

## Layout

| Path | Contents |
|------|----------|
| `src/client.rs`, `src/request_builder/` | `Client`, `ClientBuilder`, `interaction()`, `execute`/`execute_stream`, `InteractionBuilder` (one file per concern: `input`, `tools`, `output`, `generation`; `ConversationBuilder` in `conversation.rs`), the auto-function loops (`auto_functions.rs`) and the round they share (`auto_functions/round.rs`) |
| `src/request/`, `src/content/`, `src/tools/` | Request types (`generation_config.rs`, `agent_config.rs`), `Content` (annotations, result items, video processing alongside), tool configs (`function`, `choice`, `retrieval`, `builtin`) |
| `src/steps/`, `src/response/` | The steps response model (`Step`, `StepDelta`, the stream `accumulator`), `InteractionResponse` and its accessors (usage, view types and `StepSummary` alongside) |
| `src/wire_streaming/`, `src/streaming.rs` | Stream chunk/event types (their wire format in `wire_streaming/serde_impls.rs`); auto-function stream types (theirs in `streaming/serde_impls.rs`) |
| `src/webhooks.rs`, `triggers.rs`, `agents.rs`, `environments/`, `files.rs`, `file_search_stores.rs`, `credentials.rs`, `voices.rs` | Resource types (`/v1beta/...`) and each resource's accessor, handle and list builders (D-016; the old-to-new map is `docs/RESOURCES.md`) |
| `src/interactions.rs` | The `Interactions` handle: get, stream, resume, cancel and delete a stored interaction by id (D-017) |
| `src/paging.rs`, `src/poll.rs` | `pub(crate)` paging engine behind every list builder's `.pages()` / `.items()`, and the `list_builder!` macro that defines the builders; `PollOptions` and the wait behind every `wait_until_active` |
| `src/http/` | `pub(crate)` HTTP layer: one request path (`common.rs`), SSE parser, error mapping |
| `src/wire/` | `WireInspector`, `LOUD_WIRE` printer and `WireFilter` (`printer.rs`), `TracingForwarder` |
| `src/function_calling.rs`, `genai-rs-macros/` | Function registry, `#[tool]` macro (`inventory` registration) |
| `src/antigravity/` | Harness client (feature-gated); see `docs/ANTIGRAVITY.md` |
| `src/proptest_tests.rs`, `src/proptest_<feature>.rs` | Property tests (test-only): shared strategies in the root, then one file per feature (`content`, `steps`, `request`, `tools`, `response`, `resources`, `unknown_fields`). Past failures replay from `proptest-regressions/<source file>.txt`, so a property's seeds move with it |
| `fuzz/`, `src/fuzzing.rs` | cargo-fuzz targets (own workspace and lockfile, nightly; `fuzz/README.md`) and their entry points, the `#[doc(hidden)] __fuzz` module, compiled only under `cfg(fuzzing)` and in tests (`fuzzing_tests.rs` replays past crashes) |

Submodules are private; a directory's `mod.rs` re-exports its public items
explicitly, so public paths do not depend on file layout. Keep implementation
modules at most 800 lines before their test module: add a module rather than
growing a large file (D-014). A unit-test file is split past 1,500 lines, an
integration-test file past 2,000 and a shared test module (`tests/common/`,
`tests/live_verbs/`) past 800, by the feature its tests verify (D-008).
`tests/module_size.rs` enforces all four bars and names the file, its count
and the limit. When a module or directory moves, record it in
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
and keep unmodeled fields in a `#[serde(flatten)] extra` map; the known
variants of `Step`, `StepDelta`, `Content` and `Annotation` do the same with
an `extra` field, through `src/wire_fields.rs` (#491). Polling
continues on unknown statuses, bounded by timeouts. Strict mode, the
`--cfg genai_strict_unknown` flag (a cfg, not a feature: D-030), makes
`Content`, `Step` and every string enum fail on unknown values instead
(`make test-strict`). New string enums use the crate-private `wire_enum!` macro
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
| `DEFAULT_IMAGE_MODEL` | Image generation |
| `DEFAULT_TTS_MODEL` | Text-to-speech |
| `CONTINUATION_MODEL` | Decoding continuation (the only model that supports it) |
| `DEFAULT_DEEP_RESEARCH_AGENT`, `DEFAULT_ANTIGRAVITY_AGENT` | `with_agent()` |

Unit tests may use `"test-model"` where the id is filler. Non-test code that
reaches the wire must use a real id.

### API conventions

- `with_*` configures a setting (calling twice replaces); `add_*` appends to a
  collection. See `docs/BUILDER_API.md`.
- Resources are reached through handles: `client.<resources>().<verb>(..)`
  (`client.webhooks().get(id)`). Handle methods take `self`; required
  arguments are positional and optional inputs live on a value. Lists return
  a builder ending in `.send()` (one page), `.pages()` or `.items()` (every
  page). See D-016 and `docs/RESOURCES.md`.
- `*_with_auto_functions()` runs the function-calling loop
  (`docs/FUNCTION_CALLING.md`).
- `#[must_use]` on getters, handles and boolean checks.
- Lints (`Cargo.toml`, `[workspace.lints]`, #501): every public item has a
  doc comment, a public fallible function an `# Errors` section, and
  clippy's `uninlined_format_args`, `assigning_clones`,
  `redundant_closure_for_method_calls` and `must_use_candidate` hold in
  src/, tests/ and examples/; `make clippy` denies warnings.
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
  behavior is tested against a local stub (`tests/common/http_stub.rs`) via
  `ClientBuilder::with_base_url`: client behavior in
  `tests/http_mock_tests.rs`, resource endpoints in
  `tests/http_mock_resources/` (one file per resource; the harness is
  `tests/http_mock_resources.rs`), and timeouts, cut-off streams and poll
  retries in `tests/reliability_tests.rs` (paused clock; see
  `docs/TESTING.md`). Real responses, captured with their dates, replay in
  `tests/captured_fixtures.rs`.
- A crash a fuzz target finds becomes a regression test in
  `src/fuzzing_tests.rs` (`fuzz/README.md`).
- Live tests are `#[ignore = "Requires API key"]` (exact string) and must
  **fail**, not skip, when the request fails. Every live test file belongs to
  a `test-integration` matrix group in `.github/workflows/rust.yml`.
- A live test names what it creates with `test_resource_name` /
  `test_source_content` (`tests/common/mod.rs`), the marker the daily
  test-resource janitor matches (it deletes only when `JANITOR_DELETE` is
  `true`), and cleans up itself, background
  interactions included. A request field with no observable effect is
  checked as stored with `stored_echo` and `assert_stored`, not just
  accepted (#492).
- Timing tests against the HTTP stub run on a paused clock and assert
  virtual time, never `std::time::Instant` (#501; `docs/TESTING.md`). The
  Antigravity suites drive real processes, so their bounds stay wall-clock,
  as does `stream_ends_once_the_tail_after_completed_has_arrived` (#518),
  which bounds a stream's tail read from above: a released paused clock
  can overshoot it.
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
breaking changes with migration, fixes); create the section above the latest
release if it is absent. Not for internal refactors or CI.

Version bump — update all of:

| File | Field |
|------|-------|
| `Cargo.toml` | `version`, and the `genai-rs-macros` dependency version |
| `genai-rs-macros/Cargo.toml` | `version` |
| `README.md` | `genai-rs = "X.Y"`, `genai-rs-macros = "X.Y"` |
| `docs/ANTIGRAVITY.md` | `genai-rs = { version = "X.Y", ... }` |
| `CHANGELOG.md` | `## [Unreleased]` → `## [X.Y.Z] - YYYY-MM-DD` |

Harness bump (`SUPPORTED_HARNESS_VERSION` in `src/antigravity/config.rs`):
update every pin and "currently" mention
(`git grep -n 'google-antigravity==\|currently .0\.1\.'`), add a row to the
upgrade table in `docs/ANTIGRAVITY_BRIDGE_DESIGN.md`, and run
`cargo nextest run --features antigravity --run-ignored all -E
'binary(antigravity_harness)'` against the new wheel with its venv first on
`PATH` (the drift guard imports the wheel's protos with `python3`).

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

- `rust.yml` jobs: check (also fails when `cargo package` would ship a
  test-only file, and builds the packaged crates), test,
  test-strict-unknown (strict mode, under `RUSTFLAGS`), test-antigravity (pinned `localharness` wheel),
  test-integration (live, grouped), example-smoke (live), captured-fixtures
  (live, schedule and dispatch only: re-captures `tests/fixtures/captured/`,
  replays the fresh copies, and fails once the committed ones are over 90
  days old), fmt, shell-scripts (the script harnesses,
  shellcheck, the workflow parse check and actionlint over every workflow),
  clippy, doc, msrv, cross-platform (stable and beta; trybuild's `ui_tests`
  are skipped on beta), coverage, build-metrics, scheduled-run-status. Live
  jobs run for pushes, same-repo PRs, the weekly schedule (Wednesday) and
  `workflow_dispatch`, and fail on an empty `GEMINI_API_KEY`. Each
  integration group runs every binary and lists the failures;
  `.config/nextest.toml` turns fail-fast off for every run and kills a test
  hung past 10 minutes. A red scheduled run opens a `ci-health-escalation`
  issue (`scheduled-run-status`); every scheduled workflow does the same,
  which `tests/ci_workflows/` checks along with fail-fast and beta.
- Required on `main` (strict, up to date): Check, Test Suite, Format, Clippy,
  Documentation, Test Suite (antigravity), Test Suite (strict-unknown) and the
  five Integration Tests groups. A live group skipped on a fork PR counts as
  passing; renaming a job or group means updating branch protection too.
- Other workflows: `audit.yml`, `api-surface-sweep.yml`, `consumer-crate.yml`,
  `ci-flakiness-report.yml` (daily; counts failures and the tests nextest
  retried to a pass, and the week's flake rate against a 1% target),
  `release.yml`, `release-drafter.yml`, `claude.yml`,
  `claude-code-review.yml`, `antigravity-release-watch.yml` (daily; an
  `antigravity-release` issue when PyPI's `google-antigravity` differs from
  `SUPPORTED_HARNESS_VERSION`), `fuzz.yml` (the `fuzz/` cargo-fuzz targets,
  a minute each on nightly: weekly, and on PRs touching what the targets
  build, keeping the grown corpus between scheduled runs; `fuzz/README.md`),
  and `test-resource-janitor.yml` (daily; the marked resources live tests
  leaked, once six hours old. A scheduled run only lists them in the job
  summary unless the repository variable `JANITOR_DELETE` is `true`, the
  key owner's opt-in; a manual run deletes only with `mode: delete`;
  `docs/TESTING.md`).
- **Example size gate:** `build-metrics` fails when an example binary grows
  more than 15% against the last main build. For intended growth, add the
  `size-growth-ok` label **and push a commit** (a re-run replays the old event
  payload).
- `gh run view --log-failed` lines are prefixed `Job\tStep\tTimestamp\t`; use
  `sed 's/.*test //'` to extract test names.

## Technical notes

Rust edition 2024, MSRV 1.88. `reqwest` with `rustls` (OS trust store),
without its default features: `http2`, `system-proxy` and `charset` are
genai-rs default features that turn them on. Tokio runtime. `Cargo.toml`'s
`include` list decides what the published crate ships.
