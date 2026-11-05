# Fuzzing

[cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) targets for
the code that parses what the server sends. Each target must not panic, and
checks a round trip where the input is valid:

| Target | Input | Property |
|--------|-------|----------|
| `sse_parser` | A control byte (how many read sizes follow), the read sizes, then an SSE body | The events don't depend on where the body splits into reads, and re-encoding them as plain `data:` events parses back to the same values |
| `stream_event` | An SSE body of `InteractionStreamEvent`s (JSON lines work: a line starting with `{` is an event) | The whole stream pipeline (dispatch, step accumulation, the stop at the terminal event) runs, and every `StreamEvent` it yields serializes and parses back unchanged |
| `step`, `step_delta`, `content` | JSON | A value that deserializes reads back equal after one serialization, and a second write and read changes neither the wire form nor the value. Two differences are intended: an empty `Content::Text` annotation list reads back as none (it isn't written), and an input with no string `type` reads back with the `"<missing type>"` tag it was given in its data |

The entry points live in the library, in `src/fuzzing.rs` (`genai_rs::__fuzz`,
compiled only under `cfg(fuzzing)` and in tests), so the targets here are one
line each and the crate's own tests can replay failing inputs.

This crate has its own workspace and lockfile and needs nightly.

```bash
cargo install cargo-fuzz --locked
cd fuzz
cargo +nightly fuzz run sse_parser corpus/sse_parser seeds/sse_parser -- -max_total_time=60
```

`seeds/<target>/` is the committed seed corpus (small real shapes, including
a live text stream captured 2026-11-04). Pass it after the scratch corpus
directory so new finds land in `corpus/`, which is gitignored with
`artifacts/`.

## When a target fails

1. `cargo +nightly fuzz tmin <target> artifacts/<target>/<crash>` to shrink
   the input.
2. Fix the bug.
3. Keep the input as a regression test in `src/fuzzing_tests.rs`, calling the
   same entry point, with a comment saying what it found.

`.github/workflows/fuzz.yml` runs every target for a minute weekly, on
demand (with a chosen duration), and on pull requests that touch anything
the targets build (`src/` outside `antigravity/`, `genai-rs-macros/`, the root
`Cargo.toml` or `fuzz/`). A failed scheduled run opens a `ci-health-escalation` issue and uploads
the failing input as the `fuzz-<target>-crash` artifact.
