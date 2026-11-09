# Testing Guide

How the test suite is laid out, how to run each part, and the rules new tests
follow.

## Test kinds

| Kind | Where | Needs a key | Run with |
|------|-------|-------------|----------|
| Unit | `#[cfg(test)]` modules and `src/*_tests.rs` | No | `make test` |
| Property (proptest) | `src/proptest_tests.rs` and `src/proptest_<feature>.rs` | No | `make test` (or `cargo test proptest`) |
| Offline HTTP | `tests/http_mock_tests.rs`, `tests/http_mock_resources.rs` (and `tests/http_mock_resources/`), `tests/reliability_tests.rs` | No | `make test` |
| Offline values | Builders and the macro: `tests/interaction_builder_tests.rs`, `tests/function_declaration_builder_tests.rs`, `tests/macro_tests.rs`. Serde against the documented wire: `tests/wire_format_verification_tests.rs`, `tests/unknown_variant_tests.rs`, `tests/round_trip_fidelity_tests.rs`, `tests/envelope_extra.rs`, and `tests/strict_unknown_tests.rs` (both modes) | No | `make test` (and `make test-strict`) |
| Captured responses | `tests/captured_fixtures.rs` over `tests/fixtures/captured/` (see [Captured fixtures](#captured-fixtures)) | No | `make test` |
| Coverage oracle | `src/coverage_oracle/`, `tests/request_wire_oracle.rs` (see [Coverage oracle](#coverage-oracle)) | No | `make test` |
| Guards | `tests/ci_coverage.rs` (with `tests/live_verbs/`, the workflow guards in `tests/ci_workflows/` and the doc guards in `tests/doc_guards/`), `tests/model_literals.rs`, `tests/module_size.rs` (D-014's size bars), `tests/non_exhaustive_responses.rs` (structs: `#[non_exhaustive]` and a flattened `extra`; enums: `#[non_exhaustive]`, an `Unknown` variant, and `extra` on each known variant of a union, each exemption named with its reason) | No | `make test` |
| CI scripts | `.github/scripts/tests/*.sh`, one harness per script, plus shellcheck, and `check_dated_facts.sh` over the checkout (D-032's homes, a reason above every `ignore` fence, and no Rust fence that nothing compiles) | No | `make test-scripts` (bash, jq, python3; PyYAML for the workflow parse check, skipped without it locally) |
| Compile-time (trybuild) | `tests/ui/` via `tests/ui_tests.rs` | No | `cargo test --test ui_tests` |
| Live integration | In `tests/`: `api_canary_tests.rs`, `binding_parity_tests.rs`, `credentials_tests.rs`, `environment_files_tests.rs`, `error_handling_tests.rs`, `file_search_stores_tests.rs`, `files_api_tests.rs`, `function_calling_tests.rs`, `interactions_api_tests.rs`, `multimodal_tests.rs`, `multiturn_tests.rs`, `processing_steps_tests.rs`, `streaming_multiturn_tests.rs`, `streaming_resume_tests.rs`, `temp_file_tests.rs`, `tool_service_tests.rs`, `tools_and_config_tests.rs`, `voices_tests.rs`, `webhooks_and_agents_tests.rs`. Each live test is `#[ignore = "Requires API key"]`; a file's offline tests run in `make test` | Yes | `make test-all` |
| Doctests | Rustdoc examples and the markdown guides (`#[doc = include_str!(..)]` in `src/lib.rs`) | No | `cargo test --workspace --doc --all-features` (CI only; `make` skips them) |
| Scripted harness | `tests/antigravity_scripted.rs`; each test is `#[ignore = "Requires localharness binary"]` | No, only `localharness` | See [Scripted harness tests](#scripted-harness-tests) |
| Harness | `tests/antigravity_harness.rs` | Yes, plus `localharness` | See [ANTIGRAVITY.md](ANTIGRAVITY.md) |
| Fuzz | `fuzz/` (cargo-fuzz targets over `src/fuzzing.rs`; failing inputs replayed by `src/fuzzing_tests.rs`) | No | `cargo +nightly fuzz run <target>` in `fuzz/`; see [fuzz/README.md](../fuzz/README.md) |

```bash
make test                                             # everything offline
make test-all                                         # + live tests (GEMINI_API_KEY)
cargo nextest run --test multiturn_tests --run-ignored all   # one live file
cargo nextest run -E 'test(/function_calling/)' --run-ignored all
LOUD_WIRE=1 cargo nextest run -E 'test(/name/)' --run-ignored all --no-capture -j 1
make test-strict                                      # strict mode: unknown Content/Step/string-enum values become errors
```

Strict mode is the `--cfg genai_strict_unknown` flag, not a Cargo feature,
because it is not additive (D-030): `make test-strict` runs the suite with
`RUSTFLAGS="--cfg genai_strict_unknown"`, in `target/strict` so the default
build is not evicted. Tests that exercise an `Unknown` value carry
`#[cfg(not(genai_strict_unknown))]`, since strict mode rejects it.

| Variable | Purpose |
|----------|---------|
| `GEMINI_API_KEY` | Required for live tests |
| `TEST_TIMEOUT_SECS` | Per-test budget (default 60) |
| `EXTENDED_TEST_TIMEOUT_SECS` | Budget for multi-turn tests (default 120) |
| `LOUD_WIRE=1` | Wire logging (no test installs a `tracing` subscriber that reads `RUST_LOG`, so it does nothing) |

## Offline HTTP tests

`ClientBuilder::with_base_url` points a real `Client` at a local stub
(`tests/common/http_stub.rs`, a small tokio server). The stub records every
request (`Recorded`: method, path and query, headers, body) and replays
canned `Reply`s: JSON, text, SSE chunks, extra headers, or a delay.

- **`http_mock_tests.rs`** covers client behavior that only shows on the
  wire: request shapes, error mapping, SSE framing, and the auto-function
  loop.
- **`reliability_tests.rs`** covers the default timeouts, how a cut-off
  stream is classified, the read of a stream's tail after its terminal
  event, and `Retry-After` in a poll.
- **`http_mock_resources.rs`** pins every resource endpoint on method, path
  (percent-encoding included), query and JSON body. It parses a realistic
  response, preserving `Unknown` and `extra`, and covers the wait helpers'
  success, failure and timeout paths. The file is the shared harness; the
  tests are one file per resource in `tests/http_mock_resources/` (plus
  `paging.rs`, `ids.rs` and `errors.rs`), declared from it with `#[path]`
  so they build into the one binary.
- **Known bugs**, when there are any, go in
  `tests/http_mock_resources/known_bugs.rs` as failing tests ignored with a
  `known bug: ...` reason until fixed. A subdirectory, because
  `ci_coverage.rs` scans only top-level files. Run them with
  `cargo nextest run --test http_mock_resources --run-ignored only`.

Prefer an offline test for anything that doesn't depend on model behavior;
it's fast and runs on every PR.

### Paused-clock tests against the stub

Tokio advances a paused clock to the next timer whenever the runtime is
idle, even with localhost I/O in flight, so any timer armed during a request
fires early: the client's default 30 s connect timeout fails most
paused-clock requests to the stub. This applies to your own tests of code
built on `genai-rs` too. Either build the test client with
`with_connect_timeout(Duration::MAX)`, which arms no connect timer, and set
no other timeout; or hold the clock with a running `spawn_blocking` task
until the stub has the request, as `common::http_stub::hold_clock` does
when a test needs the real defaults or a timeout of its own. For a test
that sends several requests (a poll, the auto-function loop),
`Stub::paused_clock_client()` holds the clock for each request until its
response is in (its body, or a stream's terminal event), with every
default in place. See [Reliability](RELIABILITY.md#tests-on-a-paused-clock).

A timing test asserts virtual time, never wall-clock time: a bound on
`std::time::Instant` holds on a quiet machine and flakes on a loaded runner
(#501). Hold the clock until the stub has the request, so that only the
timeout under test and the stub's delays are left as timers, and release
it from the stub's handler. When the test needs data to arrive before the
timeout can fire (a stream that stalls after its first event), keep the
clock held until that data is in hand: auto-advance can otherwise fire the
timeout while the bytes travel over localhost
(`read_timeout_ends_a_stalled_stream`).

A stream's tail, which the stub sends a delay after `Completed`, needs
the clock released at `Completed` (`paused_clock_client` does that), and
then a released clock can auto-advance past the tail to the 5 s tail limit
while the tail's bytes are in flight. So a paused-clock test can bound the
tail read from below (it waited for the tail) but not from above.
The exception is the one test whose point is the upper bound, that the
stream ends at the tail and not at the limit
(`stream_ends_once_the_tail_after_completed_has_arrived`, #518): it runs on
real time, with no per-chunk timeout to flake on a slow first poll, and
wide margins, a floor at the stub's delay and a ceiling ten times it.

### Captured fixtures

`tests/captured_fixtures.rs` replays real API responses through the client
against the stub. `.github/scripts/capture_fixtures.py` records them into
`tests/fixtures/captured/`, one file per exchange with its capture date:
a streamed create, a retrieval stream, a stored create and its read-back,
an environment, a voice listing, and a 400 and a 404 envelope. The checks
are on shape, not wording: every stream event parses and none is unknown,
the streamed text reassembles into the completed interaction, an envelope
maps to its status and message, and no captured key is dropped by a parse
and a serialize.

The weekly `captured-fixtures` job in `rust.yml` re-captures them from the
live API, replays the fresh copies, and uploads them as an artifact. A red
run there means the API now sends something the crate does not parse the
same way. The job cannot commit, so it also fails once a committed
fixture is more than 90 days old (`capture_fixtures.py --check-age 90`). To
refresh the committed copies, download the artifact (or run the script
with `GEMINI_API_KEY` set) and commit `tests/fixtures/captured/`.

## Scripted harness tests

`tests/antigravity_scripted.rs` runs the real `localharness` binary against
a scripted model instead of Gemini. `AgentBuilder::with_gemini_base_url`
points the harness's model calls at `tests/common/fake_gemini.rs`, a
`FakeGemini` built on the HTTP stub that answers request *n* with turn *n*
of a script (the last one repeats):

<!-- ignore: uses the test-only FakeGemini and ModelTurn from tests/common, which a doctest cannot reach -->
```rust,ignore
let fake = FakeGemini::start(vec![
    ModelTurn::call("view_file", json!({"AbsolutePath": path,
        "toolSummary": "File read", "toolAction": "Reading file"})),
    ModelTurn::text("read it"),
])
.await;
let mut agent = AntigravityAgent::builder()
    .with_api_key("scripted-placeholder-key")
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_gemini_base_url(fake.base_url())
    .add_workspace(dir)
    .spawn()
    .await?;
agent.chat("read the file").await?;
// What the harness told the model the call returned:
let told = fake.function_responses(1);
```

The script decides what the "model" does, so the assertions are exact: the
harness's own builtins, hooks, policies, workspace containment, triggers,
cancellation and error handling run for real, and the test checks what
reached the model (`function_responses`, `request_json`) and what the bridge
reported. `ModelTurn::error(400, ..)` fails the backend call, and
`.delayed(..)` holds a reply back (for cancellation and deadline tests).
When the harness makes model calls of its own besides the agent's (the auto
policy's assessor, compaction), `FakeGemini::routed(|request, n| ..)`
answers by request rather than by position, and `system_prompt(n)` and
`tool_names(n)` show what call *n* carried.

The Python SDK options the bridge models are proved in
`tests/antigravity_scripted/` (options, subagents, lifecycle hooks,
policies, triggers). `parity.rs` holds the guard on the parity table in
`docs/ANTIGRAVITY_BRIDGE_DESIGN.md`: `.github/scripts/dump_sdk_options.py`
reads the installed wheel's sources with `ast` (never importing them), and
the test fails when the wheel exposes an option the table lacks or the
table names one the wheel does not have. The script itself fails on
anything the SDK exposes that it neither reads nor skips with a reason: a
module (its `MODULE_SKIP`), a public class in a module it reads (its
`CLASS_SKIP`), or an exported name (`__all__`, a package's re-exports,
`@beta`). So a new config model, content type or helper cannot slip past
the table. `.github/scripts/tests/test_dump_sdk_options.sh` plants each of
these in a synthetic SDK.

The suite needs no API key and takes a few seconds, so CI runs it on every
PR, forks included, as a step of the required `Test Suite (antigravity)`
job, with `GEMINI_API_KEY` blanked. Locally, with the pinned wheel
installed (its venv first on `PATH`, or `ANTIGRAVITY_HARNESS_PATH` set):

```bash
cargo nextest run --features antigravity --run-ignored all -E 'binary(antigravity_scripted)'
```

Write a scripted test, not a live one, for anything about the bridge or the
harness rather than about a model's choices. Function-call arguments use
the harness's tool-schema names (`AbsolutePath`, `CommandLine`), which a
`LOUD_WIRE` trace or `fake.request_json(0)["tools"]` shows. Two harness
behaviors to know: it allows the system temp directory besides the
workspaces, so a file meant to be outside them goes under
`CARGO_TARGET_TMPDIR`; and a turn with a backgrounded `run_command` does not
finish until the command does. A test of the turn deadline over a
synchronous tool that blocks needs
`#[tokio::test(flavor = "multi_thread", worker_threads = 2)]`: only workers
drive the timer, and on `#[tokio::test]`'s current-thread runtime, or a
multi-thread one that gets a single worker on a one-CPU runner, the blocked
body holds the only one. A deadline test also records when the tool or hook
it blocks on starts, and retries on a fresh agent with a larger budget when
the deadline fired first (`DEADLINE_BUDGETS`): a slow runner may not reach
that work within a short budget, and this suite is a required check, so it
must not flake.

## Live tests

### Rules

- **Ignore reason.** A live test is `#[ignore = "Requires API key"]`, that
  exact string. `tests/ci_coverage.rs` fails if any top-level `tests/*.rs`
  file uses another reason, except the two Antigravity suites, whose
  key-free tests are `#[ignore = "Requires localharness binary"]`
  (`antigravity_harness` and `antigravity_scripted`, which run in the
  `test-antigravity` job rather than the matrix).
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
  Where it has none (a config knob), read the stored interaction back with
  `stored_echo` and check it holds what was sent with `assert_stored`: a 200
  on create shows a field was accepted, not applied, and create responses
  drop fields the stored record keeps (#476, #492).
- **Mark what you create.** Every id, display name or inline environment
  source a live test chooses comes from `test_resource_name(label)` or
  `test_source_content(text)`, which carry the `genai-rs-test` prefix (and,
  in the name, the creation time). Clean up in the test, including on a
  panic, and cancel and delete background interactions. What a dead test
  leaves behind, the daily [janitor](#leaked-test-resources) finds, but only
  if it carries the marker; it deletes it only once the key owner has opted
  in (`JANITOR_DELETE`).

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
in strict mode.

### Template

<!-- ignore: an integration-test file; its `mod common` and helpers exist only under tests/ -->
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

### Helpers (`tests/common/`)

`mod.rs` re-exports its submodules, so every helper is `common::<name>`:
the retry helpers are in `retry.rs`, the stream consumers in `streams.rs`,
the semantic checks in `semantic.rs` and the fixture data in `fixtures.rs`.

| Helper | Purpose |
|--------|---------|
| `get_client()` | `Some(Client)` when `GEMINI_API_KEY` is set |
| `interaction_builder(&client)`, `stateful_builder(&client)` | Builders preset with `DEFAULT_MODEL` (the latter with storage on) |
| `retry_request!([vars] => { ... })`, `retry_on_transient(n, ..)` | Retry transport errors (`is_retryable()`), the model-side flakes in `is_transient_error`, and an interaction `is_transient_failure` matches |
| `with_timeout(test_timeout(), ..)`, `extended_test_timeout()` | Bound a test |
| `consume_stream(..)`, `consume_auto_function_stream(..)` | Collect a stream's text and final response; they panic on the first stream error |
| `poll_until_done(..)` | Poll a background interaction |
| `assert_response_semantic(..)`, `validate_response_semantically(..)` | Semantic checks (below) |
| `get_weather_function()`, `get_time_function()` | Shared declarations |
| `test_resource_name(label)`, `test_source_content(text)`, `TEST_RESOURCE_PREFIX` | Names and fixture content the leaked-resource janitor recognizes |
| `stored_echo(client, id, pointer)`, `assert_stored(what, sent, stored)` | Read a stored interaction back at a JSON pointer, and check it holds everything sent. The read returns a `Result`, so cancel and delete before asserting |
| `TINY_RED_PNG_BASE64`, `TINY_BLUE_PNG_BASE64`, `TINY_WAV_BASE64`, `TINY_MP4_BASE64`, `TINY_PDF_BASE64` | Minimal valid media. The MP4 is one second: shorter clips yield no sampled frame and are rejected |

`is_transient_error` matches Spanner UTF-8 errors and three 400s (`invalid
json syntax`, `there was a problem processing your request`, and `please
retry the request`, the API's advice on `malformed_tool_call` when it
reported one as a `400`), but never `too_many_tool_calls`, the url_context
loop, whatever its message says; see
[Error Handling](ERROR_HANDLING.md#known-transient-errors). A broken tool
call now comes back as a `200` instead: an interaction whose `status` is
`failed` and whose `errors` carry `malformed_tool_call`
([live record](INTERACTIONS_API_GAP.md#tools)). `retry_on_transient`, and
so `retry_request!`, retries that too (`is_transient_failure`), when the
operation returns an `InteractionResponse` or an `AutoFunctionResult`, and
never when an error names `too_many_tool_calls`; after the last attempt it
returns the failed interaction, for the test's own status check to report.
A validation rejection still fails on the first attempt.

For the auto-function loop, assert that executions **succeeded**, not just
that they happened: `result.all_executions_succeeded()`, with
`result.failed_executions()` in the message. A declared function with no
implementation is answered with an error result, not a failure, so a
"was it called" assertion passes on that bug.

### Flakes and retries

Every live nextest step sets `NEXTEST_RETRIES: 2` in its env, never a
`--retries` flag (`tests/ci_workflows/` checks both), so a test that fails
and then passes leaves its job green. The example smoke loops retry by
hand, since `cargo run` has none, and print `EXAMPLE_RETRIED <name> <failed
tries>` when an example passed only on a retry. The daily
`ci-flakiness-report.yml` reads the retry jobs' logs from every finished run
of the week, green ones included, and counts each test nextest marked
`FLAKY` and each retried example (`.github/scripts/count_retried_flakes.sh`).
It finds those jobs by name (`RETRY_JOBS`), which `tests/ci_workflows/`
ties to the `rust.yml` jobs that retry. Only the steps that retry count
toward the rate, read from the `NEXTEST_RETRIES` in the env GitHub echoes
into the log, so the antigravity job's offline suite, which shares its
log, stays out of it.

So a live test makes no retries of its own beyond `retry_request!`'s
transport and transient errors (D-052): the report counts a retry
nextest makes, and never sees one inside the test. When a known model or
server failure has a cause the test controls, remove it: send the tools a
follow-up turn needs, since they are not carried over, or a request shape
the server handles, found by live probes and recorded with its numbers in
the [gap tracker](INTERACTIONS_API_GAP.md) (the url_context test's low
thinking). Otherwise let the test fail and nextest retry it. Two repeats
are not flake retries, since neither repeats a failure of the feature under
test: a deadline retry for a slow runner (`DEADLINE_BUDGETS` in
`antigravity_scripted.rs`), and a redraw when the server's output lacks
what the assertion needs (`test_stream_resume_with_last_event_id` draws up
to three interactions for an event id with text on both sides).

The rolling report lists retried tests and examples beside failed ones,
with the week's rate: tests that passed only on a retry, over live test
runs. The report fails, and so escalates like any red scheduled workflow,
when the week breaches a target (D-046):

- the rate is 1% or more;
- no live test runs were read, so there is no rate (shown as `n/a`, not
  `0.00%`);
- a chronic flake: a test or example that passed only on a retry in more
  than 5% of the runs it ran in, and in at least two. The report lists
  each with its share. A low rate across the suite can hide one test that
  needs a retry every few runs.

The report lists the week's runs by creation date, up to 1,000 (about 115
a week now); a listing that reaches the limit fails the report rather than
leave runs out. It also adds up the [skip markers](#skip-markers) the retry
jobs printed across the week, with the tests that printed them: each job
budgets only its own run's, so a test that skips its check in run after
run shows only here. That row is reported, not a target.

Failed runs are split by event. Only main's (push, schedule, dispatch) are
categorized; a pull request's red run is usually the change under review,
so it is counted, not called a flake. A test counts as failed only when its
last try failed, and one that failed every try in jobs named for one OS
alone, and in every leg of that OS (`Cross Platform (windows-latest,
stable)` and `(windows-latest, beta)`), is labelled deterministic: a break
on that OS, not a flake (`.github/scripts/categorize_failures.sh`). A test
that fails on one leg while the other passes it is a flake.
No CI run stops at its first failure: `.config/nextest.toml` turns
fail-fast off for the default profile, which every job uses.

### Leaked test resources

Uploaded files expire 48 hours after upload, and a test deletes the
stored interactions it creates itself (they cannot be listed). Nothing else
a live test creates expires on its own: the shared key has held
environments for months (D-033). `test-resource-janitor.yml` runs
`.github/scripts/cleanup_test_resources.py` daily over resources that
carry a live test's marker and are more than six hours old: credentials
and agents whose id starts with `genai-rs-test`, triggers, file search
stores (with their documents) and prompted voices whose display name does,
webhooks on the test endpoint, and environments whose sources are all
test fixtures. Anything else on the key is left alone, with one exception.

Environments have no name, label or metadata a test can set (neither the
bindings' `Environment` nor the live listing has one; D-033), so only
their inline sources carry a marker. A test's environment with no sources
(an agent interaction run without one, a fork, a network-only create)
looks exactly like anyone else's. The janitor takes a sourceless environment
when it holds no files and has not been created, updated or used within
the six hours, **whoever made it**: opting in deletes every empty, idle,
sourceless environment on the key, not only the tests' (D-033). A
sourceless environment that holds files, such as a leaked fork, is kept
and listed in the summary for a person to check. D-033 records what the
shared key held when the rule was set. To see what it would delete now:

```bash
python3 .github/scripts/cleanup_test_resources.py --dry-run
```

Deleting is the key owner's opt-in. A scheduled run is a dry run that
lists what it would delete in the job summary, unless the repository
variable `JANITOR_DELETE` is `true` (Settings, Secrets and variables,
Actions, Variables); read the latest scheduled run's summary before
setting it. A manual run follows its `mode` input whatever the variable
says, and is a dry run unless `mode` is `delete`. The harness,
`.github/scripts/tests/test_cleanup_test_resources.sh`, replays recorded
listings through a local fake API and asserts the exact set of deletes in
each mode, including that a scheduled run deletes nothing without the
opt-in, and that a sourceless environment that holds files or was used
within the floor survives.

## Assertions on model output

```text
Is it checking LLM-generated text?
├── No  → structural assertion (status, field presence, counts)
└── Yes → is the expected value deterministic?
          ├── Yes (an error message, a computed value) → .contains() is fine
          └── No  (natural language)                  → assert_response_semantic
```

<!-- ignore: fragments that use tests/common's assert_response_semantic and a test's bindings -->
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
| `SEMANTIC_VALIDATION_SKIPPED` | The validator failed transiently (or returned no usable verdict), so the verdict was never obtained, or the test had nothing to check it against (the Antigravity question test, when the model asked no multiple-choice question) |
| `LIVE_TOOL_EVIDENCE_SKIPPED` | The interaction produced no evidence the tool ran, for a reason the test can't tell apart from a regression |

Every live nextest step keeps passing-test output
(`--success-output=final`) and counts both markers: the `test-integration`
job per binary, and the Antigravity harness step in `test-antigravity` and
in `release.yml`. Any marker gives a warning; more than 3 fails the step
(10 for `release.yml`'s whole-workspace run). `tests/ci_workflows/` checks
that every live step does this, and the flakiness report adds the markers
up across the week.

A skip gets a marker when it can't tell a benign cause from a regression.
For example, an MCP call that returns no tool evidence looks the same whether
the model chose not to call the tool or the tool is broken. A skip whose
guard names the specific cause stays unmarked, so it doesn't annotate every
run, such as a run with no API key.

## Serialization tests

Serialization is tested twice, on purpose:

| Layer | Where | Purpose |
|-------|-------|---------|
| Property | `src/proptest_tests.rs` | Random values round-trip, and Unknown variants keep their data. The strategies are in-crate so there is only one set to maintain: shared ones in `src/proptest_tests.rs`, the rest with their properties in `src/proptest_<feature>.rs` (`content`, `steps`, `request`, `tools`, `response`, `resources`). `src/proptest_stream.rs` streams every step with a delta form (its `step.start`, the step split over `step.delta` events, `step.stop`) through `dispatch_stream_event` and checks the completed response carries the step a non-streamed one would, so a stream accumulator arm that drops what its delta carries fails offline. `src/proptest_unknown_fields.rs` injects an unknown key into every object of every known `Step`, `StepDelta`, `Content` and `Annotation` variant and checks it survives (#491); it does the same over a whole generated `InteractionResponse`, reaching every known `Tool`, `ResponseFormat`, `TranscriptionMode` and `ToolChoice` variant a stored interaction serves back, except under the keys `NOT_INJECTED_UNDER` lists with their reasons (#504) |
| Example | `src/*_tests.rs`, `tests/wire_format_verification_tests.rs`, `tests/unknown_variant_tests.rs` | Documents specific wire shapes and catches regressions quickly |

Keep both: the examples are documentation, and they run fast. Both take
their expected shapes from the crate's own serde attributes, so neither can
catch a wrong wire key; the coverage oracle below can.

## Line coverage

The Code Coverage job in `rust.yml` measures the workspace with the
`antigravity` feature: the offline suite, then the key-free scripted
harness suite, merged into one report that goes to the job summary on
every run. The job then fails when line coverage is under its floor
(`--fail-under-lines`), set just under the value measured when it was
last raised (D-058): raise it in `rust.yml` as coverage grows, and
lower it only with the reason in the commit. `tests/ci_workflows/coverage.rs`
checks the job keeps all of this.

## Coverage oracle

"Full Interactions API coverage" is checked offline against a committed
snapshot of the API surface in `tests/fixtures/api_surface/`:

| File | What it is | Written by |
|------|------------|------------|
| `bindings.json` | google-genai's generated classes, every type reference resolved; its endpoints, each with its query parameters; the `Api-Revision` it pins; and from its hand-written Files, File Search Store, Document and Operation modules, their endpoints and the `types.py` classes their methods' signatures name (camelCase on the wire). The version is the one in `.github/last-swept-sdk-version` | `dump_bindings_schema.py --json` |
| `openapi-v1beta.json`, `openapi-v1.json` | The OpenAPI specs' schemas and endpoints without their prose; the fetch date and the source URL are in the file. The daily sweep compares what that URL serves with the snapshot, and opens an issue on a drift or a dead URL (`watch_spec_urls.sh`; the URL's record is in the [gap tracker](INTERACTIONS_API_GAP.md#how-to-verify-api-surface)) | `snapshot_openapi.py` |

`tests/api_surface/` loads either source into one type model. The
checks on it:

- **Request keys** (`src/coverage_oracle/request/`,
  `request_wire_keys_exist_in_bindings`). Each request type
  (the interaction body in its model and agent forms, `GenerationConfig`,
  every `Tool`, `SafetySetting`, every `AgentConfig`, `SpeechConfig`, the
  response formats, the environment, and every resource's create and update
  body) is built fully populated as struct literals with no
  `..Default::default()`, so a new field does not compile until it is
  populated (the agent configs, whose fields are private, through each
  type's `fully_populated()` literal beside it, and
  `agent_configs_set_every_field` fails on a field there that sends no
  key), plus proptest samples for the union arms and enum values.
  `request_values_are_full_literals` (`request/fills.rs`) fails on a
  `default()` fill, other than a whole `extra` map, in those literals and
  in every strategy the oracle samples. An
  `extra` map (`InteractionRequest::extra` included) stays empty: its keys
  are the caller's, not the crate's, and would count as sent in the reverse
  test (`a_populated_extra_is_sent_flat_and_not_vouched_for`). So do the
  `extra` and `arguments_extra` maps of history steps and content, which
  hold the unmodeled fields a response carried; a populated one is sent
  flat and its keys are checked as its class's own
  (`a_populated_variant_extra_is_sent_flat_and_not_vouched_for`). Every
  key it serializes to, at every depth, must exist on the binding class it
  is sent as, and every required key must be sent. A value carrying a
  union arm's tag is held to that arm, so a free-form arm (`ResponseFormat`'s
  `Dict[str, Any]`) cannot hide a wrong key. The reverse test
  (`request_classes_have_no_unsent_fields`) lists every field of a reached
  request class that nothing sends.
- **Request bodies on the wire** (`tests/request_wire_oracle.rs`). Each
  verb with a body runs against the local stub, and the captured body is
  checked the same way, wrapping included. `every_verb_with_a_body_is_called_above`
  finds the verbs that send a body (their implementation reaches a POST,
  PATCH or PUT request; the parsing is in `tests/live_verbs/`) and fails
  when one is not called there or listed in `NO_JSON_BODY`.
- **Query strings on the wire** (`tests/request_wire_oracle.rs`,
  `every_query_key_a_verb_sends_is_a_binding_query_parameter`). Every verb
  of every resource handle runs against the stub with every list builder
  option and every request type's query option set
  (`every_verb_is_called_by_the_query_test` fails on a verb not called, an
  option not set on its verb's builder, or a `with_*` method not called
  whose field a verb sends on the query string, such as
  `WebhookUpdate::with_update_mask`; those fields are found by tracing each
  `with_query` value back through `src/`, and a value the trace cannot
  follow fails the test too), and each request is
  matched to its endpoint in `bindings.json`. A key the endpoint does not
  take fails (a misspelling, a key the API dropped), and so does a binding
  query parameter no verb sends (one a release adds) and a request to a
  path no endpoint takes. A template that keeps a full resource name
  (`{operationName}`) matches only the names `FULL_NAMES` gives it.
- **Response fields** (`src/coverage_oracle/response.rs`,
  `binding_fields_deserialize_without_extra` and
  `spec_fields_deserialize_without_extra`). For each
  response type, bodies are generated from the snapshot that together carry
  every field, union arm and enum value its class reaches (the generator
  fails if they do not). Each must
  deserialize with no key in an `extra` map, no `Unknown` variant, and no
  key dropped when the value is serialized again. Every known variant of
  `Step`, `StepDelta`, `Content` and `Annotation` keeps an unmodeled key in
  its `extra` (a call's `arguments` keys in `arguments_extra`), as do the
  result items, places, review snippets, step errors and static video
  segments in them (#491), and the known variants of `Tool`,
  `ResponseFormat` and `TranscriptionMode` and `ToolChoice`'s
  `allowed_tools` object, which a stored interaction serves back (#504). So
  a new field there is reported as landing in `extra`
  (`a_new_field_on_a_known_variant_is_reported`). A type with no
  `extra` drops the key, which is reported too, including on an object that
  sits directly in a list (`a_dropped_field_on_a_list_element_is_reported`).
  Each finding is
  traced to its `Class.field` by removing keys until it goes away. It runs
  against the bindings and both specs, v1beta and v1 (whose `writeOnly`
  fields are left out of responses), and not in strict mode. v1 defines
  only the interaction schemas, with a few values of its own, which
  `v1_only_values_are_checked` shows its bodies carry.
- **Sparse response bodies** (`src/coverage_oracle/response_minimal.rs`,
  `sparse_binding_bodies_deserialize` and `sparse_spec_bodies_deserialize`).
  The bodies above set every field, so a type that demands a field the
  snapshot makes optional passes there and fails on a served object without
  it: `Tool::Function` read a function tool with no `description` as
  `Unknown`, and a stored interaction serves exactly that. Here
  every class a response type reaches is cut, one at a time, to its required
  fields and its constant discriminators (`type`, `event_type`), and the
  body must pass the same checks. A finding is traced to the optional field
  whose absence alone causes it (`a_required_optional_field_is_reported`).
- **Completeness** (`src/coverage_oracle/mod.rs`,
  `every_binding_class_is_mapped`). The checks above only see the classes
  their tables reach, so every class in `bindings.json` must be reached by a
  request case or response target, or listed in `UNMAPPED` with its reason;
  its twin, `every_spec_schema_is_mapped`, holds every schema of both specs
  to a response target or `SPEC_UNMAPPED`.
  `tests/ci_coverage.rs` (`every_api_endpoint_maps_to_a_handle_verb`) maps
  every endpoint in the bindings and the specs to the handle verbs that call
  it, and `api_revision_copies_match_the_bindings` holds the crate's
  `API_REVISION` and every copy of it to the revision the bindings pin. A
  baseline bump that adds a resource, a body or an endpoint, or moves the
  revision, fails one of them.
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
| `SPARSE` (`response_minimal.rs`) | An optional field the crate cannot read its class without, with why that is safe |
| `UNMAPPED` (`mod.rs`) | A binding class no table reaches: SDK plumbing, an empty body, a body checked at the wire, or a request not modeled yet |
| `SPEC_UNMAPPED` (`mod.rs`) | A spec schema no response target reaches: a request body (checked against the bindings), the SSE frame, a raw media body |
| `ENDPOINTS_WITHOUT_A_VERB`, `EXEMPT` (`tests/ci_coverage.rs`) | An endpoint no handle verb calls; a verb no live test can call |
| `NO_JSON_BODY` (`tests/request_wire_oracle.rs`) | A verb whose POST, PATCH or PUT carries no JSON body (an upload's bytes) |
| `QUERY_ALLOWED`, `NOT_SENT` (`tests/request_wire_oracle.rs`, by `METHOD /path?key`) | A query key the crate sends that its endpoint does not list (the `alt` system parameter); a binding query parameter no verb sends |

Date every live fact in a reason (D-010): `live_claims_are_dated` fails on
a reason that says what the API served, rejected or accepted, or quotes an
error status, without a `YYYY-MM` date. The in-crate oracle holds its lists
to it, and `tests/ci_coverage.rs` every reasoned list in `tests/` (each
`const` slice of tuples ending in a `&str` reason), so a list a test adds
is checked too. An entry
that matches nothing fails the test, so the lists only shrink when the
crate catches up. A `values()` override in `response.rs` replaces a
synthesized value where the snapshot's type is looser than what the API
sends (a function's `parameters`, a protobuf int64 sent as a string).

### Refreshing the snapshot

`api-surface-sweep` opens an issue when google-genai moves past the
baseline, and attaches the snapshot regenerated for the new release. To take
it:

```bash
.github/scripts/refresh_api_snapshot.sh --version X.Y.Z   # bumps the baseline and bindings.json together
cargo nextest run -E 'test(/coverage_oracle/) | binary(request_wire_oracle) | binary(ci_coverage)'
                                                          # every new field, class or endpoint fails until handled
```

Then update the `docs/INTERACTIONS_API_GAP.md` header (CI checks it matches
the baseline). With a newer copy of an OpenAPI spec, add `--spec-v1beta
FILE` or `--spec-v1 FILE`, with `--spec-source URL`, the URL it was
downloaded from (it defaults to `manual`), and `--spec-fetched
YYYY-MM-DD` (it defaults to today). One `--spec-source` is recorded for
every spec in the run, and the two specs have their own URLs, so refresh
them in two runs. The baseline is written last, so a failed download or
dump leaves it and the snapshot unchanged.

The committed `bindings.json` must match a fresh dump of the baseline
release, so a dump-script change needs a refresh too (run the script
without `--version`), and the file is never edited by hand.
`refresh_api_snapshot.sh --check` checks it without writing. Its offline
half fails when the snapshot's `sdk_version` is not exactly the baseline
(a bump made without the script) or the snapshot has no classes,
endpoints or `api_revision`; `--check --offline` stops there, and `make test-scripts` runs it
on every push. The full check then compares the file with a fresh dump,
which catches a hand edit; it runs as the sweep's last step every day, and
in the `shell-scripts` job on a PR that touches the dump script, the
refresh script, the baseline or `bindings.json`.
