# Multi-Turn Support Agent (manual loop, stateless)

The support agent with `with_store_disabled()`: the server keeps nothing, so
the client holds the conversation and sends all of it on every request.

```bash
cargo run --example multi_turn_agent_manual_stateless
```

## What it shows

- **Client-held history** as a `Vec<Step>`, sent with `with_history()`.
- **A manual function-calling loop.** `create_with_auto_functions()` needs
  stored interactions and refuses to run with storage disabled.
- **Replaying the model's steps verbatim.** Each round appends
  `response.output_steps()` (thoughts, function calls, their signatures)
  before the `Step::function_result`s. Rebuilding calls with
  `Step::function_call(..)` drops the signatures the API needs to accept the
  history.
- **System instruction and tools on every request**, since nothing is
  inherited.

| | `multi_turn_agent_auto` | this example |
|--|--|--|
| History | server (`previous_interaction_id`) | client (`Vec<Step>`) |
| Functions | `#[tool]`, automatic | `FunctionDeclaration`, manual loop |
| Storage | on (default) | `with_store_disabled()` |

See `docs/CONVERSATIONS.md` for the full guide.
