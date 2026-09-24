# Multi-Turn Support Agent (auto functions, server-side state)

A customer-support agent that looks up a customer, lists their orders and
starts a refund, over three chained turns.

```bash
cargo run --example multi_turn_agent_auto
```

## What it shows

- **`#[tool]` functions** over an in-memory stub CRM, executed by
  `create_with_auto_functions()`. Tool errors (unknown customer, order not
  delivered) go back to the model as JSON it can react to.
- **Server-side history**: each turn passes the previous response's ID to
  `with_previous_interaction()`.
- **What is not inherited**: the system instruction and the tools are sent on
  every turn. Only conversation history carries over through
  `previous_interaction_id`.
- **`reached_max_loops`** is checked, so a model stuck calling tools is an
  error rather than a silently truncated answer.

See [`multi_turn_agent_manual_stateless`](../multi_turn_agent_manual_stateless/)
for the same agent with no server-side state, and
`docs/MULTI_TURN_FUNCTION_CALLING.md` for the full guide.
