# Fuzzing

[cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) targets for
the code that parses what the server sends. Each target must not panic, and
checks a round trip where the input is valid:

| Target | Input | Property |
|--------|-------|----------|
| `sse_parser` | A control byte (how many read sizes follow), the read sizes, then an SSE body | The events don't depend on where the body splits into reads, and re-encoding them as plain `data:` events parses back to the same values |
| `stream_event` | An SSE body of `InteractionStreamEvent`s (JSON lines work: a line starting with `{` is an event) | The whole stream pipeline (dispatch, step accumulation, the stop at the terminal event) runs, and every `StreamEvent` it yields serializes and parses back unchanged |
| `step`, `step_delta`, `content` | JSON | A value that deserializes reads back equal after one serialization, and a second write and read changes neither the wire form nor the value. Two differences are intended: an empty `Content::Text` annotation list reads back as none (it isn't written), and an input with no string `type` reads back with the `"<missing type>"` tag it was given in its data |
| `interaction_response` | JSON | The same round trip for `InteractionResponse`, with the same differences wherever a step, content block or tool sits in it |
| `resource` | JSON | The same round trip for every type the input reads as: `Agent`, `Trigger`, `Webhook`, `Environment`, `EnvironmentSpec` and each list page (a trigger's request `Content` input reads back as one `user_input` step). `EnvironmentSpec` and `Agent` must also write back the JSON they read (see below) |
| `harness_output_event` | JSON (the `antigravity` feature) | The same round trip for the Antigravity harness's `OutputEvent`, which must also write back the JSON it read |

A type that promises to write back the JSON it read (D-NEXT-J7 in
`DECISIONS.md`) is also checked against its input, not only against its
own later writes, so a value lost on the first read fails the target. Both
sides are compared in a canonical form: a `null`, `[]` or `{}` member is
absent (so an empty value lost on the first read is not seen), a whole
float is its integer, and the `f32` fields (`temperature`, `top_p`,
`alpha`) hold what an `f32` can. In the harness's JSON, and only at the
fields its types read so, an int64 string is its number and a
`toolResponse`'s `id` at its default is absent; a oneof member, or an
unknown or `extra` value, keeps a `false`, `0`, `""` or numeric string as
it came. Each type's own normalizations are applied to the input first
(`src/fuzzing_wire.rs`): an environment's missing `type` written as
`"remote"`, its echoed `env` list written as a map, the disabled allowlist
echo written in its request form, an empty allowlist `domain` not written
(one read as `""` included), an unknown tool's tag merged in, a retrieval
tool's backend configs written as their types write them (they keep no
`extra`), and an event's two usage spellings written as one `usageUpdate`.

The entry points live in the library, in `src/fuzzing.rs` (`genai_rs::__fuzz`,
compiled only under `cfg(fuzzing)` and in tests), so the targets here are one
line each and the crate's own tests can replay failing inputs.

This crate has its own workspace and lockfile and needs nightly.

```bash
cargo install cargo-fuzz --locked
cd fuzz
cargo +nightly fuzz run sse_parser corpus/sse_parser seeds/sse_parser -- -max_total_time=60
# The harness target needs the library's antigravity feature:
cargo +nightly fuzz run --features antigravity harness_output_event \
  corpus/harness_output_event seeds/harness_output_event -- -max_total_time=60
```

`seeds/<target>/` is the committed seed corpus (small real shapes, including
a captured live text stream, and inputs that once failed). Pass it
after the scratch corpus directory so new finds land in `corpus/`, which is
gitignored with `artifacts/`. The crate's own tests replay every seed
through its target (`committed_seeds_replay_through_their_targets` in
`src/fuzzing_tests.rs`), so each target needs a seed directory.

## When a target fails

1. `cargo +nightly fuzz tmin <target> artifacts/<target>/<crash>` to shrink
   the input.
2. Fix the bug.
3. Keep the input as a regression test in `src/fuzzing_tests.rs`, calling the
   same entry point, with a comment saying what it found.

`.github/workflows/fuzz.yml` runs every target for a minute weekly, on
demand (with a chosen duration), and on pull requests that touch anything
the targets build (`src/` outside `antigravity/`, the harness protocol in
`src/antigravity/protocol/`, `genai-rs-macros/`, the root `Cargo.toml` or
`fuzz/`). A failed scheduled run opens a `ci-health-escalation` issue and uploads
the failing input as the `fuzz-<target>-crash` artifact. Each run starts
from the corpus the last green scheduled or manual run grew (an Actions
cache per target, `fuzz-corpus-<target>-*`), so the weekly runs go deeper
over time; a pull request's run reads that corpus but does not save its own.

The workflow starts on every pull request, and its `Fuzz Gate` job reports
on each one: it passes at once when no fuzzed path changed, and otherwise
only when every target ran clean. It is the check branch protection can
require, since the per-target checks exist only on runs that fuzz. A new
target goes in `Cargo.toml`'s `[[bin]]` list, the workflow's `target`
matrix and `seeds/`; `tests/ci_coverage.rs` fails until all three agree. A
target whose `[[bin]]` has `required-features` also needs them in the
matrix's `include:` (`features:`), which the run passes as `--features`.
