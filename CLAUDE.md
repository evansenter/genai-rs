# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## External Gemini API Documentation

**Important**: When working on API integration or troubleshooting, consult these authoritative sources:

| Document | URL |
|----------|-----|
| Interactions API Reference | https://ai.google.dev/static/api/interactions.md.txt |
| Interactions API Guide | https://ai.google.dev/static/api/interactions-api.md.txt |
| Function Calling Guide | https://ai.google.dev/gemini-api/docs/function-calling.md.txt |
| Thought Signatures | https://ai.google.dev/gemini-api/docs/thought-signatures.md.txt |

## Project Overview

`genai-rs` is a Rust client library for Google's Generative AI (Gemini) API using the **Interactions API** for unified model/agent interactions.

**Workspace structure:**
- **`genai-rs`** (root): Public API crate with user-facing `Client`, `InteractionBuilder`, and all type modules
- **`genai-rs-macros/`**: Procedural macro for automatic function declaration generation

## Development Commands

Use the Makefile for common operations. Requires [cargo-nextest](https://nexte.st/).

```bash
make check     # Pre-push gate: fmt + clippy + test
make test      # Unit tests only (excludes doctests for speed)
make test-all  # Full suite including integration tests (requires GEMINI_API_KEY)
make fmt       # Check formatting
make clippy    # Lint with warnings as errors
make docs      # Build docs with warnings as errors
make clean     # Clean build artifacts
```

### Testing

**Default**: Always run `make test-all` for full integration testing.

**Note**: Doctests run only in CI (`cargo test --workspace --doc`), not via `make test` or `make test-all`. This is intentional—doctests add compile overhead and CI catches them.

```bash
make test-all                                    # Integration tests (requires GEMINI_API_KEY)
make test                                        # Unit tests only
cargo nextest run -E 'test(/test_name/)'         # Single test by name
cargo nextest run --test integration_file        # Single integration test file
```

**Environment**: `GEMINI_API_KEY` required for integration tests. Tests take 2-5 minutes; some may flake due to LLM variability.

### Nextest vs Cargo Test Flags

| Purpose | cargo test | cargo nextest |
|---------|-----------|---------------|
| Include ignored | `-- --include-ignored` | `--run-ignored all` |
| Single test | `test_name` | `test_name` (or `-E 'test(/regex/)'`) |
| Release mode | `--release` | `--cargo-profile release` |
| Show output | `-- --nocapture` | `--no-capture` |

### Quality Checks

```bash
make check  # Run all quality gates (fmt + clippy + test)

# Or individually:
cargo fmt -- --check                                                 # Check format
cargo clippy --workspace --all-targets --all-features -- -D warnings # Lint
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items  # Docs
```

### Examples

All require `GEMINI_API_KEY`. Key examples:
- `cargo run --example simple_interaction` - Basic usage
- `cargo run --example auto_function_calling` - Function calling
- `cargo run --example streaming` - SSE streaming

See `examples/` for full list (multimodal, thinking, files API, image generation, etc.)

## Architecture

### Layered Design

1. **Public API** (`src/lib.rs`, `src/client.rs`, `src/request_builder/`): User-facing `Client`, `InteractionBuilder`
2. **Internal Logic** (`src/function_calling.rs`, `src/interactions_api.rs`, `src/multimodal.rs`): Function registry, content builders
3. **HTTP Layer** (`src/http/`): Raw API requests, SSE streaming (internal, `pub(crate)`)
4. **Type Modules** (`src/content.rs`, `src/request.rs`, `src/response.rs`, `src/tools.rs`): JSON models
5. **Macros** (`genai-rs-macros/`): `#[tool]` macro with `inventory` registration

### Key Patterns

**Builder API**: Fluent builders throughout (`Client::builder()`, `client.interaction().with_*()`)

**Function Calling** - Two categories:

| Category | Tools | Who Executes |
|----------|-------|--------------|
| Client-Side | `#[tool]` macro, `ToolService`, Manual | YOUR code |
| Server-Side | Google Search, Code Execution, URL Context | API |

**Choosing Client-Side Approach**:
| Approach | Registration | State | Best For |
|----------|-------------|-------|----------|
| `#[tool]` macro | Compile-time | Stateless | Simple tools, clean code |
| `ToolService` | Runtime | Stateful | DB pools, API clients, dynamic config |
| Manual handling | N/A | Flexible | Custom execution logic, rate limiting |

**Function Calling Modes**:
| Mode | Behavior |
|------|----------|
| Auto (default) | Model decides whether to call functions |
| Any | Model must call a function |
| None | Function calling disabled |
| Validated | Schema adherence for both calls and natural language |

**Multi-Turn Inheritance Rules** (critical gotcha):
| Field | Inherited by API? | SDK Behavior |
|-------|-------------------|--------------|
| `systemInstruction` | ❌ No | Available on all interactions; set explicitly per-turn if needed |
| `tools` | ❌ No | Must resend on every new user message turn |
| Conversation history | ✅ Yes | Automatically included |

**Debugging**: Use `LOUD_WIRE=1` to see wire-level request/response details.

**Comprehensive Guides** (see `docs/`):
- `docs/MULTI_TURN_FUNCTION_CALLING.md` - Stateful/stateless, auto/manual execution, thought signatures
- `docs/STREAMING_API.md` - Stream types, resume capability, auto-function streaming
- `docs/LOGGING_STRATEGY.md` - Log levels, sensitive data handling
- `docs/ENUM_WIRE_FORMATS.md` - Wire formats and Unknown variant catalog

### Error Types

- `GenaiError`: API/network errors (thiserror-based), defined in `src/errors.rs`
- `FunctionError`: Function execution errors

## Core Design Philosophy: Evergreen Soft-Typing

This library follows the [Evergreen spec](https://github.com/google-deepmind/evergreen-spec) philosophy: **unknown data should be preserved, not rejected**.

### Key Principles

1. **Graceful Unknown Handling**: Unrecognized API types deserialize into `Unknown` variants
2. **Non-Exhaustive Enums**: Use `#[non_exhaustive]` on enums that may grow
3. **Preserve Data Roundtrip**: `Unknown` variants serialize back with original data intact
4. **Continue on Unknown Status**: When polling, continue on unrecognized status (use timeouts)

### Standard Unknown Variant Pattern

All enums use consistent naming - field names follow `<context>_type` (e.g., `content_type`, `tool_type`, `status_type`):

```rust
Unknown {
    <context>_type: String,      // The unrecognized type from API
    data: serde_json::Value,     // Full JSON preserved for roundtrip
}
```

Helper methods: `is_unknown()`, `unknown_<context>_type()`, `unknown_data()`

**When adding enums with Unknown variants**, implement all three helper methods:
- `fn is_unknown(&self) -> bool`
- `fn unknown_<context>_type(&self) -> Option<&str>`
- `fn unknown_data(&self) -> Option<&serde_json::Value>`

See `Content` in `src/content.rs` for reference implementation.

**When adding/updating enums**: Always update `docs/ENUM_WIRE_FORMATS.md` with verified wire format and Unknown variant info. Test with `LOUD_WIRE=1` to confirm actual API format.

**Wire format field naming**: The Gemini Interactions API uses **snake_case** for field names. If the API appears to accept both camelCase and snake_case, always use snake_case in our serialization. Verify actual wire format with `LOUD_WIRE=1` before assuming documentation is correct.

## Test Organization

- **Unit tests**: Inline in source files
- **Integration tests** (`tests/`): Require `GEMINI_API_KEY` for most; see file names for categories
- **Property-based tests** (proptest): Serialization roundtrip verification
  - `src/proptest_tests.rs`: Strategy generators
  - `tests/proptest_roundtrip_tests.rs`: Integration proptests

**Test conventions**:
- Use `#[ignore = "Requires API key"]` (exact format) for tests needing `GEMINI_API_KEY`
- New public constructors need unit tests, not just integration tests (e.g., `Content::from_file()` should have unit tests for MIME type inference logic)

### Test Assertion Strategies

- **Structural**: Verify API mechanics (status, field presence) - default for most tests
- **Semantic**: Use `validate_response_semantically()` for behavioral tests (adds ~1-2s API call)
- **Avoid**: Brittle `text.contains("word")` assertions on LLM output - responses vary

**Decision rule**: Is it checking LLM text content with a non-deterministic expected value? → Use semantic validation.

```rust
// BAD - LLM might rephrase
assert!(text.contains("paris"));
assert!(text.contains("red") || text.contains("crimson"));

// GOOD - Handles natural language variability
validate_response_semantically(&client, context, text, "Does this identify Paris?").await?;

// OK - Deterministic values (error messages, code execution results)
assert!(text.contains("3628800"));  // factorial(10) - exact computed value
assert!(error.to_string().contains("invalid"));  // library error message
```

See `docs/TESTING.md` for the full decision flowchart and examples.

## CI/CD

GitHub Actions runs: check, test, test-strict-unknown, test-integration (5 matrix groups), fmt, clippy, doc, msrv, cross-platform, coverage, build-metrics, ci-flakiness-report (daily). Security audits run in separate `audit.yml` workflow (on Cargo.toml/lock changes + weekly). Integration tests require same-repo origin (protects API key). Release validation includes full integration test suite.

## Project Conventions

- **Model name**: Always use `gemini-3-flash-preview` (exceptions: `gemini-3-pro-image-preview` for image generation, `gemini-2.5-pro-preview-tts` for text-to-speech)

### Naming Conventions

**Builder method prefixes** (see `docs/BUILDER_API.md` for complete reference):

| Prefix | Behavior | Example |
|--------|----------|---------|
| `with_*` | **Configures** a setting (replaces if called twice) | `with_model()`, `with_text()` |
| `add_*` | **Accumulates** items to a collection | `add_function()`, `add_tool()` |

**Getter patterns**:

| Pattern | Purpose | Example |
|---------|---------|---------|
| `as_*()` | Extract enum variant as borrowed reference | `as_text()`, `as_parts()` |
| `into_*()` | Extract enum variant, consuming self | `into_text()` |
| `is_*()` | Check if value matches a variant/condition | `is_unknown()`, `is_empty()` |

**Method suffixes**:

| Suffix | Meaning | Example |
|--------|---------|---------|
| `*_stream()` | Returns `Stream<Item>` for async iteration | `create_stream()` |
| `*_chunked()` | Uses chunked I/O internally, returns single result | `upload_file_chunked()` |
| `*_with_auto_functions()` | Automatically executes functions in a loop | `create_stream_with_auto_functions()` |

### #[must_use] Annotation

Apply `#[must_use]` to getters, handles, and boolean checks where ignoring the result is likely a bug.

## Versioning Philosophy

Breaking changes are permitted and preferred when they simplify the API or align with Evergreen principles. Prefer clean breaks over backwards-compatibility shims.

**CHANGELOG**: Update `CHANGELOG.md` for user-facing changes: new features, breaking changes, bug fixes, deprecations. Internal refactors and CI changes don't need entries.

### Version Bump Checklist

When releasing a new version, update these files:

| File | Location |
|------|----------|
| `Cargo.toml` | `version = "X.Y.Z"` (line ~3) |
| `Cargo.toml` | `genai-rs-macros = { version = "X.Y.Z"` (dependencies) |
| `genai-rs-macros/Cargo.toml` | `version = "X.Y.Z"` (line ~3) |
| `README.md` | `genai-rs = "X.Y"` and `genai-rs-macros = "X.Y"` (Installation section) |
| `CHANGELOG.md` | `## [Unreleased]` → `## [X.Y.Z] - YYYY-MM-DD` |

`Cargo.lock` updates automatically—don't edit manually.

### Release Steps

After merging version bump PR:

1. **Tag the release**: `git tag -a vX.Y.Z origin/main -m "Release vX.Y.Z"`
2. **Push tag**: `git push origin vX.Y.Z`
3. **Create GitHub release**: `gh release create vX.Y.Z --title "vX.Y.Z" --notes "..."` (copy from CHANGELOG)
4. **Publish to crates.io**:
   - `cargo publish -p genai-rs-macros` (wait 30s for index)
   - `cargo publish -p genai-rs`

## Logging

See `docs/LOGGING_STRATEGY.md`. Key points:
- `error` for unrecoverable, `warn` for recoverable (including Evergreen unknowns), `debug` for API lifecycle
- API keys redacted; user content only at `debug` level
- Enable: `RUST_LOG=genai_rs=debug cargo run --example simple_interaction`

## Technical Notes

- Rust edition 2024 (requires Rust 1.88+)
- Uses `rustls-tls` (not native TLS)
- Tokio async runtime
- API version: Gemini V1Beta (configured in `src/http/common.rs`)
- See `CHANGELOG.md` for breaking changes and migration guides

### CI Debugging Tips

- **GitHub Actions log parsing**: Logs from `gh run view --log-failed` are prefixed with `JobName\tStepName\tTimestamp\t`. Use `sed 's/.*test //'` (not `sed 's/^test //'`) to extract test names from failure output.
