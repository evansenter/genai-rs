# Examples Guidelines

Examples are documentation that runs. These rules cover every example,
including `examples/antigravity/` and `examples/real_world/`. Each one must run
live against the API and exit 0 (`cargo run --example <name>` with
`GEMINI_API_KEY` set; add `--features antigravity` for the harness examples) —
run it before committing a change to it. A subset is smoke-run in CI
(`example-smoke` and the antigravity smoke step in `.github/workflows/rust.yml`).

- **One feature per example, shown for real.** No code printed as text, no
  canned output presented as a result, no simulated delays. Stub data in a
  tool is fine when the doc comment says it is a stub.
- **Propagate errors with `?`.** Don't print-and-continue, and don't
  substitute `unwrap_or` defaults for missing data. If the example's point
  depends on something happening (a tool was called, a signed thought was
  replayed), check it and return an error if it didn't — never print a
  warning and exit 0.
- **Print results, not commentary.** No closing banners ("Example Complete"),
  no printed wire walkthroughs ("What you'll see with LOUD_WIRE=1"), no
  printed advice lists ("Production considerations"). Constraints a reader
  needs go in the `//!` header.
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
