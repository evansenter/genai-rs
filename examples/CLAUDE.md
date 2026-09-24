# Examples Guidelines

Examples are documentation that runs. Each one must run live against the API
and exit 0 (`cargo run --example <name>` with `GEMINI_API_KEY` set) — run it
before committing a change to it. A subset is smoke-run in CI
(`example-smoke` in `.github/workflows/rust.yml`).

- **One feature per example, shown for real.** No code printed as text, no
  canned output presented as a result, no simulated delays. Stub data in a
  tool is fine when the doc comment says it is a stub.
- **Propagate errors with `?`.** Don't print-and-continue, don't substitute
  `unwrap_or` defaults for missing data, and don't print success banners.
  If the example's point depends on something happening (a tool was called,
  a signed thought was replayed), check it and return an error if it didn't.
- **Clean up** what the example creates (files, stores, webhooks,
  environments, stored interactions where deleting is the point), even when
  a later step fails.
- **Model and agent IDs come from the constants** in `src/lib.rs`
  (`DEFAULT_MODEL`, `DEFAULT_IMAGE_MODEL`, `DEFAULT_TTS_MODEL`,
  `MINIMAL_THINKING_MODEL`, `DEFAULT_DEEP_RESEARCH_AGENT`, ...), including in
  prose.
- **Explain in the `//!` header**: what the example shows, the constraints a
  reader would otherwise trip over, and the run command. Keep comments to the
  why; `LOUD_WIRE=1` shows the wire, so don't describe it by hand.
- Slice text with `chars()`, never byte ranges.
- The base64 fixtures (`*_BASE64` constants) are checked against
  `tests/common` by `tests/temp_file_tests.rs`; every canonical fixture must
  stay embedded in some example.
- New examples: add them to `docs/EXAMPLES_INDEX.md` (and `Cargo.toml` if
  they live in a subdirectory).
