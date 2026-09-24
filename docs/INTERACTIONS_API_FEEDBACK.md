# Interactions API Feedback Report

Feedback for the Google Gemini API team based on building and maintaining [genai-rs](https://github.com/evansenter/genai-rs), a Rust client library for the Interactions API.

**Date**: 2026-01-10
**Library Version**: 0.5.3

---

## Summary

| # | Priority | Issue | Action |
|---|----------|-------|--------|
| 1 | P0 | ThinkingSummaries wire format | Fix docs: `"THINKING_SUMMARIES_AUTO"` not `"auto"` |
| 2 | P0 | Multi-turn inheritance | Add documentation page for `previousInteractionId` behavior |
| 3 | P1 | Agent mode requirements | Make `background=true` requirement more prominent |
| 4 | P1 | Enum wire formats | Provide authoritative format table for all enums |
| 5 | P2 | Content type nesting | Document rationale or consider flattening |
| 6 | P2 | SpeechConfig format | Fix docs: nested format fails, only flat works |
| 7 | P2 | Annotation byte indexing | Document that indices are UTF-8 byte positions |
| 8 | P2 | Thought signatures differ from generateContent | Document Interactions API signature location |
| 9 | P3 | Parallel function call ordering | Document matching by `call_id` |

---

## Critical Issues (P0)

### 1. ThinkingSummaries Wire Format is Misdocumented

**Severity**: Critical — causes silent failures

**Documentation**: [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) states ThinkingSummaries accepts `"auto"` and `"none"`.

**Reality**: The API actually requires `"THINKING_SUMMARIES_AUTO"` and `"THINKING_SUMMARIES_NONE"`.

**Discovery**: API returned `"unknown enum value: 'auto'"` until the correct format was found through trial and error.

```json
// Documentation claims (WRONG):
{"agent_config": {"thinkingSummaries": "auto"}}

// What actually works:
{"agent_config": {"thinkingSummaries": "THINKING_SUMMARIES_AUTO"}}
```

**Recommendation**: Update documentation to show the correct fully-qualified enum values.

**Our workaround**: We serialize to the correct format and document the discrepancy.
- [`src/request.rs`](../src/request.rs) - `ThinkingSummaries` enum with correct wire format
- [`docs/ENUM_WIRE_FORMATS.md`](./ENUM_WIRE_FORMATS.md#thinkingsummaries) - Documents the mismatch

---

### 2. Multi-Turn Inheritance Rules Are Undocumented

**Severity**: Critical — fundamental to correct API usage

**Documentation**: [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) and [Interactions API Guide](https://ai.google.dev/static/api/interactions-api.md.txt) mention `previousInteractionId` but do not specify field inheritance behavior.

There is no official documentation on what fields are inherited when using `previousInteractionId` for multi-turn conversations:

| Field | Inherited? | Impact if wrong |
|-------|------------|-----------------|
| `systemInstruction` | ❌ No | Silently dropped on later turns; the model can still echo it through replayed thoughts, which hides the omission |
| `tools` | ❌ **No** | **Silent function calling failure** |
| `model` | ❌ No | Request fails with clear error |
| Conversation history | ✅ Yes | N/A |

The `tools` behavior is particularly sharp:
- If you **don't** resend tools on new user message turns → function calling silently fails
- If you **do** resend tools on function result turns → API returns an error

**Recommendation**: Add a dedicated "Multi-Turn Conversations" documentation page specifying exactly what's inherited and what must be resent.

**Our workaround**: We document the rules extensively and handle tools resending internally.
- [`src/request_builder/mod.rs`](../src/request_builder/mod.rs) - Builder handles tools configuration
- [`docs/MULTI_TURN_FUNCTION_CALLING.md`](./MULTI_TURN_FUNCTION_CALLING.md#inheritance-rules) - Documents inheritance behavior

---

## Important Issues (P1)

### 3. Agents Require `background=true` (Buried in Docs)

**Documentation**: [Deep Research Guide](https://ai.google.dev/gemini-api/docs/deep-research) does mention this requirement, but it's not prominent in the main API reference.

The Deep Research agent (`deep-research-pro-preview`) silently fails without `background=true`. The requirement is documented but easy to miss when using the API reference directly.

**Impact**: We had to remove our synchronous agent example because it simply doesn't work.

**Recommendation**: Add prominent callouts in the Interactions API reference that agent interactions require `background=true` and `store=true`.

**Our workaround**: We document the requirement prominently and validate at runtime.
- [`docs/AGENTS_AND_BACKGROUND.md`](./AGENTS_AND_BACKGROUND.md) - Documents agent requirements
- [`src/request_builder/mod.rs`](../src/request_builder/mod.rs) - Runtime validation catches invalid combinations

---

### 4. Multiple Enum Wire Format Mismatches

**Documentation**: [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) provides enum descriptions but not always the exact wire format strings.

| Enum | Documentation Claims | Actual Wire Format |
|------|---------------------|-------------------|
| `ThinkingSummaries` | `"auto"`, `"none"` | `"THINKING_SUMMARIES_AUTO"`, `"THINKING_SUMMARIES_NONE"` |
| ~~`CodeExecutionOutcome`~~ | Generic enum with `outcome`/`output` | Removed - actual API uses `is_error: bool` + `result: String` |
| `UrlRetrievalStatus` | Not specified | `"URL_RETRIEVAL_STATUS_SUCCESS"`, `"URL_RETRIEVAL_STATUS_ERROR"` |
| `CodeExecutionLanguage` | Implies multiple languages | Only `"PYTHON"` is supported |

**Recommendation**: Provide an authoritative enum wire format reference table in the API documentation.

**Our workaround**: We maintain our own authoritative reference with verified wire formats.
- [`docs/ENUM_WIRE_FORMATS.md`](./ENUM_WIRE_FORMATS.md) - Comprehensive table of all enum wire formats

---

## Documentation Gaps (P2)

### 5. Nesting in Content Types Adds Client Complexity

**Documentation**: [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) shows `UrlContextCall` and `CodeExecutionCall` with nested `arguments` objects.

Several content types nest data inside `arguments` objects, which requires extra extraction logic in client libraries:

| Content Type | Current Wire Format | Simpler Alternative |
|--------------|---------------------|---------------------|
| `UrlContextCall` | `{"id": "...", "arguments": {"urls": [...]}}` | `{"id": "...", "urls": [...]}` |
| `GoogleSearchCall` | `{"id": "...", "arguments": {"queries": [...]}}` | `{"id": "...", "queries": [...]}` |

**Impact**: ~100 lines of custom deserialization logic per content type.

**Question**: Is there a design rationale for this nesting pattern (e.g., future extensibility, consistency with another API)? If so, documenting it would help client authors understand the design. If not, flattening these structures in a future API version would simplify client implementations.

**Our workaround**: We implement custom deserialization to flatten the nested structures.
- [`src/content.rs`](../src/content.rs) - Custom `Deserialize` impl extracts `arguments.urls` into flat struct

---

### 6. SpeechConfig Documentation Shows Wrong Format

**Severity**: Important — documented format doesn't work

**Documentation**: [Speech Generation Guide](https://ai.google.dev/gemini-api/docs/speech-generation) shows nested `voiceConfig.prebuiltVoiceConfig.voiceName` structure.

**Reality**: The documented nested format fails with 400 error. Only a flat structure works.

| Format | Wire Structure | Works? |
|--------|----------------|--------|
| Documented (nested) | `{"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Kore"}}}` | ❌ No - 400 error |
| Actual (flat) | `{"voice": "Kore", "language": "en-US"}` | ✅ Yes |

**Discovery**: Testing the documented nested format returns:
```
400 Bad Request: no such field: 'voiceConfig'
```

```json
// What the documentation shows (DOESN'T WORK):
{
  "generationConfig": {
    "speechConfig": {
      "voiceConfig": {
        "prebuiltVoiceConfig": {
          "voiceName": "Kore"
        }
      }
    }
  }
}

// What actually works:
{
  "generationConfig": {
    "speechConfig": {
      "voice": "Kore",
      "language": "en-US"
    }
  }
}
```

**Recommendation**: Update the Speech Generation Guide to show the flat SpeechConfig format that actually works.

**Our workaround**: We use the flat structure and document our findings.
- [`src/request.rs`](../src/request.rs) - `SpeechConfig` uses flat `voice`/`language` fields
- [`docs/ENUM_WIRE_FORMATS.md`](./ENUM_WIRE_FORMATS.md#speechconfig) - Documents verified format
- [`tests/multimodal_tests.rs`](../tests/multimodal_tests.rs) - `test_speech_config_nested_format_fails_flat_succeeds` verifies both formats

---

### 7. Annotation Indices Are Byte Positions (Not Character Indices)

**Documentation**: [Interactions API Reference](https://ai.google.dev/static/api/interactions.md.txt) defines `Annotation` with `start_index` and `end_index` but doesn't specify the unit.

Annotation `start_index` and `end_index` fields are UTF-8 byte positions, not character indices. For multi-byte characters (emoji, non-ASCII text), using character indexing breaks text extraction.

```rust,ignore
// WRONG - breaks on non-ASCII:
let cited = &text[annotation.start_index..annotation.end_index];

// CORRECT - use byte slicing:
let cited = &text.as_bytes()[annotation.start_index..annotation.end_index];
```

**Recommendation**: Explicitly document that annotation indices are UTF-8 byte positions.

**Our workaround**: We document the byte semantics and provide a helper method.
- [`src/content.rs`](../src/content.rs) - `Annotation::extract_span()` uses byte slicing correctly

---

### 8. Thought Signatures Differ from generateContent

**Documentation**: [Thought Signatures Guide](https://ai.google.dev/gemini-api/docs/thought-signatures.md.txt) documents thought signatures for the `generateContent` API, showing signatures on `function_call` outputs.

**Reality**: The Interactions API handles thought signatures differently:

| API | Signature Location |
|-----|-------------------|
| `generateContent` (per docs) | On `function_call` as `thought_signature` field |
| Interactions API (actual) | On separate `thought` output as `signature` field |

Additionally, **the API rejects thought blocks in user input** with: `"User turns cannot contain thought blocks."` This means signatures cannot be echoed back in stateless multi-turn.

```json
// Interactions API response structure:
"outputs": [
  {
    "signature": "EjQKMgFyy...",  // <-- Signature is HERE on thought
    "type": "thought"
  },
  {
    "arguments": {"city": "Paris"},
    "name": "get_weather",
    "type": "function_call"        // <-- NOT here
  }
]
```

**Workaround**: For multi-turn with thinking, use `with_previous_interaction(id)` — the server preserves thought context automatically. For stateless mode, signatures are not needed since thoughts cannot be echoed back.

**Recommendation**: Document the Interactions API signature behavior separately from `generateContent`.

- [`docs/MULTI_TURN_FUNCTION_CALLING.md`](./MULTI_TURN_FUNCTION_CALLING.md#thought-signatures) - Documents our findings
- [`examples/thought_echo.rs`](../examples/thought_echo.rs) - Demonstrates the API limitation

---

## Nice-to-Have (P3)

### 9. Document Parallel Function Call Ordering

**Documentation**: [Function Calling Guide](https://ai.google.dev/gemini-api/docs/function-calling.md.txt) shows function calling but doesn't address parallel call ordering.

The API returns multiple function calls in responses, but whether order matters is undocumented. Our tests confirm calls are matched by `call_id` (order-independent), but this isn't guaranteed in documentation.

**Recommendation**: Document that function calls are matched by `call_id`, not by position.

**Our workaround**: We verified this behavior through testing and document it.
- [`tests/function_calling_tests.rs`](../tests/function_calling_tests.rs) - `test_parallel_function_result_order_independence`
- [`docs/MULTI_TURN_FUNCTION_CALLING.md`](./MULTI_TURN_FUNCTION_CALLING.md#parallel-calls) - Documents order independence

---

## Context

This feedback is based on building [genai-rs](https://github.com/evansenter/genai-rs), a production-quality Rust client library that implements the full Interactions API surface.

### Library Scope

| Component | Size |
|-----------|------|
| Source code | ~30,000 LOC |
| Internal documentation | ~8,000 LOC (17 guides) |
| Examples | 28 runnable examples (~9,600 LOC) |
| Integration tests | ~17,000 LOC (385 tests) |
| End-to-end API tests | 152 (require `GEMINI_API_KEY`) |
| Semantic validation assertions | 39 (LLM-validated) |

### Design Principles

- **Soft-typing**: Unknown API values deserialize into `Unknown` variants, preserving data for roundtrip
- **Compile-time safety**: Typestate pattern prevents invalid API usage (e.g., tools on wrong turn)
- **Comprehensive testing**: Integration tests cover streaming, function calling, agents, and all built-in tools

The internal documentation exists because the official documentation gaps required significant reverse-engineering effort. We're happy to provide additional details, test cases, or collaborate on documentation improvements.
