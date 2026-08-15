# Multi-Turn Customer Support Agent (Auto Functions)

A stateful customer support chatbot using `create_with_auto_functions()` for seamless tool execution.

See also: [multi_turn_agent_manual](../multi_turn_agent_manual/) for manual function calling.

## Overview

This example demonstrates a production-style support agent that:

1. **Stateful Conversations**: Maintains context across multiple turns
2. **Function Calling**: Integrates with backend systems via tools
3. **Customer Data Access**: Looks up orders, creates tickets, processes refunds
4. **Session Management**: Tracks conversation history and customer identity

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Customer  │────▶│   Session   │────▶│   Gemini    │
│   Message   │     │   Manager   │     │   Model     │
└─────────────┘     └─────────────┘     └─────────────┘
                           │                    │
                           ▼                    ▼
                    ┌─────────────┐     ┌─────────────┐
                    │  Previous   │     │   Tools     │
                    │ Interaction │     │ (CRM, Orders│
                    │     ID      │     │  Tickets)   │
                    └─────────────┘     └─────────────┘
```

## Key Features

### Stateful Conversations

Uses `with_previous_interaction()` to chain conversation turns. The API does NOT
inherit `system_instruction` via `previousInteractionId`, so set it on each turn
if needed. For `create_with_auto_functions()`, the SDK reuses the request
internally, so system_instruction is present on all internal iterations:

```rust
// System instruction set on each turn for clarity
let result = client.interaction()
    .with_model(genai_rs::DEFAULT_MODEL)
    .with_text(message)
    .with_previous_interaction(&prev_id)  // Chain to previous turn
    .with_system_instruction("You are a helpful assistant")
    .create_with_auto_functions()
    .await?;
```

### Integrated Tools

The agent has access to backend systems via function calling:

| Tool | Purpose |
|------|---------|
| `lookup_customer` | Find customer by ID or email |
| `get_order` | Retrieve order details |
| `list_customer_orders` | Show all customer orders |
| `create_ticket` | Open a support ticket |
| `initiate_refund` | Start refund process |

### Automatic Function Execution

Uses `create_with_auto_functions()` for seamless tool execution:

```rust
let result = builder
    .add_functions(functions)
    .create_with_auto_functions()
    .await?;
```

## Running

```bash
export GEMINI_API_KEY=your_api_key
cargo run --example multi_turn_agent_auto
```

## Sample Conversation

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
👤 Customer (Turn 1): Hi, I'm Alice Johnson and I need help with my order.
  [Tool: lookup_customer(alice@example.com)]

🤖 Agent:
Hello Alice! I found your account. You're a valued premium customer.
I can see you have 2 recent orders. How can I help you today?

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
👤 Customer (Turn 2): What's the status of order ORD-2024-1234?
  [Tool: get_order(ORD-2024-1234)]

🤖 Agent:
Order ORD-2024-1234 has been shipped! It contains:
- Wireless Headphones
- USB-C Cable
Total: $89.99, placed on December 20th.
```

## Session Management

The `SupportSession` struct manages conversation state:

```rust
struct SupportSession {
    client: Client,
    customer_id: Option<String>,
    last_interaction_id: Option<String>,
}
```

## Production Enhancements

### Authentication Layer

```rust
async fn verify_customer(&self, token: &str) -> Result<Customer, AuthError> {
    // Validate customer identity before data access
}
```

### Escalation Detection

```rust
fn should_escalate(message: &str, sentiment: f32) -> bool {
    sentiment < -0.5 || message.contains("speak to manager")
}
```

### Conversation Summarization

For long conversations, summarize context to manage token limits:

```rust
async fn summarize_context(&self, history: &[Message]) -> String {
    // Compress conversation history for context window
}
```

### Rate Limiting

```rust
async fn check_rate_limit(&self, customer_id: &str) -> Result<(), RateLimitError> {
    // Prevent abuse of support system
}
```

## Error Handling

The example demonstrates:

- Graceful fallback on API errors
- Customer-friendly error messages
- Session recovery after failures

## Best Practices

1. **Verify Identity**: Always authenticate before accessing customer data
2. **Log Interactions**: Record conversations for quality assurance
3. **Set Expectations**: Inform customers about wait times and processes
4. **Offer Alternatives**: Provide self-service options when appropriate
5. **Human Handoff**: Know when to escalate to human agents
