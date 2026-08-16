# Decisions

Durable design decisions and the reasoning behind them.

This file answers **"why is it like this?"**. `CLAUDE.md` answers **"how do I
work here?"** — commands, conventions, checklists. When a rule in CLAUDE.md
needs a paragraph of justification, the justification belongs here and the
rule stays there.

Entries are append-only in spirit: when a decision is reversed, mark it
superseded and say what changed rather than deleting it. A decision that
turned out wrong is more useful than no record — see D-010, which exists
precisely because a feature was marked done without evidence.

**Format**: context (what forced a choice) → decision → consequences
(including what it costs). No template ceremony beyond that.

---

## D-001 — Evergreen soft-typing: preserve unknown data, never reject it

**Context.** The Gemini API adds enum variants, content types, and fields
without warning. A client that rejects what it doesn't recognize breaks on
someone else's release schedule.

**Decision.** Follow the [Evergreen spec](https://github.com/google-deepmind/evergreen-spec):
unrecognized values deserialize into `Unknown` variants that preserve the
original JSON, enums are `#[non_exhaustive]`, unknown data round-trips intact,
and polling continues on unrecognized status (bounded by timeouts).

**Consequences.** Callers must handle wildcard match arms. Every new enum
costs three helper methods (`is_unknown`, `unknown_<context>_type`,
`unknown_data`) and a round-trip test. In exchange, an API addition is a
`warn!` rather than an outage.

The `strict-unknown` feature flag inverts this for tests that want to *find*
unknowns rather than tolerate them.

*Stated as a rule in CLAUDE.md ("Core Design Philosophy").*

---

## D-002 — Response structs are `#[non_exhaustive]`; users cannot construct them

**Context.** Response types grow fields as the API does. If users can build
them with struct literals, every added field is a breaking change.

**Decision.** Response structs carry `#[non_exhaustive]`, so they cannot be
constructed outside the crate.

**Consequences.** Users cannot hand-build a response to test against — which
is the point:

- Response types represent API responses, not user-constructed data
- Mocking them in unit tests gives false confidence
- Fields can be added without breaking changes

For testing, users should use integration tests against the real API, or mock
at the HTTP layer rather than the response-type layer.

Since revision 2026-05-20, `InteractionResponse` derives `Default` and uses
`#[serde(default)]`, so in-crate fixtures can be built with
`..Default::default()`.

**Known gap**: five response structs — `Trigger`, `TriggerExecution`,
`Environment`, `Agent`, `Webhook` — do not carry the attribute, so adding a
field to them *is* breaking. Tracked in #430.

*Relocated from `docs/ENUM_WIRE_FORMATS.md` ("Structs and `#[non_exhaustive]`").*

---

## D-003 — Antigravity alias enums deliberately break round-trip fidelity

**Context.** The Antigravity harness renames enum values between revisions.
`STATE_IDLE` became `STATE_FULLY_IDLE` in 0.1.5 → 0.1.10, and treating the old
spelling as `Unknown` is exactly what caused that breakage — every turn hung
to timeout.

**Decision.** These enums accept alias wire values and re-emit the *canonical*
current-harness spelling. `STATE_IDLE` in, `STATE_FULLY_IDLE` out.

**Consequences.** A deliberate departure from D-001's round-trip principle,
and worth naming as such. It is safe here because these are inbound-only
enums — the client never sends a `TrajectoryState` — so the asymmetry cannot
reach the wire. What it buys is one build driving either harness revision.

*Relocated from `docs/ENUM_WIRE_FORMATS.md` (Antigravity enums section).*

---

## D-004 — Verify API surface from generated bindings first, prose docs last

**Context.** Three sources describe the API and they disagree, consistently in
one direction. Two features found in the 2.18.1 sweep — video `processing` (a
~127x token-cost lever) and a widened `speech_config` union — appear in
*neither* `ai.google.dev` page. Conversely, the bindings describe a
`speech_config` object arm the API rejects outright.

**Decision.** Rank sources: (1) generated bindings from `google-genai`, which
ship ahead of prose; (2) live probes, which are ground truth and often
narrower than the spec; (3) prose docs, including this repo's own, which lag
both. New surface found at rank 1 must be live-probed before being modeled as
usable.

**Consequences.** "Absent from the docs" is not evidence of absence, and
"present in the bindings" is not evidence of support. `docs/INTERACTIONS_API_GAP.md`
is a point-in-time snapshot, not a completeness guarantee, and says so in its
header. The `api-surface-sweep` workflow files an issue when the SDK moves.

Getting this ordering backwards is what let both 2.18.1 features sit unmodeled
and produced a "surface fully covered" conclusion that was false (#421).

---

## D-005 — Spec-present but API-rejected: remove, or keep for parity?

**Context.** Several fields exist in the generated bindings but are rejected by
the Gemini endpoint. They fall into two groups, and the API's own error text
distinguishes them.

**Decision.**

| Error shape | Meaning | Action |
|---|---|---|
| *"not available on the Gemini API but ... available on the Gemini Enterprise Agent Platform"* | Vertex-only, real feature elsewhere | **Keep**, documented as Vertex-only |
| *"Unknown parameter 'x'"* | Absent from the schema entirely | **Remove** |

Kept for parity: `safety_settings`, `labels`, `Tool::Retrieval`,
`enable_bigquery_tool`. Removed: `response_mime_type`, `cached_content`.

**Consequences.** Modeling a field the endpoint rejects is only justified when
the field is real somewhere. `cached_content` shipped as a public builder
method that could only ever produce a 400 — the field was modeled from the
spec, never live-probed, and its test asserted the field *serialized*
correctly, which it did. See D-010.

---

## D-006 — Model ids live in constants, and only in `src/lib.rs`

**Context.** Bumping the model previously meant editing ~600 occurrences of a
string literal. A sweep that size reliably misses a few, and a missed one is
invisible: the test keeps passing against a model nobody meant to still be
using.

**Decision.** `DEFAULT_MODEL` and friends are public constants and the single
source of truth. `tests/model_literals.rs` fails the build on any hardcoded
`"gemini-<digit>"` outside them.

Capability-specific constants (`INLINE_VIDEO_MODEL`, `MINIMAL_THINKING_MODEL`)
are re-pinned *independently* of the default: they track whichever model has
the capability, so they go stale when that model retires, not when the default
moves.

**Consequences.** In-crate unit tests may use either a constant or the
synthetic `"test-model"`; they exercise serialization round-trips and never
cared which. **This does not extend to non-test code** — a fallback model id
that reaches the wire must be a real one. `AgentBuilder`'s default was once
swept to `"test-model"` on exactly this confusion, which is why the boundary
is stated explicitly rather than left to judgement.

The literal guard cannot protect the constants file itself: its whole job is
to keep ids in that file, so an id that is stale *in* that file is invisible to
it by construction.

*Relocated from CLAUDE.md (model constants table).*

---

## D-007 — Breaking changes are preferred over compatibility shims

**Context.** Pre-1.0 library tracking a beta API that itself makes breaking
changes.

**Decision.** Break cleanly when it simplifies the API or aligns with
Evergreen principles. Do not add deprecation shims or dual-path
compatibility.

**Consequences.** Consumers pin and read the CHANGELOG. In exchange the
surface stays small and there is no long tail of half-supported spellings.
CHANGELOG entries carry migration notes for anything user-facing.

*Relocated from CLAUDE.md ("Versioning Philosophy").*

---

## D-008 — Tests are organized by feature, not by pattern

**Context.** Multi-turn conversation patterns appear across
`multiturn_tests.rs`, `streaming_multiturn_tests.rs`,
`function_calling_tests.rs`, and `interactions_api_tests.rs`. It looks like
duplication.

**Decision.** Tests live with **what they primarily verify**, not with the
mechanics they happen to use. A function-calling test that uses multi-turn is
testing function calling.

**Consequences.** Don't consolidate tests because they share a pattern. Ask
"what is this test primarily verifying?" The payoff is that every test for a
feature is in one place.

*Relocated from `docs/TESTING.md` ("Intentional Design Decisions").*

---

## D-009 — Doctests run in CI only; the docs.rs check is stricter than docs.rs

**Context.** Doctests add real compile overhead to every local `make test`.
Separately, docs.rs itself only fails on hard errors, not warnings.

**Decision.** `make test` and `make test-all` exclude doctests; CI runs
`cargo test --workspace --doc`. The local docs.rs verification uses
`-D warnings`, deliberately stricter than docs.rs and than the CI gate.

**Consequences.** A doctest can break locally without being noticed until CI.
Accepted: CI catches them, and the local loop stays fast. The `-D warnings`
strictness is an early signal, not a prediction of what docs.rs will reject.

*Relocated from CLAUDE.md (Testing / Release Steps).*

---

## D-010 — A structural test is not evidence a feature works

**Context.** `cached_content` was marked complete in the gap analysis and had
a passing test, `test_with_cached_content_sets_field_and_wire_format`, which
asserted the field serialized to the right wire shape. It did. The API
rejected the request anyway: `400 Unknown parameter 'cached_content'`. The
feature was unusable end to end for its entire shipped life.

**Decision.** A feature is not verified until something exercised it against
the live API. Serialization tests pin the wire *shape*; they say nothing about
acceptance. Where a feature has an observable effect, prefer a behavioral
assertion over a structural one.

**Consequences.** More `#[ignore = "Requires API key"]` integration tests, and
they must fail loudly rather than skip on error — a test that swallows a
request failure and returns early "passes" without reaching the API, which is
the same false confidence one layer down.

Concretely: the video `processing` test asserts a >10x token-count difference
between a clipped and unclipped window rather than asserting the field
serializes, and panics rather than skips when the request fails.

*See also `docs/TESTING.md` on structural vs semantic assertions.*

---

## D-011 — Record structural moves

**Context.** #265 spent seven months pointing at `genai-client/src/models/shared.rs`,
a path that no longer exists. The crate was restructured with no record, so a
reader could not tell whether the issue was stale or the file was missing.

**Decision.** When directories or modules move, add an entry here saying what
moved and why. Renaming a file is cheap; leaving every issue and comment that
references it silently wrong is not.

**Consequences.** One more step on refactors that relocate code. Skippable for
moves within a file.
