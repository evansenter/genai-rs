# Application Examples

Small applications built from several features at once. Both are the same
customer-support agent over an in-memory stub CRM, built two ways:

| Example | State | Function calling |
|---------|-------|------------------|
| [`multi_turn_agent_auto`](./multi_turn_agent_auto/) | Server-side (`previous_interaction_id`) | `#[tool]` + `create_with_auto_functions()` |
| [`multi_turn_agent_manual_stateless`](./multi_turn_agent_manual_stateless/) | Client-side (`with_store_disabled()`) | `FunctionDeclaration` + manual loop |

```bash
cargo run --example multi_turn_agent_auto
cargo run --example multi_turn_agent_manual_stateless
```

Both need only `GEMINI_API_KEY`. The Antigravity harness applications live in
[`examples/antigravity/`](../antigravity/); they additionally need the
`antigravity` feature and the `localharness` binary.

Adding one: create `examples/real_world/<name>/main.rs` and a README, add an
`[[example]]` entry to `Cargo.toml`, list it here and in
`docs/EXAMPLES_INDEX.md`, and run it live before committing.
