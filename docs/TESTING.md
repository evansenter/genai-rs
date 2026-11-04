# Testing Guide

How the test suite is laid out, how to run each part, and the rules new tests
follow.

## Test kinds

| Kind | Where | Needs a key | Run with |
|------|-------|-------------|----------|
| Unit | `#[cfg(test)]` modules and `src/*_tests.rs` | No | `make test` |
| Property (proptest) | `src/proptest_tests.rs` | No | `make test` (or `cargo test proptest`) |
| Offline HTTP | `tests/http_mock_tests.rs`, `tests/http_mock_resources.rs` | No | `make test` |
| Coverage oracle | `src/coverage_oracle/`, `tests/request_wire_oracle.rs` (see [Coverage oracle](#coverage-oracle)) | No | `make test` |
| Guards | `tests/ci_coverage.rs` (with `tests/live_verbs/`), `tests/model_literals.rs`, `tests/non_exhaustive_responses.rs` | No | `make test` |
| Compile-time (trybuild) | `tests/ui/` via `tests/ui_tests.rs` | No | `cargo test --test ui_tests` |
| Live integration | The other `tests/*.rs` files; each test is `#[ignore = "Requires API key"]` | Yes | `make test-all` |
| Doctests | Rustdoc examples and the markdown guides (`doc_comment!` in `src/lib.rs`) | No | `cargo test --workspace --doc --all-features` (CI only; `make` skips them) |
| Harness | `tests/antigravity_harness.rs` | Yes, plus `localharness` | See [ANTIGRAVITY.md](ANTIGRAVITY.md) |

```bash
make test                                             # everything offline
make test-all                                         # + live tests (GEMINI_API_KEY)
cargo nextest run --test multiturn_tests --run-ignored all   # one live file
cargo nextest run -E 'test(/function_calling/)' --run-ignored all
LOUD_WIRE=1 cargo nextest run -E 'test(/name/)' --run-ignored all --no-capture -j 1
cargo test --features strict-unknown                  # unknown Content/Step/string-enum values become errors
```

Tests that exercise an `Unknown` value carry
`#[cfg(not(feature = "strict-unknown"))]`, since strict mode rejects it.

| Variable | Purpose |
|----------|---------|
| `GEMINI_API_KEY` | Required for live tests |
| `TEST_TIMEOUT_SECS` | Per-test budget (default 60) |
| `EXTENDED_TEST_TIMEOUT_SECS` | Budget for multi-turn tests (default 120) |
| `LOUD_WIRE=1` | Wire logging (tests install no `tracing` subscriber, so `RUST_LOG` does nothing) |

## Offline HTTP tests

`ClientBuilder::with_base_url` points a real `Client` at a local stub
(`tests/common/http_stub.rs`, a small tokio server). The stub records every
request (`Recorded`: method, path and query, headers, body) and replays
canned `Reply`s: JSON, text, SSE chunks, extra headers, or a delay.

- **`http_mock_tests.rs`** covers client behavior that only shows on the
  wire: request shapes, error mapping, SSE framing, and the auto-function
  loop.
- **`http_mock_resources.rs`** pins every resource endpoint on method, path
  (percent-encoding included), query and JSON body. It parses a realistic
  response, preserving `Unknown` and `extra`, and covers the wait helpers'
  success, failure and timeout paths. Files API uploads are left out.
- **Known bugs**, when there are any, go in
  `tests/http_mock_resources/known_bugs.rs` as failing tests ignored with a
  `known bug: ...` reason until fixed. A subdirectory, because
  `ci_coverage.rs` scans only top-level files. Run them with
  `cargo nextest run --test http_mock_resources --run-ignored only`.

Prefer an offline test for anything that doesn't depend on model behavior;
it's fast and runs on every PR.

## Live tests

### Rules

- **Ignore reason.** A live test is `#[ignore = "Requires API key"]`, that
  exact string. `tests/ci_coverage.rs` fails if any top-level `tests/*.rs`
  file uses another reason (`antigravity_harness` is exempt).
- **CI matrix.** Every top-level file with a live test must be listed in
  exactly one `test-integration` group in `.github/workflows/rust.yml`.
  `ci_coverage.rs` fails on a missing binary, a binary listed twice, or a
  listed binary with no file.
- **Fail, don't skip.** The only early return is the no-key skip. A request
  that fails must fail the test (D-010). If a turn must call a function, set
  `FunctionCallingMode::Any`; don't return when the model answers directly.
  CI fails the job outright on an empty `GEMINI_API_KEY`, so the no-key
  skip never turns a CI run green.
- **Assert the effect.** A structural check pins the wire shape, not that a
  feature works. Where the feature has an observable effect, assert that too.

| Group | Binaries |
|-------|----------|
| `core` | `interactions_api_tests`, `multiturn_tests`, `streaming_multiturn_tests`, `streaming_resume_tests`, `error_handling_tests` |
| `tools` | `tools_and_config_tests`, `webhooks_and_agents_tests`, `credentials_tests`, `environment_files_tests` |
| `functions` | `function_calling_tests`, `tool_service_tests` |
| `multimodal` | `multimodal_tests`, `api_canary_tests`, `temp_file_tests`, `processing_steps_tests`, `voices_tests`, `binding_parity_tests` |
| `files-and-wire` | `files_api_tests`, `file_search_stores_tests` |

Tests live with the feature they verify, not the mechanics they use (D-008).
A function-calling test that happens to be multi-turn belongs in
`function_calling_tests.rs`. `api_canary_tests` fail when the API returns a
step, delta or status type the crate doesn't model. They are compiled out
under `strict-unknown`.

### Template

```rust,ignore
mod common;
use common::*;

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_feature_name() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_timeout(test_timeout(), async {
        let response = retry_request!([client] => {
            interaction_builder(&client).with_text("Test prompt").create().await
        })
        .expect("request should succeed");

        assert_eq!(response.status, InteractionStatus::Completed);
        let text = response.as_text().expect("should have text");
        assert_response_semantic(&client, "Asked X", text, "Does this answer X?").await;
    })
    .await;
}
```

### Helpers (`tests/common/mod.rs`)

| Helper | Purpose |
|--------|---------|
| `get_client()` | `Some(Client)` when `GEMINI_API_KEY` is set |
| `interaction_builder(&client)`, `stateful_builder(&client)` | Builders preset with `DEFAULT_MODEL` (the latter with storage on) |
| `retry_request!([vars] => { ... })`, `retry_on_transient(n, ..)` | Retry transport errors (`is_retryable()`) and the model-side flakes in `is_transient_error` |
| `with_timeout(test_timeout(), ..)`, `extended_test_timeout()` | Bound a test |
| `consume_stream(..)`, `consume_auto_function_stream(..)` | Collect a stream's text and final response; they panic on the first stream error |
| `poll_until_done(..)` | Poll a background interaction |
| `assert_response_semantic(..)`, `validate_response_semantically(..)` | Semantic checks (below) |
| `get_weather_function()`, `get_time_function()` | Shared declarations |
| `TINY_RED_PNG_BASE64`, `TINY_BLUE_PNG_BASE64`, `TINY_WAV_BASE64`, `TINY_MP4_BASE64`, `TINY_PDF_BASE64` | Minimal valid media. The MP4 is one second: shorter clips yield no sampled frame and are rejected |

`is_transient_error` matches Spanner UTF-8 errors and two 400s (`invalid json
syntax`, `there was a problem processing your request`); see
[Error Handling](ERROR_HANDLING.md#known-transient-errors). A validation
rejection still fails on the first attempt.

For the auto-function loop, assert that executions **succeeded**, not just
that they happened: `result.all_executions_succeeded()`, with
`result.failed_executions()` in the message. A declared function with no
implementation is answered with an error result, not a failure, so a
"was it called" assertion passes on that bug.

## Assertions on model output

```text
Is it checking LLM-generated text?
├── No  → structural assertion (status, field presence, counts)
└── Yes → is the expected value deterministic?
          ├── Yes (an error message, a computed value) → .contains() is fine
          └── No  (natural language)                  → assert_response_semantic
```

```rust,ignore
// Flaky: the model may rephrase
assert!(text.contains("paris"));
assert!(text.contains("red") || text.contains("crimson"));

// Robust
assert_response_semantic(&client, "Asked for the capital of France", text,
    "Does this identify Paris as the capital of France?").await;

// Fine: deterministic
assert!(text.contains("3628800"));                 // factorial(10) from code execution
assert!(error.to_string().contains("Invalid input"));      // GenaiError::InvalidInput Display
```

`assert_response_semantic` asks the model, through structured output, whether
the text answers the question. It gives the validator call one transient
retry. A transient failure that survives the retry is tolerated and printed as
`SEMANTIC_VALIDATION_SKIPPED`; any other validator error panics. Call
`validate_response_semantically` directly only when you need the verdict as a
`Result`, for example inside a retry closure.

For forward compatibility, `response.has_unknown()`,
`response.unknown_steps()` and `response.step_summary().unknown_types` report
types the crate doesn't model.

### Skip markers

Two printed markers mean "passed without verifying":

| Marker | Meaning |
|--------|---------|
| `SEMANTIC_VALIDATION_SKIPPED` | The validator failed transiently (or returned no usable verdict), so the verdict was never obtained |
| `LIVE_TOOL_EVIDENCE_SKIPPED` | The interaction produced no evidence the tool ran, for a reason the test can't tell apart from a regression |

The `test-integration` job keeps passing-test output
(`--success-output=final`) and counts both markers per binary. Any marker
gives a warning; more than 3 fails the step (`release.yml` does the same).

A skip gets a marker when it can't tell a benign cause from a regression.
For example, an MCP call that returns no tool evidence looks the same whether
the model chose not to call the tool or the tool is broken. A skip whose
guard names the specific cause stays unmarked, so it doesn't annotate every
run. Examples are no API key, or a key that isn't allowlisted for computer
use: that guard needs both the tool name and an unavailability phrase.

## Serialization tests

Serialization is tested twice, on purpose:

| Layer | Where | Purpose |
|-------|-------|---------|
| Property | `src/proptest_tests.rs` | Random values round-trip, and Unknown variants keep their data. The strategies are in-crate so there is only one set to maintain |
| Example | `src/*_tests.rs`, `tests/wire_format_verification_tests.rs`, `tests/unknown_variant_tests.rs` | Documents specific wire shapes and catches regressions quickly |

Keep both: the examples are documentation, and they run fast. Both take
their expected shapes from the crate's own serde attributes, so neither can
catch a wrong wire key; the coverage oracle below can.

## Coverage oracle

"Full Interactions API coverage" is checked offline against a committed
snapshot of the API surface in `tests/fixtures/api_surface/`:

| File | What it is | Written by |
|------|------------|------------|
| `bindings.json` | google-genai's generated classes, every type reference resolved, plus the Files, File Search Store and Document classes from its `types.py` (camelCase on the wire), and its endpoints. The version is the one in `.github/last-swept-sdk-version` | `dump_bindings_schema.py --json` |
| `openapi-v1beta.json`, `openapi-v1.json` | The OpenAPI specs' schemas and endpoints without their prose, fetched 2026-11-01 (the published URL now 404s) | `snapshot_openapi.py` |

`tests/api_surface/` loads either source into one type model. The
checks on it:

- **Request keys** (`src/coverage_oracle/request/`,
  `request_wire_keys_exist_in_bindings`). Each request type
  (the interaction body in its model and agent forms, `GenerationConfig`,
  every `Tool`, `SafetySetting`, every `AgentConfig`, `SpeechConfig`, the
  response formats, the environment, and every resource's create and update
  body) is built fully populated as struct literals with no
  `..Default::default()`, so a new field does not compile until it is
  populated (the agent configs, whose fields are private, through every
  builder), plus proptest samples for the union arms and enum values. Every
  key it serializes to, at every depth, must exist on the binding class it
  is sent as, and every required key must be sent. A value carrying a
  union arm's tag is held to that arm, so a free-form arm (`ResponseFormat`'s
  `Dict[str, Any]`) cannot hide a wrong key. The reverse test
  (`request_classes_have_no_unsent_fields`) lists every field of a reached
  request class that nothing sends.
- **Request bodies on the wire** (`tests/request_wire_oracle.rs`). Each
  verb with a body runs against the local stub, and the captured body is
  checked the same way, wrapping included.
- **Response fields** (`src/coverage_oracle/response.rs`,
  `binding_fields_deserialize_without_extra` and
  `spec_fields_deserialize_without_extra`). For each
  response type, bodies are generated from the snapshot that together carry
  every field, union arm and enum value its class reaches (the generator
  fails if they do not). Each must
  deserialize with no key in an `extra` map, no `Unknown` variant, and no
  key dropped when the value is serialized again, including keys on an
  object that sits directly in a list (an annotation, a search result),
  which serde drops silently when the type keeps no `extra`. Each finding is
  traced to its `Class.field` by removing keys until it goes away. It runs
  against the bindings and the v1beta spec (whose `writeOnly` fields are
  left out of responses), and not under `strict-unknown`.
- **Completeness** (`src/coverage_oracle/mod.rs`,
  `every_binding_class_is_mapped`). The checks above only see the classes
  their tables reach, so every class in `bindings.json` must be reached by a
  request case or response target, or listed in `UNMAPPED` with its reason.
  `tests/ci_coverage.rs` (`every_api_endpoint_maps_to_a_handle_verb`) maps
  every endpoint in the bindings and the specs to the handle verbs that call
  it. A baseline bump that adds a resource, a body or an endpoint fails one
  of them.
- **Live coverage per handle** (`tests/ci_coverage.rs`,
  `every_resource_verb_is_called_live_per_handle`; the parsing is in
  `tests/live_verbs/`). Every public verb on every resource handle must be
  called, on a receiver of that handle's type, by a live test or a helper
  one reaches. Handles are found by their `#[must_use = "a resource handle
  does nothing until you call one of its methods"]` marker, and
  `resource_handles_carry_the_marker` fails when a `Client` accessor, or a
  handle method that is not a list builder, returns a struct without it.

### Reading a failure

A finding names a site: `Class.field` (`interactions.Interaction.status`
for the bindings, `Interaction.status` for the spec), with `=value` for an
enum value or a union tag. Either fix the crate, or add an entry with the
reason to the list that fits:

| List | For |
|------|-----|
| `ALLOWED` (request, response, wire) | Deliberate: a key the crate sends that the bindings lack but the API needs (`signature` on `function_call`), a deprecated or Vertex-only field (D-005), a value never served |
| `NOT_SENT` (request) | A request field nothing sends: deprecated, rejected live, or not modeled yet |
| `GAPS` (response) | Served or specified but not modeled yet, the work list for the next modeling pass |
| `UNMAPPED` (`mod.rs`) | A binding class no table reaches: SDK plumbing, an empty body, a body checked at the wire, or a request not modeled yet |
| `ENDPOINTS_WITHOUT_A_VERB`, `EXEMPT` (`tests/ci_coverage.rs`) | An endpoint no handle verb calls; a verb no live test can call |

Date every live fact in a reason (D-010). An entry that matches nothing
fails the test, so the lists only shrink when the crate catches up. A
`values()` override in `response.rs` replaces a synthesized value where the
snapshot's type is looser than what the API sends (a function's
`parameters`, a protobuf int64 sent as a string).

### Refreshing the snapshot

`api-surface-sweep` opens an issue when google-genai moves past the
baseline, and attaches the snapshot regenerated for the new release. To take
it:

```bash
.github/scripts/refresh_api_snapshot.sh --version X.Y.Z   # bumps the baseline and bindings.json together
cargo nextest run -E 'test(/coverage_oracle/) | binary(ci_coverage) | binary(request_wire_oracle)'
                                                          # every new field, class or endpoint fails until handled
```

Then update the `docs/INTERACTIONS_API_GAP.md` header (CI checks it matches
the baseline). With a newer copy of the OpenAPI spec, add
`--spec-v1beta FILE --spec-v1 FILE --spec-fetched YYYY-MM-DD`.

The committed `bindings.json` must match a fresh dump of the baseline
release, so a dump-script change needs a refresh too (run the script
without `--version`). `refresh_api_snapshot.sh --check` compares them
without writing; it runs as the sweep's last step, and in the
`shell-scripts` job on a PR that touches the dump script, the refresh
script, the baseline or `bindings.json`.
